//! Source `pickupable.SyncInventory` and `snItemsync.UpdInventory`: an item that another player
//! carries, holds or stores. A stored item hides. A carried or held item rides on that player and
//! moves smoothly to each pose (`SmoothMove`).
use super::items::Out;
use super::message::{InventoryMessage, ItemPoseMessage};
use super::objects::ObjectIndex;
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::starting_area::GarageRuntime;
use crate::vehicle::{VehiclePose, to_rapier};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::RigidBodyType;
use std::collections::HashMap;

/// Source `snItemsync.smoothMoveTime`.
pub const SMOOTH_MOVE_TIME: f32 = 0.3;
/// Source `snItemsync.smoothMoveSpeed`, in m/s.
pub const SMOOTH_MOVE_SPEED: f32 = 2.;

/// An item of another player.
struct Held {
    slot: u8,
    /// The body type to restore.
    body_type: RigidBodyType,
    /// Item pose relative to the figure of `slot`: source `transform.parent = handParent`.
    local: Option<Pose>,
    /// Source `SmoothMove`: the target relative to the figure, the elapsed time and the closest
    /// squared distance so far.
    target: Option<(Pose, f32, f32)>,
}

#[derive(Default)]
pub struct InventorySync {
    sent: Vec<(ItemId, u32)>,
    current: Vec<(ItemId, u32)>,
    held: HashMap<ItemId, Held>,
}

impl InventorySync {
    pub fn clear(&mut self, runtime: &mut GarageRuntime) {
        for (item, held) in std::mem::take(&mut self.held) {
            restore(runtime, item, &held);
        }
        self.sent.clear();
    }

    /// Source `SyncInventory`: sends each change of where this player keeps an item.
    pub(super) fn send(&mut self, runtime: &GarageRuntime, index: &ObjectIndex, out: &mut Out) {
        let area = &runtime.area;
        let carried = area.interaction.carry.held();
        let hand = area.interaction.equipment.item();
        self.current.clear();
        for item in area
            .inventory
            .slots
            .into_iter()
            .chain([carried, hand])
            .flatten()
        {
            if self.current.iter().any(|(i, _)| *i == item) {
                continue;
            }
            let mut flags = 0;
            if area
                .items
                .get(item)
                .is_some_and(|i| matches!(i.location, ItemLocation::Stored(_)))
            {
                flags |= InventoryMessage::STORED;
            }
            if hand == Some(item) {
                flags |= InventoryMessage::EQUIPPED;
            }
            if carried == Some(item) {
                flags |= InventoryMessage::PICKED;
            }
            self.current.push((item, flags));
        }
        for &(item, flags) in &self.current {
            if !self.sent.contains(&(item, flags))
                && let Some(key) = index.key(item)
            {
                out.push(&InventoryMessage {
                    key: key.0,
                    flags,
                    pad: 0,
                });
            }
        }
        for &(item, _) in &self.sent {
            if !self.current.iter().any(|(i, _)| *i == item)
                && let Some(key) = index.key(item)
            {
                out.push(&InventoryMessage {
                    key: key.0,
                    flags: 0,
                    pad: 0,
                });
            }
        }
        std::mem::swap(&mut self.sent, &mut self.current);
    }

    /// Source `UpdInventory`.
    pub fn got(&mut self, runtime: &mut GarageRuntime, item: ItemId, slot: u8, flags: u32) {
        let Some(state) = runtime.area.items.get(item) else {
            return;
        };
        // Source `HasNoAttachment`.
        if state.removed
            || !matches!(
                state.location,
                ItemLocation::World | ItemLocation::Held | ItemLocation::Equipped
            )
        {
            return;
        }
        let Some(handle) = state.body else {
            return;
        };
        if flags & (InventoryMessage::PICKED | InventoryMessage::EQUIPPED) != 0 {
            // Source: the other player takes it from this player's hands.
            let area = &mut runtime.area;
            if area.interaction.equipment.item() == Some(item) {
                let _ = area.release_equipped(&mut runtime.physics);
            }
            if area.interaction.carry.held() == Some(item) {
                area.interaction
                    .drop_held(&mut area.items, &mut runtime.physics);
            }
            area.sync_inventory();
        }
        if flags == 0 {
            if let Some(held) = self.held.remove(&item) {
                restore(runtime, item, &held);
            }
            return;
        }
        let held = self.held.entry(item).or_insert_with(|| Held {
            slot,
            body_type: runtime.physics.rapier().bodies[handle].body_type(),
            local: None,
            target: None,
        });
        held.slot = slot;
        // Source `UpdItemParent`: active unless stored and not in the right hand; no gravity.
        let shown =
            flags & InventoryMessage::STORED == 0 || flags & InventoryMessage::EQUIPPED != 0;
        runtime.physics.edit_body(handle, |body| {
            body.set_enabled(shown);
            body.set_body_type(RigidBodyType::KinematicPositionBased, true);
        });
    }

    /// Source `UpdInWorld` of an item that another player holds: it moves there smoothly.
    /// False if no other player holds `item`.
    pub fn target(
        &mut self,
        runtime: &GarageRuntime,
        figures: &[(u8, Pose)],
        item: ItemId,
        m: &ItemPoseMessage,
    ) -> bool {
        let Some(held) = self.held.get_mut(&item) else {
            return false;
        };
        let world = to_rapier(VehiclePose {
            position: runtime.stream.origin.local(m.position),
            rotation: m.rotation,
        });
        let figure = figure(figures, held.slot);
        held.target = Some((figure.inverse() * world, 0., f32::MAX));
        true
    }

    /// Moves the held items with their players. `figures`: each remote figure pose in stream space.
    pub fn follow(&mut self, runtime: &mut GarageRuntime, figures: &[(u8, Pose)], dt: f32) {
        self.held
            .retain(|item, _| runtime.area.items.get(*item).is_some_and(|i| !i.removed));
        for (item, held) in &mut self.held {
            let state = runtime.area.items.get(*item).unwrap();
            let Some(handle) = state.body else {
                continue;
            };
            let local_pose = to_rapier(state.local_pose);
            let figure = figure(figures, held.slot);
            let current = *runtime.physics.rapier().bodies[handle].position() * local_pose;
            let mut pose = match held.local {
                Some(local) => figure * local,
                None => current,
            };
            if let Some((target, elapsed, closest)) = &mut held.target {
                if *elapsed >= SMOOTH_MOVE_TIME {
                    held.target = None;
                } else {
                    let t = *elapsed / SMOOTH_MOVE_TIME;
                    let goal = figure * *target;
                    let gap = (pose.translation - goal.translation).length_squared();
                    if gap < *closest {
                        *closest = gap;
                        pose.translation = move_towards(
                            pose.translation,
                            goal.translation,
                            dt * SMOOTH_MOVE_SPEED,
                        );
                    }
                    pose.rotation = pose.rotation.lerp(goal.rotation, t).normalize();
                    *elapsed += dt;
                }
            }
            held.local = Some(figure.inverse() * pose);
            let body = pose * local_pose.inverse();
            runtime
                .physics
                .edit_body(handle, |b| b.set_next_kinematic_position(body));
        }
    }

    /// A player left: its items fall where they are.
    pub fn leave(&mut self, runtime: &mut GarageRuntime, slot: u8) {
        let items: Vec<ItemId> = self
            .held
            .iter()
            .filter(|(_, h)| h.slot == slot)
            .map(|(i, _)| *i)
            .collect();
        for item in items {
            let held = self.held.remove(&item).unwrap();
            restore(runtime, item, &held);
        }
    }
}

fn figure(figures: &[(u8, Pose)], slot: u8) -> Pose {
    figures
        .iter()
        .find(|(s, _)| *s == slot)
        .map_or(Pose::IDENTITY, |(_, p)| *p)
}

/// Source `UpdItemParent` with nothing held: the body is loose again.
fn restore(runtime: &mut GarageRuntime, item: ItemId, held: &Held) {
    let Some(handle) = runtime.area.items.get(item).and_then(|i| i.body) else {
        return;
    };
    let body_type = held.body_type;
    runtime.physics.edit_body(handle, |body| {
        body.set_body_type(body_type, true);
        body.set_enabled(true);
    });
}

/// Unity `Vector3.MoveTowards`.
fn move_towards(from: Vector, to: Vector, step: f32) -> Vector {
    let gap = to - from;
    let length = gap.length();
    if length <= step || length == 0. {
        to
    } else {
        from + gap / length * step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_towards_stops_at_the_target() {
        let (a, b) = (Vector::ZERO, Vector::new(3., 0., 4.));
        assert_eq!(move_towards(a, b, 1.), Vector::new(0.6, 0., 0.8));
        assert_eq!(move_towards(a, b, 6.), b);
    }
}
