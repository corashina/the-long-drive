//! Source szankoScript sleds on spawned world items: use state, slide forces, wheels and sound.
use super::WorldStream;
use super::content_kinds::{
    BindContext, Bound, ContentKind, ContentKinds, KindBindings, Ready, ready_unless_removed,
};
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, ReferenceError, SourceId, Transform};
use crate::simulation::FIXED_DT;
use crate::simulation::input::DrivingControls;
use crate::simulation::sled::{self, SledParams};
use crate::vehicle::{
    ColliderHandle, PhysicsWorld, VehiclePose, WheelMount, WheelSet, WheelTorques,
    collider_profile, to_rapier,
};
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::math::Vector;

/// Source `szankoScript` state on one body.
pub struct Sled {
    params: SledParams,
    wheels: WheelSet,
    /// Motor 1 Nm and brake 0 on each wheel.
    torques: Vec<WheelTorques>,
    steer: Vec<f32>,
    /// `cols`: sensors while the sled is in use.
    colliders: Vec<ColliderHandle>,
    in_use: bool,
    volume: f32,
    /// The last pitch the source set. `None` keeps the source pitch.
    pitch: Option<f32>,
}
impl Sled {
    pub fn new(params: SledParams, wheels: WheelSet, colliders: Vec<ColliderHandle>) -> Self {
        let count = wheels.wheels().len();
        let mut wheels = wheels;
        wheels.enabled = false;
        Self {
            params,
            wheels,
            torques: vec![
                WheelTorques {
                    motor_nm: 1.,
                    brake_nm: 0.,
                };
                count
            ],
            steer: vec![0.; count],
            colliders,
            in_use: false,
            volume: 0.,
            pitch: None,
        }
    }
    fn heap_bytes(&self) -> usize {
        self.wheels.heap_bytes()
            + self.torques.capacity() * std::mem::size_of::<WheelTorques>()
            + self.steer.capacity() * std::mem::size_of::<f32>()
            + self.colliders.capacity() * std::mem::size_of::<ColliderHandle>()
    }
    /// szankoScript.Update at the fixed step. Run before the physics step.
    pub fn step(&mut self, physics: &mut PhysicsWorld, seated: bool, controls: DrivingControls) {
        let body = self.wheels.body();
        if physics.rapier().bodies.get(body).is_none() {
            return;
        }
        let p = self.params;
        let in_use = sled::in_use(seated, physics.authored_body_mass(body), p.slide_above_mass);
        if in_use != self.in_use {
            self.in_use = in_use;
            self.wheels.enabled = in_use;
            if !in_use {
                self.wheels.reset();
            }
            for &collider in &self.colliders {
                physics.edit_collider(collider, |c| c.set_sensor(in_use));
            }
            // Wake on both edges. A sleeping body would keep its wheel height after use ends.
            physics.edit_body(body, |b| b.wake_up(true));
        }
        let state = &physics.rapier().bodies[body];
        if state.is_sleeping() && !in_use {
            return;
        }
        let forward = state.rotation() * Vector::NEG_Z;
        let (velocity, angular) = (state.linvel(), state.angvel());
        let speed = velocity.length();
        let direction = sled::direction(forward, velocity);
        let (move_, turn) = sled::input(
            controls.throttle,
            controls.brake,
            controls.steer,
            direction,
            seated,
        );
        let input = move_ != 0. || turn != 0.;
        if state.is_sleeping() && !input {
            return;
        }
        let velocity =
            sled::velocity_after_lerp(velocity, forward, FIXED_DT * p.velocity_forward_lerp);
        let angular = sled::angular_after_lerp(
            angular,
            p.turn_force,
            turn,
            FIXED_DT * p.angular_velocity_lerp,
        );
        let force =
            sled::forward_force(forward, move_, p.move_force, p.max_speed, speed, direction);
        physics.edit_body(body, |b| {
            b.set_linvel(velocity, input);
            b.set_angvel(angular, input);
            if force != Vector::ZERO {
                b.apply_impulse(force * FIXED_DT, true);
            }
        });
        physics.step_wheel_set(&mut self.wheels, &self.torques, &self.steer);
        let grounded = self.wheels.wheels().iter().any(|w| w.contact.is_some());
        let (volume, pitch) = sled::sound(speed, grounded, &p);
        self.volume = volume;
        if pitch.is_some() {
            self.pitch = pitch;
        }
    }
}

/// The sled of one spawned world item.
pub struct SledBinding {
    parent: GeneratedId,
    item: ItemId,
    owner: ObjectKey,
    /// Main seatscript sources of the sled seats.
    seats: Vec<SourceId>,
    sled: Sled,
}

impl WorldStream {
    fn bind_sled(
        &self,
        items: &ItemStore,
        physics: &PhysicsWorld,
        slot: usize,
        item: ItemId,
    ) -> Result<SledBinding, ReferenceError> {
        let body = items
            .get(item)
            .and_then(|i| i.body)
            .ok_or_else(|| ReferenceError("Sled item has no body".into()))?;
        let placement = &self.contents.active()[slot].content.placement;
        let prototype = self.reference.prototype(&placement.prototype)?;
        let reference = prototype
            .sled
            .as_ref()
            .ok_or_else(|| ReferenceError("Sled prototype has no sled".into()))?;
        let instance = self
            .structures
            .iter()
            .find(|s| s.placement.id == placement.id)
            .ok_or_else(|| ReferenceError("Sled item has no published structure".into()))?;
        let body_node = prototype.bodies[reference.body as usize].node as usize;
        let scale = placement.scale;
        let place = |t: Transform| {
            to_rapier(VehiclePose {
                position: std::array::from_fn(|i| t.translation[i] * scale[i]),
                rotation: t.rotation,
            })
        };
        let body_pose = place(crate::world::placement::node_pose(prototype, body_node));
        let mounts: Vec<WheelMount> = reference
            .wheels
            .iter()
            .map(|w| {
                let point = crate::world::placement::node_point(
                    prototype,
                    w.node as usize,
                    w.collider.center,
                );
                let point = Vector::from_array(std::array::from_fn(|i| point[i] * scale[i]));
                WheelMount {
                    connection: body_pose.inverse() * point,
                    collider: &w.collider,
                    profile: collider_profile(&w.collider),
                }
            })
            .collect();
        let colliders = instance
            .colliders
            .iter()
            .filter(|(source, handle)| {
                reference.colliders.contains(source)
                    && physics.rapier().colliders[*handle].parent() == Some(body)
            })
            .map(|(_, handle)| *handle)
            .collect();
        let r = reference;
        let params = SledParams {
            slide_above_mass: r.slide_above_mass,
            move_force: r.move_force,
            max_speed: r.max_speed,
            velocity_forward_lerp: r.velocity_forward_lerp,
            turn_force: r.turn_force,
            angular_velocity_lerp: r.angular_velocity_lerp,
            sound_min_speed: r.sound_min_speed,
            sound_max_speed: r.sound_max_speed,
            sound_max_volume: r.sound_max_volume,
            sound_pitch_min_speed: r.sound_pitch_min_speed,
            sound_pitch_max_speed: r.sound_pitch_max_speed,
            sound_min_pitch: r.sound_min_pitch,
            sound_max_pitch: r.sound_max_pitch,
        };
        Ok(SledBinding {
            parent: placement.id.clone(),
            item,
            owner: items.object_key(item),
            seats: r.seats.clone(),
            sled: Sled::new(params, WheelSet::new(body, &mounts), colliders),
        })
    }
    /// Steps every sled. `seat` is the item seat owner and main seatscript of the player.
    pub fn step_sleds(
        &mut self,
        physics: &mut PhysicsWorld,
        seat: Option<(&ObjectKey, &SourceId)>,
        controls: DrivingControls,
    ) {
        for b in &mut self.kinds.sleds.list {
            let seated =
                seat.is_some_and(|(owner, source)| *owner == b.owner && b.seats.contains(source));
            b.sled.step(physics, seated, controls);
        }
    }
    /// Calls `play` with the item, volume, pitch and body position of each sled in use.
    pub fn sled_sounds(
        &self,
        physics: &PhysicsWorld,
        mut play: impl FnMut(ItemId, f32, Option<f32>, [f32; 3]),
    ) {
        for b in &self.kinds.sleds.list {
            if !b.sled.in_use {
                continue;
            }
            if let Some(body) = physics.rapier().bodies.get(b.sled.wheels.body()) {
                play(
                    b.item,
                    b.sled.volume,
                    b.sled.pitch,
                    body.translation().to_array(),
                );
            }
        }
    }
}

impl Bound for SledBinding {
    fn parent(&self) -> &GeneratedId {
        &self.parent
    }
    fn item(&self) -> Option<ItemId> {
        Some(self.item)
    }
    fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + match &self.owner {
                ObjectKey::Generated(id) => crate::world::contents::identity_heap(id),
                ObjectKey::Imported(_) => 0,
            }
            + self.seats.capacity() * std::mem::size_of::<SourceId>()
            + self.seats.iter().map(|s| s.file.capacity()).sum::<usize>()
            + self.sled.heap_bytes()
    }
}

pub(super) struct SledKind;
impl ContentKind for SledKind {
    type Binding = SledBinding;
    type Script = ();
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<SledBinding> {
        &mut kinds.sleds
    }
    fn script(
        stream: &mut WorldStream,
        _: &BindContext,
        slot: usize,
    ) -> Result<Option<()>, ReferenceError> {
        let prototype = &stream.contents.active()[slot].content.placement.prototype;
        let prototypes = &stream.reference.prototypes;
        Ok((stream.kinds.sled_prototypes.iter())
            .any(|&p| &prototypes[p].source == prototype)
            .then_some(()))
    }
    fn ready(stream: &WorldStream, items: &ItemStore, slot: usize) -> Ready {
        ready_unless_removed(stream, items, slot)
    }
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        item: Option<ItemId>,
        _: (),
    ) -> Result<Option<SledBinding>, ReferenceError> {
        stream
            .bind_sled(cx.items, cx.physics, slot, item.unwrap())
            .map(Some)
    }
}
