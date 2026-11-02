//! Cigarette packs and smoking. Source `fpscontroller` smoke paths.
use super::GarageRuntime;
use crate::interaction::cigarettes::{
    BurnRates, CigaretteState, ExhaleRates, ExhaleState, PackState, part_pose,
};
use crate::interaction::feedback::ActionMask;
use crate::interaction::items::{ItemLocation, node_pose};
use crate::particles::ParticleState;
use crate::reference::{
    ItemId, NodeId, ParticleReference, ReferenceError, ReferenceManifest, Transform,
};
use crate::simulation::{FIXED_DT, FrameInput, keys, player_keys};
use crate::vehicle::{VehiclePose, from_rapier, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};

pub struct CigaretteRuntime {
    pub rates: BurnRates,
    pub exhale_rates: ExhaleRates,
    pub pack_prototype: usize,
    pub cigarette_prototype: usize,
    pub packs: Vec<(ItemId, PackState)>,
    pub cigarettes: Vec<(ItemId, CigaretteState)>,
    /// Wear slot of the cigarette (`wearableType` mask).
    pub mouth_slot: usize,
    pub exhale: ExhaleState,
    /// Mouth smoke emission rate. `None` disables the emitter.
    pub mouth_smoke: Option<f32>,
    /// Ember particles of each cigarette and the player mouth smoke.
    embers: Vec<(ItemId, ParticleState)>,
    exhale_particles: ParticleState,
    exhale_node: NodeId,
    exhale_distance_rate: f32,
    ember_particle: ParticleReference,
    /// Item root in the body frame.
    root_in_body: Pose,
    /// Ember particle part at its start and end pose, in the root frame (`UpdParts`).
    ember_start: Pose,
    ember_end: Pose,
    ash: Option<Ash>,
    show_stats: bool,
}

/// Source `ediblescript.hasParticle` of the cigarette. `particlePos` is a child of one part.
struct Ash {
    kind: usize,
    range: [i32; 2],
    /// The part at its start and end pose, in the root frame (`UpdParts`).
    start: Transform,
    end: Transform,
    /// `particlePos` in the part frame.
    local: Transform,
}

impl CigaretteRuntime {
    /// `mainscript.FoodParticle` of one cigarette `Eat` at `particlePos`. `hp` is the value before
    /// the bite: `UpdParts` runs after the particle.
    fn ash(
        &self,
        item: VehiclePose,
        hp: f32,
        origin: &crate::world::WorldOrigin,
        food: &mut crate::food_particles::FoodParticles,
    ) {
        let Some(a) = &self.ash else {
            return;
        };
        let part = part_pose(&a.start, &a.end, hp);
        let rotation = Rotation::from_array(part.rotation);
        let local = Vector::from_array(part.scale) * Vector::from_array(a.local.translation);
        let in_root = Pose::from_parts(
            Vector::from_array(part.translation) + rotation * local,
            rotation * Rotation::from_array(a.local.rotation),
        );
        let pose = to_rapier(item) * self.root_in_body * in_root;
        food.bite(
            a.kind,
            a.range,
            origin.absolute(pose.translation.to_array()),
            pose.rotation * Vector::NEG_Z,
        );
    }
}

impl CigaretteRuntime {
    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let a = &reference.acquisition;
        let prototype = |item: u32| {
            reference
                .item_prototypes
                .iter()
                .position(|p| p.item_id == Some(item))
                .ok_or_else(|| ReferenceError(format!("Missing item prototype {item}")))
        };
        let pack_prototype = prototype(a.pack_item_id)?;
        let cigarette_prototype = prototype(a.cigarette_item_id)?;
        let record = &reference.item_prototypes[cigarette_prototype];
        let consumable = record
            .consumable
            .as_ref()
            .ok_or_else(|| ReferenceError("Missing cigarette consumable".into()))?;
        let wearable_type = a
            .packs
            .first()
            .ok_or_else(|| ReferenceError("Missing cigarette pack rules".into()))?
            .wearable_type;
        let root = to_rapier(node_pose(reference, record.root)?);
        let body = &record.bodies[0];
        let body_local = to_rapier(VehiclePose {
            position: body.pose.translation,
            rotation: body.pose.rotation,
        });
        let ember = &consumable.ember;
        let part = consumable
            .parts
            .iter()
            .position(|n| *n == ember.particle_node)
            .ok_or_else(|| ReferenceError("Cigarette ember particle is no part".into()))?;
        let in_root = |node| -> Result<Pose, ReferenceError> {
            Ok(root.inverse() * to_rapier(node_pose(reference, node)?))
        };
        let node = |id: NodeId| {
            reference
                .nodes
                .iter()
                .find(|n| n.id == id)
                .ok_or_else(|| ReferenceError(format!("Missing node {}", id.0)))
        };
        let ash = if consumable.has_particle {
            let position = node(consumable.particle_position)?;
            let part = consumable
                .parts
                .iter()
                .position(|n| Some(*n) == position.parent)
                .ok_or_else(|| {
                    ReferenceError("Cigarette particle position is no part child".into())
                })?;
            let in_root = |id: NodeId| -> Result<Transform, ReferenceError> {
                let n = node(id)?;
                if n.parent != Some(record.root) {
                    return Err(ReferenceError("Cigarette part is no root child".into()));
                }
                Ok(n.transform)
            };
            in_root(consumable.parts[part])?;
            Some(Ash {
                kind: consumable.particle_type as usize,
                range: [consumable.min_particle, consumable.max_particle],
                start: in_root(consumable.start[part])?,
                end: in_root(consumable.end[part])?,
                local: position.transform,
            })
        } else {
            None
        };
        let exhale = &reference.small_items.exhale;
        Ok(Self {
            embers: Vec::new(),
            exhale_particles: ParticleState::new(
                &exhale.particle,
                exhale.particle.source.path_id as u64,
            ),
            exhale_node: exhale.node,
            exhale_distance_rate: exhale.particle.rate_over_distance,
            ember_particle: ember.particle.clone(),
            root_in_body: body_local.inverse(),
            ember_start: in_root(consumable.start[part])?,
            ember_end: in_root(consumable.end[part])?,
            ash,
            rates: BurnRates::from(consumable),
            exhale_rates: ExhaleRates::from(&reference.small_items.exhale),
            pack_prototype,
            cigarette_prototype,
            packs: Vec::new(),
            cigarettes: Vec::new(),
            mouth_slot: wearable_type as usize,
            exhale: ExhaleState::default(),
            mouth_smoke: None,
            show_stats: !consumable.dont_show_stats,
        })
    }
    pub fn embers(&self) -> impl Iterator<Item = (ItemId, &[crate::particles::ParticleOutput])> {
        self.embers.iter().map(|(id, p)| (*id, p.particles()))
    }
    pub fn exhale_particles(&self) -> &[crate::particles::ParticleOutput] {
        self.exhale_particles.particles()
    }
    pub fn pack(&self, id: ItemId) -> Option<&PackState> {
        self.packs.iter().find(|(i, _)| *i == id).map(|(_, p)| p)
    }
    pub fn cigarette(&self, id: ItemId) -> Option<&CigaretteState> {
        self.cigarettes
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, c)| c)
    }
    fn cigarette_mut(&mut self, id: ItemId) -> Option<&mut CigaretteState> {
        self.cigarettes
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, c)| c)
    }
    /// HUD actions for the held item.
    pub fn actions(&self, held: Option<ItemId>) -> u32 {
        let Some(held) = held else { return 0 };
        let mut mask = 0;
        if self.pack(held).is_some_and(PackState::can_take) {
            mask |= ActionMask::TAKE_ONE;
        }
        if self.cigarette(held).is_some() {
            mask |= ActionMask::SMOKE;
        }
        mask
    }
}

fn strip(input: &mut FrameInput, bits: u32) {
    input.held &= !bits;
    input.pressed &= !bits;
    input.released &= !bits;
}

impl GarageRuntime {
    /// Source `pickedUp`, or the right-hand item when one is held there.
    fn smoke_held(&self) -> Option<ItemId> {
        self.area
            .interaction
            .equipment
            .item()
            .or(self.area.interaction.held())
    }

    pub(crate) fn spawn_cigarette(
        &mut self,
        reference: &ReferenceManifest,
        root: VehiclePose,
    ) -> Result<ItemId, ReferenceError> {
        let prototype = self.area.cigarettes.cigarette_prototype;
        let id = super::spawned::spawn_prototype(
            reference,
            &mut self.area,
            &mut self.physics,
            prototype,
            root,
        )?[0];
        let state = CigaretteState::new(&self.area.cigarettes.rates);
        self.area.cigarettes.cigarettes.push((id, state));
        Ok(id)
    }

    pub fn spawn_pack(
        &mut self,
        reference: &ReferenceManifest,
        root: VehiclePose,
        count: u32,
    ) -> Result<ItemId, ReferenceError> {
        let prototype = self.area.cigarettes.pack_prototype;
        let id = super::spawned::spawn_prototype(
            reference,
            &mut self.area,
            &mut self.physics,
            prototype,
            root,
        )?[0];
        self.area.cigarettes.packs.push((id, PackState { count }));
        Ok(id)
    }

    /// The cigarette in the mouth slot.
    fn mouth_cigarette(&self) -> Option<ItemId> {
        let c = &self.area.cigarettes;
        self.area
            .wear
            .item(c.mouth_slot)
            .filter(|id| c.cigarette(*id).is_some())
    }

    /// Pack and smoke input. Consumed keys leave `input`.
    pub(super) fn step_cigarettes(
        &mut self,
        reference: &ReferenceManifest,
        input: &mut FrameInput,
    ) {
        let dt = FIXED_DT;
        let held = self.smoke_held();
        let mut smoking = false;
        // Worn cigarette: Caps Lock.
        if input.player_held & player_keys::SMOKE != 0
            && let Some(id) = self.mouth_cigarette()
        {
            let rates = self.area.cigarettes.rates;
            let exhale = self.area.cigarettes.exhale_rates;
            let c = &mut self.area.cigarettes;
            let state = c
                .cigarettes
                .iter_mut()
                .find(|(i, _)| *i == id)
                .map(|(_, s)| s);
            if let Some(state) = state {
                let hp = state.hp;
                if hp > 0. {
                    c.exhale.charge(&exhale, dt);
                }
                let effect = state.eat(&rates, dt, false);
                if hp > 0.
                    && let Some(item) = self.area.items.get(id)
                {
                    let c = &self.area.cigarettes;
                    c.ash(item.pose, hp, &self.stream.origin, &mut self.food_particles);
                }
                self.survival.queue(effect);
                if self.area.cigarettes.show_stats {
                    self.survival.show_stats();
                }
                smoking = true;
            }
        }
        if let Some(id) = held {
            // Held pack: E takes one.
            if self
                .area
                .cigarettes
                .pack(id)
                .is_some_and(PackState::can_take)
                && input.pressed & keys::USE != 0
            {
                strip(input, keys::USE);
                self.take_one(reference, id);
            }
            // Held cigarette: LMB.
            if self.area.cigarettes.cigarette(id).is_some() {
                if input.held & keys::PRIMARY != 0 {
                    let rates = self.area.cigarettes.rates;
                    let exhale = self.area.cigarettes.exhale_rates;
                    let state = self.area.cigarettes.cigarette_mut(id).unwrap();
                    let hp = state.hp;
                    let effect = state.eat(&rates, dt, false);
                    let lit = state.hp > 0.;
                    if hp > 0.
                        && let Some(item) = self.area.items.get(id)
                    {
                        let c = &self.area.cigarettes;
                        c.ash(item.pose, hp, &self.stream.origin, &mut self.food_particles);
                    }
                    self.survival.queue(effect);
                    if self.area.cigarettes.show_stats {
                        self.survival.show_stats();
                    }
                    if lit {
                        smoking = true;
                        self.area.cigarettes.exhale.charge(&exhale, dt);
                    }
                }
                strip(input, keys::PRIMARY);
            }
        }
        let blocked = smoking
            || input.player_held & player_keys::SMOKE != 0
            || input.held & keys::PRIMARY != 0;
        let exhale = self.area.cigarettes.exhale_rates;
        self.area.cigarettes.mouth_smoke = self.area.cigarettes.exhale.emit(&exhale, blocked, dt);
    }

    fn take_one(&mut self, reference: &ReferenceManifest, pack: ItemId) {
        let Some(state) = self.area.items.get(pack) else {
            return;
        };
        let Some(spawned) = self.area.spawned.get(pack).copied() else {
            return;
        };
        let record = &reference.item_prototypes[spawned.prototype];
        let body = &record.bodies[0];
        // The cigarette prefab spawns at the pack pickupable transform.
        let pickup = (|| -> Result<Pose, ReferenceError> {
            let root = to_rapier(node_pose(reference, record.root)?);
            let node = to_rapier(node_pose(reference, record.pickups[spawned.pickup].node)?);
            Ok(to_rapier(state.pose)
                * to_rapier(VehiclePose {
                    position: body.pose.translation,
                    rotation: body.pose.rotation,
                })
                .inverse()
                * root.inverse()
                * node)
        })();
        let Ok(pickup) = pickup else {
            self.notice = "Cannot take a cigarette.";
            return;
        };
        let p = self
            .area
            .cigarettes
            .packs
            .iter_mut()
            .find(|(i, _)| *i == pack)
            .unwrap();
        if !p.1.take_one() {
            return;
        }
        match self.spawn_cigarette(reference, from_rapier(&pickup)) {
            Ok(id) => {
                self.area.put_on(&mut self.physics, id);
            }
            Err(_) => self.notice = "Cannot take a cigarette.",
        }
    }

    /// Automatic burn and ember decay of every cigarette, then the ember and mouth smoke.
    pub(super) fn advance_cigarettes(&mut self) {
        let rates = self.area.cigarettes.rates;
        let c = &mut self.area.cigarettes;
        for i in 0..c.cigarettes.len() {
            let (id, state) = &mut c.cigarettes[i];
            let (id, hp) = (*id, state.hp);
            if state.update(&rates, FIXED_DT)
                && let Some(item) = self.area.items.get(id)
            {
                c.ash(item.pose, hp, &self.stream.origin, &mut self.food_particles);
            }
        }
        let origin = self.stream.origin;
        let c = &mut self.area.cigarettes;
        let cigarettes = &c.cigarettes;
        c.embers
            .retain(|(id, _)| cigarettes.iter().any(|(i, _)| i == id));
        for (id, state) in &c.cigarettes {
            let Some(item) = self.area.items.get(*id) else {
                continue;
            };
            let index = match c.embers.iter().position(|(i, _)| i == id) {
                Some(index) => index,
                None => {
                    let seed = c.ember_particle.source.path_id as u64 ^ u64::from(id.0);
                    c.embers
                        .push((*id, ParticleState::new(&c.ember_particle, seed)));
                    c.embers.len() - 1
                }
            };
            let out = state.ember.output(&rates.ember);
            // UpdParts: the part lerps from end to start by hp.
            let hp = state.part_blend();
            let part = c.ember_end.translation.lerp(c.ember_start.translation, hp);
            let local = rapier3d::math::Vector::from_array(c.ember_particle.local_pose.translation);
            let root = to_rapier(item.pose) * c.root_in_body;
            let emitter = root.translation + root.rotation * (part - local);
            let shown = !matches!(item.location, ItemLocation::Stored(_));
            let p = &mut c.embers[index].1;
            p.rotation = root.rotation;
            p.set_origin(origin.offset);
            p.set_rates(out.particle_rate, c.ember_particle.rate_over_distance);
            p.advance(
                FIXED_DT,
                origin.absolute(emitter.to_array()),
                if shown { 1. } else { 0. },
                &self.physics,
            );
        }
        // fpscontroller smokeParticle: the emission is off while `mouth_smoke` is None.
        if let Some(pose) = self.area.pose.anchor(c.exhale_node) {
            let p = &mut c.exhale_particles;
            p.rotation = rapier3d::math::Rotation::from_array(pose.rotation);
            p.set_origin(origin.offset);
            p.set_rates(c.mouth_smoke.unwrap_or(0.), c.exhale_distance_rate);
            p.advance(
                FIXED_DT,
                origin.absolute(pose.position),
                if c.mouth_smoke.is_some() { 1. } else { 0. },
                &self.physics,
            );
        }
    }
}

/// New game only: the 3 loose StartSpawnItems packs and the selected start car pack,
/// as physics-space root poses and counts.
pub(super) fn start_pack_poses(
    reference: &ReferenceManifest,
    start_car: crate::world::StartCar,
    startup: &crate::world::StartupTerrain,
    origin: [f64; 3],
) -> Result<Vec<(VehiclePose, u32)>, ReferenceError> {
    let local = |pose: VehiclePose| VehiclePose {
        position: std::array::from_fn(|i| (pose.position[i] as f64 - origin[i]) as f32),
        rotation: pose.rotation,
    };
    let mut packs = Vec::new();
    for v in &reference.small_items.start_packs {
        let pose = crate::world::source_world_pose(&reference.world, startup, &v.transform, &[])?;
        packs.push((local(pose), v.ammo.count));
    }
    // The export keeps the packs of the start car groups. Only the picked start car keeps its pack.
    for v in reference
        .acquisition
        .startup_packs
        .iter()
        .filter(|v| v.start_car() == start_car.field())
    {
        let parent = v
            .group_pose
            .parent
            .as_ref()
            .ok_or_else(|| ReferenceError("Start car pack group parent".into()))?;
        let pose = crate::world::source_world_pose(
            &reference.world,
            startup,
            parent,
            &[v.group_pose.transform, v.pose.transform],
        )?;
        packs.push((local(pose), v.ammo.count));
    }
    Ok(packs)
}

impl CigaretteRuntime {
    pub fn capture_save(
        &self,
        spawned: &super::spawned::SpawnedItems,
    ) -> crate::save::CigarettesSnapshot {
        use crate::save::{CigaretteSnapshot, PackSnapshot};
        let serial = |id: ItemId| spawned.get(id).map(|s| s.serial);
        let mut packs: Vec<_> = self
            .packs
            .iter()
            .filter_map(|(id, p)| {
                Some(PackSnapshot {
                    serial: serial(*id)?,
                    count: p.count,
                })
            })
            .collect();
        packs.sort_by_key(|p| p.serial);
        let mut cigarettes: Vec<_> = self
            .cigarettes
            .iter()
            .filter_map(|(id, c)| {
                Some(CigaretteSnapshot {
                    serial: serial(*id)?,
                    hp: c.hp,
                    ate_once: c.ate_once,
                })
            })
            .collect();
        cigarettes.sort_by_key(|c| c.serial);
        crate::save::CigarettesSnapshot { packs, cigarettes }
    }
}

/// Saved records name spawned items of the matching prototype.
pub fn validate_save(
    reference: &ReferenceManifest,
    spawned: &[crate::save::SpawnedItemSnapshot],
    saved: &crate::save::CigarettesSnapshot,
) -> Result<(), crate::save::SaveError> {
    let invalid = |detail: &str| crate::save::SaveError::InvalidLinks(detail.into());
    let of = |serial: u32, item: u32| {
        spawned.iter().any(|s| {
            s.serial == serial
                && reference.item_prototypes.iter().any(|p| {
                    p.item_id == Some(item)
                        && crate::save::SourceKey::from(&p.source) == s.prototype
                })
        })
    };
    let a = &reference.acquisition;
    let mut seen = std::collections::HashSet::new();
    for p in &saved.packs {
        if !of(p.serial, a.pack_item_id) || !seen.insert(p.serial) {
            return Err(invalid("Pack save identity"));
        }
    }
    for c in &saved.cigarettes {
        if !of(c.serial, a.cigarette_item_id) || !seen.insert(c.serial) {
            return Err(invalid("Cigarette save identity"));
        }
        if !c.hp.is_finite() || !(0. ..=1.).contains(&c.hp) {
            return Err(crate::save::SaveError::CorruptData("Cigarette hp".into()));
        }
    }
    Ok(())
}

impl GarageRuntime {
    /// Rebuild pack and cigarette state after the spawned items exist.
    pub(super) fn restore_cigarettes(&mut self, saved: &crate::save::CigarettesSnapshot) {
        let item = |serial: u32, prototype: usize| {
            self.area
                .spawned
                .iter()
                .find(|s| s.serial == serial && s.prototype == prototype)
                .map(|s| s.id)
        };
        let c = &self.area.cigarettes;
        let packs: Vec<_> = saved
            .packs
            .iter()
            .filter_map(|p| {
                Some((
                    item(p.serial, c.pack_prototype)?,
                    PackState { count: p.count },
                ))
            })
            .collect();
        let cigarettes: Vec<_> = saved
            .cigarettes
            .iter()
            .filter_map(|s| {
                Some((
                    item(s.serial, c.cigarette_prototype)?,
                    CigaretteState::restored(&c.rates, s.hp, s.ate_once, FIXED_DT),
                ))
            })
            .collect();
        let c = &mut self.area.cigarettes;
        c.packs = packs;
        c.cigarettes = cigarettes;
    }
}
