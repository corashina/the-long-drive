//! Source caller boundaries survive unloading; generated content does not.
use super::{GeneratedId, UnityRandom, UnityRandomState};
use crate::reference::{ReferenceError, SourceId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Inputs that can change source selection, captured at each first invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnContext {
    pub current_car: i32,
    pub peaceful_enemies: i32,
    pub date: [i32; 3],
    pub chance_bits: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnBoundary {
    pub parent: GeneratedId,
    pub caller: SourceId,
    pub ordinal: u32,
    pub phase: SpawnPhase,
    pub random: UnityRandomState,
    pub context: SpawnContext,
    pub painting_pools: Vec<PaintingPool>,
    pub initialization: SpawnInitialization,
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnHistory {
    pub live: UnityRandomState,
    pub painting_pools: Vec<PaintingPool>,
    pub boundaries: Vec<SpawnBoundary>,
    pub context: SpawnContext,
}
pub struct SourceSpawnState {
    live: UnityRandom,
    painting_pools: Vec<PaintingPool>,
    parents: HashSet<GeneratedId>,
    boundaries: Vec<SpawnBoundary>,
    /// Boundary indices by key hash. Lookups stay constant time on long trips.
    index: HashMap<u64, Vec<usize>>,
    /// Heap of boundaries, index buckets and parent identities.
    heap_bytes: usize,
    pub context: SpawnContext,
}
fn boundary_hash(parent: &GeneratedId, caller: &SourceId, ordinal: u32, phase: SpawnPhase) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    (parent, caller, ordinal, phase).hash(&mut hasher);
    hasher.finish()
}
impl SourceSpawnState {
    pub fn capture_history(&self) -> SpawnHistory {
        SpawnHistory {
            live: self.live.snapshot(),
            painting_pools: self.painting_pools.clone(),
            boundaries: self.boundaries.clone(),
            context: self.context,
        }
    }
    pub fn restore_history(history: SpawnHistory) -> Result<Self, ReferenceError> {
        let mut state = Self {
            live: UnityRandom::restore(history.live),
            painting_pools: history.painting_pools,
            boundaries: Vec::with_capacity(history.boundaries.len()),
            index: HashMap::with_capacity(history.boundaries.len()),
            heap_bytes: 0,
            parents: HashSet::new(),
            context: history.context,
        };
        for boundary in history.boundaries {
            if state.has_boundary(
                &boundary.parent,
                &boundary.caller,
                boundary.ordinal,
                boundary.phase,
            ) {
                return Err(ReferenceError("Duplicate spawn boundary".into()));
            }
            state.push(boundary);
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
        self.index
            .get(&boundary_hash(parent, caller, ordinal, phase))?
            .iter()
            .map(|&i| &self.boundaries[i])
            .find(|b| {
                &b.parent == parent
                    && &b.caller == caller
                    && b.ordinal == ordinal
                    && b.phase == phase
            })
    }
    fn push(&mut self, boundary: SpawnBoundary) {
        let bucket = self
            .index
            .entry(boundary_hash(
                &boundary.parent,
                &boundary.caller,
                boundary.ordinal,
                boundary.phase,
            ))
            .or_default();
        let before = bucket.capacity();
        bucket.push(self.boundaries.len());
        self.heap_bytes += (bucket.capacity() - before) * std::mem::size_of::<usize>()
            + identity_bytes(&boundary.parent)
            + boundary.caller.file.capacity()
            + pools_bytes(&boundary.painting_pools)
            + boundary.initialization.retained_bytes();
        if !self.parents.contains(&boundary.parent) {
            self.heap_bytes += identity_bytes(&boundary.parent);
            self.parents.insert(boundary.parent.clone());
        }
        self.boundaries.push(boundary);
    }

    pub fn supplied_controlled_startup(
        reference: &crate::reference::WorldSpawnContext,
        context: SpawnContext,
    ) -> Self {
        Self {
            live: UnityRandom::restore(UnityRandomState {
                words: reference.startup_state,
            }),
            boundaries: Vec::new(),
            index: HashMap::new(),
            heap_bytes: 0,
            painting_pools: Vec::new(),
            parents: HashSet::new(),
            context,
        }
    }
    /// Constant time: per-boundary heap is summed when a boundary is pushed.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + pools_bytes(&self.painting_pools)
            + self.boundaries.capacity() * std::mem::size_of::<SpawnBoundary>()
            + self.index.capacity() * std::mem::size_of::<(u64, Vec<usize>)>()
            + self.parents.capacity() * std::mem::size_of::<GeneratedId>()
            + self.heap_bytes
    }
    pub fn boundaries(&self) -> &[SpawnBoundary] {
        &self.boundaries
    }
    pub fn parent_count(&self) -> usize {
        self.parents.len()
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
            let overlap = pools_bytes(&boundary.painting_pools);
            if overlap > staging {
                return Err(ReferenceError(
                    "Spawn replay pool clone exceeds staging".into(),
                ));
            }
            return call(
                &mut UnityRandom::restore(boundary.random),
                boundary.context,
                &mut boundary.painting_pools.clone(),
                &boundary.initialization,
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
        let boundary = SpawnBoundary {
            parent: parent.clone(),
            caller: caller.clone(),
            ordinal,
            phase,
            random: self.live.snapshot(),
            context: self.context,
            painting_pools: self.painting_pools.clone(),
            initialization: initialization.clone(),
        };
        let mut random = self.live;
        let mut pools = self.painting_pools.clone();
        let value = call(
            &mut random,
            self.context,
            &mut pools,
            initialization,
            staging - overlap,
        )?;
        self.push(boundary);
        self.live = random;
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
mod tests {
    use super::*;

    fn boundary(ordinal: u32, painting: usize) -> SpawnBoundary {
        let source = SourceId {
            file: "level1".into(),
            path_id: 1,
        };
        SpawnBoundary {
            parent: GeneratedId {
                layer: super::super::WorldLayer::Building,
                generator: source.clone(),
                unit: [i64::from(ordinal / 3), 0],
                ordinal: ordinal / 3,
                children: Vec::new(),
            },
            caller: source.clone(),
            ordinal,
            phase: [
                SpawnPhase::Synchronous,
                SpawnPhase::ComponentStart,
                SpawnPhase::SynchronousAndStarts,
            ][ordinal as usize % 3],
            random: UnityRandomState {
                words: [ordinal; 4],
            },
            context: SpawnContext {
                current_car: 0,
                peaceful_enemies: 1,
                date: [0; 3],
                chance_bits: None,
            },
            painting_pools: vec![PaintingPool {
                source,
                available: vec![0; painting],
            }],
            initialization: SpawnInitialization::default(),
        }
    }
    fn history(boundaries: Vec<SpawnBoundary>) -> SpawnHistory {
        SpawnHistory {
            live: UnityRandomState { words: [1; 4] },
            painting_pools: Vec::new(),
            boundaries,
            context: boundary(0, 0).context,
        }
    }

    #[test]
    fn boundary_index_matches_scan() {
        let boundaries: Vec<_> = (0..1000).map(|i| boundary(i, 1)).collect();
        let state = SourceSpawnState::restore_history(history(boundaries.clone())).unwrap();
        assert_eq!(state.parent_count(), 334);
        for b in &boundaries {
            let found = state
                .find(&b.parent, &b.caller, b.ordinal, b.phase)
                .unwrap();
            assert_eq!(found, b);
        }
        let b = &boundaries[0];
        assert!(!state.has_boundary(&b.parent, &b.caller, b.ordinal, SpawnPhase::ComponentStart));
        assert_eq!(state.capture_history().boundaries, boundaries);
        let mut duplicate = boundaries;
        duplicate.push(duplicate[7].clone());
        assert!(SourceSpawnState::restore_history(history(duplicate)).is_err());
    }

    #[test]
    fn large_spawn_history_restores() {
        // 3,000 boundaries with 8 KiB pools each is about 24 MiB, over the old 16 MiB cap.
        let boundaries: Vec<_> = (0..3000).map(|i| boundary(i, 2048)).collect();
        let state = SourceSpawnState::restore_history(history(boundaries)).unwrap();
        assert!(state.retained_bytes() > 16 * 1024 * 1024);
        assert_eq!(state.boundary_count(), 3000);
    }
}
