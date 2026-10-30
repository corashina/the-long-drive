//! Weather on the fixed step: wind roots and the live interior volumes.
use super::GarageRuntime;
use crate::audio::interiors::WorldInteriors;
use crate::interaction::items::ItemStore;
use crate::reference::{ColliderId, ItemId, ReferenceManifest};
use crate::vehicle::{PhysicsWorld, VehiclePose};
use crate::weather::{ImportantObject, InteriorVolume};
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray, RigidBodyHandle};
use std::collections::HashMap;

/// Retained wind bindings. Item and interior changes rebuild them, not each step.
pub(super) struct WeatherBindings {
    player: RigidBodyHandle,
    /// The first live item of each physical body.
    items: HashMap<RigidBodyHandle, ItemId>,
    item_revision: Option<u64>,
    garage: Vec<ColliderHandle>,
    world: WorldInteriors,
    volumes: Vec<InteriorVolume>,
}

impl WeatherBindings {
    /// `interiors` are the garage interior colliders of the live interior list.
    pub(super) fn new(
        physics: &PhysicsWorld,
        player: RigidBodyHandle,
        interiors: &[ColliderId],
    ) -> Self {
        let garage: Vec<_> = physics
            .rapier()
            .colliders
            .iter()
            .filter(|(_, c)| interiors.contains(&ColliderId((c.user_data >> 64) as u32)))
            .map(|(handle, _)| handle)
            .collect();
        Self {
            player,
            items: HashMap::new(),
            item_revision: None,
            world: WorldInteriors::new(garage.len()),
            volumes: Vec::with_capacity(garage.len() + 256),
            garage,
        }
    }

    fn sync(
        &mut self,
        items: &ItemStore,
        physics: &PhysicsWorld,
        stream: &crate::world::WorldStream,
    ) {
        let mut changed = self.world.sync(
            stream.structure_revision(),
            stream.frame().structures,
            &stream.world_reference().prototypes,
        );
        if self.item_revision != Some(items.membership_revision()) {
            self.item_revision = Some(items.membership_revision());
            self.items.clear();
            for item in items.iter().filter(|item| !item.removed) {
                if let Some(body) = item.body {
                    self.items.entry(body).or_insert(item.id);
                }
            }
            changed = true;
        }
        if !changed {
            return;
        }
        self.volumes.clear();
        for collider in self.garage.iter().copied().chain(self.world.colliders()) {
            let root = physics
                .rapier()
                .colliders
                .get(collider)
                .and_then(|c| c.parent())
                .filter(|body| {
                    physics
                        .rapier()
                        .bodies
                        .get(*body)
                        .is_some_and(|b| !b.is_fixed())
                })
                .map(|body| root(&self.items, items, body));
            self.volumes.push(InteriorVolume { collider, root });
        }
    }

    /// The root of a body that wind moves. The player body has none.
    fn wind_root(&self, items: &ItemStore, body: RigidBodyHandle) -> Option<RigidBodyHandle> {
        (body != self.player).then(|| root(&self.items, items, body))
    }
}

/// The body of the item at the top of the parent chain: attachment parent, lock owner or slot owner.
/// Installed parts and wheels already share the body of their vehicle.
fn root(
    bodies: &HashMap<RigidBodyHandle, ItemId>,
    items: &ItemStore,
    body: RigidBodyHandle,
) -> RigidBodyHandle {
    let Some(&(mut id)) = bodies.get(&body) else {
        return body;
    };
    for _ in 0..items.len() {
        match items.parent(id) {
            Some(parent) => id = parent,
            None => break,
        }
    }
    items.get(id).and_then(|item| item.body).unwrap_or(body)
}

impl GarageRuntime {
    /// Runs once before physics, with normal simulation time. Sleep does not speed storms up.
    pub(super) fn advance_weather(&mut self, reference: &ReferenceManifest, dt: f32) {
        let bindings = &mut self.weather_bindings;
        bindings.sync(&self.area.items, &self.physics, &self.stream);
        self.weather
            .reserve_bodies(self.physics.rapier().bodies.len());
        let player = self
            .stream
            .origin
            .absolute(self.area.player.body_pose(&self.physics).position);
        let stream = &mut self.stream;
        self.weather.advance(
            &reference.weather,
            self.simulation_seconds,
            dt,
            player,
            |point| stream.terrain_sample(point),
        );
        let (bindings, items) = (&self.weather_bindings, &self.area.items);
        // WaitForSeconds resumes after Update: apply cached targets before later rescans.
        self.weather.apply_wind(
            &mut self.physics,
            &reference.weather,
            &bindings.volumes,
            dt,
            |body| bindings.wind_root(items, body),
        );
        let physics = &self.physics;
        self.weather.scan_wind(
            physics,
            &reference.weather,
            self.simulation_seconds,
            |body| {
                physics
                    .rapier()
                    .bodies
                    .get(body)
                    .is_some_and(|b| b.is_dynamic() && b.is_enabled())
                    .then(|| bindings.wind_root(items, body))
                    .flatten()
            },
        );
        let volumes = &bindings.volumes;
        self.weather.advance_particles(
            &reference.weather,
            dt,
            player,
            self.lighting.cycle.output().night(),
            physics,
            |collider| volumes.iter().any(|v| v.collider == collider),
        );
        self.lighting
            .cycle
            .compose_weather(&reference.weather, self.weather.influence());
        self.advance_tumbleweeds(reference, dt);
        let Some(audio) = &mut self.audio else {
            return;
        };
        use crate::audio::AudioKind;
        audio.remove_storm_loops(self.weather.storms());
        // sandstormscript.Update: source.volume = defVolume * llerp.
        let volume = audio
            .reference
            .survival
            .iter()
            .find(|b| b.kind == AudioKind::Storm as u32)
            .expect("validated storm audio")
            .gain[0];
        for storm in self.weather.storms() {
            audio.source_loop(
                AudioKind::Storm,
                storm.id.0,
                volume * storm.life_strength,
                storm.absolute_position,
            );
        }
    }

    /// Source thumbleweedMain and thumbleweedScript. The player body is the important object.
    fn advance_tumbleweeds(&mut self, reference: &ReferenceManifest, dt: f32) {
        let r = &reference.weather.tumbleweeds;
        let now = self.simulation_seconds;
        let (physics, items, spawned) = (&self.physics, &self.area.items, &self.area.spawned);
        // Physics.Raycast does not hit colliders that hold the origin.
        let cast = |origin: Vector, direction: Vector, length: f32, mask: u32| {
            let filter = |_: ColliderHandle, c: &Collider| {
                c.is_enabled()
                    && c.collision_groups().memberships.bits() & mask != 0
                    && !c.shape().contains_point(c.position(), origin)
            };
            physics
                .queries(QueryFilter::default().predicate(&filter))
                .cast_ray(&Ray::new(origin, direction), length, true)
                .map(|(_, distance)| distance)
        };
        let body = |item: ItemId| items.get(item).filter(|i| !i.removed).and_then(|i| i.body);
        self.weather.tumbleweeds.gust(
            r,
            now,
            dt,
            |item| body(item).map(|b| physics.rapier().bodies[b].translation()),
            cast,
        );
        let pose = self.area.player.body_pose(physics);
        let player = ImportantObject {
            position: Vector::from_array(pose.position),
            forward: Rotation::from_array(pose.rotation) * Vector::NEG_Z,
        };
        let spot =
            self.weather
                .tumbleweeds
                .spawn(r, now, &[player], |item| spawned.contains(item), cast);
        // Unity AddForce uses Force mode: integrate one fixed step.
        let integration_dt = physics.rapier().integration_parameters.dt;
        for (item, force) in self.weather.tumbleweeds.forces() {
            if let Some(handle) = body(item) {
                self.physics
                    .edit_body(handle, |b| b.apply_impulse(force * integration_dt, true));
            }
        }
        let Some(spot) = spot else {
            return;
        };
        let prototype = reference
            .item_prototypes
            .iter()
            .position(|p| p.item_id == Some(r.item_id))
            .expect("validated tumbleweed prototype");
        let ids = super::spawned::spawn_prototype(
            reference,
            &mut self.area,
            &mut self.physics,
            prototype,
            VehiclePose {
                position: spot.to_array(),
                rotation: [0., 0., 0., 1.],
            },
        )
        .expect("validated tumbleweed prototype");
        self.weather.tumbleweeds.add(ids[0]);
    }
}
