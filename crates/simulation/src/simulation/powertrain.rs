//! Engine, radiator and gauge parts. Part state lives on the part. Each car copy reads the parts
//! mounted on it.
use std::collections::BTreeMap;

use crate::interaction::items::ItemStore;
use crate::reference::{EnginePartReference, GaugePartReference, ItemId, RadiatorPartReference};
use crate::simulation::engine::EngineState;
use crate::simulation::gauges::GaugeState;
use crate::world::{GeneratedId, ObjectKey};

/// An engine part item and its source `enginescript` record and state.
#[derive(Clone, Debug)]
pub struct EnginePart {
    pub item: ItemId,
    pub record: EnginePartReference,
    /// Item of `enginescript.T`.
    pub oil_tank: Option<ItemId>,
    /// Source `enginescript` runtime fields. They move with the part.
    pub state: EngineState,
    /// Gear stick `TransmissionHandPos`, `KickNitionHandPos` relative to the part item.
    pub gear_stick: [Option<crate::vehicle::VehiclePose>; 2],
}

impl EnginePart {
    pub fn new(item: ItemId, record: EnginePartReference, oil_tank: Option<ItemId>) -> Self {
        let state = EngineState::new(record.initial_temperature);
        Self {
            item,
            record,
            oil_tank,
            state,
            gear_stick: [None; 2],
        }
    }
}

/// A radiator part item and its source `coolantTankscript` record and state.
#[derive(Clone, Debug)]
pub struct RadiatorPart {
    pub item: ItemId,
    pub record: RadiatorPartReference,
    pub coolant_tank: Option<ItemId>,
    /// The placement whose prototype holds `record`. Its instance presents the fan.
    pub parent: GeneratedId,
    /// Source `coolantTankscript` fan hysteresis state.
    pub fan_on: bool,
    /// Source `Fan` turn around its local Z axis in degrees, modulo 360. It is not saved.
    pub fan_angle: f32,
    /// The fan angle of the last presented frame.
    pub presented_angle: f32,
}

impl RadiatorPart {
    pub fn new(
        item: ItemId,
        record: RadiatorPartReference,
        coolant_tank: Option<ItemId>,
        parent: GeneratedId,
    ) -> Self {
        Self {
            item,
            record,
            coolant_tank,
            parent,
            fan_on: false,
            fan_angle: 0.,
            presented_angle: 0.,
        }
    }
}

/// A gauge part item, its source `meterscript` record and state.
#[derive(Clone, Debug)]
pub struct GaugePart {
    pub item: ItemId,
    pub record: GaugePartReference,
    /// The placement whose prototype holds `record`. Its instance presents the gauge.
    pub parent: GeneratedId,
    /// Last known active content slot of `parent`.
    pub slot_hint: usize,
    pub state: GaugeState,
}

impl GaugePart {
    pub fn new(item: ItemId, record: GaugePartReference, parent: GeneratedId) -> Self {
        let state = GaugeState::new(&record);
        Self {
            item,
            record,
            parent,
            slot_hint: 0,
            state,
        }
    }
}

/// Items of the parts mounted on one car copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Powertrain {
    pub engine: Option<ItemId>,
    pub radiator: Option<ItemId>,
}

/// Every live engine and radiator part by item.
#[derive(Default)]
pub struct PowertrainParts {
    engines: BTreeMap<ItemId, EnginePart>,
    radiators: BTreeMap<ItemId, RadiatorPart>,
    gauges: BTreeMap<ItemId, GaugePart>,
}

impl PowertrainParts {
    pub fn register_engine(&mut self, part: EnginePart) {
        self.engines.insert(part.item, part);
    }
    pub fn register_radiator(&mut self, part: RadiatorPart) {
        self.radiators.insert(part.item, part);
    }
    pub fn register_gauge(&mut self, part: GaugePart) {
        self.gauges.insert(part.item, part);
    }
    /// Removes the engine, radiator or gauge of a retired item.
    pub fn remove(&mut self, item: ItemId) {
        self.engines.remove(&item);
        self.radiators.remove(&item);
        self.gauges.remove(&item);
    }
    pub fn gauge(&self, item: ItemId) -> Option<&GaugePart> {
        self.gauges.get(&item)
    }
    pub fn gauge_mut(&mut self, item: ItemId) -> Option<&mut GaugePart> {
        self.gauges.get_mut(&item)
    }
    pub fn gauges(&self) -> impl Iterator<Item = &GaugePart> {
        self.gauges.values()
    }
    pub fn gauges_mut(&mut self) -> impl Iterator<Item = &mut GaugePart> {
        self.gauges.values_mut()
    }
    /// The engine and radiator mounted on car `key`: the parts whose mount chain reaches its root item.
    pub fn mounted_on(&self, items: &ItemStore, key: &ObjectKey) -> Powertrain {
        let root = items.resolve_key(key);
        self.mounted(|item| root.is_some_and(|root| items.descends(item, root)))
    }
    /// The first engine and radiator by item for which `on_car` is true.
    fn mounted(&self, on_car: impl Fn(ItemId) -> bool) -> Powertrain {
        Powertrain {
            engine: self.engines.keys().copied().find(|item| on_car(*item)),
            radiator: self.radiators.keys().copied().find(|item| on_car(*item)),
        }
    }
    pub fn engine(&self, item: ItemId) -> Option<&EnginePart> {
        self.engines.get(&item)
    }
    pub fn engine_mut(&mut self, item: ItemId) -> Option<&mut EnginePart> {
        self.engines.get_mut(&item)
    }
    pub fn engines(&self) -> impl Iterator<Item = &EnginePart> {
        self.engines.values()
    }
    pub fn engines_mut(&mut self) -> impl Iterator<Item = &mut EnginePart> {
        self.engines.values_mut()
    }
    pub fn radiators(&self) -> impl Iterator<Item = &RadiatorPart> {
        self.radiators.values()
    }
    pub fn radiators_mut(&mut self) -> impl Iterator<Item = &mut RadiatorPart> {
        self.radiators.values_mut()
    }
    pub fn radiator(&self, item: ItemId) -> Option<&RadiatorPart> {
        self.radiators.get(&item)
    }
    pub fn radiator_mut(&mut self, item: ItemId) -> Option<&mut RadiatorPart> {
        self.radiators.get_mut(&item)
    }
    /// The engine and the radiator of one car, both writable.
    pub fn pair_mut(
        &mut self,
        powertrain: Powertrain,
    ) -> (Option<&mut EnginePart>, Option<&mut RadiatorPart>) {
        (
            powertrain
                .engine
                .and_then(|item| self.engines.get_mut(&item)),
            powertrain
                .radiator
                .and_then(|item| self.radiators.get_mut(&item)),
        )
    }
}
