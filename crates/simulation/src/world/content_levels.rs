//! Spirit level bubbles (bubbleLevelScript) on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use super::content_kinds::{BindContext, Bound, ContentKind, ContentKinds, KindBindings};
use crate::interaction::items::{ItemLocation, ItemStore};
use crate::interaction::level::{Vial, pose, step};
use crate::reference::{ItemId, ReferenceError, SpiritLevelReference, Transform};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::spawn_rules::link;
use rapier3d::math::{Rotation, Vector};

pub struct LevelBubble {
    /// Prototype node index.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub node: usize,
    pub pose: Transform,
    vial: Vial,
    value: f32,
}

pub struct LevelBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    item: ItemId,
    /// World scale of the item node.
    scale: Vector,
    bubbles: Vec<LevelBubble>,
    /// No bubble value changed in the last step.
    settled: bool,
}

impl LevelBinding {
    pub fn bubbles(&self) -> impl Iterator<Item = &LevelBubble> {
        self.bubbles.iter()
    }
}

impl WorldStream {
    fn bind_level(
        &self,
        level: &SpiritLevelReference,
        slot: usize,
        item: ItemId,
    ) -> Result<LevelBinding, ReferenceError> {
        let content = &self.contents.active()[slot];
        let placement = &content.content.placement;
        let reference = &self.reference;
        let rules = &self.contents.rules;
        let prototype = reference.prototype(&placement.prototype)?;
        // The item pose is the pose of the pickup node, as content bindings place it.
        let pickup = match rules.component(reference, &placement.prototype, "childunparent")? {
            Some(unparent) => link(unparent.fields, "g")?,
            None => &placement.prototype,
        };
        let anchor = prototype.node(pickup)?;
        let scale = Vector::from_array(placement.scale)
            * Vector::from_array(crate::world::placement::node_pose(prototype, anchor).scale);
        let mut bubbles = Vec::with_capacity(level.bubbles.len());
        for b in &level.bubbles {
            let node = prototype.node(&b.bubble)?;
            bubbles.push(LevelBubble {
                node,
                pose: prototype.nodes[node].transform,
                vial: Vial::new(
                    prototype,
                    anchor,
                    node,
                    prototype.node(&b.t1)?,
                    prototype.node(&b.t2)?,
                )?,
                value: b.startup_value,
            });
        }
        Ok(LevelBinding {
            parent: placement.id.clone(),
            slot,
            item,
            scale,
            bubbles,
            settled: false,
        })
    }

    /// bubbleLevelScript Update, for near and awake spirit levels only.
    pub fn advance_levels(
        &mut self,
        level: &SpiritLevelReference,
        dt: f32,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        for b in &mut self.kinds.levels.list {
            let Some(item) = items.get(b.item) else {
                continue;
            };
            if item.removed
                || item.inactive_pose.is_some()
                || matches!(item.location, ItemLocation::Stored(_))
            {
                continue;
            }
            let asleep = item
                .body
                .and_then(|h| physics.rapier().bodies.get(h))
                .is_none_or(rapier3d::dynamics::RigidBody::is_sleeping);
            if asleep && b.settled {
                continue;
            }
            let rotation = Rotation::from_array(item.pose.rotation);
            let mut changed = false;
            let mut settled = true;
            for (bubble, source) in b.bubbles.iter_mut().zip(&level.bubbles) {
                let value = bubble.value;
                let next = pose(
                    bubble.pose,
                    step(
                        &mut bubble.value,
                        source,
                        &bubble.vial,
                        rotation,
                        b.scale,
                        dt,
                    ),
                );
                settled &= bubble.value == value;
                changed |=
                    next.translation != bubble.pose.translation || next.scale != bubble.pose.scale;
                bubble.pose = next;
            }
            b.settled = settled;
            let slot = ContentSlot::Active(b.slot);
            if changed && !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
    }

    pub fn level(&self, parent: &GeneratedId) -> Option<&LevelBinding> {
        self.kinds.levels.list.iter().find(|b| &b.parent == parent)
    }
}

impl Bound for LevelBinding {
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
            + self.bubbles.capacity() * std::mem::size_of::<LevelBubble>()
    }
}

pub(super) struct LevelKind;
impl ContentKind for LevelKind {
    type Binding = LevelBinding;
    type Script = ();
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<LevelBinding> {
        &mut kinds.levels
    }
    fn script(
        stream: &mut WorldStream,
        cx: &BindContext,
        slot: usize,
    ) -> Result<Option<()>, ReferenceError> {
        let prototype = &stream.contents.active()[slot].content.placement.prototype;
        Ok((*prototype == cx.reference.small_items.spirit_level.prototype).then_some(()))
    }
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        item: Option<ItemId>,
        _: (),
    ) -> Result<Option<LevelBinding>, ReferenceError> {
        let level = &cx.reference.small_items.spirit_level;
        stream.bind_level(level, slot, item.unwrap()).map(Some)
    }
}
