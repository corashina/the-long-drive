//! Ordered caster data and revision-based extraction, independent of camera visibility.
use super::shadow_frame::{CasterInput, validate_casters};
use std::{collections::HashMap, hash::Hash};

pub struct CasterData<K> {
    pub entities: Vec<K>,
    pub casters: Vec<CasterInput>,
    revision: u64,
    changed: bool,
}
impl<K> Default for CasterData<K> {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            casters: Vec::new(),
            revision: 0,
            changed: false,
        }
    }
}
impl<K: Copy> CasterData<K> {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn reserve(&mut self, count: usize) {
        self.entities
            .reserve_exact(count.saturating_sub(self.entities.len()));
        self.casters
            .reserve_exact(count.saturating_sub(self.casters.len()));
    }
    /// Publish only complete, validated collections. Rebuilds always change the revision.
    pub fn publish(&mut self) -> Result<(), &'static str> {
        if self.changed {
            validate_casters(&self.casters)?;
            self.revision = self
                .revision
                .checked_add(1)
                .expect("Source shadow caster revision exhausted");
            self.changed = false;
        }
        Ok(())
    }
    /// The render world owns this copy. Unchanged revisions do not touch its arrays.
    pub fn extract_from(&mut self, input: &Self) -> bool {
        if self.revision == input.revision {
            return false;
        }
        self.entities.clear();
        self.entities.extend_from_slice(&input.entities);
        self.casters.clear();
        self.casters.extend_from_slice(&input.casters);
        self.revision = input.revision;
        self.changed = false;
        true
    }
}

/// Only the main world owns the entity-to-row index.
pub struct CasterIndex<K>(HashMap<K, usize>);
impl<K> Default for CasterIndex<K> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}
impl<K: Copy + Eq + Hash> CasterIndex<K> {
    pub fn contains(&self, entity: K) -> bool {
        self.0.contains_key(&entity)
    }
    pub fn reserve(&mut self, count: usize) {
        self.0.reserve(count.saturating_sub(self.0.len()));
    }
    /// Membership changes rebuild in query order. Retained allocations survive scene replacement.
    pub fn clear(&mut self, data: &mut CasterData<K>) {
        self.0.clear();
        data.entities.clear();
        data.casters.clear();
        data.changed = true;
    }
    /// Returns the row of the caster.
    pub fn update(&mut self, data: &mut CasterData<K>, entity: K, caster: CasterInput) -> usize {
        if let Some(&row) = self.0.get(&entity) {
            if !data.casters[row].unchanged(&caster) {
                data.casters[row] = caster;
                data.changed = true;
            }
            row
        } else {
            let row = data.entities.len();
            self.0.insert(entity, row);
            data.entities.push(entity);
            data.casters.push(caster);
            data.changed = true;
            row
        }
    }
}
