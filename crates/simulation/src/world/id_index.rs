//! Reused hash index of generated ids in a slice. Lookups need no scan. The map keeps its
//! capacity, so a rebuild does not allocate.
use super::GeneratedId;
use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

/// Id hash to the first position with that hash. When the id at that position differs from the
/// asked id, two ids share a hash, and the lookup scans the slice.
#[derive(Default)]
pub struct IdIndex {
    first: HashMap<u64, u32>,
    hasher: BuildHasherDefault<DefaultHasher>,
}
impl IdIndex {
    /// Indexes `items`. Later lookups must pass the same slice, changed only through `push`.
    pub fn rebuild<T>(&mut self, items: &[T], id: impl Fn(&T) -> &GeneratedId) {
        self.first.clear();
        for (i, item) in items.iter().enumerate() {
            self.push(id(item), i);
        }
    }
    /// Adds the id of the item at `position`.
    pub fn push(&mut self, id: &GeneratedId, position: usize) {
        self.first
            .entry(self.hasher.hash_one(id))
            .or_insert(position as u32);
    }
    /// First position of `wanted` in `items`.
    pub fn position<T>(
        &self,
        items: &[T],
        id: impl Fn(&T) -> &GeneratedId,
        wanted: &GeneratedId,
    ) -> Option<usize> {
        let &i = self.first.get(&self.hasher.hash_one(wanted))?;
        if id(&items[i as usize]) == wanted {
            Some(i as usize)
        } else {
            items.iter().position(|item| id(item) == wanted)
        }
    }
    /// Room for `count` ids in total.
    pub fn try_reserve(&mut self, count: usize) -> Result<(), std::collections::TryReserveError> {
        self.first
            .try_reserve(count.saturating_sub(self.first.len()))
    }
}
