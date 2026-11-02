//! Source music boxes: musicboxscript crank, notes and turning parts on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::interaction::items::ItemStore;
use crate::interaction::{InteractionState, Knob};
use crate::reference::{ColliderId, ItemId, ReferenceError, SourceId, Transform, WorldValue};
use crate::simulation::music_box::MusicBox;
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::spawn_rules::*;
use rapier3d::glamx::{Quat, Vec3};

/// One musicboxscript `rot`: a source transform turned about its local axis.
pub struct MusicBoxNode {
    /// Prototype node index.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub node: usize,
    pub pose: Transform,
    /// Converted local axis.
    axis: Vec3,
    speed: f32,
}
pub struct MusicBoxBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    item: ItemId,
    knobs: Vec<ColliderId>,
    music: MusicBox,
    /// usablescript lastRotX and lastRotY since the last update.
    crank: [f32; 2],
    detune: f32,
    chance: f32,
    pub nodes: Vec<MusicBoxNode>,
}
impl MusicBoxBinding {
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.knobs.capacity() * std::mem::size_of::<ColliderId>()
            + self.music.heap_bytes()
            + self.nodes.capacity() * std::mem::size_of::<MusicBoxNode>()
    }
}

impl WorldStream {
    pub(super) fn sync_music_box_bindings(
        &mut self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.music_box_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.music_box_bindings.len() {
            let b = &mut self.music_box_bindings[i];
            if let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
            {
                b.slot = slot;
                i += 1;
            } else {
                for knob in &b.knobs {
                    interaction.remove_dial_knob(*knob);
                }
                self.music_box_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            if self.music_box_bindings.iter().any(|b| b.item == item) {
                continue;
            }
            let root = &content.content.placement.prototype;
            let Some(script) =
                self.contents
                    .rules
                    .component(&self.reference, root, "musicboxscript")?
            else {
                continue;
            };
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let script = script.source.clone();
            let binding = self.bind_music_box(items, interaction, physics, slot, item, &script)?;
            self.music_box_bindings.push(binding);
        }
        self.music_box_scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_music_box(
        &self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        item: ItemId,
        script: &SourceId,
    ) -> Result<MusicBoxBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &self.reference;
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| p.source == content.content.placement.prototype)
            .ok_or_else(|| ReferenceError("Music box prototype missing".into()))?;
        let f = rules.get(reference, script)?.fields;
        let mut program = Vec::new();
        for line in array(f, "defprogram")? {
            match line {
                WorldValue::Text(line) => program.push(line.as_str()),
                _ => return Err(ReferenceError("Invalid music box program".into())),
            }
        }
        let music = MusicBox::new(
            program.into_iter(),
            number(f, "pauseTime")?,
            number(f, "sensModifier")?,
        )
        .ok_or_else(|| ReferenceError("Empty music box program".into()))?;
        let mut nodes = Vec::new();
        for r in array(f, "rots")? {
            let r = fields(r)?;
            let t = link(r, "T")?;
            let axis = fields(value(r, "axis")?)?;
            let axis = Vec3::new(-number(axis, "x")?, -number(axis, "y")?, number(axis, "z")?);
            nodes.push(MusicBoxNode {
                node: prototype
                    .nodes
                    .iter()
                    .position(|n| &n.source == t)
                    .ok_or_else(|| ReferenceError("Music box part outside its prototype".into()))?,
                pose: rules.node(reference, t)?.pose,
                axis: axis.try_normalize().unwrap_or(Vec3::ZERO),
                speed: number(r, "speed")?,
            });
        }
        // The crank is every collider on the rotateAble usable's own transform.
        let usable = link(f, "use")?;
        if !flag(rules.get(reference, usable)?.fields, "rotateAble")? {
            return Err(ReferenceError("Music box crank is not rotateAble".into()));
        }
        let crank_node = rules.root(reference, usable)?;
        let body = items.get(item).and_then(|i| i.body);
        let mut knobs = Vec::new();
        let colliders = self
            .structures
            .iter()
            .filter(|s| s.placement.id == parent)
            .flat_map(|s| &s.colliders);
        for (source, handle) in colliders {
            let on_crank = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .is_some_and(|c| &prototype.nodes[c.node as usize].source == crank_node);
            if !on_crank || physics.rapier().colliders[*handle].parent() != body {
                continue;
            }
            let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            interaction.bind_dial_knob(id, Knob::Crank);
            knobs.push(id);
        }
        Ok(MusicBoxBinding {
            parent,
            slot,
            item,
            knobs,
            music,
            crank: [0.; 2],
            detune: number(f, "pitchDifference")?,
            chance: number(f, "pitchDifferenceChance")?,
            nodes,
        })
    }
    /// usablescript.Rot from the crank look input. Returns false when the knob is not a crank.
    pub fn crank_music_box(&mut self, knob: ColliderId, look: [f32; 2]) -> bool {
        let Some(b) = self
            .music_box_bindings
            .iter_mut()
            .find(|b| b.knobs.contains(&knob))
        else {
            return false;
        };
        b.crank = look;
        true
    }
    /// musicboxscript.Update. Calls `play` with the item, pitch, pitch difference and chance
    /// of each note.
    pub fn advance_music_boxes(&mut self, dt: f32, mut play: impl FnMut(ItemId, f32, f32, f32)) {
        for b in &mut self.music_box_bindings {
            let look = std::mem::take(&mut b.crank);
            let (item, detune, chance) = (b.item, b.detune, b.chance);
            let speed = b
                .music
                .step(look, dt, |pitch| play(item, pitch, detune, chance));
            if speed == 0. {
                continue;
            }
            for n in &mut b.nodes {
                let turn = Quat::from_axis_angle(n.axis, (speed * dt * n.speed).to_radians());
                n.pose.rotation = (Quat::from_array(n.pose.rotation) * turn)
                    .normalize()
                    .to_array();
            }
            let slot = ContentSlot::Active(b.slot);
            if !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
    }
    pub fn music_box(&self, parent: &GeneratedId) -> Option<&MusicBoxBinding> {
        self.music_box_bindings.iter().find(|b| &b.parent == parent)
    }
}
