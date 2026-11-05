//! Multiplayer spawn: a guest steps off the host spot, and a dead player respawns.
use super::GarageRuntime;
use crate::reference::ReferenceManifest;
use crate::vehicle::{from_rapier, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::shape::Shape;
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray};
use std::f32::consts::TAU;

/// Ring radii around the start spot, in metres.
const RADII: [f32; 3] = [1.5, 2.5, 3.5];
/// Angles to try on each ring.
const ANGLES: usize = 8;
/// The ground ray starts this far above the spot and reaches this far.
const RAY_UP: f32 = 1.5;
const RAY_LENGTH: f32 = 4.;

impl GarageRuntime {
    /// A guest starts from the host save at the host spot. It moves to a free spot on a ring
    /// around that spot, from `turn` (0 to 1) of a full circle, so each slot takes its own spot.
    /// A guest in the host seat stands beside the car.
    pub fn step_aside(&mut self, turn: f32) {
        if self.needs_output().dead {
            return;
        }
        let base = to_rapier(self.area.player.body_pose(&self.physics));
        let base = Pose::from_parts(base.translation, yaw_only(base.rotation));
        if self.area.player.seated() {
            self.area
                .interaction
                .drop_held(&mut self.area.items, &mut self.physics);
            if self.area.player.try_exit(&mut self.physics) {
                self.after_move();
                return;
            }
        }
        if let Some(pose) = self.free_spot(base, turn) {
            self.area
                .interaction
                .drop_held(&mut self.area.items, &mut self.physics);
            self.area
                .player
                .recover(from_rapier(&pose), &mut self.physics);
            self.after_move();
        }
    }

    /// The dead player comes back beside the body with the needs of a new game. The worn items
    /// stay where they fell.
    pub fn respawn(&mut self, reference: &ReferenceManifest) {
        if !self.needs_output().dead {
            return;
        }
        let base = to_rapier(self.area.player.body_pose(&self.physics));
        let base = Pose::from_parts(base.translation, yaw_only(base.rotation));
        self.area.ragdoll.remove(&mut self.physics);
        let pose = self.free_spot(base, 0.).unwrap_or(base);
        self.area
            .player
            .revive(from_rapier(&pose), &mut self.physics);
        self.survival = crate::survival::SurvivalState::new(&reference.survival);
        self.death_announced = false;
        self.after_move();
    }

    fn after_move(&mut self) {
        self.previous_speed = None;
        self.death_protected = true;
        self.audio_motion_revision = self.audio_motion_revision.wrapping_add(1);
        self.physics.refresh_queries();
    }

    /// The first spot on the rings around `base` with ground under it and room for the player.
    fn free_spot(&self, base: Pose, turn: f32) -> Option<Pose> {
        let player = self.area.player.body_handle();
        let solid = |_: ColliderHandle, c: &Collider| c.is_enabled() && c.parent() != Some(player);
        let filter = || {
            QueryFilter::default()
                .exclude_sensors()
                .groups(self.area.player.collision_groups())
                .predicate(&solid)
        };
        let ground = |at: Vector| {
            let ray = Ray::new(at + Vector::Y * RAY_UP, -Vector::Y);
            self.physics
                .queries(filter())
                .cast_ray(&ray, RAY_LENGTH, true)
                .map(|(_, toi)| at.y + RAY_UP - toi)
        };
        let shape = self.area.player.sweep_shape();
        // The player pose stands this far above the ground.
        let stand = 0.05 - shape.compute_local_aabb().mins.y;
        for radius in RADII {
            for i in 0..ANGLES {
                let angle = (turn + i as f32 / ANGLES as f32) * TAU;
                let mut at = base.translation + Vector::new(angle.cos(), 0., angle.sin()) * radius;
                let Some(y) = ground(at) else {
                    continue;
                };
                at.y = y + stand;
                let pose = Pose::from_parts(at, base.rotation);
                if self
                    .physics
                    .queries(filter())
                    .intersect_shape(pose, shape)
                    .next()
                    .is_none()
                {
                    return Some(pose);
                }
            }
        }
        None
    }
}

/// The yaw part of `rotation`, so a body pose from a car or a ragdoll stands upright.
fn yaw_only(rotation: Rotation) -> Rotation {
    let forward = rotation * Vector::Z;
    Rotation::from_rotation_y(forward.x.atan2(forward.z))
}
