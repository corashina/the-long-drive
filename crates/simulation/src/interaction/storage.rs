//! Source `mountStuff` physics locks. Any loose item can lock to a storage volume.
//! World prototype volumes bind while their owner streams in.
use super::{
    items::{ItemLocation, ItemStore},
    mechanisms::MechanismState,
    parts::PartAttachments,
};
use crate::reference::{
    ItemId, PartSlotId, PrototypeStorageFlag, ReferenceError, ReferenceManifest, SourceId,
};
use crate::vehicle::{PhysicsWorld, VEHICLE_COLLIDER_FLAG, from_rapier, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, QueryFilter, RigidBodyHandle, SharedShape};
use std::collections::{HashMap, HashSet};

/// Scan buffer capacity per volume. Distinct items seen in one volume.
const SCAN_CAPACITY: usize = 64;
/// Collider scratch capacity for one lock or unlock.
const COLLIDER_CAPACITY: usize = 256;
/// Preallocated world volume slots and world flag entries.
const WORLD_CAPACITY: usize = 64;
/// Source `mountStuff.StartFreeze`: two yields, then three `QuickFreeze` passes.
const START_FRAMES: u8 = 5;
const START_PASSES: u8 = 3;

/// One locked item. Its own body is disabled while its colliders ride on the carrier.
struct Locked {
    item: ItemId,
    volume: u16,
    body: RigidBodyHandle,
    /// The item pose relative to its own body.
    loose_local: Pose,
    /// Mass of the item body, with the items locked to it.
    mass: f32,
}
/// Where a volume gets its world pose.
enum Frame {
    /// The box pose relative to the owner item, and its prototype volume record.
    Owner { local: Pose, record: u16 },
    /// Free volume slot.
    Free,
}
struct Volume {
    owner: ItemId,
    storage_type: u8,
    active: bool,
    /// Source `onlyManual`. The periodic scan skips the volume.
    only_manual: bool,
    dont_lock_if_owner_picked_up: bool,
    slots: Vec<PartSlotId>,
    usables: Vec<ItemId>,
    frame: Frame,
    shape: SharedShape,
    /// Gate slots are filled and gate usables are closed. Set before each scan.
    gates_closed: bool,
    /// Frames left of the source `StartFreeze`. Zero: started.
    startup: u8,
    previous: Vec<ItemId>,
    current: Vec<ItemId>,
}
impl Volume {
    fn free() -> Self {
        Self {
            owner: ItemId(u32::MAX),
            storage_type: 0,
            active: false,
            only_manual: false,
            dont_lock_if_owner_picked_up: false,
            slots: Vec::with_capacity(4),
            usables: Vec::with_capacity(4),
            frame: Frame::Free,
            shape: SharedShape::ball(0.0),
            gates_closed: false,
            startup: 0,
            previous: Vec::with_capacity(SCAN_CAPACITY),
            current: Vec::with_capacity(SCAN_CAPACITY),
        }
    }
}
/// A storage volume of a world prototype, in the prototype root frame.
struct PrototypeVolume {
    prototype: SourceId,
    source: SourceId,
    center: [f32; 3],
    rotation: Rotation,
    half_extents: [f32; 3],
    shape: SharedShape,
    storage_type: u8,
    only_manual: bool,
    /// Prototype node of each gate usable.
    usables: Vec<u32>,
    /// Source of each gate part slot.
    slots: Vec<SourceId>,
    dont_lock_if_owner_picked_up: bool,
}
pub struct StorageLocks {
    locked: Vec<Locked>,
    /// Bound volumes. A freed slot is reused.
    volumes: Vec<Volume>,
    free: Vec<u16>,
    prototype_volumes: Vec<PrototypeVolume>,
    prototype_flags: Vec<PrototypeStorageFlag>,
    /// Generated items that left the lock with their unbound owner. The stream unloads them.
    departing: Vec<ItemId>,
    dont_mount: HashSet<ItemId>,
    /// Source `dontMountTillPickup`. A pick-up removes the item. Not saved.
    till_pickup: HashSet<ItemId>,
    drop_times: HashMap<ItemId, f32>,
    colliders: Vec<ColliderHandle>,
    riders: Vec<ItemId>,
    mount_mask: u32,
    drop_delay: f32,
    scan_interval: f32,
    elapsed: f32,
    last_scan: f32,
    startup_frame: u8,
}
impl StorageLocks {
    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let world = &reference.world;
        let prototype_volumes = reference
            .storage
            .prototype_volumes
            .iter()
            .map(|record| {
                let usables = record
                    .usables
                    .iter()
                    .map(|source| {
                        world
                            .mechanisms
                            .iter()
                            .find(|m| m.prototype == record.prototype && m.recipe.source == *source)
                            .map(|m| m.node)
                            .ok_or_else(|| error("Missing world storage usable"))
                    })
                    .collect::<Result<Vec<_>, ReferenceError>>()?;
                let [x, y, z] = record.half_extents;
                Ok(PrototypeVolume {
                    prototype: record.prototype.clone(),
                    source: record.source.clone(),
                    center: record.center,
                    rotation: Rotation::from_array(record.rotation).normalize(),
                    half_extents: record.half_extents,
                    shape: SharedShape::cuboid(x, y, z),
                    storage_type: record.storage_type,
                    only_manual: record.only_manual,
                    usables,
                    slots: record.slots.clone(),
                    dont_lock_if_owner_picked_up: record.dont_lock_if_owner_picked_up,
                })
            })
            .collect::<Result<Vec<_>, ReferenceError>>()?;
        let flags = &reference.storage.flags;
        let mut locks = Self::with_volumes(
            Vec::with_capacity(WORLD_CAPACITY),
            reference.items.len(),
            &reference.storage.policy,
        );
        locks.prototype_volumes = prototype_volumes;
        locks.prototype_flags = reference.storage.prototype_flags.clone();
        locks
            .dont_mount
            .extend(flags.iter().filter(|f| f.dont_mount).map(|f| f.item));
        locks.till_pickup.extend(
            flags
                .iter()
                .filter(|f| f.dont_mount_till_pickup)
                .map(|f| f.item),
        );
        Ok(locks)
    }
    fn with_volumes(
        volumes: Vec<Volume>,
        items: usize,
        policy: &crate::reference::StoragePolicy,
    ) -> Self {
        Self {
            locked: Vec::with_capacity(items),
            volumes,
            free: Vec::with_capacity(WORLD_CAPACITY),
            prototype_volumes: Vec::new(),
            prototype_flags: Vec::new(),
            departing: Vec::with_capacity(WORLD_CAPACITY),
            dont_mount: HashSet::with_capacity(items + WORLD_CAPACITY),
            till_pickup: HashSet::with_capacity(items + WORLD_CAPACITY),
            drop_times: HashMap::with_capacity(items + WORLD_CAPACITY),
            colliders: Vec::with_capacity(COLLIDER_CAPACITY),
            riders: Vec::with_capacity(items),
            mount_mask: policy.mount_mask,
            drop_delay: policy.drop_delay_seconds,
            scan_interval: policy.scan_interval_seconds,
            elapsed: 0.0,
            last_scan: 0.0,
            startup_frame: 0,
        }
    }
    /// Binds the storage volumes and flags of a generated item of `prototype`.
    /// `item_root` is the item frame in the prototype root frame, before `scale`.
    /// `usable_item` maps a prototype node to its generated mechanism item.
    /// `slot` maps a `partslotscript` source to the generated part slot of this item. A static
    /// control has no part slots, so its volumes bind without them.
    pub fn bind_world_volumes(
        &mut self,
        item: ItemId,
        prototype: &SourceId,
        item_root: Pose,
        scale: [f32; 3],
        usable_item: impl Fn(u32) -> Option<ItemId>,
        slot: impl Fn(&SourceId) -> Option<PartSlotId>,
    ) -> Result<(), ReferenceError> {
        if self
            .volumes
            .iter()
            .any(|v| v.owner == item && !matches!(v.frame, Frame::Free))
        {
            return Ok(());
        }
        for record in self
            .prototype_volumes
            .iter()
            .filter(|r| r.prototype == *prototype)
        {
            if record
                .usables
                .iter()
                .any(|node| usable_item(*node).is_none())
            {
                return Err(error("Missing world storage usable item"));
            }
        }
        if let Some(flag) = self
            .prototype_flags
            .iter()
            .find(|f| f.prototype == *prototype)
        {
            if flag.dont_mount {
                self.dont_mount.insert(item);
            }
            if flag.dont_mount_till_pickup {
                self.till_pickup.insert(item);
            }
        }
        let unit = scale.iter().all(|s| (s - 1.0).abs() < 1e-4);
        let root = Pose::from_parts(
            item_root.translation * Vector::from_array(scale),
            item_root.rotation,
        );
        for (record_index, record) in self
            .prototype_volumes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.prototype == *prototype)
        {
            let index = match self.free.pop() {
                Some(index) => index as usize,
                None if self.volumes.len() < u16::MAX as usize => {
                    self.volumes.push(Volume::free());
                    self.volumes.len() - 1
                }
                None => return Err(error("Storage volume slots exhausted")),
            };
            let volume = &mut self.volumes[index];
            let center = Vector::from_array(record.center) * Vector::from_array(scale);
            volume.owner = item;
            volume.storage_type = record.storage_type;
            volume.active = true;
            volume.only_manual = record.only_manual;
            volume.dont_lock_if_owner_picked_up = record.dont_lock_if_owner_picked_up;
            volume.usables.clear();
            volume
                .usables
                .extend(record.usables.iter().filter_map(|node| usable_item(*node)));
            volume.slots.clear();
            volume
                .slots
                .extend(record.slots.iter().filter_map(|source| slot(source)));
            volume.frame = Frame::Owner {
                local: root.inverse() * Pose::from_parts(center, record.rotation),
                record: record_index as u16,
            };
            // A non-unit placement scale needs its own box. Source boxes use unit scale.
            volume.shape = if unit {
                record.shape.clone()
            } else {
                let [x, y, z] = std::array::from_fn(|a| record.half_extents[a] * scale[a].abs());
                SharedShape::cuboid(x, y, z)
            };
            volume.gates_closed = false;
            volume.startup = START_FRAMES;
            volume.previous.clear();
            volume.current.clear();
        }
        Ok(())
    }
    /// Unbinds the world volumes and flags of a generated item that streams out or rolls back.
    /// Items locked to it leave with it if `leaves` allows and the owner is not removed.
    /// They keep their disabled body and are listed by `departing`. Other items unlock.
    /// Returns true if an item departs.
    pub fn unbind_owner(
        &mut self,
        owner: ItemId,
        leaves: impl Fn(&ItemStore, ItemId) -> bool,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<bool, ReferenceError> {
        self.dont_mount.remove(&owner);
        self.till_pickup.remove(&owner);
        self.drop_times.remove(&owner);
        let removed = items.get(owner).is_none_or(|o| o.removed);
        let buried = items.get(owner).is_some_and(|o| o.buried);
        let mut departs = false;
        let mut changed = false;
        for index in 0..self.volumes.len() {
            if matches!(self.volumes[index].frame, Frame::Free)
                || self.volumes[index].owner != owner
            {
                continue;
            }
            let volume = index as u16;
            while let Some(item) = self
                .locked
                .iter()
                .find(|l| l.volume == volume)
                .map(|l| l.item)
            {
                if !removed && leaves(items, item) {
                    self.release(item, false, items, physics)?;
                    self.departing.push(item);
                    departs = true;
                } else {
                    self.unlock(item, items, physics)?;
                    // Source mountStuff.Unstore: contents of a buried owner stay buried.
                    if buried {
                        super::buried::bury(item, items, physics);
                    }
                }
                changed = true;
            }
            let slot = &mut self.volumes[index];
            slot.frame = Frame::Free;
            slot.active = false;
            slot.usables.clear();
            slot.slots.clear();
            slot.previous.clear();
            slot.current.clear();
            self.free.push(volume);
        }
        if changed {
            physics.refresh_queries();
        }
        Ok(departs)
    }
    /// True for a generated item that left the lock with its owner and waits to unload.
    pub fn departing(&self, item: ItemId) -> bool {
        self.departing.contains(&item)
    }
    /// The stream unloaded a departing item.
    pub fn departed(&mut self, item: ItemId) {
        self.departing.retain(|i| *i != item);
    }
    /// The prototype volume source of a bound volume.
    pub fn volume_source(&self, volume: u16) -> Option<&SourceId> {
        match &self.volumes.get(volume as usize)?.frame {
            Frame::Owner { record, .. } => Some(&self.prototype_volumes[*record as usize].source),
            Frame::Free => None,
        }
    }
    /// The bound volume of `owner` with `source`.
    pub fn owner_volume(&self, owner: ItemId, source: &SourceId) -> Option<u16> {
        (0..self.volumes.len())
            .find(|&index| {
                self.volumes[index].owner == owner
                    && self.volume_source(index as u16) == Some(source)
            })
            .map(|index| index as u16)
    }
    pub fn stored(&self, item: ItemId) -> bool {
        self.locked.iter().any(|l| l.item == item)
    }
    /// The disabled own body of a locked item.
    pub fn locked_body(&self, item: ItemId) -> Option<RigidBodyHandle> {
        self.locked.iter().find(|l| l.item == item).map(|l| l.body)
    }
    /// Keep an item out of every volume.
    pub fn exclude(&mut self, item: ItemId) {
        self.dont_mount.insert(item);
    }
    pub fn mark_dropped(&mut self, item: ItemId) {
        self.drop_times.insert(item, self.elapsed);
    }
    /// Source `dontMountTillPickup = true`.
    pub fn hold_till_pickup(&mut self, item: ItemId) {
        self.till_pickup.insert(item);
    }
    pub fn picked_up(&mut self, item: ItemId) {
        self.till_pickup.remove(&item);
    }
    /// The part slots that gate bound volume `volume`. Each must hold a closed part.
    pub fn gate_slots(&self, volume: u16) -> &[PartSlotId] {
        &self.volumes[volume as usize].slots
    }
    fn gates_closed(volume: &Volume, parts: &PartAttachments, mechanisms: &MechanismState) -> bool {
        let open = |usable: ItemId| {
            mechanisms
                .value(usable, false)
                .is_some_and(|value| value > 0.1)
        };
        volume.slots.iter().all(|slot| {
            parts
                .slot_item(*slot)
                .is_some_and(|item| mechanisms.part_usable(item).is_none_or(|u| !open(u)))
        }) && !volume.usables.iter().any(|usable| open(*usable))
    }
    /// Source `CanStore` at setting 2, with owner presence.
    fn permitted(volume: &Volume, items: &ItemStore, physics: &PhysicsWorld) -> bool {
        volume.active
            && volume.gates_closed
            && crate::reference::storage::stores(volume.storage_type)
            && items.get(volume.owner).is_some_and(|owner| {
                !owner.removed
                    && owner.inactive_pose.is_none()
                    && !(volume.dont_lock_if_owner_picked_up
                        && owner.location == ItemLocation::Held)
                    && owner.body.is_some_and(|body| {
                        physics.body_active(body) && physics.rapier().bodies.get(body).is_some()
                    })
            })
    }
    /// The world pose of a bound volume box.
    fn pose(volume: &Volume, items: &ItemStore, physics: &PhysicsWorld) -> Option<Pose> {
        let Frame::Owner { local, .. } = &volume.frame else {
            return None;
        };
        let owner = items.get(volume.owner)?;
        let body = physics.rapier().bodies.get(owner.body?)?;
        Some(*body.position() * to_rapier(owner.local_pose) * *local)
    }
    /// The world pose of the box of bound volume `volume`.
    pub fn volume_pose(
        &self,
        volume: u16,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> Option<Pose> {
        Self::pose(self.volumes.get(volume as usize)?, items, physics)
    }
    /// Clears `current`, then fills it with the items that overlap the volume.
    fn scan_volume(
        volume: &mut Volume,
        items: &ItemStore,
        physics: &PhysicsWorld,
        mount_mask: u32,
    ) {
        volume.current.clear();
        let Some(pose) = Self::pose(volume, items, physics) else {
            return;
        };
        let predicate = |_, collider: &rapier3d::prelude::Collider| {
            collider.is_enabled()
                && collider.collision_groups().memberships.bits() & mount_mask != 0
        };
        let filter = QueryFilter::default().predicate(&predicate);
        for (_, collider) in physics
            .queries(filter)
            .intersect_shape(pose, volume.shape.as_ref())
        {
            if let Some(item) = collider_item(collider.user_data)
                && !volume.current.contains(&item)
            {
                volume.current.push(item);
            }
        }
    }
    /// Source `FreezeUpd` lock checks, without the two-scan rule.
    fn can_lock(
        &self,
        item: ItemId,
        owner: ItemId,
        occupied: Option<RigidBodyHandle>,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> bool {
        let Some(state) = items.get(item) else {
            return false;
        };
        let Some(carrier) = items.get(owner).and_then(|o| o.body) else {
            return false;
        };
        state.location == ItemLocation::World
            && !state.removed
            && !state.pending_attachment
            && state.inactive_pose.is_none()
            && !self.dont_mount.contains(&item)
            && !self.till_pickup.contains(&item)
            && self
                .drop_times
                .get(&item)
                .is_none_or(|time| self.elapsed - time >= self.drop_delay)
            && item != owner
            && !items.descends(owner, item)
            && !items.mounted(item)
            && state.body.is_some_and(|body| {
                body != carrier
                    && Some(body) != occupied
                    && physics.body_active(body)
                    && physics.vehicle_of(body).is_none()
                    && physics
                        .rapier()
                        .bodies
                        .get(body)
                        .is_some_and(|b| b.is_enabled() && b.is_dynamic())
            })
    }
    /// Adds mass to the carrier and to each locked owner body between them.
    fn add_mass(
        &mut self,
        owner: ItemId,
        carrier: RigidBodyHandle,
        mass: f32,
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        physics.add_cargo_mass(carrier, mass);
        let mut id = owner;
        while let Some(ItemLocation::Locked { owner: next, .. }) = items.get(id).map(|i| i.location)
        {
            if let Some(entry) = self.locked.iter_mut().find(|l| l.item == id) {
                entry.mass += mass;
                physics.add_cargo_mass(entry.body, mass);
            }
            id = next;
        }
    }
    /// Moves the item colliders, and the colliders of items locked to it, onto the owner carrier.
    /// Returns false if another loose item shares the item body.
    fn lock(
        &mut self,
        item: ItemId,
        volume: u16,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<bool, ReferenceError> {
        let owner = self.volumes[volume as usize].owner;
        let carrier = items
            .get(owner)
            .and_then(|i| i.body)
            .ok_or_else(|| error("Storage owner has no body"))?;
        let state = items
            .get(item)
            .ok_or_else(|| error("Missing storage item"))?;
        let body = state
            .body
            .ok_or_else(|| error("Storage item has no body"))?;
        let loose_local = to_rapier(state.local_pose);
        if items
            .iter()
            .any(|s| s.body == Some(body) && s.id != item && !items.descends(s.id, item))
        {
            return Ok(false);
        }
        let body_pose = *physics.rapier().bodies[body].position();
        let carrier_pose = *physics.rapier().bodies[carrier].position();
        physics.recompute_body_mass(body);
        let mass = physics.authored_body_mass(body);
        let flag = if physics.vehicle_of(carrier).is_some() {
            VEHICLE_COLLIDER_FLAG
        } else {
            0
        };
        self.colliders.clear();
        for handle in physics.rapier().bodies[body].colliders() {
            if collider_item(physics.rapier().colliders[*handle].user_data)
                .is_some_and(|id| id == item || items.descends(id, item))
            {
                self.colliders.push(*handle);
            }
        }
        for &handle in &self.colliders {
            let world = body_pose * local_pose(physics, handle);
            physics.set_collider_parent(handle, Some(carrier));
            physics.edit_collider(handle, |collider| {
                collider.set_position_wrt_parent(carrier_pose.inverse() * world);
                collider.user_data = (collider.user_data & !VEHICLE_COLLIDER_FLAG) | flag;
            });
        }
        physics.edit_body(body, |body| body.set_enabled(false));
        let rebase = carrier_pose.inverse() * body_pose;
        self.riders.clear();
        self.riders
            .extend(items.iter().filter(|s| s.body == Some(body)).map(|s| s.id));
        for &rider in &self.riders {
            let state = items.set_body(rider, Some(carrier)).unwrap();
            state.local_pose = from_rapier(&(rebase * to_rapier(state.local_pose)));
        }
        items.get_mut(item).unwrap().location = ItemLocation::Locked { owner, volume };
        self.add_mass(owner, carrier, mass, items, physics);
        self.locked.push(Locked {
            item,
            volume,
            body,
            loose_local,
            mass,
        });
        Ok(true)
    }
    /// The locked item pose relative to its disabled own body.
    pub fn loose_local(&self, item: ItemId) -> Option<Pose> {
        self.locked
            .iter()
            .find(|l| l.item == item)
            .map(|l| l.loose_local)
    }
    /// Source `Lock(_ignoreTime: true)` for a saved item: no drop delay and no two-scan rule.
    /// `owner_local` is the item pose relative to the owner. The owner must be locked first.
    pub fn lock_saved(
        &mut self,
        item: ItemId,
        volume: u16,
        owner_local: Pose,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let owner = self
            .volumes
            .get(volume as usize)
            .ok_or_else(|| error("Unknown storage volume"))?
            .owner;
        let owner_state = items
            .get(owner)
            .ok_or_else(|| error("Missing storage owner"))?;
        let carrier = owner_state
            .body
            .ok_or_else(|| error("Storage owner has no body"))?;
        let owner_pose =
            *physics.rapier().bodies[carrier].position() * to_rapier(owner_state.local_pose);
        let state = items
            .get(item)
            .ok_or_else(|| error("Missing storage item"))?;
        let body = state
            .body
            .ok_or_else(|| error("Storage item has no body"))?;
        if state.location != ItemLocation::World || body == carrier {
            return Err(error("Saved storage item is not loose"));
        }
        let body_pose = owner_pose * owner_local * to_rapier(state.local_pose).inverse();
        physics.edit_body(body, |body| body.set_position(body_pose, false));
        if !self.lock(item, volume, items, physics)? {
            return Err(error("Saved storage item shares its body"));
        }
        Ok(())
    }
    /// Returns the item to its own body with the carrier velocity. Its locked children stay locked to it.
    pub fn unlock(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.release(item, true, items, physics)
    }
    /// Moves the item colliders back to its own body at the same world pose.
    /// Without `enable`, the body stays disabled: the item unloads with its owner.
    fn release(
        &mut self,
        item: ItemId,
        enable: bool,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let index = self
            .locked
            .iter()
            .position(|l| l.item == item)
            .ok_or_else(|| error("Item is not stored"))?;
        let state = items
            .get(item)
            .ok_or_else(|| error("Missing storage item"))?;
        let ItemLocation::Locked { owner, .. } = state.location else {
            return Err(error("Item is not stored"));
        };
        let carrier = state.body.ok_or_else(|| error("Missing storage carrier"))?;
        let bodies = &physics.rapier().bodies;
        if !bodies.contains(carrier) || !bodies.contains(self.locked[index].body) {
            return Err(error("Storage body was removed before unlock"));
        }
        let item_pose = to_rapier(state.local_pose);
        let entry = self.locked.swap_remove(index);
        let carrier_pose = *physics.rapier().bodies[carrier].position();
        let body_pose = carrier_pose * item_pose * entry.loose_local.inverse();
        let velocity = physics.rapier().bodies[carrier].velocity_at_point(body_pose.translation);
        let angular = physics.rapier().bodies[carrier].angvel();
        self.add_mass(owner, carrier, -entry.mass, items, physics);
        self.colliders.clear();
        for handle in physics.rapier().bodies[carrier].colliders() {
            if collider_item(physics.rapier().colliders[*handle].user_data)
                .is_some_and(|id| id == item || items.descends(id, item))
            {
                self.colliders.push(*handle);
            }
        }
        for &handle in &self.colliders {
            let world = carrier_pose * local_pose(physics, handle);
            physics.set_collider_parent(handle, Some(entry.body));
            physics.edit_collider(handle, |collider| {
                collider.set_position_wrt_parent(body_pose.inverse() * world);
                collider.user_data &= !VEHICLE_COLLIDER_FLAG;
            });
        }
        physics.edit_body(entry.body, |body| {
            body.set_position(body_pose, enable);
            if enable {
                body.set_enabled(true);
                body.set_linvel(velocity, true);
                body.set_angvel(angular, true);
            }
        });
        let rebase = body_pose.inverse() * carrier_pose;
        self.riders.clear();
        self.riders.extend(
            items
                .iter()
                .filter(|s| s.body == Some(carrier) && (s.id == item || items.descends(s.id, item)))
                .map(|s| s.id),
        );
        for &rider in &self.riders {
            let state = items.set_body(rider, Some(entry.body)).unwrap();
            state.local_pose = from_rapier(&(rebase * to_rapier(state.local_pose)));
        }
        items.get_mut(item).unwrap().location = ItemLocation::World;
        if enable {
            self.drop_times.insert(item, self.elapsed);
        }
        Ok(())
    }
    pub fn initialization_ready(&self) -> bool {
        self.startup_frame == 5
    }

    /// Run the two source yields, then one QuickFreeze pass per loading frame.
    pub fn initialize_frame(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        parts: &PartAttachments,
        mechanisms: &MechanismState,
        occupied: Option<RigidBodyHandle>,
        dt: f32,
    ) -> Result<bool, ReferenceError> {
        if !dt.is_finite() || dt < 0.0 {
            return Err(error("Invalid storage initialization frame"));
        }
        if self.initialization_ready() {
            return Ok(false);
        }
        self.elapsed += dt;
        if self.startup_frame < 2 {
            self.startup_frame += 1;
            return Ok(false);
        }
        physics.refresh_queries();
        if self.startup_frame == 2 {
            for volume in &mut self.volumes {
                volume.startup = 0;
                Self::scan_volume(volume, items, physics, self.mount_mask);
                std::mem::swap(&mut volume.previous, &mut volume.current);
            }
        }
        self.set_gates(parts, mechanisms);
        let changed = self.scan(items, physics, occupied, true)?;
        self.startup_frame += 1;
        Ok(changed)
    }
    /// `occupied` is the body of the item seat of the player. It does not lock.
    pub fn advance(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        parts: &PartAttachments,
        mechanisms: &MechanismState,
        occupied: Option<RigidBodyHandle>,
        dt: f32,
    ) -> Result<bool, ReferenceError> {
        if !dt.is_finite() || dt < 0.0 {
            return Err(error("Invalid storage step"));
        }
        self.elapsed += dt;
        let mut changed = self.start_volumes(items, physics, parts, mechanisms, occupied)?;
        if dt > 0.0 && self.elapsed - self.last_scan <= self.scan_interval {
            return Ok(changed);
        }
        self.last_scan = self.elapsed;
        self.set_gates(parts, mechanisms);
        changed |= self.scan(items, physics, occupied, false)?;
        Ok(changed)
    }
    fn set_gates(&mut self, parts: &PartAttachments, mechanisms: &MechanismState) {
        for volume in &mut self.volumes {
            volume.gates_closed = Self::gates_closed(volume, parts, mechanisms);
        }
    }
    /// Source `StartFreeze` of volumes that bound after loading. `QuickFreeze` ignores `onlyManual`.
    fn start_volumes(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        parts: &PartAttachments,
        mechanisms: &MechanismState,
        occupied: Option<RigidBodyHandle>,
    ) -> Result<bool, ReferenceError> {
        let mut changed = false;
        for index in 0..self.volumes.len() {
            let volume = &mut self.volumes[index];
            if volume.startup == 0 || matches!(volume.frame, Frame::Free) {
                continue;
            }
            volume.startup -= 1;
            if volume.startup >= START_PASSES {
                continue;
            }
            volume.gates_closed = Self::gates_closed(volume, parts, mechanisms);
            // The first pass scans twice: QuickFreeze(_prescan: true).
            if volume.startup == START_PASSES - 1 {
                Self::scan_volume(volume, items, physics, self.mount_mask);
                std::mem::swap(&mut volume.previous, &mut volume.current);
            }
            changed |= self.upd_volume(index, items, physics, occupied)?;
        }
        if changed {
            physics.refresh_queries();
        }
        Ok(changed)
    }
    /// One `Upd` pass over every volume. Gate state is already set.
    /// `quick` is the source `QuickFreeze`, which also runs `onlyManual` volumes.
    fn scan(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        occupied: Option<RigidBodyHandle>,
        quick: bool,
    ) -> Result<bool, ReferenceError> {
        let mut changed = false;
        for index in 0..self.volumes.len() {
            if matches!(self.volumes[index].frame, Frame::Free)
                || (self.volumes[index].only_manual && !quick)
            {
                continue;
            }
            changed |= self.upd_volume(index, items, physics, occupied)?;
        }
        if changed {
            physics.refresh_queries();
        }
        Ok(changed)
    }
    /// Source `CanStore`, then `FreezeUpd` or `Unstore` for one volume.
    fn upd_volume(
        &mut self,
        index: usize,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        occupied: Option<RigidBodyHandle>,
    ) -> Result<bool, ReferenceError> {
        let mut changed = false;
        let volume = index as u16;
        let permitted = Self::permitted(&self.volumes[index], items, physics);
        if !permitted {
            while let Some(item) = self
                .locked
                .iter()
                .find(|l| l.volume == volume)
                .map(|l| l.item)
            {
                self.unlock(item, items, physics)?;
                changed = true;
            }
        }
        Self::scan_volume(&mut self.volumes[index], items, physics, self.mount_mask);
        if permitted {
            let owner = self.volumes[index].owner;
            for slot in 0..self.volumes[index].current.len() {
                let item = self.volumes[index].current[slot];
                if self.volumes[index].previous.contains(&item)
                    && self.can_lock(item, owner, occupied, items, physics)
                    && self.lock(item, volume, items, physics)?
                {
                    changed = true;
                }
            }
        }
        let volume = &mut self.volumes[index];
        std::mem::swap(&mut volume.previous, &mut volume.current);
        Ok(changed)
    }
}
/// Collider world poses update in the next query refresh. The parent pose is current.
fn local_pose(physics: &PhysicsWorld, handle: ColliderHandle) -> Pose {
    physics.rapier().colliders[handle]
        .position_wrt_parent()
        .copied()
        .unwrap_or(Pose::IDENTITY)
}
/// Collider user data low 32 bits hold the item id plus one. Zero is no item.
fn collider_item(user_data: u128) -> Option<ItemId> {
    let value = (user_data & u32::MAX as u128) as u32;
    value.checked_sub(1).map(ItemId)
}
fn error(message: &str) -> ReferenceError {
    ReferenceError(message.into())
}
