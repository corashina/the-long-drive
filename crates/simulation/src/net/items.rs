//! Source `tosaveitemscript.MultiUpd` and `snItemsync`: the owner sends item poses and velocity, the
//! other peers move their copy there.
use super::claim::{ClaimChange, Claims};
use super::message::{
    self, ClaimMessage, ItemPoseMessage, Message, RemoveMessage, RequestMessage, VelocityMessage,
};
use super::objects::{NetKey, ObjectIndex};
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::starting_area::GarageRuntime;
use crate::vehicle::{VehiclePose, to_rapier};
use rapier3d::math::{Rotation, Vector};
use std::collections::{HashMap, HashSet};

/// Source `itemPlaceRemoveScript.checkInterval`.
pub const CHECK_INTERVAL: f32 = 1.;
/// Source `MorePosUpd.moreCount`.
const MORE_COUNT: u8 = 3;
/// Source `snItemsync.syncMinPos`. The source keeps it unused; the port skips unmoved items.
pub const SYNC_MIN_POS: f64 = 0.05;
/// Source `snItemsync.syncMinRot`, in degrees.
pub const SYNC_MIN_ROT: f32 = 1.;

/// The outbox of one send.
pub(super) struct Out<'a> {
    pub slot: u8,
    pub bytes: &'a mut Vec<u8>,
}

impl Out<'_> {
    pub fn push<M: Message>(&mut self, message: &M) {
        message::push(self.slot, message, self.bytes);
    }
}

#[derive(Default)]
pub struct ItemSync {
    pub claims: Claims,
    /// The slot of this peer.
    pub slot: u8,
    timer: f32,
    /// Source `MorePosUpd`: held items get more sends between checks.
    more: Vec<(ItemId, f32, u8)>,
    last: HashMap<ItemId, ([f64; 3], [f32; 4])>,
    removed: HashSet<ItemId>,
    changes: Vec<ClaimChange>,
    fresh: Vec<NetKey>,
    /// Host: requested keys to answer with their state. `Net` clears it after each frame.
    pub requested: Vec<NetKey>,
    /// Host: the latest pose and velocity of items that it has not loaded.
    pending: HashMap<NetKey, (Option<ItemPoseMessage>, Option<VelocityMessage>)>,
}

impl ItemSync {
    pub fn clear(&mut self) {
        *self = Self {
            slot: self.slot,
            ..Self::default()
        };
    }

    /// Host: a pose or velocity of an item that it has not loaded. It applies when the item binds.
    pub fn keep(
        &mut self,
        key: NetKey,
        pose: Option<ItemPoseMessage>,
        velocity: Option<VelocityMessage>,
    ) {
        let entry = self.pending.entry(key).or_default();
        if pose.is_some() {
            entry.0 = pose;
        }
        if velocity.is_some() {
            entry.1 = velocity;
        }
    }
    /// Host: source `requestData` keys.
    pub fn requested(&mut self, keys: &[u64]) {
        self.requested.extend(keys.iter().map(|k| NetKey(*k)));
    }

    /// Host: applies kept states of items that bound. Guest: asks for their state.
    pub fn bound(
        &mut self,
        runtime: &mut GarageRuntime,
        index: &mut ObjectIndex,
        host: bool,
        out: &mut Vec<u8>,
    ) {
        index.take_fresh(&mut self.fresh);
        if host {
            for key in &self.fresh {
                let Some((pose, velocity)) = self.pending.remove(key) else {
                    continue;
                };
                let Some(item) = index.item(*key) else {
                    continue;
                };
                if let Some(pose) = pose {
                    apply_pose(runtime, &self.claims, item, &pose);
                }
                if let Some(velocity) = velocity {
                    apply_velocity(runtime, &self.claims, item, &velocity);
                }
            }
            return;
        }
        // Source `LoadStuff` on a guest: `SRequestData`.
        for keys in self.fresh.chunks(RequestMessage::KEYS) {
            let mut m = RequestMessage {
                keys: [0; RequestMessage::KEYS],
                count: keys.len() as u32,
                pad: 0,
            };
            for (k, key) in m.keys.iter_mut().zip(keys) {
                *k = key.0;
            }
            message::push(self.slot, &m, out);
        }
    }

    /// Claim changes, then the due poses. `host`: this peer is the host.
    pub fn send(
        &mut self,
        runtime: &GarageRuntime,
        index: &ObjectIndex,
        host: bool,
        clock: f64,
        dt: f32,
        out: &mut Vec<u8>,
    ) {
        let out = &mut Out {
            slot: self.slot,
            bytes: out,
        };
        let interaction = &runtime.area.interaction;
        let slots = runtime.area.inventory.slots;
        self.claims.observe(
            [
                interaction.carry.held(),
                interaction.equipment.item(),
                slots[0],
                slots[1],
                slots[2],
                slots[3],
            ],
            clock,
            &mut self.changes,
        );
        for change in std::mem::take(&mut self.changes) {
            match change {
                ClaimChange::Claim(item) => {
                    if let Some(key) = index.key(item) {
                        out.push(&ClaimMessage {
                            key: key.0,
                            claim: 1,
                            pad: 0,
                        });
                        self.more.push((item, 0., MORE_COUNT));
                    }
                }
                // Source `DoClaim(false)`: `ForcePosUpd(sendPosIfHasParent: true)` first.
                ClaimChange::Release(item) => {
                    if let Some(key) = index.key(item) {
                        self.send_pose(runtime, item, key, out);
                        out.push(&ClaimMessage {
                            key: key.0,
                            claim: 0,
                            pad: 0,
                        });
                    }
                }
            }
        }
        // Source `SSendAllData`.
        let requested = std::mem::take(&mut self.requested);
        for &key in &requested {
            if let Some(item) = index.item(key)
                && self.claims.owns(item, host)
            {
                self.send_pose(runtime, item, key, out);
            }
        }
        self.requested = requested;
        let step = CHECK_INTERVAL / f32::from(MORE_COUNT + 1);
        let mut i = 0;
        while i < self.more.len() {
            let (item, timer, left) = &mut self.more[i];
            *timer += dt;
            if *timer < step {
                i += 1;
                continue;
            }
            *timer = 0.;
            *left -= 1;
            let (item, left) = (*item, *left);
            if self.claims.owns(item, host)
                && let Some(key) = index.key(item)
            {
                self.send_pose(runtime, item, key, out);
            }
            if left == 0 {
                self.more.swap_remove(i);
            } else {
                i += 1;
            }
        }
        self.timer += dt;
        if self.timer < CHECK_INTERVAL {
            return;
        }
        self.timer = 0.;
        let origin = &runtime.stream.origin;
        for item in runtime.area.items.iter() {
            let Some(key) = index.key(item.id) else {
                continue;
            };
            // Source `OnDestroy`: the host sends the removal.
            if item.removed {
                if host && self.removed.insert(item.id) {
                    out.push(&RemoveMessage { key: key.0 });
                }
                continue;
            }
            if item.body.is_none()
                || item.inactive_pose.is_some()
                || !self.claims.owns(item.id, host)
            {
                continue;
            }
            let held = match item.location {
                ItemLocation::World => false,
                ItemLocation::Held | ItemLocation::Equipped => true,
                _ => continue,
            };
            let position = origin.absolute(item.pose.position);
            if !held
                && let Some((p, r)) = self.last.get(&item.id)
                && distance(p, &position) <= SYNC_MIN_POS
                && angle(r, &item.pose.rotation) <= SYNC_MIN_ROT
            {
                continue;
            }
            self.send_pose(runtime, item.id, key, out);
            if held {
                self.more.push((item.id, 0., MORE_COUNT));
            }
        }
        self.last
            .retain(|id, _| runtime.area.items.get(*id).is_some());
    }

    /// Source `ForcePosUpd`: the world pose, then the velocity. A held item sends no velocity.
    fn send_pose(&mut self, runtime: &GarageRuntime, item: ItemId, key: NetKey, out: &mut Out) {
        let Some(state) = runtime.area.items.get(item) else {
            return;
        };
        let position = runtime.stream.origin.absolute(state.pose.position);
        self.last.insert(item, (position, state.pose.rotation));
        let spawn = runtime.area.spawned.get(item).map_or([0; 3], |s| {
            [s.prototype as u32 + 1, s.serial, s.pickup as u32]
        });
        out.push(&ItemPoseMessage {
            key: key.0,
            position,
            rotation: state.pose.rotation,
            spawn,
            pad: 0,
        });
        let held = matches!(state.location, ItemLocation::Held | ItemLocation::Equipped);
        let (linear, angular) = match state.body {
            Some(handle) if !held => {
                let body = &runtime.physics.rapier().bodies[handle];
                (body.linvel().to_array(), body.angvel().to_array())
            }
            _ => ([0.; 3], [0.; 3]),
        };
        out.push(&VelocityMessage {
            key: key.0,
            linear,
            angular,
        });
    }
}

/// Source `UpdInWorld`: an item that this peer does not claim moves to the pose.
pub fn apply_pose(runtime: &mut GarageRuntime, claims: &Claims, item: ItemId, m: &ItemPoseMessage) {
    let Some(state) = runtime.area.items.get(item) else {
        return;
    };
    if claims.claimed(item)
        || state.removed
        || state.buried
        || state.inactive_pose.is_some()
        || state.location != ItemLocation::World
    {
        return;
    }
    let Some(handle) = state.body else {
        return;
    };
    let pose = VehiclePose {
        position: runtime.stream.origin.local(m.position),
        rotation: m.rotation,
    };
    let body_pose = to_rapier(pose) * to_rapier(state.local_pose).inverse();
    runtime
        .physics
        .edit_body(handle, |body| body.set_position(body_pose, true));
}

/// Source `VelocityUpd`.
pub fn apply_velocity(
    runtime: &mut GarageRuntime,
    claims: &Claims,
    item: ItemId,
    m: &VelocityMessage,
) {
    let Some(state) = runtime.area.items.get(item) else {
        return;
    };
    if claims.claimed(item) || state.removed || state.location != ItemLocation::World {
        return;
    }
    let Some(handle) = state.body else {
        return;
    };
    runtime.physics.edit_body(handle, |body| {
        if body.is_dynamic() {
            body.set_linvel(Vector::from_array(m.linear), true);
            body.set_angvel(Vector::from_array(m.angular), true);
        }
    });
}

fn distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// Angle between two rotations, in degrees.
fn angle(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    Rotation::from_array(*a)
        .angle_between(Rotation::from_array(*b))
        .to_degrees()
}

/// Source `UpdOrSpawnItem` and `AddObj`: a runtime spawn that this peer does not have.
pub fn spawn(
    reference: &crate::reference::ReferenceManifest,
    runtime: &mut GarageRuntime,
    m: &ItemPoseMessage,
) -> Option<ItemId> {
    let [prototype, serial, pickup] = m.spawn;
    let prototype = prototype.checked_sub(1)? as usize;
    let pose = VehiclePose {
        position: runtime.stream.origin.local(m.position),
        rotation: m.rotation,
    };
    // A spawned item pose is its body pose.
    crate::starting_area::spawned::spawn_body(
        reference,
        &mut runtime.area,
        &mut runtime.physics,
        prototype,
        pickup as usize,
        pose,
        serial,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_moves_are_below_the_source_limits() {
        assert!(distance(&[1e6, 0., 0.], &[1e6 + 0.04, 0., 0.]) <= SYNC_MIN_POS);
        assert!(distance(&[1e6, 0., 0.], &[1e6, 0.06, 0.]) > SYNC_MIN_POS);
        let turned = Rotation::from_rotation_y(2f32.to_radians()).to_array();
        assert!(angle(&[0., 0., 0., 1.], &turned) > SYNC_MIN_ROT);
        assert!(angle(&turned, &turned) <= SYNC_MIN_ROT);
    }
}
