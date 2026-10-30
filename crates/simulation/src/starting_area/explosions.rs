//! Explosions in the world: tank and explosive prop triggers, the blast on colliders and bodies,
//! the removal of exploded items, and the blast particle and sound.
use super::GarageRuntime;
use crate::interaction::explosions::{Blast, power, prop_factor, tank_factor};
use crate::interaction::guns::Strike;
use crate::interaction::items::ItemLocation;
use crate::reference::{ItemId, ReferenceManifest};
use crate::vehicle::to_rapier;
use crate::world::ObjectKey;
use rapier3d::math::Vector;
use rapier3d::prelude::ColliderHandle;

/// An imported item on the body of a portable tank: an exploded tank removes it.
pub fn explosive_imported(reference: &ReferenceManifest, item: ItemId) -> bool {
    let body = |id: ItemId| reference.items.iter().find(|i| i.id == id).map(|i| i.body);
    let Some(own) = body(item) else {
        return false;
    };
    reference
        .tanks
        .iter()
        .any(|t| t.role == crate::reference::TankRole::Portable && body(t.item) == Some(own))
}

impl GarageRuntime {
    /// `explosionscript.Explode` of each queued blast and of the blasts that they set off.
    pub(crate) fn run_explosions(&mut self, reference: &ReferenceManifest, dt: f32) {
        let origin = self.area.origin;
        let explosions = &mut self.area.interaction.explosions;
        explosions.advance(dt, origin, &self.physics);
        while let Some(blast) = self.area.interaction.explosions.next() {
            self.explode(reference, &blast);
        }
        self.area.interaction.explosions.finish();
        self.announce_death(reference);
    }
    /// `weaponscript.Shot` on a tank and an explosive prop.
    pub(crate) fn shot_explosive(&mut self, s: Strike) {
        if let Some(tank) = self.tank_of(s.item)
            && let Some(e) = self.area.interaction.tank_explosion(tank)
            && ((s.explode && !e.no_shoot_explode) || e.always_explode)
        {
            self.try_explode_tank(s.item, s.force, false);
        }
        if let Some([point, normal]) = s.hole
            && let Some(tank) = self.tank_of(s.item)
            && self
                .area
                .interaction
                .tank_explosion(tank)
                .is_some_and(|e| !e.no_shoot_hole)
            && let Some(state) = self.area.items.get(tank)
        {
            let pose = to_rapier(state.pose);
            let holes = &mut self.area.interaction.holes;
            holes.add(tank, pose, point, normal);
        }
        self.try_explode_prop(s.item, s.force);
    }
    /// `tankscript.TryExplode(force, add)` of the tank of a hit item.
    pub(super) fn try_explode_tank(&mut self, item: ItemId, force: f32, add: bool) {
        let Some(tank) = self.tank_of(item) else {
            return;
        };
        let interaction = &self.area.interaction;
        let e = *interaction.tank_explosion(tank).expect("tank of the item");
        let Some(fluid) = self.area.items.get(tank).and_then(|s| s.fluid.as_ref()) else {
            return;
        };
        let explosions = &interaction.explosions;
        let Some(factor) = tank_factor(&e, fluid, explosions.explosive_fluids(), force) else {
            return;
        };
        let root = self.content_root(tank);
        let Some(at) = self
            .explodable(root)
            .then(|| self.item_point(tank))
            .flatten()
        else {
            return;
        };
        let dist_add = if add { force } else { 0. };
        let blast = explosions.blast(e.explosion_type, factor, dist_add, at, None);
        self.trigger(root, blast);
    }
    /// `explosivescript.TryExplode(force)` of a hit item.
    pub(super) fn try_explode_prop(&mut self, item: ItemId, force: f32) {
        let root = self.content_root(item);
        let Some(e) = self.stream.explosive_item(root) else {
            return;
        };
        let rounds = e
            .ammo
            .as_ref()
            .map(|_| self.stream.ammo_item(root).map_or(0., |a| a.ammo.count));
        let Some(factor) = prop_factor(e, rounds, force) else {
            return;
        };
        let (kind, tint) = (e.explosion_type, e.color);
        let Some(at) = self
            .explodable(root)
            .then(|| self.item_point(root))
            .flatten()
        else {
            return;
        };
        let blast = self
            .area
            .interaction
            .explosions
            .blast(kind, factor, 0., at, Some(tint));
        self.trigger(root, blast);
    }
    /// Queues the blast. `Destroy` and `removeFromMemory`: the item leaves the player and the
    /// world, and the removal is saved.
    fn trigger(&mut self, root: ItemId, blast: Blast) {
        let area = &mut self.area;
        if !area.interaction.explosions.trigger(root, blast) {
            return;
        }
        if area.interaction.held() == Some(root) {
            area.interaction
                .drop_held(&mut area.items, &mut self.physics);
        }
        if area.interaction.equipment.item() == Some(root) {
            area.interaction
                .equipment
                .detach(&mut area.items, &mut self.physics);
        }
        area.inventory.forget(root);
        if matches!(area.items.object_key(root), ObjectKey::Imported(_)) {
            self.remove_imported_item(root);
            return;
        }
        self.stream
            .remove_exploded_item(
                root,
                &mut area.items,
                &mut area.interaction.storage,
                &mut self.physics,
            )
            .expect("bound storage");
    }
    /// One blast: particle, sound, colliders in the radius, then the bodies.
    fn explode(&mut self, reference: &ReferenceManifest, blast: &Blast) {
        let origin = self.area.origin;
        let at = std::array::from_fn(|i| origin[i] + f64::from(blast.position[i]));
        let (clip, gain) = self.area.interaction.explosions.effect(blast, at);
        if let Some(audio) = &mut self.audio {
            audio.survival_event(crate::audio::SurvivalAudioEvent {
                session: audio.session,
                emitter: 0,
                kind: crate::audio::AudioKind::ItemHit,
                action: crate::audio::AudioAction::Start,
                clip,
                gain,
                pitch: 1.,
                position: at,
            });
        }
        let player = self.area.interaction.player_body();
        let mut heard: Option<f32> = None;
        self.area.interaction.explosions.query(&self.physics, blast);
        for i in 0..self.area.interaction.explosions.hits().len() {
            let (collider, power) = self.area.interaction.explosions.hits()[i];
            let breakables = &mut self.stream.breakables;
            if let Some(target) = breakables.target_of(&self.physics, collider) {
                breakables.try_break_explosion(&reference.breakables, target, power);
            }
            let parent = self.physics.rapier().colliders[collider].parent();
            if parent.is_some() && parent == player {
                // fpscontrollerholder is on one collider: the strongest player collider counts.
                heard = Some(heard.map_or(power, |p| p.max(power)));
                continue;
            }
            if let Some(item) = self.collider_item(collider) {
                self.try_explode_tank(item, power, true);
                self.try_explode_prop(item, power);
            }
        }
        if let Some(power) = heard {
            self.hear_blast(reference, power, blast.position);
        }
        // Source: each body with `carscript` gets `DamageStuff(power * 5)` at its body position.
        let mut vehicles = std::mem::take(&mut self.vehicles);
        for i in 0..self.area.interaction.explosions.bodies().len() {
            let body = self.area.interaction.explosions.bodies()[i];
            let Some(key) = self.physics.vehicle_of(body).cloned() else {
                continue;
            };
            let Some(vehicle) = vehicles.get_mut(&key) else {
                continue;
            };
            let d = self.physics.rapier().bodies[body]
                .translation()
                .distance(blast.position);
            let v = power(d, blast.radius, blast.force) * 5.;
            self.crash_car(reference, &key, vehicle, v);
        }
        self.vehicles = vehicles;
        let explosions = &self.area.interaction.explosions;
        explosions.push(&mut self.physics, blast, player);
    }
    /// The `fpscontrollerholder` branch: hearing loss, ear ringing and `Death`.
    fn hear_blast(&mut self, reference: &ReferenceManifest, power: f32, at: Vector) {
        let camera = to_rapier(self.area.player.view(&self.physics).camera_pose);
        let to = at - camera.translation;
        let forward = camera.rotation * -Vector::Z;
        // `Vector3.SignedAngle(forward, to, up)`: positive to the right.
        let right = camera.rotation * Vector::X;
        let angle = forward.angle_between(to).to_degrees();
        let angle = if to.dot(right) < 0. { -angle } else { angle };
        let draw = self.draw_survival();
        if self.hearing.blast(power, angle, draw) {
            self.survival
                .damage_instant(&reference.survival, reference.survival.maximum.hp);
        }
    }
    /// The item of a collider: its user data, else its body.
    fn collider_item(&self, collider: ColliderHandle) -> Option<ItemId> {
        let c = &self.physics.rapier().colliders[collider];
        ((c.user_data & 0xffff_ffff) as u32)
            .checked_sub(1)
            .map(ItemId)
            .or_else(|| {
                let body = c.parent()?;
                self.area
                    .items
                    .iter()
                    .find(|s| s.body == Some(body))
                    .map(|s| s.id)
            })
    }
    /// The content item that owns an attached item.
    fn content_root(&self, item: ItemId) -> ItemId {
        match self.area.items.get(item).map(|s| s.location) {
            Some(ItemLocation::Attached(owner)) => owner,
            _ => item,
        }
    }
    /// The generated tank of an item's content: the root or an item attached to it.
    fn tank_of(&self, item: ItemId) -> Option<ItemId> {
        let root = self.content_root(item);
        let interaction = &self.area.interaction;
        if interaction.tank_explosion(root).is_some() {
            return Some(root);
        }
        self.area
            .items
            .iter()
            .find(|s| {
                s.location == ItemLocation::Attached(root)
                    && interaction.tank_explosion(s.id).is_some()
            })
            .map(|s| s.id)
    }
    /// A generated content item or a moving imported item in the world or in the hands can
    /// explode. Fixed imported tanks only leak.
    fn explodable(&self, root: ItemId) -> bool {
        let Some(state) = self.area.items.get(root).filter(|s| {
            !s.removed
                && matches!(
                    s.location,
                    ItemLocation::World | ItemLocation::Held | ItemLocation::Equipped
                )
        }) else {
            return false;
        };
        let Some(body) = state.body else {
            return false;
        };
        match self.area.items.object_key(root) {
            ObjectKey::Generated(_) => true,
            ObjectKey::Imported(_) => {
                !self.area.spawned.contains(root)
                    && self
                        .physics
                        .rapier()
                        .bodies
                        .get(body)
                        .is_some_and(|b| b.is_dynamic())
            }
        }
    }
    /// The item root point in physics space.
    fn item_point(&self, item: ItemId) -> Option<Vector> {
        let s = self.area.items.get(item)?;
        let body = self.physics.rapier().bodies.get(s.body?)?;
        Some((*body.position() * to_rapier(s.local_pose)).translation)
    }
}
