use super::GarageRuntime;
use crate::fluids::{contact_drink_transfer, drink_transfer};
use crate::interaction::items::ItemLocation;
use crate::reference::{ConsumptionEffect, ItemId, NodeId, ReferenceError, ReferenceManifest};
use crate::vehicle::{PhysicsWorld, VehiclePose, collider_builder, from_rapier, to_rapier};
use crate::world::stream::content_bindings::{hit_events, hit_sound};
use rapier3d::prelude::{Group, InteractionGroups, InteractionTestMode, RigidBodyBuilder};

/// A runtime food copy at `bite`. It saves in the world.
pub fn spawn_consumable(
    reference: &ReferenceManifest,
    area: &mut super::StartingArea,
    physics: &mut PhysicsWorld,
    id: ItemId,
    prototype_index: usize,
    bite: u8,
    pose: VehiclePose,
) -> Result<(), ReferenceError> {
    let prototype = reference
        .survival
        .foods
        .get(prototype_index)
        .ok_or_else(|| ReferenceError("Missing food prototype".into()))?;
    if area.items.get(id).is_some() {
        return Err(ReferenceError("Duplicate food item".into()));
    }
    let root = to_rapier(super::node_pose(reference, prototype.root)?);
    let hit = hit_sound(&reference.world, &prototype.pickup.source);
    let body = physics.insert_body(
        RigidBodyBuilder::dynamic()
            .pose(to_rapier(pose))
            .ccd_enabled(true)
            .additional_mass(prototype.mass_kg)
            .linear_damping(prototype.linear_damping)
            .angular_damping(prototype.angular_damping),
    );
    let mut handles = Vec::with_capacity(prototype.colliders.len());
    for record in &prototype.colliders {
        let node = to_rapier(super::node_pose(reference, record.node)?);
        let mut collider = collider_builder(&record.geometry)?
            .sensor(record.sensor)
            .enabled(record.enabled && active_part(reference, record.node, &prototype.parts, bite))
            .density(0.)
            .active_events(hit_events(hit))
            .collision_groups(InteractionGroups::new(
                Group::from_bits_truncate(record.membership),
                Group::from_bits_truncate(record.filter),
                InteractionTestMode::And,
            ))
            .user_data(
                crate::vehicle::step_surface_bits(record.step_surface)
                    | crate::vehicle::tag_bits(record.tag)
                    | (id.0 as u128 + 1),
            )
            .build();
        collider.set_position(root.inverse() * node * *collider.position());

        handles.push(physics.insert_collider_with_parent(collider, body));
    }
    area.items
        .insert_consumable(id, prototype_index, prototype, bite, pose, body, hit)?;
    area.interaction
        .register_consumable(id, prototype, &handles);
    area.item_render_poses.push((id, pose, pose));
    area.foods.push(id);
    physics.refresh_queries();
    Ok(())
}

impl super::StartingArea {
    /// Spawned items and runtime foods save in the world, not in a hand or slot.
    pub fn saves_in_world(&self, id: ItemId) -> bool {
        self.spawned.contains(id) || self.foods.contains(&id)
    }
    /// Every runtime food that is not removed, at its current pose.
    pub fn capture_foods(
        &self,
        reference: &ReferenceManifest,
        origin: &crate::world::WorldOrigin,
    ) -> Vec<crate::save::FoodItemSnapshot> {
        self.foods
            .iter()
            .filter_map(|id| {
                let state = self.items.get(*id).filter(|s| !s.removed)?;
                let food = state.food?;
                Some(crate::save::FoodItemSnapshot {
                    source: (&reference.survival.foods[food.prototype_index].source).into(),
                    bite: food.bite,
                    pose: state.inactive_pose.unwrap_or(crate::save::AbsolutePose {
                        position: origin.absolute(state.pose.position),
                        rotation: state.pose.rotation,
                    }),
                })
            })
            .collect()
    }
}

/// Food prototype index of a saved source.
pub fn saved_food(
    reference: &ReferenceManifest,
    source: &crate::save::SourceKey,
) -> Result<usize, crate::save::SaveError> {
    reference
        .survival
        .foods
        .iter()
        .position(|p| crate::save::SourceKey::from(&p.source) == *source)
        .ok_or_else(|| crate::save::SaveError::InvalidLinks("Unknown food source".into()))
}

pub fn restore_foods(
    reference: &ReferenceManifest,
    area: &mut super::StartingArea,
    physics: &mut PhysicsWorld,
    origin: &crate::world::WorldOrigin,
    saved: &[crate::save::FoodItemSnapshot],
) -> Result<(), crate::save::SaveError> {
    for record in saved {
        let prototype = saved_food(reference, &record.source)?;
        let id = area.items.allocate_item_id()?;
        let pose = VehiclePose {
            position: origin.local(record.pose.position),
            rotation: record.pose.rotation,
        };
        spawn_consumable(reference, area, physics, id, prototype, record.bite, pose)?;
    }
    Ok(())
}

pub fn active_part(
    reference: &ReferenceManifest,
    mut node: NodeId,
    parts: &[Vec<NodeId>],
    bite: u8,
) -> bool {
    loop {
        if let Some(index) = parts.iter().position(|group| group.contains(&node)) {
            return index == bite as usize;
        }
        let Some(parent) = reference
            .nodes
            .iter()
            .find(|entry| entry.id == node)
            .and_then(|entry| entry.parent)
        else {
            return true;
        };
        node = parent;
    }
}

pub fn sync_consumable_colliders(
    reference: &ReferenceManifest,
    area: &super::StartingArea,
    physics: &mut PhysicsWorld,
    id: ItemId,
) {
    let Some(state) = area.items.get(id) else {
        return;
    };
    let Some(food) = state.food else { return };
    let Some(body) = state.body else { return };
    let starter = id == reference.survival.starter_food;
    let parts = if starter {
        &reference.survival.starter_parts
    } else {
        &reference.survival.foods[food.prototype_index].parts
    };
    let handles = physics.rapier().bodies[body].colliders().to_vec();
    for (index, handle) in handles.into_iter().enumerate() {
        let record = if starter {
            let collider_id = (physics.rapier().colliders[handle].user_data >> 64) as u32;
            reference
                .colliders
                .iter()
                .find(|entry| entry.id.0 == collider_id)
                .map(|entry| (entry.node, entry.enabled))
        } else {
            reference.survival.foods[food.prototype_index]
                .colliders
                .get(index)
                .map(|entry| (entry.node, entry.enabled))
        };
        if let Some((node, enabled)) = record {
            physics.edit_collider(handle, |collider| {
                collider.set_enabled(enabled && active_part(reference, node, parts, food.bite))
            });
        }
    }
    physics.refresh_queries();
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrinkResult {
    pub requested_liters: f32,
    pub debited_liters: f32,
    pub effect: ConsumptionEffect,
}

impl GarageRuntime {
    pub fn apply_toilet_output(
        &mut self,
        reference: &ReferenceManifest,
        output: crate::survival::ToiletOutput,
    ) -> Result<Option<ItemId>, ReferenceError> {
        if output.defecated {
            self.spawn_turd(reference).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn spawn_turd(&mut self, reference: &ReferenceManifest) -> Result<ItemId, ReferenceError> {
        let prototype_index = reference
            .survival
            .foods
            .iter()
            .position(|food| food.kind == crate::reference::FoodKind::Turd)
            .ok_or_else(|| ReferenceError("Missing turd prototype".into()))?;
        let id = self.area.items.allocate_item_id()?;
        let mut pose = self.area.player.body_pose(&self.physics);
        pose.rotation = [0., 0., 0., 1.];
        spawn_consumable(
            reference,
            &mut self.area,
            &mut self.physics,
            id,
            prototype_index,
            reference.survival.foods[prototype_index].index,
            pose,
        )?;
        Ok(id)
    }
    /// `ediblescript.Shot`: the item advances `n` bites without nutrition or sound. A new
    /// piece with `n` bites spawns turned 180 degrees around Y. Without a piece and with
    /// `only_separate` false, the item loses one bite (`RemoveOne`).
    pub fn cut_food(
        &mut self,
        reference: &ReferenceManifest,
        item: ItemId,
        only_separate: bool,
    ) -> Result<(), ReferenceError> {
        let Some(state) = self.area.items.get(item) else {
            return Ok(());
        };
        let (Some(food), Some(body)) = (state.food, state.body) else {
            return Ok(());
        };
        let prototype = &reference.survival.foods[food.prototype_index];
        if state.removed {
            return Ok(());
        }
        let n = prototype
            .spawn_if_shot
            .then(|| crate::interaction::melee::cut(prototype.bites.saturating_sub(food.bite)))
            .flatten();
        let Some(n) = n else {
            return if only_separate {
                Ok(())
            } else {
                self.remove_one(reference, item)
            };
        };
        let mut root = *self.physics.rapier().bodies[body].position() * to_rapier(state.local_pose);
        root.rotation *= rapier3d::math::Rotation::from_rotation_y(std::f32::consts::PI);
        self.area
            .items
            .get_mut(item)
            .unwrap()
            .food
            .as_mut()
            .unwrap()
            .bite += n;
        self.sync_food_colliders(reference, item);
        let id = self.area.items.allocate_item_id()?;
        spawn_consumable(
            reference,
            &mut self.area,
            &mut self.physics,
            id,
            food.prototype_index,
            prototype.bites - n,
            from_rapier(&root),
        )
    }
    /// `ediblescript.RemoveOne`: one bite without nutrition or sound.
    fn remove_one(
        &mut self,
        reference: &ReferenceManifest,
        item: ItemId,
    ) -> Result<(), ReferenceError> {
        let food = self
            .area
            .items
            .get_mut(item)
            .unwrap()
            .food
            .as_mut()
            .unwrap();
        let prototype = &reference.survival.foods[food.prototype_index];
        if food.bite >= prototype.bites {
            return Ok(());
        }
        food.bite += 1;
        let end = food.bite == prototype.bites && prototype.destroy_on_end;
        self.sync_food_colliders(reference, item);
        if end {
            self.remove_consumable(item)?;
        }
        Ok(())
    }
    /// Enabled food colliders follow the bite.
    fn sync_food_colliders(&mut self, reference: &ReferenceManifest, item: ItemId) {
        if matches!(
            self.area.items.object_key(item),
            crate::world::ObjectKey::Generated(_)
        ) {
            self.stream
                .sync_generated_food(reference, &self.area.items, &mut self.physics);
        } else {
            sync_consumable_colliders(reference, &self.area, &mut self.physics, item);
        }
    }
    pub fn consume(
        &mut self,
        reference: &ReferenceManifest,
        item: ItemId,
    ) -> Result<bool, ReferenceError> {
        if self.needs_output().dead || self.sleeping() || !self.survival.output().can_eat {
            return Ok(false);
        }
        let Some(state) = self.area.items.get(item) else {
            return Err(ReferenceError("Missing food item".into()));
        };
        let Some(food) = state.food else {
            return Ok(false);
        };
        if state.removed
            || state.body.is_none()
            || food.bite >= reference.survival.foods[food.prototype_index].bites
        {
            return Ok(false);
        }
        let prototype = &reference.survival.foods[food.prototype_index];
        let divisor = prototype.bites as f32;
        self.survival.queue(ConsumptionEffect {
            food: prototype.effect.food / divisor,
            water: prototype.effect.water / divisor,
            bowel: prototype.effect.bowel / divisor,
            hp: prototype.effect.hp / divisor,
        });
        self.survival.show_stats();
        // ediblescript.soundType: 0 eat, 1 crunchy.
        self.emit_survival_audio(if prototype.sound == 1 {
            crate::audio::AudioKind::EatCrunchy
        } else {
            crate::audio::AudioKind::Eat
        });
        // mainscript.FoodParticle at particlePos: the food root for every selected food.
        if prototype.has_particle {
            let pose = self.area.items.get(item).unwrap().pose;
            let rotation = rapier3d::math::Rotation::from_array(pose.rotation);
            self.food_particles.bite(
                prototype.particle_type.into(),
                [prototype.min_particle as i32, prototype.max_particle as i32],
                self.stream.origin.absolute(pose.position),
                rotation * rapier3d::math::Vector::NEG_Z,
            );
        }
        let bite = {
            let state = self.area.items.get_mut(item).unwrap();
            state.food.as_mut().unwrap().bite += 1;
            state.food.unwrap().bite
        };
        self.sync_food_colliders(reference, item);
        if bite == prototype.bites && prototype.destroy_on_end {
            self.remove_consumable(item)?;
        }
        Ok(true)
    }

    pub fn drink(
        &mut self,
        reference: &ReferenceManifest,
        source: ItemId,
        requested_liters: f32,
    ) -> Result<DrinkResult, ReferenceError> {
        if self.needs_output().dead || self.sleeping() {
            return Err(ReferenceError("Player cannot drink".into()));
        }
        if !requested_liters.is_finite() || requested_liters < 0. {
            return Err(ReferenceError("Invalid drink volume".into()));
        }
        if !reference
            .tanks
            .iter()
            .any(|tank| tank.item == source && !tank.no_drink)
        {
            return Err(ReferenceError("Source cannot be drunk".into()));
        }
        self.apply_drink_request(reference, source, requested_liters)
    }

    pub fn drink_contact(
        &mut self,
        reference: &ReferenceManifest,
        source: ItemId,
        poured_liters: f32,
        fractions: [f32; 6],
    ) -> Result<DrinkResult, ReferenceError> {
        if self.needs_output().dead {
            return Err(ReferenceError("Player is dead".into()));
        }
        if !poured_liters.is_finite()
            || poured_liters < 0.
            || !fractions
                .iter()
                .all(|value| value.is_finite() && *value >= 0.)
        {
            return Err(ReferenceError("Invalid particle drink".into()));
        }
        if self
            .area
            .items
            .get(source)
            .and_then(|state| state.fluid.as_ref())
            .is_none()
        {
            return Err(ReferenceError("Missing particle source".into()));
        }
        if self.survival.bladder.total() >= self.survival.bladder.capacity_liters {
            return Ok(DrinkResult {
                requested_liters: poured_liters,
                debited_liters: 0.,
                effect: ConsumptionEffect::default(),
            });
        }
        let needs = self.survival.output().needs;
        let filter_water = needs.water < reference.survival.maximum.water;
        let filter_blood = needs.hp < reference.survival.maximum.hp;
        let before = self.survival.pending();
        self.survival
            .apply_drink(&reference.survival, fractions, poured_liters);
        let after = self.survival.pending();
        contact_drink_transfer(
            &mut self.survival.bladder,
            self.area.items.get(source).unwrap().fluid.as_ref().unwrap(),
            fractions,
            poured_liters,
            filter_water,
            filter_blood,
        );
        Ok(DrinkResult {
            requested_liters: poured_liters,
            debited_liters: 0.,
            effect: ConsumptionEffect {
                food: after.food - before.food,
                water: after.water - before.water,
                bowel: after.bowel - before.bowel,
                hp: after.hp - before.hp,
            },
        })
    }

    fn apply_drink_request(
        &mut self,
        reference: &ReferenceManifest,
        source: ItemId,
        requested_liters: f32,
    ) -> Result<DrinkResult, ReferenceError> {
        if self.needs_output().dead || self.sleeping() {
            return Err(ReferenceError("Player cannot drink".into()));
        }
        if !requested_liters.is_finite() || requested_liters < 0. {
            return Err(ReferenceError("Invalid drink volume".into()));
        }
        let Some(state) = self.area.items.get(source) else {
            return Err(ReferenceError("Missing drink source".into()));
        };
        let Some(container) = state.fluid.as_ref() else {
            return Err(ReferenceError("Drink source has no fluid".into()));
        };
        if state.removed
            || container.total() <= 0.
            || self.survival.bladder.total() >= self.survival.bladder.capacity_liters
        {
            return Ok(DrinkResult {
                requested_liters,
                debited_liters: 0.,
                effect: ConsumptionEffect::default(),
            });
        }
        let total = container.total();
        let fractions = container
            .mixture()
            .quantities_liters
            .map(|amount| amount / total);
        let needs = self.survival.output().needs;
        let filter_water = needs.water < reference.survival.maximum.water;
        let filter_blood = needs.hp < reference.survival.maximum.hp;
        let before = self.survival.pending();
        self.survival
            .apply_drink(&reference.survival, fractions, requested_liters);
        let after = self.survival.pending();
        let debited_liters = drink_transfer(
            self.area
                .items
                .get_mut(source)
                .unwrap()
                .fluid
                .as_mut()
                .unwrap(),
            &mut self.survival.bladder,
            requested_liters,
            filter_water,
            filter_blood,
        );
        Ok(DrinkResult {
            requested_liters,
            debited_liters,
            effect: ConsumptionEffect {
                food: after.food - before.food,
                water: after.water - before.water,
                bowel: after.bowel - before.bowel,
                hp: after.hp - before.hp,
            },
        })
    }

    pub fn remove_consumable(&mut self, item: ItemId) -> Result<(), ReferenceError> {
        let state = self
            .area
            .items
            .get(item)
            .ok_or_else(|| ReferenceError("Missing food item".into()))?;
        if state.food.is_none() || state.removed {
            return Err(ReferenceError("Food item is not active".into()));
        }
        let body = state
            .body
            .ok_or_else(|| ReferenceError("Food body is missing".into()))?;
        if self.area.interaction.held() == Some(item) {
            self.area
                .interaction
                .drop_held(&mut self.area.items, &mut self.physics);
        }
        if matches!(
            self.area.items.object_key(item),
            crate::world::ObjectKey::Generated(_)
        ) {
            self.area.interaction.forget_removed(item, body);
            self.physics.edit_body(body, |body| body.set_enabled(false));
            let state = self.area.items.get_mut(item).unwrap();
            state.location = ItemLocation::World;
            state.removed = true;
            self.physics.refresh_queries();
            return Ok(());
        }
        self.remove_imported_item(item);
        self.area.foods.retain(|id| *id != item);
        Ok(())
    }
    /// Destroy of an imported item and the items on its body: its body leaves the world, and
    /// the item stays as a removed record for the save. The caller first releases it from the
    /// player.
    pub(super) fn remove_imported_item(&mut self, item: ItemId) {
        let Some(body) = self.area.items.get(item).and_then(|s| s.body) else {
            return;
        };
        let on_body: Vec<ItemId> = self
            .area
            .items
            .iter()
            .filter(|s| s.body == Some(body))
            .map(|s| s.id)
            .collect();
        let body_id = self.area.items.body_id(item).unwrap();
        self.physics.remove_body(body);
        self.physics.refresh_queries();
        self.area
            .attachments
            .retain(|(handle, parent, _)| *handle != body && *parent != body);
        self.area
            .portable_mass
            .retain(|(handle, _, _, _)| *handle != body);
        if body_id.0 != u32::MAX {
            self.area.bodies.remove(&body_id);
            self.area.render_poses.retain(|(id, _, _)| *id != body_id);
        }
        for id in on_body {
            self.area.interaction.forget_removed(id, body);
            self.area.item_render_poses.retain(|(i, _, _)| *i != id);
            let state = self.area.items.set_body(id, None).unwrap();
            state.location = ItemLocation::World;
            state.removed = true;
            self.area.inventory.forget(id);
        }
    }
}

impl GarageRuntime {
    pub fn needs_output(&self) -> crate::survival::NeedsOutput {
        self.survival.output()
    }
    pub fn advance_needs(&mut self, reference: &ReferenceManifest, dt: f32, need_scale: f32) {
        if !self.started || self.paused {
            return;
        }
        self.simulation_seconds += f64::from(dt);
        self.hearing.advance(dt);
        self.survival.advance(
            &reference.survival,
            dt,
            self.lighting.cycle.output().ambient_temperature,
            need_scale,
            self.options.survival,
        );
        self.announce_death(reference);
        if self.needs_output().menu_due {
            self.paused = true;
        }
    }
    pub(super) fn draw_survival(&mut self) -> f32 {
        self.survival_draw = self.survival_draw.wrapping_add(1);
        super::start_sample(super::DEFAULT_SEED, self.survival_draw)
    }
    fn select_survival_audio(&mut self, kind: crate::audio::AudioKind) -> (u32, f32, f32, f32) {
        let b = self.audio_gameplay.binding(kind as u32).clone();
        let clip = b.clips
            [((self.draw_survival() * b.clips.len() as f32) as usize).min(b.clips.len() - 1)];
        let gain_draw = self.draw_survival();
        // Keep source draw order and gate timing when playback is disabled.
        let gain = self.audio.as_ref().map_or(0., |p| {
            let b = p
                .reference
                .survival
                .iter()
                .find(|b| b.kind == kind as u32)
                .unwrap();
            b.gain[0] + (b.gain[1] - b.gain[0]) * gain_draw
        });
        let pitch = b.pitch[0] + (b.pitch[1] - b.pitch[0]) * self.draw_survival();
        (
            clip,
            gain,
            pitch,
            self.audio_gameplay
                .clips
                .iter()
                .find(|c| c.id == clip)
                .unwrap()
                .duration_seconds as f32
                / pitch,
        )
    }
    fn emit_selected_audio(
        &mut self,
        kind: crate::audio::AudioKind,
        selection: (u32, f32, f32, f32),
    ) {
        let Some(playback) = &mut self.audio else {
            return;
        };
        let b = playback
            .reference
            .survival
            .iter()
            .find(|b| b.kind == kind as u32)
            .unwrap();
        let pose = if matches!(
            kind,
            crate::audio::AudioKind::Eat | crate::audio::AudioKind::EatCrunchy
        ) {
            self.area.player.view(&self.physics).head_pose
        } else {
            self.area.player.body_pose(&self.physics)
        };
        let local = to_rapier(pose) * rapier3d::math::Vector::from_array(b.offset);
        let position = self.stream.origin.absolute(local.to_array());
        playback.survival_event(crate::audio::SurvivalAudioEvent {
            session: playback.session,
            emitter: 0,
            kind,
            action: crate::audio::AudioAction::Start,
            clip: selection.0,
            gain: selection.1,
            pitch: selection.2,
            position,
        });
    }
    fn emit_survival_audio(&mut self, kind: crate::audio::AudioKind) {
        let selection = self.select_survival_audio(kind);
        self.emit_selected_audio(kind, selection);
    }
    pub(super) fn announce_death(&mut self, reference: &ReferenceManifest) {
        if self.needs_output().dead && !self.death_announced {
            self.death_announced = true;
            self.interrupt_sleep(reference);
            self.clear_input();
            self.area
                .interaction
                .drop_held(&mut self.area.items, &mut self.physics);
            self.take_all_off();
            self.spawn_ragdoll();
            self.emit_survival_audio(crate::audio::AudioKind::Death);
        }
    }
    /// Source `playerdeathtriggerscript`. Cardboard boxes have no break runtime yet.
    pub(super) fn apply_hazards(&mut self, reference: &ReferenceManifest) {
        let damage = self.area.player.hazard_damage();
        if damage > 0. {
            self.survival
                .damage_instant(&reference.survival, damage * super::FIXED_DT);
            self.announce_death(reference);
        }
    }
    pub(super) fn sample_survival_speed(&mut self, reference: &ReferenceManifest) {
        // Kinematic controller velocity comes from Rapier's completed fixed step.
        // Seat entry and exit are placement operations, not source acceleration.
        if self.area.player.seated() || self.sleeping() {
            self.previous_speed = None;
            return;
        }
        let speed = self.physics.rapier().bodies[self.area.player.body_handle()]
            .linvel()
            .length();
        if !self.death_protected
            && self
                .previous_speed
                .is_some_and(|last| (speed - last).abs() > reference.survival.death_velocity_change)
        {
            self.survival
                .damage_instant(&reference.survival, reference.survival.maximum.hp);
        }
        self.previous_speed = Some(speed);
        if self.area.player.grounded() {
            self.death_protected = false;
        }
        self.announce_death(reference);
    }
    pub(super) fn process_toilet(
        &mut self,
        reference: &ReferenceManifest,
        input: crate::simulation::FrameInput,
        dt: f32,
        fluid_scale: f32,
    ) {
        if self.needs_output().dead || self.sleeping() {
            return;
        }
        use crate::audio::AudioKind;
        use crate::simulation::keys;
        let pressed = input.pressed & keys::BURP != 0;
        let draws = if pressed {
            [self.draw_survival(), self.draw_survival()]
        } else {
            [0., 0.]
        };
        let kind = if draws[0] < reference.survival.toilet.fart_burp_ratio {
            if self.needs_output().needs.bowel
                >= reference.survival.maximum.bowel * reference.survival.toilet.bowel_fraction
                && draws[1] > reference.survival.toilet.fart_shit_ratio
            {
                AudioKind::Shit
            } else {
                AudioKind::Fart
            }
        } else {
            AudioKind::Burp
        };
        let selection = if pressed {
            self.select_survival_audio(kind)
        } else {
            (0, 0., 1., 0.)
        };
        let total = self.survival.bladder.total();
        let fill = (total / self.survival.bladder.capacity_liters).clamp(0., 1.);
        if total > 0. {
            self.urine.set_color(
                self.survival
                    .bladder
                    .mixture()
                    .quantities_liters
                    .map(|v| v / total),
            );
        }
        let output = self.survival.toilet_step(
            &reference.survival,
            crate::survival::ToiletInput {
                urinate: input.held & keys::URINATE != 0,
                burp_held: input.held & keys::BURP != 0,
                burp_pressed: pressed,
            },
            dt,
            draws,
            selection.3,
            fluid_scale,
        );
        if output.poured_liters > 0. {
            let flow = output.poured_liters / (dt * fluid_scale);
            self.urine_emission = [
                reference.survival.toilet.min_rate
                    + (reference.survival.toilet.max_rate - reference.survival.toilet.min_rate)
                        * flow.clamp(0., 1.),
                reference.survival.toilet.max_speed * fill,
            ];
        }
        if output.burp || output.fart || output.defecated {
            self.emit_selected_audio(kind, selection);
        }
        if self.apply_toilet_output(reference, output).is_err() {
            self.notice = "Cannot create toilet item.";
        }
    }
}

impl GarageRuntime {
    pub fn sleeping(&self) -> bool {
        self.sleep.output().phase == crate::sleep::SleepPhase::Sleeping
    }

    pub fn sleep_camera(&self) -> Option<crate::sleep::SleepCamera> {
        self.sleep_camera
    }

    fn select_sleep_camera(&mut self, reference: &ReferenceManifest) {
        use rapier3d::math::{Rotation, Vector};
        use rapier3d::prelude::{ColliderHandle, QueryFilter, Ray};
        let r = &reference.sleep;
        // Uniform direction matches normalized insideUnitCircle; the sequence is a runtime adapter.
        let angle = self.draw_survival() * std::f32::consts::TAU;
        let radius = r.sleep_cam_ring_min
            + self.draw_survival() * (r.sleep_cam_ring_max - r.sleep_cam_ring_min);
        let target = Vector::from_array(self.area.player.body_pose(&self.physics).position);
        let ray = Ray::new(
            target + Vector::new(angle.cos() * radius, 10000., angle.sin() * radius),
            -Vector::Y,
        );
        let mask = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            collider.is_enabled()
                && collider.collision_groups().memberships.bits() & r.sleep_layer_mask != 0
                && !collider
                    .shape()
                    .contains_point(collider.position(), ray.origin)
        };
        let hit = self
            .physics
            .queries(QueryFilter::default().predicate(&mask))
            .cast_ray(&ray, f32::INFINITY, true);
        let (position, rotation) = if let Some((_, distance)) = hit {
            let height = r.sleep_cam_height_min
                + self.draw_survival() * (r.sleep_cam_height_max - r.sleep_cam_height_min);
            let position = ray.point_at(distance) + Vector::Y * height;
            let direction = (target - position).normalize();
            let rotation = Rotation::from_rotation_y((-direction.x).atan2(-direction.z))
                * Rotation::from_rotation_x(direction.y.asin());
            (position, rotation)
        } else {
            let yaw = self.draw_survival() * std::f32::consts::TAU;
            (
                target + Vector::Y * 50.,
                Rotation::from_rotation_y(yaw)
                    * Rotation::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            )
        };
        self.sleep_camera = Some(crate::sleep::SleepCamera {
            absolute_position: self.stream.origin.absolute(position.to_array()),
            rotation: rotation.to_array(),
        });
    }

    pub(super) fn sync_sleep_transition(
        &mut self,
        reference: &ReferenceManifest,
        was_sleeping: bool,
    ) {
        if self.sleeping() {
            if !was_sleeping {
                self.select_sleep_camera(reference);
                self.clear_input();
                self.aim = super::ResolvedAim::default();
                self.notice = "";
                self.audio_motion_revision = self.audio_motion_revision.wrapping_add(1);
            }
            let progress = self.sleep.output().progress;
            let gain = ((progress - 0.05) / 0.35)
                .clamp(0., 1.)
                .min(((0.95 - progress) / 0.35).clamp(0., 1.));
            self.sleep_audio(
                if was_sleeping {
                    crate::audio::AudioAction::Update
                } else {
                    crate::audio::AudioAction::Start
                },
                gain,
            );
        } else if was_sleeping {
            self.interrupt_sleep(reference);
        }
    }

    pub(super) fn interrupt_sleep(&mut self, reference: &ReferenceManifest) {
        self.sleep.interrupt();
        if self.sleep_camera.take().is_some() {
            self.sleep_audio(crate::audio::AudioAction::Stop, 0.);
            self.audio_motion_revision = self.audio_motion_revision.wrapping_add(1);
            self.previous_speed = None;
        }
        self.lighting
            .cycle
            .set_sleep_progress(&reference.lighting.cycle, None);
    }

    fn sleep_audio(&mut self, action: crate::audio::AudioAction, gain: f32) {
        let kind = crate::audio::AudioKind::Sleep;
        let Some(playback) = &mut self.audio else {
            return;
        };
        let position = self.sleep_camera.map_or_else(
            || {
                self.stream
                    .origin
                    .absolute(self.area.player.view(&self.physics).head_pose.position)
            },
            |camera| camera.absolute_position,
        );
        if action == crate::audio::AudioAction::Stop {
            playback.stop_survival_loop(kind, 0);
            playback.sleep_radio_advanced = false;
        } else {
            playback.source_loop(kind, 0, gain, position);
            if gain > 0.5 && !playback.sleep_radio_advanced {
                playback.sleep_radio_advanced = true;
                playback.broadcasts.advance_sleep(playback.game_seconds);
            }
        }
    }
}

fn urine_outlet_pose(
    view: crate::player::PlayerView,
    anchor_midpoint: rapier3d::math::Vector,
    reach: f32,
    queries: &rapier3d::prelude::QueryPipeline<'_>,
) -> rapier3d::math::Pose {
    use rapier3d::math::{Rotation, Vector};
    use rapier3d::prelude::Ray;
    let view = to_rapier(view.head_pose);
    let mut outlet = rapier3d::math::Pose::from_translation(anchor_midpoint);
    let ray = Ray::new(view.translation, view.rotation * -Vector::Z);
    let distance = queries
        .cast_ray(&ray, reach, true)
        .map_or(reach, |(_, t)| t);
    let direction =
        (ray.point_at(distance) - outlet.translation + Vector::Y * 0.25).normalize_or_zero();
    outlet.rotation = Rotation::from_rotation_arc(-Vector::Z, direction);
    outlet
}

impl GarageRuntime {
    pub(super) fn advance_urine(&mut self, reference: &ReferenceManifest, dt: f32) {
        use rapier3d::math::Vector;
        use rapier3d::prelude::QueryFilter;
        let body = self.area.player.body_handle();
        let surfaces = |_: rapier3d::prelude::ColliderHandle, c: &rapier3d::prelude::Collider| {
            c.is_enabled()
                && !c.is_sensor()
                && c.parent() != Some(body)
                && c.collision_groups().memberships.bits() & reference.player.action_masks.use_mask
                    != 0
        };
        let cap = urine_outlet_pose(
            self.area.player.view(&self.physics),
            self.area.player.source_anchor_midpoint(&self.physics),
            reference.player.aim_reach,
            &self
                .physics
                .queries(QueryFilter::default().predicate(&surfaces)),
        );
        // The mouth and urine outlet own tank 73403. Source Tankolas excludes self-drinking.
        self.urine.advance(
            &self.urine_reference,
            cap,
            -Vector::Z,
            Some(body),
            Vector::ZERO,
            0.,
            0.,
            &self.physics,
            dt,
            self.urine_emission[0],
            self.urine_emission[1],
            None,
        );
    }
}
