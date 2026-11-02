//! Source tosaveitemscript buried state: IsBuried, IsBuriedRay, UnBury and IsRecentlyUnburied.
use super::items::ItemStore;
use crate::reference::ItemId;
use crate::vehicle::{PhysicsWorld, to_rapier};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{QueryFilter, Ray, RigidBodyHandle, RigidBodyType};

/// IsRecentlyUnburied window: `Time.time < unburiedTime + 1f`.
pub const RECENT_SECONDS: f32 = 1.;
/// Collider layer that IsBuriedRay skips for the lowest point (player layer).
const SKIPPED_LAYER: u32 = 1 << 9;

/// Items unburied in the last second, with their remaining window.
pub struct BuriedItems {
    recent: Vec<(ItemId, f32)>,
    /// Items lifted in this frame (`buryLastUpdTime`).
    lifted: Vec<ItemId>,
}
impl Default for BuriedItems {
    fn default() -> Self {
        Self {
            recent: Vec::with_capacity(8),
            lifted: Vec::with_capacity(8),
        }
    }
}
impl BuriedItems {
    pub fn tick(&mut self, dt: f32) {
        self.lifted.clear();
        if self.recent.is_empty() {
            return;
        }
        for (_, left) in &mut self.recent {
            *left -= dt;
        }
        self.recent.retain(|(_, left)| *left > 0.);
    }
    pub fn recently_unburied(&self, id: ItemId) -> bool {
        self.recent.iter().any(|(item, _)| *item == id)
    }
    /// UnBury: a dynamic body that can be picked up again.
    pub fn unbury(&mut self, id: ItemId, items: &mut ItemStore, physics: &mut PhysicsWorld) {
        let Some(item) = items.get_mut(id) else {
            return;
        };
        if item.buried {
            self.recent.retain(|(item, _)| *item != id);
            self.recent.push((id, RECENT_SECONDS));
        }
        item.buried = false;
        if let Some(body) = item.body {
            physics.edit_body(body, |body| {
                body.set_body_type(RigidBodyType::Dynamic, true)
            });
        }
        items.mark_dirty(id);
    }
    /// UpdBuried: once per frame the item rises by `dt * speed`, then IsBuriedRay runs.
    pub fn lift(
        &mut self,
        id: ItemId,
        dt: f32,
        speed: f32,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        player: Option<RigidBodyHandle>,
    ) {
        if self.lifted.contains(&id) {
            return;
        }
        self.lifted.push(id);
        let Some(body) = items.get(id).and_then(|i| i.body) else {
            return;
        };
        physics.edit_body(body, |body| {
            let mut pose = *body.position();
            pose.translation.y += dt * speed;
            body.set_position(pose, true);
            if body.is_kinematic() {
                body.set_next_kinematic_position(pose);
            }
        });
        if items.get(id).is_some_and(|i| i.buried) && !buried_ray(id, items, physics, player) {
            self.unbury(id, items, physics);
        }
    }
    /// IsBuried: a buried item with a dynamic body or ground below it unburies.
    pub fn is_buried(
        &mut self,
        id: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        player: Option<RigidBodyHandle>,
    ) -> bool {
        let Some(item) = items.get(id) else {
            return false;
        };
        if !item.buried {
            return false;
        }
        let dynamic = item
            .body
            .and_then(|b| physics.rapier().bodies.get(b))
            .is_some_and(|b| b.is_dynamic());
        if dynamic || !buried_ray(id, items, physics, player) {
            self.unbury(id, items, physics);
            return false;
        }
        true
    }
}

/// Bury: a kinematic body that digging lifts.
pub fn bury(id: ItemId, items: &mut ItemStore, physics: &mut PhysicsWorld) {
    let Some(item) = items.get_mut(id) else {
        return;
    };
    item.buried = true;
    if let Some(body) = item.body {
        physics.edit_body(body, |body| {
            body.set_body_type(RigidBodyType::KinematicPositionBased, true)
        });
    }
    items.mark_dirty(id);
}

/// IsBuriedRay: true while no other collider lies below the item's lowest point.
/// Other buried items and the player do not count. Triggers count (queriesHitTriggers).
pub fn buried_ray(
    id: ItemId,
    items: &ItemStore,
    physics: &PhysicsWorld,
    player: Option<RigidBodyHandle>,
) -> bool {
    let Some(handle) = items.get(id).and_then(|i| i.body) else {
        return true;
    };
    let rapier = physics.rapier();
    let Some(body) = rapier.bodies.get(handle) else {
        return true;
    };
    let pose = *body.position();
    let origin = (pose * to_rapier(items.get(id).unwrap().local_pose)).translation;
    // ClosestPoint to a point far below is the lowest point of a convex collider.
    // Collider poses follow the body now: a lift moves the body before the next step.
    let lowest = body
        .colliders()
        .iter()
        .filter_map(|c| rapier.colliders.get(*c))
        .filter(|c| {
            c.is_enabled()
                && !c.is_sensor()
                && c.collision_groups().memberships.bits() != SKIPPED_LAYER
        })
        .map(|c| {
            let local = c.position_wrt_parent().copied().unwrap_or(Pose::IDENTITY);
            c.shape().compute_aabb(&(pose * local)).mins.y
        })
        .fold(f32::INFINITY, f32::min);
    let ray = Ray::new(origin, -Vector::Y);
    let filter = QueryFilter::default().exclude_rigid_body(handle);
    !physics
        .queries(filter)
        .intersect_ray(ray, f32::MAX, true)
        .any(|(_, collider, hit)| {
            let parent = collider.parent();
            collider.is_enabled()
                && origin.y - hit.time_of_impact < lowest
                && (parent.is_none() || parent != player)
                && !parent
                    .is_some_and(|parent| items.iter().any(|i| i.buried && i.body == Some(parent)))
        })
}
