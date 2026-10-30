mod finalize;

use super::{GarageRuntime, StartingArea};
use crate::reference::ReferenceManifest;
use crate::save::*;
use crate::vehicle::{PhysicsWorld, capture_body_save};
use crate::world::WorldOrigin;
use std::collections::HashSet;

pub(super) struct PendingRestore {
    items: Vec<ItemSnapshot>,
    carry: Option<CarrySnapshot>,
    inventory: Option<InventorySnapshot>,
    ids: IdentityMap,
    ready: bool,
    motion: Vec<BodySnapshot>,
    /// A saved item seat. It waits for the world item seat binding.
    seat: Option<SeatSnapshot>,
}
impl GarageRuntime {
    /// Source `mainscript.Update` autosave state, without the seat and the toggles.
    /// `grab_held` is the pickup and push key (F) in this frame.
    pub fn autosave_safe(&self, grab_held: bool) -> bool {
        let area = &self.area;
        area.player.grounded()
            && area.interaction.held().is_none()
            && !area.interaction.push_gripping()
            && !self.sleeping()
            && !grab_held
    }
    pub fn restore_pending(&self) -> bool {
        self.pending_restore.as_ref().is_some_and(|p| !p.ready)
    }
    pub fn has_deferred_save_links(&self) -> bool {
        self.pending_restore.is_some()
    }

    /// Poll after bounded source publication and binding. Never advance physics here.
    pub fn advance_saved_restore(
        &mut self,
        reference: &ReferenceManifest,
    ) -> Result<bool, SaveError> {
        let Some(pending) = &mut self.pending_restore else {
            return Ok(true);
        };
        let ids = &mut pending.ids;
        let before = pending.items.len();
        let mut index = 0;
        // Imported edges are bounded by the immutable imported item set.
        while index < pending.items.len() {
            let item = &pending.items[index];
            let ItemLocationSave::GeneratedAttachment { attachment } = &item.location else {
                unreachable!()
            };
            if attachment
                .owner
                .as_ref()
                .is_some_and(|key| !self.stream.save_owner_ready(key, &self.area.items))
            {
                index += 1;
                continue;
            }
            let snapshot = crate::world::ObjectSnapshot {
                conditions: Vec::new(),
                pose: crate::world::AbsolutePose {
                    position: item.pose.position,
                    rotation: item.pose.rotation,
                },
                food: self
                    .area
                    .items
                    .get(ids.item(&item.instance)?)
                    .and_then(|i| i.food)
                    .map(|food| crate::world::changes::FoodSnapshot {
                        source: reference.survival.foods[food.prototype_index]
                            .source
                            .clone(),
                        bite: food.bite,
                    }),
                temperature: item.temperature,
                fluids: self
                    .area
                    .items
                    .get(ids.item(&item.instance)?)
                    .and_then(|item| item.fluid.as_ref())
                    .map(crate::fluids::FluidSnapshot::from),
                valve: None,
                lamp: None,
                lamp_broken: None,
                dial: None,
                ammo: None,
                glow: None,
                radio: None,
                detector: None,
                tool: None,
                gun: None,
                linear_velocity: [0.; 3],
                angular_velocity: [0.; 3],
                sleeping: false,
                held: false,
                buried: false,
                attachment: Some(attachment.clone()),
                mechanism: None,
            };
            if crate::world::stream::content_bindings::restore_item(
                ids.item(&item.instance)?,
                &snapshot,
                reference,
                &mut self.area.items,
                &mut self.area.interaction,
                &mut self.physics,
                &mut self.wheels,
            )? {
                pending.items.swap_remove(index);
            } else {
                index += 1;
            }
        }
        if pending
            .items
            .iter()
            .any(|item| self.stream.save_link_required(item.pose.position))
        {
            pending.ready = false;
            return Ok(false);
        }
        if pending.ready && before == pending.items.len() {
            return Ok(true);
        }
        if !self.stream.saved_bindings_ready(&self.area.items) {
            return Ok(false);
        }
        ids.bind_generated(&self.area.items);
        if let Some(inventory) = &pending.inventory {
            // Generated locations depend on bindings which do not exist in from_save.
            for (slot, instance) in inventory.slots.iter().enumerate() {
                let Some(instance @ InstanceId::Generated { .. }) = instance else {
                    continue;
                };
                let Ok(id) = ids.item(instance) else {
                    return Ok(false);
                };
                let state = self.area.items.get(id).ok_or_else(|| {
                    SaveError::ReconstructionFailure("Missing inventory item".into())
                })?;
                let body = state.body.ok_or_else(|| {
                    SaveError::ReconstructionFailure("Missing inventory body".into())
                })?;
                let rule = self.area.interaction.pickup_rule(id);
                if state.removed || !rule.is_some_and(|p| p.can_inventory && !p.cant_pickup) {
                    return Err(SaveError::InvalidLinks(
                        "Item cannot enter inventory".into(),
                    ));
                }
                self.area.inventory.slots[slot] = Some(id);
                if slot as u8 != inventory.selected {
                    self.area.items.get_mut(id).unwrap().location =
                        crate::interaction::items::ItemLocation::Stored(slot as u8);
                    self.physics
                        .edit_body(body, |state| state.set_enabled(false));
                }
            }
            if let Some(instance @ InstanceId::Generated { .. }) = &inventory.equipped {
                let Ok(id) = ids.item(instance) else {
                    return Ok(false);
                };
                let state = self.area.items.get(id).unwrap();
                if state.removed
                    || state.body.is_none()
                    || !self
                        .area
                        .interaction
                        .pickup_rule(id)
                        .is_some_and(|p| p.can_hold_right_hand && !p.cant_pickup)
                {
                    return Err(SaveError::InvalidLinks("Equipment ownership".into()));
                }
                self.area.interaction.equipment.equip(
                    id,
                    crate::interaction::equipment::EquipmentPlacement::Immediate,
                    &mut self.area.items,
                    &mut self.physics,
                );
            }
            let mut worn = [None; 5];
            for (id, saved) in worn.iter_mut().zip(&inventory.worn) {
                if let Some(WornSnapshot::Item {
                    item: instance @ InstanceId::Generated { .. },
                }) = saved
                {
                    let Ok(item) = ids.item(instance) else {
                        return Ok(false);
                    };
                    *id = Some(item);
                }
            }
            for (slot, id) in worn.into_iter().enumerate() {
                if let Some(id) = id {
                    self.area.restore_worn(&mut self.physics, slot, id)?;
                }
            }
            self.area.update_glasses(&self.stream);
        }
        if let Some(carry) = &pending.carry {
            let Ok(id) = ids.item(&carry.item) else {
                return Ok(false);
            };
            if self.area.items.get(id).and_then(|i| i.body).is_none() {
                return Ok(false);
            }
            self.area.interaction.restore_carry_save(
                carry,
                &ids,
                &self.area.items,
                &mut self.physics,
            )?;
        }
        finalize::RestoreFinalizer {
            area: &mut self.area,
            physics: &mut self.physics,
            wheels: &mut self.wheels,
            persistent: &mut self.persistent_world,
            stream: &mut self.stream,
        }
        .deferred(reference, ids, &mut pending.motion, &pending.items)?;
        if let Some(seat) = pending.seat.take() {
            let owner = match seat.owner {
                SeatOwnerSnapshot::Item { item } => crate::player::SeatOwner::Item(item),
                SeatOwnerSnapshot::Vehicle { vehicle } => {
                    crate::player::SeatOwner::Vehicle(vehicle)
                }
            };
            // The stream has bound all saved items, so a missing seat never binds.
            let bound = self
                .area
                .interaction
                .find_item_seat(&owner, &seat.source.source_id()?)
                .ok_or_else(|| SaveError::InvalidLinks("Seat item is not bound".into()))?;
            self.area.player.restore_item_seat(
                &bound,
                local_pose(&seat.local_pose),
                &mut self.physics,
            );
        }
        pending.carry = None;
        pending.inventory = None;
        pending.ready = true;
        if pending.items.is_empty() {
            self.pending_restore = None;
        }
        Ok(true)
    }
    pub fn from_save(
        reference: &ReferenceManifest,
        expected: &SourceStamp,
        saved: &SaveSnapshot,
        mode: crate::world::GameMode,
        draw_distance: f32,
    ) -> Result<Self, SaveError> {
        saved.validate(reference, expected)?;
        let ids = IdentityMap::new(reference)?;
        // The saved start car keeps its start placement.
        let mode = crate::world::GameMode {
            start_car: crate::world::StartCarChoice::Field(saved.world.start_car),
            ..mode
        };
        let mut candidate = Self::new_candidate(
            reference,
            false,
            saved.world.seed,
            saved.world.date,
            mode,
            draw_distance,
        )?;
        candidate
            .area
            .interaction
            .conditions
            .restore_save(reference, &saved.conditions)?;
        candidate
            .area
            .interaction
            .cleaning
            .restore_save(reference, &saved.conditions.tools)?;
        candidate
            .area
            .items
            .restore_save(&ids, &saved.items, reference)?;
        candidate
            .wheels
            .refresh_conditions(&candidate.area.interaction.conditions);
        candidate
            .wheels
            .restore_save(&mut candidate.area.items, &mut candidate.physics)?;
        candidate.area.interaction.parts.restore_save(
            &ids,
            &saved.attachments.parts,
            &mut candidate.area.items,
            &mut candidate.area.interaction.mechanisms,
            &mut candidate.physics,
        )?;
        candidate
            .area
            .interaction
            .restore_save(&saved.mechanisms, &ids);
        candidate
            .area
            .interaction
            .sync_parts(&mut candidate.area.items, &mut candidate.physics)?;
        // Assembly setup may change local poses. The snapshot owns their saved frames.
        candidate
            .area
            .items
            .restore_save(&ids, &saved.items, reference)?;
        candidate.area.inventory.selected = saved.inventory.selected;
        for (slot, instance) in candidate
            .area
            .inventory
            .slots
            .iter_mut()
            .zip(&saved.inventory.slots)
        {
            *slot = instance
                .as_ref()
                .filter(|i| matches!(i, InstanceId::Initial { .. }))
                .map(|id| ids.item(id))
                .transpose()?;
        }
        candidate.stream.restore_save(&saved.world)?;
        let mut protected = Vec::new();
        for instance in saved
            .inventory
            .slots
            .iter()
            .flatten()
            .chain(saved.inventory.equipped.iter())
            .chain(
                saved
                    .inventory
                    .worn
                    .iter()
                    .flatten()
                    .filter_map(|w| match w {
                        WornSnapshot::Item { item } => Some(item),
                        WornSnapshot::Spawned { .. } => None,
                    }),
            )
            .chain(saved.player.carry.iter().map(|c| &c.item))
            .chain(
                saved
                    .items
                    .iter()
                    .filter(|i| !matches!(i.location, ItemLocationSave::World))
                    .map(|i| &i.instance),
            )
        {
            if let InstanceId::Generated { id } = instance {
                protected.push(crate::world::ObjectKey::Generated(id.clone()));
            }
        }
        if let Some(seat) = saved.player.seat.as_ref() {
            protected.push(seat_owner_key(seat).clone());
        }
        for item in &saved.items {
            match &item.location {
                ItemLocationSave::GeneratedAttachment { attachment } => {
                    protected.extend(attachment.owner.iter().cloned());
                }
                ItemLocationSave::Attached {
                    owner: InstanceId::Generated { id },
                } => {
                    protected.push(crate::world::ObjectKey::Generated(id.clone()));
                }
                _ => (),
            }
        }
        candidate.stream.protect_saved_items(&protected);
        candidate.stream.hold_saved_item_locations();
        candidate.stream.origin.offset = saved.player.pose.position;
        candidate.area.sync_origin(candidate.stream.origin.offset);
        candidate.weather.set_origin(candidate.stream.origin.offset);
        candidate.weather.restore(
            &reference.weather,
            candidate.simulation_seconds,
            &saved.weather,
        );
        candidate.creatures.restore(
            &reference.creatures.rabbit,
            &mut candidate.physics,
            &candidate.stream.origin,
            &saved.rabbits,
            candidate.simulation_seconds,
        )?;
        candidate.restore_dead_rabbits(reference, &saved.rabbits)?;
        let spawned = super::spawned::restore_save(
            reference,
            &mut candidate.area,
            &mut candidate.physics,
            &candidate.stream.origin,
            &saved.spawned,
        )?;
        for (item, record) in spawned.into_iter().zip(&saved.spawned) {
            if let Some(gun) = record.gun {
                candidate.area.interaction.guns.restore(item, gun);
            }
        }
        super::survival::restore_foods(
            reference,
            &mut candidate.area,
            &mut candidate.physics,
            &candidate.stream.origin,
            &saved.foods,
        )?;
        super::hose::restore(reference, &mut candidate.area, &mut candidate.physics)?;
        candidate.restore_cigarettes(&saved.cigarettes);
        candidate
            .stream
            .request_at(saved.player.pose.position, selected_vehicle_position(saved));
        candidate.persistent_world = PersistentWorld::restore(saved, &ids)?;
        finalize::RestoreFinalizer {
            area: &mut candidate.area,
            physics: &mut candidate.physics,
            wheels: &mut candidate.wheels,
            persistent: &mut candidate.persistent_world,
            stream: &mut candidate.stream,
        }
        .initial(reference, &ids, saved)?;
        candidate.area.player.restore_save(
            &saved.player,
            &candidate.stream.origin,
            &mut candidate.physics,
        )?;
        candidate
            .sleep
            .restore(saved.player.since_wake, candidate.simulation_seconds);
        candidate.survival.restore_save(&saved.needs);
        candidate.hearing.restore_save(&saved.hearing);
        if let Some(carry) = saved
            .player
            .carry
            .as_ref()
            .filter(|c| matches!(c.item, InstanceId::Initial { .. }))
        {
            candidate.area.interaction.restore_carry_save(
                carry,
                &ids,
                &candidate.area.items,
                &mut candidate.physics,
            )?;
        }
        if let Some(instance) = saved
            .inventory
            .equipped
            .as_ref()
            .filter(|i| matches!(i, InstanceId::Initial { .. }))
        {
            let id = ids.item(instance)?;
            candidate.area.interaction.equipment.equip(
                id,
                crate::interaction::equipment::EquipmentPlacement::Immediate,
                &mut candidate.area.items,
                &mut candidate.physics,
            );
        }
        for (slot, worn) in saved.inventory.worn.iter().enumerate() {
            let id = match worn {
                Some(WornSnapshot::Item {
                    item: instance @ InstanceId::Initial { .. },
                }) => ids.item(instance)?,
                Some(WornSnapshot::Spawned { serial }) => {
                    candidate
                        .area
                        .spawned
                        .iter()
                        .find(|s| s.serial == *serial)
                        .ok_or_else(|| SaveError::InvalidLinks("Unknown worn serial".into()))?
                        .id
                }
                _ => continue,
            };
            candidate
                .area
                .restore_worn(&mut candidate.physics, slot, id)?;
        }
        candidate.area.update_glasses(&candidate.stream);
        let starter = reference.survival.starter_food;
        for tank in &reference.tanks {
            if candidate
                .area
                .items
                .get(tank.item)
                .is_some_and(|i| i.removed)
            {
                candidate.remove_imported_item(tank.item);
            }
        }
        if candidate.area.items.get(starter).is_some_and(|i| i.removed) {
            candidate.area.items.get_mut(starter).unwrap().removed = false;
            candidate.remove_consumable(starter)?;
        } else {
            super::survival::sync_consumable_colliders(
                reference,
                &candidate.area,
                &mut candidate.physics,
                starter,
            );
        }
        candidate
            .area
            .sync_equipment(reference, &mut candidate.physics);
        candidate.area.items.sync_poses(&candidate.physics);
        candidate.vehicle = saved.progress.selected_vehicle.clone();
        candidate.driving_assists = saved.driving_assists;
        candidate.attempts = crate::save::identity::unsigned(&saved.progress.start_attempts)?;
        candidate.driven_km = saved.progress.driven_km;
        candidate
            .lighting
            .restore_save(&reference.lighting, &ids, &saved.lighting)?;
        candidate.area.commit_poses(&candidate.physics, true);
        candidate.refresh_powertrains();
        // Car sounds sync each engine's running edge on their first step.
        if let Some(audio) = &mut candidate.audio {
            audio.bind_game(reference, &candidate.area, &candidate.physics);
            audio.sync_lamps(&reference.lighting, &candidate.lighting);
        }
        candidate.startup_aligned = true;
        candidate.pending_restore = Some(PendingRestore {
            items: saved
                .items
                .iter()
                .filter(|i| matches!(i.location, ItemLocationSave::GeneratedAttachment { .. }))
                .cloned()
                .collect(),
            carry: saved
                .player
                .carry
                .clone()
                .filter(|c| matches!(c.item, InstanceId::Generated { .. })),
            inventory: Some(saved.inventory.clone()),
            ids: IdentityMap::new(reference)?,
            ready: false,
            motion: saved.bodies.clone(),
            seat: saved.player.seat.clone(),
        });
        candidate.clear_input();
        candidate.paused = true;
        candidate.started = true;
        Ok(candidate)
    }
    pub fn capture_save(
        &self,
        reference: &ReferenceManifest,
        source: &SourceStamp,
        saved_at: &str,
    ) -> Result<SaveSnapshot, SaveError> {
        if self.restore_pending() {
            return Err(SaveError::ReconstructionFailure(
                "Save restore is pending".into(),
            ));
        }
        if self.needs_output().dead {
            return Err(SaveError::ReconstructionFailure(
                "The player is dead".into(),
            ));
        }
        let mut ids = IdentityMap::new(reference)?;
        ids.bind_generated(&self.area.items);
        let origin = &self.stream.origin;
        let mut required: Vec<_> = self
            .pending_restore
            .iter()
            .flat_map(|p| &p.items)
            .filter_map(|i| {
                if let ItemLocationSave::GeneratedAttachment { attachment } = &i.location {
                    attachment.owner.clone()
                } else {
                    None
                }
            })
            .collect();
        for item in self
            .area
            .items
            .iter()
            .filter(|i| !matches!(i.location, crate::interaction::items::ItemLocation::World))
        {
            let key = self.area.items.object_key(item.id);
            if matches!(key, crate::world::ObjectKey::Generated(_)) {
                required.push(key);
            }
        }
        // A seat item or world car saves its change, so the load waits for its binding.
        if let Some(
            crate::player::SeatOwner::Item(key @ crate::world::ObjectKey::Generated(_))
            | crate::player::SeatOwner::Vehicle(key @ crate::world::ObjectKey::Generated(_)),
        ) = self.area.player.seat_owner()
        {
            required.push(key.clone());
        }
        let mut world = self.stream.capture_save(
            reference,
            &self.area.items,
            &self.area.interaction,
            &self.physics,
            &required,
        );
        self.keep_object_states(&mut world.changes);
        let (surfaces, washes) = self.area.interaction.conditions.capture_save(reference)?;
        let tools = self.area.interaction.cleaning.capture_save(reference)?;
        let mut snapshot = SaveSnapshot {
            conditions: ConditionSnapshot {
                surfaces,
                washes,
                tools,
            },
            driving_assists: self.driving_assists,
            schema_version: SAVE_SCHEMA_VERSION,
            inventory: self.area.capture_inventory(&ids)?,
            saved_at: saved_at.into(),
            source: source.clone(),
            world,
            player: self.area.player.capture_save(
                origin,
                &self.physics,
                // Spawned items save in the world, so the hand saves empty.
                if self
                    .area
                    .interaction
                    .held()
                    .is_some_and(|id| self.area.saves_in_world(id))
                {
                    None
                } else {
                    self.area.interaction.capture_carry_save(&ids)?
                },
            )?,
            needs: self.survival.capture_save(),
            hearing: self.hearing.capture_save(),
            bodies: self.area.capture_save(
                reference,
                &ids,
                origin,
                &self.physics,
                &self.persistent_world,
            )?,
            items: self.area.items.capture_save(
                &ids,
                origin,
                &self.physics,
                &self.area.interaction,
                reference,
            )?,
            attachments: AttachmentsSnapshot {
                parts: self.area.interaction.parts.capture_save(&ids)?,
            },
            mechanisms: self.area.interaction.capture_save(&ids)?,
            progress: ProgressSnapshot {
                selected_vehicle: self.vehicle.clone(),
                start_attempts: self.attempts.to_string(),
                driven_km: self.driven_km,
            },
            lighting: self.lighting.capture_save(&reference.lighting, &ids)?,
            spawned: self.area.spawned.capture_save(
                reference,
                &self.area.items,
                &self.area.interaction.guns,
                origin,
            ),
            foods: self.area.capture_foods(reference, origin),
            cigarettes: self.area.cigarettes.capture_save(&self.area.spawned),
            weather: self
                .weather
                .snapshot(&reference.weather, self.simulation_seconds),
            rabbits: self.creatures.snapshot(&self.physics, origin),
        };
        if let Some(pending) = &self.pending_restore {
            for saved in &pending.items {
                let target = snapshot
                    .items
                    .iter_mut()
                    .find(|i| i.instance == saved.instance)
                    .unwrap();
                *target = saved.clone();
                snapshot.bodies.retain(|b| !matches!(&b.identity, BodyIdentity::Assembly { root } if root == &saved.instance));
            }
        }
        snapshot.player.since_wake = self.sleep.since_wake(self.simulation_seconds);
        snapshot.validate(reference, source)?;
        Ok(snapshot)
    }
}
impl GarageRuntime {
    /// Writes the state of every live car, its wheel slots and every engine, radiator and gauge part
    /// into `changes`.
    fn keep_object_states(&self, changes: &mut crate::world::ChangeRecords) {
        for (key, vehicle) in &self.vehicles {
            changes.keep_vehicle(key.clone(), vehicle.state());
        }
        for (key, rims) in self.wheels.world_slots() {
            changes.keep_wheel_slots(key.clone(), rims);
        }
        for part in self.powertrain.engines() {
            changes.keep_engine(
                self.area.items.object_key(part.item),
                part.state.capture_save(),
            );
        }
        for part in self.powertrain.radiators() {
            changes.keep_radiator(self.area.items.object_key(part.item), part.fan_on);
        }
        for part in self.powertrain.gauges() {
            changes.keep_gauge(
                self.area.items.object_key(part.item),
                part.state.capture_save(),
            );
        }
    }
}
fn seat_owner_key(seat: &SeatSnapshot) -> &crate::world::ObjectKey {
    match &seat.owner {
        SeatOwnerSnapshot::Item { item } => item,
        SeatOwnerSnapshot::Vehicle { vehicle } => vehicle,
    }
}
/// Saved position of the last seated car. The player position if it has none.
fn selected_vehicle_position(saved: &SaveSnapshot) -> [f64; 3] {
    let Some(key) = &saved.progress.selected_vehicle else {
        return saved.player.pose.position;
    };
    saved
        .world
        .changes
        .get(key)
        .and_then(|c| c.snapshot.as_ref())
        .map_or(saved.player.pose.position, |s| s.pose.position)
}
impl StartingArea {
    pub fn capture_save(
        &self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        origin: &WorldOrigin,
        physics: &PhysicsWorld,
        world: &PersistentWorld,
    ) -> Result<Vec<BodySnapshot>, SaveError> {
        let mut records = Vec::new();
        let mut captured = HashSet::new();
        for item in self.items.iter() {
            if !matches!(
                item.location,
                crate::interaction::items::ItemLocation::World
                    | crate::interaction::items::ItemLocation::Held
                    | crate::interaction::items::ItemLocation::Stored(_)
                    | crate::interaction::items::ItemLocation::Equipped
            ) {
                continue;
            }
            let detachable = reference.parts.iter().any(|part| part.item == item.id)
                || reference
                    .wheel_parts
                    .iter()
                    .any(|part| part.item == item.id);
            if !detachable {
                continue;
            }
            let handle = item
                .body
                .ok_or_else(|| SaveError::InvalidLinks("Missing detached assembly body".into()))?;
            if !captured.insert(handle) {
                continue;
            }
            let body = physics
                .rapier()
                .bodies
                .get(handle)
                .ok_or_else(|| SaveError::InvalidLinks("Missing detached body".into()))?;
            let identity = BodyIdentity::Assembly {
                root: ids.item_instance(item.id)?,
            };
            records.push(
                world
                    .body_snapshot(handle, identity.clone())
                    .unwrap_or_else(|| capture_body_save(body, identity, origin)),
            );
        }
        for record in &reference.bodies {
            if record.parent_body.is_some() {
                continue;
            }
            // An eaten food or an exploded tank has no body.
            if reference
                .items
                .iter()
                .any(|i| i.body == record.id && self.items.get(i.id).is_some_and(|s| s.removed))
            {
                continue;
            }
            let handle = *self
                .bodies
                .get(&record.id)
                .ok_or_else(|| SaveError::InvalidLinks("Missing source body".into()))?;
            if !captured.insert(handle) {
                continue;
            }
            let body = physics
                .rapier()
                .bodies
                .get(handle)
                .ok_or_else(|| SaveError::InvalidLinks("Missing source body".into()))?;
            let identity = BodyIdentity::Source {
                source: SourceKey::from(&record.source),
            };
            let saved = world
                .body_snapshot(handle, identity.clone())
                .unwrap_or_else(|| capture_body_save(body, identity, origin));
            let at_source = saved.pose.position == record.pose.translation.map(f64::from)
                || (physics.body_active(handle)
                    && body.translation().to_array()
                        == origin.local(record.pose.translation.map(f64::from)));
            if !at_source
                || saved.pose.rotation != record.pose.rotation
                || saved.linear_velocity != [0.; 3]
                || saved.angular_velocity != [0.; 3]
                || (record.dynamic && saved.sleeping)
            {
                records.push(saved);
            }
        }
        records.sort_by(|a, b| match (&a.identity, &b.identity) {
            (BodyIdentity::Source { source: a }, BodyIdentity::Source { source: b }) => a.cmp(b),
            (BodyIdentity::Assembly { root: a }, BodyIdentity::Assembly { root: b }) => a.cmp(b),
            (BodyIdentity::Source { .. }, _) => std::cmp::Ordering::Less,
            _ => std::cmp::Ordering::Greater,
        });
        Ok(records)
    }
}
