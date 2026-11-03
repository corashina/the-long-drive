//! Source `partconditionscript.Refresh` with `multiUpd` (`SPartCondition`): rust, dirt and paint.
//! The port reads the entries each `SYNC_TIME` and sends what changed since it last sent or
//! received it.
use super::items::Out;
use super::message::{ConditionMessage, Message};
use super::objects::{NetKey, ObjectIndex};
use super::state::SYNC_TIME;
use crate::conditions::ConditionState;
use crate::reference::{ConditionId, ItemId};
use crate::save::SourceKey;
use crate::starting_area::GarageRuntime;
use crate::world::fnv1a;
use std::collections::{HashMap, HashSet};

/// The shared fields. The source sends no repair progress.
#[derive(Clone, Copy, PartialEq)]
struct Value {
    state: u8,
    state2: u32,
    state3: u32,
    color: [f32; 4],
}

impl Value {
    fn of(state: &ConditionState) -> Self {
        Self {
            state: state.state,
            state2: state.state2,
            state3: state.state3,
            color: state.color,
        }
    }
}

#[derive(Default)]
pub struct ConditionSync {
    timer: f32,
    /// Each entry as last sent or received, with the item that saves it.
    known: HashMap<ConditionId, (Option<ItemId>, Value)>,
    wanted: HashSet<NetKey>,
}

impl ConditionSync {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Sends changed entries each `SYNC_TIME`, and the changed entries of `requested` items.
    pub(super) fn send(
        &mut self,
        runtime: &GarageRuntime,
        index: &ObjectIndex,
        requested: &[NetKey],
        dt: f32,
        out: &mut Out,
    ) {
        let conditions = &runtime.area.interaction.conditions;
        if !requested.is_empty() {
            self.wanted.clear();
            self.wanted.extend(requested);
            for (_, item, source, state, spawn) in conditions.world_entries() {
                if state != spawn
                    && let Some(key) = index.key(item)
                    && self.wanted.contains(&key)
                {
                    send(key.0, seed(source), &Value::of(state), out);
                }
            }
        }
        self.timer += dt;
        if self.timer < SYNC_TIME {
            return;
        }
        self.timer = 0.;
        // The first read is the baseline. Both peers start from one save.
        for (id, state) in conditions.iter() {
            let now = Value::of(state);
            if let Some((_, known)) = self.known.insert(id, (None, now))
                && known != now
            {
                send(0, u64::from(id.0), &now, out);
            }
        }
        for (id, item, source, state, _) in conditions.world_entries() {
            let now = Value::of(state);
            // A reused id with another owner is a new baseline.
            if let Some((owner, known)) = self.known.insert(id, (Some(item), now))
                && owner == Some(item)
                && known != now
                && let Some(key) = index.key(item)
            {
                send(key.0, seed(source), &now, out);
            }
        }
    }

    /// Source `PartConditionUpd` of another peer.
    pub fn receive(&mut self, runtime: &mut GarageRuntime, index: &ObjectIndex, packet: &[u8]) {
        if let Ok((_, m)) = ConditionMessage::decode(packet) {
            self.got(runtime, index, &m);
        }
    }

    fn got(&mut self, runtime: &mut GarageRuntime, index: &ObjectIndex, m: &ConditionMessage) {
        if m.state > 5 || !m.color.iter().all(|c| c.is_finite()) {
            return;
        }
        let conditions = &mut runtime.area.interaction.conditions;
        let (id, owner) = if m.key == 0 {
            let Some((id, _)) = conditions.iter().find(|(id, _)| u64::from(id.0) == m.seed) else {
                return;
            };
            (id, None)
        } else {
            let Some(item) = index.item(NetKey(m.key)) else {
                return;
            };
            let Some((id, ..)) = conditions
                .world_entries()
                .find(|(_, owner, source, ..)| *owner == item && seed(source) == m.seed)
            else {
                return;
            };
            (id, Some(item))
        };
        let mut state = *conditions.get(id);
        state.state = m.state as u8;
        state.state2 = m.state2;
        state.state3 = m.state3;
        state.color = m.color;
        conditions.set_shared(id, state);
        self.known.insert(id, (owner, Value::of(&state)));
    }
}

/// The seed of a saved entry source.
fn seed(source: &SourceKey) -> u64 {
    fnv1a(source.file.as_bytes()) ^ fnv1a(source.path_id.as_bytes()).rotate_left(1)
}

fn send(key: u64, seed: u64, value: &Value, out: &mut Out) {
    out.push(&ConditionMessage {
        key,
        seed,
        state: u32::from(value.state),
        state2: value.state2,
        state3: value.state3,
        pad: 0,
        color: value.color,
    });
}
