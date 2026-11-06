//! Published structure instances with an id index.
use crate::world::{GeneratedId, StructureInstance, id_index::IdIndex};

#[derive(Default)]
pub struct StructureList {
    list: Vec<StructureInstance>,
    index: IdIndex,
}
impl StructureList {
    pub fn contains(&self, id: &GeneratedId) -> bool {
        self.index
            .position(&self.list, |s| &s.placement.id, id)
            .is_some()
    }
    pub fn push(&mut self, instance: StructureInstance) {
        self.index.push(&instance.placement.id, self.list.len());
        self.list.push(instance);
    }
    pub fn swap_remove(&mut self, index: usize) -> StructureInstance {
        let instance = self.list.swap_remove(index);
        self.index.rebuild(&self.list, |s| &s.placement.id);
        instance
    }
    /// The instance of `id`. The caller must not change the placement id.
    pub fn find_mut(&mut self, id: &GeneratedId) -> Option<&mut StructureInstance> {
        let index = self.index.position(&self.list, |s| &s.placement.id, id)?;
        Some(&mut self.list[index])
    }
    /// The caller must not change the placement id.
    pub fn get_mut(&mut self, index: usize) -> &mut StructureInstance {
        &mut self.list[index]
    }
}
impl std::ops::Deref for StructureList {
    type Target = Vec<StructureInstance>;
    fn deref(&self) -> &Self::Target {
        &self.list
    }
}
