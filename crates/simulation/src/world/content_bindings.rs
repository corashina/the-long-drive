//! Generated item bindings belong to their published source instance.
use super::WorldStream;
use crate::interaction::{
    InteractionState,
    items::{HitSound, ItemStore},
};
use crate::reference::{ActionBinding, PickupReference, ReferenceError, ReferenceManifest};
use crate::vehicle::{PhysicsWorld, STEP_SURFACE_MASK, from_rapier, to_rapier};
use crate::world::spawn_rules::*;
use crate::world::{ObjectKey, changes};
use rapier3d::prelude::ActiveEvents;
impl WorldStream {
    pub fn initialize_imported_changes(
        &mut self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &InteractionState,
        physics: &PhysicsWorld,
    ) {
        if self.imported_baselines.is_empty() {
            self.imported_baselines
                .extend(items.iter().filter_map(|item| {
                    crate::world::changes::capture(item.id, items, interaction, physics, reference)
                        .map(|s| (item.id, s))
                }));
        }
    }
    pub fn capture_changes(
        &mut self,
        reference: &ReferenceManifest,
        items: &mut ItemStore,
        interaction: &InteractionState,
        physics: &PhysicsWorld,
    ) {
        while let Some(id) = items.take_dirty() {
            if self
                .contents
                .active
                .iter()
                .chain(&self.static_controls)
                .any(|c| {
                    !c.restored && (c.item == Some(id) || c.bindings.iter().any(|b| b.item == id))
                })
            {
                continue;
            }
            let Some(mut snapshot) =
                crate::world::changes::capture(id, items, interaction, physics, reference)
            else {
                continue;
            };
            snapshot.lamp = self
                .contents
                .active
                .iter()
                .find(|c| c.item == Some(id))
                .and_then(|c| c.content.portable_lamp());
            let key = items.object_key(id);
            // An unbound dial keeps its saved value.
            snapshot.dial = self.dial_value(id).unwrap_or_else(|| {
                self.changes
                    .get(&key)
                    .and_then(|c| c.snapshot.as_ref())
                    .and_then(|s| s.dial)
            });
            // An unbound bottle or lamp keeps its saved value.
            (snapshot.ammo, snapshot.glow) = self.small_item_values(id).unwrap_or_else(|| {
                let saved = self.changes.get(&key).and_then(|c| c.snapshot.as_ref());
                (saved.and_then(|s| s.ammo), saved.and_then(|s| s.glow))
            });
            // An unbound gun keeps its saved state.
            if interaction.guns.runtime(id).is_none() {
                snapshot.gun = self
                    .changes
                    .get(&key)
                    .and_then(|c| c.snapshot.as_ref())
                    .and_then(|s| s.gun);
            }
            snapshot.radio = self.radio_value(id).or_else(|| {
                self.changes
                    .get(&key)
                    .and_then(|c| c.snapshot.as_ref())
                    .and_then(|s| s.radio)
            });
            // An unbound loot tool keeps its saved state.
            snapshot.tool = self.tool_value(id, interaction).unwrap_or_else(|| {
                self.changes
                    .get(&key)
                    .and_then(|c| c.snapshot.as_ref())
                    .and_then(|s| s.tool)
            });
            snapshot.detector = self.detector_value(id).or_else(|| {
                self.changes
                    .get(&key)
                    .and_then(|c| c.snapshot.as_ref())
                    .and_then(|s| s.detector)
            });
            if items.get(id).and_then(|i| i.body).is_some_and(|b| {
                physics.rapier().bodies[b].body_type()
                    == rapier3d::prelude::RigidBodyType::KinematicPositionBased
            }) && items.get(id).is_some_and(|i| !i.buried)
                && self.contents.active.iter().any(|c| c.item == Some(id))
            {
                if let Some(previous) = self.changes.get(&key).and_then(|c| c.snapshot.as_ref()) {
                    snapshot.linear_velocity = previous.linear_velocity;
                    snapshot.angular_velocity = previous.angular_velocity;
                    snapshot.sleeping = previous.sleeping;
                }
            }
            let baseline = match &key {
                crate::world::ObjectKey::Imported(_) => self
                    .imported_baselines
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map(|(_, s)| s),
                crate::world::ObjectKey::Generated(_) => self
                    .contents
                    .active
                    .iter()
                    .chain(&self.static_controls)
                    .find_map(|c| {
                        if c.item == Some(id) {
                            c.baseline.as_ref()
                        } else {
                            c.bindings
                                .iter()
                                .find(|b| b.item == id)
                                .map(|b| &b.baseline)
                        }
                    })
                    .or_else(|| {
                        self.supplies.iter().find_map(|s| {
                            s.items
                                .zip(s.baselines.as_ref())
                                .and_then(|((tank, cap), b)| {
                                    if id == tank {
                                        Some(&b[0])
                                    } else if id == cap {
                                        Some(&b[1])
                                    } else {
                                        None
                                    }
                                })
                        })
                    }),
            };
            let Some(baseline) = baseline else {
                continue;
            };
            if items.get(id).is_some_and(|item| item.removed) {
                self.changes.remove(key.clone());
            } else {
                self.changes.record(key.clone(), snapshot, baseline);
            }
            self.retain_storage_owner(&key, reference, items, interaction, physics);
            if let crate::world::ObjectKey::Generated(_) = &key {
                if let Some(owner) = self
                    .contents
                    .active
                    .iter()
                    .chain(&self.static_controls)
                    .find(|c| c.item == Some(id) || c.bindings.iter().any(|b| b.item == id))
                {
                    self.changes
                        .set_replay(&key, &owner.origin, &owner.content.placement.id);
                } else if let Some(supply) = self
                    .supplies
                    .iter()
                    .find(|s| s.items.is_some_and(|(a, b)| a == id || b == id))
                {
                    if let Some(instance) = self
                        .structures
                        .iter()
                        .find(|s| s.placement.id == supply.key)
                    {
                        self.changes
                            .set_replay(&key, &instance.placement, &supply.key);
                    }
                }
            }
        }
    }

    /// A generated storage owner needs a record while an item locks to it.
    /// Its locked generated items unload with it and refer to it by key.
    fn retain_storage_owner(
        &mut self,
        key: &ObjectKey,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &InteractionState,
        physics: &PhysicsWorld,
    ) {
        let Some(owner) = self
            .changes
            .get(key)
            .and_then(|c| c.snapshot.as_ref())
            .and_then(|s| s.attachment.as_ref())
            .filter(|a| a.volume.is_some())
            .and_then(|a| a.owner.as_ref())
            .filter(|owner| matches!(owner, ObjectKey::Generated(_)))
        else {
            return;
        };
        if self.changes.get(owner).is_some() {
            return;
        }
        let owner = owner.clone();
        let Some(id) = items.resolve_key(&owner) else {
            return;
        };
        let Some(snapshot) = changes::capture(id, items, interaction, physics, reference) else {
            return;
        };
        self.changes.retain_dependency(owner.clone(), snapshot);
        if let Some(content) = self
            .contents
            .active
            .iter()
            .chain(&self.static_controls)
            .find(|c| c.item == Some(id))
        {
            self.changes
                .set_replay(&owner, &content.origin, &content.content.placement.id);
        }
    }
    /// Bounds new owner payload, type-fit pairs, and existing vector/map reallocation.
    pub(super) fn binding_reservation(
        &self,
        prototype: &crate::reference::WorldPrototypeReference,
        id: &crate::world::GeneratedId,
        fixed_decoration: bool,
    ) -> Result<(usize, usize), ReferenceError> {
        if fixed_decoration {
            return Ok((0, 0));
        }
        let mut components = 0usize;
        let mut parts = 0usize;
        let mut fields_bytes = 0usize;
        for node in &prototype.nodes {
            for class in [
                "pickupable",
                "partscript",
                "partslotscript",
                "tankcapscript",
                "tankscript",
                "wheelscript",
                "gumiscript",
                "flashlightscript",
                "ediblescript",
            ] {
                if let Some(record) =
                    self.contents
                        .rules
                        .component(&self.reference, &node.source, class)?
                {
                    components += 1;
                    parts += usize::from(matches!(class, "partscript" | "partslotscript"));
                    fields_bytes += match record.fields {
                        Fields::Record(fields) => {
                            fields.values().map(|v| v.input_heap_bytes()).sum::<usize>()
                        }
                        Fields::Compact(fields) => fields
                            .iter()
                            .map(|f| f.value.input_heap_bytes())
                            .sum::<usize>(),
                    };
                }
            }
        }
        components += self
            .reference
            .mechanisms
            .iter()
            .filter(|r| r.prototype == prototype.source)
            .count();
        if components == 0 {
            return Ok((0, 0));
        }
        let identities = components.saturating_mul(
            super::super::contents::identity_heap(id)
                + prototype
                    .nodes
                    .iter()
                    .map(|n| n.name.len())
                    .max()
                    .unwrap_or(0)
                + 128,
        );
        // Each component covers its typed owner, maps, copied recipes and auxiliary item.
        // Fits can add a pair to both directions; 64 bytes covers the hash/table slot.
        let additions = components.saturating_mul(8192)
            + prototype.nodes.len().saturating_mul(1024)
            + prototype.colliders.len().saturating_mul(2048)
            + fields_bytes.saturating_mul(8)
            + identities.saturating_mul(8)
            + parts
                .saturating_mul(self.binding_objects + components)
                .saturating_mul(128);
        Ok((
            self.binding_owner_bytes
                .saturating_mul(2)
                .saturating_add(additions.saturating_mul(2)),
            components,
        ))
    }
    pub(super) fn sync_static_controls(
        &mut self,
        reference: &ReferenceManifest,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        wheels: &mut crate::interaction::wheels::WheelAssemblies,
    ) -> Result<(), ReferenceError> {
        for content in &mut self.static_controls {
            crate::world::content_mechanisms::sync(content, items, interaction, physics);
        }
        for index in 0..self.structures.len() {
            let instance = &mut self.structures[index];
            if instance.fixed_decoration
                || self
                    .contents
                    .active
                    .iter()
                    .any(|c| c.content.placement.id == instance.placement.id)
                || self
                    .static_controls
                    .iter()
                    .any(|c| c.content.placement.id == instance.placement.id)
                || !reference
                    .world
                    .mechanisms
                    .iter()
                    .any(|r| r.prototype == instance.placement.prototype)
            {
                continue;
            }
            let prototype = reference
                .world
                .prototypes
                .iter()
                .find(|p| p.source == instance.placement.prototype)
                .unwrap();
            let mut content =
                crate::world::contents::ContentInstance::for_structure(instance.placement.clone());
            content.item = Some(items.insert_generated(
                instance.placement.id.clone(),
                prototype,
                instance.placement.pose,
            )?);
            if let Err(error) = crate::world::content_mechanisms::bind(
                &mut content,
                instance,
                prototype,
                reference,
                &self.contents.rules,
                items,
                interaction,
                physics,
            )
            .and_then(|()| {
                bind_storage(
                    &content,
                    prototype,
                    rapier3d::math::Pose::IDENTITY,
                    &self.contents.rules,
                    reference,
                    interaction,
                )
            }) {
                rollback(&mut content, instance, items, interaction, physics, wheels);
                self.structures
                    .swap_remove(index)
                    .retire(physics, &mut self.trigger_roles);
                self.structure_revision = self.structure_revision.wrapping_add(1);
                return Err(error);
            }
            crate::world::content_mechanisms::sync(&mut content, items, interaction, physics);
            capture_baselines(&mut content, reference, items, interaction, physics);
            content.suspended_bodies.reserve(instance.bodies.len());
            self.static_controls.push(content);
            self.static_control_revision += 1;
        }
        Ok(())
    }
    /// Borrowed source baselines for active generated item bindings.
    pub fn generated_baselines(
        &self,
    ) -> impl Iterator<Item = (crate::reference::ItemId, &crate::world::ObjectSnapshot)> {
        self.contents
            .active
            .iter()
            .chain(&self.static_controls)
            .flat_map(|c| {
                c.item
                    .zip(c.baseline.as_ref())
                    .into_iter()
                    .chain(c.bindings.iter().map(|b| (b.item, &b.baseline)))
            })
    }
    pub fn generated_cap_visuals(
        &self,
        id: &crate::world::GeneratedId,
    ) -> &[crate::world::GeneratedCapVisual] {
        self.contents
            .active
            .iter()
            .find(|c| &c.content.placement.id == id)
            .map_or(&[], |c| c.caps.as_slice())
    }
    pub fn generated_node_pose(
        &self,
        id: &crate::world::GeneratedId,
        node: usize,
    ) -> Option<crate::vehicle::VehiclePose> {
        self.contents
            .active
            .iter()
            .chain(&self.static_controls)
            .find(|c| &c.content.placement.id == id)
            .and_then(|c| c.poses.get(node))
            .copied()
    }
    /// The bound item of a door `usablescript` in a published structure.
    pub fn door_item(
        &self,
        id: &crate::world::GeneratedId,
        prototype: &crate::reference::WorldPrototypeReference,
        door: &crate::reference::SourceId,
    ) -> Option<crate::reference::ItemId> {
        let row = self
            .reference
            .mechanisms
            .iter()
            .find(|r| r.prototype == prototype.source && r.recipe.source == *door)?;
        let content = self
            .contents
            .active
            .iter()
            .chain(&self.static_controls)
            .find(|c| &c.content.placement.id == id)?;
        crate::world::content_mechanisms::node_item(
            content,
            prototype,
            &self.reference,
            &self.contents.rules,
            row.node as usize,
        )
    }
    pub fn world_reference(&self) -> &std::sync::Arc<crate::reference::WorldReference> {
        &self.reference
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sync_contents(
        &mut self,
        reference: &ReferenceManifest,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        wheels: &mut crate::interaction::wheels::WheelAssemblies,
        vehicles: &mut std::collections::BTreeMap<ObjectKey, crate::starting_area::VehicleRuntime>,
        powertrain: &mut crate::simulation::powertrain::PowertrainParts,
    ) -> Result<(), ReferenceError> {
        // A structure that becomes a content binds again as that content.
        // Its static control goes before that bind.
        let mut index = 0;
        while index < self.static_controls.len() {
            let id = &self.static_controls[index].content.placement.id;
            if !self
                .contents
                .active
                .iter()
                .any(|c| &c.content.placement.id == id && c.item.is_none())
            {
                index += 1;
                continue;
            }
            let mut content = self.static_controls.swap_remove(index);
            self.static_control_revision += 1;
            if let Some(item) = content.item {
                unbind_storage(
                    item,
                    &self.contents.active,
                    items,
                    interaction,
                    physics,
                    wheels,
                )?;
            }
            let instance = self
                .structures
                .iter()
                .find(|s| s.placement.id == content.content.placement.id)
                .unwrap();
            rollback(&mut content, instance, items, interaction, physics, wheels);
        }
        for content in &mut self.contents.active {
            crate::world::content_mechanisms::sync(content, items, interaction, physics);
            for binding in &content.bindings {
                if let (Some(state), Some((body, local))) =
                    (items.get(binding.item), binding.render_body)
                {
                    if state.body != Some(body) {
                        let pose = to_rapier(state.pose) * to_rapier(local).inverse();
                        if physics.rapier().bodies[body].position() != &pose {
                            physics.edit_body(body, |state| state.set_position(pose, false));
                        }
                    }
                }
            }
            if let Some(item) = content.item {
                if let (Some(state), Some((body, local))) = (items.get(item), content.render_body) {
                    if state.body != Some(body) {
                        let pose = to_rapier(state.pose) * to_rapier(local).inverse();
                        if physics.rapier().bodies[body].position() != &pose {
                            physics.edit_body(body, |state| state.set_position(pose, false));
                        }
                    }
                }
                continue;
            }
            let placement_id = &content.content.placement.id;
            let Some(instance) = self
                .structures
                .iter_mut()
                .find(|i| &i.placement.id == placement_id)
            else {
                continue;
            };
            if instance.fixed_decoration {
                content.restored = true;
                continue;
            }
            let placement = content.content.placement.clone();
            let prototype = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == placement.prototype)
                .unwrap();
            let hose = reference
                .item_prototypes
                .iter()
                .find(|p| p.hose.is_some() && p.source == placement.prototype);
            let result = if let Some(hose) = hose {
                bind_hose(
                    content,
                    instance,
                    prototype,
                    hose,
                    reference,
                    &self.contents.rules,
                    items,
                    interaction,
                    physics,
                )
            } else {
                let pickup_root = if let Some(unparent) = self.contents.rules.component(
                    &self.reference,
                    &placement.prototype,
                    "childunparent",
                )? {
                    link(unparent.fields, "g")?
                } else {
                    &placement.prototype
                };
                let pickup =
                    self.contents
                        .rules
                        .component(&self.reference, pickup_root, "pickupable")?;
                // Car and trailer roots are items without a pickupable. Only bikes have one.
                if pickup.is_none() && prototype.trailer.is_none() && prototype.vehicle.is_none() {
                    continue;
                }
                (|| -> Result<(), ReferenceError> {
                    let BoundPickup {
                        id,
                        body,
                        body_index,
                        rule,
                        layers,
                    } = bind_pickup(
                        content,
                        pickup,
                        false,
                        instance,
                        prototype,
                        &self.reference,
                        &self.contents.rules,
                        items,
                        interaction,
                        physics,
                    )?;
                    if let Some(edible) = self.contents.rules.component(
                        &self.reference,
                        pickup_root,
                        "ediblescript",
                    )? {
                        if let Some((prototype_index, food)) = reference
                            .survival
                            .foods
                            .iter()
                            .enumerate()
                            .find(|(_, food)| &food.source == edible.source)
                        {
                            items.get_mut(id).unwrap().food =
                                Some(crate::interaction::items::FoodState {
                                    prototype_index,
                                    bite: food.index,
                                });
                            let handles: Vec<_> = instance
                                .colliders
                                .iter()
                                .filter(|(source, _)| {
                                    food.colliders.iter().any(|c| &c.source == source)
                                })
                                .map(|(_, handle)| *handle)
                                .collect();
                            interaction.register_consumable(id, food, &handles);
                        }
                    }
                    crate::world::content_containers::bind(
                        content,
                        instance,
                        prototype,
                        &self.contents.rules,
                        reference,
                        items,
                        interaction,
                        physics,
                    )?;
                    crate::world::content_parts::bind(
                        content,
                        instance,
                        prototype,
                        &self.contents.rules,
                        reference,
                        rule,
                        layers,
                        items,
                        interaction,
                        physics,
                    )?;
                    crate::world::content_wheels::bind(
                        content,
                        instance,
                        prototype,
                        &self.contents.rules,
                        reference,
                        items,
                        interaction,
                        physics,
                        wheels,
                    )?;
                    crate::world::content_slots::bind(
                        content,
                        instance,
                        prototype,
                        &self.contents.rules,
                        reference,
                        items,
                        interaction,
                        physics,
                        wheels,
                    )?;
                    crate::world::content_mechanisms::bind(
                        content,
                        instance,
                        prototype,
                        reference,
                        &self.contents.rules,
                        items,
                        interaction,
                        physics,
                    )?;
                    if interaction.parts.contains(id) {
                        wheels.register_generated_general(
                            id,
                            physics.rapier().bodies[body].mass(),
                            items.get(id).unwrap().fluid.is_some(),
                        );
                    }
                    crate::world::content_mechanisms::sync(content, items, interaction, physics);
                    super::content_conditions::bind(
                        content,
                        instance,
                        prototype,
                        &self.contents.rules,
                        &self.reference,
                        interaction,
                    )?;
                    let root = crate::world::placement::node_pose(
                        prototype,
                        prototype.bodies[body_index].node as usize,
                    );
                    bind_storage(
                        content,
                        prototype,
                        rapier3d::math::Pose::from_parts(
                            rapier3d::math::Vector::from_array(root.translation),
                            rapier3d::math::Rotation::from_array(root.rotation),
                        ),
                        &self.contents.rules,
                        reference,
                        interaction,
                    )?;
                    bind_push(
                        id,
                        body,
                        body_index,
                        instance,
                        prototype,
                        interaction,
                        physics,
                    )?;
                    // The last fallible step: a failed binding leaves no car behind.
                    // Seats bind later and find the vehicle of the chassis.
                    crate::world::content_vehicles::bind(
                        content,
                        instance,
                        prototype,
                        &self.reference,
                        &reference.skid,
                        &self.contents.rules,
                        items,
                        interaction,
                        physics,
                        wheels,
                        vehicles,
                        powertrain,
                        &mut self.changes,
                    )?;
                    content.suspended_bodies.reserve(instance.bodies.len());
                    capture_baselines(content, reference, items, interaction, physics);
                    let state = items.get(id).unwrap();
                    physics.edit_body(body, |body| {
                        body.set_position(
                            to_rapier(state.pose) * to_rapier(state.local_pose).inverse(),
                            true,
                        )
                    });
                    Ok(())
                })()
            };
            if let Err(error) = result {
                rollback(content, instance, items, interaction, physics, wheels);
                let index = self
                    .structures
                    .iter()
                    .position(|s| s.placement.id == placement.id)
                    .unwrap();
                self.structures
                    .swap_remove(index)
                    .retire(physics, &mut self.trigger_roles);
                self.structure_revision = self.structure_revision.wrapping_add(1);
                return Err(error);
            }
        }
        for content_index in 0..self.contents.active.len() {
            for slot_index in 0..self.contents.active[content_index].slots.len() {
                let content = &self.contents.active[content_index];
                let binding = &content.slots[slot_index];
                let source = &binding.source;
                let slot = binding.slot;
                if binding.initialized {
                    continue;
                }
                if interaction.parts.slot_item(slot).is_some() {
                    self.contents.active[content_index].slots[slot_index].initialized = true;
                    continue;
                }
                let embedded = self
                    .contents
                    .selected_slot_part(&content.content.placement.id, source, &self.reference)?
                    .and_then(|part| {
                        content
                            .bindings
                            .iter()
                            .find(|b| &b.source == part)
                            .map(|b| b.item)
                    });
                let spawned = self
                    .contents
                    .active
                    .iter()
                    .find(|child| {
                        let mut iter = child.content.placement.id.children.iter();
                        child.content.placement.id.layer == content.content.placement.id.layer
                            && child.content.placement.id.generator
                                == content.content.placement.id.generator
                            && child.content.placement.id.unit == content.content.placement.id.unit
                            && child.content.placement.id.ordinal
                                == content.content.placement.id.ordinal
                            && child.content.placement.id.children.len()
                                == content.content.placement.id.children.len() + 1
                            && iter
                                .by_ref()
                                .take(content.content.placement.id.children.len())
                                .eq(content.content.placement.id.children.iter())
                            && iter.next().is_some_and(|(s, n)| s == source && *n == 0)
                    })
                    .and_then(|c| c.item);
                if let Some(item) = embedded
                    .or(spawned)
                    .filter(|i| interaction.parts.contains(*i))
                {
                    interaction
                        .parts
                        .initialize_generated_slot(item, slot, items, physics)?;
                    interaction
                        .parts
                        .mount_mechanism(item, &mut interaction.mechanisms);
                    self.rebaseline(item, reference, items, interaction, physics);
                    self.contents.active[content_index].slots[slot_index].initialized = true;
                }
            }
        }
        // World car wheel slots mount their rim when its item binds.
        if wheels.install_pending(items, physics)? {
            for (sensor, rim) in wheels.sensor_rims() {
                interaction.bind_wheel_sensor(sensor, rim, physics);
            }
            for &item in wheels.last_installed() {
                self.rebaseline(item, reference, items, interaction, physics);
            }
        }
        self.restore_content_changes(reference, items, interaction, physics, wheels)?;
        Ok(())
    }
    /// A slot part that mounted at binding: its owner content takes the mounted state as
    /// its baseline.
    fn rebaseline(
        &mut self,
        item: crate::reference::ItemId,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &InteractionState,
        physics: &PhysicsWorld,
    ) {
        if let Some(mut snapshot) =
            crate::world::changes::capture(item, items, interaction, physics, reference)
        {
            for owner in &mut self.contents.active {
                if owner.item == Some(item) {
                    snapshot.lamp = owner.content.portable_lamp();
                    owner.baseline = Some(snapshot);
                    for binding in &mut owner.bindings {
                        binding.baseline = crate::world::changes::capture(
                            binding.item,
                            items,
                            interaction,
                            physics,
                            reference,
                        )
                        .unwrap();
                    }
                    owner.restored = false;
                    break;
                }
                if let Some(binding) = owner.bindings.iter_mut().find(|b| b.item == item) {
                    binding.baseline = snapshot;
                    owner.restored = false;
                    break;
                }
            }
        }
    }
    fn restore_content_changes(
        &mut self,
        reference: &ReferenceManifest,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        wheels: &mut crate::interaction::wheels::WheelAssemblies,
    ) -> Result<(), ReferenceError> {
        let count = self.contents.active.len() + self.static_controls.len();
        for index in 0..count {
            self.publish_content_visual(index, items);
            let pending_parent = {
                let content = self
                    .contents
                    .active
                    .iter()
                    .chain(&self.static_controls)
                    .nth(index)
                    .unwrap();
                if content.restored || content.item.is_none() {
                    continue;
                }
                content
                    .item
                    .into_iter()
                    .chain(content.bindings.iter().map(|b| b.item))
                    .any(|item| {
                        self.changes
                            .get(items.object_key(item))
                            .and_then(|c| c.snapshot.as_ref())
                            .and_then(|s| s.attachment.as_ref())
                            .and_then(|a| a.owner.as_ref())
                            .and_then(|k| items.resolve_key(k))
                            .is_some_and(|parent| {
                                self.contents
                                    .active
                                    .iter()
                                    .chain(&self.static_controls)
                                    .any(|owner| {
                                        owner.content.placement.id != content.content.placement.id
                                            && !owner.restored
                                            && (owner.item == Some(parent)
                                                || owner.bindings.iter().any(|b| b.item == parent))
                                    })
                            })
                    })
            };
            let content = self
                .contents
                .active
                .iter_mut()
                .chain(&mut self.static_controls)
                .nth(index)
                .unwrap();
            if content.restored || content.item.is_none() {
                continue;
            }
            for (body, enabled) in content.suspended_bodies.drain(..) {
                physics.edit_body(body, |state| state.set_enabled(enabled));
            }
            let mut complete = !pending_parent;
            let mut restore_error = None;
            for item in content
                .item
                .into_iter()
                .chain(content.bindings.iter().map(|b| b.item))
            {
                if pending_parent {
                    break;
                }
                if let Some(change) = self.changes.get(items.object_key(item)) {
                    if let Some(snapshot) = &change.snapshot {
                        match restore_item(
                            item,
                            snapshot,
                            reference,
                            items,
                            interaction,
                            physics,
                            wheels,
                        ) {
                            Ok(ready) => complete &= ready,
                            Err(error) => {
                                complete = false;
                                restore_error = Some(error);
                                break;
                            }
                        }
                    }
                }
            }
            content.restored = complete;
            if let Some(instance) = self
                .structures
                .iter()
                .find(|i| i.placement.id == content.content.placement.id)
            {
                let prototype = reference
                    .world
                    .prototypes
                    .iter()
                    .find(|p| p.source == instance.placement.prototype)
                    .unwrap();
                for (source, handle) in &instance.colliders {
                    let collider = prototype
                        .colliders
                        .iter()
                        .find(|c| &c.source == source)
                        .unwrap();
                    physics.edit_collider(*handle, |state| {
                        state.set_enabled(
                            complete
                                && collider.enabled
                                && crate::world::spawn_appearance::node_active(
                                    prototype,
                                    collider.node as usize,
                                    &content.content.appearance,
                                ),
                        )
                    });
                }
                if !complete {
                    for body in &instance.bodies {
                        physics.edit_body(*body, |body_state| {
                            content
                                .suspended_bodies
                                .push((*body, body_state.is_enabled()));
                            body_state.set_enabled(false);
                        });
                    }
                }
            }
            if complete {
                crate::world::content_mechanisms::sync(content, items, interaction, physics);
            }
            self.publish_content_visual(index, items);
            if let Some(error) = restore_error {
                physics.refresh_queries();
                return Err(error);
            }
        }
        Ok(())
    }
    fn publish_content_visual(&mut self, index: usize, items: &ItemStore) {
        let active_count = self.contents.active.len();
        let (content, slot) = if index < active_count {
            (
                &mut self.contents.active[index],
                super::content_index::ContentSlot::Active(index),
            )
        } else {
            (
                &mut self.static_controls[index - active_count],
                super::content_index::ContentSlot::Control(index - active_count),
            )
        };
        let food = content
            .item
            .and_then(|id| items.get(id))
            .and_then(|item| item.food.map(|food| (food, item.removed)));
        if content.take_visual_change(food) {
            self.presentation_changes.push(slot);
        }
    }

    pub fn bindings_ready(&self, id: &crate::world::GeneratedId) -> bool {
        self.contents
            .active
            .iter()
            .chain(&self.static_controls)
            .find(|c| &c.content.placement.id == id)
            .is_none_or(|c| c.item.is_none() || c.restored)
    }
    /// Squared distance to the nearest source important object: the player, the start car and the last seated car.
    /// A car does not count itself: `own` is its chassis.
    pub(crate) fn important_distance(
        &self,
        position: [f64; 3],
        own: Option<rapier3d::prelude::RigidBodyHandle>,
    ) -> f32 {
        [self.request.player, self.request.vehicle]
            .into_iter()
            .chain(
                self.cars
                    .iter()
                    .filter(|(chassis, _)| Some(*chassis) != own)
                    .map(|(_, p)| *p),
            )
            .map(|p| crate::world::contents::distance(p, position))
            .fold(f32::INFINITY, f32::min)
    }
    pub(super) fn retire_contents(
        &mut self,
        physics: &mut PhysicsWorld,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
        wheels: &mut crate::interaction::wheels::WheelAssemblies,
        vehicles: &mut std::collections::BTreeMap<ObjectKey, crate::starting_area::VehicleRuntime>,
        powertrain: &mut crate::simulation::powertrain::PowertrainParts,
    ) -> Result<(), ReferenceError> {
        let remove = self.distances.item_remove_dist;
        let freeze = self.distances.item_freeze_dist;
        let unfreeze = self.distances.item_unfreeze_dist;
        let mut index = 0;
        while index < self.contents.active.len() {
            let content = &self.contents.active[index];
            // An item that left a storage lock with its owner unloads now.
            let departing = content
                .item
                .is_some_and(|id| interaction.storage.departing(id));
            // Replay keeps its source pose until binding applies the destination snapshot.
            // Its caller owns cancellation, including intermediate deferred output.
            if !departing
                && content.replay_only
                && !content.restored
                && (content.item.is_none() || self.contents.replaying(&content.origin.id))
            {
                index += 1;
                continue;
            }
            let id = content.item;
            let pose = id
                .and_then(|id| items.get(id))
                .map(|s| crate::world::AbsolutePose {
                    position: self.origin.absolute(s.pose.position),
                    rotation: s.pose.rotation,
                })
                .unwrap_or(content.content.placement.pose);
            let offset =
                crate::world::placement::rotate(pose.rotation, content.content.distance_offset);
            let position = std::array::from_fn(|i| pose.position[i] + offset[i] as f64);
            let body = id.and_then(|id| items.get(id)).and_then(|s| s.body);
            let own = body.filter(|b| physics.vehicle_of(*b).is_some());
            let mut distance = self.important_distance(position, own);
            let local_item = |item| {
                content.item == Some(item) || content.bindings.iter().any(|b| b.item == item)
            };
            let local_slot = |slot| {
                content.slots.iter().any(|s| s.slot == slot)
                    || self.contents.active.iter().any(|owner| {
                        let parent = &owner.content.placement.id;
                        let child = &content.content.placement.id;
                        parent.layer == child.layer
                            && parent.generator == child.generator
                            && parent.unit == child.unit
                            && parent.ordinal == child.ordinal
                            && child.children.len() > parent.children.len()
                            && child.children.starts_with(&parent.children)
                            && owner.slots.iter().any(|s| s.slot == slot)
                    })
            };
            // A world car slot rim may leave with its car or the car content that spawned it.
            let local_owner = |item| {
                local_item(item)
                    || self.contents.active.iter().any(|owner| {
                        let parent = &owner.content.placement.id;
                        let child = &content.content.placement.id;
                        parent.layer == child.layer
                            && parent.generator == child.generator
                            && parent.unit == child.unit
                            && parent.ordinal == child.ordinal
                            && child.children.len() > parent.children.len()
                            && child.children.starts_with(&parent.children)
                            && owner.item == Some(item)
                    })
            };
            let protected_location = |location| match location {
                crate::interaction::items::ItemLocation::World => false,
                crate::interaction::items::ItemLocation::Attached(owner) => !local_item(owner),
                crate::interaction::items::ItemLocation::PartSlot(slot) => !local_slot(slot),
                crate::interaction::items::ItemLocation::WheelSlot(slot) => {
                    !wheels.slot_owner(slot).is_some_and(local_owner)
                }
                _ => true,
            };
            let mut protected = id
                .and_then(|id| items.get(id))
                .is_some_and(|item| protected_location(item.location));
            for binding in &content.bindings {
                if let Some(item) = items.get(binding.item) {
                    protected |= protected_location(item.location);
                    let position = self.origin.absolute(item.pose.position);
                    distance = distance.min(self.important_distance(position, own));
                }
            }
            protected |= content.has_external_dependents(items, &interaction.parts, wheels);
            // The seat owner and the last seated car stay.
            protected |= body.is_some_and(|b| self.kept.contains(&Some(b)));
            // A trailer, its grip and the car that tows it load and unload as one group.
            for group in interaction.hitch.groups() {
                if !group.iter().flatten().any(|&item| local_item(item)) {
                    continue;
                }
                for member in group
                    .into_iter()
                    .flatten()
                    .filter(|&item| !local_item(item))
                {
                    let Some(state) = items.get(member) else {
                        continue;
                    };
                    let body = state.body;
                    let vehicle = body.filter(|b| physics.vehicle_of(*b).is_some());
                    let position = self.origin.absolute(state.pose.position);
                    distance = distance.min(self.important_distance(position, vehicle));
                    protected |= body.is_some_and(|b| self.kept.contains(&Some(b)));
                }
            }
            protected |= self
                .saved_item_protection
                .iter()
                .any(|key| items.resolve_key(key).is_some_and(local_item));
            let removed = id
                .and_then(|id| items.get(id))
                .is_some_and(|item| item.removed);
            let buried = id
                .and_then(|id| items.get(id))
                .is_some_and(|item| item.buried);
            if !departing && (distance < remove * remove || protected) && !removed {
                if let Some(body) = id.and_then(|id| items.get(id)).and_then(|s| s.body) {
                    if !protected
                        && (buried || distance >= freeze * freeze || distance < unfreeze * unfreeze)
                    {
                        let kind = if buried || distance >= freeze * freeze {
                            rapier3d::prelude::RigidBodyType::KinematicPositionBased
                        } else {
                            rapier3d::prelude::RigidBodyType::Dynamic
                        };
                        if physics.rapier().bodies[body].body_type() != kind {
                            physics.edit_body(body, |state| state.set_body_type(kind, true));
                            if kind == rapier3d::prelude::RigidBodyType::Dynamic {
                                if let Some(item) = id {
                                    if let Some(snapshot) = self
                                        .changes
                                        .get(items.object_key(item))
                                        .and_then(|c| c.snapshot.as_ref())
                                    {
                                        physics.edit_body(body, |body| {
                                            body.set_linvel(snapshot.linear_velocity.into(), true)
                                        });
                                        physics.edit_body(body, |body| {
                                            body.set_angvel(snapshot.angular_velocity.into(), true)
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                index += 1;
                continue;
            }
            let mut content = self.contents.remove_active(index);
            // Hitches end while the bodies still exist.
            for item in content
                .item
                .into_iter()
                .chain(content.bindings.iter().map(|b| b.item))
            {
                interaction.hitch.remove_item(item, items, physics);
            }
            crate::world::content_vehicles::retire(
                &content,
                items,
                interaction,
                physics,
                vehicles,
                wheels,
                powertrain,
                &mut self.changes,
            );
            if let Some(item) = content.item {
                interaction.storage.departed(item);
                // Restart: items that leave with this owner may sit before `index`.
                if unbind_storage(
                    item,
                    &self.contents.active,
                    items,
                    interaction,
                    physics,
                    wheels,
                )? {
                    index = 0;
                }
            }
            for item in content
                .item
                .into_iter()
                .chain(content.bindings.iter().map(|b| b.item))
            {
                if interaction.storage.stored(item) {
                    interaction.storage.unlock(item, items, physics)?;
                }
            }
            for binding in &content.bindings {
                interaction.parts.remove_generated(binding.item);
            }
            for slot in &content.slots {
                interaction.parts.remove_generated_slot(slot.slot);
                items.remove_generated_slot(slot.slot);
            }
            if let Some(item) = content.item {
                interaction.parts.remove_generated(item);
                wheels.remove_generated(item);
                interaction
                    .mechanisms
                    .remove_generated(item, &content.nodes);
            }
            super::content_conditions::unbind(&mut content, interaction);
            for binding in &content.bindings {
                wheels.remove_generated(binding.item);
                if let Some(instance) = self
                    .structures
                    .iter()
                    .find(|i| i.placement.id == content.content.placement.id)
                {
                    for (_, collider) in &instance.colliders {
                        interaction.unbind_generated(binding.item, std::slice::from_ref(collider));
                    }
                }
                items.set_body(binding.item, None);
                items.get_mut(binding.item).unwrap().location =
                    crate::interaction::items::ItemLocation::World;
                items.remove_generated(binding.item);
            }
            if let Some(id) = content.item {
                if let Some(instance) = self
                    .structures
                    .iter()
                    .find(|i| i.placement.id == content.content.placement.id)
                {
                    for (_, collider) in &instance.colliders {
                        interaction.unbind_generated(id, std::slice::from_ref(collider));
                    }
                }
                items.set_body(id, None);
                items.get_mut(id).unwrap().location =
                    crate::interaction::items::ItemLocation::World;
                items.remove_generated(id);
            }
            self.wanted_structures
                .retain(|p| p.id != content.content.placement.id);
            self.ready_structures
                .retain(|id| *id != content.content.placement.id);
        }
        Ok(())
    }
}

/// A generated pickup item on its instance body.
struct BoundPickup {
    id: crate::reference::ItemId,
    body: rapier3d::prelude::RigidBodyHandle,
    body_index: usize,
    /// None: a trailer root item without a pickupable.
    rule: Option<PickupReference>,
    layers: u32,
}

/// Bind one `pickupable` to its instance body, colliders and hit sound.
/// The item becomes the content item, or a content binding when `child`.
/// Without a pickupable, the prototype root becomes an item that cannot be picked up.
#[allow(clippy::too_many_arguments)]
fn bind_pickup(
    content: &mut crate::world::contents::ContentInstance,
    pickup: Option<SourceRecord<'_>>,
    child: bool,
    instance: &crate::world::StructureInstance,
    prototype: &crate::reference::WorldPrototypeReference,
    world: &crate::reference::WorldReference,
    rules: &SourceRules,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<BoundPickup, ReferenceError> {
    let rule = pickup.map(pickup_rule).transpose()?;
    let root = match pickup {
        Some(pickup) => rules.root(world, pickup.source)?,
        None => &prototype.source,
    };
    let body_index = prototype
        .bodies
        .iter()
        .position(|b| prototype.nodes[b.node as usize].source == *root)
        .ok_or_else(|| ReferenceError("Generated pickup has no source body".into()))?;
    let body = instance.bodies[body_index];
    let pickup_node = prototype
        .nodes
        .iter()
        .position(|n| &n.source == root)
        .ok_or_else(|| ReferenceError("Generated pickup transform missing".into()))?;
    let placement = &content.content.placement;
    let pickup_pose = crate::world::placement::place_pose(
        placement.pose,
        placement.scale,
        crate::world::placement::node_pose(prototype, pickup_node),
    );
    let mut key = placement.id.clone();
    if let (true, Some(pickup)) = (child, pickup) {
        key.children.push((pickup.source.clone(), 0));
    }
    let id = items.insert_generated(key, prototype, pickup_pose)?;
    let hit = pickup.and_then(|pickup| hit_sound(world, pickup.source));
    let state = items.set_body(id, Some(body)).unwrap();
    state.hit_sound = hit;
    if !child {
        state.fluid = content.content.fluid.clone();
        state.buried = content.content.buried;
    }
    state.local_pose =
        from_rapier(&(physics.rapier().bodies[body].position().inverse() * to_rapier(state.pose)));
    let local = state.local_pose;
    let container = state.fluid.is_some();
    let body_pose = to_rapier(state.pose) * to_rapier(local).inverse();
    physics.edit_body(body, |state| state.set_position(body_pose, true));
    // Record the item before fallible steps so rollback removes it.
    if child {
        content
            .bindings
            .push(crate::world::contents::ContentBinding {
                source: pickup.map_or_else(|| root.clone(), |p| p.source.clone()),
                item: id,
                baseline: items.generated_snapshot(id, None).unwrap(),
                render_body: Some((body, local)),
            });
    } else {
        content.item = Some(id);
        content.render_body = Some((body, local));
    }
    let mut layers = 0u32;
    for (source, handle) in &instance.colliders {
        let collider = prototype
            .colliders
            .iter()
            .find(|c| c.source == *source)
            .unwrap();
        if collider.body != Some(body_index as u32) {
            continue;
        }
        layers |= 1u32 << prototype.nodes[collider.node as usize].layer;
        let collider_id = interaction.allocate_generated_collider()?;
        physics.edit_collider(*handle, |collider| {
            collider.user_data = (collider.user_data & STEP_SURFACE_MASK)
                | ((collider_id.0 as u128) << 64)
                | (id.0 as u128 + 1);
            collider.set_active_events(hit_events(hit));
        });
        interaction.bind_surface(
            *handle,
            ActionBinding {
                collider: collider_id,
                use_owner: None,
                pickup_owner: rule.as_ref().map(|_| id),
                removal_owner: None,
                push_owner: None,
            },
        );
    }
    if let Some(rule) = &rule {
        interaction.register_generated_pickup(id, rule.clone(), layers, container);
    }
    Ok(BoundPickup {
        id,
        body,
        body_index,
        rule,
        layers,
    })
}

/// Binds the `pushablescript` on the item body. Its colliders at or below the push node
/// push the item. `InteractionState::unbind_generated` removes the push.
fn bind_push(
    id: crate::reference::ItemId,
    body: rapier3d::prelude::RigidBodyHandle,
    body_index: usize,
    instance: &crate::world::StructureInstance,
    prototype: &crate::reference::WorldPrototypeReference,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    // A push on another body has no item of its own here.
    let Some(push) = prototype
        .pushes
        .iter()
        .find(|p| p.body as usize == body_index)
    else {
        return Ok(());
    };
    let under = |mut node: u32| loop {
        if node == push.node {
            return true;
        }
        match prototype.nodes[node as usize].parent {
            Some(parent) => node = parent,
            None => return false,
        }
    };
    let colliders: Vec<(u32, rapier3d::prelude::ColliderHandle)> = instance
        .colliders
        .iter()
        .filter_map(|(source, handle)| {
            let index = prototype
                .colliders
                .iter()
                .position(|c| &c.source == source)?;
            let collider = &prototype.colliders[index];
            (collider.body == Some(body_index as u32) && under(collider.node))
                .then_some((index as u32, *handle))
        })
        .collect();
    let node = |i: u32| crate::world::placement::node_pose(prototype, i as usize);
    let hand = push.grip.as_ref().map(|g| {
        crate::interaction::push::grip_hand(
            &g.local_pose,
            &node(g.node),
            g.hand_node.map(node).as_ref(),
        )
    });
    interaction.register_generated_push(id, body, push, &colliders, hand, physics)
}

/// Siphon hose placement: one item per segment body, then one runtime hose.
/// The first main segment is the content item. The other segments are content bindings.
#[allow(clippy::too_many_arguments)]
fn bind_hose(
    content: &mut crate::world::contents::ContentInstance,
    instance: &crate::world::StructureInstance,
    prototype: &crate::reference::WorldPrototypeReference,
    hose_prototype: &crate::reference::ItemPrototype,
    reference: &ReferenceManifest,
    rules: &SourceRules,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    let hose = hose_prototype.hose.as_ref().unwrap();
    let mut segments = [crate::reference::ItemId(0); crate::reference::HOSE_SEGMENTS];
    for (item, segment) in segments.iter_mut().zip(hose.segments) {
        // The world prototype and the item prototype share the prefab Rigidbody sources.
        let source = &hose_prototype
            .bodies
            .iter()
            .find(|b| b.id == segment)
            .ok_or_else(|| ReferenceError("Hose segment body missing".into()))?
            .source;
        let body_index = prototype
            .bodies
            .iter()
            .position(|b| &b.source == source)
            .ok_or_else(|| ReferenceError("Generated hose segment has no body".into()))?;
        let node = &prototype.nodes[prototype.bodies[body_index].node as usize].source;
        let pickup = rules
            .component(&reference.world, node, "pickupable")?
            .ok_or_else(|| ReferenceError("Generated hose segment has no pickup".into()))?;
        let bound = bind_pickup(
            content,
            Some(pickup),
            segment != hose.mains[0],
            instance,
            prototype,
            &reference.world,
            rules,
            items,
            interaction,
            physics,
        )?;
        if bound.body_index != body_index {
            return Err(ReferenceError("Generated hose pickup body differs".into()));
        }
        *item = bound.id;
    }
    if content.item.is_none() {
        return Err(ReferenceError("Generated hose has no main segment".into()));
    }
    interaction.register_hose(hose_prototype, segments, items, physics)?;
    content.suspended_bodies.reserve(instance.bodies.len());
    capture_baselines(content, reference, items, interaction, physics);
    Ok(())
}

/// Source baselines of the content item and its bindings.
fn capture_baselines(
    content: &mut crate::world::contents::ContentInstance,
    reference: &ReferenceManifest,
    items: &ItemStore,
    interaction: &InteractionState,
    physics: &PhysicsWorld,
) {
    content.baseline = content
        .item
        .and_then(|id| crate::world::changes::capture(id, items, interaction, physics, reference));
    if let Some(baseline) = &mut content.baseline {
        baseline.lamp = content.content.portable_lamp();
    }
    for binding in &mut content.bindings {
        binding.baseline =
            crate::world::changes::capture(binding.item, items, interaction, physics, reference)
                .unwrap();
    }
}

/// Binds the storage volumes and flags of a generated item.
/// `item_root` is the item frame in the prototype root frame.
fn bind_storage(
    content: &super::super::contents::ContentInstance,
    prototype: &crate::reference::WorldPrototypeReference,
    item_root: rapier3d::math::Pose,
    rules: &SourceRules,
    reference: &ReferenceManifest,
    interaction: &mut InteractionState,
) -> Result<(), ReferenceError> {
    let Some(item) = content.item else {
        return Ok(());
    };
    interaction.storage.bind_world_volumes(
        item,
        &prototype.source,
        item_root,
        content.content.placement.scale,
        |node| {
            crate::world::content_mechanisms::node_item(
                content,
                prototype,
                &reference.world,
                rules,
                node as usize,
            )
        },
        |source| {
            content
                .slots
                .iter()
                .find(|s| s.source == *source)
                .map(|s| s.slot)
        },
    )
}

/// Unbinds the storage of a generated item before it unloads.
/// Active content roots locked to it leave with it. Other locked items unlock.
/// Returns true if an item departs.
pub(super) fn unbind_storage(
    item: crate::reference::ItemId,
    active: &[super::super::contents::ContentInstance],
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
    wheels: &crate::interaction::wheels::WheelAssemblies,
) -> Result<bool, ReferenceError> {
    let parts = &interaction.parts;
    interaction.storage.unbind_owner(
        item,
        |items, locked| {
            active
                .iter()
                .any(|c| c.item == Some(locked) && !c.has_external_dependents(items, parts, wheels))
        },
        items,
        physics,
    )
}

pub fn pickup_rule(pickup: SourceRecord<'_>) -> Result<PickupReference, ReferenceError> {
    let f = pickup.fields;
    let mut position = vector(f, "recenterPos")?;
    position[2] = -position[2];
    Ok(PickupReference {
        source: pickup.source.clone(),
        cant_pickup: flag(f, "cantPickup")?,
        cant_pickup_while_mounted: flag(f, "cantPickupWhileMount")?,
        cant_pickup_while_seated: flag(f, "cantPickupWhileSitting")?,
        no_rotation: flag(f, "noRot")?,
        move_with_rotation: flag(f, "moveWithRot")?,
        use_recenter_position: flag(f, "useRecenter")?,
        recenter_position: position,
        use_recenter_rotation: flag(f, "useRecenterRot")?,
        recenter_rotation: crate::world::spawn_building::source_euler(vector(f, "recenterRot")?),
        recenter_distance: number(f, "recenterDist")?,
        can_inventory: flag(f, "canInventory")?,
        can_hold_right_hand: flag(f, "canHoldRightHand")?,
        disable_rh_fix: flag(f, "disableRHFix")?,
        right_hand_position: {
            let mut p = vector(f, "RHP")?;
            p[2] = -p[2];
            p
        },
        right_hand_rotation: crate::world::spawn_building::source_euler(vector(f, "RHR")?),
        right_hand_grip: optional(f, "RightHandPos")?.cloned(),
        left_hand_grip: optional(f, "LeftHandPos")?.cloned(),
        double_hand: flag(f, "doubleHand")?,
        disable_when_stored: optional(f, "disableThisWhenStored")?.cloned(),
        keep_when_stored: optional(f, "dontDisableWhenStored")?.cloned(),
        inventory_icon: optional(f, "invImg")?.cloned(),
    })
}

/// Source hit sound of a generated pickup.
pub fn hit_sound(
    world: &crate::reference::WorldReference,
    pickup: &crate::reference::SourceId,
) -> Option<HitSound> {
    world
        .pickup_hits
        .iter()
        .find(|h| &h.source == pickup)
        .map(|h| HitSound {
            clip: h.collision_clip,
            gain: h.collision_gain,
        })
}
/// Only pickups with a hit sound report contacts.
pub fn hit_events(sound: Option<HitSound>) -> ActiveEvents {
    if sound.is_some() {
        ActiveEvents::COLLISION_EVENTS
    } else {
        ActiveEvents::empty()
    }
}

// Failed unpublished bindings cannot leave item identities or attachment owners behind.
pub(super) fn rollback(
    content: &mut super::super::contents::ContentInstance,
    instance: &super::super::StructureInstance,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
    wheels: &mut crate::interaction::wheels::WheelAssemblies,
) {
    for item in content.bindings.iter().map(|b| b.item).chain(content.item) {
        interaction.parts.remove_generated(item);
        wheels.remove_generated(item);
        for (_, collider) in &instance.colliders {
            interaction.unbind_generated(item, std::slice::from_ref(collider));
        }
        if let Some(state) = items.set_body(item, None) {
            state.location = crate::interaction::items::ItemLocation::World;
        }
        items.remove_generated(item);
    }
    for slot in &content.slots {
        interaction.parts.remove_generated_slot(slot.slot);
        items.remove_generated_slot(slot.slot);
    }
    if let Some(item) = content.item {
        interaction
            .mechanisms
            .remove_generated(item, &content.nodes);
    }
    super::content_conditions::unbind(content, interaction);
    for (_, collider) in &instance.colliders {
        physics.edit_collider(*collider, |collider| {
            collider.user_data &= STEP_SURFACE_MASK;
            collider.set_active_events(ActiveEvents::empty());
        });
    }
    content.item = None;
    content.baseline = None;
    content.render_body = None;
    content.bindings.clear();
    content.slots.clear();
    content.caps.clear();
    content.nodes.clear();
    content.poses.clear();
    content.mechanism_colliders.clear();
}

pub fn restore_item(
    id: crate::reference::ItemId,
    snapshot: &crate::world::ObjectSnapshot,
    reference: &ReferenceManifest,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
    wheels: &mut crate::interaction::wheels::WheelAssemblies,
) -> Result<bool, ReferenceError> {
    use crate::interaction::items::ItemLocation;
    let owner = snapshot.attachment.as_ref().and_then(|a| a.owner.as_ref());
    let resolved = owner.and_then(|key| items.resolve_key(key));
    if owner.is_some() && resolved.is_none() {
        return Ok(false);
    }
    let slot = snapshot.attachment.as_ref().and_then(|a| a.slot.as_ref());
    // A saved hitch waits for the target and the trailer of the grip.
    if let Some(target) = resolved
        .zip(slot)
        .and_then(|(owner, source)| interaction.hitch.resolve_target(owner, source))
    {
        if !interaction.hitch.is_grip(id) {
            return Ok(false);
        }
        items.restore_generated(id, snapshot)?;
        items.get_mut(id).unwrap().pending_attachment = false;
        interaction.hitch.attach(id, target, items, physics)?;
        return Ok(true);
    }
    let volume = snapshot.attachment.as_ref().and_then(|a| a.volume.as_ref());
    let target_slot = slot.and_then(|source| interaction.parts.resolve_slot(resolved, source));
    // A world car slot mounts its rim from the car slot record.
    let world_wheel = slot.and_then(|source| interaction.resolve_wheel_slot(resolved, source));
    if slot.is_some() && target_slot.is_none() && world_wheel.is_none() {
        return Ok(false);
    }
    let current = items.get(id).unwrap().location;
    if let ItemLocation::PartSlot(current_slot) = current {
        if target_slot != Some(current_slot) {
            interaction.parts.remove(id, items, physics)?;
            interaction
                .parts
                .detach_mechanism(id, &mut interaction.mechanisms, physics);
        }
    }
    if let ItemLocation::Attached(parent) = current {
        if resolved != Some(parent) && wheels.rules().iter().any(|r| r.item == id) {
            wheels.split(parent, items, physics)?;
        }
    }
    if let Some(slot) = target_slot {
        interaction.parts.validate_saved_slot(id, slot)?;
        interaction
            .parts
            .initialize_generated_slot(id, slot, items, physics)?;
        interaction
            .parts
            .mount_mechanism(id, &mut interaction.mechanisms);
    } else if let Some(parent) = resolved.filter(|_| volume.is_none()) {
        if slot.is_none()
            && items.get(id).unwrap().location != ItemLocation::Attached(parent)
            && wheels.rules().iter().any(|r| r.item == id)
        {
            wheels.assemble(parent, id, items, physics)?;
        }
    }
    if let (Some(parent_id), Some(attachment)) = (resolved, &snapshot.attachment) {
        if attachment.slot.is_none()
            && volume.is_none()
            && !wheels.rules().iter().any(|r| r.item == id)
        {
            let parent = items.get(parent_id).unwrap();
            let Some(parent_body) = parent.body else {
                return Ok(false);
            };
            let item = items.get(id).unwrap();
            let Some(old_body) = item.body else {
                return Ok(false);
            };
            if old_body != parent_body {
                if items
                    .iter()
                    .any(|other| other.body == Some(old_body) && !items.attached_to(other.id, id))
                {
                    return Err(ReferenceError(
                        "Saved attachment shares an unrelated body".into(),
                    ));
                }
                let local = crate::vehicle::VehiclePose {
                    position: attachment.local_pose.position.map(|v| v as f32),
                    rotation: attachment.local_pose.rotation,
                };
                let transform = to_rapier(parent.local_pose)
                    * to_rapier(local)
                    * to_rapier(item.local_pose).inverse();
                let colliders = physics.rapier().bodies[old_body].colliders().to_vec();
                for collider in colliders {
                    let local = *physics.rapier().colliders[collider]
                        .position_wrt_parent()
                        .unwrap();
                    physics.set_collider_parent(collider, Some(parent_body));
                    physics.edit_collider(collider, |state| {
                        state.set_position_wrt_parent(transform * local)
                    });
                }
                let pose = *physics.rapier().bodies[parent_body].position();
                let children: Vec<_> = items
                    .iter()
                    .filter(|i| i.body == Some(old_body))
                    .map(|i| i.id)
                    .collect();
                for child_id in children {
                    let child = items.set_body(child_id, Some(parent_body)).unwrap();
                    child.local_pose = from_rapier(&(transform * to_rapier(child.local_pose)));
                    child.pose = from_rapier(&(pose * to_rapier(child.local_pose)));
                    child.inactive_pose = None;
                }
                physics.edit_body(old_body, |body| body.set_enabled(false));
            }
        }
    }
    match (&snapshot.food, items.get(id).and_then(|i| i.food)) {
        (Some(saved), Some(food)) => {
            let recipe = &reference.survival.foods[food.prototype_index];
            if saved.source != recipe.source || saved.bite > recipe.bites {
                return Err(ReferenceError(
                    "Saved edible recipe or bite is invalid".into(),
                ));
            }
            items.get_mut(id).unwrap().food.as_mut().unwrap().bite = saved.bite;
        }
        (None, None) => (),
        _ => {
            return Err(ReferenceError(
                "Saved edible owner differs from source".into(),
            ));
        }
    }
    items.restore_generated(id, snapshot)?;
    items.get_mut(id).unwrap().pending_attachment = false;
    if snapshot.held {
        items.get_mut(id).unwrap().location = ItemLocation::Held;
        if let Some(handle) = items.get(id).and_then(|i| i.body) {
            physics.edit_body(handle, |body| {
                if body.is_kinematic() {
                    body.set_body_type(rapier3d::prelude::RigidBodyType::Dynamic, false);
                }
            });
        }
    }
    if let Some(valve) = snapshot.valve {
        interaction.restore_generated_valve(id, valve)?;
    }
    if let Some(mechanism) = snapshot.mechanism {
        interaction.mechanisms.restore(id, mechanism)?;
    }
    interaction
        .conditions
        .restore_item(id, &snapshot.conditions)?;
    if let (Some(parent), Some(attachment), Some(_)) = (resolved, &snapshot.attachment, volume) {
        changes::restore_lock(
            id,
            parent,
            attachment,
            items,
            &mut interaction.storage,
            physics,
        )?;
    } else if let (Some(parent), Some(attachment)) = (resolved, &snapshot.attachment) {
        let parent = items.get(parent).unwrap().clone();
        let local = crate::vehicle::VehiclePose {
            position: attachment.local_pose.position.map(|v| v as f32),
            rotation: attachment.local_pose.rotation,
        };
        let item = items.get_mut(id).unwrap();
        if attachment.slot.is_none() {
            item.location = ItemLocation::Attached(parent.id);
        }
        item.pose = from_rapier(&(to_rapier(parent.pose) * to_rapier(local)));
        item.local_pose = from_rapier(&(to_rapier(parent.local_pose) * to_rapier(local)));
    } else {
        crate::world::changes::restore_motion(id, snapshot, items, physics);
    }
    Ok(true)
}
