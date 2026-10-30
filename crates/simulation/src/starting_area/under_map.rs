//! Source `visszarako` rescue: bodies that fell through the ground return above it.
//! Source PhysX terrain pushes bodies up from below. Rapier terrain does not, so every
//! body is checked, not only the player and the cars.
use super::recovery::{RecoveryTransport, transport_prepared};
use super::*;
use crate::interaction::items::ItemLocation;
use rapier3d::{
    math::{Pose, Vector},
    prelude::{QueryFilter, Ray},
};

/// Source `mainscript.DoVisszaRakas` checks every two seconds.
const INTERVAL: f64 = 2.;
/// Source `visszarako.isUnderMap` depth below the terrain.
const DEPTH: f32 = 2.;
/// Source `visszarako.UnderMap` lift above the terrain.
const LIFT: f32 = 5.;

impl GarageRuntime {
    pub(super) fn advance_under_map(&mut self) {
        if self.simulation_seconds < self.under_map_check {
            return;
        }
        self.under_map_check = self.simulation_seconds + INTERVAL;
        let player = self.area.player.body_handle();
        // A seated player follows the seat owner, which this check lifts.
        let seated = self.area.player.seated();
        // Every car lifts with its cargo. The check runs every two seconds, not each frame.
        let vehicles: Vec<_> = self
            .vehicles
            .keys()
            .filter_map(|key| {
                let chassis = self.physics.chassis_handle(key);
                self.physics.body_active(chassis).then_some(())?;
                Some((key.clone(), self.under_map(chassis)?))
            })
            .collect();
        let walking = (!seated).then(|| self.under_map(player)).flatten();
        let lifted = !vehicles.is_empty() || walking.is_some();
        for (vehicle, ground) in vehicles {
            self.lift_vehicle(&vehicle, ground);
        }
        if let Some(ground) = walking {
            let mut pose = self.area.player.body_pose(&self.physics);
            pose.position[1] = ground + LIFT;
            self.area.player.recover(pose, &mut self.physics);
        }
        let held = self.area.interaction.held();
        let fallen: Vec<_> = self
            .area
            .items
            .iter()
            .filter(|item| {
                item.location == ItemLocation::World
                    && !held.is_some_and(|root| self.area.items.attached_to(item.id, root))
            })
            .filter_map(|item| item.body)
            .filter(|&handle| {
                self.physics.vehicle_of(handle).is_none()
                    && self.physics.body_active(handle)
                    && self.physics.rapier().bodies[handle].is_dynamic()
            })
            .filter_map(|handle| Some((handle, self.under_map(handle)?)))
            .collect();
        if !lifted && fallen.is_empty() {
            return;
        }
        for (handle, ground) in fallen {
            self.physics.edit_body(handle, |body| {
                let mut pose = *body.position();
                pose.translation.y = ground + LIFT;
                body.set_position(pose, true);
                body.set_linvel(Vector::ZERO, true);
                body.set_angvel(Vector::ZERO, true);
            });
        }
        self.physics.refresh_queries();
        self.area.items.sync_poses(&self.physics);
        self.area.commit_poses(&self.physics, true);
    }

    /// Ground height when the body is below it and nothing is under the body.
    fn under_map(&self, handle: RigidBodyHandle) -> Option<f32> {
        let position = self.physics.rapier().bodies[handle].translation();
        let ground = self
            .stream
            .ground_height(&self.physics, position.to_array())?;
        if position.y >= ground - DEPTH {
            return None;
        }
        let ray = Ray::new(position, -Vector::Y);
        let filter = QueryFilter::default()
            .exclude_sensors()
            .exclude_rigid_body(handle);
        self.physics
            .queries(filter)
            .cast_ray(&ray, f32::MAX, true)
            .is_none()
            .then_some(ground)
    }

    /// Move the car, its parts, its cargo, and a seated player up together.
    fn lift_vehicle(&mut self, vehicle: &ObjectKey, ground: f32) {
        self.collect_recovery_ownership(vehicle);
        let mut pose = self.physics.snapshot(vehicle).pose;
        let rise = ground + LIFT - pose.position[1];
        pose.position[1] += rise;
        let seated = self.area.player.seat_vehicle() == Some(vehicle);
        let plan = RecoveryTransport {
            vehicle: pose,
            player: (!seated).then(|| self.area.player.body_pose(&self.physics)),
            held_delta: if seated {
                Pose::from_translation(Vector::Y * rise)
            } else {
                Pose::IDENTITY
            },
        };
        transport_prepared(
            &mut self.area,
            &mut self.physics,
            &mut self.wheels,
            &self.recovery_bodies,
            &self.recovery_held_bodies,
            plan,
            vehicle,
        );
    }
}
