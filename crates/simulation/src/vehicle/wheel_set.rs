//! Source WheelCollider set on one rigid body: a car, a sled or a bike.
//!
//! Each wheel casts a ray down the body up axis, solves its suspension load
//! and tire slip, then applies normal and tire impulses to the body.

use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{QueryFilter, QueryPipeline, Ray, RigidBodyHandle, RigidBodySet};

use super::WheelTorques;
use super::presentation_bodies::{BodyPresentationState, PresentationBodies};
use super::query_bounds::QueryBounds;
use super::wheel::{self, WheelContact, WheelForce, WheelFrame, WheelProfile, WheelState};
use crate::reference::WheelColliderReference;
use crate::simulation::FIXED_DT;

/// Tire impulse solver passes per physics step.
const TIRE_ITERATIONS: usize = 4;

/// One wheel of a set.
#[derive(Clone, Copy, Debug)]
pub struct WheelMount<'a> {
    /// Body-space wheel center at full compression.
    pub connection: Vector,
    pub collider: &'a WheelColliderReference,
    pub profile: WheelProfile,
}

type Contact = Option<(WheelForce, Vector, Option<RigidBodyHandle>)>;

/// Wheels on one body. Wheel indices are local to the owner.
pub struct WheelSet {
    body: RigidBodyHandle,
    /// A disabled set casts no rays and applies no forces.
    pub enabled: bool,
    wheels: Vec<WheelState>,
    colliders: Vec<WheelColliderReference>,
    contacts: Vec<Contact>,
    accumulated: Vec<[f32; 2]>,
}

impl WheelSet {
    /// An enabled set. Each wheel starts at its source rest length.
    pub fn new(body: RigidBodyHandle, mounts: &[WheelMount]) -> Self {
        Self {
            body,
            enabled: true,
            wheels: mounts
                .iter()
                .map(|mount| WheelState::new(mount.connection, mount.profile, rest(mount.collider)))
                .collect(),
            colliders: mounts.iter().map(|mount| mount.collider.clone()).collect(),
            contacts: vec![None; mounts.len()],
            accumulated: vec![[0.0; 2]; mounts.len()],
        }
    }

    pub fn body(&self) -> RigidBodyHandle {
        self.body
    }

    pub fn heap_bytes(&self) -> usize {
        self.wheels.capacity() * std::mem::size_of::<WheelState>()
            + self.colliders.capacity() * std::mem::size_of::<WheelColliderReference>()
            + self.contacts.capacity() * std::mem::size_of::<Contact>()
            + self.accumulated.capacity() * std::mem::size_of::<[f32; 2]>()
    }

    pub fn wheels(&self) -> &[WheelState] {
        &self.wheels
    }

    pub fn set_profile(&mut self, index: usize, profile: WheelProfile) {
        self.wheels[index].profile = profile;
    }

    /// Stops the wheels and puts them back at their rest length.
    pub fn reset(&mut self) {
        for (wheel, collider) in self.wheels.iter_mut().zip(&self.colliders) {
            wheel.omega = 0.0;
            wheel.previous_angle = wheel.angle;
            wheel.contact = None;
            wheel.suspension_length = rest(collider);
        }
    }

    pub fn clear_contacts(&mut self) {
        for wheel in &mut self.wheels {
            wheel.contact = None;
        }
    }

    /// One physics step. `torques` and `steer` hold one value per wheel.
    /// A disabled set or a sleeping body does no work.
    pub(super) fn step(
        &mut self,
        physics: &mut rapier3d::prelude::PhysicsWorld,
        query_bounds: &mut QueryBounds,
        presentation_bodies: &mut PresentationBodies,
        filter: QueryFilter,
        torques: &[WheelTorques],
        steer: &[f32],
    ) {
        assert_eq!(torques.len(), self.wheels.len());
        assert_eq!(steer.len(), self.wheels.len());
        let own = self.body;
        if !self.enabled || physics.bodies[own].is_sleeping() {
            return;
        }
        let pose = *physics.bodies[own].position();
        let up = pose.rotation * Vector::Y;
        {
            let queries = QueryPipeline {
                dispatcher: physics.narrow_phase.query_dispatcher(),
                bvh: &query_bounds.bvh,
                bodies: &physics.bodies,
                colliders: &physics.colliders,
                filter: filter.exclude_sensors().exclude_rigid_body(own),
            };
            for ((wheel, collider), steering) in
                self.wheels.iter_mut().zip(&self.colliders).zip(steer)
            {
                let travel = collider.suspension_distance;
                let radius = wheel.profile.radius;
                let origin = pose * wheel.connection;
                let hit =
                    queries.cast_ray_and_get_normal(&Ray::new(origin, -up), travel + radius, true);
                wheel.steering = *steering;
                wheel.previous_angle = wheel.angle;
                wheel.contact = hit.map(|(collider, hit)| WheelContact {
                    collider,
                    point: origin - up * hit.time_of_impact,
                    normal: if hit.time_of_impact > 0.0 && hit.normal != Vector::ZERO {
                        hit.normal
                    } else {
                        up
                    },
                });
                wheel.suspension_length = hit.map_or(travel, |(_, hit)| {
                    (hit.time_of_impact - radius).clamp(0.0, travel)
                });
            }
        }
        let gravity = physics.gravity.length();
        let sprung_mass = physics.bodies[own].mass() / self.wheels.len() as f32;
        for (((wheel, collider), contact), torques) in self
            .wheels
            .iter_mut()
            .zip(&self.colliders)
            .zip(&mut self.contacts)
            .zip(torques)
        {
            let ground = wheel.contact.and_then(|contact| {
                physics.colliders[contact.collider]
                    .parent()
                    .filter(|parent| physics.bodies[*parent].is_dynamic())
            });
            let point = wheel.contact.map_or(Vector::ZERO, |contact| {
                contact.point + up * collider.force_application_distance_m
            });
            let frame = WheelFrame {
                up,
                axle: pose.rotation * Rotation::from_rotation_y(wheel.steering) * Vector::X,
                velocity: relative_velocity(&physics.bodies, own, ground, point),
                sprung_mass,
                gravity,
            };
            *contact = wheel::solve(wheel, collider, frame, *torques, FIXED_DT)
                .map(|force| (force, point, ground));
        }
        // All wheels see the same pre-impulse state.
        for (force, point, ground) in self.contacts.iter().flatten() {
            apply_wheel_impulse(
                &mut physics.bodies,
                query_bounds,
                presentation_bodies,
                own,
                *ground,
                *point,
                force.up * (force.load * FIXED_DT),
            );
        }
        // Tire impulses: a few projected Gauss-Seidel passes over the contacts.
        self.accumulated.fill([0.0; 2]);
        for _ in 0..TIRE_ITERATIONS {
            for (contact, accumulated) in self.contacts.iter().zip(&mut self.accumulated) {
                let Some((force, point, ground)) = contact else {
                    continue;
                };
                let velocity = relative_velocity(&physics.bodies, own, *ground, *point);
                let mut inverse = [
                    inverse_mass_at(&physics.bodies[own], *point, force.forward),
                    inverse_mass_at(&physics.bodies[own], *point, force.side),
                ];
                if let Some(ground) = ground {
                    let body = &physics.bodies[*ground];
                    inverse[0] += inverse_mass_at(body, *point, force.forward);
                    inverse[1] += inverse_mass_at(body, *point, force.side);
                }
                let needed = [
                    (force.surface_speed - velocity.dot(force.forward)) / inverse[0],
                    -velocity.dot(force.side) / inverse[1],
                ];
                let limits = force.limits(FIXED_DT);
                let previous = *accumulated;
                let next = force.ellipse(
                    std::array::from_fn(|axis| {
                        (previous[axis] + needed[axis]).clamp(limits[axis].0, limits[axis].1)
                    }),
                    FIXED_DT,
                );
                *accumulated = next;
                let delta =
                    force.forward * (next[0] - previous[0]) + force.side * (next[1] - previous[1]);
                apply_wheel_impulse(
                    &mut physics.bodies,
                    query_bounds,
                    presentation_bodies,
                    own,
                    *ground,
                    *point,
                    delta,
                );
            }
        }
    }
}

/// Source rest position is targetPosition of the travel below the connection.
fn rest(collider: &WheelColliderReference) -> f32 {
    collider.suspension_distance * collider.target_position
}

/// Applies a wheel impulse to the body and its reaction to a dynamic ground body.
fn apply_wheel_impulse(
    bodies: &mut RigidBodySet,
    query_bounds: &mut QueryBounds,
    presentation_bodies: &mut PresentationBodies,
    own: RigidBodyHandle,
    ground: Option<RigidBodyHandle>,
    point: Vector,
    impulse: Vector,
) {
    bodies[own].apply_impulse_at_point(impulse, point, false);
    if let Some(ground) = ground {
        query_bounds.mark_body(ground);
        let body = &mut bodies[ground];
        let before = BodyPresentationState::capture(body);
        body.apply_impulse_at_point(-impulse, point, true);
        presentation_bodies.mark_if_changed(ground, before, body);
    }
}

/// Body velocity minus ground body velocity at `point`.
fn relative_velocity(
    bodies: &RigidBodySet,
    own: RigidBodyHandle,
    ground: Option<RigidBodyHandle>,
    point: Vector,
) -> Vector {
    let velocity = bodies[own].velocity_at_point(point);
    ground.map_or(velocity, |ground| {
        velocity - bodies[ground].velocity_at_point(point)
    })
}

/// Inverse effective mass of `body` for an impulse at `point` along `direction`.
fn inverse_mass_at(body: &rapier3d::prelude::RigidBody, point: Vector, direction: Vector) -> f32 {
    let props = body.mass_properties();
    let arm = (point - props.world_com).cross(direction);
    (props.effective_inv_mass * direction * direction).element_sum()
        + arm.dot(props.effective_world_inv_inertia * arm)
}

#[cfg(test)]
mod tests;
