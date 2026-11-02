use super::*;
use crate::reference::{self as r, ReferenceManifest};
use std::collections::{HashMap, HashSet};

impl SaveSnapshot {
    pub fn validate(
        &self,
        reference: &ReferenceManifest,
        expected: &SourceStamp,
    ) -> Result<(), SaveError> {
        if !schema_supported(self.schema_version) {
            return Err(SaveError::IncompatibleSources("Schema version".into()));
        }
        data(
            !self.driving_assists.automatic_gears || self.driving_assists.automatic_clutch,
            "Driving assist dependency",
        )?;
        validate_source(&self.source, expected, reference)?;
        data(timestamp(&self.saved_at), "Save timestamp")?;
        let ids = IdentityMap::new(reference)?;
        self.validate_world(reference)?;
        identity::unsigned(&self.progress.start_attempts)?;
        data(
            self.progress.driven_km.is_finite() && self.progress.driven_km >= 0.,
            "Driven distance",
        )?;
        self.validate_object_states()?;
        let mut items = HashMap::new();
        for item in &self.items {
            let id = ids.item(&item.instance)?;
            links(items.insert(id, item).is_none(), "Duplicate item")?;
            if id == reference.survival.starter_food {
                let food = item
                    .food
                    .as_ref()
                    .ok_or_else(|| invalid("Missing food progress"))?;
                let source = reference
                    .survival
                    .foods
                    .iter()
                    .find(|p| p.kind == r::FoodKind::Sausage)
                    .unwrap();
                links(
                    food.source == SourceKey::from(&source.source),
                    "Food source",
                )?;
                data(
                    food.bite <= source.bites
                        && item.removed == (source.destroy_on_end && food.bite == source.bites),
                    "Food progress",
                )?;
            } else {
                links(
                    item.food.is_none()
                        && (!item.removed
                            || crate::starting_area::explosive_imported(reference, id)),
                    "Unexpected removed item or food progress",
                )?;
            }
            links(
                !item.removed || item.location == ItemLocationSave::World,
                "Removed item ownership",
            )?;
            absolute(&item.pose)?;
            local(&item.local_pose)?;
            links(item.temperature.is_none(), "Item temperature owner")?;
            let tank = reference.tanks.iter().find(|tank| tank.item == id);
            match (&item.fluids, tank) {
                (Some(entries), Some(tank)) => data(
                    crate::fluids::validate_fluid_entries(entries)
                        && tank.capacity_liters.is_finite()
                        && tank.capacity_liters > 0.,
                    "Fluid entries",
                )?,
                (None, None) => (),
                _ => {
                    return Err(SaveError::InvalidLinks(
                        "Missing or unexpected fluid container".into(),
                    ));
                }
            }
        }
        links(items.len() == reference.items.len(), "Missing source items")?;
        let mut spawned = HashSet::new();
        for record in &self.spawned {
            let (prototype, _) = crate::starting_area::spawned::saved_prototype(reference, record)?;
            if let Some(count) = record.ammo {
                let rules = reference.item_prototypes[prototype].behavior.ammo_rules;
                data(
                    rules.is_some_and(|r| {
                        count.is_finite()
                            && (0. ..=r.maximum).contains(&count)
                            && (r.float || count.fract() == 0.)
                    }),
                    "Spawned ammo",
                )?;
            }
            absolute(&record.pose)?;
            links(
                spawned.insert((record.serial, record.pickup)),
                "Duplicate spawned item",
            )?;
        }
        for record in &self.foods {
            let source = &reference.survival.foods
                [crate::starting_area::saved_food(reference, &record.source)?];
            data(
                record.bite <= source.bites
                    && !(source.destroy_on_end && record.bite == source.bites),
                "Food progress",
            )?;
            absolute(&record.pose)?;
        }
        crate::starting_area::cigarettes::validate_save(
            reference,
            &self.spawned,
            &self.cigarettes,
        )?;
        crate::starting_area::hose::validate_save(reference, &self.spawned)?;
        self.validate_attachments(reference, &ids, &items)?;
        self.validate_player(reference, &ids, &items)?;
        self.validate_needs(reference)?;
        self.validate_inventory(reference, &ids, &items)?;
        self.validate_worn(reference, &ids, &items)?;
        self.validate_bodies(reference, &ids, &items)?;
        self.validate_mechanisms(reference, &ids)?;
        self.validate_lighting(reference, &ids)?;
        validate_weather(&reference.weather, &self.weather)?;
        for rabbit in &self.rabbits {
            absolute(&rabbit.pose)?;
            data(
                unit(rabbit.size)
                    && rabbit.shoot_health.is_finite()
                    && rabbit.bones.len()
                        == if rabbit.alive {
                            0
                        } else {
                            reference.creatures.rabbit.ragdoll.bodies.len()
                        },
                "Rabbit state",
            )?;
            for bone in &rabbit.bones {
                absolute(bone)?;
            }
        }
        crate::conditions::validate_surfaces(reference, &self.conditions)?;
        crate::interaction::cleaning::validate_tools(reference, &self.conditions.tools)?;
        for brush in &reference.conditions.brushes {
            let tool = self
                .conditions
                .tools
                .iter()
                .find(|t| t.source == SourceKey::from(&brush.tool.source))
                .unwrap();
            let owner = reference
                .conditions
                .surfaces
                .iter()
                .find(|s| s.id == brush.own_condition)
                .unwrap();
            let surface = self
                .conditions
                .surfaces
                .iter()
                .find(|s| s.source == SourceKey::from(&owner.source))
                .unwrap();
            data(
                tool.disabled == (surface.state == 5 || brush.tool.disabled),
                "Tool disabled condition",
            )?;
        }
        Ok(())
    }

    /// Car, engine and radiator states in the world records, and the last seated car.
    fn validate_object_states(&self) -> Result<(), SaveError> {
        if let Some(key) = &self.progress.selected_vehicle {
            links(
                matches!(key, crate::world::ObjectKey::Generated(_)),
                "Selected car identity",
            )?;
        }
        let changes = &self.world.changes;
        // The binder checks the car record when the car spawns.
        for (_, state) in changes.vehicles() {
            data(
                state.trip_m.is_finite() && state.trip_m >= 0.,
                "Trip distance",
            )?;
            data(state.requested_headlights <= 2, "Requested headlights")?;
            data(
                state.steer.is_finite() && (-1.0..=1.0).contains(&state.steer),
                "Vehicle steer",
            )?;
        }
        for (_, e) in changes.engines() {
            super::identity::unsigned(&e.start_draw)?;
            data(
                finite(&[
                    e.rpm,
                    e.temperature,
                    e.elapsed,
                    e.last_run_time,
                    e.last_start_chance_time,
                    e.random_start,
                    e.supplied_fuel,
                    e.oil_penalty,
                ]) && e.elapsed >= 0.
                    && e.last_run_time >= -1.
                    && e.last_run_time <= e.elapsed
                    && e.last_start_chance_time >= 0.
                    && e.last_start_chance_time <= e.elapsed
                    && unit(e.random_start)
                    && unit(e.fuel_suitability)
                    && unit(e.heat_penalty)
                    && unit(e.oil_penalty)
                    && e.supplied_fuel >= 0.,
                "Engine state",
            )?;
        }
        Ok(())
    }

    fn validate_attachments<'a>(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        items: &HashMap<r::ItemId, &'a ItemSnapshot>,
    ) -> Result<HashMap<r::ItemId, r::ItemId>, SaveError> {
        let mut parents = HashMap::new();
        let mut part_slots = HashMap::new();
        let mut linked_parts = HashSet::new();
        for attachment in &self.attachments.parts {
            let id = *ids
                .part_slots
                .get(&attachment.slot)
                .ok_or_else(|| invalid("Unknown part slot"))?;
            links(
                part_slots.insert(id, attachment.part.as_ref()).is_none(),
                "Duplicate part slot",
            )?;
            let slot = reference
                .part_slots
                .iter()
                .find(|slot| slot.id == id)
                .unwrap();
            if let Some(instance) = &attachment.part {
                let item = ids.item(instance)?;
                links(linked_parts.insert(item), "Part has multiple owners")?;
                let part = reference
                    .parts
                    .iter()
                    .find(|part| part.item == item)
                    .ok_or_else(|| invalid("Unknown part"))?;
                links(
                    fits(
                        &slot.main_types,
                        &slot.part_types,
                        &part.main_types,
                        &part.part_types,
                    ),
                    "Incompatible part slot",
                )?;
                links(
                    items[&item].location
                        == ItemLocationSave::PartSlot {
                            slot: attachment.slot.clone(),
                        },
                    "Part location conflicts with slot",
                )?;
                parents.insert(item, slot.owner.ok_or_else(|| invalid("Part slot owner"))?);
            }
        }
        links(
            part_slots.len() == reference.part_slots.len(),
            "Missing part slot",
        )?;
        for (&id, item) in items {
            let source_item = reference.items.iter().find(|i| i.id == id).unwrap();
            let detachable = reference.parts.iter().any(|part| part.item == id)
                || reference.wheel_parts.iter().any(|part| part.item == id);
            // A destroyed item (an exploded can and its cap) leaves its owner.
            if let Some(owner) = source_item
                .attached_to
                .filter(|_| !detachable && !item.removed)
            {
                links(
                    matches!(&item.location, ItemLocationSave::Attached { owner: saved_owner }
                        if ids.item(saved_owner).ok() == Some(owner)),
                    "Fixed source attachment changed",
                )?;
            }
            match &item.location {
                ItemLocationSave::PartSlot { slot } => links(
                    ids.part_slots
                        .get(slot)
                        .and_then(|slot| part_slots.get(slot))
                        .copied()
                        .flatten()
                        == Some(&item.instance),
                    "Unlisted part attachment",
                )?,
                ItemLocationSave::Attached { owner } => {
                    let owner = ids.item(owner)?;
                    if matches!(source_item.kind, r::ItemKind::Tire) {
                        links(tire_fits(reference, owner, id), "Incompatible tire and rim")?;
                    } else {
                        links(
                            source_item.attached_to == Some(owner),
                            "Invalid source attachment",
                        )?;
                    }
                    parents.insert(id, owner);
                }
                ItemLocationSave::GeneratedAttachment { .. }
                | ItemLocationSave::World
                | ItemLocationSave::Held
                | ItemLocationSave::Stored { .. }
                | ItemLocationSave::Equipped => (),
            }
        }
        for &start in items.keys() {
            let mut seen = HashSet::new();
            let mut next = Some(start);
            while let Some(id) = next {
                links(seen.insert(id), "Attachment cycle")?;
                next = parents.get(&id).copied();
            }
        }
        Ok(parents)
    }

    fn validate_needs(&self, reference: &ReferenceManifest) -> Result<(), SaveError> {
        let n = &self.needs;
        let maximum = reference.survival.maximum;
        data(
            [
                (n.needs.hp, maximum.hp),
                (n.needs.food, maximum.food),
                (n.needs.water, maximum.water),
                (n.needs.bowel, maximum.bowel),
            ]
            .iter()
            .all(|(value, max)| value.is_finite() && (0.0..=*max).contains(value))
                && n.needs.hp > 0.,
            "Player needs",
        )?;
        data(
            finite(&[
                n.pending.food,
                n.pending.water,
                n.pending.bowel,
                n.pending.hp,
            ]),
            "Pending needs",
        )?;
        let h = &self.hearing;
        data(
            finite(&[h.hearing, h.ringing, h.target, h.dir]) && h.hearing <= 1.,
            "Player hearing",
        )?;
        let bladder: f32 = n.bladder.as_slice().iter().map(|e| e.amount).sum();
        data(
            bladder <= reference.survival.bladder_capacity_liters + 1e-4,
            "Bladder fluids",
        )
    }

    fn validate_player(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        items: &HashMap<r::ItemId, &ItemSnapshot>,
    ) -> Result<(), SaveError> {
        let p = &self.player;
        absolute(&p.pose)?;
        data(
            p.posture.is_finite() && (0.0..=1.0).contains(&p.posture),
            "Player posture",
        )?;
        data(
            p.seat.is_none() || p.posture == 1.0,
            "Seated player posture",
        )?;
        data(
            finite(&[p.yaw, p.pitch, p.vertical_velocity])
                && (-89.0..=89.0).contains(&p.pitch)
                && bounded_local(&p.recoil_velocity)
                && bounded_local(&p.momentum)
                && bounded_local(&[p.vertical_velocity]),
            "Player motion",
        )?;
        data(p.since_wake.is_finite(), "Player sleep cooldown")?;
        if let Some(seat) = &p.seat {
            local(&seat.local_pose)?;
            match &seat.owner {
                // A world car seat binds with its car. The seat binding checks the source.
                SeatOwnerSnapshot::Vehicle { vehicle } => {
                    links(
                        matches!(vehicle, crate::world::ObjectKey::Generated(_))
                            && Some(vehicle) == self.progress.selected_vehicle.as_ref(),
                        "Selected seat vehicle",
                    )?;
                    seat.source.source_id()?;
                }
                // validate_world checks the item key. The seat binding checks the source at restore.
                SeatOwnerSnapshot::Item { .. } => {
                    seat.source.source_id()?;
                }
            }
        }
        let held: Vec<_> = items
            .values()
            .filter(|item| item.location == ItemLocationSave::Held)
            .collect();
        match &p.carry {
            Some(carry) => {
                if let InstanceId::Generated { id } = &carry.item {
                    links(held.is_empty(), "Multiple held owners")?;
                    let state = self
                        .world
                        .changes
                        .get(crate::world::ObjectKey::Generated(id.clone()))
                        .and_then(|c| c.snapshot.as_ref())
                        .ok_or_else(|| invalid("Missing generated carry state"))?;
                    links(
                        state.held && state.attachment.is_none(),
                        "Generated carry ownership",
                    )?;
                    data(
                        state
                            .pose
                            .position
                            .iter()
                            .zip(p.pose.position)
                            .all(|(a, b)| (*a - b).abs() <= f64::from(crate::world::REBASE_LIMIT)),
                        "Held relative position",
                    )?;
                    data(bounded_local(&carry.offset), "Carry offset")?;
                    quaternion(&carry.rotation)?;
                    local(&carry.local_grip)?;
                    return Ok(());
                }
                let id = ids.item(&carry.item)?;
                links(
                    held.len() == 1 && held[0].instance == carry.item,
                    "Held item ownership",
                )?;
                let pickup = reference
                    .items
                    .iter()
                    .find(|i| i.id == id)
                    .unwrap()
                    .pickup
                    .as_ref();
                links(
                    pickup.is_some_and(|rule| {
                        !rule.cant_pickup && !(p.seat.is_some() && rule.cant_pickup_while_seated)
                    }),
                    "Item cannot be held",
                )?;
                data(
                    items[&id]
                        .pose
                        .position
                        .iter()
                        .zip(p.pose.position)
                        .all(|(a, b)| (*a - b).abs() <= f64::from(crate::world::REBASE_LIMIT)),
                    "Held relative position",
                )?;
                let source_item = reference.items.iter().find(|i| i.id == id).unwrap();
                let assembly = reference.parts.iter().any(|p| p.item == id)
                    || reference.wheel_parts.iter().any(|p| p.item == id);
                let body_position = if assembly {
                    self.bodies.iter().find(|b|matches!(&b.identity,BodyIdentity::Assembly {root} if *root==carry.item)).map(|b|b.pose.position)
                } else {
                    let source = reference
                        .bodies
                        .iter()
                        .find(|b| b.id == source_item.body)
                        .unwrap();
                    Some(self.bodies.iter().find(|b|matches!(&b.identity,BodyIdentity::Source {source:key} if *key==SourceKey::from(&source.source))).map_or(source.pose.translation.map(f64::from),|b|b.pose.position))
                };
                links(body_position.is_some(), "Missing held body")?;
                data(
                    body_position
                        .unwrap()
                        .iter()
                        .zip(p.pose.position)
                        .all(|(a, b)| (*a - b).abs() <= f64::from(crate::world::REBASE_LIMIT)),
                    "Held body relative position",
                )?;
                data(bounded_local(&carry.offset), "Carry offset")?;
                quaternion(&carry.rotation)?;
                local(&carry.local_grip)?;
            }
            None => links(held.is_empty(), "Missing carry state")?,
        }
        Ok(())
    }

    /// Each worn item is a wearable of its slot type, lies free in the world, and is worn once.
    fn validate_worn(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        items: &HashMap<r::ItemId, &ItemSnapshot>,
    ) -> Result<(), SaveError> {
        let inventory = &self.inventory;
        // The wear slot of a pickup component.
        let slot_of = |source: &r::SourceId| {
            reference
                .clothing
                .wearables
                .iter()
                .find(|w| {
                    reference.item_prototypes.iter().any(|p| {
                        p.id == w.prototype && p.pickups.iter().any(|k| k.pickup.source == *source)
                    })
                })
                .map(|w| w.type_index as usize)
        };
        for (slot, worn) in inventory.worn.iter().enumerate() {
            let Some(worn) = worn else { continue };
            links(
                !inventory.worn[..slot].contains(&Some(worn.clone())),
                "Duplicate worn item",
            )?;
            match worn {
                WornSnapshot::Spawned { serial } => {
                    let record = self
                        .spawned
                        .iter()
                        .find(|s| s.serial == *serial)
                        .ok_or_else(|| invalid("Unknown worn serial"))?;
                    let (prototype, pickup) =
                        crate::starting_area::spawned::saved_prototype(reference, record)?;
                    let source = &reference.item_prototypes[prototype].pickups[pickup]
                        .pickup
                        .source;
                    links(slot_of(source) == Some(slot), "Worn item slot")?;
                }
                WornSnapshot::Item { item } => {
                    links(
                        !inventory.slots.contains(&Some(item.clone()))
                            && inventory.equipped.as_ref() != Some(item)
                            && self.player.carry.as_ref().is_none_or(|c| &c.item != item),
                        "Worn item ownership",
                    )?;
                    if let InstanceId::Generated { id } = item {
                        // The pickup rule of a generated item binds later. Its restore checks the slot.
                        let state = self
                            .world
                            .changes
                            .get(crate::world::ObjectKey::Generated(id.clone()))
                            .and_then(|c| c.snapshot.as_ref())
                            .ok_or_else(|| invalid("Missing generated worn item"))?;
                        links(
                            !state.held && state.attachment.is_none(),
                            "Worn item ownership",
                        )?;
                        continue;
                    }
                    let id = ids.item(item)?;
                    let rule = reference
                        .items
                        .iter()
                        .find(|i| i.id == id)
                        .and_then(|i| i.pickup.as_ref());
                    links(
                        rule.is_some_and(|p| slot_of(&p.source) == Some(slot))
                            && !items[&id].removed
                            && items[&id].location == ItemLocationSave::World,
                        "Worn item slot",
                    )?;
                }
            }
        }
        Ok(())
    }

    fn validate_inventory(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        items: &HashMap<r::ItemId, &ItemSnapshot>,
    ) -> Result<(), SaveError> {
        let saved = &self.inventory;
        data(saved.selected < 4, "Selected inventory slot")?;
        let generated_state =
            |instance: &InstanceId| -> Result<&crate::world::ObjectSnapshot, SaveError> {
                let InstanceId::Generated { id } = instance else {
                    unreachable!()
                };
                let change = self
                    .world
                    .changes
                    .get(crate::world::ObjectKey::Generated(id.clone()))
                    .ok_or_else(|| invalid("Missing generated inventory item"))?;
                let snapshot = change
                    .snapshot
                    .as_ref()
                    .ok_or_else(|| invalid("Removed generated inventory item"))?;
                links(snapshot.attachment.is_none(), "Attached inventory item")?;
                Ok(snapshot)
            };
        let mut generated_members = HashSet::new();
        let mut members = HashMap::new();
        for (slot, instance) in saved.slots.iter().enumerate() {
            if let Some(instance) = instance {
                if matches!(instance, InstanceId::Generated { .. }) {
                    links(
                        generated_members.insert(instance),
                        "Duplicate inventory member",
                    )?;
                    let state = generated_state(instance)?;
                    let selected = slot as u8 == saved.selected;
                    let carried = self
                        .player
                        .carry
                        .as_ref()
                        .is_some_and(|c| &c.item == instance);
                    let equipped = saved.equipped.as_ref() == Some(instance);
                    links(
                        if selected {
                            carried != equipped
                        } else {
                            !carried && !equipped
                        },
                        "Inventory location",
                    )?;
                    links(state.held == carried, "Generated inventory carry ownership")?;
                    continue;
                }
                let id = ids.item(instance)?;
                links(
                    members.insert(id, slot as u8).is_none(),
                    "Duplicate inventory member",
                )?;
                let rule = reference
                    .items
                    .iter()
                    .find(|i| i.id == id)
                    .and_then(|i| i.pickup.as_ref());
                links(
                    rule.is_some_and(|p| p.can_inventory && !p.cant_pickup) && !items[&id].removed,
                    "Item cannot enter inventory",
                )?;
                let location = &items[&id].location;
                links(
                    if slot as u8 == saved.selected {
                        matches!(
                            location,
                            ItemLocationSave::Held | ItemLocationSave::Equipped
                        )
                    } else {
                        *location == ItemLocationSave::Stored { slot: slot as u8 }
                    },
                    "Inventory location",
                )?;
            }
        }
        if let Some(instance @ InstanceId::Generated { .. }) = &saved.equipped {
            let state = generated_state(instance)?;
            links(
                !state.held
                    && self
                        .player
                        .carry
                        .as_ref()
                        .is_none_or(|c| &c.item != instance),
                "Equipment ownership",
            )?;
        }
        let equipped = saved
            .equipped
            .as_ref()
            .filter(|i| matches!(i, InstanceId::Initial { .. }))
            .map(|i| ids.item(i))
            .transpose()?;
        for (&id, item) in items {
            match item.location {
                ItemLocationSave::Stored { slot } => {
                    links(members.get(&id) == Some(&slot), "Unlisted stored item")?
                }
                ItemLocationSave::Equipped => {
                    links(equipped == Some(id), "Unlisted equipped item")?
                }
                _ => (),
            }
        }
        if let Some(id) = equipped {
            let rule = reference
                .items
                .iter()
                .find(|i| i.id == id)
                .and_then(|i| i.pickup.as_ref());
            links(
                rule.is_some_and(|p| p.can_hold_right_hand && !p.cant_pickup)
                    && !items[&id].removed
                    && items[&id].location == ItemLocationSave::Equipped,
                "Equipment ownership",
            )?;
            links(
                self.player
                    .carry
                    .as_ref()
                    .is_none_or(|c| saved.equipped.as_ref() != Some(&c.item)),
                "Carry and equipment share an item",
            )?;
        }
        Ok(())
    }

    fn validate_bodies(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        items: &HashMap<r::ItemId, &ItemSnapshot>,
    ) -> Result<(), SaveError> {
        let mut identities = HashSet::new();
        let mut assemblies = HashSet::new();
        for record in &self.bodies {
            links(
                identities.insert(&record.identity),
                "Duplicate independent body",
            )?;
            body(record)?;
            match &record.identity {
                BodyIdentity::Source { source } => {
                    let id = ids
                        .bodies
                        .get(source)
                        .ok_or_else(|| invalid("Unknown source body"))?;
                    let source_body = reference.bodies.iter().find(|body| body.id == *id).unwrap();
                    links(
                        source_body.parent_body.is_none(),
                        "Parented source body is not independent",
                    )?;
                }
                BodyIdentity::Assembly { root } => {
                    let id = ids.item(root)?;
                    links(
                        reference.parts.iter().any(|p| p.item == id)
                            || reference.wheel_parts.iter().any(|p| p.item == id),
                        "Unknown assembly root",
                    )?;
                    links(
                        matches!(
                            items[&id].location,
                            ItemLocationSave::World
                                | ItemLocationSave::Held
                                | ItemLocationSave::Stored { .. }
                                | ItemLocationSave::Equipped
                        ),
                        "Assembly root is attached",
                    )?;
                    assemblies.insert(id);
                }
            }
        }
        for part in reference
            .parts
            .iter()
            .map(|p| p.item)
            .chain(reference.wheel_parts.iter().map(|p| p.item))
        {
            if matches!(
                items[&part].location,
                ItemLocationSave::World
                    | ItemLocationSave::Held
                    | ItemLocationSave::Stored { .. }
                    | ItemLocationSave::Equipped
            ) {
                links(assemblies.contains(&part), "Missing detached assembly body")?;
            }
        }
        Ok(())
    }

    fn validate_mechanisms(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
    ) -> Result<(), SaveError> {
        let mut controls = HashSet::new();
        for saved in &self.mechanisms.controls {
            let id = ids
                .mechanisms
                .get(&saved.source)
                .ok_or_else(|| invalid("Unknown mechanism"))?;
            links(controls.insert(id), "Duplicate mechanism")?;
            let source = reference.mechanisms.iter().find(|m| m.id == *id).unwrap();
            data(
                finite(&saved.degrees) && unit(saved.slide),
                "Mechanism values",
            )?;
            match &source.rotation {
                Some(rotation) => data(
                    (0..2).all(|axis| {
                        saved.degrees[axis] == rotation.initial_degrees[axis]
                            || (rotation.limits_degrees[axis][0]..=rotation.limits_degrees[axis][1])
                                .contains(&saved.degrees[axis])
                    }),
                    "Mechanism rotation limits",
                )?,
                None => data(saved.degrees == [0.; 2], "Unexpected mechanism rotation")?,
            }
            if source.slide.is_none() {
                data(saved.slide == 0., "Unexpected slide value")?;
            }
            data(
                saved.turn_state <= source.turn.as_ref().map_or(0, |turn| turn.max_state),
                "Mechanism turn state",
            )?;
        }
        links(
            controls.len() == reference.mechanisms.len(),
            "Missing mechanism",
        )?;
        let mut openings = HashSet::new();
        for saved in &self.mechanisms.openings {
            let id = ids
                .openings
                .get(&saved.source)
                .ok_or_else(|| invalid("Unknown opening"))?;
            links(openings.insert(id), "Duplicate opening")?;
            data(unit(saved.valve), "Opening valve")?;
        }
        Ok(())
    }

    fn validate_lighting(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
    ) -> Result<(), SaveError> {
        let cycle = &self.lighting.cycle;
        let duration = f64::from(
            reference.lighting.cycle.day_seconds + reference.lighting.cycle.night_seconds,
        );
        data(
            cycle.elapsed_seconds.is_finite()
                && (cycle.elapsed_seconds as f32).is_finite()
                && cycle.elapsed_seconds >= 0.
                && cycle.cycle_seconds.is_finite()
                && cycle.cycle_seconds >= 0.
                && cycle.cycle_seconds as f32 <= duration as f32
                && cycle.evaluated_cycle_seconds.is_finite()
                && (cycle.evaluated_cycle_seconds as f32).is_finite()
                && cycle.evaluated_cycle_seconds >= 0.
                && cycle.cloud_coordinate.is_finite()
                && (-1.0..=0.0).contains(&cycle.cloud_coordinate)
                && cycle.star_rotation.is_finite()
                && (-360.0..360.0).contains(&cycle.star_rotation),
            "Cycle state",
        )?;
        let mut controllers = HashSet::new();
        for saved in &self.lighting.controllers {
            let id = ids
                .controllers
                .get(&saved.source)
                .ok_or_else(|| invalid("Unknown lamp controller"))?;
            links(controllers.insert(id), "Duplicate lamp controller")?;
            let source = reference
                .lighting
                .controllers
                .iter()
                .find(|c| c.id == *id)
                .unwrap();
            data(
                [
                    saved.current_state,
                    saved.requested_state,
                    saved.refreshed_state,
                ]
                .iter()
                .all(|state| *state < source.states.len())
                    && unit(saved.energy),
                "Lamp controller state",
            )?;
            let mut poses = HashSet::new();
            for pose in &saved.switch_poses {
                let node = ids
                    .nodes
                    .get(&pose.node)
                    .ok_or_else(|| invalid("Unknown switch node"))?;
                links(
                    poses.insert(node) && source.animated_poses.iter().any(|p| p.node == *node),
                    "Duplicate or invalid switch node",
                )?;
                local(&pose.pose)?;
            }
            links(
                poses.len() == source.animated_poses.len(),
                "Missing switch pose",
            )?;
            if let Some(motion) = saved.switch_motion {
                links(
                    !source.animated_poses.is_empty(),
                    "Switch motion without animated poses",
                )?;
                if let SwitchMotionSave::Moving { progress } = motion {
                    data(unit(progress), "Switch motion")?;
                }
            }
        }
        links(
            controllers.len() == reference.lighting.controllers.len(),
            "Missing lamp controller",
        )
    }
}

fn validate_source(
    saved: &SourceStamp,
    expected: &SourceStamp,
    reference: &ReferenceManifest,
) -> Result<(), SaveError> {
    fn hashes<'a>(
        hashes: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<HashMap<&'a str, &'a str>, SaveError> {
        let mut values = HashMap::new();
        for (file, hash) in hashes {
            data(!file.is_empty() && sha256(hash), "Source hash")?;
            data(values.insert(file, hash).is_none(), "Duplicate source hash")?;
        }
        data(!values.is_empty(), "Missing source hashes")?;
        Ok(values)
    }
    data(
        sha256(&saved.manifest_sha256) && sha256(&expected.manifest_sha256),
        "Manifest hash",
    )?;
    let saved_hashes = hashes(
        saved
            .source_hashes
            .iter()
            .map(|h| (h.file.as_str(), h.sha256.as_str())),
    )?;
    let expected_hashes = hashes(
        expected
            .source_hashes
            .iter()
            .map(|h| (h.file.as_str(), h.sha256.as_str())),
    )?;
    let source_hashes = hashes(
        reference
            .source_hashes
            .iter()
            .map(|h| (h.file.as_str(), h.sha256.as_str())),
    )?;
    if saved.app_id != expected.app_id
        || saved.build_id != expected.build_id
        || expected.app_id != reference.app_id
        || expected.build_id != reference.build_id
        || saved.manifest_sha256 != expected.manifest_sha256
        || saved_hashes != expected_hashes
        || expected_hashes != source_hashes
    {
        return Err(SaveError::IncompatibleSources(
            "Build, manifest, or source-file hashes".into(),
        ));
    }
    Ok(())
}
fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate_weather(
    reference: &r::WeatherReference,
    saved: &WeatherSnapshot,
) -> Result<(), SaveError> {
    data(
        saved.since_spawn.is_finite()
            && saved.since_spawn >= 0.
            && saved.wait.is_finite()
            && saved.wait > 0.,
        "Storm spawn timer",
    )?;
    let within = |value: f32, range: [f32; 2]| (range[0]..=range[1]).contains(&value);
    for storm in &saved.storms {
        links(storm.prefab == reference.prefab_id, "Storm prefab")?;
        data(
            storm
                .position
                .iter()
                .all(|v| v.is_finite() && v.abs() <= crate::world::MAX_ABSOLUTE_COORDINATE)
                && finite(&[storm.yaw])
                && finite(&storm.direction)
                && unit(storm.life)
                && within(storm.size, reference.storm.size_range)
                && within(storm.speed, reference.storm.speed_range),
            "Storm state",
        )?;
    }
    Ok(())
}
fn finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}
fn bounded_local(values: &[f32]) -> bool {
    values
        .iter()
        .all(|v| v.is_finite() && v.abs() <= crate::world::REBASE_LIMIT)
}
fn unit(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn quaternion(rotation: &[f32; 4]) -> Result<(), SaveError> {
    data(
        finite(rotation)
            && (rotation.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() - 1.).abs() <= 0.0001,
        "Normalized rotation",
    )
}
fn absolute(pose: &AbsolutePose) -> Result<(), SaveError> {
    data(
        pose.position
            .iter()
            .all(|v| v.is_finite() && v.abs() <= crate::world::MAX_ABSOLUTE_COORDINATE),
        "Absolute position",
    )?;
    quaternion(&pose.rotation)
}
fn local(pose: &LocalPose) -> Result<(), SaveError> {
    data(bounded_local(&pose.position), "Local position")?;
    quaternion(&pose.rotation)
}
fn body(body: &BodySnapshot) -> Result<(), SaveError> {
    absolute(&body.pose)?;
    data(
        bounded_local(&body.linear_velocity) && bounded_local(&body.angular_velocity),
        "Body velocity",
    )
}
fn fits(
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
/// Source rim item `rim` accepts source tire item `tire`.
fn tire_fits(reference: &ReferenceManifest, rim: r::ItemId, tire: r::ItemId) -> bool {
    let part = |id| reference.wheel_parts.iter().find(|p| p.item == id);
    let kind = |id| reference.items.iter().find(|i| i.id == id).map(|i| &i.kind);
    let (Some(rim_part), Some(tire_part)) = (part(rim), part(tire)) else {
        return false;
    };
    matches!(kind(rim), Some(r::ItemKind::Rim))
        && fits(
            &rim_part.tire_main_types,
            &rim_part.tire_types,
            &tire_part.main_types,
            &tire_part.part_types,
        )
}
fn invalid(detail: &str) -> SaveError {
    SaveError::InvalidLinks(detail.into())
}
fn links(valid: bool, detail: &str) -> Result<(), SaveError> {
    if valid { Ok(()) } else { Err(invalid(detail)) }
}
fn data(valid: bool, detail: &str) -> Result<(), SaveError> {
    if valid {
        Ok(())
    } else {
        Err(SaveError::CorruptData(detail.into()))
    }
}

// Browser Date.toISOString emits UTC, with optional fractional seconds accepted here.
fn timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes.last() != Some(&b'Z')
    {
        return false;
    }
    let number = |start: usize, end: usize| -> Option<u32> {
        let digits = bytes.get(start..end)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        number(0, 4),
        number(5, 7),
        number(8, 10),
        number(11, 13),
        number(14, 16),
        number(17, 19),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return false,
    };
    day >= 1
        && day <= days
        && hour < 24
        && minute < 60
        && second < 60
        && (bytes.len() == 20
            || (bytes.len() > 21
                && bytes[19] == b'.'
                && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)))
}

#[path = "validate_world.rs"]
mod world;
