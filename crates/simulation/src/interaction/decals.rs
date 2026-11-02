//! Bullet holes: `mainscript.ShootDecals` instances parented to the hit collider, and the
//! `DecalDestroyer` lifetime. Not saved.
use super::guns::DecalSpawn;
use crate::boundary::{DecalRow, PoseRow};
use crate::reference::{GUN_DECALS, ReferenceManifest};
use crate::vehicle::{PhysicsWorld, from_rapier};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{ColliderHandle, RigidBodyHandle};
use std::collections::VecDeque;

/// Live decal cap. A new decal drops the oldest. The source keeps every decal.
pub const DECAL_CAP: usize = 256;

#[derive(Clone, Copy, Debug)]
struct Decal {
    kind: u8,
    scale: f32,
    age: f32,
    collider: ColliderHandle,
    /// The collider's parent body. `local` is relative to it, else in stream space.
    body: Option<RigidBodyHandle>,
    local: Pose,
    /// The renderer interpolates the body. Other decals copy `current` into `previous`.
    smooth: bool,
    previous: Pose,
    current: Pose,
}

pub struct DecalStore {
    lifetimes: [Option<f32>; GUN_DECALS],
    /// Oldest first.
    decals: VecDeque<Decal>,
}

impl DecalStore {
    pub fn new(reference: &ReferenceManifest) -> Self {
        Self {
            lifetimes: std::array::from_fn(|i| reference.guns.decals[i].lifetime),
            decals: VecDeque::with_capacity(DECAL_CAP),
        }
    }
    /// Parents `spawn` to its collider. `smooth`: the renderer interpolates the collider's body.
    pub fn spawn(&mut self, spawn: DecalSpawn, physics: &PhysicsWorld, smooth: bool) {
        let Some(collider) = physics.rapier().colliders.get(spawn.collider) else {
            return;
        };
        let body = collider.parent();
        let local = match body {
            Some(b) => physics.rapier().bodies[b].position().inverse() * spawn.pose,
            None => spawn.pose,
        };
        if self.decals.len() == DECAL_CAP {
            self.decals.pop_front();
        }
        self.decals.push_back(Decal {
            kind: spawn.kind,
            scale: spawn.scale,
            age: 0.,
            collider: spawn.collider,
            body,
            local,
            smooth,
            previous: spawn.pose,
            current: spawn.pose,
        });
    }
    /// `DecalDestroyer`: drops decals past their lifetime.
    pub fn advance(&mut self, dt: f32) {
        let lifetimes = self.lifetimes;
        self.decals.retain_mut(|d| {
            d.age += dt;
            lifetimes[d.kind as usize].is_none_or(|l| d.age < l)
        });
    }
    /// Follows the parent bodies after a step. Drops decals whose collider or body left.
    pub fn commit(&mut self, physics: &PhysicsWorld, teleport: bool) {
        let rapier = physics.rapier();
        self.decals.retain_mut(|d| {
            if !rapier.colliders.contains(d.collider) {
                return false;
            }
            let pose = match d.body {
                Some(b) if !physics.body_active(b) => return false,
                Some(b) => match rapier.bodies.get(b) {
                    Some(body) => body.position() * d.local,
                    None => return false,
                },
                None => d.local,
            };
            d.previous = if teleport || !d.smooth {
                pose
            } else {
                d.current
            };
            d.current = pose;
            true
        });
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        let shift = Vector::from_array(shift);
        for d in &mut self.decals {
            if d.body.is_none() {
                d.local.translation -= shift;
            }
            d.previous.translation -= shift;
            d.current.translation -= shift;
        }
    }
    /// Frame rows, oldest first. Poses are in stream space.
    pub fn rows(&self) -> impl Iterator<Item = DecalRow> + '_ {
        self.decals.iter().map(|d| DecalRow {
            previous: PoseRow::new(from_rapier(&d.previous), 0),
            current: PoseRow::new(from_rapier(&d.current), 0),
            kind: u32::from(d.kind),
            scale: d.scale,
        })
    }
}
