//! General parts own slot occupancy and reuse one collider set when moved.
use super::items::{ItemLocation, ItemStore, node_pose};
use super::mechanisms::MechanismState;
use super::{AimTarget, InteractionBlock, SlotTarget};
use crate::player::PlayerState;
use crate::reference::*;
use crate::vehicle::{PhysicsWorld, VEHICLE_COLLIDER_FLAG, from_rapier, to_rapier};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{ColliderHandle, RigidBodyBuilder, RigidBodyHandle};
use std::collections::HashMap;

struct Shape {
    id: ColliderId,
    handle: Option<ColliderHandle>,
    body: BodyId,
    shape: Pose,
    part_local: Pose,
    enabled: bool,
}
struct ScanShape {
    id: ColliderId,
    handle: Option<ColliderHandle>,
    node: Pose,
    local_node: Pose,
}
struct Part {
    scan_offset: Vector,
    reference: PartReference,
    slot: Option<usize>,
    body: Option<RigidBodyHandle>,
    shapes: Vec<Shape>,
    members: Vec<(ItemId, NodeId, f32)>,
    pickup: PickupReference,
    pose: Pose,
    mass: f32,
    assembly_dry_mass: f32,
    assembly_fluids: Vec<ItemId>,
}
struct Slot {
    scan_shapes: Vec<ScanShape>,
    reference: PartSlotReference,
    part: Option<usize>,
    mount_node: NodeId,
    detach_node: Option<NodeId>,
    compatible: Vec<ItemId>,
    mount_pose: Pose,
    detach_pose: Option<Pose>,
}
pub struct PartAttachments {
    scan_mask: u32,
    scan_radius: f32,
    parts: Vec<Part>,
    indices: HashMap<ItemId, usize>,
    slots: Vec<Slot>,
    slot_indices: HashMap<PartSlotId, usize>,
    collider_slots: HashMap<ColliderId, PartSlotId>,
    wheel_covers: HashMap<ItemId, PartSlotId>,
    bound: bool,
}
impl PartAttachments {
    pub fn restore_save(
        &mut self,
        ids: &crate::save::IdentityMap,
        saved: &[crate::save::PartAttachment],
        items: &mut ItemStore,
        mechanisms: &mut MechanismState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), crate::save::SaveError> {
        for p in &mut self.parts {
            p.slot = None;
        }
        for record in saved {
            let slot = self.slot_indices[&ids.part_slots[&record.slot]];
            let part = record
                .part
                .as_ref()
                .map(|id| ids.item(id).map(|id| self.indices[&id]))
                .transpose()?;
            self.slots[slot].part = part;
            if let Some(part) = part {
                self.parts[part].slot = Some(slot);
            }
        }
        for p in &self.parts {
            physics.edit_body(p.body.unwrap(), |body| body.set_enabled(p.slot.is_none()));
            if let Some(slot) = p.slot {
                mechanisms.mount_part(
                    p.reference.node,
                    self.slots[slot].mount_node,
                    p.reference.local_rotation_after_mount,
                );
            } else {
                mechanisms.set_part_root(p.reference.node, p.body);
            }
        }
        for i in 0..self.parts.len() {
            let body = self.physical_body(i, items);
            for &(id, _, _) in &self.parts[i].members {
                items.set_body(id, Some(body)).unwrap();
            }
        }
        self.cache_assembly_mass(items);
        mechanisms.sync(physics);
        self.capture_mechanisms(items, mechanisms, physics)?;
        Ok(())
    }
    pub fn slot_identity(
        &self,
        id: PartSlotId,
    ) -> Option<(Option<ItemId>, &crate::reference::SourceId)> {
        let slot = &self.slots[*self.slot_indices.get(&id)?].reference;
        Some((slot.owner, &slot.source))
    }
    pub fn resolve_slot(
        &self,
        owner: Option<ItemId>,
        source: &crate::reference::SourceId,
    ) -> Option<PartSlotId> {
        self.slots
            .iter()
            .find(|s| s.reference.owner == owner && &s.reference.source == source)
            .map(|s| s.reference.id)
    }

    pub(super) fn retained_binding_bytes(&self) -> usize {
        use super::memory::{map, strings, vector};
        vector(&self.parts)
            + map(&self.indices)
            + vector(&self.slots)
            + map(&self.slot_indices)
            + map(&self.collider_slots)
            + map(&self.wheel_covers)
            + self
                .parts
                .iter()
                .map(|p| {
                    let r = &p.reference;
                    r.source.file.capacity()
                        + strings(&r.main_types)
                        + strings(&r.part_types)
                        + vector(&r.colliders)
                        + vector(&r.enable_if_mounted)
                        + vector(&r.enable_if_dismounted)
                        + vector(&r.disable_if_mounted)
                        + vector(&r.disable_if_dismounted)
                        + vector(&p.shapes)
                        + vector(&p.members)
                        + p.pickup.source.file.capacity()
                        + vector(&p.assembly_fluids)
                })
                .sum::<usize>()
            + self
                .slots
                .iter()
                .map(|s| {
                    s.reference.source.file.capacity()
                        + strings(&s.reference.main_types)
                        + strings(&s.reference.part_types)
                        + vector(&s.reference.colliders)
                        + vector(&s.scan_shapes)
                        + vector(&s.compatible)
                })
                .sum::<usize>()
    }

    pub fn new(r: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let indices: HashMap<_, _> = r
            .parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.item, i))
            .collect();
        let slot_indices: HashMap<_, _> = r
            .part_slots
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id, i))
            .collect();
        let ancestry = |mut node: NodeId| {
            loop {
                if let Some(p) = r.parts.iter().position(|p| p.node == node) {
                    return Some(p);
                }
                node = r.nodes.iter().find(|n| n.id == node)?.parent?;
            }
        };
        let mut parts = Vec::new();
        for (i, p) in r.parts.iter().enumerate() {
            let item = r
                .items
                .iter()
                .find(|x| x.id == p.item)
                .ok_or_else(|| error("Missing part item"))?;
            let members: Vec<_> = r
                .items
                .iter()
                .filter(|x| ancestry(x.node) == Some(i))
                .map(|x| (x.id, x.node, x.mass_kg))
                .collect();
            let mut shapes = Vec::new();
            for id in &p.colliders {
                let c = r
                    .colliders
                    .iter()
                    .find(|c| c.id == *id)
                    .ok_or_else(|| error("Missing part collider"))?;
                shapes.push(Shape {
                    id: *id,
                    handle: None,
                    body: c.body,
                    shape: *crate::vehicle::collider_builder(&c.geometry)?
                        .build()
                        .position(),
                    part_local: Pose::IDENTITY,
                    enabled: c.enabled,
                });
            }
            parts.push(Part {
                scan_offset: to_rapier(node_pose(r, item.node)?)
                    .inverse()
                    .transform_point(to_rapier(node_pose(r, p.node)?).translation),
                reference: p.clone(),
                slot: r.part_slots.iter().position(|s| s.part == Some(p.item)),
                body: None,
                shapes,
                members,
                pickup: item
                    .pickup
                    .clone()
                    .ok_or_else(|| error("Part has no pickup"))?,
                pose: to_rapier(node_pose(r, p.node)?),
                mass: f32::NAN,
                assembly_dry_mass: 0.,
                assembly_fluids: Vec::with_capacity(r.tanks.len()),
            });
        }
        let mut slots = Vec::new();
        let mut collider_slots = HashMap::new();
        for s in &r.part_slots {
            let anchor = |id| {
                r.anchors
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| a.node)
                    .ok_or_else(|| error("Missing part anchor"))
            };
            for c in &s.colliders {
                collider_slots.insert(*c, s.id);
            }
            slots.push(Slot {
                scan_shapes: s
                    .colliders
                    .iter()
                    .map(|id| {
                        Ok(ScanShape {
                            id: *id,
                            handle: None,
                            node: to_rapier(node_pose(r, s.node)?),
                            local_node: Pose::IDENTITY,
                        })
                    })
                    .collect::<Result<_, ReferenceError>>()?,
                reference: s.clone(),
                part: s
                    .part
                    .map(|id| {
                        indices
                            .get(&id)
                            .copied()
                            .ok_or_else(|| error("Missing slot part"))
                    })
                    .transpose()?,
                mount_node: anchor(s.mount_anchor)?,
                detach_node: s.detach_anchor.map(anchor).transpose()?,
                mount_pose: Pose::IDENTITY,
                detach_pose: None,
                compatible: r
                    .parts
                    .iter()
                    .filter(|p| fits(&s.main_types, &s.part_types, &p.main_types, &p.part_types))
                    .map(|p| p.item)
                    .collect(),
            });
        }
        Ok(Self {
            scan_mask: r.part_scan.mask,
            scan_radius: r.part_scan.radius,
            parts,
            indices,
            slots,
            slot_indices,
            collider_slots,
            bound: false,
            wheel_covers: r
                .wheel_parts
                .iter()
                .filter_map(|p| p.cover_slot.map(|s| (p.item, s)))
                .collect(),
        })
    }
    pub fn capture_save(
        &self,
        ids: &crate::save::IdentityMap,
    ) -> Result<Vec<crate::save::PartAttachment>, crate::save::SaveError> {
        let mut records = self
            .slots
            .iter()
            .filter(|slot| ids.part_slots.values().any(|id| *id == slot.reference.id))
            .map(|slot| {
                Ok(crate::save::PartAttachment {
                    slot: crate::save::identity::source_key(&ids.part_slots, slot.reference.id)?,
                    part: slot
                        .part
                        .filter(|part| {
                            ids.items
                                .values()
                                .any(|id| *id == self.parts[*part].reference.item)
                        })
                        .map(|part| ids.item_instance(self.parts[part].reference.item))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, crate::save::SaveError>>()?;
        records.sort_by(|a, b| a.slot.cmp(&b.slot));
        Ok(records)
    }
    pub fn register_generated(
        &mut self,
        reference: PartReference,
        pickup: PickupReference,
        body: RigidBodyHandle,
        colliders: &[(ColliderId, ColliderHandle)],
        pose: crate::vehicle::VehiclePose,
        members: Vec<(ItemId, NodeId, f32)>,
        mass: f32,
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if self.indices.contains_key(&reference.item) {
            return Err(error("Generated part already bound"));
        }
        let item = reference.item;
        let world_pose = to_rapier(pose);
        let mut shapes = Vec::with_capacity(colliders.len());
        for &(id, handle) in colliders {
            let collider = &physics.rapier().colliders[handle];
            let local = world_pose.inverse() * *collider.position();
            shapes.push(Shape {
                id,
                handle: Some(handle),
                body: BodyId(u32::MAX),
                shape: local,
                part_local: local,
                enabled: collider.is_enabled(),
            });
        }
        for slot in &mut self.slots {
            if fits(
                &slot.reference.main_types,
                &slot.reference.part_types,
                &reference.main_types,
                &reference.part_types,
            ) {
                slot.compatible.push(item);
            }
        }
        self.indices.insert(item, self.parts.len());
        self.parts.push(Part {
            scan_offset: to_rapier(items.get(item).unwrap().pose)
                .inverse()
                .transform_point(world_pose.translation),
            reference,
            slot: None,
            body: Some(body),
            shapes,
            members,
            pickup,
            pose: world_pose,
            mass: f32::NAN,
            assembly_dry_mass: mass,
            assembly_fluids: Vec::with_capacity(1),
        });
        for &(_, handle) in colliders {
            physics.edit_collider(handle, |collider| collider.set_density(0.));
        }
        self.cache_assembly_mass(items);
        Ok(())
    }
    /// The collider is a shape of a bound part.
    pub fn owns_collider(&self, handle: ColliderHandle) -> bool {
        self.parts
            .iter()
            .any(|p| p.shapes.iter().any(|s| s.handle == Some(handle)))
    }
    pub fn set_generated_usable(&mut self, item: ItemId, usable: ItemId) {
        if let Some(i) = self.indices.get(&item) {
            self.parts[*i].reference.usable = Some(usable);
        }
    }
    pub fn has_external_dependents(&self, owns: impl Fn(ItemId) -> bool) -> bool {
        self.slots.iter().any(|slot| {
            slot.reference.owner.is_some_and(&owns)
                && slot
                    .part
                    .is_some_and(|part| !owns(self.parts[part].reference.item))
        }) || self.parts.iter().any(|part| {
            owns(part.reference.item) && part.members.iter().any(|(item, _, _)| !owns(*item))
        })
    }
    pub fn next_generated_slot(&self) -> Result<PartSlotId, ReferenceError> {
        self.slots
            .iter()
            .map(|s| s.reference.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .map(PartSlotId)
            .ok_or_else(|| error("Generated slot identity exhausted"))
    }
    pub fn register_generated_slot(
        &mut self,
        reference: PartSlotReference,
        mount_node: NodeId,
        detach_node: Option<NodeId>,
        shapes: &[(ColliderId, ColliderHandle)],
        mechanisms: &MechanismState,
        physics: &PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let node = to_rapier(
            mechanisms
                .node_pose(reference.node)
                .ok_or_else(|| error("Generated slot node missing"))?,
        );
        let mount_pose = to_rapier(
            mechanisms
                .node_pose(mount_node)
                .ok_or_else(|| error("Generated mount node missing"))?,
        );
        let mut scan_shapes = Vec::with_capacity(shapes.len());
        for &(id, handle) in shapes {
            scan_shapes.push(ScanShape {
                id,
                handle: Some(handle),
                node,
                local_node: physics.rapier().colliders[handle].position().inverse() * node,
            });
            self.collider_slots.insert(id, reference.id);
        }
        let compatible = self
            .parts
            .iter()
            .filter(|p| {
                fits(
                    &reference.main_types,
                    &reference.part_types,
                    &p.reference.main_types,
                    &p.reference.part_types,
                )
            })
            .map(|p| p.reference.item)
            .collect();
        self.slot_indices.insert(reference.id, self.slots.len());
        self.slots.push(Slot {
            scan_shapes,
            part: None,
            mount_node,
            detach_node,
            compatible,
            mount_pose,
            detach_pose: detach_node
                .and_then(|n| mechanisms.node_pose(n))
                .map(to_rapier),
            reference,
        });
        Ok(())
    }
    pub fn remove_generated_slot(&mut self, id: PartSlotId) {
        let Some(index) = self.slot_indices.remove(&id) else {
            return;
        };
        self.slots.remove(index);
        self.collider_slots.retain(|_, slot| *slot != id);
        self.wheel_covers.retain(|_, slot| *slot != id);
        for (i, slot) in self.slots.iter().enumerate() {
            self.slot_indices.insert(slot.reference.id, i);
        }
        for part in &mut self.parts {
            if let Some(slot) = part.slot {
                part.slot = if slot == index {
                    None
                } else {
                    Some(slot - usize::from(slot > index))
                };
            }
        }
    }
    pub fn set_generated_cover(&mut self, item: ItemId, slot: PartSlotId) {
        if let Some(&index) = self.indices.get(&item) {
            self.parts[index].reference.cover_slot = Some(slot);
        } else {
            self.wheel_covers.insert(item, slot);
        }
    }
    pub fn validate_saved_slot(
        &self,
        item: ItemId,
        slot: PartSlotId,
    ) -> Result<(), ReferenceError> {
        let valid = self
            .slot_indices
            .get(&slot)
            .is_some_and(|s| self.slots[*s].compatible.contains(&item));
        if valid {
            Ok(())
        } else {
            Err(error("Saved part does not fit source slot"))
        }
    }
    pub fn initialize_generated_slot(
        &mut self,
        item: ItemId,
        id: PartSlotId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let i = *self
            .indices
            .get(&item)
            .ok_or_else(|| error("Generated initial slot part missing"))?;
        let s = *self
            .slot_indices
            .get(&id)
            .ok_or_else(|| error("Generated initial slot missing"))?;
        if self.slots[s].part == Some(i) {
            return Ok(());
        }
        if self.slots[s].part.is_some() {
            return Err(error("Generated initial slot occupied"));
        }
        self.parts[i].slot = Some(s);
        self.slots[s].part = Some(i);
        physics.edit_body(self.parts[i].body.unwrap(), |body| body.set_enabled(false));
        items.get_mut(item).unwrap().location = ItemLocation::PartSlot(id);
        let old = to_rapier(items.get(item).unwrap().pose);
        let world = self.slots[s].mount_pose
            * Pose::from_rotation(rotation(self.parts[i].reference.local_rotation_after_mount));
        self.move_assembly(i, old, world, items, physics);
        self.cache_assembly_mass(items);
        self.sync(items, physics)?;
        Ok(())
    }
    pub fn remove_generated(&mut self, item: ItemId) {
        let Some(index) = self.indices.remove(&item) else {
            return;
        };
        self.parts.remove(index);
        for slot in &mut self.slots {
            slot.compatible.retain(|id| *id != item);
            slot.part = slot
                .part
                .and_then(|part| (part != index).then_some(part - usize::from(part > index)));
        }
        for (i, part) in self.parts.iter().enumerate() {
            self.indices.insert(part.reference.item, i);
        }
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        let shift = rapier3d::math::Vector::from_array(shift);
        for part in &mut self.parts {
            part.pose.translation -= shift;
        }
        for slot in &mut self.slots {
            slot.mount_pose.translation -= shift;
            if let Some(pose) = &mut slot.detach_pose {
                pose.translation -= shift;
            }
        }
    }
    pub fn bind(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if self.bound {
            return Ok(());
        }
        let handles: HashMap<_, _> = physics
            .rapier()
            .colliders
            .iter()
            .map(|(h, c)| (ColliderId((c.user_data >> 64) as u32), h))
            .collect();
        for p in &mut self.parts {
            let body = physics.insert_body(
                RigidBodyBuilder::dynamic()
                    .enabled(false)
                    .ccd_enabled(true)
                    .linear_damping(p.reference.linear_damping)
                    .angular_damping(p.reference.angular_damping),
            );
            p.body = Some(body);
            for shape in &mut p.shapes {
                let h = *handles
                    .get(&shape.id)
                    .ok_or_else(|| error("Missing part collider binding"))?;
                shape.handle = Some(h);
                let collider = &physics.rapier().colliders[h];
                shape.part_local = to_rapier(items.get(p.reference.item).unwrap().pose).inverse()
                    * *collider.position();
                // Source dry mass already includes these shapes.
                physics.edit_collider(h, |collider| collider.set_density(0.));
            }
            if let Some(slot) = p.slot {
                items.get_mut(p.reference.item).unwrap().location =
                    ItemLocation::PartSlot(self.slots[slot].reference.id);
            }
        }
        for slot in &mut self.slots {
            for shape in &mut slot.scan_shapes {
                let handle = *handles
                    .get(&shape.id)
                    .ok_or_else(|| error("Missing slot scan collider"))?;
                shape.handle = Some(handle);
                shape.local_node =
                    physics.rapier().colliders[handle].position().inverse() * shape.node;
            }
        }
        self.bound = true;
        self.cache_assembly_mass(items);
        Ok(())
    }
    /// Source partscript.Scan: sphere overlap, then nearest compatible collider transform.
    /// Cache bindings once and select in one pass instead of allocating and sorting each frame.
    pub fn scan(
        &self,
        item: ItemId,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> Option<AimTarget> {
        let Some(&index) = self.indices.get(&item) else {
            return None;
        };
        let Some(state) = items.get(item) else {
            return None;
        };
        let part = &self.parts[index];
        let center = to_rapier(state.pose).transform_point(part.scan_offset);
        let scale = if part.reference.scan_radius == 0. {
            1.
        } else {
            part.reference.scan_radius
        };
        let radius = self.scan_radius * scale;
        let mut nearest = f32::INFINITY;
        let mut selected = None;
        for slot in &self.slots {
            for shape in &slot.scan_shapes {
                let Some(handle) = shape.handle else {
                    continue;
                };
                let Some(collider) = physics.rapier().colliders.get(handle) else {
                    continue;
                };
                if !collider.is_enabled()
                    || collider.collision_groups().memberships.bits() & self.scan_mask == 0
                {
                    continue;
                }
                let closest = collider
                    .shape()
                    .project_point(collider.position(), center, true)
                    .point;
                if closest.distance_squared(center) > radius * radius {
                    continue;
                }
                let node = *collider.position() * shape.local_node;
                let distance = node.translation.distance_squared(center);
                if self.can_install(item, slot.reference.id, items).is_ok() && distance < nearest {
                    nearest = distance;
                    selected = Some(AimTarget {
                        collider: handle,
                        item,
                        distance: distance.sqrt(),
                        point: closest.to_array(),
                        slot: Some(SlotTarget::Part(slot.reference.id)),
                    });
                }
            }
        }
        selected
    }
    pub fn loose_body(&self, item: ItemId) -> Option<RigidBodyHandle> {
        self.parts
            .get(*self.indices.get(&item)?)
            .and_then(|p| p.body)
    }
    pub fn contains(&self, item: ItemId) -> bool {
        self.indices.contains_key(&item)
    }
    pub fn slot_target(&self, id: ColliderId) -> Option<PartSlotId> {
        self.collider_slots.get(&id).copied()
    }
    pub fn slot_item(&self, id: PartSlotId) -> Option<ItemId> {
        self.slots[*self.slot_indices.get(&id)?]
            .part
            .map(|p| self.parts[p].reference.item)
    }
    pub fn covering_item(&self, item: ItemId) -> Option<ItemId> {
        self.wheel_covers
            .get(&item)
            .copied()
            .and_then(|s| self.slot_item(s))
    }
    pub fn covered(&self, item: ItemId) -> bool {
        self.wheel_covers
            .get(&item)
            .copied()
            .or_else(|| {
                self.indices
                    .get(&item)
                    .and_then(|i| self.parts[*i].reference.cover_slot)
            })
            .is_some_and(|s| self.slot_item(s).is_some())
    }
    pub fn installed(&self, item: ItemId) -> bool {
        let Some(&i) = self.indices.get(&item) else {
            return false;
        };
        let Some(s) = self.parts[i].slot else {
            return false;
        };
        self.slots[s]
            .reference
            .owner
            .is_none_or(|owner| !self.contains(owner) || self.installed(owner))
    }
    pub fn can_install(
        &self,
        item: ItemId,
        id: PartSlotId,
        items: &ItemStore,
    ) -> Result<(), InteractionBlock> {
        let s = self
            .slots
            .get(
                *self
                    .slot_indices
                    .get(&id)
                    .ok_or(InteractionBlock::Unavailable)?,
            )
            .unwrap();
        if s.part.is_some() {
            return Err(InteractionBlock::Occupied);
        }
        if s.reference.cant_craft {
            return Err(InteractionBlock::Unavailable);
        }
        if !s.compatible.contains(&item) {
            return Err(InteractionBlock::Incompatible);
        }
        if self.covered(item) {
            return Err(InteractionBlock::Covered);
        }
        if items
            .get(item)
            .is_none_or(|i| !matches!(i.location, ItemLocation::World | ItemLocation::Held))
        {
            return Err(InteractionBlock::Unavailable);
        }
        Ok(())
    }
    pub fn can_remove(
        &self,
        item: ItemId,
        player: &PlayerState,
        items: &ItemStore,
    ) -> Result<(), InteractionBlock> {
        let p = &self.parts[*self
            .indices
            .get(&item)
            .ok_or(InteractionBlock::Unavailable)?];
        let s = &self.slots[p.slot.ok_or(InteractionBlock::Unavailable)?];
        if self.covered(item) {
            return Err(InteractionBlock::Covered);
        }
        if s.reference.cant_craft
            || s.reference.cant_uncraft
            || p.pickup.cant_pickup
            || p.pickup.cant_pickup_while_mounted
            || (player.seated() && p.pickup.cant_pickup_while_seated)
            || player
                .seat_item()
                .is_some_and(|seat| p.members.iter().any(|m| m.0 == seat))
            || items.get(item).is_none()
        {
            return Err(InteractionBlock::Unavailable);
        }
        Ok(())
    }
    /// True if `member` is one of the items of part `item`.
    pub fn holds(&self, item: ItemId, member: ItemId) -> bool {
        self.indices
            .get(&item)
            .is_some_and(|&i| self.parts[i].members.iter().any(|m| m.0 == member))
    }
    /// Mounted parts and their `fallOffCategory`.
    pub fn mounted(&self) -> impl Iterator<Item = (ItemId, u8)> + '_ {
        self.parts
            .iter()
            .filter(|p| p.slot.is_some())
            .map(|p| (p.reference.item, p.reference.fall_off_category))
    }
    /// Part and slot indices of a mounted part.
    fn mounted_indices(&self, item: ItemId) -> Result<(usize, usize), ReferenceError> {
        let i = *self
            .indices
            .get(&item)
            .ok_or_else(|| error("Unknown part"))?;
        let s = self.parts[i]
            .slot
            .ok_or_else(|| error("Part is not mounted"))?;
        Ok((i, s))
    }
    pub fn remove(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        let (i, s) = self.mounted_indices(item)?;
        if self.parts[i].pickup.cant_pickup
            || self.parts[i].pickup.cant_pickup_while_mounted
            || self.covered(item)
            || self.slots[s].reference.cant_craft
            || self.slots[s].reference.cant_uncraft
        {
            return Err(error("Cannot remove part"));
        }
        self.fall_off(item, items, physics)
    }
    /// Source `partslotscript.UnCraft(_fallOff: true)`: `remove` without the cover, craft
    /// and pickup checks.
    pub fn fall_off(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        let (i, s) = self.mounted_indices(item)?;
        if !self.bound {
            return Err(error("Cannot remove part"));
        }
        let mut world = self.slots[s].detach_pose.unwrap_or(to_rapier(
            items
                .get(item)
                .ok_or_else(|| error("Missing part state"))?
                .pose,
        ));
        if self.slots[s].detach_pose.is_some() {
            world.rotation *= rotation(self.parts[i].reference.local_rotation_after_mount);
        }
        if self.slots[s].reference.fixed_detach {
            world.translation = self.slots[s].mount_pose.translation
                + rapier3d::math::Vector::from_array(self.slots[s].reference.detach_position);
            world.rotation = rotation(self.slots[s].reference.detach_rotation);
        }
        let body = self.parts[i].body.unwrap();
        let parent = items.get(item).unwrap().body.unwrap();
        let velocity = physics.rapier().bodies[parent].velocity_at_point(world.translation);
        let angular = physics.rapier().bodies[parent].angvel();
        physics.edit_body(body, |b| {
            b.set_position(world, true);
            b.set_enabled(true);
            b.set_linvel(velocity, true);
            b.set_angvel(angular, true);
        });
        self.parts[i].pose = world;
        self.parts[i].slot = None;
        self.slots[s].part = None;
        let old = to_rapier(items.get(item).unwrap().pose);
        items.get_mut(item).unwrap().location = ItemLocation::World;
        self.move_assembly(i, old, world, items, physics);
        self.cache_assembly_mass(items);
        self.sync(items, physics)?;
        physics.refresh_queries();
        Ok(item)
    }
    pub fn install(
        &mut self,
        item: ItemId,
        id: PartSlotId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.can_install(item, id, items)
            .map_err(|_| error("Cannot install part"))?;
        if !self.bound || items.get(item).unwrap().location != ItemLocation::World {
            return Err(error("Release carry constraint before installation"));
        }
        let i = self.indices[&item];
        let s = self.slot_indices[&id];
        self.parts[i].slot = Some(s);
        self.slots[s].part = Some(i);
        physics.edit_body(self.parts[i].body.unwrap(), |body| body.set_enabled(false));
        items.get_mut(item).unwrap().location = ItemLocation::PartSlot(id);
        let old = to_rapier(items.get(item).unwrap().pose);
        let world = self.slots[s].mount_pose
            * Pose::from_rotation(rotation(self.parts[i].reference.local_rotation_after_mount));
        self.move_assembly(i, old, world, items, physics);
        self.cache_assembly_mass(items);
        self.sync(items, physics)?;
        physics.refresh_queries();
        Ok(())
    }
    fn move_assembly(
        &self,
        root: usize,
        old: Pose,
        pose: Pose,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        let body = self.physical_body(root, items);
        let delta = pose * old.inverse();
        let inverse = physics.rapier().bodies[body].position().inverse();
        for (i, p) in self.parts.iter().enumerate() {
            let mut parent = i;
            while parent != root {
                let Some(slot) = self.parts[parent].slot else {
                    break;
                };
                let Some(owner) = self.slots[slot]
                    .reference
                    .owner
                    .and_then(|id| self.indices.get(&id).copied())
                else {
                    break;
                };
                parent = owner;
            }
            if parent != root {
                continue;
            }
            for &(id, _, _) in &p.members {
                let state = items.set_body(id, Some(body)).unwrap();
                state.pose = from_rapier(&(delta * to_rapier(state.pose)));
                state.local_pose = from_rapier(&(inverse * to_rapier(state.pose)));
            }
            for shape in &p.shapes {
                let handle = shape.handle.unwrap();
                let local = inverse * delta * *physics.rapier().colliders[handle].position();

                physics.set_collider_parent(handle, Some(body));
                physics.edit_collider(handle, |collider| collider.set_position_wrt_parent(local));
                physics.edit_collider(handle, |collider| {
                    crate::vehicle::set_collider_enabled(
                        collider,
                        p.slot
                            .is_none_or(|s| !self.slots[s].reference.disable_colliders_on_craft)
                            && shape.enabled,
                    )
                });
            }
        }
    }
    pub fn detach_mechanism(
        &self,
        item: ItemId,
        m: &mut MechanismState,
        physics: &mut PhysicsWorld,
    ) {
        let p = &self.parts[self.indices[&item]];
        m.set_part_root(p.reference.node, Some(p.body.unwrap()));
        m.sync(physics);
        let rotation = p
            .reference
            .usable
            .and_then(|usable| m.uncraft(usable, p.reference.node));
        if let Some(rotation) = rotation {
            physics.edit_body(p.body.unwrap(), |body| body.set_rotation(rotation, true));
        }
        m.set_mounted(item, false);
    }
    /// Source partslotscript.SpawnPart crafts scene parts at start, so they take the mount rotation.
    pub fn mount_initial(&self, m: &mut MechanismState) {
        for p in &self.parts {
            if let Some(slot) = p.slot {
                m.mount_part(
                    p.reference.node,
                    self.slots[slot].mount_node,
                    p.reference.local_rotation_after_mount,
                );
            }
        }
    }
    pub fn mount_mechanism(&self, item: ItemId, m: &mut MechanismState) {
        let p = &self.parts[self.indices[&item]];
        let s = &self.slots[p.slot.unwrap()];
        m.mount_part(
            p.reference.node,
            s.mount_node,
            p.reference.local_rotation_after_mount,
        );
        m.set_mounted(item, true);
    }
    pub fn prepare_mechanisms(&mut self, items: &ItemStore, m: &mut MechanismState) {
        for p in &self.parts {
            if items
                .get(p.reference.item)
                .is_some_and(|item| item.inactive_pose.is_some())
            {
                continue;
            }
            if p.slot.is_none() {
                m.set_part_root(p.reference.node, p.body);
            }
        }
        for s in &self.slots {
            if let Some(owner) = s.reference.owner
                && !self.contains(owner)
                && let Some(item) = items.get(owner)
            {
                m.set_item_pose(owner, item.pose);
            }
        }
    }
    pub fn capture_mechanisms(
        &mut self,
        items: &mut ItemStore,
        m: &MechanismState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        for slot in &mut self.slots {
            slot.mount_pose = to_rapier(
                m.node_pose(slot.mount_node)
                    .ok_or_else(|| error("Missing slot mount"))?,
            );
            slot.detach_pose = slot.detach_node.and_then(|n| m.node_pose(n)).map(to_rapier);
        }
        for p in &mut self.parts {
            if items
                .get(p.reference.item)
                .is_some_and(|item| item.inactive_pose.is_some())
            {
                continue;
            }
            p.pose = to_rapier(
                m.node_pose(p.reference.node)
                    .ok_or_else(|| error("Missing part transform"))?,
            );
            for &(id, node, _) in &p.members {
                items.get_mut(id).unwrap().pose = m
                    .node_pose(node)
                    .ok_or_else(|| error("Missing member transform"))?;
            }
        }
        for i in 0..self.parts.len() {
            if items
                .get(self.parts[i].reference.item)
                .is_some_and(|item| item.inactive_pose.is_some())
            {
                continue;
            }
            let body = self.physical_body(i, items);
            let inverse = physics.rapier().bodies[body].position().inverse();
            for &(id, _, _) in &self.parts[i].members {
                let state = items.set_body(id, Some(body)).unwrap();
                state.local_pose = from_rapier(&(inverse * to_rapier(state.pose)));
            }
            for shape in &self.parts[i].shapes {
                let vehicle = physics.vehicle_of(body).is_some();
                let local = inverse
                    * m.body_pose(shape.body)
                        .map(|pose| pose * shape.shape)
                        .unwrap_or(self.parts[i].pose * shape.part_local);
                let h = shape.handle.unwrap();

                if physics.rapier().colliders[h].parent() != Some(body) {
                    physics.set_collider_parent(h, Some(body));
                }
                physics.edit_collider(h, |c| {
                    if crate::vehicle::pose_changed(c.position_wrt_parent(), &local) {
                        c.set_position_wrt_parent(local);
                    }
                    let enabled = self.parts[i]
                        .slot
                        .is_none_or(|s| !self.slots[s].reference.disable_colliders_on_craft)
                        && shape.enabled;
                    if c.is_enabled() != enabled {
                        crate::vehicle::set_collider_enabled(c, enabled);
                    }
                    if vehicle {
                        c.user_data |= VEHICLE_COLLIDER_FLAG;
                    } else {
                        c.user_data &= !VEHICLE_COLLIDER_FLAG;
                    }
                });
            }
        }
        self.sync(items, physics)
    }
    fn physical_body(&self, mut i: usize, items: &ItemStore) -> RigidBodyHandle {
        loop {
            let p = &self.parts[i];
            let Some(s) = p.slot else {
                return p.body.unwrap();
            };
            let Some(owner) = self.slots[s].reference.owner else {
                return p.body.unwrap();
            };
            if let Some(&parent) = self.indices.get(&owner) {
                i = parent;
            } else {
                return items.get(owner).and_then(|i| i.body).unwrap();
            }
        }
    }
    // Attachment transactions rebuild membership; fixed steps only read cached fluid items.
    fn cache_assembly_mass(&mut self, items: &ItemStore) {
        for p in &mut self.parts {
            p.assembly_dry_mass = 0.;
            p.assembly_fluids.clear();
        }
        for i in 0..self.parts.len() {
            let mut root = i;
            while let Some(slot) = self.parts[root].slot {
                let Some(parent) = self.slots[slot]
                    .reference
                    .owner
                    .and_then(|id| self.indices.get(&id).copied())
                else {
                    break;
                };
                root = parent;
            }
            if self.parts[root].slot.is_some() {
                continue;
            }
            for member in 0..self.parts[i].members.len() {
                let (id, _, dry) = self.parts[i].members[member];
                self.parts[root].assembly_dry_mass += dry;
                if items.get(id).is_some_and(|item| item.fluid.is_some()) {
                    self.parts[root].assembly_fluids.push(id);
                }
            }
        }
    }
    pub fn sync(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        for i in 0..self.parts.len() {
            if items
                .get(self.parts[i].reference.item)
                .is_some_and(|item| item.inactive_pose.is_some())
            {
                continue;
            }
            if self.parts[i].slot.is_some() {
                continue;
            }
            let body = self.parts[i]
                .body
                .ok_or_else(|| error("Part bodies are not bound"))?;
            let mass = (self.parts[i].assembly_dry_mass
                + self.parts[i]
                    .assembly_fluids
                    .iter()
                    .map(|id| {
                        items
                            .get(*id)
                            .unwrap()
                            .fluid
                            .as_ref()
                            .unwrap()
                            .finite_total()
                    })
                    .sum::<f32>())
            .max(0.001);
            if self.parts[i].mass != mass {
                physics.set_body_mass(body, mass);
                self.parts[i].mass = mass;
            }
        }
        Ok(())
    }
}
/// A slot with main types `a` and part types `b` takes a part with main types `c` and part types `d`.
pub(crate) fn fits(a: &[String], b: &[String], c: &[String], d: &[String]) -> bool {
    (a.is_empty() || c.is_empty() || a.iter().any(|x| c.contains(x)))
        && b.iter().any(|x| d.contains(x))
}
fn error(s: &str) -> ReferenceError {
    ReferenceError(s.into())
}

fn rotation(q: [f32; 4]) -> rapier3d::math::Rotation {
    rapier3d::math::Rotation::from_xyzw(q[0], q[1], q[2], q[3])
}
