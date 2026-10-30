//! Wanted structure placements with a revision that changes on each edit.
use crate::world::Placement;

#[derive(Default)]
pub struct PendingStructures {
    list: Vec<Placement>,
    revision: u64,
}
impl PendingStructures {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn push(&mut self, placement: Placement) {
        self.list.push(placement);
        self.bump();
    }
    pub fn retain(&mut self, keep: impl FnMut(&Placement) -> bool) {
        let count = self.list.len();
        self.list.retain(keep);
        if self.list.len() != count {
            self.bump();
        }
    }
    /// An edit that only appends. A longer list changes the revision.
    pub fn append<R>(&mut self, edit: impl FnOnce(&mut Vec<Placement>) -> R) -> R {
        let count = self.list.len();
        let result = edit(&mut self.list);
        if self.list.len() != count {
            self.bump();
        }
        result
    }
    /// Any edit. The revision always changes.
    pub fn edit<R>(&mut self, edit: impl FnOnce(&mut Vec<Placement>) -> R) -> R {
        let result = edit(&mut self.list);
        self.bump();
        result
    }
    pub fn try_reserve_exact(
        &mut self,
        count: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        self.list.try_reserve_exact(count)
    }
}
impl std::ops::Deref for PendingStructures {
    type Target = Vec<Placement>;
    fn deref(&self) -> &Self::Target {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(ordinal: u32) -> Placement {
        let source = crate::reference::SourceId {
            file: String::new(),
            path_id: 0,
        };
        Placement {
            id: crate::world::GeneratedId {
                layer: crate::world::WorldLayer::Prop,
                generator: source.clone(),
                unit: [0, 0],
                ordinal,
                children: Vec::new(),
            },
            prototype: source,
            pose: crate::world::AbsolutePose {
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
            },
            scale: [1.; 3],
            bounds: crate::world::WorldBounds {
                min: [0.; 3],
                max: [0.; 3],
            },
            object_seed: 0,
        }
    }

    #[test]
    fn revision_changes_only_on_add_and_remove() {
        let mut pending = PendingStructures::default();
        let start = pending.revision();
        pending.push(placement(1));
        let added = pending.revision();
        assert_ne!(added, start);
        pending.retain(|_| true);
        pending.append(|_| ());
        assert_eq!(pending.revision(), added);
        pending.append(|list| list.push(placement(2)));
        let appended = pending.revision();
        assert_ne!(appended, added);
        pending.retain(|p| p.id.ordinal != 1);
        assert_ne!(pending.revision(), appended);
        assert_eq!(pending.len(), 1);
    }
}
