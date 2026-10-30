//! Recovery clearance is complete before any transported body changes pose.
use super::*;
use rapier3d::{
    math::{Pose, Vector},
    parry::{query::intersection_test, shape::Shape},
    prelude::{Collider, ColliderHandle, QueryFilter},
};
#[derive(Clone, Copy)]
pub struct RecoveryTransport {
    pub vehicle: VehiclePose,
    pub player: Option<VehiclePose>,
    pub held_delta: Pose,
}
impl GarageRuntime {
    /// Bodies that move with car `vehicle`: its chassis, its other bodies and its cargo.
    pub fn collect_recovery_ownership(&mut self, vehicle: &ObjectKey) {
        self.recovery_bodies.clear();
        self.recovery_held_bodies.clear();
        let chassis = self.physics.chassis_handle(vehicle);
        self.recovery_bodies.push(chassis);
        let inverse = self.physics.rapier().bodies[chassis].position().inverse();
        let held = self.area.interaction.held();
        let car = &self.vehicles[vehicle];
        let bounds = &car.recovery_bounds;
        for &handle in &car.bodies {
            if !self.recovery_bodies.contains(&handle) {
                self.recovery_bodies.push(handle);
            }
        }
        for item in self.area.items.iter() {
            let Some(handle) = item.body else { continue };
            if held.is_some_and(|root| self.area.items.attached_to(item.id, root)) {
                if !self.recovery_held_bodies.contains(&handle) {
                    self.recovery_held_bodies.push(handle);
                }
                continue;
            }
            if self.recovery_bodies.contains(&handle) {
                continue;
            }
            let body = &self.physics.rapier().bodies[handle];
            let point = inverse * body.position().translation;
            let cargo = body.is_enabled()
                && body.is_dynamic()
                && (0..3)
                    .all(|axis| point[axis] >= bounds.min[axis] && point[axis] <= bounds.max[axis]);
            let dormant_cargo =
                self.persistent_world
                    .recovery_cargo(&self.physics, handle, vehicle, bounds);
            if item.body == Some(chassis) || cargo || dormant_cargo {
                self.recovery_bodies.push(handle);
            }
        }
    }
    pub fn recovery_clearance(
        &self,
        vehicle: &ObjectKey,
        pose: VehiclePose,
    ) -> Option<RecoveryTransport> {
        recovery_clearance(
            &self.area,
            &self.physics,
            &self.persistent_world,
            &self.recovery_bodies,
            &self.recovery_held_bodies,
            pose,
            vehicle,
        )
    }
    /// Moves the last seated car to the prepared recovery pose. Without a car it stops.
    pub fn advance_recovery(&mut self) -> Result<bool, ReferenceError> {
        if !self.recovering {
            return Ok(false);
        }
        let Some(vehicle) = self.vehicle.clone() else {
            self.recovering = false;
            return Ok(false);
        };
        let vehicle = &vehicle;
        self.collect_recovery_ownership(vehicle);
        let origin = self.stream.origin;
        let area = &self.area;
        let persistent = &self.persistent_world;
        let bodies = &self.recovery_bodies;
        let held = &self.recovery_held_bodies;
        let Some(target) =
            self.stream
                .prepare_recovery_pose(&self.physics, |physics, absolute| {
                    recovery_clearance(
                        area,
                        physics,
                        persistent,
                        bodies,
                        held,
                        VehiclePose {
                            position: origin.local(absolute.position),
                            rotation: absolute.rotation,
                        },
                        vehicle,
                    )
                    .is_some()
                })?
        else {
            return Ok(false);
        };
        let plan = self
            .recovery_clearance(
                vehicle,
                VehiclePose {
                    position: origin.local(target.position),
                    rotation: target.rotation,
                },
            )
            .ok_or_else(|| ReferenceError("Source recovery clearance changed".into()))?;
        let player = plan
            .player
            .map_or(target.position, |p| origin.absolute(p.position));
        let permit = self
            .stream
            .preflight_recovery(target, player, target.position)?;
        self.persistent_world.activate_recovery_bodies(
            &mut self.area,
            &mut self.physics,
            vehicle,
            &self.recovery_bodies,
        );
        self.clear_input();
        self.aim = ResolvedAim::default();
        let Self {
            stream,
            area,
            physics,
            wheels,
            recovery_bodies,
            recovery_held_bodies,
            ..
        } = self;
        stream.finish_recovery(permit, || {
            transport_prepared(
                area,
                physics,
                wheels,
                recovery_bodies,
                recovery_held_bodies,
                plan,
                vehicle,
            )
        })?;
        self.recovering = false;
        if let Some(car) = self.vehicles.get_mut(vehicle) {
            car.balance = Default::default();
        }
        Ok(true)
    }
}
pub(super) fn transport_prepared(
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    wheels: &mut crate::interaction::wheels::WheelAssemblies,
    bodies: &[RigidBodyHandle],
    held: &[RigidBodyHandle],
    plan: RecoveryTransport,
    vehicle: &ObjectKey,
) {
    area.interaction
        .finish_cancelled_input(&mut area.player, physics);
    area.interaction.drop_held(&mut area.items, physics);
    let delta = move_car(physics, vehicle, bodies, plan.vehicle);
    area.interaction.sync_mechanisms(physics);
    area.sync_attachments(physics, false);
    area.interaction
        .sync_parts(&mut area.items, physics)
        .expect("bound parts");
    wheels
        .sync(&mut area.items, physics)
        .expect("validated wheel bindings");
    if let Some(pose) = plan.player {
        area.player.recover(pose, physics);
    } else {
        transport_body(physics, area.player.body_handle(), delta);
    }
    for &body in held {
        transport_body(physics, body, plan.held_delta);
    }
    area.interaction
        .sync_parts(&mut area.items, physics)
        .expect("bound parts");
    physics.refresh_queries();
    area.items.sync_poses(physics);
    area.commit_poses(physics, true);
}

/// Moves car `vehicle` to `pose` and its other `bodies` with it. Returns the pose change.
fn move_car(
    physics: &mut PhysicsWorld,
    vehicle: &ObjectKey,
    bodies: &[RigidBodyHandle],
    pose: VehiclePose,
) -> Pose {
    let chassis = physics.chassis_handle(vehicle);
    let delta = to_rapier(pose) * physics.rapier().bodies[chassis].position().inverse();
    physics.reset(vehicle, pose);
    for &body in bodies {
        if body != chassis {
            transport_body(physics, body, delta);
        }
    }
    delta
}

pub fn recovery_clearance(
    area: &StartingArea,
    physics: &PhysicsWorld,
    persistent: &crate::save::PersistentWorld,
    bodies: &[RigidBodyHandle],
    held: &[RigidBodyHandle],
    pose: VehiclePose,
    vehicle: &ObjectKey,
) -> Option<RecoveryTransport> {
    let chassis = physics.chassis_handle(vehicle);
    let old_car = to_rapier(persistent.recovery_body_pose(physics, chassis, vehicle)?);
    let dormant = !physics.body_active(chassis);
    let collider_pose = |handle: RigidBodyHandle, collider: &Collider| -> Option<Pose> {
        if dormant && bodies.contains(&handle) {
            Some(
                to_rapier(persistent.recovery_body_pose(physics, handle, vehicle)?)
                    * *collider.position_wrt_parent()?,
            )
        } else {
            Some(*collider.position())
        }
    };
    let delta = to_rapier(pose) * old_car.inverse();
    let player_body = area.player.body_handle();
    let obstacle = |_: ColliderHandle, collider: &Collider| {
        collider.parent().is_none_or(|body| {
            body != player_body && !bodies.contains(&body) && !held.contains(&body)
        })
    };
    let clear_body = |body: RigidBodyHandle, shift: Pose| {
        physics.rapier().bodies[body]
            .colliders()
            .iter()
            .all(|handle| {
                let collider = &physics.rapier().colliders[*handle];
                collider.is_sensor()
                    || (!dormant && !collider.is_enabled())
                    || collider_pose(body, collider).is_some_and(|pose| {
                        physics
                            .queries(
                                QueryFilter::default()
                                    .exclude_sensors()
                                    .predicate(&obstacle),
                            )
                            .intersect_shape(shift * pose, collider.shape())
                            .next()
                            .is_none()
                    })
            })
    };
    if !bodies.iter().all(|body| clear_body(*body, delta)) {
        return None;
    }
    let clear_transported =
        |target: Pose, shape: &dyn Shape, groups: rapier3d::prelude::InteractionGroups| {
            bodies.iter().all(|body| {
                physics.rapier().bodies[*body]
                    .colliders()
                    .iter()
                    .all(|handle| {
                        let collider = &physics.rapier().colliders[*handle];
                        collider.is_sensor()
                            || !collider.collision_groups().test(groups)
                            || (!dormant && !collider.is_enabled())
                            || collider_pose(*body, collider).is_some_and(|pose| {
                                intersection_test(&target, shape, &(delta * pose), collider.shape())
                                    .is_ok_and(|hit| !hit.intersecting)
                            })
                    })
            })
        };
    let old_view = to_rapier(area.player.view(physics).head_pose);
    if area.player.seat_vehicle().is_some() {
        let neutral = area.player.neutral_view_at(crate::vehicle::from_rapier(
            &(delta * to_rapier(area.player.body_pose(physics))),
        ));
        let held_delta = to_rapier(neutral.head_pose) * old_view.inverse();
        if !held.iter().all(|body| clear_body(*body, held_delta)) {
            return None;
        }
        return Some(RecoveryTransport {
            vehicle: pose,
            player: None,
            held_delta,
        });
    }
    let shape = area.player.sweep_shape();
    for distance in [3.0, 4.0, 5.0, 6.0] {
        for side in [-1.0, 1.0] {
            let mut candidate = to_rapier(pose);
            candidate.translation += candidate.rotation
                * Vector::new(
                    distance * side,
                    -shape.compute_local_aabb().mins.y - 1.1 + 0.05,
                    0.0,
                );
            if physics
                .queries(
                    QueryFilter::default()
                        .exclude_sensors()
                        .groups(area.player.collision_groups())
                        .predicate(&obstacle),
                )
                .intersect_shape(candidate, shape)
                .next()
                .is_some()
                || !clear_transported(candidate, shape, area.player.collision_groups())
            {
                continue;
            }
            let view = to_rapier(
                area.player
                    .neutral_view_at(crate::vehicle::from_rapier(&candidate))
                    .head_pose,
            );
            let held_delta = view * old_view.inverse();
            if !held.iter().all(|body| clear_body(*body, held_delta)) {
                continue;
            }
            if !held.iter().all(|body| {
                physics.rapier().bodies[*body]
                    .colliders()
                    .iter()
                    .all(|handle| {
                        let collider = &physics.rapier().colliders[*handle];
                        collider.is_sensor()
                            || !collider.is_enabled()
                            || clear_transported(
                                held_delta * *collider.position(),
                                collider.shape(),
                                collider.collision_groups(),
                            )
                    })
            }) {
                continue;
            }
            return Some(RecoveryTransport {
                vehicle: pose,
                player: Some(crate::vehicle::from_rapier(&candidate)),
                held_delta,
            });
        }
    }
    None
}
