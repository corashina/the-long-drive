//! Session edits and their destination index. Records contain no runtime handles.
use super::{AbsolutePose, ObjectKey, Placement, WorldBounds};
use crate::reference::SourceId;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentSnapshot {
    pub owner: Option<ObjectKey>,
    pub slot: Option<SourceId>,
    /// Storage lock: the source volume, or the prototype volume of a world owner.
    pub volume: Option<SourceId>,
    pub local_pose: AbsolutePose,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismSnapshot {
    pub degrees: [f32; 2],
    pub slide: f32,
    pub enabled: bool,
    pub turn_state: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodSnapshot {
    pub source: SourceId,
    pub bite: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectSnapshot {
    pub pose: AbsolutePose,
    pub food: Option<FoodSnapshot>,
    pub temperature: Option<f32>,
    pub fluids: Option<crate::fluids::FluidSnapshot>,
    pub valve: Option<f32>,
    pub lamp: Option<bool>,
    /// Source `Lamp.broken` set by flashlightscript.Break. Bit i is `Lamps[i]`. None at spawn.
    pub lamp_broken: Option<u64>,
    /// Tuned compass bearing or clock offset (meterscript szamlap value).
    pub dial: Option<f32>,
    /// Spray bottle or loose ammo ammoscript count; None at its spawn count.
    pub ammo: Option<f32>,
    /// Gun state; None at its spawn state.
    pub gun: Option<crate::interaction::guns::GunSnapshot>,
    /// Genie lamp ginlampscript value; None at its startup value.
    pub glow: Option<f32>,
    /// save_radioscript controls of a portable radio.
    pub radio: Option<crate::save::RadioSnapshot>,
    /// Metal detector onOffButton turn state.
    pub detector: Option<bool>,
    /// Loot tool state; None at its spawn state.
    pub tool: Option<crate::save::ToolStateSnapshot>,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub sleeping: bool,
    pub held: bool,
    /// Source tosaveitemscript.buried.
    pub buried: bool,
    pub attachment: Option<AttachmentSnapshot>,
    pub mechanism: Option<MechanismSnapshot>,
    /// Source save_partcondition of the item's world parts that differ from their spawn state.
    pub conditions: Vec<crate::save::SurfaceSnapshot>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectChange {
    /// None is a retained removal, not an absent edit.
    pub snapshot: Option<ObjectSnapshot>,
    /// Original source caller placement. Initialized arrays are replayed.
    pub replay: Option<Placement>,
    pub instance: Option<super::GeneratedId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeRecordIndex(usize);

#[derive(Clone, Debug, Default, Serialize)]
pub struct ChangeRecords {
    records: Vec<(ObjectKey, ObjectChange)>,
    #[serde(skip)]
    identities: HashMap<ObjectKey, usize>,
    #[serde(skip)]
    attachment_owners: HashMap<ObjectKey, usize>,
    #[serde(skip)]
    attachment_owner_bytes: usize,
    #[serde(skip)]
    regions: BTreeMap<(i64, i64), Vec<usize>>,
    #[serde(skip)]
    revision: u64,
    #[serde(skip)]
    payload_bytes: usize,
    #[serde(skip)]
    region_bytes: usize,
    /// Car-side state of car objects. A car takes its state back when it spawns.
    #[serde(serialize_with = "pairs")]
    vehicles: BTreeMap<ObjectKey, crate::starting_area::VehicleState>,
    /// The rim object of each wheel slot of car objects, in wheel order.
    #[serde(serialize_with = "pairs")]
    wheel_slots: BTreeMap<ObjectKey, Vec<Option<ObjectKey>>>,
    /// Source `enginescript` state of engine part objects.
    #[serde(serialize_with = "pairs")]
    engines: BTreeMap<ObjectKey, crate::save::EngineSnapshot>,
    /// Source `coolantTankscript` fan state of radiator part objects.
    #[serde(serialize_with = "pairs")]
    radiators: BTreeMap<ObjectKey, bool>,
    /// Source `busdoorscripts` door state (`Bopened`, `Bsigned`) of bus objects, in door order.
    #[serde(serialize_with = "pairs")]
    bus_doors: BTreeMap<ObjectKey, Vec<[bool; 2]>>,
    /// Source `save_meterscript` values of gauge part objects.
    #[serde(serialize_with = "pairs")]
    gauges: BTreeMap<ObjectKey, crate::simulation::gauges::GaugeSnapshot>,
}
impl PartialEq for ChangeRecords {
    fn eq(&self, other: &Self) -> bool {
        self.records == other.records
            && self.vehicles == other.vehicles
            && self.wheel_slots == other.wheel_slots
            && self.engines == other.engines
            && self.radiators == other.radiators
            && self.bus_doors == other.bus_doors
            && self.gauges == other.gauges
    }
}
/// Writes a map with object keys as a list of pairs. JSON object keys must be strings.
fn pairs<V: Serialize, S: serde::Serializer>(
    map: &BTreeMap<ObjectKey, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(map.iter())
}
/// Reads a list of pairs. A key must occur once.
fn unique<V, E: serde::de::Error>(pairs: Vec<(ObjectKey, V)>) -> Result<BTreeMap<ObjectKey, V>, E> {
    let count = pairs.len();
    let map: BTreeMap<_, _> = pairs.into_iter().collect();
    if map.len() != count {
        return Err(E::custom("Duplicate object state identity"));
    }
    Ok(map)
}
impl<'de> Deserialize<'de> for ChangeRecords {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            records: Vec<(ObjectKey, ObjectChange)>,
            vehicles: Vec<(ObjectKey, crate::starting_area::VehicleState)>,
            wheel_slots: Vec<(ObjectKey, Vec<Option<ObjectKey>>)>,
            engines: Vec<(ObjectKey, crate::save::EngineSnapshot)>,
            radiators: Vec<(ObjectKey, bool)>,
            bus_doors: Vec<(ObjectKey, Vec<[bool; 2]>)>,
            gauges: Vec<(ObjectKey, crate::simulation::gauges::GaugeSnapshot)>,
        }
        let envelope = Envelope::deserialize(deserializer)?;
        let mut result = Self {
            vehicles: unique(envelope.vehicles)?,
            wheel_slots: unique(envelope.wheel_slots)?,
            engines: unique(envelope.engines)?,
            radiators: unique(envelope.radiators)?,
            bus_doors: unique(envelope.bus_doors)?,
            gauges: unique(envelope.gauges)?,
            ..Self::default()
        };
        for (key, change) in envelope.records {
            if result.identities.contains_key(&key) {
                return Err(serde::de::Error::custom("Duplicate world change identity"));
            }
            result.insert(key, change);
        }
        Ok(result)
    }
}
fn region(p: [f64; 3]) -> (i64, i64) {
    ((p[0] / 256.).floor() as i64, (p[2] / 256.).floor() as i64)
}
impl ChangeRecords {
    /// Generated object records and every object state.
    pub fn generated_records(&self) -> Self {
        let mut result = Self {
            vehicles: self.vehicles.clone(),
            wheel_slots: self.wheel_slots.clone(),
            engines: self.engines.clone(),
            radiators: self.radiators.clone(),
            bus_doors: self.bus_doors.clone(),
            gauges: self.gauges.clone(),
            ..Self::default()
        };
        for (key, change) in self.iter() {
            if matches!(key, ObjectKey::Generated(_)) {
                result.insert(key.clone(), change.clone());
            }
        }
        result
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn has_attachment_dependents(&self, key: &ObjectKey) -> bool {
        self.attachment_owners.contains_key(key)
    }
    fn index_attachment_owner(&mut self, owner: &ObjectKey) {
        if let Some(count) = self.attachment_owners.get_mut(owner) {
            *count += 1;
        } else {
            self.attachment_owners.insert(owner.clone(), 1);
            self.attachment_owner_bytes += key_heap(owner);
        }
    }
    fn unindex_attachment_owner(&mut self, owner: &ObjectKey) {
        let count = self.attachment_owners.get_mut(owner).unwrap();
        *count -= 1;
        if *count == 0 {
            self.attachment_owners.remove(owner);
            self.attachment_owner_bytes =
                self.attachment_owner_bytes.saturating_sub(key_heap(owner));
        }
    }
    pub fn record(&mut self, key: ObjectKey, current: ObjectSnapshot, baseline: &ObjectSnapshot) {
        // An attachment owner keeps its record: its dependents can unload with it.
        if &current == baseline && !self.has_attachment_dependents(&key) {
            self.forget(&key);
            return;
        }
        if self
            .get(&key)
            .is_some_and(|c| c.snapshot.as_ref() == Some(&current))
        {
            return;
        }
        if let Some(&index) = self.identities.get(&key) {
            let old_owner = self.records[index]
                .1
                .snapshot
                .as_ref()
                .and_then(attachment_owner);
            let new_owner = attachment_owner(&current);
            if old_owner != new_owner {
                let old_owner = old_owner.cloned();
                let new_owner = new_owner.cloned();
                if let Some(owner) = old_owner.as_ref() {
                    self.unindex_attachment_owner(owner);
                }
                if let Some(owner) = new_owner.as_ref() {
                    self.index_attachment_owner(owner);
                }
            }
            let old_region = self.records[index]
                .1
                .snapshot
                .as_ref()
                .map(|s| region(s.pose.position));
            let new_region = region(current.pose.position);
            if old_region != Some(new_region) {
                self.unindex(index);
                self.index(new_region, index);
            }
            self.payload_bytes = self.payload_bytes.saturating_sub(
                self.records[index]
                    .1
                    .snapshot
                    .as_ref()
                    .map_or(0, ObjectSnapshot::heap_bytes),
            );
            self.payload_bytes += current.heap_bytes();
            self.records[index].1.snapshot = Some(current);
            self.revision = self.revision.wrapping_add(1);
        } else {
            self.insert(
                key,
                ObjectChange {
                    snapshot: Some(current),
                    replay: None,
                    instance: None,
                },
            );
        }
    }

    pub fn retain_dependency(&mut self, key: ObjectKey, snapshot: ObjectSnapshot) {
        if self.get(&key).is_none() {
            self.insert(
                key,
                ObjectChange {
                    snapshot: Some(snapshot),
                    replay: None,
                    instance: None,
                },
            );
        }
    }
    pub fn set_replay(
        &mut self,
        key: &ObjectKey,
        placement: &Placement,
        instance: &super::GeneratedId,
    ) {
        if let Some(index) = self.identities.get(key) {
            if self.records[*index].1.replay.is_none() {
                self.records[*index].1.replay = Some(placement.clone());
                self.records[*index].1.instance = Some(instance.clone());
                self.payload_bytes += super::contents::placement_heap(
                    self.records[*index].1.replay.as_ref().unwrap(),
                ) + super::contents::identity_heap(
                    self.records[*index].1.instance.as_ref().unwrap(),
                );
                self.revision = self.revision.wrapping_add(1);
            }
        }
    }
    pub fn remove(&mut self, key: ObjectKey) {
        let replay = self.get(&key).and_then(|c| c.replay.clone());
        let instance = self.get(&key).and_then(|c| c.instance.clone());
        self.insert(
            key,
            ObjectChange {
                snapshot: None,
                replay,
                instance,
            },
        );
    }
    fn unindex(&mut self, index: usize) {
        if let Some(snapshot) = &self.records[index].1.snapshot {
            let cell = region(snapshot.pose.position);
            if let Some(entries) = self.regions.get_mut(&cell) {
                entries.retain(|i| *i != index);
                if entries.is_empty() {
                    self.region_bytes = self
                        .region_bytes
                        .saturating_sub(entries.capacity() * std::mem::size_of::<usize>());
                    self.regions.remove(&cell);
                }
            }
        }
    }
    fn index(&mut self, cell: (i64, i64), index: usize) {
        let entries = self.regions.entry(cell).or_default();
        let before = entries.capacity();
        entries.push(index);
        self.region_bytes += (entries.capacity() - before) * std::mem::size_of::<usize>();
    }
    fn insert(&mut self, key: ObjectKey, change: ObjectChange) {
        let index = if let Some(&index) = self.identities.get(&key) {
            let old_owner = self.records[index]
                .1
                .snapshot
                .as_ref()
                .and_then(attachment_owner);
            let new_owner = change.snapshot.as_ref().and_then(attachment_owner);
            if old_owner != new_owner {
                let old_owner = old_owner.cloned();
                let new_owner = new_owner.cloned();
                if let Some(owner) = old_owner.as_ref() {
                    self.unindex_attachment_owner(owner);
                }
                if let Some(owner) = new_owner.as_ref() {
                    self.index_attachment_owner(owner);
                }
            }
            self.unindex(index);
            self.payload_bytes = self
                .payload_bytes
                .saturating_sub(change_heap(&self.records[index].0, &self.records[index].1));
            self.payload_bytes += change_heap(&self.records[index].0, &change);
            self.records[index].1 = change;
            index
        } else {
            if let Some(owner) = change.snapshot.as_ref().and_then(attachment_owner) {
                self.index_attachment_owner(owner);
            }
            let index = self.records.len();
            self.identities.insert(key.clone(), index);
            self.payload_bytes += change_heap(&key, &change);
            self.records.push((key, change));
            index
        };
        if let Some(snapshot) = &self.records[index].1.snapshot {
            self.index(region(snapshot.pose.position), index);
        }
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn forget(&mut self, key: &ObjectKey) -> bool {
        let Some(index) = self.identities.remove(key) else {
            return false;
        };
        if let Some(owner) = self.records[index]
            .1
            .snapshot
            .as_ref()
            .and_then(attachment_owner)
            .cloned()
        {
            self.unindex_attachment_owner(&owner);
        }
        self.unindex(index);
        let last = self.records.len() - 1;
        if index != last {
            self.unindex(last);
        }
        self.payload_bytes = self
            .payload_bytes
            .saturating_sub(change_heap(&self.records[index].0, &self.records[index].1));
        self.records.swap_remove(index);
        if index != last {
            self.identities.insert(self.records[index].0.clone(), index);
            if let Some(snapshot) = &self.records[index].1.snapshot {
                self.index(region(snapshot.pose.position), index);
            }
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn get(&self, key: impl std::borrow::Borrow<ObjectKey>) -> Option<&ObjectChange> {
        self.identities
            .get(key.borrow())
            .map(|i| &self.records[*i].1)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&ObjectKey, &ObjectChange)> {
        self.records.iter().map(|(key, change)| (key, change))
    }
    pub fn query_reservation(&self, bounds: WorldBounds) -> (usize, usize) {
        let mut count = 0;
        let mut bytes = 0;
        let min = region(bounds.min);
        let max = region(bounds.max);
        for (_, entries) in self.regions.range((min.0, i64::MIN)..=(max.0, i64::MAX)) {
            for &index in entries {
                let (key, record) = &self.records[index];
                if record.snapshot.as_ref().is_some_and(|s| {
                    (0..3).all(|i| {
                        s.pose.position[i] >= bounds.min[i] && s.pose.position[i] <= bounds.max[i]
                    })
                }) {
                    count += 1;
                    bytes += std::mem::size_of::<ObjectKey>()
                        + key_heap(key)
                        + record.replay.as_ref().map_or(0, |p| {
                            std::mem::size_of::<Placement>() + super::contents::placement_heap(p)
                        });
                }
            }
        }
        (count, bytes)
    }
    pub fn get_indexed(&self, index: ChangeRecordIndex) -> (&ObjectKey, &ObjectChange) {
        let (key, change) = &self.records[index.0];
        (key, change)
    }
    pub fn indices_in(&self, bounds: WorldBounds, output: &mut Vec<ChangeRecordIndex>) {
        output.clear();
        let min = region(bounds.min);
        let max = region(bounds.max);
        for (_, indices) in self.regions.range((min.0, i64::MIN)..=(max.0, i64::MAX)) {
            for &index in indices {
                let (_, change) = &self.records[index];
                if change.snapshot.as_ref().is_some_and(|s| {
                    (0..3).all(|i| {
                        s.pose.position[i] >= bounds.min[i] && s.pose.position[i] <= bounds.max[i]
                    })
                }) {
                    output.push(ChangeRecordIndex(index));
                }
            }
        }
    }
    // Test oracle: the previous key query stays independent of indexed access.
    #[cfg(test)]
    pub(super) fn old_keys_in(&self, bounds: WorldBounds, output: &mut Vec<ObjectKey>) {
        output.clear();
        let min = region(bounds.min);
        let max = region(bounds.max);
        for (_, indices) in self.regions.range((min.0, i64::MIN)..=(max.0, i64::MAX)) {
            for &index in indices {
                let (key, change) = &self.records[index];
                if change.snapshot.as_ref().is_some_and(|s| {
                    (0..3).all(|i| {
                        s.pose.position[i] >= bounds.min[i] && s.pose.position[i] <= bounds.max[i]
                    })
                }) {
                    output.push(key.clone());
                }
            }
        }
    }
    /// Keeps the car-side state of car `key` until it spawns again.
    pub fn keep_vehicle(&mut self, key: ObjectKey, state: crate::starting_area::VehicleState) {
        self.vehicles.insert(key, state);
    }
    pub fn take_vehicle(&mut self, key: &ObjectKey) -> Option<crate::starting_area::VehicleState> {
        self.vehicles.remove(key)
    }
    pub fn vehicle(&self, key: &ObjectKey) -> Option<&crate::starting_area::VehicleState> {
        self.vehicles.get(key)
    }
    pub fn vehicles(
        &self,
    ) -> impl Iterator<Item = (&ObjectKey, &crate::starting_area::VehicleState)> {
        self.vehicles.iter()
    }
    /// Keeps the rim object of each wheel slot of car `key` until it spawns again.
    pub fn keep_wheel_slots(&mut self, key: ObjectKey, rims: Vec<Option<ObjectKey>>) {
        self.wheel_slots.insert(key, rims);
    }
    pub fn take_wheel_slots(&mut self, key: &ObjectKey) -> Option<Vec<Option<ObjectKey>>> {
        self.wheel_slots.remove(key)
    }
    /// Keeps the state of engine part `key` until it spawns again.
    pub fn keep_engine(&mut self, key: ObjectKey, state: crate::save::EngineSnapshot) {
        self.engines.insert(key, state);
    }
    pub fn take_engine(&mut self, key: &ObjectKey) -> Option<crate::save::EngineSnapshot> {
        self.engines.remove(key)
    }
    pub fn engine(&self, key: &ObjectKey) -> Option<&crate::save::EngineSnapshot> {
        self.engines.get(key)
    }
    pub fn engines(&self) -> impl Iterator<Item = (&ObjectKey, &crate::save::EngineSnapshot)> {
        self.engines.iter()
    }
    /// Keeps the fan state of radiator part `key` until it spawns again.
    pub fn keep_radiator(&mut self, key: ObjectKey, fan_on: bool) {
        self.radiators.insert(key, fan_on);
    }
    pub fn take_radiator(&mut self, key: &ObjectKey) -> Option<bool> {
        self.radiators.remove(key)
    }
    pub fn radiator(&self, key: &ObjectKey) -> Option<bool> {
        self.radiators.get(key).copied()
    }
    /// Sets the door state of bus `key`. It stays while the bus is retired.
    pub fn set_bus_doors(&mut self, key: ObjectKey, doors: Vec<[bool; 2]>) {
        self.bus_doors.insert(key, doors);
    }
    pub fn bus_doors(&self, key: &ObjectKey) -> Option<&[[bool; 2]]> {
        self.bus_doors.get(key).map(Vec::as_slice)
    }
    /// Keeps the meter values of gauge part `key` until it spawns again.
    pub fn keep_gauge(&mut self, key: ObjectKey, state: crate::simulation::gauges::GaugeSnapshot) {
        self.gauges.insert(key, state);
    }
    pub fn take_gauge(
        &mut self,
        key: &ObjectKey,
    ) -> Option<crate::simulation::gauges::GaugeSnapshot> {
        self.gauges.remove(key)
    }
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + object_states_bytes(&self.vehicles)
            + object_states_bytes(&self.wheel_slots)
            + self
                .wheel_slots
                .values()
                .map(|rims| {
                    rims.capacity() * std::mem::size_of::<Option<ObjectKey>>()
                        + rims.iter().flatten().map(key_heap).sum::<usize>()
                })
                .sum::<usize>()
            + object_states_bytes(&self.engines)
            + self
                .engines
                .values()
                .map(|e| e.start_draw.capacity())
                .sum::<usize>()
            + object_states_bytes(&self.radiators)
            + object_states_bytes(&self.bus_doors)
            + self
                .bus_doors
                .values()
                .map(|d| d.capacity() * std::mem::size_of::<[bool; 2]>())
                .sum::<usize>()
            + object_states_bytes(&self.gauges)
            + self
                .gauges
                .values()
                .map(|g| (g.values.capacity() + g.counters.capacity()) * std::mem::size_of::<f32>())
                .sum::<usize>()
            + self.records.capacity() * std::mem::size_of::<(ObjectKey, ObjectChange)>()
            + self.identities.capacity().saturating_mul(2)
                * (std::mem::size_of::<(ObjectKey, usize)>() + 1)
            + 16
            + self.attachment_owners.capacity().saturating_mul(2)
                * (std::mem::size_of::<(ObjectKey, usize)>() + 1)
            + 16
            + self.attachment_owner_bytes
            + self.payload_bytes
            + self.region_bytes
            + (self.regions.len() + 1)
                * (11 * std::mem::size_of::<((i64, i64), Vec<usize>)>()
                    + 12 * std::mem::size_of::<usize>()
                    + 64)
    }
}
/// B-tree nodes and key heap of an object state map.
fn object_states_bytes<V>(map: &BTreeMap<ObjectKey, V>) -> usize {
    map.len() * (std::mem::size_of::<(ObjectKey, V)>() + 16)
        + map.keys().map(key_heap).sum::<usize>()
}
fn attachment_owner(snapshot: &ObjectSnapshot) -> Option<&ObjectKey> {
    snapshot.attachment.as_ref()?.owner.as_ref()
}
pub fn key_heap(key: &ObjectKey) -> usize {
    match key {
        ObjectKey::Imported(_) => 0,
        ObjectKey::Generated(id) => super::contents::identity_heap(id),
    }
}

/// Capture active state before teardown. Owner identities never contain runtime handles.
pub fn capture(
    id: crate::reference::ItemId,
    items: &crate::interaction::items::ItemStore,
    interaction: &crate::interaction::InteractionState,
    physics: &crate::vehicle::PhysicsWorld,
    reference: &crate::reference::ReferenceManifest,
) -> Option<ObjectSnapshot> {
    use crate::interaction::items::ItemLocation;
    use crate::vehicle::{from_rapier, to_rapier};
    // A departing item keeps its locked record until it unloads.
    if interaction.storage.departing(id) {
        return None;
    }
    let state = items.get(id)?;
    let mut snapshot = items.generated_snapshot(id, interaction.generated_valve(id))?;
    snapshot.mechanism = interaction.mechanisms.snapshot(id);
    snapshot.conditions = interaction.conditions.item_surfaces(id);
    snapshot.gun = interaction.guns.snapshot(id);
    snapshot.food = state.food.map(|food| FoodSnapshot {
        source: reference.survival.foods[food.prototype_index]
            .source
            .clone(),
        bite: food.bite,
    });
    snapshot.held = state.location == ItemLocation::Held;
    if let Some(body) = state.body.and_then(|b| physics.rapier().bodies.get(b)) {
        snapshot.linear_velocity = body.linvel().to_array();
        snapshot.angular_velocity = body.angvel().to_array();
        snapshot.sleeping = body.is_sleeping();
    }
    if let ItemLocation::Locked { .. } = state.location {
        snapshot.attachment = Some(locked_attachment(id, items, &interaction.storage)?);
        return Some(snapshot);
    }
    // A hitched grip saves its target as the slot of its car.
    if let Some((owner, source)) = interaction.hitch.hitched_target(id) {
        let parent = items.get(owner)?;
        let pose = from_rapier(&(to_rapier(parent.pose).inverse() * to_rapier(state.pose)));
        snapshot.attachment = Some(AttachmentSnapshot {
            owner: Some(items.object_key(owner)),
            slot: Some(source.clone()),
            volume: None,
            local_pose: AbsolutePose {
                position: pose.position.map(f64::from),
                rotation: pose.rotation,
            },
        });
        return Some(snapshot);
    }
    let relation = match state.location {
        ItemLocation::Attached(parent) => Some((Some(parent), None)),
        ItemLocation::PartSlot(slot) => interaction
            .parts
            .slot_identity(slot)
            .map(|(owner, source)| (owner, Some(source.clone()))),
        ItemLocation::WheelSlot(slot) => interaction
            .wheel_slot_identity(slot)
            .map(|(owner, source)| (owner, Some(source.clone()))),
        _ => None,
    };
    if let Some((parent, slot)) = relation {
        let pose = if let Some(parent) = parent.and_then(|p| items.get(p)) {
            from_rapier(&(to_rapier(parent.pose).inverse() * to_rapier(state.pose)))
        } else {
            state.local_pose
        };
        snapshot.attachment = Some(AttachmentSnapshot {
            owner: parent.map(|p| items.object_key(p)),
            slot,
            volume: None,
            local_pose: AbsolutePose {
                position: pose.position.map(f64::from),
                rotation: pose.rotation,
            },
        });
    }
    Some(snapshot)
}

/// A locked item attaches to its owner with the volume source and the pose to the owner.
pub fn locked_attachment(
    id: crate::reference::ItemId,
    items: &crate::interaction::items::ItemStore,
    storage: &crate::interaction::storage::StorageLocks,
) -> Option<AttachmentSnapshot> {
    let crate::interaction::items::ItemLocation::Locked { owner, volume } = items.get(id)?.location
    else {
        return None;
    };
    let pose = items.saved_local_pose(id, storage).ok()?;
    Some(AttachmentSnapshot {
        owner: Some(items.object_key(owner)),
        slot: None,
        volume: Some(storage.volume_source(volume)?.clone()),
        local_pose: AbsolutePose {
            position: pose.position.map(f64::from),
            rotation: pose.rotation,
        },
    })
}

/// Source `Lock(_ignoreTime: true)`: locks a saved item at once into the bound volume of `owner`.
/// No drop delay and no two-scan wait. The owner must be in place first.
pub fn restore_lock(
    id: crate::reference::ItemId,
    owner: crate::reference::ItemId,
    attachment: &AttachmentSnapshot,
    items: &mut crate::interaction::items::ItemStore,
    storage: &mut crate::interaction::storage::StorageLocks,
    physics: &mut crate::vehicle::PhysicsWorld,
) -> Result<(), crate::reference::ReferenceError> {
    use crate::reference::ReferenceError;
    use crate::vehicle::{from_rapier, to_rapier};
    use rapier3d::math::{Pose, Rotation, Vector};
    let slot = attachment
        .volume
        .as_ref()
        .and_then(|volume| storage.owner_volume(owner, volume))
        .ok_or_else(|| ReferenceError("Saved storage volume is not bound to its owner".into()))?;
    if storage.stored(id) {
        storage.unlock(id, items, physics)?;
    }
    let local = Pose::from_parts(
        Vector::from_array(attachment.local_pose.position.map(|v| v as f32)),
        Rotation::from_array(attachment.local_pose.rotation),
    );
    storage.lock_saved(id, slot, local, items, physics)?;
    let state = items.get(id).unwrap();
    let carrier = state.body.unwrap();
    let pose = *physics.rapier().bodies[carrier].position() * to_rapier(state.local_pose);
    items.get_mut(id).unwrap().pose = from_rapier(&pose);
    physics.refresh_queries();
    Ok(())
}

pub fn restore_motion(
    id: crate::reference::ItemId,
    snapshot: &ObjectSnapshot,
    items: &crate::interaction::items::ItemStore,
    physics: &mut crate::vehicle::PhysicsWorld,
) {
    if let Some(item) = items.get(id) {
        if let Some(handle) = item.body {
            physics.try_edit_body(handle, |body| {
                body.set_position(
                    crate::vehicle::to_rapier(item.pose)
                        * crate::vehicle::to_rapier(item.local_pose).inverse(),
                    false,
                );
                body.set_linvel(snapshot.linear_velocity.into(), false);
                body.set_angvel(snapshot.angular_velocity.into(), false);
                if snapshot.sleeping {
                    body.sleep();
                } else {
                    body.wake_up(true);
                }
            });
        }
    }
}

impl ObjectSnapshot {
    pub fn heap_bytes(&self) -> usize {
        self.food.as_ref().map_or(0, |f| f.source.file.capacity())
            + self
                .conditions
                .iter()
                .map(|c| {
                    size_of::<crate::save::SurfaceSnapshot>()
                        + c.source.file.capacity()
                        + c.source.path_id.capacity()
                })
                .sum::<usize>()
            + self.attachment.as_ref().map_or(0, |a| {
                a.owner.as_ref().map_or(0, key_heap)
                    + a.slot.as_ref().map_or(0, |s| s.file.capacity())
                    + a.volume.as_ref().map_or(0, |s| s.file.capacity())
            })
    }
}

fn change_heap(key: &ObjectKey, change: &ObjectChange) -> usize {
    key_heap(key) * 2
        + change
            .replay
            .as_ref()
            .map_or(0, super::contents::placement_heap)
        + change
            .instance
            .as_ref()
            .map_or(0, super::contents::identity_heap)
        + change
            .snapshot
            .as_ref()
            .map_or(0, ObjectSnapshot::heap_bytes)
}
