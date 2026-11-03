mod body;
mod mass;
mod mutations;
mod presentation_bodies;
mod query_bounds;
mod wheel;
mod wheel_set;

use body::VehicleBody;
#[cfg(test)]
pub use body::tests as body_fixture;
/// A 820 kg box car prototype with four wheels, for native tests.
#[cfg(test)]
pub use body::tests::car as car_prototype_fixture;
use presentation_bodies::{BodyPresentationState, PresentationBodies};
use query_bounds::QueryBounds;
use rapier3d::math::{Pose, Rotation, Vector};
pub use rapier3d::prelude::ColliderHandle;
use rapier3d::prelude::{
    Array2, ColliderBuilder, ColliderSet, CollisionEvent, ContactPair, EventHandler, Group,
    InteractionGroups, InteractionTestMode, QueryFilter, QueryPipeline, RigidBodyHandle,
    RigidBodySet,
};
pub use wheel::{
    WheelAssembly, WheelContact, WheelProfile, WheelState, WheelTorques, collider_profile,
};
pub use wheel_set::{WheelMount, WheelSet};

use crate::reference::{ColliderGeometry, ReferenceError};
use crate::simulation::{DriveInput, FIXED_DT};
use crate::world::ObjectKey;
use std::collections::HashMap;

/// Axle torque after gear ratios and engine engagement. Motor torque has a sign.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriveTorques {
    pub motor_nm: f32,
    pub engine_brake_nm: f32,
    /// Source `bikepedalscript` motor torque of each pedal wheel. None: the pedals are off.
    pub pedal_nm: Option<f32>,
}

pub const VEHICLE_COLLIDER_FLAG: u128 = 1 << 127;

/// Rapier 0.36 CCD sweeps disabled colliders too. A disabled collider below the
/// ground then clamps its body's motion each step. Empty solver groups stop this.
pub fn solver_groups(enabled: bool) -> InteractionGroups {
    if enabled {
        InteractionGroups::all()
    } else {
        InteractionGroups::none()
    }
}

/// Enables or disables a collider. See [`solver_groups`].
pub fn set_collider_enabled(collider: &mut rapier3d::prelude::Collider, enabled: bool) {
    collider.set_enabled(enabled);
    collider.set_solver_groups(solver_groups(enabled));
}
/// Collider pose changes below this are float noise of world round trips.
const POSE_TOLERANCE: f32 = 1e-4;
/// A pose write is due when `next` differs from `current` by more than float noise.
/// Each collider pose write recomputes the body mass and wakes the body.
pub fn pose_changed(current: Option<&rapier3d::math::Pose>, next: &rapier3d::math::Pose) -> bool {
    current.is_none_or(|current| {
        !current
            .translation
            .abs_diff_eq(next.translation, POSE_TOLERANCE)
            || !current.rotation.abs_diff_eq(next.rotation, POSE_TOLERANCE)
    })
}
pub const SLOT_SENSOR_FLAG: u128 = 1 << 126;
/// Collider user data bits 32 to 34 hold the `StepSurface` code. 0 is none.
pub const STEP_SURFACE_SHIFT: u32 = 32;
pub const STEP_SURFACE_MASK: u128 = 0b111 << STEP_SURFACE_SHIFT;
pub fn step_surface_bits(surface: Option<crate::reference::StepSurface>) -> u128 {
    surface.map_or(0, |s| (s as u128) << STEP_SURFACE_SHIFT)
}
/// Collider user data bits 35 to 38 hold the source tag code. 0 is none.
pub const TAG_SHIFT: u32 = 35;
const TAG_MASK: u128 = 0b1111 << TAG_SHIFT;
pub fn tag_bits(tag: Option<crate::reference::ColliderTag>) -> u128 {
    tag.map_or(0, |t| (t as u128 + 1) << TAG_SHIFT)
}
pub fn collider_tag(user_data: u128) -> Option<crate::reference::ColliderTag> {
    let code = ((user_data & TAG_MASK) >> TAG_SHIFT) as usize;
    code.checked_sub(1)
        .and_then(|i| crate::reference::COLLIDER_TAGS.get(i).copied())
}

/// One started contact. `speed` is the relative speed at `point` before the solver.
#[derive(Clone, Copy, Debug)]
pub struct HitContact {
    pub colliders: [ColliderHandle; 2],
    pub point: [f32; 3],
    pub speed: f32,
}
const HIT_CAPACITY: usize = 64;

/// Records started contacts and started sensor intersections (`OnTriggerEnter`: the sensor
/// and the other collider in event order) of colliders with `ActiveEvents::COLLISION_EVENTS`.
/// A full buffer drops new events.
pub struct HitCollector {
    hits: std::sync::Mutex<Vec<HitContact>>,
    enters: std::sync::Mutex<Vec<[ColliderHandle; 2]>>,
}
impl Default for HitCollector {
    fn default() -> Self {
        Self {
            hits: std::sync::Mutex::new(Vec::with_capacity(HIT_CAPACITY)),
            enters: std::sync::Mutex::new(Vec::with_capacity(HIT_CAPACITY)),
        }
    }
}
impl HitCollector {
    /// Moves the recorded hits into `out` without allocation.
    pub fn take(&self, out: &mut Vec<HitContact>) {
        let mut hits = self.hits.lock().unwrap();
        out.clear();
        std::mem::swap(out, &mut *hits);
    }
    /// Moves the recorded sensor intersections into `out` without allocation.
    fn take_enters(&self, out: &mut Vec<[ColliderHandle; 2]>) {
        let mut enters = self.enters.lock().unwrap();
        out.clear();
        std::mem::swap(out, &mut *enters);
    }
}
impl EventHandler for HitCollector {
    fn handle_collision_event(
        &self,
        bodies: &RigidBodySet,
        colliders: &ColliderSet,
        event: CollisionEvent,
        pair: Option<&ContactPair>,
    ) {
        if let CollisionEvent::Started(a, b, flags) = event
            && flags.contains(rapier3d::prelude::CollisionEventFlags::SENSOR)
        {
            let mut enters = self.enters.lock().unwrap();
            if enters.len() < HIT_CAPACITY {
                enters.push([a, b]);
            }
            return;
        }
        let (CollisionEvent::Started(a, b, _), Some(pair)) = (event, pair) else {
            return;
        };
        let (Some(first), Some(second)) = (colliders.get(a), colliders.get(b)) else {
            return;
        };
        let Some(point) = pair.manifolds().iter().find_map(|m| {
            let contact = m.points.first()?;
            let pose = colliders.get(pair.collider1)?.position();
            let pose = match m.subshape_poses.as_ref().and_then(|s| s.pos1) {
                Some(sub) => *pose * sub,
                None => *pose,
            };
            Some(pose * contact.local_p1)
        }) else {
            return;
        };
        let velocity = |collider: &rapier3d::prelude::Collider| {
            collider
                .parent()
                .and_then(|h| bodies.get(h))
                .map_or(Vector::ZERO, |body| body.velocity_at_point(point))
        };
        let mut hits = self.hits.lock().unwrap();
        if hits.len() < HIT_CAPACITY {
            hits.push(HitContact {
                colliders: [a, b],
                point: point.to_array(),
                speed: (velocity(first) - velocity(second)).length(),
            });
        }
    }
    fn handle_contact_force_event(
        &self,
        _: f32,
        _: &RigidBodySet,
        _: &ColliderSet,
        _: &ContactPair,
        _: f32,
    ) {
    }
    fn handle_soft_body_tear_event(
        &self,
        _: &rapier3d::prelude::SoftBodySet,
        _: &rapier3d::prelude::SoftBodyTearEvent,
    ) {
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VehiclePose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct VehicleSnapshot {
    pub pose: VehiclePose,
    pub speed_mps: f32,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

pub struct PhysicsWorld {
    body_mass_override: Option<mass::BodyMassOverride>,
    /// Locked item mass on non-vehicle bodies. `set_body_mass` adds it.
    body_cargo: Vec<(RigidBodyHandle, f32)>,
    /// Local centre of mass overrides. Mass changes keep them.
    center_of_mass: Vec<(RigidBodyHandle, Vector)>,
    pub inactive_bodies: std::collections::HashSet<RigidBodyHandle>,
    physics: rapier3d::prelude::PhysicsWorld,
    query_bounds: QueryBounds,
    presentation_bodies: PresentationBodies,
    /// Chassis bodies of all vehicles, for the wheel ray rule.
    chassis: Vec<RigidBodyHandle>,
    vehicles: HashMap<ObjectKey, VehicleBody>,
    hit_collector: HitCollector,
    hits: Vec<HitContact>,
    trigger_enters: Vec<[ColliderHandle; 2]>,
    ignored: mutations::IgnoredCollisions,
    /// Farthest-point speed below which a body sleeps.
    sleep_speed: f32,
}

impl PhysicsWorld {
    /// A world without vehicles.
    pub fn empty(vehicles: usize, bodies: usize) -> Self {
        let mut physics = Self {
            physics: rapier3d::prelude::PhysicsWorld::new(),
            body_mass_override: None,
            body_cargo: Vec::with_capacity(mass::CARGO_BODIES),
            center_of_mass: Vec::with_capacity(mass::CARGO_BODIES),
            query_bounds: QueryBounds::default(),
            presentation_bodies: PresentationBodies::default(),
            chassis: Vec::with_capacity(vehicles),
            vehicles: HashMap::with_capacity(vehicles),
            hit_collector: HitCollector::default(),
            hits: Vec::with_capacity(HIT_CAPACITY),
            trigger_enters: Vec::with_capacity(HIT_CAPACITY),
            ignored: Default::default(),
            inactive_bodies: std::collections::HashSet::with_capacity(bodies),
            sleep_speed:
                rapier3d::prelude::RigidBodyActivation::default_normalized_linear_threshold(),
        };
        physics.physics.integration_parameters.dt = FIXED_DT;
        // A recycled contact keeps its fixed-collider points in world space. The origin rebase
        // moves fixed colliders, so a recycled contact would push against the old surface.
        physics.physics.integration_parameters.contact_recycling = false;
        physics
    }
}

impl PhysicsWorld {
    /// A world with no vehicles and no gravity.
    #[cfg(test)]
    pub fn empty_for_tests() -> Self {
        let mut physics = Self::empty(0, 0);
        physics.physics.gravity = Vector::ZERO;
        physics
    }
    /// Vehicle keys in no fixed order.
    pub fn vehicle_keys(&self) -> impl Iterator<Item = &ObjectKey> {
        self.vehicles.keys()
    }

    /// Maximum steer angle of the vehicle on chassis `body`, in radians.
    pub fn max_steer(&self, body: RigidBodyHandle) -> Option<f32> {
        self.vehicles
            .values()
            .find(|vehicle| vehicle.chassis == body)
            .map(|vehicle| vehicle.max_steer)
    }
    /// Returns the vehicle whose chassis is `body`.
    pub fn vehicle_of(&self, body: RigidBodyHandle) -> Option<&ObjectKey> {
        if !self.chassis.contains(&body) {
            return None;
        }
        self.vehicles
            .iter()
            .find(|(_, vehicle)| vehicle.chassis == body)
            .map(|(key, _)| key)
    }

    fn vehicle(&self, key: &ObjectKey) -> &VehicleBody {
        self.vehicles.get(key).expect("Unknown vehicle")
    }

    pub fn apply_vehicle_forces(
        &mut self,
        key: &ObjectKey,
        input: DriveInput,
        torques: DriveTorques,
    ) {
        let vehicle = self.vehicles.get_mut(key).expect("Unknown vehicle");
        let drive_count = vehicle.driven_wheel_count().max(1) as f32;
        let pedal = |value: f32| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let brake = pedal(input.brake);
        let handbrake = pedal(input.handbrake);
        let motor_nm = if torques.motor_nm.is_finite() {
            torques.motor_nm
        } else {
            0.0
        };
        let pedal_nm = torques.pedal_nm.filter(|t| t.is_finite());
        let engine_brake_nm = if torques.engine_brake_nm.is_finite() {
            torques.engine_brake_nm.max(0.0)
        } else {
            0.0
        };
        let steering = if input.steer.is_finite() {
            input.steer.clamp(-1.0, 1.0) * vehicle.max_steer
        } else {
            0.0
        };
        for (rules, steer) in vehicle.rules.iter().zip(&mut vehicle.steer) {
            *steer = if rules.steer { steering } else { 0.0 };
        }
        // A parked car keeps its steer. Only a steer change wakes it.
        let steer_changed = vehicle
            .wheels
            .wheels()
            .iter()
            .zip(&vehicle.steer)
            .any(|(wheel, steer)| wheel.steering != *steer);
        if pedal(input.throttle) > 0.0
            || brake > 0.0
            || motor_nm != 0.0
            || pedal_nm.is_some_and(|t| t != 0.0)
            || steer_changed
            || handbrake != vehicle.last_handbrake
        {
            self.query_bounds.mark_body(vehicle.chassis);
            let body = &mut self.physics.bodies[vehicle.chassis];
            let before = BodyPresentationState::capture(body);
            body.wake_up(true);
            self.presentation_bodies
                .mark_if_changed(vehicle.chassis, before, body);
        }
        vehicle.last_handbrake = handbrake;
        // Source carscript.SetWheel: driven wheels take motor torque and engine brake.
        // Service brake and handbrake add to the wheels in their source lists.
        // With pedals on, SetWheel keeps the pedal torque unless the engine torque is not 0.
        for (rules, torques) in vehicle.rules.iter().zip(&mut vehicle.torques) {
            *torques = WheelTorques::default();
            if let (true, Some(pedal)) = (rules.pedal, pedal_nm) {
                torques.motor_nm = pedal;
            }
            if rules.drive {
                if motor_nm != 0.0 {
                    torques.motor_nm = motor_nm / drive_count;
                }
                torques.brake_nm = engine_brake_nm / drive_count;
            }
            if rules.brake {
                torques.brake_nm += vehicle.brake_torque * brake;
            }
            if rules.handbrake {
                torques.brake_nm += vehicle.handbrake_torque * handbrake;
            }
        }
        let own = vehicle.chassis;
        let chassis = &self.chassis;
        let ground = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            wheel_ground(own, chassis, collider)
        };
        vehicle.wheels.step(
            &mut self.physics,
            &mut self.query_bounds,
            &mut self.presentation_bodies,
            QueryFilter::default().predicate(&ground),
            &vehicle.torques,
            &vehicle.steer,
        );
    }

    /// Steps a wheel set on any body. `torques` and `steer` hold one value per wheel.
    pub fn step_wheel_set(&mut self, set: &mut WheelSet, torques: &[WheelTorques], steer: &[f32]) {
        let own = set.body();
        let chassis = &self.chassis;
        let ground = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            wheel_ground(own, chassis, collider)
        };
        set.step(
            &mut self.physics,
            &mut self.query_bounds,
            &mut self.presentation_bodies,
            QueryFilter::default().predicate(&ground),
            torques,
            steer,
        );
    }

    /// Started contacts of the last step.
    pub fn hits(&self) -> &[HitContact] {
        &self.hits
    }
    /// Started sensor intersections of the last step, as collider pairs in either order.
    pub fn trigger_enters(&self) -> &[[ColliderHandle; 2]] {
        &self.trigger_enters
    }

    pub fn body_active(&self, handle: RigidBodyHandle) -> bool {
        !self.inactive_bodies.contains(&handle)
    }

    pub fn step_physics(&mut self) {
        for body in self.physics.islands.active_bodies() {
            self.query_bounds.mark_simulated_body(body);
            self.presentation_bodies.mark(body);
        }
        self.presentation_bodies
            .begin_step(&self.physics.bodies, self.physics.islands.active_bodies());
        self.physics
            .step_with_events(&self.ignored, &self.hit_collector);
        self.presentation_bodies.end_step(&self.physics.bodies);
        self.hit_collector.take(&mut self.hits);
        self.hit_collector.take_enters(&mut self.trigger_enters);
        for body in self.physics.islands.active_bodies() {
            self.query_bounds.mark_simulated_body(body);
            self.presentation_bodies.mark(body);
        }
        self.query_bounds.requeue_step_mutations();
        self.query_bounds.refresh(&mut self.physics);
        self.query_bounds.finish_step();
    }

    pub fn capture_save(
        &self,
        vehicle: &ObjectKey,
        identity: crate::save::BodyIdentity,
        origin: &crate::world::WorldOrigin,
    ) -> crate::save::BodySnapshot {
        capture_body_save(
            &self.physics.bodies[self.chassis_handle(vehicle)],
            identity,
            origin,
        )
    }
    /// Source WheelCollider rpm, radius and isGrounded of one wheel slot.
    pub fn wheel_motion(&self, vehicle: &ObjectKey, wheel: usize) -> (f32, f32, bool) {
        let w = &self.vehicle(vehicle).wheels.wheels()[wheel];
        (w.rpm(), w.profile.radius, w.contact.is_some())
    }

    /// Source carscript avgRpm: all driven slots over the driven rim count.
    pub fn driven_wheel_rpm(&self, key: &ObjectKey) -> f32 {
        let vehicle = self.vehicle(key);
        let sum: f32 = vehicle
            .wheels
            .wheels()
            .iter()
            .zip(&vehicle.rules)
            .filter(|(_, rules)| rules.drive)
            .map(|(wheel, _)| wheel.rpm())
            .sum();
        sum / vehicle.driven_wheel_count().max(1) as f32
    }

    /// Source carscript.DrivenWheelsNum2: driven slots with a mounted rim.
    pub fn driven_wheel_count(&self, key: &ObjectKey) -> usize {
        self.vehicle(key).driven_wheel_count()
    }

    /// Sets the vehicle mass without locked cargo. It adds the cargo mass.
    pub fn set_vehicle_mass(&mut self, key: &ObjectKey, mass_kg: f32) {
        let vehicle = self.vehicle(key);
        let (chassis, mass_kg) = (vehicle.chassis, mass_kg + vehicle.cargo_mass);
        self.apply_vehicle_mass(chassis, mass_kg);
    }

    /// Sets the chassis mass with cargo. Collider mass counts. The centre of mass stays.
    fn apply_vehicle_mass(&mut self, chassis: RigidBodyHandle, mass_kg: f32) {
        let geometry_mass: f32 = self.physics.bodies[chassis]
            .colliders()
            .iter()
            .map(|handle| self.physics.colliders[*handle].mass())
            .sum();
        self.edit_body(chassis, |body| {
            body.set_additional_mass((mass_kg - geometry_mass).max(0.0), true)
        });
        self.recompute_body_mass(chassis);
        self.apply_center_of_mass(chassis);
    }

    /// The parts on one wheel. `index` is local to the vehicle.
    pub fn wheel_assembly(&self, key: &ObjectKey, index: usize) -> WheelAssembly {
        self.vehicle(key).assemblies[index]
    }

    /// Changes the parts on one wheel. `index` is local to the vehicle.
    pub fn set_wheel(&mut self, key: &ObjectKey, index: usize, assembly: WheelAssembly) {
        let vehicle = self.vehicles.get_mut(key).expect("Unknown vehicle");
        vehicle.assemblies[index] = assembly;
        vehicle.wheels.set_profile(
            index,
            wheel::resolve_profile(&vehicle.rules[index], &vehicle.slip, assembly),
        );
        let chassis = vehicle.chassis;
        self.edit_body(chassis, |body| body.wake_up(true));
    }

    pub fn rapier(&self) -> &rapier3d::prelude::PhysicsWorld {
        &self.physics
    }

    pub fn chassis_handle(&self, key: &ObjectKey) -> RigidBodyHandle {
        self.vehicle(key).chassis
    }

    pub fn refresh_queries(&mut self) {
        self.query_bounds.refresh(&mut self.physics);
    }

    pub fn query_storage_bytes(&self) -> usize {
        self.query_bounds.retained_bytes()
    }

    pub fn queries<'a>(&'a self, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        QueryPipeline {
            dispatcher: self.physics.narrow_phase.query_dispatcher(),
            bvh: &self.query_bounds.bvh,
            bodies: &self.physics.bodies,
            colliders: &self.physics.colliders,
            filter,
        }
    }

    /// Player movement queries. Shape-cast normals come from contact manifolds.
    pub fn character_queries<'a>(&'a self, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        QueryPipeline {
            dispatcher: &crate::player::normals::CharacterQueryDispatcher,
            ..self.queries(filter)
        }
    }

    pub fn snapshot(&self, key: &ObjectKey) -> VehicleSnapshot {
        let body = &self.physics.bodies[self.vehicle(key).chassis];
        let pose = body.position();
        VehicleSnapshot {
            pose: from_rapier(pose),
            speed_mps: body.linvel().dot(pose.rotation * -Vector::Z),
            linear_velocity: body.linvel().to_array(),
            angular_velocity: body.angvel().to_array(),
        }
    }

    /// World pose of one wheel. `index` is local to the vehicle.
    pub fn wheel_pose(&self, key: &ObjectKey, index: usize) -> VehiclePose {
        let vehicle = self.vehicle(key);
        let pose = self.physics.bodies[vehicle.chassis].position();
        let wheel = &vehicle.wheels.wheels()[index];
        let local = wheel.connection - Vector::Y * wheel.suspension_length;
        VehiclePose {
            position: (pose * local).to_array(),
            rotation: (pose.rotation
                * Rotation::from_rotation_y(wheel.steering)
                * Rotation::from_rotation_x(-wheel.angle))
            .to_array(),
        }
    }

    /// Wheel centers in the chassis body frame, in wheel order. None if `key` has no vehicle.
    pub fn wheel_centers(&self, key: &ObjectKey) -> Option<impl Iterator<Item = Vector> + '_> {
        let vehicle = self.vehicles.get(key)?;
        Some(
            vehicle
                .wheels
                .wheels()
                .iter()
                .map(|w| w.connection - Vector::Y * w.suspension_length),
        )
    }

    /// Ground contact of each wheel, in wheel order.
    pub fn wheel_contacts(&self, key: &ObjectKey) -> impl Iterator<Item = bool> + '_ {
        self.vehicle(key)
            .wheels
            .wheels()
            .iter()
            .map(|wheel| wheel.contact.is_some())
    }

    /// Moves the car and stops it. Wheel angles and spin stay unchanged.
    pub fn reset_preserving_wheels(&mut self, key: &ObjectKey, pose: VehiclePose) {
        self.reset_chassis(key, pose);
        self.vehicles.get_mut(key).unwrap().wheels.clear_contacts();
    }

    pub fn reset(&mut self, key: &ObjectKey, pose: VehiclePose) {
        self.vehicles
            .get_mut(key)
            .expect("Unknown vehicle")
            .wheels
            .reset();
        self.reset_chassis(key, pose);
    }

    fn reset_chassis(&mut self, key: &ObjectKey, pose: VehiclePose) {
        let chassis = self.chassis_handle(key);
        self.edit_body(chassis, |body| {
            body.set_position(to_rapier(pose), true);
            body.set_linvel(Vector::ZERO, true);
            body.set_angvel(Vector::ZERO, true);
            body.reset_forces(true);
            body.reset_torques(true);
        });
        self.refresh_queries();
    }

    /// Fallible source mesh publication leaves existing collision untouched on failure.
    /// `grid` is the tile size when the surface is a regular terrain grid
    /// (`terrain::regular_grid`). Such a surface becomes a heightfield, else a triangle mesh.
    pub fn insert_source_mesh(
        &mut self,
        surface: &crate::world::WorldSurface,
        grid: Option<f32>,
        position: [f32; 3],
        layer: u8,
        filter: u32,
        friction: f32,
        restitution: f32,
        available_cpu: usize,
    ) -> Result<(ColliderHandle, usize), ReferenceError> {
        // Reserve vertices, indices, BVH construction and retained collider storage before allocation.
        let reservation = match grid {
            Some(_) => surface.vertices.len().saturating_mul(5),
            None => surface
                .vertices
                .len()
                .saturating_mul(48)
                .saturating_add(surface.triangles.len().saturating_mul(256)),
        }
        .saturating_add(std::mem::size_of::<rapier3d::prelude::Collider>());
        if reservation > available_cpu {
            return Err(ReferenceError(format!(
                "Source collision reservation exceeds budget: {reservation}"
            )));
        }
        let mesh = source_collider(surface, grid)?
            .friction(friction)
            .restitution(restitution)
            .translation(Vector::from_array(position))
            .collision_groups(InteractionGroups::new(
                Group::from_bits_truncate(1u32 << layer),
                Group::from_bits_truncate(filter),
                InteractionTestMode::And,
            ))
            .build();
        let shape = mesh.shape();
        let bytes = std::mem::size_of::<rapier3d::prelude::Collider>()
            + match shape.as_heightfield() {
                Some(field) => {
                    std::mem::size_of_val(field.heights().data())
                        + std::mem::size_of_val(field.cells_statuses().data())
                }
                None => shape
                    .as_trimesh()
                    .expect("source triangle mesh")
                    .total_memory_size(),
            };
        if bytes > available_cpu {
            return Err(ReferenceError(format!(
                "Source collision allocation exceeds budget: {bytes}"
            )));
        }
        let handle = self.insert_collider(mesh);
        self.refresh_queries();
        Ok((handle, bytes))
    }

    pub fn remove_collider(&mut self, handle: ColliderHandle) {
        for vehicle in self.vehicles.values() {
            if vehicle.wheels.wheels().iter().any(|wheel| {
                wheel
                    .contact
                    .is_some_and(|contact| contact.collider == handle)
            }) {
                self.query_bounds.mark_body(vehicle.chassis);
                let body = &mut self.physics.bodies[vehicle.chassis];
                let before = BodyPresentationState::capture(body);
                body.wake_up(true);
                self.presentation_bodies
                    .mark_if_changed(vehicle.chassis, before, body);
            }
        }
        if self.physics.colliders.contains(handle) {
            self.query_bounds.retire_collider(handle);
        }
        if let Some(parent) = self
            .physics
            .colliders
            .get(handle)
            .and_then(|collider| collider.parent())
        {
            self.query_bounds.mark_body(parent);
        }
        self.physics.remove_collider(handle);
        self.refresh_queries();
    }

    pub fn rebase(&mut self, shift: [f32; 3]) {
        let shift = Vector::from_array(shift);
        self.presentation_bodies.rebase(shift);
        for (handle, body) in self.physics.bodies.iter_mut() {
            if self.inactive_bodies.contains(&handle) {
                continue;
            }
            self.query_bounds.mark_body(handle);
            let before = BodyPresentationState::capture(body);
            body.set_translation(body.translation() - shift, false);
            self.presentation_bodies
                .mark_if_changed(handle, before, body);
        }
        for (handle, collider) in self.physics.colliders.iter_mut() {
            if collider.parent().is_none() {
                self.query_bounds.mark_collider(handle, false);
                collider.set_translation(collider.translation() - shift);
            }
        }
        // Rapier refreshes private wheel ray points before applying forces next step.
        // Keep suspension lengths, wheel rotations, and velocities unchanged here.
        self.refresh_queries();
    }

    pub fn insert_box(&mut self, center: [f32; 3], half_size: [f32; 3]) -> ColliderHandle {
        let handle = self.insert_collider(
            ColliderBuilder::cuboid(half_size[0], half_size[1], half_size[2])
                .translation(Vector::from_array(center))
                .friction(0.8),
        );
        self.refresh_queries();
        handle
    }

    pub fn collider_count(&self) -> usize {
        self.physics.colliders.len()
    }
}

/// Wheel ray rule. The ray never hits its own body, and of vehicle colliders it hits only chassis.
fn wheel_ground(
    own: RigidBodyHandle,
    chassis: &[RigidBodyHandle],
    collider: &rapier3d::prelude::Collider,
) -> bool {
    let parent = collider.parent();
    parent != Some(own)
        && (collider.user_data & VEHICLE_COLLIDER_FLAG == 0
            || parent.is_some_and(|parent| chassis.contains(&parent)))
}

pub fn capture_body_save(
    body: &rapier3d::prelude::RigidBody,
    identity: crate::save::BodyIdentity,
    origin: &crate::world::WorldOrigin,
) -> crate::save::BodySnapshot {
    let pose = from_rapier(body.position());
    crate::save::BodySnapshot {
        identity,
        pose: crate::save::AbsolutePose {
            position: origin.absolute(pose.position),
            rotation: pose.rotation,
        },
        linear_velocity: body.linvel().to_array(),
        angular_velocity: body.angvel().to_array(),
        sleeping: body.is_sleeping(),
    }
}

pub fn collider_builder(geometry: &ColliderGeometry) -> Result<ColliderBuilder, ReferenceError> {
    match geometry {
        ColliderGeometry::Box {
            center,
            half_extents,
            rotation,
        } => Ok(
            ColliderBuilder::cuboid(half_extents[0], half_extents[1], half_extents[2]).position(
                to_rapier(VehiclePose {
                    position: *center,
                    rotation: *rotation,
                }),
            ),
        ),
        ColliderGeometry::Sphere { center, radius } => {
            Ok(ColliderBuilder::ball(*radius).translation(Vector::from_array(*center)))
        }
        ColliderGeometry::Capsule { a, b, radius } => Ok(ColliderBuilder::capsule_from_endpoints(
            Vector::from_array(*a),
            Vector::from_array(*b),
            *radius,
        )),
        ColliderGeometry::Convex { points } => {
            let points: Vec<_> = points.iter().copied().map(Vector::from_array).collect();
            ColliderBuilder::convex_hull(&points)
                .ok_or_else(|| ReferenceError("Invalid convex collision geometry".into()))
        }
        ColliderGeometry::Trimesh { points, triangles } => ColliderBuilder::trimesh(
            points.iter().copied().map(Vector::from_array).collect(),
            triangles.clone(),
        )
        .map_err(|error| ReferenceError(format!("Invalid collision mesh: {error}"))),
    }
}

pub fn to_rapier(pose: VehiclePose) -> Pose {
    Pose::from_parts(
        Vector::from_array(pose.position),
        Rotation::from_array(pose.rotation),
    )
}

pub fn from_rapier(pose: &Pose) -> VehiclePose {
    VehiclePose {
        position: pose.translation.to_array(),
        rotation: pose.rotation.to_array(),
    }
}

/// Source surface shape in the surface's local frame.
fn source_collider(
    surface: &crate::world::WorldSurface,
    grid: Option<f32>,
) -> Result<ColliderBuilder, ReferenceError> {
    let Some(size) = grid else {
        return ColliderBuilder::trimesh(
            surface
                .vertices
                .iter()
                .copied()
                .map(Vector::from_array)
                .collect(),
            surface.triangles.clone(),
        )
        .map_err(|error| ReferenceError(format!("Source collision mesh: {error}")));
    };
    // Vertex rows run from +z to -z. Heightfield rows run from -z, stored column major.
    let n = surface.vertices.len().isqrt();
    let mut heights = Vec::with_capacity(n * n);
    for column in 0..n {
        for row in (0..n).rev() {
            heights.push(surface.vertices[row * n + column][1]);
        }
    }
    // The grid diagonal (-x, +z) to (+x, -z) is the default cell split.
    Ok(ColliderBuilder::heightfield(
        Array2::new(n, n, heights),
        Vector::new(size, 1.0, size),
    ))
}

#[cfg(test)]
mod resting_tests {
    use super::*;
    use rapier3d::prelude::RigidBodyBuilder;

    /// The garage barrel: a 32-sided hull. The oil can stands near its rim.
    #[test]
    fn box_rests_on_many_sided_hull() {
        let mut physics = PhysicsWorld::empty(0, 2);
        physics.sleep_speed = (2f32 * 0.01).sqrt();
        physics.insert_collider(
            ColliderBuilder::cuboid(5., 0.1, 5.).translation(Vector::new(0., -0.1, 0.)),
        );
        let ring = |y: f32| {
            (0..32).map(move |i| {
                let a = i as f32 * std::f32::consts::TAU / 32.;
                Vector::new(0.237 * a.cos(), y, 0.237 * a.sin())
            })
        };
        let hull: Vec<_> = ring(-0.4).chain(ring(0.4)).collect();
        let barrel = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .additional_mass(20.)
                .translation(Vector::new(0., 0.4, 0.)),
        );
        physics.insert_collider_with_parent(
            ColliderBuilder::convex_hull(&hull)
                .unwrap()
                .density(0.)
                .build(),
            barrel,
        );
        let can = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .additional_mass(5.)
                .translation(Vector::new(0.13, 0.96, 0.08)),
        );
        physics.insert_collider_with_parent(
            ColliderBuilder::cuboid(0.05, 0.15, 0.1).density(0.).build(),
            can,
        );
        for _ in 0..(5. / FIXED_DT) as usize {
            physics.step_physics();
        }
        let rapier = physics.rapier();
        assert!(rapier.bodies[barrel].is_sleeping() && rapier.bodies[can].is_sleeping());
        assert!((rapier.bodies[can].translation().y - 0.95).abs() < 0.01);
    }
}
