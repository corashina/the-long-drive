//! Source `mountStuff.Lock` and `Upd` with `SLocalPos`: the host sends the pose of each stored
//! item relative to its storage owner. The port sends a pose when it changes, and to requests.
use super::claim::Claims;
use super::items::{CHECK_INTERVAL, Out, SYNC_MIN_POS, SYNC_MIN_ROT};
use super::message::{LocalPoseMessage, Message};
use super::objects::{NetKey, ObjectIndex};
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::starting_area::GarageRuntime;
use crate::vehicle::to_rapier;
use rapier3d::math::{Pose, Rotation, Vector};
use std::collections::HashMap;

#[derive(Default)]
pub struct LockSync {
    timer: f32,
    /// Host: the owner pose of each stored item as last sent.
    sent: HashMap<ItemId, Pose>,
    /// Guest: the owner and the owner pose that the host sent. A local lock moves there.
    wanted: HashMap<ItemId, (ItemId, Pose)>,
    /// Scratch list of the stored items to move.
    moves: Vec<(ItemId, Pose)>,
}

impl LockSync {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Host: sends changed owner poses each `CHECK_INTERVAL`, and the pose of each `requested`
    /// item. Guest: moves its stored items to the poses of the host.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn send(
        &mut self,
        runtime: &mut GarageRuntime,
        index: &ObjectIndex,
        claims: &Claims,
        host: bool,
        requested: &[NetKey],
        dt: f32,
        out: &mut Out,
    ) {
        let storage = &runtime.area.interaction.storage;
        if host {
            for &key in requested {
                if let Some(item) = index.item(key)
                    && let Some((owner, local)) = storage.owner_local(item, &runtime.area.items)
                {
                    send(index, item, owner, local, out);
                }
            }
        }
        self.timer += dt;
        if self.timer < CHECK_INTERVAL {
            return;
        }
        self.timer = 0.;
        let items = &runtime.area.items;
        if host {
            for state in items.iter() {
                let Some((owner, local)) = storage.owner_local(state.id, items) else {
                    continue;
                };
                if self.sent.get(&state.id).is_some_and(|p| same(p, &local)) {
                    continue;
                }
                if send(index, state.id, owner, local, out) {
                    self.sent.insert(state.id, local);
                }
            }
            self.sent
                .retain(|id, _| storage.owner_local(*id, items).is_some());
            return;
        }
        // The host locks at its own pose. A later local lock moves to it.
        self.wanted.retain(|id, _| {
            items.get(*id).is_some_and(|s| {
                !s.removed
                    && !claims.claimed(*id)
                    && matches!(
                        s.location,
                        ItemLocation::World | ItemLocation::Locked { .. }
                    )
            })
        });
        self.moves.clear();
        self.moves
            .extend(self.wanted.iter().filter_map(|(&item, &(owner, local))| {
                let (now_owner, now) = storage.owner_local(item, items)?;
                (now_owner == owner && !same(&now, &local)).then_some((item, local))
            }));
        let area = &mut runtime.area;
        for &(item, local) in &self.moves {
            area.interaction.storage.set_owner_local(
                item,
                local,
                &mut area.items,
                &mut runtime.physics,
            );
        }
    }

    /// Source `UpdLocalPos` on a guest. A loose item moves to the owner pose.
    pub fn receive(
        &mut self,
        runtime: &mut GarageRuntime,
        index: &ObjectIndex,
        claims: &Claims,
        host: bool,
        packet: &[u8],
    ) {
        let Ok((_, m)) = LocalPoseMessage::decode(packet) else {
            return;
        };
        let local = Pose::from_parts(
            Vector::from_array(m.position),
            Rotation::from_array(m.rotation).normalize(),
        );
        let (Some(item), Some(owner)) = (index.item(NetKey(m.key)), index.item(NetKey(m.owner)))
        else {
            return;
        };
        if host || !local.is_finite() || claims.claimed(item) {
            return;
        }
        self.wanted.insert(item, (owner, local));
        let area = &mut runtime.area;
        let (Some(state), Some(owner_state)) = (area.items.get(item), area.items.get(owner)) else {
            return;
        };
        match state.location {
            ItemLocation::Locked { owner: o, .. } if o == owner => {
                area.interaction.storage.set_owner_local(
                    item,
                    local,
                    &mut area.items,
                    &mut runtime.physics,
                );
            }
            ItemLocation::World if !state.removed && state.inactive_pose.is_none() => {
                let Some(body) = state.body else {
                    return;
                };
                let pose =
                    to_rapier(owner_state.pose) * local * to_rapier(state.local_pose).inverse();
                runtime
                    .physics
                    .edit_body(body, |b| b.set_position(pose, true));
            }
            _ => {}
        }
    }
}

fn send(index: &ObjectIndex, item: ItemId, owner: ItemId, local: Pose, out: &mut Out) -> bool {
    let (Some(key), Some(owner)) = (index.key(item), index.key(owner)) else {
        return false;
    };
    out.push(&LocalPoseMessage {
        owner: owner.0,
        key: key.0,
        position: local.translation.to_array(),
        rotation: local.rotation.to_array(),
        pad: 0,
    });
    true
}

/// Within the source `syncMinPos` and `syncMinRot`.
fn same(a: &Pose, b: &Pose) -> bool {
    f64::from(a.translation.distance(b.translation)) <= SYNC_MIN_POS
        && a.rotation.angle_between(b.rotation).to_degrees() <= SYNC_MIN_ROT
}
