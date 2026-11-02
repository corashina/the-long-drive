//! Source spray bottles (spricniscript), genie lamps (ginlampscript) and binoculars (tavcsoscript) on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::conditions::ConditionStore;
use crate::interaction::InteractionState;
use crate::interaction::cleaning::{
    CleaningTools, Target, ToolFrame, ToolOwner, ToolWorld, WashOwner,
};
use crate::interaction::genie_lamp::GenieLamp;
use crate::interaction::items::ItemStore;
use crate::interaction::spray::{Ammo, SprayTrigger};
use crate::particles::ParticleState;
use crate::reference::{
    ColliderId, ItemId, ReferenceError, ReferenceManifest, SourceId, Transform,
    WorldPrototypeReference,
};
use crate::vehicle::{PhysicsWorld, to_rapier};
use crate::world::spawn_rules::*;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, QueryFilter, Ray};

pub struct SprayBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    item: ItemId,
    /// The trigger node local pose.
    pose: Transform,
    /// The hole relative to the item pose.
    hole: Pose,
    trigger: SprayTrigger,
    ammo: Ammo,
    initial: f32,
    /// Pressing this frame: the particle direction and the selected target with its hit.
    direction: Option<Vector>,
    target: Option<(Target, Vector, Vector)>,
}
struct LampBinding {
    parent: GeneratedId,
    slot: usize,
    item: ItemId,
    light: GeneratedId,
    lamp: GenieLamp,
    initial: f32,
    /// The last written `_EmissionColor`.
    emission: [f32; 4],
}
/// One spray or splash particle system, restarted by each shot.
struct Effect {
    particle: ParticleState,
    duration: f32,
    elapsed: Option<f32>,
    emitter: [f64; 3],
    rotation: Rotation,
}
pub struct SmallItems {
    sprays: Vec<SprayBinding>,
    lamps: Vec<LampBinding>,
    /// Binoculars bound in `InteractionState::binoculars`.
    binoculars: Vec<(GeneratedId, ItemId)>,
    pub(super) ammo: Vec<super::content_ammo::AmmoBinding>,
    /// Prototypes checked for gun ammo.
    ammo_prototypes: std::collections::HashMap<SourceId, bool>,
    pub(super) explosives: Vec<super::content_explosives::ExplosiveBinding>,
    /// Prototypes checked for an explosivescript.
    pub(super) explosive_prototypes: std::collections::HashMap<SourceId, bool>,
    scan: Option<(u64, u64)>,
    effects: [Effect; 2],
    /// The bottle whose spray follows its hole.
    firing: Option<ItemId>,
    clips: [Option<u32>; 2],
    time: f64,
    /// PlayClipAtPoint requests: clip, volume, absolute position.
    pub sounds: Vec<(u32, f32, [f64; 3])>,
    /// fpscontroller.lastReloadTime.
    last_reload: f64,
    /// `ItemPrompt` bits of the bottle refill.
    refill_prompt: u8,
}
/// Player input of the bottle refill (fpscontroller RayStuff).
#[derive(Clone, Copy, Debug, Default)]
pub struct RefillInput {
    /// pickedUp: the carried item.
    pub carried: Option<ItemId>,
    /// inHandP: the right-hand item.
    pub hand: Option<ItemId>,
    /// The aimed item.
    pub aimed: Option<ItemId>,
    pub use_held: bool,
    pub use_released: bool,
    pub reload_held: bool,
    pub reload_released: bool,
}
impl SmallItems {
    pub fn new(reference: &ReferenceManifest) -> Self {
        let s = &reference.small_items.spray_bottle;
        let effect = |p: &crate::reference::ParticleReference, seed: u64| Effect {
            particle: ParticleState::new(p, seed),
            duration: p.duration,
            elapsed: None,
            emitter: [0.; 3],
            rotation: Rotation::IDENTITY,
        };
        Self {
            sprays: Vec::new(),
            lamps: Vec::new(),
            binoculars: Vec::new(),
            ammo: Vec::new(),
            ammo_prototypes: std::collections::HashMap::new(),
            explosives: Vec::new(),
            explosive_prototypes: std::collections::HashMap::new(),
            scan: None,
            effects: [
                effect(&s.spray, s.source.path_id as u64),
                effect(&s.splash, s.source.path_id as u64 + 1),
            ],
            firing: None,
            clips: [
                reference.clip_id(&s.set_clip),
                reference.clip_id(&s.reset_clip),
            ],
            time: 0.,
            sounds: Vec::with_capacity(8),
            last_reload: f64::NEG_INFINITY,
            refill_prompt: 0,
        }
    }
    pub fn heap_bytes(&self) -> usize {
        self.sprays.capacity() * std::mem::size_of::<SprayBinding>()
            + self.lamps.capacity() * std::mem::size_of::<LampBinding>()
            + self.binoculars.capacity() * std::mem::size_of::<(GeneratedId, ItemId)>()
            + self.ammo.capacity() * std::mem::size_of::<super::content_ammo::AmmoBinding>()
            + self.explosives.capacity()
                * std::mem::size_of::<super::content_explosives::ExplosiveBinding>()
            + self.sounds.capacity() * std::mem::size_of::<(u32, f32, [f64; 3])>()
    }
    /// Spray (0) and splash (1) particles.
    pub fn particles(&self, index: usize) -> &[crate::particles::ParticleOutput] {
        self.effects[index].particle.particles()
    }
}
pub(super) fn node_index(
    prototype: &WorldPrototypeReference,
    source: &SourceId,
) -> Result<usize, ReferenceError> {
    prototype
        .nodes
        .iter()
        .position(|n| &n.source == source)
        .ok_or_else(|| ReferenceError("Small item node outside its prototype".into()))
}
pub(super) fn relative(from: crate::world::AbsolutePose, to: crate::world::AbsolutePose) -> Pose {
    let r = Rotation::from_array(from.rotation).inverse();
    let d = Vector::from_array(std::array::from_fn(|i| {
        (to.position[i] - from.position[i]) as f32
    }));
    Pose::from_parts(r * d, r * Rotation::from_array(to.rotation))
}

impl WorldStream {
    pub(super) fn sync_small_items(
        &mut self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.small_items.scan == Some(revision) {
            return Ok(());
        }
        let active = |parent: &GeneratedId, item: ItemId| {
            self.contents
                .active
                .iter()
                .position(|c| &c.content.placement.id == parent && c.item == Some(item))
        };
        let mut i = 0;
        while i < self.small_items.sprays.len() {
            let b = &self.small_items.sprays[i];
            if let Some(slot) = active(&b.parent, b.item) {
                self.small_items.sprays[i].slot = slot;
                i += 1;
            } else {
                interaction.cleaning.reserve_lmb(b.item, None);
                self.small_items.sprays.swap_remove(i);
            }
        }
        let mut i = 0;
        while i < self.small_items.lamps.len() {
            let b = &self.small_items.lamps[i];
            if let Some(slot) = active(&b.parent, b.item) {
                self.small_items.lamps[i].slot = slot;
                i += 1;
            } else {
                interaction
                    .cleaning
                    .remove_generated_wash(WashOwner::Item(b.item));
                self.small_items.lamps.swap_remove(i);
            }
        }
        let contents = &self.contents.active;
        self.small_items.binoculars.retain(|(parent, item)| {
            let active = contents
                .iter()
                .any(|c| &c.content.placement.id == parent && c.item == Some(*item));
            if !active {
                interaction.binoculars.unbind(*item);
            }
            active
        });
        let small = &mut self.small_items;
        small.ammo.retain(|b| {
            self.contents
                .active
                .iter()
                .any(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
        });
        let spray = &reference.small_items.spray_bottle.prototype;
        let lamp = &reference.small_items.genie_lamp.prototype;
        let binoculars = &reference.small_items.binoculars.prototype;
        let mut complete = true;
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            let prototype = &content.content.placement.prototype;
            let ammo = if prototype == spray || prototype == lamp || prototype == binoculars {
                false
            } else if let Some(&ammo) = self.small_items.ammo_prototypes.get(prototype) {
                ammo
            } else {
                let ammo = self.ammo_prototype(reference, prototype)?;
                let prototype = prototype.clone();
                self.small_items.ammo_prototypes.insert(prototype, ammo);
                ammo
            };
            let content = &self.contents.active[slot];
            let prototype = &content.content.placement.prototype;
            if (prototype != spray && prototype != lamp && prototype != binoculars && !ammo)
                || self.small_items.sprays.iter().any(|b| b.item == item)
                || self.small_items.lamps.iter().any(|b| b.item == item)
                || self.small_items.binoculars.iter().any(|b| b.1 == item)
                || self.small_items.ammo.iter().any(|b| b.item == item)
            {
                continue;
            }
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            if ammo {
                let binding = self.bind_ammo(slot, item)?;
                self.small_items.ammo.push(binding);
            } else if prototype == binoculars {
                let camera = self.binocular_camera(reference, slot)?;
                interaction.binoculars.bind(item, camera);
                let parent = self.contents.active[slot].content.placement.id.clone();
                self.small_items.binoculars.push((parent, item));
            } else if prototype == spray {
                let binding = self.bind_spray(reference, slot, item)?;
                interaction
                    .cleaning
                    .reserve_lmb(item, Some(reference.small_items.spray_bottle.text));
                self.small_items.sprays.push(binding);
            } else {
                let binding = self.bind_lamp(reference, interaction, physics, slot, item)?;
                self.small_items.lamps.push(binding);
                self.presentation_changes.push(ContentSlot::Active(slot));
            }
        }
        self.sync_explosives()?;
        self.small_items.scan = complete.then_some(revision);
        Ok(())
    }
    fn small_item_prototype(
        &self,
        slot: usize,
    ) -> Result<&WorldPrototypeReference, ReferenceError> {
        let placement = &self.contents.active[slot].content.placement;
        self.reference
            .prototypes
            .iter()
            .find(|p| p.source == placement.prototype)
            .ok_or_else(|| ReferenceError("Small item prototype missing".into()))
    }
    /// `CamR` relative to the item pose, which is the pickup node pose.
    fn binocular_camera(
        &self,
        reference: &ReferenceManifest,
        slot: usize,
    ) -> Result<Pose, ReferenceError> {
        let placement = &self.contents.active[slot].content.placement;
        let prototype = self.small_item_prototype(slot)?;
        let rules = &self.contents.rules;
        let pickup = rules
            .component(&self.reference, &placement.prototype, "pickupable")?
            .ok_or_else(|| ReferenceError("Binoculars have no pickup".into()))?;
        let root = node_index(prototype, rules.root(&self.reference, pickup.source)?)?;
        let place = |index| {
            crate::world::placement::place_pose(
                placement.pose,
                placement.scale,
                crate::world::placement::node_pose(prototype, index),
            )
        };
        let camera = node_index(prototype, &reference.small_items.binoculars.camera)?;
        Ok(relative(place(root), place(camera)))
    }
    fn saved(&self, parent: &GeneratedId) -> Option<&crate::world::ObjectSnapshot> {
        self.changes
            .get(ObjectKey::Generated(parent.clone()))
            .and_then(|c| c.snapshot.as_ref())
    }
    fn bind_spray(
        &self,
        reference: &ReferenceManifest,
        slot: usize,
        item: ItemId,
    ) -> Result<SprayBinding, ReferenceError> {
        let r = &reference.small_items.spray_bottle;
        let content = &self.contents.active[slot];
        let placement = &content.content.placement;
        let prototype = self.small_item_prototype(slot)?;
        let rules = &self.contents.rules;
        let pickup = rules
            .component(&self.reference, &placement.prototype, "pickupable")?
            .ok_or_else(|| ReferenceError("Spray bottle has no pickup".into()))?;
        let root = node_index(prototype, rules.root(&self.reference, pickup.source)?)?;
        let place = |index| {
            crate::world::placement::place_pose(
                placement.pose,
                placement.scale,
                crate::world::placement::node_pose(prototype, index),
            )
        };
        let node = node_index(prototype, &r.trigger)?;
        let ammo = Ammo::new(&r.ammo);
        let initial = content.content.ammo.unwrap_or(ammo.count);
        let ammo = ammo
            .with_count(initial)
            .ok_or_else(|| ReferenceError("Invalid spawned spray bottle ammo".into()))?;
        let saved = self
            .saved(&placement.id)
            .and_then(|s| s.ammo)
            .and_then(|count| ammo.with_count(count));
        Ok(SprayBinding {
            parent: placement.id.clone(),
            slot,
            item,
            pose: prototype.nodes[node].transform,
            hole: relative(place(root), place(node_index(prototype, &r.hole)?)),
            trigger: SprayTrigger::default(),
            ammo: saved.unwrap_or(ammo),
            initial,
            direction: None,
            target: None,
        })
    }
    fn bind_lamp(
        &self,
        reference: &ReferenceManifest,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        item: ItemId,
    ) -> Result<LampBinding, ReferenceError> {
        let r = &reference.small_items.genie_lamp;
        let parent = self.contents.active[slot].content.placement.id.clone();
        let prototype = self.small_item_prototype(slot)?;
        let rules = &self.contents.rules;
        let wash = rules.get(&self.reference, &r.wash_trigger)?.fields;
        let permission = integer(wash, "CleanProperty")?;
        if !(0..reference.conditions.clean_tables.len() as i32).contains(&permission) {
            return Err(ReferenceError(
                "Genie lamp wash property outside the clean table".into(),
            ));
        }
        let node = node_index(prototype, rules.root(&self.reference, &r.wash_trigger)?)?;
        // GetComponentInParent: colliders on the trigger node and below it.
        let below = |mut i: usize| loop {
            if i == node {
                return true;
            }
            match prototype.nodes[i].parent {
                Some(p) => i = p as usize,
                None => return false,
            }
        };
        let mut colliders = Vec::new();
        for (source, handle) in self
            .structures
            .iter()
            .filter(|s| s.placement.id == parent)
            .flat_map(|s| &s.colliders)
        {
            let on_trigger = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .is_some_and(|c| below(c.node as usize));
            if !on_trigger {
                continue;
            }
            let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            colliders.push((id, *handle));
        }
        interaction.cleaning.register_generated_wash(
            WashOwner::Item(item),
            permission as u8,
            flag(wash, "neverRepair")?,
            colliders,
        );
        let value = self
            .saved(&parent)
            .and_then(|s| s.glow)
            .filter(|v| (0. ..=1.).contains(v))
            .unwrap_or(r.startup_value);
        let mut light = parent.clone();
        light.children.push((r.light.clone(), 0));
        Ok(LampBinding {
            parent,
            slot,
            item,
            light,
            lamp: GenieLamp::new(value),
            initial: r.startup_value,
            emission: crate::interaction::genie_lamp::output(r, value).emission,
        })
    }
    /// spricniscript.Pressing for a held bottle. Returns false when the item is no bottle.
    pub fn press_spray(
        &mut self,
        owner: ToolOwner,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        cleaning: &CleaningTools,
        world: ToolWorld<'_>,
    ) -> bool {
        let (item, equipped) = match owner {
            ToolOwner::Equipped(item) => (item, true),
            ToolOwner::Carried(item) => (item, false),
        };
        let ToolWorld { items, physics, .. } = world;
        let Some(b) = self.small_items.sprays.iter_mut().find(|b| b.item == item) else {
            return false;
        };
        let Some(state) = items.get(item) else {
            return true;
        };
        if !frame.lmb || !frame.dt.is_finite() || frame.dt <= 0. {
            return true;
        }
        let r = &reference.small_items.spray_bottle;
        b.trigger.press(r, frame.dt);
        let hole = to_rapier(state.pose) * b.hole;
        let start = if equipped {
            to_rapier(frame.view)
        } else {
            hole
        };
        let mut direction = start.rotation * -Vector::Z;
        let ray = Ray::new(start.translation, direction);
        let own = state.body;
        let filter = |_: ColliderHandle, c: &rapier3d::prelude::Collider| {
            c.is_enabled()
                && (own.is_none() || c.parent() != own)
                && c.collision_groups().memberships.bits() & r.mask != 0
                && !c.shape().contains_point(c.position(), ray.origin)
        };
        b.target = None;
        if let Some((handle, hit)) = physics
            .queries(QueryFilter::default().predicate(&filter))
            .cast_ray_and_get_normal(&ray, r.distance, true)
        {
            let point = ray.point_at(hit.time_of_impact);
            direction = point - hole.translation;
            b.target = cleaning
                .spray_target((handle, point), reference, world)
                .map(|t| (t, point, hit.normal));
        }
        b.direction = Some(direction);
        true
    }
    /// spricniscript.Update and Fire, then ginlampscript.Update.
    pub fn advance_sprays(
        &mut self,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        cleaning: &mut CleaningTools,
        conditions: &mut ConditionStore,
        items: &mut ItemStore,
        physics: &PhysicsWorld,
    ) {
        let dt = frame.dt;
        if !frame.active || !dt.is_finite() || dt <= 0. {
            return;
        }
        let r = &reference.small_items.spray_bottle;
        let origin = self.origin;
        let small = &mut self.small_items;
        for b in &mut small.sprays {
            let Some(state) = items.get(b.item) else {
                continue;
            };
            let root = to_rapier(state.pose);
            let hole = root * b.hole;
            let position = origin.absolute(root.translation.to_array());
            let events = b.trigger.update(r, dt);
            if events.fire && b.ammo.count > 0. {
                let direction = b.direction.unwrap_or(hole.rotation * -Vector::Z);
                let e = &mut small.effects[0];
                e.particle.restart();
                e.elapsed = Some(0.);
                e.rotation = Rotation::from_rotation_arc(-Vector::Z, direction.normalize_or_zero());
                small.firing = Some(b.item);
                if let Some(clip) = small.clips[0] {
                    small.sounds.push((clip, r.set_volume, position));
                }
                if let Some((target, point, normal)) = b.target
                    && cleaning.spray(target, r.cleaning_type, r.gain, reference, conditions)
                {
                    let e = &mut small.effects[1];
                    e.particle.restart();
                    e.elapsed = Some(0.);
                    e.emitter = origin.absolute(point.to_array());
                    e.rotation =
                        Rotation::from_rotation_arc(-Vector::Z, normal.normalize_or_zero());
                }
                b.ammo.decrease(1.);
                items.mark_dirty(b.item);
            }
            if events.reset
                && let Some(clip) = small.clips[1]
            {
                small.sounds.push((clip, r.reset_volume, position));
            }
            if small.firing == Some(b.item) {
                small.effects[0].emitter = origin.absolute(hole.translation.to_array());
            }
            let rotation = crate::simulation::dials::unity_euler([b.trigger.euler_x(r), 0., 0.]);
            if rotation != b.pose.rotation {
                b.pose.rotation = rotation;
                let slot = ContentSlot::Active(b.slot);
                if !self.presentation_changes.contains(&slot) {
                    self.presentation_changes.push(slot);
                }
            }
            b.direction = None;
            b.target = None;
        }
        for e in &mut small.effects {
            let Some(elapsed) = e.elapsed.as_mut() else {
                continue;
            };
            e.particle.set_origin(frame.origin);
            e.particle.emission_rotation = Some(e.rotation);
            e.particle.advance(
                dt,
                e.emitter,
                if *elapsed < e.duration { 1. } else { 0. },
                physics,
            );
            *elapsed += dt;
        }
    }
    /// ginlampscript.Update after the tools rubbed this frame.
    pub fn advance_genie_lamps(
        &mut self,
        dt: f32,
        sleep_modifier: f32,
        reference: &ReferenceManifest,
        cleaning: &mut CleaningTools,
        items: &mut ItemStore,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let small = &mut self.small_items;
        small.time += f64::from(dt);
        let g = &reference.small_items.genie_lamp;
        for b in &mut small.lamps {
            let rubbed = cleaning.take_generated_rub(b.item);
            let before = b.lamp.value;
            let out = b.lamp.update(g, rubbed, small.time, dt, sleep_modifier);
            if b.lamp.value != before {
                items.mark_dirty(b.item);
            }
            if out.emission != b.emission {
                b.emission = out.emission;
                let slot = ContentSlot::Active(b.slot);
                if !self.presentation_changes.contains(&slot) {
                    self.presentation_changes.push(slot);
                }
            }
            let content = self
                .contents
                .active
                .get_mut(b.slot)
                .filter(|c| c.item == Some(b.item))
                .map(|c| &mut c.content.lights);
            for light in content
                .into_iter()
                .flatten()
                .chain(self.lights.iter_mut())
                .filter(|l| l.id == b.light)
            {
                light.intensity = out.intensity;
                light.range = out.range;
                light.record.range_meters = out.range;
            }
        }
    }
    /// Source ammoscript.CanReload: `to` takes from another bottle of its type.
    fn can_refill(&self, to: ItemId, from: ItemId) -> bool {
        let ammo = |id| {
            self.small_items
                .sprays
                .iter()
                .find(|b| b.item == id)
                .map(|b| b.ammo)
        };
        to != from
            && ammo(to)
                .zip(ammo(from))
                .is_some_and(|(a, b)| a.can_reload(&b))
    }
    /// Source ammoscript.Reload(from, 1).
    fn refill(&mut self, to: ItemId, from: ItemId, items: &mut ItemStore) {
        let sprays = &mut self.small_items.sprays;
        let (Some(a), Some(b)) = (
            sprays.iter().position(|s| s.item == to),
            sprays.iter().position(|s| s.item == from),
        ) else {
            return;
        };
        let mut source = sprays[b].ammo;
        sprays[a].ammo.reload(&mut source, 1.);
        sprays[b].ammo = source;
        items.mark_dirty(to);
        items.mark_dirty(from);
    }
    /// fpscontroller RayStuff for an aimed bottle. E (useUp) moves one unit from the carried bottle
    /// into the aimed one. R (reloadUp) moves one unit from the aimed bottle into the held one.
    /// With canPour the move repeats every pourTime while the key is held.
    pub fn refill_bottles(
        &mut self,
        r: &crate::reference::SprayBottleReference,
        input: RefillInput,
        items: &mut ItemStore,
    ) {
        use crate::interaction::feedback::ItemPrompt;
        self.small_items.refill_prompt = 0;
        let bottle = |id: Option<ItemId>| {
            id.filter(|id| self.small_items.sprays.iter().any(|b| b.item == *id))
        };
        let (carried, hand, aimed) = (
            bottle(input.carried),
            bottle(input.hand),
            bottle(input.aimed),
        );
        let Some(aimed) = aimed else {
            return;
        };
        let time = self.small_items.time;
        let pour = |held: bool, released: bool, last: &mut f64| {
            if r.can_pour {
                let due = held && time - *last >= f64::from(r.pour_time);
                if due {
                    *last = time;
                }
                due
            } else {
                released
            }
        };
        let mut last = self.small_items.last_reload;
        if let Some(from) = carried
            && input.hand.is_none()
            && self.can_refill(aimed, from)
        {
            self.small_items.refill_prompt |= ItemPrompt::UNLOAD;
            if pour(input.use_held, input.use_released, &mut last) {
                self.refill(aimed, from, items);
            }
        }
        if let Some(to) = hand.or(carried)
            && self.can_refill(to, aimed)
        {
            self.small_items.refill_prompt |= ItemPrompt::RELOAD;
            if pour(input.reload_held, input.reload_released, &mut last) {
                self.refill(to, aimed, items);
            }
        }
        self.small_items.last_reload = last;
    }
    pub fn refill_prompt(&self) -> u8 {
        self.small_items.refill_prompt
    }
    /// Saved spray ammo and lamp value; None at the spawn value.
    pub(super) fn small_item_values(&self, item: ItemId) -> Option<(Option<f32>, Option<f32>)> {
        if let Some(b) = self.small_items.sprays.iter().find(|b| b.item == item) {
            return Some(((b.ammo.count != b.initial).then_some(b.ammo.count), None));
        }
        if let Some(b) = self.small_items.ammo.iter().find(|b| b.item == item) {
            let count = b.ammo.ammo.count;
            return Some(((count != b.initial).then_some(count), None));
        }
        self.small_items
            .lamps
            .iter()
            .find(|b| b.item == item)
            .map(|b| (None, (b.lamp.value != b.initial).then_some(b.lamp.value)))
    }
    pub fn take_small_item_sounds(&mut self) -> std::vec::Drain<'_, (u32, f32, [f64; 3])> {
        self.small_items.sounds.drain(..)
    }
    pub fn small_item_particles(&self, index: usize) -> &[crate::particles::ParticleOutput] {
        self.small_items.particles(index)
    }
    /// ginlampscript `M.SetColor("_EmissionColor")`.
    pub fn lamp_emission(&self, parent: &GeneratedId) -> Option<[f32; 4]> {
        self.small_items
            .lamps
            .iter()
            .find(|b| &b.parent == parent)
            .map(|b| b.emission)
    }
    /// The spray bottle trigger node local pose.
    pub fn spray_trigger(&self, parent: &GeneratedId) -> Option<Transform> {
        self.small_items
            .sprays
            .iter()
            .find(|b| &b.parent == parent)
            .map(|b| b.pose)
    }
}

#[cfg(test)]
mod tests {
    use crate::world::{AbsolutePose, ObjectSnapshot};

    fn snapshot(ammo: Option<f32>, glow: Option<f32>) -> ObjectSnapshot {
        ObjectSnapshot {
            conditions: Vec::new(),
            pose: AbsolutePose {
                position: [1., 2., 3.],
                rotation: [0., 0., 0., 1.],
            },
            food: None,
            temperature: None,
            fluids: None,
            valve: None,
            lamp: None,
            lamp_broken: None,
            dial: None,
            ammo,
            glow,
            radio: None,
            detector: None,
            tool: None,
            gun: None,
            linear_velocity: [0.; 3],
            angular_velocity: [0.; 3],
            sleeping: false,
            held: false,
            buried: false,
            attachment: None,
            mechanism: None,
        }
    }

    #[test]
    fn ammo_and_lamp_value_round_trip() {
        let saved = snapshot(Some(37.), Some(0.25));
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(
            serde_json::from_str::<ObjectSnapshot>(&json).unwrap(),
            saved
        );
    }
}
