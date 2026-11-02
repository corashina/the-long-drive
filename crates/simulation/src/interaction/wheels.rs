//! Source wheel parts and the wheel slots of every car.
//! A slot is one wheel of one car: a vehicle key and a wheel index.
//! Attachment changes reuse collider and item identities.

use super::items::{ItemLocation, ItemStore, node_pose};
use crate::reference::{
    ColliderId, ItemId, ItemKind, PartSlotId, ReferenceError, ReferenceManifest, Transform,
};
use crate::vehicle::{
    PhysicsWorld, SLOT_SENSOR_FLAG, VEHICLE_COLLIDER_FLAG, VehiclePose, WheelAssembly, from_rapier,
    to_rapier,
};
use crate::world::ObjectKey;
use rapier3d::math::{Pose, Rotation};
use rapier3d::prelude::{ColliderHandle, RigidBodyBuilder, RigidBodyHandle};
use std::collections::{HashMap, HashSet};

struct Part {
    item: ItemId,
    rim: bool,
    radius: f32,
    quality: f32,
    mass: f32,
    last_mass: f32,
    rotation: Rotation,
    tire_mount: Option<Pose>,
    tire: Option<usize>,
    collider_ids: Vec<ColliderId>,
    colliders: Vec<(ColliderHandle, Pose)>,
    free_body: Option<RigidBodyHandle>,
}
/// Runtime id of one wheel slot of a car copy. Ids are reused after a car retires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WheelSlotId(pub u16);

/// Slot sensor user data: bits 96 to 111 hold the wheel slot id.
pub const SLOT_ID_SHIFT: u32 = 96;

/// The wheel slot id in collider user data.
pub fn slot_id(user_data: u128) -> WheelSlotId {
    WheelSlotId((user_data >> SLOT_ID_SHIFT) as u16)
}

struct Slot {
    id: WheelSlotId,
    /// Index in `WheelAssemblies::vehicles`.
    vehicle: usize,
    /// Wheel index, local to the vehicle.
    index: usize,
    rim: Option<usize>,
    /// The rim object of this slot, mounted or not yet bound.
    rim_key: Option<ObjectKey>,
    /// The sensor item while the slot is empty.
    empty_item: ItemId,
    hub_to_mount: Pose,
    rim_main_types: Vec<String>,
    rim_types: Vec<String>,
    compatible: HashSet<ItemId>,
    last_support: Option<WheelAssembly>,
    sensor: Option<ColliderHandle>,
}
/// Chassis mass terms that this module owns for one vehicle.
struct VehicleMass {
    key: ObjectKey,
    /// The car root item. Slot state saves in the world change records.
    owner: ItemId,
    base_mass: f32,
    /// Prototype tank items. Source `massScript` adds one kilogram per liter.
    tanks: Vec<ItemId>,
    last_mass: f32,
}

/// One wheel slot of a world car, as the car binder reads it.
pub struct WorldWheelSlot {
    /// Source slot `maintipus` and `tipus`.
    pub rim_main_types: Vec<String>,
    pub rim_types: Vec<String>,
    /// Slot mount (`partslotscript.T`) relative to the resting hub, in the chassis frame.
    pub hub_to_mount: Pose,
    pub sensor_radius: f32,
    /// The rim object of the slot: the saved one or the prototype part.
    pub rim: Option<ObjectKey>,
}
#[derive(Default)]
pub struct WheelAssemblies {
    rules: Vec<crate::reference::WheelPartReference>,
    parts: Vec<Part>,
    indices: HashMap<ItemId, usize>,
    slots: Vec<Slot>,
    tire_fits: HashSet<(ItemId, ItemId)>,
    vehicles: Vec<VehicleMass>,
    generated_tanks: Vec<ItemId>,
    bound: bool,
    general_mass: Vec<(ItemId, f32)>,
    covers: HashMap<ItemId, PartSlotId>,
    /// Rims that `install_pending` mounted in its last call.
    installed: Vec<ItemId>,
}

impl WheelAssemblies {
    pub fn refresh_conditions(&mut self, conditions: &crate::conditions::ConditionStore) {
        for part in &mut self.parts {
            part.quality = conditions.item_quality(part.item).unwrap_or(1.);
        }
    }
    /// Rebuilds the imported wheel parts from restored item locations. A tire attached to a
    /// rim item rides on that rim. World car slots restore with their car.
    pub fn restore_save(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), crate::save::SaveError> {
        self.bind(items, physics)?;
        for part in &mut self.parts {
            if let Some(body) = part.free_body {
                physics.edit_body(body, |body| body.set_enabled(false));
            }
            part.tire = None;
        }
        for tire in 0..self.parts.len() {
            if let ItemLocation::Attached(rim) = items.get(self.parts[tire].item).unwrap().location
                && !self.parts[tire].rim
                && let Some(&rim) = self.indices.get(&rim)
            {
                self.parts[rim].tire = Some(tire);
            }
        }
        for i in 0..self.parts.len() {
            let state = items.get(self.parts[i].item).unwrap();
            if matches!(state.location, ItemLocation::World | ItemLocation::Held) {
                let body = self.free_body(i, Pose::IDENTITY, physics);
                let local = to_rapier(items.get(self.parts[i].item).unwrap().local_pose);
                self.place_part(i, body, local, true, items, physics)?;
            }
        }
        for i in 0..self.parts.len() {
            if let Some(tire) = self.parts[i].tire
                && let Some(body) = items.get(self.parts[i].item).unwrap().body
            {
                let local = to_rapier(items.get(self.parts[tire].item).unwrap().local_pose);
                self.place_part(
                    tire,
                    body,
                    local,
                    physics.vehicle_of(body).is_none(),
                    items,
                    physics,
                )?;
            }
        }
        self.sync(items, physics)?;
        Ok(())
    }
    /// A rim with a foreign tire, or a car with a foreign rim.
    pub fn has_external_dependents(&self, owns: impl Fn(ItemId) -> bool) -> bool {
        self.parts.iter().any(|part| {
            owns(part.item) && part.tire.is_some_and(|tire| !owns(self.parts[tire].item))
        }) || self.slots.iter().any(|slot| {
            owns(self.vehicles[slot.vehicle].owner)
                && slot.rim.is_some_and(|rim| !owns(self.parts[rim].item))
        })
    }
    /// The car root item of a world car slot.
    pub fn slot_owner(&self, id: WheelSlotId) -> Option<ItemId> {
        let slot = self.slots.iter().find(|slot| slot.id == id)?;
        Some(self.vehicles[slot.vehicle].owner)
    }
    pub fn retained_binding_bytes(&self) -> usize {
        use super::memory::{map, set, strings, vector};
        vector(&self.rules)
            + self
                .rules
                .iter()
                .map(|r| {
                    r.source.file.capacity()
                        + strings(&r.main_types)
                        + strings(&r.part_types)
                        + vector(&r.colliders)
                        + strings(&r.tire_main_types)
                        + strings(&r.tire_types)
                })
                .sum::<usize>()
            + vector(&self.parts)
            + self
                .parts
                .iter()
                .map(|p| vector(&p.collider_ids) + vector(&p.colliders))
                .sum::<usize>()
            + map(&self.indices)
            + vector(&self.slots)
            + self
                .slots
                .iter()
                .map(|s| set(&s.compatible) + strings(&s.rim_main_types) + strings(&s.rim_types))
                .sum::<usize>()
            + set(&self.tire_fits)
            + vector(&self.generated_tanks)
            + vector(&self.vehicles)
            + self
                .vehicles
                .iter()
                .map(|v| vector(&v.tanks))
                .sum::<usize>()
            + vector(&self.general_mass)
            + map(&self.covers)
            + vector(&self.installed)
    }

    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let mut parts = Vec::with_capacity(reference.wheel_parts.len());
        let mut indices = HashMap::with_capacity(reference.wheel_parts.len());
        let mut collider_ids = HashSet::new();
        for record in &reference.wheel_parts {
            if record.colliders.is_empty()
                || record.colliders.iter().any(|id| !collider_ids.insert(*id))
            {
                return Err(error("Missing or shared wheel part collider"));
            }
            let item = reference
                .items
                .iter()
                .find(|item| item.id == record.item)
                .ok_or_else(|| error("Missing wheel part item"))?;
            let rim = match item.kind {
                ItemKind::Rim => true,
                ItemKind::Tire => false,
                _ => return Err(error("Invalid wheel part kind")),
            };
            let tire_mount = if let Some(id) = record.tire_slot_anchor {
                let anchor = reference
                    .anchors
                    .iter()
                    .find(|anchor| anchor.id == id)
                    .ok_or_else(|| error("Missing tire mount anchor"))?;
                let body = reference
                    .bodies
                    .iter()
                    .find(|body| body.id == anchor.body)
                    .ok_or_else(|| error("Missing tire mount body"))?;
                Some(
                    to_rapier(node_pose(reference, item.node)?).inverse()
                        * pose(&body.pose)
                        * pose(&anchor.transform),
                )
            } else {
                None
            };
            indices.insert(item.id, parts.len());
            parts.push(Part {
                item: item.id,
                rim,
                radius: record.radius,
                quality: item.condition,
                mass: item.mass_kg,
                last_mass: f32::NAN,
                rotation: Rotation::from_array(record.local_rotation_after_mount),
                tire_mount,
                tire: None,
                collider_ids: record.colliders.clone(),
                colliders: Vec::with_capacity(record.colliders.len()),
                free_body: None,
            });
        }
        for item in &reference.items {
            if matches!(item.kind, ItemKind::Tire)
                && let Some(parent) = item.attached_to
                && let (Some(&rim), Some(&tire)) = (indices.get(&parent), indices.get(&item.id))
            {
                parts[rim].tire = Some(tire);
            }
        }
        let mut tire_fits = HashSet::new();
        for rim in &reference.wheel_parts {
            if !parts[indices[&rim.item]].rim {
                continue;
            }
            for tire in &reference.wheel_parts {
                if !parts[indices[&tire.item]].rim
                    && fits(
                        &rim.tire_main_types,
                        &rim.tire_types,
                        &tire.main_types,
                        &tire.part_types,
                    )
                {
                    tire_fits.insert((rim.item, tire.item));
                }
            }
        }
        Ok(Self {
            rules: reference.wheel_parts.clone(),
            parts,
            indices,
            slots: Vec::new(),
            tire_fits,
            vehicles: Vec::new(),
            generated_tanks: Vec::new(),
            general_mass: Vec::new(),
            covers: reference
                .wheel_parts
                .iter()
                .filter_map(|p| p.cover_slot.map(|slot| (p.item, slot)))
                .collect(),
            bound: false,
            installed: Vec::new(),
        })
    }

    pub fn set_generated_cover(&mut self, item: ItemId, slot: PartSlotId) {
        self.covers.insert(item, slot);
    }
    pub fn rules(&self) -> &[crate::reference::WheelPartReference] {
        &self.rules
    }
    pub fn register_generated(
        &mut self,
        rule: crate::reference::WheelPartReference,
        rim: bool,
        mass: f32,
        colliders: &[(ColliderId, ColliderHandle)],
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.bind(items, physics)?;
        if self.indices.contains_key(&rule.item) {
            return Err(error("Generated wheel already bound"));
        }
        let item = items
            .get(rule.item)
            .ok_or_else(|| error("Generated wheel item missing"))?;
        let inverse = to_rapier(item.pose).inverse();
        let shapes = colliders
            .iter()
            .map(|&(id, handle)| {
                let _ = id;
                (
                    handle,
                    inverse * *physics.rapier().colliders[handle].position(),
                )
            })
            .collect();
        for slot in &mut self.slots {
            if rim
                && fits(
                    &slot.rim_main_types,
                    &slot.rim_types,
                    &rule.main_types,
                    &rule.part_types,
                )
            {
                slot.compatible.insert(rule.item);
            }
        }
        for old in &self.rules {
            let old_rim = self.parts[self.indices[&old.item]].rim;
            if rim
                && !old_rim
                && fits(
                    &rule.tire_main_types,
                    &rule.tire_types,
                    &old.main_types,
                    &old.part_types,
                )
            {
                self.tire_fits.insert((rule.item, old.item));
            }
            if !rim
                && old_rim
                && fits(
                    &old.tire_main_types,
                    &old.tire_types,
                    &rule.main_types,
                    &rule.part_types,
                )
            {
                self.tire_fits.insert((old.item, rule.item));
            }
        }
        self.indices.insert(rule.item, self.parts.len());
        self.parts.push(Part {
            // `refresh_conditions` reads the store each step.
            quality: 1.,
            item: rule.item,
            rim,
            radius: rule.radius,
            mass,
            last_mass: f32::NAN,
            rotation: Rotation::from_array(rule.local_rotation_after_mount),
            tire_mount: rule.tire_mount.as_ref().map(pose),
            tire: None,
            collider_ids: rule.colliders.clone(),
            colliders: shapes,
            free_body: (item.location == ItemLocation::World)
                .then_some(item.body)
                .flatten(),
        });
        self.rules.push(rule);
        Ok(())
    }
    pub fn register_generated_general(&mut self, item: ItemId, mass: f32, fluid: bool) {
        self.general_mass.push((item, mass));
        if fluid {
            self.generated_tanks.push(item);
        }
    }
    pub fn remove_generated(&mut self, item: ItemId) {
        self.covers.remove(&item);
        self.general_mass.retain(|(id, _)| *id != item);
        self.generated_tanks.retain(|id| *id != item);
        let Some(index) = self.indices.remove(&item) else {
            return;
        };
        self.parts.remove(index);
        self.rules.retain(|r| r.item != item);
        self.tire_fits
            .retain(|(rim, tire)| *rim != item && *tire != item);
        for slot in &mut self.slots {
            slot.compatible.remove(&item);
            slot.rim = slot
                .rim
                .and_then(|rim| (rim != index).then_some(rim - usize::from(rim > index)));
        }
        for (i, part) in self.parts.iter_mut().enumerate() {
            self.indices.insert(part.item, i);
            if let Some(tire) = &mut part.tire {
                if *tire > index {
                    *tire -= 1;
                }
            }
        }
    }
    fn covered(&self, item: ItemId, items: &ItemStore) -> bool {
        self.covers.get(&item).is_some_and(|slot| {
            items
                .iter()
                .any(|state| state.location == ItemLocation::PartSlot(*slot))
        })
    }

    pub fn slot_for_item(&self, item: ItemId) -> Option<WheelSlotId> {
        self.slots
            .iter()
            .find(|slot| {
                slot.rim.is_some_and(|rim| {
                    self.parts[rim].item == item
                        || self.parts[rim]
                            .tire
                            .is_some_and(|tire| self.parts[tire].item == item)
                })
            })
            .map(|slot| slot.id)
    }
    pub fn installed(&self, slot: WheelSlotId) -> bool {
        self.slots
            .iter()
            .any(|candidate| candidate.id == slot && candidate.rim.is_some())
    }

    pub fn sensor(&self, slot: WheelSlotId) -> Option<ColliderHandle> {
        self.slots
            .iter()
            .find(|candidate| candidate.id == slot)
            .and_then(|slot| slot.sensor)
    }

    fn bind(
        &mut self,
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if self.bound {
            return Ok(());
        }
        let handles: HashMap<_, _> = physics
            .rapier()
            .colliders
            .iter()
            .filter_map(|(handle, collider)| {
                let id = (collider.user_data >> 64) as u32;
                (id != 0).then_some((ColliderId(id), handle))
            })
            .collect();
        for part in &mut self.parts {
            let item = items
                .get(part.item)
                .ok_or_else(|| error("Missing wheel item state"))?;
            let inverse = to_rapier(item.pose).inverse();
            if item.location == ItemLocation::World
                && let Some(body) = item.body
                && physics.rapier().bodies[body].is_dynamic()
            {
                part.free_body = Some(body);
            }
            for id in &part.collider_ids {
                let handle = *handles
                    .get(id)
                    .ok_or_else(|| error("Missing recovered wheel collider binding"))?;
                part.colliders.push((
                    handle,
                    inverse * *physics.rapier().colliders[handle].position(),
                ));
                physics.edit_collider(handle, |collider| {
                    collider.user_data =
                        (collider.user_data & !(u32::MAX as u128)) | (part.item.0 as u128 + 1);
                });
            }
        }
        for (handle, collider) in physics.rapier().colliders.iter() {
            if collider.user_data & SLOT_SENSOR_FLAG != 0 {
                let id = slot_id(collider.user_data);
                if let Some(slot) = self.slots.iter_mut().find(|slot| slot.id == id) {
                    slot.sensor = Some(handle);
                }
            }
        }
        self.bound = true;
        Ok(())
    }

    /// Mounted rims of the car with root item `owner`: slot, rim item and `fallOffCategory`.
    pub fn mounted_rims(
        &self,
        owner: ItemId,
    ) -> impl Iterator<Item = (WheelSlotId, ItemId, u8)> + '_ {
        self.slots.iter().filter_map(move |slot| {
            let rim = slot.rim?;
            (self.vehicles[slot.vehicle].owner == owner).then(|| {
                (
                    slot.id,
                    self.parts[rim].item,
                    self.rules[rim].fall_off_category,
                )
            })
        })
    }
    /// Slot and rim indices of a mounted wheel slot.
    fn mounted_indices(&self, id: WheelSlotId) -> Result<(usize, usize), ReferenceError> {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.id == id)
            .ok_or_else(|| error("Unknown wheel slot"))?;
        let rim = self.slots[slot]
            .rim
            .ok_or_else(|| error("Wheel slot is empty"))?;
        Ok((slot, rim))
    }
    pub fn remove(
        &mut self,
        id: WheelSlotId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        let (_, rim) = self.mounted_indices(id)?;
        if self.covered(self.parts[rim].item, items) {
            return Err(error("Remove the wheel cover first"));
        }
        self.fall_off(id, items, physics)
    }
    /// Source `partslotscript.UnCraft(_fallOff: true)`: `remove` without the cover check.
    pub fn fall_off(
        &mut self,
        id: WheelSlotId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        self.bind(items, physics)?;
        let (slot, rim) = self.mounted_indices(id)?;
        let world = item_pose(items, self.parts[rim].item, physics)?;
        let body = self.free_body(rim, world, physics);
        let chassis = physics.chassis_handle(&self.vehicles[self.slots[slot].vehicle].key);
        let velocity = physics.rapier().bodies[chassis].velocity_at_point(world.translation);
        let angular = physics.rapier().bodies[chassis].angvel();
        physics.edit_body(body, |body| body.set_linvel(velocity, true));
        physics.edit_body(body, |body| body.set_angvel(angular, true));
        self.place_part(rim, body, Pose::IDENTITY, true, items, physics)?;
        items.get_mut(self.parts[rim].item).unwrap().location = ItemLocation::World;
        if let Some(tire) = self.parts[rim].tire {
            let local = world.inverse() * item_pose(items, self.parts[tire].item, physics)?;
            self.place_part(tire, body, local, true, items, physics)?;
            items.get_mut(self.parts[tire].item).unwrap().location =
                ItemLocation::Attached(self.parts[rim].item);
        }
        self.slots[slot].rim = None;
        self.slots[slot].rim_key = None;
        self.update_body_mass(rim, body, items, physics);
        self.sync(items, physics)?;
        physics.refresh_queries();
        Ok(self.parts[rim].item)
    }

    pub fn can_install(
        &self,
        item: ItemId,
        id: WheelSlotId,
        items: &ItemStore,
    ) -> Result<(), ReferenceError> {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.id == id)
            .ok_or_else(|| error("Unknown wheel slot"))?;
        if self.slots[slot].rim.is_some() {
            return Err(error("Wheel slot is occupied"));
        }
        let rim = *self
            .indices
            .get(&item)
            .ok_or_else(|| error("Unknown rim"))?;
        if self.covered(item, items) {
            return Err(error("Remove the wheel cover first"));
        }
        if !self.parts[rim].rim || !self.slots[slot].compatible.contains(&item) {
            return Err(error("Rim does not fit wheel slot"));
        }
        if items
            .get(item)
            .is_none_or(|item| !matches!(item.location, ItemLocation::World | ItemLocation::Held))
        {
            return Err(error("Rim must be loose or held before installation"));
        }
        Ok(())
    }

    pub fn install(
        &mut self,
        item: ItemId,
        id: WheelSlotId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.bind(items, physics)?;
        self.can_install(item, id, items)?;
        if items
            .get(item)
            .is_none_or(|item| item.location != ItemLocation::World)
        {
            return Err(error("Release the carry constraint before installation"));
        }
        let slot = self.slots.iter().position(|slot| slot.id == id).unwrap();
        let rim = self.indices[&item];
        let key = &self.vehicles[self.slots[slot].vehicle].key;
        let chassis = physics.chassis_handle(key);
        let rim_pose = to_rapier(physics.wheel_pose(key, self.slots[slot].index))
            * self.slots[slot].hub_to_mount
            * Pose::from_rotation(self.parts[rim].rotation);
        let local = to_rapier(physics.snapshot(key).pose).inverse() * rim_pose;
        self.place_part(rim, chassis, local, false, items, physics)?;
        if let Some(tire) = self.parts[rim].tire {
            let tire_local = local
                * self.parts[rim].tire_mount.unwrap()
                * Pose::from_rotation(self.parts[tire].rotation);
            self.place_part(tire, chassis, tire_local, false, items, physics)?;
        }
        if let Some(body) = self.parts[rim].free_body {
            physics.edit_body(body, |body| body.set_enabled(false));
        }
        self.slots[slot].rim = Some(rim);
        self.slots[slot].rim_key = Some(items.object_key(item));
        items.get_mut(item).unwrap().location = ItemLocation::WheelSlot(id);
        self.sync(items, physics)?;
        physics.refresh_queries();
        Ok(())
    }

    pub fn can_split(&self, item: ItemId, items: &ItemStore) -> Result<ItemId, ReferenceError> {
        let rim = *self
            .indices
            .get(&item)
            .ok_or_else(|| error("Unknown rim"))?;
        if !self.parts[rim].rim
            || items.get(item).is_none_or(|item| {
                !matches!(
                    item.location,
                    ItemLocation::World | ItemLocation::WheelSlot(_)
                )
            })
        {
            return Err(error("Split requires a loose or mounted rim"));
        }
        let tire = self.parts[rim]
            .tire
            .ok_or_else(|| error("Rim has no tire"))?;
        Ok(self.parts[tire].item)
    }

    pub fn split(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(ItemId, ItemId), ReferenceError> {
        self.can_split(item, items)?;
        self.bind(items, physics)?;
        let rim = self.indices[&item];
        let tire = self.parts[rim].tire.unwrap();
        let world = item_pose(items, self.parts[tire].item, physics)?;
        let body = self.free_body(tire, world, physics);
        let rim_body = items
            .get(item)
            .unwrap()
            .body
            .ok_or_else(|| error("Missing loose rim body"))?;
        let velocity = physics.rapier().bodies[rim_body].velocity_at_point(world.translation);
        let angular = physics.rapier().bodies[rim_body].angvel();
        physics.edit_body(body, |body| body.set_linvel(velocity, true));
        physics.edit_body(body, |body| body.set_angvel(angular, true));
        self.place_part(tire, body, Pose::IDENTITY, true, items, physics)?;
        items.get_mut(self.parts[tire].item).unwrap().location = ItemLocation::World;
        self.parts[rim].tire = None;
        self.update_body_mass(rim, rim_body, items, physics);
        self.update_body_mass(tire, body, items, physics);
        self.sync(items, physics)?;
        physics.refresh_queries();
        Ok((item, self.parts[tire].item))
    }

    pub fn can_assemble(
        &self,
        rim_id: ItemId,
        tire_id: ItemId,
        items: &ItemStore,
    ) -> Result<(), ReferenceError> {
        if !self.tire_fits.contains(&(rim_id, tire_id)) {
            return Err(error("Tire does not fit rim"));
        }
        let rim = self.indices[&rim_id];
        if self.parts[rim].tire.is_some() {
            return Err(error("Rim already has a tire"));
        }
        if items
            .get(rim_id)
            .is_none_or(|i| !matches!(i.location, ItemLocation::World | ItemLocation::WheelSlot(_)))
            || items
                .get(tire_id)
                .is_none_or(|i| !matches!(i.location, ItemLocation::World | ItemLocation::Held))
        {
            return Err(error("Assembly requires loose parts"));
        }
        if self.parts[rim].tire_mount.is_none() || items.get(rim_id).unwrap().body.is_none() {
            return Err(error("Missing rim mount or body"));
        }
        Ok(())
    }

    pub fn assemble(
        &mut self,
        rim_id: ItemId,
        tire_id: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        self.can_assemble(rim_id, tire_id, items)?;
        if items.get(tire_id).unwrap().location != ItemLocation::World {
            return Err(error("Release the carry constraint before assembly"));
        }
        self.bind(items, physics)?;
        let rim = self.indices[&rim_id];
        let tire = self.indices[&tire_id];
        let body = items
            .get(rim_id)
            .unwrap()
            .body
            .ok_or_else(|| error("Missing rim body"))?;
        let local = to_rapier(items.get(rim_id).unwrap().local_pose)
            * self.parts[rim]
                .tire_mount
                .ok_or_else(|| error("Missing tire mount"))?
            * Pose::from_rotation(self.parts[tire].rotation);
        self.place_part(
            tire,
            body,
            local,
            physics.vehicle_of(body).is_none(),
            items,
            physics,
        )?;
        if let Some(old) = self.parts[tire].free_body
            && old != body
        {
            physics.edit_body(old, |body| body.set_enabled(false));
        }
        items.get_mut(tire_id).unwrap().location = ItemLocation::Attached(rim_id);
        self.parts[rim].tire = Some(tire);
        self.update_body_mass(rim, body, items, physics);
        self.sync(items, physics)?;
        physics.refresh_queries();
        Ok(rim_id)
    }

    fn free_body(
        &mut self,
        part: usize,
        world: Pose,
        physics: &mut PhysicsWorld,
    ) -> RigidBodyHandle {
        let handle = *self.parts[part].free_body.get_or_insert_with(|| {
            physics.insert_body(RigidBodyBuilder::dynamic().pose(world).ccd_enabled(true))
        });
        physics.edit_body(handle, |body| {
            body.set_enabled(true);
            body.set_position(world, true);
            body.reset_forces(true);
            body.reset_torques(true);
        });
        handle
    }
    fn place_part(
        &self,
        part: usize,
        body: RigidBodyHandle,
        local: Pose,
        enabled: bool,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let state = items
            .set_body(self.parts[part].item, Some(body))
            .ok_or_else(|| error("Missing wheel item"))?;
        state.local_pose = from_rapier(&local);
        state.pose = from_rapier(&(*physics.rapier().bodies[body].position() * local));
        let vehicle = physics.vehicle_of(body).is_some();
        for &(handle, pose) in &self.parts[part].colliders {
            physics.set_collider_parent(handle, Some(body));
            physics.edit_collider(handle, |collider| {
                collider.set_position_wrt_parent(local * pose);
                crate::vehicle::set_collider_enabled(collider, enabled);
                collider.set_density(0.0);
                if vehicle {
                    collider.user_data |= VEHICLE_COLLIDER_FLAG;
                } else {
                    collider.user_data &= !VEHICLE_COLLIDER_FLAG;
                }
            });
        }
        Ok(())
    }
    // This module owns complete loose-wheel mass, including general part descendants.
    // The source member list is cached at construction; transactions move their body ownership.
    fn update_body_mass(
        &mut self,
        rim: usize,
        body: RigidBodyHandle,
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        if physics.vehicle_of(body).is_some() {
            return;
        }
        let mass = self.parts[rim].mass
            + self.parts[rim]
                .tire
                .map_or(0.0, |tire| self.parts[tire].mass)
            + self
                .general_mass
                .iter()
                .filter_map(|(id, dry)| {
                    let item = items.get(*id)?;
                    (item.body == Some(body))
                        .then(|| dry + item.fluid.as_ref().map_or(0., |f| f.finite_total()))
                })
                .sum::<f32>();
        if self.parts[rim].last_mass != mass {
            physics.set_body_mass(body, mass);
            self.parts[rim].last_mass = mass;
        }
    }

    /// Mass of a wheel part.
    pub fn part_mass(&self, item: ItemId) -> Option<f32> {
        self.indices.get(&item).map(|&part| self.parts[part].mass)
    }

    /// Adds the wheel slots of world car `key`, one per wheel in wheel order.
    /// `base_mass` is the dry chassis mass without wheel parts. `tanks` add their fluid.
    /// `saved` replaces the slot rims. Returns the slot ids.
    #[allow(clippy::too_many_arguments)]
    pub fn add_vehicle(
        &mut self,
        key: ObjectKey,
        owner: ItemId,
        base_mass: f32,
        tanks: Vec<ItemId>,
        slots: &[WorldWheelSlot],
        saved: Option<Vec<Option<ObjectKey>>>,
        physics: &mut PhysicsWorld,
    ) -> Vec<WheelSlotId> {
        let vehicle = self.vehicles.len();
        self.vehicles.push(VehicleMass {
            key,
            owner,
            base_mass,
            tanks,
            last_mass: f32::NAN,
        });
        let mut ids = Vec::with_capacity(slots.len());
        let mut saved = saved.map(Vec::into_iter);
        for (index, slot) in slots.iter().enumerate() {
            let rim = match &mut saved {
                Some(rims) => rims.next().flatten(),
                None => slot.rim.clone(),
            };
            let mut id = 0;
            while self.slots.iter().any(|s| s.id.0 == id) {
                id += 1;
            }
            let id = WheelSlotId(id);
            let sensor = physics.insert_collider(
                rapier3d::prelude::ColliderBuilder::ball(slot.sensor_radius)
                    .sensor(true)
                    .enabled(false)
                    .density(0.0)
                    .user_data(
                        VEHICLE_COLLIDER_FLAG
                            | SLOT_SENSOR_FLAG
                            | ((id.0 as u128) << SLOT_ID_SHIFT)
                            | (owner.0 as u128 + 1),
                    ),
            );
            let compatible = self
                .rules
                .iter()
                .filter(|rule| {
                    self.parts[self.indices[&rule.item]].rim
                        && fits(
                            &slot.rim_main_types,
                            &slot.rim_types,
                            &rule.main_types,
                            &rule.part_types,
                        )
                })
                .map(|rule| rule.item)
                .collect();
            self.slots.push(Slot {
                id,
                vehicle,
                index,
                rim: None,
                rim_key: rim,
                empty_item: owner,
                hub_to_mount: slot.hub_to_mount,
                rim_main_types: slot.rim_main_types.clone(),
                rim_types: slot.rim_types.clone(),
                compatible,
                last_support: None,
                sensor: Some(sensor),
            });
            ids.push(id);
        }
        ids
    }

    /// Removes the slots and sensors of world car `key`. Returns the rim object of each wheel,
    /// and the slot ids with their old sensors. The mounted parts stay registered: the car
    /// content removes them.
    #[allow(clippy::type_complexity)]
    pub fn retire_vehicle(
        &mut self,
        key: &ObjectKey,
        physics: &mut PhysicsWorld,
    ) -> Option<(
        Vec<Option<ObjectKey>>,
        Vec<(WheelSlotId, Option<ColliderHandle>)>,
    )> {
        let vehicle = self.vehicles.iter().position(|v| &v.key == key)?;
        let mut rims = Vec::new();
        let mut ids = Vec::new();
        let mut i = 0;
        while i < self.slots.len() {
            if self.slots[i].vehicle != vehicle {
                i += 1;
                continue;
            }
            let slot = self.slots.remove(i);
            if let Some(sensor) = slot.sensor {
                physics.remove_collider(sensor);
            }
            if rims.len() <= slot.index {
                rims.resize(slot.index + 1, None);
            }
            rims[slot.index] = slot.rim_key;
            ids.push((slot.id, slot.sensor));
        }
        self.vehicles.remove(vehicle);
        for slot in &mut self.slots {
            if slot.vehicle > vehicle {
                slot.vehicle -= 1;
            }
        }
        Some((rims, ids))
    }

    /// The rim object of each wheel of every world car.
    pub fn world_slots(&self) -> impl Iterator<Item = (&ObjectKey, Vec<Option<ObjectKey>>)> + '_ {
        self.vehicles.iter().enumerate().map(|(vehicle, v)| {
            let mut rims = Vec::new();
            for slot in self.slots.iter().filter(|s| s.vehicle == vehicle) {
                if rims.len() <= slot.index {
                    rims.resize(slot.index + 1, None);
                }
                rims[slot.index] = slot.rim_key.clone();
            }
            (&v.key, rims)
        })
    }

    /// Mounts the rim object of each empty world car slot when its item is bound.
    /// A rim that cannot mount at that time leaves the slot. Returns true if a rim mounted.
    pub fn install_pending(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<bool, ReferenceError> {
        self.installed.clear();
        for slot in 0..self.slots.len() {
            if self.slots[slot].rim.is_some() {
                continue;
            }
            let Some(item) = self.slots[slot]
                .rim_key
                .as_ref()
                .and_then(|key| items.resolve_key(key))
            else {
                continue;
            };
            let id = self.slots[slot].id;
            if items
                .get(item)
                .is_some_and(|state| state.location == ItemLocation::World)
                && self.can_install(item, id, items).is_ok()
            {
                self.install(item, id, items, physics)?;
                self.installed.push(item);
            } else {
                self.slots[slot].rim_key = None;
            }
        }
        Ok(!self.installed.is_empty())
    }

    /// The collider is a shape of a rim or tire mounted on a car. Mounted wheel shapes stay off.
    pub fn mounted_collider(&self, handle: ColliderHandle) -> bool {
        let owns = |part: usize| self.parts[part].colliders.iter().any(|(h, _)| *h == handle);
        self.slots
            .iter()
            .filter_map(|slot| slot.rim)
            .any(|rim| owns(rim) || self.parts[rim].tire.is_some_and(owns))
    }

    /// Rims that the last `install_pending` call mounted.
    pub fn last_installed(&self) -> &[ItemId] {
        &self.installed
    }

    /// The sensor of each slot and its mounted rim.
    pub fn sensor_rims(&self) -> impl Iterator<Item = (ColliderHandle, ItemId)> + '_ {
        self.slots
            .iter()
            .filter_map(|slot| Some((slot.sensor?, self.parts[slot.rim?].item)))
    }

    pub fn sync_activity(&mut self, physics: &mut PhysicsWorld) {
        for slot in &self.slots {
            let active =
                physics.body_active(physics.chassis_handle(&self.vehicles[slot.vehicle].key));
            if let Some(handle) = slot.sensor {
                physics.edit_collider(handle, |collider| collider.set_enabled(active));
            }
        }
    }

    pub fn sync(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.bind(items, physics)?;
        self.sync_activity(physics);
        for vehicle in 0..self.vehicles.len() {
            self.sync_vehicle(vehicle, items, physics)?;
        }
        for index in 0..self.parts.len() {
            if let Some(item) = items.get(self.parts[index].item)
                && matches!(item.location, ItemLocation::World | ItemLocation::Held)
                && let Some(body) = item.body
                && physics.body_active(body)
            {
                self.update_body_mass(index, body, items, physics);
            }
        }
        for vehicle in 0..self.vehicles.len() {
            let state = &self.vehicles[vehicle];
            let chassis = physics.chassis_handle(&state.key);
            let mut mass = state.base_mass;
            for slot in self.slots.iter().filter(|slot| slot.vehicle == vehicle) {
                if let Some(rim) = slot.rim {
                    mass += self.parts[rim].mass
                        + self.parts[rim]
                            .tire
                            .map_or(0.0, |tire| self.parts[tire].mass);
                }
            }
            for &(id, dry) in &self.general_mass {
                // Storage adds the mass of locked items.
                if items.get(id).is_some_and(|i| {
                    i.body == Some(chassis) && !matches!(i.location, ItemLocation::Locked { .. })
                }) {
                    mass += dry;
                }
            }
            for id in self.generated_tanks.iter().chain(&state.tanks) {
                if items.get(*id).is_some_and(|i| {
                    i.body != Some(chassis) || matches!(i.location, ItemLocation::Locked { .. })
                }) {
                    continue;
                }
                mass += items
                    .get(*id)
                    .and_then(|item| item.fluid.as_ref())
                    .ok_or_else(|| error("Missing vehicle tank"))?
                    .finite_total();
            }
            if !state.last_mass.is_finite() || (mass - state.last_mass).abs() > 0.0001 {
                physics.set_vehicle_mass(&state.key, mass);
                self.vehicles[vehicle].last_mass = mass;
            }
        }
        Ok(())
    }

    /// Moves mounted wheels with the suspension and updates wheel supports.
    fn sync_vehicle(
        &mut self,
        vehicle: usize,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let key = &self.vehicles[vehicle].key;
        if !physics.body_active(physics.chassis_handle(key)) {
            return Ok(());
        }
        let snapshot = physics.snapshot(key);
        let chassis = physics.chassis_handle(key);

        let inverse = to_rapier(snapshot.pose).inverse();
        for slot in self.slots.iter_mut().filter(|slot| slot.vehicle == vehicle) {
            let support = if let Some(rim) = slot.rim {
                let part = &self.parts[rim];
                let rim_pose = to_rapier(physics.wheel_pose(key, slot.index))
                    * slot.hub_to_mount
                    * Pose::from_rotation(part.rotation);
                let state = items
                    .set_body(part.item, Some(chassis))
                    .ok_or_else(|| error("Missing mounted rim"))?;
                state.local_pose = from_rapier(&(inverse * rim_pose));
                state.pose = from_rapier(&rim_pose);
                if let Some(tire) = part.tire {
                    let tire_part = &self.parts[tire];
                    let tire_pose = rim_pose
                        * part
                            .tire_mount
                            .ok_or_else(|| error("Missing rim tire mount"))?
                        * Pose::from_rotation(tire_part.rotation);
                    let tire_state = items
                        .set_body(tire_part.item, Some(chassis))
                        .ok_or_else(|| error("Missing mounted tire"))?;
                    tire_state.local_pose = from_rapier(&(inverse * tire_pose));
                    tire_state.pose = from_rapier(&tire_pose);
                    tire_state.location = ItemLocation::Attached(part.item);
                    // Source wheelscript mass includes the mounted tire.
                    WheelAssembly::Tire {
                        radius_m: tire_part.radius,
                        mass_kg: part.mass + tire_part.mass,
                        quality: tire_part.quality,
                    }
                } else {
                    WheelAssembly::Rim {
                        radius_m: part.radius,
                        mass_kg: part.mass,
                    }
                }
            } else {
                WheelAssembly::Hub
            };
            if slot.last_support != Some(support) {
                physics.set_wheel(key, slot.index, support);
                slot.last_support = Some(support);
            }
            if let Some(handle) = slot.sensor {
                let world = to_rapier(physics.wheel_pose(key, slot.index)) * slot.hub_to_mount;
                physics.edit_collider(handle, |collider| {
                    if !collider.is_enabled() {
                        collider.set_enabled(true);
                    }
                    let item = slot.rim.map_or(slot.empty_item, |rim| self.parts[rim].item);
                    let mut data = (collider.user_data & !(SLOT_SENSOR_FLAG | u32::MAX as u128))
                        | (item.0 as u128 + 1);
                    if slot.rim.is_none() {
                        data |= SLOT_SENSOR_FLAG;
                    }
                    if collider.user_data != data {
                        collider.user_data = data;
                    }
                    if crate::vehicle::pose_changed(Some(collider.position()), &world) {
                        collider.set_position(world);
                    }
                });
            }
        }

        Ok(())
    }
}

pub(super) fn fits(
    slot_main: &[String],
    slot_types: &[String],
    part_main: &[String],
    part_types: &[String],
) -> bool {
    (slot_main.is_empty()
        || part_main.is_empty()
        || slot_main.iter().any(|tag| part_main.contains(tag)))
        && slot_types.iter().any(|tag| part_types.contains(tag))
}
fn pose(transform: &Transform) -> Pose {
    to_rapier(VehiclePose {
        position: transform.translation,
        rotation: transform.rotation,
    })
}
fn item_pose(
    items: &ItemStore,
    id: ItemId,
    physics: &PhysicsWorld,
) -> Result<Pose, ReferenceError> {
    let item = items.get(id).ok_or_else(|| error("Missing wheel item"))?;
    Ok(item
        .body
        .and_then(|handle| physics.rapier().bodies.get(handle))
        .map_or(to_rapier(item.pose), |body| {
            *body.position() * to_rapier(item.local_pose)
        }))
}
fn error(message: &str) -> ReferenceError {
    ReferenceError(message.into())
}
