//! Live interior list: garage interiors first, then world building interiors.
use crate::reference::{SourceId, WorldInteriorReference, WorldPrototypeReference};
use crate::vehicle::ColliderHandle;
use crate::world::{GeneratedId, StructureInstance};
use std::collections::HashMap;

const CAPACITY: usize = 256;

struct Entry {
    placement: GeneratedId,
    prototype: usize,
    interior: usize,
    /// List index.
    parent: Option<usize>,
}

/// World entries of the live interior list. List indexes start after the garage interiors.
pub struct WorldInteriors {
    offset: usize,
    entries: Vec<Entry>,
    /// Entries before the last rebuild, to remap latched sensor hits.
    previous: Vec<Entry>,
    handles: HashMap<ColliderHandle, usize>,
    revision: Option<u64>,
}

/// `playerscript.SetOutSideAudio` target gain of the outside mixers.
pub fn outside_target(default_gain: f32, use_value: bool, value: f32, doors: f32) -> f32 {
    default_gain + (1. - if use_value { value } else { default_gain }) * doors
}

impl WorldInteriors {
    pub fn new(offset: usize) -> Self {
        Self {
            offset,
            entries: Vec::with_capacity(CAPACITY),
            previous: Vec::with_capacity(CAPACITY),
            handles: HashMap::with_capacity(CAPACITY),
            revision: None,
        }
    }
    pub fn reset(&mut self) {
        self.revision = None;
    }
    /// Rebuilds the entries from the published structures when the stream revision changes.
    /// Returns true after a rebuild.
    pub fn sync(
        &mut self,
        revision: u64,
        structures: &[StructureInstance],
        prototypes: &[WorldPrototypeReference],
    ) -> bool {
        if self.revision == Some(revision) {
            return false;
        }
        self.revision = Some(revision);
        std::mem::swap(&mut self.entries, &mut self.previous);
        self.entries.clear();
        self.handles.clear();
        for s in structures {
            let Some(prototype) = prototypes
                .iter()
                .position(|p| !p.interiors.is_empty() && p.source == s.placement.prototype)
            else {
                continue;
            };
            let base = self.offset + self.entries.len();
            for (interior, r) in prototypes[prototype].interiors.iter().enumerate() {
                if let Some((_, handle)) = s.colliders.iter().find(|(c, _)| *c == r.collider) {
                    self.handles.insert(*handle, base + interior);
                }
                self.entries.push(Entry {
                    placement: s.placement.id.clone(),
                    prototype,
                    interior,
                    parent: r.parent.map(|p| base + p as usize),
                });
            }
        }
        true
    }
    /// Maps a list index from before the last rebuild to the current list.
    pub fn remap(&self, index: usize) -> Option<usize> {
        let Some(old) = index.checked_sub(self.offset) else {
            return Some(index);
        };
        let old = self.previous.get(old)?;
        self.entries
            .iter()
            .position(|e| e.interior == old.interior && e.placement == old.placement)
            .map(|i| self.offset + i)
    }
    /// The list index of a world interior collider.
    pub fn entry(&self, handle: ColliderHandle) -> Option<usize> {
        self.handles.get(&handle).copied()
    }
    /// The live world interior colliders.
    pub fn colliders(&self) -> impl Iterator<Item = ColliderHandle> + '_ {
        self.handles.keys().copied()
    }
    /// The settings of a world list index.
    pub fn get<'a>(
        &self,
        index: usize,
        prototypes: &'a [WorldPrototypeReference],
    ) -> Option<&'a WorldInteriorReference> {
        let e = self.entries.get(index.checked_sub(self.offset)?)?;
        Some(&prototypes[e.prototype].interiors[e.interior])
    }
    /// The placement and prototype interior index of a world list index.
    pub fn owner(&self, index: usize) -> Option<(&GeneratedId, usize)> {
        let e = self.entries.get(index.checked_sub(self.offset)?)?;
        Some((&e.placement, e.interior))
    }
    pub fn parent(&self, index: usize) -> Option<usize> {
        self.entries.get(index.checked_sub(self.offset)?)?.parent
    }
    /// `interiorscript.GetValue` for a world list index. `door` returns a bound door value.
    pub fn value(
        &self,
        index: usize,
        prototypes: &[WorldPrototypeReference],
        door: &impl Fn(&GeneratedId, &WorldPrototypeReference, &SourceId) -> Option<f32>,
        caller: Option<usize>,
        depth: usize,
    ) -> f32 {
        let Some(e) = index
            .checked_sub(self.offset)
            .and_then(|i| self.entries.get(i))
        else {
            return 0.;
        };
        if depth > self.entries.len() {
            return 0.;
        }
        let prototype = &prototypes[e.prototype];
        let r = &prototype.interiors[e.interior];
        // A door that is not bound yet counts as open.
        let value = |d| door(&e.placement, prototype, d).unwrap_or(1.);
        // Door slots have no part binding yet: each counts as an empty, open slot.
        let mut total = r.door_slots.len() as f32;
        for (d, &invert) in r.doors.iter().zip(&r.invert_doors) {
            let v = value(d);
            total += (2. * if invert { 1. - v } else { v }).clamp(0., 1.);
        }
        if let Some(parent) = e.parent
            && Some(parent) != caller
        {
            let v = r.parent_door.as_ref().map_or(1., value);
            total += self.value(parent, prototypes, door, Some(index), depth + 1)
                * (2. * v).clamp(0., 1.);
        }
        super::interior_transmission(
            total,
            r.doors.len() + r.door_slots.len(),
            r.minimum_doors_to_max,
            r.maximum_transmission,
        )
    }
}
