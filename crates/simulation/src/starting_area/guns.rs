//! Guns in the player's hands: `fpscontroller` trigger, reload and unload, break barrel and
//! cylinder input, and the gun outputs for save, audio, the flash light and explosive hits.
use super::GarageRuntime;
use crate::interaction::feedback::ItemPrompt;
use crate::interaction::guns::{AmmoItem, HeldGun, ShotWorld, accepts};
use crate::reference::{ItemId, ReferenceManifest};
use crate::simulation::{FrameInput, keys};
use crate::vehicle::{VehiclePose, to_rapier};
use crate::world::ObjectKey;

impl GarageRuntime {
    /// `fpscontroller.PullTrigger`, `Csoletores`, `RWheel`, `Reload` and `Unload`, and
    /// `weaponscript.Update`. True while a gun is in the hand.
    pub(super) fn advance_guns(
        &mut self,
        reference: &ReferenceManifest,
        input: FrameInput,
        dt: f32,
        view: VehiclePose,
    ) -> bool {
        self.bind_guns();
        let interaction = &mut self.area.interaction;
        let is_gun = |id: &ItemId| interaction.guns.runtime(*id).is_some();
        let hand = interaction.equipment.item();
        let hand_gun = hand.filter(is_gun);
        // pP.weapon: the gun in the hand, else the carried gun.
        let active = if hand.is_some() {
            hand_gun
        } else {
            interaction.held().filter(is_gun)
        };
        let tap = interaction.guns.reload_key(
            dt,
            input.pressed & keys::RELOAD != 0,
            input.held & keys::RELOAD != 0,
            input.released & keys::RELOAD != 0,
        );
        let reloading = input.held & keys::RELOAD != 0 && active.is_some();
        interaction.guns.reloading = reloading;
        if reloading && let Some(gun) = active {
            let position = self.item_position(gun);
            let guns = &mut self.area.interaction.guns;
            // Source LOOKY is positive up. Browser +Y is down.
            guns.break_barrel(gun, [input.mouse[0], -input.mouse[1]], position);
            guns.wheel(gun, input.wheel);
        }
        self.reload_guns(reference, input, tap);
        let interaction = &mut self.area.interaction;
        let held = hand_gun.and_then(|item| {
            let rule = interaction.carry.rule(item)?;
            let gun = interaction.guns.weapon_for(&rule.source)?;
            let condition = interaction.conditions.item_state(item).map(f32::from);
            Some(HeldGun {
                item,
                gun,
                view: to_rapier(view),
                condition,
            })
        });
        let player = interaction.player_body();
        let mut world = ShotWorld {
            items: &self.area.items,
            physics: &mut self.physics,
            breakables: &mut self.stream.breakables,
            records: &reference.breakables,
            hit_particles: interaction.melee.particles_mut(),
            player,
            origin: self.area.origin,
        };
        interaction
            .guns
            .step(dt, input.held & keys::PRIMARY != 0, held, &mut world);
        for item in interaction.guns.take_changed() {
            self.area.items.mark_dirty(item);
        }
        if let Some((item, node, intensity)) = interaction.guns.flash_light()
            && let ObjectKey::Generated(parent) = self.area.items.object_key(item)
        {
            self.stream
                .set_node_light(&parent, node, intensity.is_some(), intensity.unwrap_or(0.));
        }
        for sound in interaction.guns.take_sounds() {
            if let Some(audio) = &mut self.audio {
                // `PlayClipAtPoint` destroys the copy after the clip length.
                let seconds = audio
                    .reference
                    .clips
                    .iter()
                    .find(|c| c.id == sound.clip)
                    .map_or(0., |c| c.source_duration_seconds as f32);
                self.creatures
                    .sound(sound.position, sound.gain, self.simulation_seconds, seconds);
                audio.survival_event(crate::audio::SurvivalAudioEvent {
                    session: audio.session,
                    emitter: 0,
                    kind: crate::audio::AudioKind::ItemHit,
                    action: crate::audio::AudioAction::Start,
                    clip: sound.clip,
                    gain: sound.gain,
                    pitch: sound.pitch,
                    position: sound.position,
                });
            }
        }
        while let Some(item) = self.area.interaction.guns.take_food() {
            if self.cut_food(reference, item, false).is_err() {
                self.notice = "Cannot shoot food.";
            }
        }
        while let Some(strike) = self.area.interaction.guns.take_strike() {
            self.shot_explosive(strike);
        }
        while let Some(hit) = self.area.interaction.guns.take_creature_hit() {
            self.creatures.shoot(
                &reference.creatures.rabbit,
                &self.physics,
                hit.collider,
                hit.force,
                hit.direction,
            );
        }
        self.settle_rabbits(reference);
        self.store_decals(dt);
        held.is_some()
    }
    /// Moves the shot bullet holes into the decal store and ages the store.
    fn store_decals(&mut self, dt: f32) {
        let area = &mut self.area;
        let interaction = &mut area.interaction;
        for spawn in interaction.guns.take_decals() {
            // The renderer interpolates area bodies and items. World structures show the step pose.
            let body = self
                .physics
                .rapier()
                .colliders
                .get(spawn.collider)
                .and_then(|c| c.parent());
            let smooth = body.is_some_and(|b| {
                area.bodies.values().any(|h| *h == b)
                    || area.items.iter().any(|i| i.body == Some(b))
            });
            interaction.decals.spawn(spawn, &self.physics, smooth);
        }
        interaction.decals.advance(dt);
    }
    /// Gives every gun item its runtime when the items change: the saved state, or the
    /// `RandomSpawned` count of a generated gun.
    fn bind_guns(&mut self) {
        let items = &self.area.items;
        let interaction = &mut self.area.interaction;
        if !interaction.guns.needs_bind(items.membership_revision()) {
            return;
        }
        interaction
            .guns
            .retain(|id| items.get(id).is_some_and(|s| !s.removed));
        for state in items.iter() {
            let id = state.id;
            if state.removed || interaction.guns.runtime(id).is_some() {
                continue;
            }
            let Some(gun) = interaction
                .carry
                .rule(id)
                .and_then(|r| interaction.guns.weapon_for(&r.source))
            else {
                continue;
            };
            let saved = self.stream.saved_gun(items, id);
            let count = self.stream.content_ammo(id);
            interaction.guns.bind(id, gun, saved, count);
            // weaponscript.Update keeps the flash off until a shot.
            if let Some(node) = interaction.guns.flash_node(id)
                && let ObjectKey::Generated(parent) = items.object_key(id)
            {
                self.stream.set_node_light(&parent, node, false, 0.);
            }
        }
    }
    fn item_position(&self, item: ItemId) -> [f64; 3] {
        self.area.items.get(item).map_or(self.area.origin, |s| {
            self.stream.origin.absolute(s.pose.position)
        })
    }
    /// `fpscontroller` reload and unload rules: a gun alone in the hands and the aimed ammo,
    /// ammo in the hands and the aimed gun, then `ReloadFromInventory`. R taps load, E unloads.
    fn reload_guns(&mut self, reference: &ReferenceManifest, input: FrameInput, mut tap: bool) {
        let mut unload = input.released & keys::USE != 0;
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
        let interaction = &self.area.interaction;
        let guns = &interaction.guns;
        let gun = |id: Option<ItemId>| id.filter(|id| guns.runtime(*id).is_some());
        let ammo = |id: Option<ItemId>| id.and_then(|id| Some((id, self.ammo_item(id)?)));
        let hand = interaction.equipment.item();
        let carried = interaction.held();
        let mut prompt = 0;
        let mut action = None;
        // RayStuff: a gun alone in the hands and the aimed loose ammo.
        let alone = match (hand, carried) {
            (Some(h), None) => gun(Some(h)),
            (None, Some(c)) => gun(Some(c)),
            _ => None,
        };
        if let Some(g) = alone
            && let Some((a, other)) = ammo(aimed)
        {
            if guns.can_reload(g, &other) {
                prompt |= ItemPrompt::RELOAD;
                if std::mem::take(&mut tap) {
                    action = Some((g, a, false));
                }
            }
            if guns.can_unload(g, &other) {
                prompt |= ItemPrompt::UNLOAD;
                if std::mem::take(&mut unload) {
                    action = Some((g, a, true));
                }
            }
        }
        // RayStuff: ammo in the hands and the aimed gun. R unloads the gun into the ammo.
        if let Some(g) = gun(aimed) {
            let source = ammo(hand.filter(|h| gun(Some(*h)).is_none()))
                .or_else(|| ammo(carried.filter(|c| gun(Some(*c)).is_none())));
            if prompt & ItemPrompt::RELOAD == 0
                && let Some((s, other)) = source
                && guns.unloads(g)
                && guns
                    .gun_ammo(g)
                    .is_some_and(|own| accepts(&other.ammo, &own.ammo, own.is_mag))
            {
                prompt |= ItemPrompt::RELOAD;
                if std::mem::take(&mut tap) {
                    action = Some((g, s, true));
                }
            }
            if hand.is_none()
                && let Some((s, other)) = ammo(carried)
                && guns.can_reload(g, &other)
            {
                prompt |= ItemPrompt::UNLOAD;
                if std::mem::take(&mut unload) {
                    action = Some((g, s, false));
                }
            }
        }
        // ReloadFromInventory: the carried ammo, else the first inventory ammo of the type.
        if prompt & ItemPrompt::RELOAD == 0
            && let Some(g) = gun(hand).or_else(|| hand.is_none().then(|| gun(carried)).flatten())
            && let Some(kind) = guns.runtime(g).map(|rt| rt.ammo.kind)
        {
            let fits = |a: &AmmoItem| a.ammo.kind == kind && a.ammo.count > 0.;
            let source = ammo(carried).filter(|(_, a)| fits(a)).or_else(|| {
                // Magazines in the inventory need DropFromInv, which this port does not have.
                self.area
                    .inventory
                    .slots
                    .into_iter()
                    .filter_map(ammo)
                    .find(|(_, a)| fits(a) && !a.is_mag)
            });
            if let Some((s, other)) = source
                && guns.can_reload(g, &other)
            {
                prompt |= ItemPrompt::RELOAD;
                if tap {
                    action = Some((g, s, false));
                }
            }
        }
        self.area.interaction.guns.prompt = prompt;
        if let Some((gun, source, unload)) = action {
            self.reload_gun(reference, gun, source, unload);
        }
    }
    /// The loose ammo or magazine of a world item or a spawned magazine.
    fn ammo_item(&self, id: ItemId) -> Option<AmmoItem> {
        self.stream
            .ammo_item(id)
            .or_else(|| self.area.spawned.ammo(id))
    }
    /// `weaponscript.Reload(source, unload)`. A used magazine leaves the player and the world.
    /// A magazine gun drops its old magazine at `magPos`.
    pub(crate) fn reload_gun(
        &mut self,
        reference: &ReferenceManifest,
        gun: ItemId,
        source: ItemId,
        unload: bool,
    ) {
        let Some(mut other) = self.ammo_item(source) else {
            return;
        };
        let interaction = &self.area.interaction;
        let held =
            interaction.held() == Some(source) || interaction.equipment.item() == Some(source);
        let drop = interaction
            .guns
            .mag_drop(gun, &self.area.items, &self.physics)
            .map(|(pose, prefab)| (pose, prefab.clone()));
        let position = self.item_position(gun);
        let result = self
            .area
            .interaction
            .guns
            .reload(gun, &mut other, unload, held, position);
        self.set_ammo_item(source, other);
        self.area.items.mark_dirty(gun);
        if let (true, Some((pose, _))) = (result.to_mag_pos, &drop) {
            self.move_item(source, *pose);
        }
        if let (Some(rounds), Some((pose, prefab))) = (result.dropped, &drop) {
            self.drop_magazine(reference, prefab, *pose, rounds);
        }
        if result.used {
            self.consume_magazine(source);
        }
    }
    fn set_ammo_item(&mut self, id: ItemId, ammo: AmmoItem) {
        if self.area.spawned.contains(id) {
            self.area.spawned.set_ammo(id, ammo);
            self.area.items.mark_dirty(id);
        } else {
            self.stream.set_ammo_item(id, ammo, &mut self.area.items);
        }
    }
    /// The magazine went into a gun: `Destroy`.
    fn consume_magazine(&mut self, source: ItemId) {
        let area = &mut self.area;
        if area.interaction.held() == Some(source) {
            area.interaction
                .drop_held(&mut area.items, &mut self.physics);
        }
        if area.interaction.equipment.item() == Some(source) {
            area.interaction
                .equipment
                .detach(&mut area.items, &mut self.physics);
        }
        area.inventory.forget(source);
        if area.spawned.contains(source) {
            super::spawned::despawn(area, &mut self.physics, source).expect("spawned magazine");
        } else {
            self.stream
                .remove_ammo_item(source, &mut area.items, &mut self.physics);
        }
    }
    /// `Instantiate(magPrefab, magPos)` with the gun rounds.
    fn drop_magazine(
        &mut self,
        reference: &ReferenceManifest,
        prefab: &crate::reference::SourceId,
        pose: rapier3d::math::Pose,
        rounds: f32,
    ) {
        let prototype = reference
            .item_prototypes
            .iter()
            .position(|p| &p.source == prefab)
            .expect("validated magazine prefab");
        let ids = super::spawned::spawn_prototype(
            reference,
            &mut self.area,
            &mut self.physics,
            prototype,
            crate::vehicle::from_rapier(&pose),
        )
        .expect("validated magazine prefab");
        for id in ids {
            if let Some(mut ammo) = self.area.spawned.ammo(id) {
                ammo.ammo.count = rounds;
                self.area.spawned.set_ammo(id, ammo);
            }
        }
        self.area.items.sync_poses(&self.physics);
    }
    /// Moves a world item body to `pose` at rest.
    fn move_item(&mut self, item: ItemId, pose: rapier3d::math::Pose) {
        let Some(body) = self
            .area
            .items
            .get(item)
            .filter(|s| s.location == crate::interaction::items::ItemLocation::World)
            .and_then(|s| s.body)
        else {
            return;
        };
        let local = to_rapier(self.area.items.get(item).unwrap().local_pose);
        self.physics.edit_body(body, |b| {
            b.set_position(pose * local.inverse(), true);
            b.set_linvel(rapier3d::math::Vector::ZERO, true);
            b.set_angvel(rapier3d::math::Vector::ZERO, true);
        });
        self.physics.refresh_queries();
        self.area.items.sync_poses(&self.physics);
    }
}
