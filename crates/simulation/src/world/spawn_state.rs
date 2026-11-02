//! Source caller boundaries survive unloading; generated content does not.
use super::{GeneratedId, UnityRandom, UnityRandomState};
use crate::reference::{ReferenceError, SourceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Inputs that can change source selection, captured at each first invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpawnContext {
    pub current_car: i32,
    pub peaceful_enemies: i32,
    pub date: [i32; 3],
    pub chance_bits: Option<u32>,
}
/// One first invocation. `parent`, `caller`, `context` and `pools` index the history tables.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnBoundary {
    pub parent: u32,
    pub caller: u32,
    pub ordinal: u32,
    pub phase: SpawnPhase,
    pub random: UnityRandomState,
    pub context: u32,
    pub pools: u32,
    /// None when the inputs are empty.
    pub initialization: Option<Box<SpawnInitialization>>,
}
/// Mutable component inputs needed by a yielded slot continuation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnInitialization {
    pub paintings: Vec<(GeneratedId, SourceId, i32)>,
    pub scales: Vec<(GeneratedId, SourceId, [f32; 3])>,
    pub lamps: Vec<(GeneratedId, SourceId, bool)>,
    /// Source `Lamp.broken` set by flashlightscript.Break. Bit i is `Lamps[i]`.
    /// Runtime state only: spawn boundaries do not keep it.
    #[serde(skip)]
    pub broken_lamps: Vec<(GeneratedId, SourceId, u64)>,
    pub conditions: Vec<super::spawn_conditions::ConditionState>,
    pub initialized: Vec<(GeneratedId, SourceId)>,
    pub selectors: Vec<(GeneratedId, SourceId, usize)>,
    pub activations: Vec<(GeneratedId, SourceId, bool)>,
    pub slot_parts: Vec<(GeneratedId, SourceId, SourceId)>,
    /// Source `busdooropenscript.speed` drawn in Start.
    pub door_speeds: Vec<(GeneratedId, SourceId, f32)>,
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
            conditions: self
                .conditions
                .iter()
                .filter(|v| &v.parent == parent)
                .cloned()
                .collect(),
        }
    }

    /// Source flashlightscript.Break: sets the lamps in `lamps` broken. True when one changed.
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
                .map(|c| c.retained_heap())
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
/// Boundaries refer to the tables by index, so each identity, context and pool state is stored once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnHistory {
    pub live: UnityRandomState,
    pub painting_pools: Vec<PaintingPool>,
    pub context: SpawnContext,
    pub parents: Vec<GeneratedId>,
    pub callers: Vec<SourceId>,
    pub contexts: Vec<SpawnContext>,
    pub pools: Vec<Vec<PaintingPool>>,
    pub boundaries: Vec<SpawnBoundary>,
}
/// Values stored once and referred to by index.
struct Table<T: Eq + std::hash::Hash> {
    values: Vec<Arc<T>>,
    index: HashMap<Arc<T>, u32>,
}
impl<T: Eq + std::hash::Hash> Table<T> {
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
    fn intern(&mut self, value: &T, bytes: impl FnOnce(&T) -> usize) -> (u32, usize)
    where
        T: Clone,
    {
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
    fn capture(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.values.iter().map(|v| T::clone(v)).collect()
    }
}
type BoundaryKey = (u32, u32, u32, SpawnPhase);
pub struct SourceSpawnState {
    live: UnityRandom,
    painting_pools: Vec<PaintingPool>,
    parents: Table<GeneratedId>,
    callers: Table<SourceId>,
    contexts: Table<SpawnContext>,
    /// Pool states in first-use order. Equal consecutive states share one entry.
    pools: Vec<Arc<Vec<PaintingPool>>>,
    boundaries: Vec<SpawnBoundary>,
    /// Boundary index by key. Lookups stay constant time on long trips.
    index: HashMap<BoundaryKey, u32>,
    /// Heap of table values, pool states and boundary inputs.
    heap_bytes: usize,
    pub context: SpawnContext,
}
impl SourceSpawnState {
    fn empty(live: UnityRandom, painting_pools: Vec<PaintingPool>, context: SpawnContext) -> Self {
        Self {
            live,
            painting_pools,
            parents: Table::new(),
            callers: Table::new(),
            contexts: Table::new(),
            pools: Vec::new(),
            boundaries: Vec::new(),
            index: HashMap::new(),
            heap_bytes: 0,
            context,
        }
    }
    pub fn capture_history(&self) -> SpawnHistory {
        SpawnHistory {
            live: self.live.snapshot(),
            painting_pools: self.painting_pools.clone(),
            context: self.context,
            parents: self.parents.capture(),
            callers: self.callers.capture(),
            contexts: self.contexts.capture(),
            pools: self.pools.iter().map(|p| Vec::clone(p)).collect(),
            boundaries: self.boundaries.clone(),
        }
    }
    pub fn restore_history(history: SpawnHistory) -> Result<Self, ReferenceError> {
        let mut state = Self::empty(
            UnityRandom::restore(history.live),
            history.painting_pools,
            history.context,
        );
        let invalid = || ReferenceError("Invalid spawn boundary table index".into());
        for boundary in history.boundaries {
            let parent = history
                .parents
                .get(boundary.parent as usize)
                .ok_or_else(invalid)?;
            let caller = history
                .callers
                .get(boundary.caller as usize)
                .ok_or_else(invalid)?;
            let context = *history
                .contexts
                .get(boundary.context as usize)
                .ok_or_else(invalid)?;
            let pools = history
                .pools
                .get(boundary.pools as usize)
                .ok_or_else(invalid)?;
            if state.has_boundary(parent, caller, boundary.ordinal, boundary.phase) {
                return Err(ReferenceError("Duplicate spawn boundary".into()));
            }
            state.push(
                parent,
                caller,
                boundary.ordinal,
                boundary.phase,
                boundary.random,
                context,
                pools,
                boundary.initialization,
            );
        }
        Ok(state)
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
    ) -> Option<&SpawnBoundary> {
        let key = (
            self.parents.find(parent)?,
            self.callers.find(caller)?,
            ordinal,
            phase,
        );
        self.index.get(&key).map(|&i| &self.boundaries[i as usize])
    }
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        parent: &GeneratedId,
        caller: &SourceId,
        ordinal: u32,
        phase: SpawnPhase,
        random: UnityRandomState,
        context: SpawnContext,
        pools: &Vec<PaintingPool>,
        initialization: Option<Box<SpawnInitialization>>,
    ) {
        let (parent, parent_bytes) = self.parents.intern(parent, identity_bytes);
        let (caller, caller_bytes) = self.callers.intern(caller, |s| s.file.capacity());
        let (context, _) = self.contexts.intern(&context, |_| 0);
        if self.pools.last().is_none_or(|last| **last != *pools) {
            self.heap_bytes += 2 * std::mem::size_of::<usize>()
                + std::mem::size_of::<Vec<PaintingPool>>()
                + pools_bytes(pools);
            self.pools.push(Arc::new(pools.clone()));
        }
        self.heap_bytes += parent_bytes
            + caller_bytes
            + initialization.as_ref().map_or(0, |i| {
                std::mem::size_of::<SpawnInitialization>() + i.retained_bytes()
            });
        self.index.insert(
            (parent, caller, ordinal, phase),
            self.boundaries.len() as u32,
        );
        self.boundaries.push(SpawnBoundary {
            parent,
            caller,
            ordinal,
            phase,
            random,
            context,
            pools: self.pools.len() as u32 - 1,
            initialization,
        });
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
    /// Constant time: per-boundary heap is summed when a boundary is pushed.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + pools_bytes(&self.painting_pools)
            + self.parents.capacity_bytes()
            + self.callers.capacity_bytes()
            + self.contexts.capacity_bytes()
            + self.pools.capacity() * std::mem::size_of::<Arc<Vec<PaintingPool>>>()
            + self.boundaries.capacity() * std::mem::size_of::<SpawnBoundary>()
            + self.index.capacity() * (std::mem::size_of::<(BoundaryKey, u32)>() + 1)
            + self.heap_bytes
    }
    pub fn parent_count(&self) -> usize {
        self.parents.values.len()
    }
    pub fn boundary_count(&self) -> usize {
        self.boundaries.len()
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
        if let Some(boundary) = self.find(parent, caller, ordinal, phase) {
            let pools = &self.pools[boundary.pools as usize];
            let overlap = pools_bytes(pools);
            if overlap > staging {
                return Err(ReferenceError(
                    "Spawn replay pool clone exceeds staging".into(),
                ));
            }
            // Broken lamps are runtime state that the boundary does not keep: the live inputs
            // carry them on.
            let empty = SpawnInitialization::default();
            let stored = boundary.initialization.as_deref().unwrap_or(&empty);
            let restored;
            let inputs = if initialization.broken_lamps.is_empty() {
                stored
            } else {
                restored = SpawnInitialization {
                    broken_lamps: initialization.broken_lamps.clone(),
                    ..stored.clone()
                };
                &restored
            };
            return call(
                &mut UnityRandom::restore(boundary.random),
                *self.contexts.values[boundary.context as usize],
                &mut Vec::clone(pools),
                inputs,
                staging - overlap,
            );
        }
        let extra = std::mem::size_of::<SpawnBoundary>()
            + identity_bytes(parent)
            + caller.file.len()
            + pools_bytes(&self.painting_pools)
            + initialization.retained_bytes();
        // History growth is not frame staging; try_reserve reports a real allocation failure.
        let overlap = extra + pools_bytes(&self.painting_pools);
        if overlap > staging {
            return Err(ReferenceError(
                "Spawn boundary preparation exceeds staging".into(),
            ));
        }
        self.boundaries
            .try_reserve_exact(1)
            .map_err(|_| ReferenceError("Source spawn replay metadata allocation failed".into()))?;
        let random = self.live.snapshot();
        let captured = (self.context, self.painting_pools.clone());
        let stored = (*initialization != SpawnInitialization::default())
            .then(|| Box::new(initialization.clone()));
        let mut live = self.live;
        let mut pools = self.painting_pools.clone();
        let value = call(
            &mut live,
            self.context,
            &mut pools,
            initialization,
            staging - overlap,
        )?;
        self.push(
            parent,
            caller,
            ordinal,
            phase,
            random,
            captured.0,
            &captured.1,
            stored,
        );
        self.live = live;
        self.painting_pools = pools;
        Ok(value)
    }
}
fn identity_bytes(id: &GeneratedId) -> usize {
    id.generator.file.capacity()
        + id.children.capacity() * std::mem::size_of::<(SourceId, u32)>()
        + id.children
            .iter()
            .map(|(s, _)| s.file.capacity())
            .sum::<usize>()
}

#[cfg(test)]
impl SourceSpawnState {
    /// Temporary diagnostic: retained and JSON bytes by part.
    pub fn breakdown(&self) -> String {
        let init: usize = self
            .boundaries
            .iter()
            .filter_map(|b| b.initialization.as_ref())
            .map(|i| std::mem::size_of::<SpawnInitialization>() + i.retained_bytes())
            .sum();
        let with_init = self.boundaries.iter().filter(|b| b.initialization.is_some()).count();
        let h = self.capture_history();
        let j = |v: String| v.len();
        format!(
            "boundaries {} parents {} callers {} contexts {} pools {}; retained {} (init {init} in {with_init}); json total {} parents {} init {} pools {}",
            self.boundaries.len(),
            self.parents.values.len(),
            self.callers.values.len(),
            self.contexts.values.len(),
            self.pools.len(),
            self.retained_bytes(),
            j(serde_json::to_string(&h).unwrap()),
            j(serde_json::to_string(&h.parents).unwrap()),
            j(serde_json::to_string(&h.boundaries.iter().map(|b| &b.initialization).collect::<Vec<_>>()).unwrap()),
            j(serde_json::to_string(&h.pools).unwrap()),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SourceId {
        SourceId {
            file: "level1".into(),
            path_id: 1,
        }
    }
    fn parent(ordinal: u32) -> GeneratedId {
        GeneratedId {
            layer: super::super::WorldLayer::Building,
            generator: source(),
            unit: [i64::from(ordinal / 3), 0],
            ordinal: ordinal / 3,
            children: Vec::new(),
        }
    }
    fn phase(ordinal: u32) -> SpawnPhase {
        [
            SpawnPhase::Synchronous,
            SpawnPhase::ComponentStart,
            SpawnPhase::SynchronousAndStarts,
        ][ordinal as usize % 3]
    }
    fn context() -> SpawnContext {
        SpawnContext {
            current_car: 0,
            peaceful_enemies: 1,
            date: [0; 3],
            chance_bits: None,
        }
    }
    /// A history with `count` boundaries, three per parent. Each boundary has its own pool state.
    fn history(count: u32, painting: usize) -> SpawnHistory {
        SpawnHistory {
            live: UnityRandomState { words: [1; 4] },
            painting_pools: Vec::new(),
            context: context(),
            parents: (0..count.div_ceil(3)).map(|i| parent(i * 3)).collect(),
            callers: vec![source()],
            contexts: vec![context()],
            pools: (0..count)
                .map(|i| {
                    vec![PaintingPool {
                        source: source(),
                        available: vec![i as i32; painting],
                    }]
                })
                .collect(),
            boundaries: (0..count)
                .map(|i| SpawnBoundary {
                    parent: i / 3,
                    caller: 0,
                    ordinal: i,
                    phase: phase(i),
                    random: UnityRandomState { words: [i; 4] },
                    context: 0,
                    pools: i,
                    initialization: None,
                })
                .collect(),
        }
    }

    #[test]
    fn boundary_index_matches_scan() {
        let history = history(1000, 1);
        let state = SourceSpawnState::restore_history(history.clone()).unwrap();
        assert_eq!(state.parent_count(), 334);
        for b in &history.boundaries {
            let found = state
                .find(&parent(b.ordinal), &source(), b.ordinal, b.phase)
                .unwrap();
            assert_eq!(found, b);
        }
        assert!(!state.has_boundary(&parent(0), &source(), 0, SpawnPhase::ComponentStart));
        assert_eq!(state.capture_history(), history);
        let mut duplicate = history.clone();
        duplicate.boundaries.push(duplicate.boundaries[7].clone());
        assert!(SourceSpawnState::restore_history(duplicate).is_err());
        let mut invalid = history;
        invalid.boundaries[5].pools = 1000;
        assert!(SourceSpawnState::restore_history(invalid).is_err());
    }

    #[test]
    fn large_spawn_history_restores() {
        // 3,000 boundaries with 8 KiB pools each is about 24 MiB, over the old 16 MiB cap.
        let state = SourceSpawnState::restore_history(history(3000, 2048)).unwrap();
        assert!(state.retained_bytes() > 16 * 1024 * 1024);
        assert_eq!(state.boundary_count(), 3000);
    }

    #[test]
    fn boundaries_share_tables() {
        let mut state = SourceSpawnState::restore_history(history(0, 0)).unwrap();
        let before = state.retained_bytes();
        let initialization = SpawnInitialization {
            initialized: vec![(parent(0), source())],
            ..Default::default()
        };
        for i in 0..3000 {
            let inputs = if i == 0 {
                initialization.clone()
            } else {
                SpawnInitialization::default()
            };
            state
                .invoke(
                    &parent(i),
                    &source(),
                    i,
                    phase(i),
                    usize::MAX / 2,
                    &inputs,
                    |r, _, _, _, _| {
                        r.value();
                        Ok(())
                    },
                )
                .unwrap();
        }
        let history = state.capture_history();
        assert_eq!(history.parents.len(), 1000);
        assert_eq!(
            (
                history.callers.len(),
                history.contexts.len(),
                history.pools.len()
            ),
            (1, 1, 1)
        );
        assert_eq!(
            history.boundaries[0].initialization.as_deref(),
            Some(&initialization)
        );
        assert!(history.boundaries[1].initialization.is_none());
        // About 100 bytes per boundary with one identity per three boundaries.
        assert!(
            state.retained_bytes() - before < 3000 * 150,
            "{}",
            state.retained_bytes() - before
        );
        let restored = SourceSpawnState::restore_history(history.clone()).unwrap();
        assert_eq!(restored.capture_history(), history);
        assert_eq!(restored.live_state(), state.live_state());
    }
}
