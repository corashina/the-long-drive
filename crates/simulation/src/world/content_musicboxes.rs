//! Source music boxes: musicboxscript crank, notes and turning parts on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use super::content_kinds::{BindContext, Bound, ContentKind, ContentKinds, KindBindings};
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

impl WorldStream {
    fn bind_music_box(
        &self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        item: ItemId,
        script: &SourceId,
    ) -> Result<MusicBoxBinding, ReferenceError> {
        let content = &self.contents.active()[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &self.reference;
        let prototype = reference.prototype(&content.content.placement.prototype)?;
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
                node: prototype.node(t)?,
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
            let id = interaction.bind_knob(physics, *handle, Knob::Crank, None)?;
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
            .kinds
            .music_boxes
            .list
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
        for b in &mut self.kinds.music_boxes.list {
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
        self.kinds
            .music_boxes
            .list
            .iter()
            .find(|b| &b.parent == parent)
    }
}

impl Bound for MusicBoxBinding {
    fn parent(&self) -> &GeneratedId {
        &self.parent
    }
    fn item(&self) -> Option<ItemId> {
        Some(self.item)
    }
    fn set_slot(&mut self, slot: usize) {
        self.slot = slot;
    }
    fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.knobs.capacity() * std::mem::size_of::<ColliderId>()
            + self.music.heap_bytes()
            + self.nodes.capacity() * std::mem::size_of::<MusicBoxNode>()
    }
}

pub(super) struct MusicBoxKind;
impl ContentKind for MusicBoxKind {
    type Binding = MusicBoxBinding;
    type Script = SourceId;
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<MusicBoxBinding> {
        &mut kinds.music_boxes
    }
    fn script(
        stream: &mut WorldStream,
        _: &BindContext,
        slot: usize,
    ) -> Result<Option<SourceId>, ReferenceError> {
        let root = &stream.contents.active()[slot].content.placement.prototype;
        let rules = &stream.contents.rules;
        Ok(rules
            .component(&stream.reference, root, "musicboxscript")?
            .map(|s| s.source.clone()))
    }
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        item: Option<ItemId>,
        script: SourceId,
    ) -> Result<Option<MusicBoxBinding>, ReferenceError> {
        let (items, physics) = (&*cx.items, &mut *cx.physics);
        stream
            .bind_music_box(items, cx.interaction, physics, slot, item.unwrap(), &script)
            .map(Some)
    }
    fn unbind(binding: &MusicBoxBinding, interaction: &mut InteractionState) {
        for knob in &binding.knobs {
            interaction.controls.remove_knob(*knob);
        }
    }
}
