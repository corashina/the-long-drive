use super::*;
use crate::{
    interaction::{equipment::EquipmentPlacement, items::ItemLocation},
    inventory::{InventoryCommand, InventoryContext},
};
use rapier3d::math::{Pose, Rotation, Vector};

impl StartingArea {
    pub fn equip_carried(&mut self, physics: &mut PhysicsWorld) -> Result<bool, ReferenceError> {
        if !self
            .interaction
            .can_equip_carried(&self.player, &self.items, physics)
        {
            return Ok(false);
        }
        let id = self.interaction.held().expect("validated carry equipment");
        self.interaction.carry.drop(&mut self.items, physics);
        self.interaction.equipment.equip(
            id,
            EquipmentPlacement::Animated,
            &mut self.items,
            physics,
        );
        physics.refresh_queries();
        Ok(true)
    }
    pub fn release_equipped(&mut self, physics: &mut PhysicsWorld) -> Result<bool, ReferenceError> {
        let Some(id) = self.interaction.equipment.item() else {
            return Ok(false);
        };
        if !self
            .interaction
            .carry
            .can_receive_equipment(id, &self.player, &self.items, physics)
        {
            return Ok(false);
        }
        self.interaction.equipment.detach(&mut self.items, physics);
        self.interaction
            .pickup_item(
                id,
                self.player.view(physics).head_pose.position,
                &self.player,
                &mut self.items,
                physics,
            )
            .expect("validated equipment receipt");
        // Source RightHandRelease: Drop leaves pickedUpPoint on TpickedUpParent.
        self.interaction.carry.grip_at_anchor();
        physics.refresh_queries();
        Ok(true)
    }
    pub fn sync_inventory(&mut self) {
        for slot in &mut self.inventory.slots {
            if slot.is_some_and(|id| {
                self.items.get(id).is_none_or(|i| {
                    i.removed
                        || !matches!(
                            i.location,
                            ItemLocation::Stored(_) | ItemLocation::Held | ItemLocation::Equipped
                        )
                })
            }) {
                *slot = None;
            }
        }
        if let Some(id) = self.interaction.held() {
            if self.inventory.slots[self.inventory.selected as usize].is_none()
                && self
                    .interaction
                    .pickup_rule(id)
                    .is_some_and(|p| p.can_inventory)
                && !self.inventory.slots.contains(&Some(id))
            {
                self.inventory.slots[self.inventory.selected as usize] = Some(id);
            }
        }
    }
    pub fn inventory_context(&self, input: FrameInput) -> InventoryContext {
        let carried = self.interaction.held();
        let ancestor_carry = self.player.seat_body().is_some_and(|body| {
            carried
                .and_then(|id| self.items.get(id))
                .and_then(|i| i.body)
                == Some(body)
        });
        InventoryContext {
            carried,
            equipped: self.interaction.equipment.item(),
            ancestor_carry,
            seated_clutch: self.inventory_clutch_conflict
                && self.player.seated()
                && input.held & keys::CLUTCH != 0,
        }
    }
    pub fn apply_inventory_command(
        &mut self,
        command: InventoryCommand,
        reference: &ReferenceManifest,
        physics: &mut PhysicsWorld,
    ) -> Result<bool, ReferenceError> {
        let index = match command {
            InventoryCommand::Select(i) | InventoryCommand::Swap(i) => i as usize,
        };
        if index >= 4 {
            return Ok(false);
        }
        let selected = self.inventory.selected as usize;
        let outgoing = self.inventory.slots[selected];
        let incoming = self.inventory.slots[index];
        let held = self.interaction.held();
        let equipped = self.interaction.equipment.item();
        let drop_carry = matches!(command, InventoryCommand::Select(_))
            && incoming.is_some()
            && held.is_some_and(|id| {
                self.interaction
                    .pickup_rule(id)
                    .is_some_and(|p| !p.can_inventory)
            });
        let carried_after = held.filter(|id| Some(*id) != outgoing && !drop_carry);
        let equipped_after = equipped.filter(|id| Some(*id) != outgoing);
        if self.inventory_context(FrameInput::default()).ancestor_carry
            || (held.is_some() && !drop_carry && equipped.is_some() && incoming.is_some())
        {
            return Ok(false);
        }
        // Validate both ends before releasing a joint, body or slot.
        for id in outgoing.into_iter().chain(incoming) {
            let Some(item) = self.items.get(id) else {
                return Ok(false);
            };
            let Some(body) = item.body.and_then(|h| physics.rapier().bodies.get(h)) else {
                return Ok(false);
            };
            if item.removed
                || !self
                    .interaction
                    .pickup_rule(id)
                    .is_some_and(|p| p.can_inventory)
                || item.pending_attachment
                || !physics.body_active(item.body.unwrap())
                || physics.vehicle_of(item.body.unwrap()).is_some()
                || !self.interaction.carry.allows_pickup(
                    id,
                    self.player.seated(),
                    self.items.mounted(id),
                )
                || item.inactive_pose.is_some()
            {
                return Ok(false);
            }
            if Some(id) == outgoing {
                let carried = held == Some(id)
                    && item.location == ItemLocation::Held
                    && self.interaction.carry.body() == item.body
                    && body.is_dynamic();
                let equipped = equipped == Some(id)
                    && item.location == ItemLocation::Equipped
                    && body.body_type() == rapier3d::prelude::RigidBodyType::KinematicPositionBased;
                if !body.is_enabled() || !(carried || equipped) {
                    return Ok(false);
                }
            } else if item.location != ItemLocation::Stored(index as u8)
                || !body.is_dynamic()
                || body.is_enabled()
            {
                return Ok(false);
            }
        }
        let to_hand = incoming.is_some_and(|id| {
            self.interaction
                .pickup_rule(id)
                .unwrap()
                .can_hold_right_hand
        }) && equipped_after.is_none();
        if incoming.is_some() && !to_hand && carried_after.is_some() {
            return Ok(false);
        }
        if drop_carry {
            self.interaction.drop_held(&mut self.items, physics);
        }
        if let Some(id) = outgoing {
            let storage = to_rapier(self.pose.inventory_pose(self.player.body_pose(physics)));
            let body = self.items.get(id).unwrap().body.unwrap();
            self.inventory_offsets[selected] = Some((
                id,
                crate::vehicle::from_rapier(
                    &(storage.inverse() * physics.rapier().bodies[body].position()),
                ),
            ));
            if self.interaction.held() == Some(id) {
                self.interaction.carry.drop(&mut self.items, physics);
            }
            if self.interaction.equipment.item() == Some(id) {
                self.interaction.equipment.detach(&mut self.items, physics);
            }
            let item = self.items.get_mut(id).unwrap();
            item.location = ItemLocation::Stored(selected as u8);
            physics.edit_body(item.body.unwrap(), |body| body.set_enabled(false));
        }
        match command {
            InventoryCommand::Select(i) => {
                self.inventory.selected = i;
                self.inventory.swap_progress = 0.;
            }
            InventoryCommand::Swap(_) => {
                self.inventory.record_swap();
                self.inventory.slots.swap(selected, index);
                self.inventory_offsets.swap(selected, index);
                if index != selected
                    && let Some(id) = self.inventory.slots[index]
                {
                    self.items.get_mut(id).unwrap().location = ItemLocation::Stored(index as u8);
                }
            }
        }
        if let Some(id) = self.inventory.slots[self.inventory.selected as usize] {
            self.inventory_offsets[self.inventory.selected as usize] = None;
            let body = self.items.get(id).unwrap().body.unwrap();
            physics.edit_body(body, |body| body.set_enabled(true));
            self.items.get_mut(id).unwrap().location = ItemLocation::World;
            if to_hand {
                self.interaction.equipment.equip(
                    id,
                    EquipmentPlacement::Immediate,
                    &mut self.items,
                    physics,
                );
            } else {
                let rule = self.interaction.pickup_rule(id).unwrap();
                let view = to_rapier(self.player.view(physics).head_pose);
                let offset = if rule.use_recenter_position {
                    Vector::from_array(rule.recenter_position)
                } else {
                    Vector::from_array(reference.player.carry_anchor_offset)
                        * rule.recenter_distance
                };
                let rotation = if rule.use_recenter_rotation {
                    Rotation::from_array(rule.recenter_rotation)
                } else {
                    Rotation::IDENTITY
                };
                let target = Pose::from_parts(
                    view.translation
                        + view.rotation
                            * (Vector::from_array(reference.player.carry_anchor_offset) + offset),
                    view.rotation * rotation,
                ) * to_rapier(self.items.get(id).unwrap().local_pose).inverse();
                physics.edit_body(body, |body| body.set_position(target, false));
                self.interaction
                    .pickup_item(
                        id,
                        self.player.view(physics).head_pose.position,
                        &self.player,
                        &mut self.items,
                        physics,
                    )
                    .expect("validated inventory retrieval");
                // Source inventory retrieval puts pickedUpPoint on TpickedUpParent.
                self.interaction.carry.grip_at_anchor();
            }
        }
        self.sync_inventory();
        self.sync_equipment(reference, physics);
        physics.refresh_queries();
        Ok(true)
    }
    pub fn sync_equipment(&mut self, reference: &ReferenceManifest, physics: &mut PhysicsWorld) {
        self.pose.set_frames(
            self.player.view(physics).head_pose,
            self.player.body_pose(physics),
        );
        self.pose.update(
            &self.player,
            &self.interaction,
            &self.items,
            &reference.clothing,
            FIXED_DT,
        );
        if let Some(id) = self.interaction.equipment.item() {
            self.interaction.equipment.sync(
                &self.pose,
                &self.items,
                self.interaction.carry.rule(id).unwrap(),
                physics,
            );
        }
        let storage = to_rapier(self.pose.inventory_pose(self.player.body_pose(physics)));
        for (slot, id) in self.inventory.slots.iter().enumerate() {
            let Some(id) = id else { continue };
            let item = self.items.get(*id).unwrap();
            if matches!(item.location, ItemLocation::Stored(_))
                && let Some(h) = item.body
            {
                let frame = self.inventory_offsets[slot].get_or_insert_with(|| {
                    (
                        *id,
                        crate::vehicle::from_rapier(
                            &(storage.inverse() * physics.rapier().bodies[h].position()),
                        ),
                    )
                });
                physics.edit_body(h, |body| {
                    body.set_position(storage * to_rapier(frame.1), false);
                    body.set_enabled(false);
                });
            }
        }
    }
    pub fn inventory_body(&self, handle: RigidBodyHandle) -> Option<ItemLocation> {
        self.inventory
            .slots
            .iter()
            .flatten()
            .filter_map(|id| self.items.get(*id))
            .find(|i| i.body == Some(handle))
            .map(|i| i.location)
    }
}
/// One gameplay-frame result for the pending fixed-step input.
pub struct InventoryFrame {
    pub fixed_input: FrameInput,
    pub primary_reserved: bool,
    pub actions_blocked: bool,
    pub recentered: Option<ItemId>,
}
impl InventoryFrame {
    /// Merge frame actions into the pending fixed input once.
    pub fn merge_fixed_input(&self, pending: &mut FrameInput) {
        if self.actions_blocked {
            pending.clear();
        } else {
            if self.primary_reserved {
                pending.held &= !keys::PRIMARY;
                pending.pressed &= !keys::PRIMARY;
                pending.released &= !keys::PRIMARY;
            }
            if self.recentered.is_some() && pending.carry.owner == self.recentered {
                pending.carry = crate::simulation::CarryDeltas::default();
            }
        }
        pending.merge(self.fixed_input);
    }
}
impl StartingArea {
    fn equipment_context(&self) -> crate::interaction::equipment_input::EquipmentContext {
        crate::interaction::equipment_input::EquipmentContext {
            carried_can_equip: self
                .interaction
                .held()
                .and_then(|id| self.interaction.pickup_rule(id))
                .is_some_and(|r| r.can_hold_right_hand),
            carried: self.interaction.held(),
            has_equipment: self.interaction.equipment.item().is_some(),
            seated: self.player.seated(),
        }
    }
}
impl GarageRuntime {
    pub fn advance_inventory_frame(
        &mut self,
        reference: &ReferenceManifest,
        input: FrameInput,
        dt: f32,
        view: VehiclePose,
    ) -> InventoryFrame {
        use crate::interaction::{
            cleaning::{ToolFrame, ToolOwner},
            equipment_input::EquipAction,
        };
        let dead = self.needs_output().dead;
        if !self.started
            || self.paused
            || self.recovering
            || !self.world_supported()
            || self.sleeping()
            || dead
            || !dt.is_finite()
            || dt <= 0.
        {
            self.clear_input();
            self.sync_tool_audio(reference);
            return InventoryFrame {
                fixed_input: FrameInput {
                    mouse: if dead { input.mouse } else { [0.; 2] },
                    ..Default::default()
                },
                primary_reserved: false,
                actions_blocked: true,
                recentered: None,
            };
        }
        self.area
            .interaction
            .finish_cancelled_input(&mut self.area.player, &mut self.physics);
        let input = self.area.interaction.filter_cancelled_input(input);
        self.area.sync_inventory();
        let context = self.area.equipment_context();
        let action = self
            .area
            .interaction
            .equipment_input
            .begin_frame(input, dt, context);
        let recentered = context
            .carried
            .filter(|_| action == Some(EquipAction::Recenter));
        match action {
            Some(EquipAction::Equip) => {
                if self
                    .area
                    .equip_carried(&mut self.physics)
                    .expect("checked equipment bindings")
                {
                    self.area.interaction.equipment_input.record_equipped();
                }
            }
            Some(EquipAction::Recenter) => self
                .area
                .interaction
                .carry
                .recenter(&self.area.player, &self.physics),
            None => {}
        }
        let context = self.area.inventory_context(input);
        if let Some(command) = self
            .area
            .inventory
            .released_command(input.inventory_released, context)
        {
            self.area
                .apply_inventory_command(command, reference, &mut self.physics)
                .expect("checked inventory bindings");
        }
        let context = self.area.inventory_context(input);
        if let Some(command) = self
            .area
            .inventory
            .held_command(input.inventory_held, dt, context)
        {
            self.area
                .apply_inventory_command(command, reference, &mut self.physics)
                .expect("checked inventory bindings");
        }
        self.advance_melee(reference, input, dt);
        let gun_held = self.advance_guns(reference, input, dt, view);
        self.run_explosions(reference, dt);
        self.area.sync_equipment(reference, &mut self.physics);
        if let Some(id) = self.area.interaction.equipment.item() {
            self.area.interaction.equipment.advance_hand(
                dt,
                &self.area.pose,
                &self.area.items,
                self.area.interaction.carry.rule(id).unwrap(),
                &mut self.physics,
            );
        }
        self.area.items.sync_poses(&self.physics);
        self.physics.refresh_queries();
        let frame = ToolFrame {
            lmb: input.held & keys::PRIMARY != 0,
            active: true,
            dt,
            view,
            origin: self.stream.origin.offset,
        };
        // fpscontroller: a gun in the hand sets noLMBUse.
        let mut primary_reserved = gun_held || self.area.interaction.reserves_lmb();
        self.area
            .interaction
            .cleaning
            .sync_wash_colliders(&self.area.interaction.conditions, &mut self.physics);
        self.area.interaction.cleaning.begin_frame(frame);
        if let Some(id) = self.area.interaction.equipment.item() {
            self.advance_owned_tool(
                reference,
                input,
                frame,
                ToolOwner::Equipped(id),
                id,
                &mut primary_reserved,
            );
        }
        let context = self.area.equipment_context();
        if self
            .area
            .interaction
            .equipment_input
            .should_release(input, context)
            && self
                .area
                .release_equipped(&mut self.physics)
                .expect("checked equipment release")
        {
            self.area.interaction.equipment_input.record_released();
        }
        self.area.sync_equipment(reference, &mut self.physics);
        self.area.items.sync_poses(&self.physics);
        self.physics.refresh_queries();
        if let Some(id) = self.area.interaction.held() {
            self.advance_owned_tool(
                reference,
                input,
                frame,
                ToolOwner::Carried(id),
                id,
                &mut primary_reserved,
            );
        }
        let point = input.player_held & crate::simulation::player_keys::POINT != 0;
        self.area.player.step_point(point, &self.physics, dt);
        self.area.interaction.cleaning.advance_player(
            point,
            self.area.player.body_pose(&self.physics),
            frame,
            reference,
            &mut self.area.interaction.conditions,
            &self.area.items,
            &self.physics,
            &self.area.interaction.parts,
        );
        self.stream.advance_sprays(
            frame,
            reference,
            &mut self.area.interaction.cleaning,
            &mut self.area.interaction.conditions,
            &mut self.area.items,
            &self.physics,
        );
        self.stream.advance_genie_lamps(
            frame.dt,
            self.clock_factors(reference).fluid_scale,
            reference,
            &mut self.area.interaction.cleaning,
            &mut self.area.items,
        );
        self.clean_car_blood();
        self.stream.advance_cobwebs(
            frame,
            reference,
            &mut self.area.interaction.cleaning,
            &mut self.physics,
        );
        self.play_small_item_sounds();
        self.area.interaction.cleaning.finish_frame(
            frame,
            reference,
            &mut self.area.interaction.conditions,
            &self.area.items,
            &self.physics,
            &self.area.interaction.parts,
        );
        for item in self.area.interaction.cleaning.take_changed() {
            self.area.items.mark_dirty(item);
        }
        self.stream.sync_tool_looks(
            &self.area.interaction.cleaning,
            &self.area.interaction.conditions,
            &self.area.items,
        );
        self.area
            .interaction
            .cleaning
            .sync_wash_colliders(&self.area.interaction.conditions, &mut self.physics);
        self.sync_tool_audio(reference);
        let reach = reference.player.aim_reach;
        let aimed = [
            self.aim.focus_target,
            self.aim.grab_target,
            self.aim.use_target,
        ]
        .into_iter()
        .flatten()
        .find(|t| t.distance.is_finite() && t.distance <= reach)
        .map(|t| t.item);
        self.stream.refill_bottles(
            &reference.small_items.spray_bottle,
            crate::world::stream::content_small_items::RefillInput {
                carried: self.area.interaction.held(),
                hand: self.area.interaction.equipment.item(),
                aimed,
                use_held: input.held & keys::USE != 0,
                use_released: input.released & keys::USE != 0,
                reload_held: input.held & keys::RELOAD != 0,
                reload_released: input.released & keys::RELOAD != 0,
            },
            &mut self.area.items,
        );
        self.area.interaction.equipment_input.finish_frame(input);
        let available = self.area.interaction.available_actions(
            &self.aim,
            &self.area.player,
            &self.area.items,
            &self.physics,
            self.needs_output(),
        );
        let mut look_input = input;
        if self.area.interaction.guns.reloading {
            // fpscontroller.reloading: R with a gun moves the gun, not the view or the carry.
            look_input.mouse = [0.; 2];
            look_input.wheel = 0.;
        }
        if self.driving_input.mouse_owned() {
            // Source `inputscript.mouseDrive`: mouse look axes read zero.
            look_input.mouse = [0.; 2];
        }
        let mut fixed_input =
            self.area
                .interaction
                .route_frame_input(look_input, &available, &self.area.player);
        let view_recentering = context.seated && context.carried.is_none();
        // `step_wear` reads the Tab release that puts the carried wearable on.
        let put_on = self.area.wear.actions(&self.area.interaction)
            & crate::interaction::feedback::ActionMask::PUT_ON
            != 0;
        if !view_recentering && !put_on {
            fixed_input.held &= !keys::RECENTER;
            fixed_input.pressed &= !keys::RECENTER;
            fixed_input.released &= !keys::RECENTER;
        }
        fixed_input.inventory_held = 0;
        fixed_input.inventory_released = 0;
        if recentered.is_some() && fixed_input.carry.owner == recentered {
            fixed_input.carry = crate::simulation::CarryDeltas::default();
        }
        if primary_reserved {
            fixed_input.held &= !keys::PRIMARY;
            fixed_input.pressed &= !keys::PRIMARY;
            fixed_input.released &= !keys::PRIMARY;
        }
        InventoryFrame {
            fixed_input,
            primary_reserved,
            actions_blocked: false,
            recentered,
        }
    }
    fn advance_owned_tool(
        &mut self,
        reference: &ReferenceManifest,
        input: FrameInput,
        frame: crate::interaction::cleaning::ToolFrame,
        owner: crate::interaction::cleaning::ToolOwner,
        id: ItemId,
        primary_reserved: &mut bool,
    ) {
        if !*primary_reserved
            && input.pressed & keys::PRIMARY != 0
            && let Some(control) = self.area.interaction.owned_light_control(id)
        {
            self.use_light_control(reference, control);
            *primary_reserved = true;
        }
        self.area.interaction.cleaning.advance_owner(
            owner,
            frame,
            reference,
            &mut self.area.interaction.conditions,
            &self.area.items,
            &self.physics,
            &self.area.interaction.parts,
        );
        self.stream.press_spray(
            owner,
            frame,
            reference,
            &self.area.interaction.cleaning,
            crate::interaction::cleaning::ToolWorld {
                conditions: &self.area.interaction.conditions,
                items: &self.area.items,
                physics: &self.physics,
                parts: &self.area.interaction.parts,
            },
        );
    }
    /// `weaponhandlingscript` and `meleeweaponscript` for a melee weapon in the right hand.
    fn advance_melee(&mut self, reference: &ReferenceManifest, input: FrameInput, dt: f32) {
        use crate::interaction::melee::MeleeInput;
        let interaction = &mut self.area.interaction;
        let weapon = interaction.equipment.item().and_then(|id| {
            let rule = interaction.carry.rule(id)?;
            Some((id, interaction.melee.weapon_for(&rule.source)?))
        });
        let held = |key| input.held & key != 0;
        let melee = MeleeInput {
            pressed: input.pressed & keys::PRIMARY != 0,
            released: input.released & keys::PRIMARY != 0,
            held: held(keys::PRIMARY),
            back: held(keys::BACK),
            left: held(keys::LEFT),
            right: held(keys::RIGHT),
        };
        let player = interaction.player_body();
        interaction.melee.step(
            melee,
            dt,
            weapon,
            &mut self.area.items,
            &mut self.physics,
            &mut interaction.buried,
            &mut self.stream.breakables,
            &reference.breakables,
            player,
            self.area.origin,
        );
        self.area.pose.set_hand(interaction.melee.hand());
        for sound in interaction.melee.take_sounds() {
            if let Some(audio) = &mut self.audio {
                audio.survival_event(crate::audio::SurvivalAudioEvent {
                    session: audio.session,
                    emitter: 0,
                    kind: crate::audio::AudioKind::MeleeHit,
                    action: crate::audio::AudioAction::Start,
                    clip: sound.clip,
                    gain: sound.gain,
                    pitch: 1.,
                    position: sound.position,
                });
            }
        }
        while let Some(item) = self.area.interaction.melee.take_cut() {
            if self.cut_food(reference, item, true).is_err() {
                self.notice = "Cannot cut food.";
            }
        }
        while let Some((item, damage)) = self.area.interaction.melee.take_strike() {
            self.try_explode_tank(item, damage, false);
            self.try_explode_prop(item, damage);
        }
    }
    /// mainscript.PlayClipAtPoint for spray bottle clips.
    fn play_small_item_sounds(&mut self) {
        for (clip, gain, position) in self.stream.take_small_item_sounds() {
            if let Some(audio) = &mut self.audio {
                audio.survival_event(crate::audio::SurvivalAudioEvent {
                    session: audio.session,
                    emitter: 0,
                    kind: crate::audio::AudioKind::ItemHit,
                    action: crate::audio::AudioAction::Start,
                    clip,
                    gain,
                    pitch: 1.,
                    position,
                });
            }
        }
    }
}

impl StartingArea {
    pub fn capture_inventory(
        &self,
        ids: &crate::save::IdentityMap,
    ) -> Result<crate::save::InventorySnapshot, crate::save::SaveError> {
        let mut slots = [None, None, None, None];
        // Spawned items and runtime foods save in the world, so their slots save empty.
        let saved_item = |id: &ItemId| !self.saves_in_world(*id);
        for (saved, id) in slots.iter_mut().zip(self.inventory.slots) {
            *saved = id
                .filter(saved_item)
                .map(|id| ids.item_instance(id))
                .transpose()?;
        }
        Ok(crate::save::InventorySnapshot {
            slots,
            selected: self.inventory.selected,
            equipped: self
                .interaction
                .equipment
                .item()
                .filter(saved_item)
                .map(|id| ids.item_instance(id))
                .transpose()?,
            worn: self.capture_worn(ids)?,
        })
    }
    fn capture_worn(
        &self,
        ids: &crate::save::IdentityMap,
    ) -> Result<[Option<crate::save::WornSnapshot>; 5], crate::save::SaveError> {
        use crate::save::WornSnapshot;
        let mut worn = [const { None }; 5];
        for (saved, item) in worn.iter_mut().zip(self.wear.worn.slots) {
            let Some(item) = item else { continue };
            *saved = Some(match self.spawned.get(item.id) {
                Some(spawned) => WornSnapshot::Spawned {
                    serial: spawned.serial,
                },
                None => WornSnapshot::Item {
                    item: ids.item_instance(item.id)?,
                },
            });
        }
        Ok(worn)
    }
}
