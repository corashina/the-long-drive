//! Network names of objects. Source `tosaveitemscript.idInSave`. Every peer names one object the same way.
use crate::interaction::items::ItemStore;
use crate::reference::ItemId;
use crate::starting_area::spawned::{SpawnedItem, SpawnedItems};
use crate::world::{ObjectKey, fnv1a};
use std::collections::{HashMap, HashSet};

/// The wire name of an object. World objects use `ObjectKey::seed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetKey(pub u64);

impl NetKey {
    /// A runtime spawn: its save serial and pickup index.
    pub fn spawned(item: &SpawnedItem) -> Self {
        let mut bytes = [0u8; 15];
        bytes[..7].copy_from_slice(b"spawned");
        bytes[7..11].copy_from_slice(&item.serial.to_le_bytes());
        bytes[11..].copy_from_slice(&(item.pickup as u32).to_le_bytes());
        Self(fnv1a(&bytes))
    }
}

/// `NetKey` and `ItemId` of each item. The first `refresh` builds it. Later calls apply the
/// items that entered and left `ItemStore` since the last call.
#[derive(Default)]
pub struct ObjectIndex {
    built: bool,
    items: HashMap<NetKey, ItemId>,
    keys: HashMap<ItemId, NetKey>,
    /// Keys that bound since the last `take_fresh`.
    fresh: Vec<NetKey>,
    /// Runtime spawn keys.
    spawned: HashSet<NetKey>,
    /// Runtime spawns that left since the last `take_gone`. A despawn deletes the item.
    gone: Vec<NetKey>,
    /// Scratch list of `ItemStore::take_membership`.
    membership: Vec<(ItemId, bool)>,
}

impl ObjectIndex {
    pub fn refresh(&mut self, items: &mut ItemStore, spawned: &SpawnedItems) {
        items.take_membership(&mut self.membership);
        if !self.built {
            self.built = true;
            self.build(items, spawned);
            return;
        }
        for i in 0..self.membership.len() {
            let (id, entered) = self.membership[i];
            if entered {
                self.enter(id, items, spawned);
            } else {
                self.leave(id);
            }
        }
    }
    /// Offline: drop the membership changes. The next `refresh` after `clear` builds again.
    pub fn skip(&mut self, items: &mut ItemStore) {
        items.take_membership(&mut self.membership);
    }
    fn build(&mut self, items: &ItemStore, spawned: &SpawnedItems) {
        for (id, state) in items.generated_bindings() {
            self.bind(NetKey(id.seed()), state.id);
        }
        for item in spawned.iter() {
            if items.get(item.id).is_some() {
                let key = NetKey::spawned(item);
                self.bind(key, item.id);
                self.spawned.insert(key);
            }
        }
        for state in items.iter() {
            if !self.keys.contains_key(&state.id) {
                self.bind(NetKey(ObjectKey::Imported(state.id).seed()), state.id);
            }
        }
    }
    /// An item that left later in the same list is skipped.
    fn enter(&mut self, id: ItemId, items: &ItemStore, spawned: &SpawnedItems) {
        if items.get(id).is_none() {
            return;
        }
        let key = if let Some(generated) = items.generated_key(id) {
            NetKey(generated.seed())
        } else if let Some(item) = spawned.get(id) {
            let key = NetKey::spawned(item);
            self.spawned.insert(key);
            key
        } else {
            NetKey(ObjectKey::Imported(id).seed())
        };
        self.bind(key, id);
    }
    fn leave(&mut self, id: ItemId) {
        let Some(key) = self.keys.remove(&id) else {
            return;
        };
        if self.items.get(&key) == Some(&id) {
            self.items.remove(&key);
        }
        if self.spawned.remove(&key) {
            self.gone.push(key);
        }
    }
    fn bind(&mut self, key: NetKey, item: ItemId) {
        if self.items.insert(key, item) != Some(item) {
            self.fresh.push(key);
        }
        self.keys.insert(item, key);
    }
    /// Runtime spawns that left since the last call.
    pub fn take_gone(&mut self, out: &mut Vec<NetKey>) {
        out.clear();
        std::mem::swap(out, &mut self.gone);
    }
    /// Keys that bound since the last call.
    pub fn take_fresh(&mut self, out: &mut Vec<NetKey>) {
        out.clear();
        std::mem::swap(out, &mut self.fresh);
    }
    pub fn item(&self, key: NetKey) -> Option<ItemId> {
        self.items.get(&key).copied()
    }
    pub fn key(&self, item: ItemId) -> Option<NetKey> {
        self.keys.get(&item).copied()
    }
    /// Forces the next `refresh` to build.
    pub fn clear(&mut self) {
        self.built = false;
        self.items.clear();
        self.keys.clear();
        self.fresh.clear();
        self.spawned.clear();
        self.gone.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawned_keys_differ_by_serial_and_pickup() {
        let item = |serial, pickup| SpawnedItem {
            id: ItemId(1),
            serial,
            prototype: 0,
            pickup,
        };
        let a = NetKey::spawned(&item(1, 0));
        assert_eq!(a, NetKey::spawned(&item(1, 0)));
        assert_ne!(a, NetKey::spawned(&item(2, 0)));
        assert_ne!(a, NetKey::spawned(&item(1, 1)));
    }

    #[test]
    fn membership_changes_match_a_new_build() {
        use crate::vehicle::VehiclePose;
        use rapier3d::prelude::RigidBodyHandle;
        let mut items = ItemStore::empty_for_tests();
        let spawned = SpawnedItems::default();
        let add = |items: &mut ItemStore, id| {
            let pose = VehiclePose {
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
            };
            let body = RigidBodyHandle::from_raw_parts(id, 0);
            items
                .insert_spawned(ItemId(id), "", pose, body, None)
                .unwrap();
        };
        let remove = |items: &mut ItemStore, id| {
            items.set_body(ItemId(id), None);
            assert!(items.remove_spawned(ItemId(id)));
        };
        for id in 1..=3 {
            add(&mut items, id);
        }
        let mut index = ObjectIndex::default();
        let mut fresh = Vec::new();
        index.refresh(&mut items, &spawned);
        index.take_fresh(&mut fresh);
        assert_eq!(fresh.len(), 3);
        add(&mut items, 4);
        remove(&mut items, 2);
        add(&mut items, 5);
        remove(&mut items, 5);
        items.set_body(ItemId(1), None);
        index.refresh(&mut items, &spawned);
        index.take_fresh(&mut fresh);
        assert_eq!(fresh, [NetKey(ObjectKey::Imported(ItemId(4)).seed())]);
        let mut built = ObjectIndex::default();
        built.refresh(&mut items, &spawned);
        assert_eq!(index.items, built.items);
        assert_eq!(index.keys, built.keys);
    }
}
