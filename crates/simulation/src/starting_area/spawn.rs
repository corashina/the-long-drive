//! Multiplayer spawn. A joining guest starts in the garage. A dead player respawns near another
//! player, or in the garage. Then the player moves to a free spot near that base.
use super::GarageRuntime;
use crate::reference::ReferenceManifest;
use crate::vehicle::{from_rapier, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::bounding_volume::{Aabb, BoundingVolume};
use rapier3d::parry::shape::Shape;
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray};
use std::f32::consts::TAU;

/// Ring radii around the base, in metres. The base itself is tried first.
const RADII: [f32; 5] = [1.5, 2.5, 3.5, 4.5, 6.];
/// Angles to try on each ring.
const ANGLES: usize = 8;
/// The ground ray starts this far above the spot and reaches this far.
const RAY_UP: f32 = 1.5;
const RAY_LENGTH: f32 = 4.;
/// A spot this close to the bounds of a car is not free.
const CAR_MARGIN: f32 = 0.3;
/// A spot this close to another player, flat, is not free. A new figure body is not solid yet.
const PLAYER_GAP: f32 = 1.;

/// The save of a joining guest: its player stands still on the garage start spot.
pub fn garage_start(reference: &ReferenceManifest, player: &mut crate::save::PlayerSnapshot) {
    let spawn = &reference.area.player_spawn;
    player.pose = crate::save::AbsolutePose {
        position: spawn.translation.map(f64::from),
        rotation: spawn.rotation,
    };
    player.yaw = crate::player::yaw_from_rotation(Rotation::from_array(spawn.rotation));
    player.pitch = 0.;
    player.seat = None;
    player.posture = 1.;
    player.vertical_velocity = 0.;
    player.recoil_velocity = [0.; 3];
    player.momentum = [0.; 3];
    player.running = false;
}

impl GarageRuntime {
    /// The garage start spot in stream space.
    pub fn garage_spot(&self, reference: &ReferenceManifest) -> Pose {
        let spawn = &reference.area.player_spawn;
        Pose::from_parts(
            Vector::from_array(self.stream.origin.local(spawn.translation.map(f64::from))),
            Rotation::from_array(spawn.rotation),
        )
    }

    /// Moves the player from its base to a free spot near it, from `turn` (0 to 1) of a full
    /// circle, so each slot takes its own spot. `others` are the other player figures. Without a
    /// free spot, the player stays on the base.
    pub fn place(&mut self, turn: f32, others: &[Vector]) {
        if self.needs_output().dead || self.area.player.seated() {
            return;
        }
        self.area
            .interaction
            .drop_held(&mut self.area.items, &mut self.physics);
        self.physics.refresh_queries();
        let base = to_rapier(self.area.player.body_pose(&self.physics));
        let base = Pose::from_parts(base.translation, yaw_only(base.rotation));
        if let Some(pose) = self.free_spot(base, turn, others) {
            self.area
                .player
                .recover(from_rapier(&pose), &mut self.physics);
            self.after_move();
        }
    }

    /// The dead player comes back on `base` with the needs of a new game. The worn items stay
    /// where they fell. `place` then finds a free spot, once the world there stands.
    pub fn respawn(&mut self, reference: &ReferenceManifest, base: Pose) {
        if !self.needs_output().dead {
            return;
        }
        let base = Pose::from_parts(base.translation, yaw_only(base.rotation));
        self.area.ragdoll.remove(&mut self.physics);
        self.area
            .player
            .revive(from_rapier(&base), &mut self.physics);
        self.survival = crate::survival::SurvivalState::new(&reference.survival);
        self.death_announced = false;
        self.after_move();
        // The world check reads the stream request, so the request moves to the base now.
        self.request_focus();
    }

    fn after_move(&mut self) {
        self.previous_speed = None;
        self.death_protected = true;
        self.audio_motion_revision = self.audio_motion_revision.wrapping_add(1);
        self.physics.refresh_queries();
    }

    /// The base, then the first spot on the rings around it, that is free: fixed ground is the
    /// first hit under it, no solid collider of any layer is in the player shape, and it is
    /// clear of every car and of `others`. A car cabin is hollow, so only its bounds keep a spot
    /// out of it.
    fn free_spot(&self, base: Pose, turn: f32, others: &[Vector]) -> Option<Pose> {
        let player = self.area.player.body_handle();
        let rapier = self.physics.rapier();
        let solid = |_: ColliderHandle, c: &Collider| c.is_enabled() && c.parent() != Some(player);
        let filter = || QueryFilter::default().exclude_sensors().predicate(&solid);
        let fixed = |handle: ColliderHandle| {
            rapier.colliders[handle]
                .parent()
                .is_none_or(|body| rapier.bodies[body].is_fixed())
        };
        let ground = |at: Vector| {
            let ray = Ray::new(at + Vector::Y * RAY_UP, -Vector::Y);
            self.physics
                .queries(filter())
                .cast_ray(&ray, RAY_LENGTH, true)
                .filter(|(handle, _)| fixed(*handle))
                .map(|(_, toi)| at.y + RAY_UP - toi)
        };
        let cars: Vec<Aabb> = self
            .vehicles
            .keys()
            .filter_map(|key| {
                rapier.bodies[self.physics.chassis_handle(key)]
                    .colliders()
                    .iter()
                    .map(|c| rapier.colliders[*c].compute_aabb())
                    .reduce(|a, b| a.merged(&b))
                    .map(|aabb| aabb.loosened(CAR_MARGIN))
            })
            .collect();
        let shape = self.area.player.sweep_shape();
        // The player pose stands this far above the ground.
        let stand = 0.05 - shape.compute_local_aabb().mins.y;
        let spots = std::iter::once(base.translation).chain(RADII.into_iter().flat_map(|radius| {
            (0..ANGLES).map(move |i| {
                let angle = (turn + i as f32 / ANGLES as f32) * TAU;
                base.translation + Vector::new(angle.cos(), 0., angle.sin()) * radius
            })
        }));
        let clear = |at: Vector| {
            others
                .iter()
                .all(|o| (*o - at).with_y(0.).length() >= PLAYER_GAP)
        };
        for mut at in spots.filter(|at| clear(*at)) {
            let Some(y) = ground(at) else {
                continue;
            };
            at.y = y + stand;
            let pose = Pose::from_parts(at, base.rotation);
            let bounds = shape.compute_aabb(&pose);
            if cars.iter().all(|car| !car.intersects(&bounds))
                && self
                    .physics
                    .queries(filter())
                    .intersect_shape(pose, shape)
                    .next()
                    .is_none()
            {
                return Some(pose);
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
