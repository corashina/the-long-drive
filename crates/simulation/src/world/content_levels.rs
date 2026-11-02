//! Spirit level bubbles (bubbleLevelScript) on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
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
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.bubbles.capacity() * std::mem::size_of::<LevelBubble>()
    }
    pub fn bubbles(&self) -> impl Iterator<Item = &LevelBubble> {
        self.bubbles.iter()
    }
}

impl WorldStream {
    pub(super) fn sync_level_bindings(
        &mut self,
        level: &SpiritLevelReference,
        items: &ItemStore,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.level_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.level_bindings.len() {
            let b = &mut self.level_bindings[i];
            if let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
            {
                b.slot = slot;
                i += 1;
            } else {
                self.level_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            if content.content.placement.prototype != level.prototype
                || self.level_bindings.iter().any(|b| b.item == item)
            {
                continue;
            }
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let binding = self.bind_level(level, slot, item)?;
            self.level_bindings.push(binding);
        }
        self.level_scan = complete.then_some(revision);
        Ok(())
    }

    fn bind_level(
        &self,
        level: &SpiritLevelReference,
        slot: usize,
        item: ItemId,
    ) -> Result<LevelBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let placement = &content.content.placement;
        let reference = &self.reference;
        let rules = &self.contents.rules;
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| p.source == placement.prototype)
            .ok_or_else(|| ReferenceError("Spirit level prototype missing".into()))?;
        // The item pose is the pose of the pickup node, as content bindings place it.
        let pickup = match rules.component(reference, &placement.prototype, "childunparent")? {
            Some(unparent) => link(unparent.fields, "g")?,
            None => &placement.prototype,
        };
        let index = |source| {
            prototype
                .nodes
                .iter()
                .position(|n| &n.source == source)
                .ok_or_else(|| ReferenceError("Spirit level node outside its prototype".into()))
        };
        let anchor = index(pickup)?;
        let scale = Vector::from_array(placement.scale)
            * Vector::from_array(crate::world::placement::node_pose(prototype, anchor).scale);
        let mut bubbles = Vec::with_capacity(level.bubbles.len());
        for b in &level.bubbles {
            let node = index(&b.bubble)?;
            bubbles.push(LevelBubble {
                node,
                pose: prototype.nodes[node].transform,
                vial: Vial::new(prototype, anchor, node, index(&b.t1)?, index(&b.t2)?)?,
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
        for b in &mut self.level_bindings {
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
                .is_none_or(|body| body.is_sleeping());
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
        self.level_bindings.iter().find(|b| &b.parent == parent)
    }
}
