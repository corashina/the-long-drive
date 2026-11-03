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

/// `NetKey` and `ItemId` of each item. It refreshes when the item membership changes.
#[derive(Default)]
pub struct ObjectIndex {
    revision: Option<u64>,
    items: HashMap<NetKey, ItemId>,
    keys: HashMap<ItemId, NetKey>,
    previous: HashMap<NetKey, ItemId>,
    /// Keys that bound since the last `take_fresh`.
    fresh: Vec<NetKey>,
    /// Runtime spawn keys, now and at the last rebuild.
    spawned: HashSet<NetKey>,
    previous_spawned: HashSet<NetKey>,
    /// Runtime spawns that left since the last `take_gone`. A despawn deletes the item.
    gone: Vec<NetKey>,
}

impl ObjectIndex {
    pub fn refresh(&mut self, items: &ItemStore, spawned: &SpawnedItems) {
        if self.revision == Some(items.membership_revision()) {
            return;
        }
        self.revision = Some(items.membership_revision());
        std::mem::swap(&mut self.items, &mut self.previous);
        self.items.clear();
        self.keys.clear();
        for (id, state) in items.generated_bindings() {
            self.insert(NetKey(id.seed()), state.id);
        }
        std::mem::swap(&mut self.spawned, &mut self.previous_spawned);
        self.spawned.clear();
        for item in spawned.iter() {
            if items.get(item.id).is_some() {
                let key = NetKey::spawned(item);
                self.insert(key, item.id);
                self.spawned.insert(key);
            }
        }
        self.gone.extend(
            self.previous_spawned
                .iter()
                .filter(|key| !self.spawned.contains(key)),
        );
        for state in items.iter() {
            if !self.keys.contains_key(&state.id) {
                self.insert(NetKey(ObjectKey::Imported(state.id).seed()), state.id);
            }
        }
        for (key, item) in &self.items {
            if self.previous.get(key) != Some(item) {
                self.fresh.push(*key);
            }
        }
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
    fn insert(&mut self, key: NetKey, item: ItemId) {
        self.items.insert(key, item);
        self.keys.insert(item, key);
    }
    pub fn item(&self, key: NetKey) -> Option<ItemId> {
        self.items.get(&key).copied()
    }
    pub fn key(&self, item: ItemId) -> Option<NetKey> {
        self.keys.get(&item).copied()
    }
    /// Forces the next `refresh` to rebuild.
    pub fn clear(&mut self) {
        self.revision = None;
        self.items.clear();
        self.keys.clear();
        self.previous.clear();
        self.fresh.clear();
        self.spawned.clear();
        self.previous_spawned.clear();
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
}
