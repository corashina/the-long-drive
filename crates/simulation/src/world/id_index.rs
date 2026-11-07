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
            .entry(self.hasher.hash_one(id.seed()))
            .or_insert(position as u32);
    }
    /// First position of `wanted` in `items`.
    pub fn position<T>(
        &self,
        items: &[T],
        id: impl Fn(&T) -> &GeneratedId,
        wanted: &GeneratedId,
    ) -> Option<usize> {
        self.position_by(items, id, wanted.seed(), |other| other == wanted)
    }
    /// First position in `items` of an id that passes `matches`. Only ids with seed `seed` pass.
    pub fn position_by<T>(
        &self,
        items: &[T],
        id: impl Fn(&T) -> &GeneratedId,
        seed: u64,
        matches: impl Fn(&GeneratedId) -> bool,
    ) -> Option<usize> {
        let &i = self.first.get(&self.hasher.hash_one(seed))?;
        if matches(id(&items[i as usize])) {
            Some(i as usize)
        } else {
            items.iter().position(|item| matches(id(item)))
        }
    }
    /// Room for `count` ids in total.
    pub fn try_reserve(&mut self, count: usize) -> Result<(), std::collections::TryReserveError> {
        self.first
            .try_reserve(count.saturating_sub(self.first.len()))
    }
    pub fn retained_bytes(&self) -> usize {
        super::metrics::map_bytes(self.first.capacity(), std::mem::size_of::<(u64, u32)>())
    }
}
