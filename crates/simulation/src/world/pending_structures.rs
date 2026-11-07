//! Wanted structure placements with a revision that changes on each edit, and an id index.
use crate::world::{GeneratedId, Placement, id_index::IdIndex};

#[derive(Default)]
pub struct PendingStructures {
    list: Vec<Placement>,
    index: IdIndex,
    revision: u64,
}
impl PendingStructures {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    fn reindex(&mut self) {
        self.index.rebuild(&self.list, |p| &p.id);
    }
    pub fn contains(&self, id: &GeneratedId) -> bool {
        self.position(id).is_some()
    }
    pub fn position(&self, id: &GeneratedId) -> Option<usize> {
        self.index.position(&self.list, |p| &p.id, id)
    }
    pub fn push(&mut self, placement: Placement) {
        self.index.push(&placement.id, self.list.len());
        self.list.push(placement);
        self.bump();
    }
    pub fn retain(&mut self, keep: impl FnMut(&Placement) -> bool) {
        let count = self.list.len();
        self.list.retain(keep);
        if self.list.len() != count {
            self.reindex();
            self.bump();
        }
    }
    /// An edit that only appends. A longer list changes the revision.
    pub fn append<R>(&mut self, edit: impl FnOnce(&mut Vec<Placement>) -> R) -> R {
        let count = self.list.len();
        let result = edit(&mut self.list);
        if self.list.len() != count {
            self.reindex();
            self.bump();
        }
        result
    }
    /// Any edit. The revision always changes.
    pub fn edit<R>(&mut self, edit: impl FnOnce(&mut Vec<Placement>) -> R) -> R {
        let result = edit(&mut self.list);
        self.reindex();
        self.bump();
        result
    }
    pub fn try_reserve_exact(
        &mut self,
        count: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        self.list.try_reserve_exact(count)?;
        self.index.try_reserve(self.list.capacity())
    }
}
impl std::ops::Deref for PendingStructures {
    type Target = Vec<Placement>;
    fn deref(&self) -> &Self::Target {
        &self.list
    }
}
