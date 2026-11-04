//! Source caller boundaries survive unloading. Generated content does not.
use super::{GeneratedId, UnityRandom, UnityRandomState, WorldLayer};
use crate::reference::{ReferenceError, SourceId};
use bevy_platform::collections::HashTable;
use bevy_platform::hash::FixedHasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};
use std::sync::Arc;

/// Inputs that can change source selection, captured at each first invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpawnContext {
    pub current_car: i32,
    pub peaceful_enemies: i32,
    pub chance_bits: Option<u32>,
}
/// A `GeneratedId` whose sources index `SpawnHistory::sources`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnParent {
    pub layer: WorldLayer,
    pub generator: u32,
    pub unit: [i64; 2],
    pub ordinal: u32,
    pub children: Vec<(u32, u32)>,
}
/// One first invocation. `caller` indexes `sources`. `context` and `pools` index their tables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnBoundary {
    pub parent: SpawnParent,
    pub caller: u32,
    pub ordinal: u32,
    pub phase: SpawnPhase,
    pub random: UnityRandomState,
    pub context: u32,
    pub pools: u32,
    /// None when the inputs are empty.
    pub initialization: Option<SpawnInitialization>,
}
/// Mutable component inputs needed by a yielded slot continuation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnInitialization {
    pub paintings: Vec<(GeneratedId, SourceId, i32)>,
    pub scales: Vec<(GeneratedId, SourceId, [f32; 3])>,
    pub lamps: Vec<(GeneratedId, SourceId, bool)>,
    /// Source `Lamp.broken` set by flashlightscript.Break. Bit i is `Lamps[i]`.
    /// It is runtime state only. Spawn boundaries do not keep it.
    #[serde(skip)]
    pub broken_lamps: Vec<(GeneratedId, SourceId, u64)>,
    /// Live `electronicsscript.jamValue` of a component. It is runtime state only, as in the source.
    #[serde(skip)]
    pub jams: Vec<(GeneratedId, SourceId, f32)>,
    pub conditions: Vec<super::spawn_conditions::ConditionState>,
    pub initialized: Vec<(GeneratedId, SourceId)>,
    pub selectors: Vec<(GeneratedId, SourceId, usize)>,
    pub activations: Vec<(GeneratedId, SourceId, bool)>,
    pub slot_parts: Vec<(GeneratedId, SourceId, SourceId)>,
    /// Source `busdooropenscript.speed` drawn in Start.
    pub door_speeds: Vec<(GeneratedId, SourceId, f32)>,
    /// Source `newAiScript.Start` rolls of each human monster placement.
    pub humans: Vec<(GeneratedId, HumanRoll)>,
}
/// Source `newAiScript.Start`: `sleeping`, `voicePitch` and `NewPatrolTime`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanRoll {
    pub sleeping: bool,
    pub voice_pitch: f32,
    pub patrol_time: f32,
}
impl SpawnInitialization {
    pub fn for_parent(&self, parent: &GeneratedId) -> Self {
        Self {
            scales: self
                .scales
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            lamps: self
                .lamps
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            broken_lamps: self
                .broken_lamps
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            jams: self
                .jams
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            initialized: self
                .initialized
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            selectors: self
                .selectors
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            activations: self
                .activations
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            slot_parts: self
                .slot_parts
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            paintings: self
                .paintings
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            door_speeds: self
                .door_speeds
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            humans: self
                .humans
                .iter()
                .filter(|v| &v.0 == parent)
                .cloned()
                .collect(),
            conditions: self
                .conditions
                .iter()
                .filter(|v| &v.parent == parent)
                .cloned()
                .collect(),
        }
    }

    /// Source flashlightscript.Break. Sets the lamps in `lamps` broken. True when one changed.
    pub fn break_lamps(&mut self, parent: &GeneratedId, source: &SourceId, lamps: u64) -> bool {
        match self
            .broken_lamps
            .iter_mut()
            .find(|(p, s, _)| p == parent && s == source)
        {
            Some((_, _, broken)) => {
                let old = *broken;
                *broken |= lamps;
                *broken != old
            }
            None => {
                self.broken_lamps
                    .push((parent.clone(), source.clone(), lamps));
                true
            }
        }
    }
    /// The live jam of electronics `component` on `parent`. None: its serialized `jamValue`.
    pub fn jam(&self, parent: &GeneratedId, component: &SourceId) -> Option<f32> {
        self.jams
            .iter()
            .find(|(p, s, _)| p == parent && s == component)
            .map(|v| v.2)
    }
    /// Sets the live jam of electronics `component` on `parent`.
    pub fn set_jam(&mut self, parent: &GeneratedId, component: &SourceId, jam: f32) {
        match self
            .jams
            .iter_mut()
            .find(|(p, s, _)| p == parent && s == component)
        {
            Some(v) => v.2 = jam,
            None => self.jams.push((parent.clone(), component.clone(), jam)),
        }
    }
    /// True when flashlightscript.Break set `Lamps[index]` broken.
    pub fn lamp_broken(&self, parent: &GeneratedId, source: &SourceId, index: usize) -> bool {
        self.broken_lamps
            .iter()
            .any(|(p, s, bits)| p == parent && s == source && bits & (1 << index) != 0)
    }

    pub fn retained_bytes(&self) -> usize {
        self.paintings.capacity() * std::mem::size_of::<(GeneratedId, SourceId, i32)>()
            + self
                .paintings
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self.scales.capacity() * std::mem::size_of::<(GeneratedId, SourceId, [f32; 3])>()
            + self
                .scales
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self.lamps.capacity() * std::mem::size_of::<(GeneratedId, SourceId, bool)>()
            + self
                .lamps
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self.jams.capacity() * std::mem::size_of::<(GeneratedId, SourceId, f32)>()
            + self
                .jams
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self.broken_lamps.capacity() * std::mem::size_of::<(GeneratedId, SourceId, u64)>()
            + self
                .broken_lamps
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self.conditions.capacity()
                * std::mem::size_of::<super::spawn_conditions::ConditionState>()
            + self
                .conditions
                .iter()
                .map(super::spawn_conditions::ConditionState::retained_heap)
                .sum::<usize>()
            + self.initialized.capacity() * std::mem::size_of::<(GeneratedId, SourceId)>()
            + self.selectors.capacity() * std::mem::size_of::<(GeneratedId, SourceId, usize)>()
            + self.activations.capacity() * std::mem::size_of::<(GeneratedId, SourceId, bool)>()
            + self.slot_parts.capacity() * std::mem::size_of::<(GeneratedId, SourceId, SourceId)>()
            + self
                .initialized
                .iter()
                .map(|(p, s)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self
                .selectors
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self
                .activations
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self
                .slot_parts
                .iter()
                .map(|(p, s, t)| identity_bytes(p) + s.file.capacity() + t.file.capacity())
                .sum::<usize>()
            + self.door_speeds.capacity() * std::mem::size_of::<(GeneratedId, SourceId, f32)>()
            + self
                .door_speeds
                .iter()
                .map(|(p, s, _)| identity_bytes(p) + s.file.capacity())
                .sum::<usize>()
            + self.humans.capacity() * std::mem::size_of::<(GeneratedId, HumanRoll)>()
            + self
                .humans
                .iter()
                .map(|(p, _)| identity_bytes(p))
                .sum::<usize>()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaintingPool {
    pub source: SourceId,
    pub available: Vec<i32>,
}
fn pools_bytes(pools: &Vec<PaintingPool>) -> usize {
    pools.capacity() * std::mem::size_of::<PaintingPool>()
        + pools
            .iter()
            .map(|p| p.source.file.capacity() + p.available.capacity() * std::mem::size_of::<i32>())
            .sum::<usize>()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnPhase {
    Synchronous,
    ComponentStart,
    SynchronousAndStarts,
}

/// The supplied controlled startup state does not describe untouched game startup.
/// Boundaries refer to the tables by index, so the tables store each source, context and pool state once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnHistory {
    pub live: UnityRandomState,
    pub painting_pools: Vec<PaintingPool>,
    pub context: SpawnContext,
    pub sources: Vec<SourceId>,
    pub contexts: Vec<SpawnContext>,
    pub pools: Vec<Vec<PaintingPool>>,
    pub boundaries: Vec<SpawnBoundary>,
}
/// Values stored once and referred to by index.
struct Table<T: Eq + Hash> {
    values: Vec<Arc<T>>,
    index: HashMap<Arc<T>, u32>,
}
impl<T: Eq + Hash + Clone> Table<T> {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            index: HashMap::new(),
        }
    }
    fn find(&self, value: &T) -> Option<u32> {
        self.index.get(value).copied()
    }
    /// The index of `value`, and its new heap bytes when `value` is new.
    fn intern(&mut self, value: &T, bytes: impl FnOnce(&T) -> usize) -> (u32, usize) {
        if let Some(i) = self.find(value) {
            return (i, 0);
        }
        let i = self.values.len() as u32;
        let value = Arc::new(value.clone());
        self.values.push(value.clone());
        self.index.insert(value.clone(), i);
        (
            i,
            2 * std::mem::size_of::<usize>() + std::mem::size_of::<T>() + bytes(&value),
        )
    }
    fn capacity_bytes(&self) -> usize {
        self.values.capacity() * std::mem::size_of::<Arc<T>>()
            + self.index.capacity() * (std::mem::size_of::<(Arc<T>, u32)>() + 1)
    }
    fn capture(&self) -> Vec<T> {
        self.values.iter().map(|v| T::clone(v)).collect()
    }
}
/// Fields that identify a boundary. Sources and children are table indices.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    layer: WorldLayer,
    generator: u32,
    unit: [i64; 2],
    parent_ordinal: u32,
    children: u32,
    caller: u32,
    ordinal: u32,
    phase: SpawnPhase,
}
/// One boundary in memory: about 90 bytes without inputs.
struct Record {
    key: Key,
    random: UnityRandomState,
    context: u32,
    pools: u32,
    /// Postcard bytes of the inputs and their broken lamps. None when the inputs are empty.
    initialization: Option<Box<[u8]>>,
}
fn pack(initialization: &SpawnInitialization) -> Box<[u8]> {
    postcard::to_stdvec(&(initialization, &initialization.broken_lamps))
        .expect("spawn inputs encode")
        .into_boxed_slice()
}
fn unpack(bytes: &[u8]) -> SpawnInitialization {
    let (mut initialization, broken): (SpawnInitialization, _) =
        postcard::from_bytes(bytes).expect("spawn inputs decode");
    initialization.broken_lamps = broken;
    initialization
}
pub struct SourceSpawnState {
    live: UnityRandom,
    painting_pools: Vec<PaintingPool>,
    sources: Table<SourceId>,
    /// Child paths of parent identities. Index 0 is the empty path.
    children: Table<Vec<(u32, u32)>>,
    contexts: Table<SpawnContext>,
    /// Pool states in first-use order. Equal consecutive states share one entry.
    pools: Vec<Arc<Vec<PaintingPool>>>,
    records: Vec<Record>,
    /// Record indices by key hash. Lookups stay constant time on long trips.
    index: HashTable<u32>,
    /// Heap of table values, pool states and packed inputs.
    heap_bytes: usize,
    pub context: SpawnContext,
}
impl SourceSpawnState {
    fn empty(live: UnityRandom, painting_pools: Vec<PaintingPool>, context: SpawnContext) -> Self {
        let mut children = Table::new();
        children.intern(&Vec::new(), |_| 0);
        Self {
            live,
            painting_pools,
            sources: Table::new(),
            children,
            contexts: Table::new(),
            pools: Vec::new(),
            records: Vec::new(),
            index: HashTable::new(),
            heap_bytes: 0,
            context,
        }
    }
    pub fn capture_history(&self) -> SpawnHistory {
        SpawnHistory {
            live: self.live.snapshot(),
            painting_pools: self.painting_pools.clone(),
            context: self.context,
            sources: self.sources.capture(),
            contexts: self.contexts.capture(),
            pools: self.pools.iter().map(|p| Vec::clone(p)).collect(),
            boundaries: self
                .records
                .iter()
                .map(|r| SpawnBoundary {
                    parent: SpawnParent {
                        layer: r.key.layer,
                        generator: r.key.generator,
                        unit: r.key.unit,
                        ordinal: r.key.parent_ordinal,
                        children: Vec::clone(&self.children.values[r.key.children as usize]),
                    },
                    caller: r.key.caller,
                    ordinal: r.key.ordinal,
                    phase: r.key.phase,
                    random: r.random,
                    context: r.context,
                    pools: r.pools,
                    initialization: r.initialization.as_deref().map(unpack),
                })
                .collect(),
        }
    }
    pub fn restore_history(history: SpawnHistory) -> Result<Self, ReferenceError> {
        let mut state = Self::empty(
            UnityRandom::restore(history.live),
            history.painting_pools,
            history.context,
        );
        let invalid = || ReferenceError("Invalid spawn boundary table index".into());
        let sources: Vec<u32> = history
            .sources
            .iter()
            .map(|s| state.intern_source(s))
            .collect();
        let contexts: Vec<u32> = history
            .contexts
            .iter()
            .map(|c| state.contexts.intern(c, |_| 0).0)
            .collect();
        for pools in &history.pools {
            state.heap_bytes += pool_state_bytes(pools);
        }
        state.pools = history.pools.into_iter().map(Arc::new).collect();
        let source = |i: u32| sources.get(i as usize).copied().ok_or_else(invalid);
        for boundary in history.boundaries {
            let children = boundary
                .parent
                .children
                .iter()
                .map(|&(s, n)| Ok((source(s)?, n)))
                .collect::<Result<Vec<_>, ReferenceError>>()?;
            let (children, bytes) = state.children.intern(&children, children_bytes);
            state.heap_bytes += bytes;
            let key = Key {
                layer: boundary.parent.layer,
                generator: source(boundary.parent.generator)?,
                unit: boundary.parent.unit,
                parent_ordinal: boundary.parent.ordinal,
                children,
                caller: source(boundary.caller)?,
                ordinal: boundary.ordinal,
                phase: boundary.phase,
            };
            if state.find_key(&key).is_some() {
                return Err(ReferenceError("Duplicate spawn boundary".into()));
            }
            if boundary.pools as usize >= state.pools.len() {
                return Err(invalid());
            }
            state.push(Record {
                key,
                random: boundary.random,
                context: *contexts
                    .get(boundary.context as usize)
                    .ok_or_else(invalid)?,
                pools: boundary.pools,
                initialization: boundary.initialization.as_ref().map(pack),
            });
        }
        Ok(state)
    }
    fn intern_source(&mut self, source: &SourceId) -> u32 {
        let (i, bytes) = self.sources.intern(source, |s| s.file.capacity());
        self.heap_bytes += bytes;
        i
    }
    /// The key of a boundary, or None when one of its sources or child paths is not stored.
    fn key(
        &self,
        parent: &GeneratedId,
        caller: &SourceId,
        ordinal: u32,
        phase: SpawnPhase,
    ) -> Option<Key> {
        let children = if parent.children.is_empty() {
            0
        } else {
            let path = parent
                .children
                .iter()
                .map(|(s, n)| Some((self.sources.find(s)?, *n)))
                .collect::<Option<Vec<_>>>()?;
            self.children.find(&path)?
        };
        Some(Key {
            layer: parent.layer,
            generator: self.sources.find(&parent.generator)?,
            unit: parent.unit,
            parent_ordinal: parent.ordinal,
            children,
            caller: self.sources.find(caller)?,
            ordinal,
            phase,
        })
    }
    fn find_key(&self, key: &Key) -> Option<&Record> {
        self.index
            .find(FixedHasher.hash_one(key), |&i| {
                self.records[i as usize].key == *key
            })
            .map(|&i| &self.records[i as usize])
    }
    pub fn has_boundary(
        &self,
        parent: &GeneratedId,
        caller: &SourceId,
        ordinal: u32,
        phase: SpawnPhase,
    ) -> bool {
        self.find(parent, caller, ordinal, phase).is_some()
    }
    fn find(
        &self,
        parent: &GeneratedId,
        caller: &SourceId,
        ordinal: u32,
        phase: SpawnPhase,
    ) -> Option<&Record> {
        self.find_key(&self.key(parent, caller, ordinal, phase)?)
    }
    fn push(&mut self, record: Record) {
        let records = &self.records;
        self.index.insert_unique(
            FixedHasher.hash_one(record.key),
            records.len() as u32,
            |&i| FixedHasher.hash_one(records[i as usize].key),
        );
        self.heap_bytes += record.initialization.as_ref().map_or(0, |i| i.len());
        self.records.push(record);
    }

    pub fn supplied_controlled_startup(
        reference: &crate::reference::WorldSpawnContext,
        context: SpawnContext,
    ) -> Self {
        Self::empty(
            UnityRandom::restore(UnityRandomState {
                words: reference.startup_state,
            }),
            Vec::new(),
            context,
        )
    }
    /// Constant time. A push adds the heap of its boundary to the sum.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + pools_bytes(&self.painting_pools)
            + self.sources.capacity_bytes()
            + self.children.capacity_bytes()
            + self.contexts.capacity_bytes()
            + self.pools.capacity() * std::mem::size_of::<Arc<Vec<PaintingPool>>>()
            + self.records.capacity() * std::mem::size_of::<Record>()
            + self.index.capacity() * (std::mem::size_of::<u32>() + 1)
            + self.heap_bytes
    }
    pub fn boundary_count(&self) -> usize {
        self.records.len()
    }
    pub fn live_state(&self) -> UnityRandomState {
        self.live.snapshot()
    }
    /// Reserve metadata before invoking an atomic source caller. Failed calls cannot consume live draws.
    pub fn invoke<T>(
        &mut self,
        parent: &GeneratedId,
        caller: &SourceId,
        ordinal: u32,
        phase: SpawnPhase,
        staging: usize,
        initialization: &SpawnInitialization,
        call: impl FnOnce(
            &mut UnityRandom,
            SpawnContext,
            &mut Vec<PaintingPool>,
            &SpawnInitialization,
            usize,
        ) -> Result<T, ReferenceError>,
    ) -> Result<T, ReferenceError> {
        if let Some(record) = self.find(parent, caller, ordinal, phase) {
            let pools = &self.pools[record.pools as usize];
            let overlap = pools_bytes(pools);
            if overlap > staging {
                return Err(ReferenceError(
                    "Spawn replay pool clone exceeds staging".into(),
                ));
            }
            let mut inputs = record
                .initialization
                .as_deref()
                .map(unpack)
                .unwrap_or_default();
            // Broken lamps are runtime state that the boundary does not keep. The live inputs
            // carry them on.
            if !initialization.broken_lamps.is_empty() {
                inputs.broken_lamps = initialization.broken_lamps.clone();
            }
            return call(
                &mut UnityRandom::restore(record.random),
                *self.contexts.values[record.context as usize],
                &mut Vec::clone(pools),
                &inputs,
                staging - overlap,
            );
        }
        let extra = std::mem::size_of::<Record>()
            + identity_bytes(parent)
            + caller.file.len()
            + pools_bytes(&self.painting_pools)
            + initialization.retained_bytes();
        // History growth is not frame staging. Here try_reserve reports a real allocation failure.
        let overlap = extra + pools_bytes(&self.painting_pools);
        if overlap > staging {
            return Err(ReferenceError(
                "Spawn boundary preparation exceeds staging".into(),
            ));
        }
        self.records
            .try_reserve_exact(1)
            .map_err(|_| ReferenceError("Source spawn replay metadata allocation failed".into()))?;
        let random = self.live.snapshot();
        let context = self.context;
        let captured = self.painting_pools.clone();
        let stored =
            (*initialization != SpawnInitialization::default()).then(|| pack(initialization));
        let mut live = self.live;
        let mut pools = self.painting_pools.clone();
        let value = call(
            &mut live,
            self.context,
            &mut pools,
            initialization,
            staging - overlap,
        )?;
        let generator = self.intern_source(&parent.generator);
        let path: Vec<_> = parent
            .children
            .iter()
            .map(|(s, n)| (self.intern_source(s), *n))
            .collect();
        let (children, bytes) = self.children.intern(&path, children_bytes);
        self.heap_bytes += bytes;
        let caller = self.intern_source(caller);
        let (context, _) = self.contexts.intern(&context, |_| 0);
        if self.pools.last().is_none_or(|last| **last != captured) {
            self.heap_bytes += pool_state_bytes(&captured);
            self.pools.push(Arc::new(captured));
        }
        self.push(Record {
            key: Key {
                layer: parent.layer,
                generator,
                unit: parent.unit,
                parent_ordinal: parent.ordinal,
                children,
                caller,
                ordinal,
                phase,
            },
            random,
            context,
            pools: self.pools.len() as u32 - 1,
            initialization: stored,
        });
        self.live = live;
        self.painting_pools = pools;
        Ok(value)
    }
}
fn pool_state_bytes(pools: &Vec<PaintingPool>) -> usize {
    2 * std::mem::size_of::<usize>() + std::mem::size_of::<Vec<PaintingPool>>() + pools_bytes(pools)
}
fn children_bytes(path: &Vec<(u32, u32)>) -> usize {
    path.capacity() * std::mem::size_of::<(u32, u32)>()
}
fn identity_bytes(id: &GeneratedId) -> usize {
    id.generator.file.capacity()
        + id.children.capacity() * std::mem::size_of::<(SourceId, u32)>()
        + id.children
            .iter()
            .map(|(s, _)| s.file.capacity())
            .sum::<usize>()
}
