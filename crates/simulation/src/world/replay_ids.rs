//! Reused hash set of the generated ids that selected change records name. A miss means the id
//! is not named. A hit needs an exact check, because two ids can share a hash.
use crate::world::changes::{ChangeRecordIndex, ChangeRecords};
use crate::world::{GeneratedId, ObjectKey};
use std::collections::HashSet;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

#[derive(Default)]
pub struct ReplayIds {
    hashes: HashSet<u64>,
    hasher: BuildHasherDefault<DefaultHasher>,
}
impl ReplayIds {
    /// Adds the record key, instance, replay root and attachment owner of each record.
    pub fn rebuild(&mut self, indices: &[ChangeRecordIndex], changes: &ChangeRecords) {
        self.hashes.clear();
        for &index in indices {
            let (key, change) = changes.get_indexed(index);
            let owner = change
                .snapshot
                .as_ref()
                .and_then(|s| s.attachment.as_ref())
                .and_then(|a| a.owner.as_ref());
            for key in [Some(key), owner].into_iter().flatten() {
                if let ObjectKey::Generated(id) = key {
                    self.hashes.insert(self.hasher.hash_one(id));
                }
            }
            for id in change
                .instance
                .iter()
                .chain(change.replay.as_ref().map(|p| &p.id))
            {
                self.hashes.insert(self.hasher.hash_one(id));
            }
        }
    }
    pub fn may_contain(&self, id: &GeneratedId) -> bool {
        self.hashes.contains(&self.hasher.hash_one(id))
    }
    pub fn retained_bytes(&self) -> usize {
        crate::world::metrics::map_bytes(self.hashes.capacity(), std::mem::size_of::<u64>())
    }
}

/// A selected record is the change of `id`.
pub fn changed(
    ids: &ReplayIds,
    indices: &[ChangeRecordIndex],
    changes: &ChangeRecords,
    id: &GeneratedId,
) -> bool {
    ids.may_contain(id)
        && indices.iter().any(|&index| {
            matches!(changes.get_indexed(index).0, ObjectKey::Generated(key) if key == id)
        })
}

/// A selected record names `id` as its instance, replay root or attachment owner.
pub fn selects(
    ids: &ReplayIds,
    indices: &[ChangeRecordIndex],
    changes: &ChangeRecords,
    id: &GeneratedId,
) -> bool {
    ids.may_contain(id)
        && indices.iter().any(|&index| {
            let (_, c) = changes.get_indexed(index);
            c.instance.as_ref() == Some(id)
                || c.replay.as_ref().is_some_and(|root| &root.id == id)
                || matches!(c.snapshot
                        .as_ref()
                        .and_then(|s| s.attachment.as_ref())
                        .and_then(|a| a.owner.as_ref()),
                        Some(ObjectKey::Generated(owner)) if owner == id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{ItemId, SourceId};
    use crate::world::{Placement, StructureInstance, WorldLayer};

    fn id(ordinal: u32) -> GeneratedId {
        GeneratedId {
            layer: WorldLayer::Prop,
            generator: SourceId {
                file: "a".into(),
                path_id: 1,
            },
            unit: [0, 0],
            ordinal,
            children: Vec::new(),
        }
    }
    fn placement(ordinal: u32) -> Placement {
        serde_json::from_value(serde_json::json!({
            "id": id(ordinal),
            "prototype": { "file": "a", "path_id": 2 },
            "pose": { "position": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0] },
            "scale": [1.0, 1.0, 1.0],
            "bounds": { "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 1.0] },
            "object_seed": 0,
        }))
        .unwrap()
    }
    fn snapshot(owner: Option<u32>) -> crate::world::ObjectSnapshot {
        let attachment = owner.map(|o| {
            serde_json::json!({
                "owner": ObjectKey::Generated(id(o)),
                "slot": null,
                "volume": null,
                "local_pose": { "position": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0] },
            })
        });
        serde_json::from_value(serde_json::json!({
            "pose": { "position": [1.0, 0.0, 1.0], "rotation": [0.0, 0.0, 0.0, 1.0] },
            "linear_velocity": [0.0, 0.0, 0.0],
            "angular_velocity": [0.0, 0.0, 0.0],
            "sleeping": false,
            "held": false,
            "buried": false,
            "conditions": [],
            "attachment": attachment,
        }))
        .unwrap()
    }

    /// The indexed lookups give the same answers as the scans they replace.
    #[test]
    fn indexed_selection_matches_scans() {
        let mut baseline = snapshot(None);
        baseline.sleeping = true;
        let mut changes = ChangeRecords::default();
        changes.record(ObjectKey::Generated(id(1)), snapshot(None), &baseline);
        changes.record(ObjectKey::Generated(id(2)), snapshot(None), &baseline);
        changes.set_replay(&ObjectKey::Generated(id(2)), &placement(3), &id(4));
        changes.record(ObjectKey::Imported(ItemId(9)), snapshot(Some(5)), &baseline);
        changes.record(ObjectKey::Generated(id(6)), snapshot(Some(7)), &baseline);
        changes.forget(&ObjectKey::Generated(id(1)));
        changes.record(ObjectKey::Generated(id(8)), snapshot(None), &baseline);
        let mut indices = Vec::new();
        let everywhere = crate::world::WorldBounds {
            min: [-1e9; 3],
            max: [1e9; 3],
        };
        changes.indices_in(everywhere, &mut indices);
        assert_eq!(indices.len(), 4);
        // The record of id 6 is not selected.
        indices.retain(|&i| changes.get_indexed(i).0 != &ObjectKey::Generated(id(6)));
        let mut ids = ReplayIds::default();
        ids.rebuild(&indices, &changes);

        let mut structures = super::super::structure_list::StructureList::default();
        let mut wanted = super::super::pending_structures::PendingStructures::default();
        for ordinal in [2, 4, 6, 3, 5] {
            structures.push(StructureInstance {
                fixed_decoration: false,
                cpu_bytes: 0,
                placement: placement(ordinal),
                bodies: Vec::new(),
                colliders: Vec::new(),
            });
            wanted.push(placement(ordinal));
        }
        structures.swap_remove(1);
        wanted.retain(|p| p.id != id(6));
        for ordinal in 0..10 {
            let wanted_id = id(ordinal);
            let key = ObjectKey::Generated(wanted_id.clone());
            let records = indices.iter().map(|&i| changes.get_indexed(i));
            assert_eq!(
                changed(&ids, &indices, &changes, &wanted_id),
                records.clone().any(|(k, _)| k == &key),
            );
            assert_eq!(
                selects(&ids, &indices, &changes, &wanted_id),
                records.clone().any(|(_, c)| {
                    c.instance.as_ref() == Some(&wanted_id)
                        || c.replay.as_ref().is_some_and(|r| r.id == wanted_id)
                        || c.snapshot
                            .as_ref()
                            .and_then(|s| s.attachment.as_ref())
                            .and_then(|a| a.owner.as_ref())
                            == Some(&key)
                }),
            );
            assert_eq!(changes.get_generated(&wanted_id), changes.get(&key));
            assert_eq!(
                structures.contains(&wanted_id),
                structures.iter().any(|s| s.placement.id == wanted_id),
            );
            assert_eq!(
                wanted.position(&wanted_id),
                wanted.iter().position(|p| p.id == wanted_id),
            );
        }
        assert!(selects(&ids, &indices, &changes, &id(4)));
        assert!(selects(&ids, &indices, &changes, &id(5)));
        assert!(!selects(&ids, &indices, &changes, &id(7)));
        assert!(changed(&ids, &indices, &changes, &id(8)));
    }
}
