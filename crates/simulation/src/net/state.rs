//! Item state of source `tankscript.Upd` (fluid), `tankcapscript.UpdCap` (tankcap) and
//! `usablescript.SyncMulti` (usable). The source sends at each change. The port reads the state each
//! `SYNC_TIME` and sends what changed since it last sent or received it.
use super::items::Out;
use super::message::{FluidMessage, TankCapMessage, UsableMessage};
use super::objects::{NetKey, ObjectIndex};
use crate::fluids::FluidEntry;
use crate::reference::ItemId;
use crate::starting_area::GarageRuntime;
use crate::world::MechanismSnapshot;
use std::collections::HashMap;

/// Source `snItemsync.syncTime`.
pub const SYNC_TIME: f32 = 0.25;
/// Smaller fluid changes are float noise.
const FLUID_EPSILON: f32 = 1e-4;

#[derive(Clone, Copy, PartialEq)]
struct Fluid {
    types: [u32; 6],
    amounts: [f32; 6],
    count: u32,
}

impl Fluid {
    fn of(entries: impl Iterator<Item = FluidEntry>) -> Self {
        let mut fluid = Self {
            types: [0; 6],
            amounts: [0.; 6],
            count: 0,
        };
        for (i, entry) in entries.take(6).enumerate() {
            fluid.types[i] = u32::from(entry.fluid_type);
            fluid.amounts[i] = entry.amount;
            fluid.count = i as u32 + 1;
        }
        fluid
    }
    fn of_message(m: &FluidMessage) -> Option<Self> {
        let count = m.count as usize;
        (count <= 6
            && m.types[..count].iter().all(|t| *t < 6)
            && m.amounts[..count].iter().all(|a| a.is_finite()))
        .then_some(Self {
            types: m.types,
            amounts: m.amounts,
            count: m.count,
        })
    }
    fn differs(&self, other: &Self) -> bool {
        let n = self.count as usize;
        self.count != other.count
            || self.types[..n] != other.types[..n]
            || (0..n).any(|i| (self.amounts[i] - other.amounts[i]).abs() > FLUID_EPSILON)
    }
    fn entries(&self) -> impl Iterator<Item = FluidEntry> + '_ {
        (0..self.count as usize).map(|i| FluidEntry {
            fluid_type: self.types[i] as u8,
            amount: self.amounts[i],
        })
    }
}

#[derive(Default)]
pub struct StateSync {
    timer: f32,
    fluids: HashMap<ItemId, Fluid>,
    valves: HashMap<ItemId, f32>,
    usables: HashMap<ItemId, MechanismSnapshot>,
    entries: Vec<FluidEntry>,
}

impl StateSync {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Sends changed states each `SYNC_TIME`, and every state of `requested` items.
    pub(super) fn send(
        &mut self,
        runtime: &GarageRuntime,
        index: &ObjectIndex,
        requested: &[NetKey],
        dt: f32,
        out: &mut Out,
    ) {
        for &key in requested {
            if let Some(item) = index.item(key) {
                self.send_all(runtime, item, key, out);
            }
        }
        self.timer += dt;
        if self.timer < SYNC_TIME {
            return;
        }
        self.timer = 0.;
        let area = &runtime.area;
        for state in area.items.iter() {
            let Some(fluid) = state.fluid.as_ref().filter(|_| !state.removed) else {
                continue;
            };
            let now = Fluid::of(fluid.entries());
            // The first read is the baseline: both peers start from one save.
            if let Some(known) = self.fluids.insert(state.id, now)
                && known.differs(&now)
                && let Some(key) = index.key(state.id)
            {
                send_fluid(key, &now, out);
            }
        }
        for (cap, valve) in area.interaction.valves() {
            if let Some(known) = self.valves.insert(cap, valve)
                && known != valve
                && let Some(key) = index.key(cap)
            {
                out.push(&TankCapMessage {
                    key: key.0,
                    valve,
                    pad: 0,
                });
            }
        }
        let mechanisms = &area.interaction.mechanisms;
        for item in mechanisms.controlled() {
            let Some(now) = mechanisms.snapshot(item) else {
                continue;
            };
            if let Some(known) = self.usables.insert(item, now)
                && known != now
                && let Some(key) = index.key(item)
            {
                send_usable(key, &now, out);
            }
        }
        self.fluids.retain(|id, _| area.items.get(*id).is_some());
        self.usables
            .retain(|id, _| mechanisms.snapshot(*id).is_some());
    }

    /// Source `SyncAll` for the states of this module.
    fn send_all(&mut self, runtime: &GarageRuntime, item: ItemId, key: NetKey, out: &mut Out) {
        let area = &runtime.area;
        if let Some(fluid) = area.items.get(item).and_then(|s| s.fluid.as_ref()) {
            send_fluid(key, &Fluid::of(fluid.entries()), out);
        }
        if let Some((_, valve)) = area.interaction.valves().find(|(cap, _)| *cap == item) {
            out.push(&TankCapMessage {
                key: key.0,
                valve,
                pad: 0,
            });
        }
        if let Some(now) = area.interaction.mechanisms.snapshot(item) {
            send_usable(key, &now, out);
        }
    }

    /// Source `FluidUpdate` with `dontSync`.
    pub fn got_fluid(&mut self, runtime: &mut GarageRuntime, item: ItemId, m: &FluidMessage) {
        let Some(fluid) = Fluid::of_message(m) else {
            return;
        };
        let Some(container) = runtime
            .area
            .items
            .get_mut(item)
            .and_then(|s| s.fluid.as_mut())
        else {
            return;
        };
        self.entries.clear();
        self.entries.extend(fluid.entries());
        container.restore_entries(&self.entries);
        self.fluids.insert(item, fluid);
        runtime.area.items.mark_dirty(item);
    }

    /// Source `TankCapUpdate`.
    pub fn got_valve(&mut self, runtime: &mut GarageRuntime, item: ItemId, m: &TankCapMessage) {
        if runtime
            .area
            .interaction
            .restore_generated_valve(item, m.valve)
            .is_ok()
        {
            self.valves.insert(item, m.valve);
            runtime.area.items.mark_dirty(item);
        }
    }

    /// Source `UsableUpd`: `usablescript.UpdFromMulti`.
    pub fn got_usable(&mut self, runtime: &mut GarageRuntime, item: ItemId, m: &UsableMessage) {
        let state = MechanismSnapshot {
            degrees: m.degrees,
            slide: m.slide,
            enabled: m.enabled != 0,
            turn_state: m.turn_state.min(u32::from(u8::MAX)) as u8,
        };
        let mechanisms = &mut runtime.area.interaction.mechanisms;
        if mechanisms.restore(item, state).is_ok()
            && let Some(now) = mechanisms.snapshot(item)
        {
            self.usables.insert(item, now);
            runtime.area.items.mark_dirty(item);
        }
    }
}

fn send_fluid(key: NetKey, fluid: &Fluid, out: &mut Out) {
    out.push(&FluidMessage {
        key: key.0,
        types: fluid.types,
        amounts: fluid.amounts,
        count: fluid.count,
        pad: 0,
    });
}

fn send_usable(key: NetKey, state: &MechanismSnapshot, out: &mut Out) {
    out.push(&UsableMessage {
        key: key.0,
        degrees: state.degrees,
        slide: state.slide,
        enabled: u32::from(state.enabled),
        turn_state: u32::from(state.turn_state),
        pad: 0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fluid_noise_is_no_change() {
        let entry = |amount| FluidEntry {
            fluid_type: 2,
            amount,
        };
        let a = Fluid::of([entry(5.)].into_iter());
        assert!(!a.differs(&Fluid::of([entry(5.00001)].into_iter())));
        assert!(a.differs(&Fluid::of([entry(4.9)].into_iter())));
        assert!(a.differs(&Fluid::of([entry(5.), entry(1.)].into_iter())));
        let mut bad = FluidMessage {
            count: 1,
            ..Default::default()
        };
        bad.types[0] = 9;
        assert!(Fluid::of_message(&bad).is_none());
    }
}
