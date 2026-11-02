//! Cached membership bindings and retained presentation changes.
use super::WorldStream;
use crate::reference::{SourceId, Transform};
use crate::world::{GeneratedCapVisual, GeneratedId, NodeAppearance, contents::ContentInstance};
use bevy_platform::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentSlot {
    Active(usize),
    Control(usize),
}
#[derive(Default)]
pub struct ContentIndex {
    contents: HashMap<GeneratedId, ContentSlot>,
    lamps: HashMap<GeneratedId, Vec<usize>>,
    content_key: Option<(u64, u64, u64)>,
    lamp_key: Option<(u64, u64)>,
    #[cfg(test)]
    pub(super) rebuilds: [usize; 2],
}
pub struct IndexedContent<'a> {
    content: Option<&'a ContentInstance>,
    active: bool,
}
impl<'a> IndexedContent<'a> {
    pub fn bindings_ready(&self) -> bool {
        self.content.is_none_or(|c| c.item.is_none() || c.restored)
    }
    pub fn caps(&self) -> &'a [GeneratedCapVisual] {
        match self.content {
            Some(c) if self.active => &c.caps,
            _ => &[],
        }
    }
    pub fn node_pose(&self, node: usize) -> Option<crate::vehicle::VehiclePose> {
        self.content.and_then(|c| c.poses.get(node)).copied()
    }
}
impl ContentIndex {
    fn update_contents(
        &mut self,
        key: (u64, u64, u64),
        active: &[ContentInstance],
        controls: &[ContentInstance],
    ) {
        if self.content_key == Some(key) {
            return;
        }
        #[cfg(test)]
        {
            self.rebuilds[0] += 1;
        }
        self.contents.clear();
        for (i, c) in active.iter().enumerate() {
            self.contents
                .entry(c.content.placement.id.clone())
                .or_insert(ContentSlot::Active(i));
        }
        for (i, c) in controls.iter().enumerate() {
            self.contents
                .entry(c.content.placement.id.clone())
                .or_insert(ContentSlot::Control(i));
        }
        self.content_key = Some(key);
    }
    pub(super) fn update_lamps(
        &mut self,
        key: (u64, u64),
        lamps: &[super::content_lamps::LampBinding],
    ) {
        if self.lamp_key == Some(key) {
            return;
        }
        #[cfg(test)]
        {
            self.rebuilds[1] += 1;
        }
        for indices in self.lamps.values_mut() {
            indices.clear();
        }
        for (i, b) in lamps.iter().enumerate() {
            self.lamps.entry(b.parent.clone()).or_default().push(i);
        }
        self.lamps.retain(|_, indices| !indices.is_empty());
        self.lamp_key = Some(key);
    }
    pub fn retained_bytes(&self) -> usize {
        crate::interaction::memory::platform_map(&self.contents)
            + crate::interaction::memory::platform_map(&self.lamps)
            + self
                .contents
                .keys()
                .map(crate::world::contents::identity_heap)
                .sum::<usize>()
            + self
                .lamps
                .iter()
                .map(|(id, slots)| {
                    crate::world::contents::identity_heap(id)
                        + slots.capacity() * std::mem::size_of::<usize>()
                })
                .sum::<usize>()
    }
    pub fn slot(&self, id: &GeneratedId) -> Option<ContentSlot> {
        self.contents.get(id).copied()
    }
    pub fn lamp_slots(&self, id: &GeneratedId) -> &[usize] {
        self.lamps.get(id).map_or(&[], Vec::as_slice)
    }
    #[cfg(test)]
    fn content<'a>(
        &self,
        active: &'a [ContentInstance],
        controls: &'a [ContentInstance],
        id: &GeneratedId,
    ) -> IndexedContent<'a> {
        content_at(active, controls, self.slot(id))
    }
}
fn content_at<'a>(
    active: &'a [ContentInstance],
    controls: &'a [ContentInstance],
    slot: Option<ContentSlot>,
) -> IndexedContent<'a> {
    let content = slot.and_then(|slot| match slot {
        ContentSlot::Active(i) => active.get(i),
        ContentSlot::Control(i) => controls.get(i),
    });
    IndexedContent {
        content,
        active: matches!(slot, Some(ContentSlot::Active(_))),
    }
}
impl WorldStream {
    pub fn index_contents(&self, index: &mut ContentIndex) {
        index.update_contents(
            (
                self.request.session,
                self.contents.active_revision(),
                self.static_control_revision,
            ),
            &self.contents.active(),
            &self.static_controls,
        );
        index.update_lamps(
            (self.request.session, self.lamp_binding_revision),
            &self.lamp_bindings,
        );
    }
    pub fn presentation_membership(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.request.session,
            self.contents.active_revision(),
            self.static_control_revision,
            self.lamp_binding_revision,
            self.structure_revision,
        )
    }
    #[cfg(test)]
    pub fn indexed_content(&self, index: &ContentIndex, id: &GeneratedId) -> IndexedContent<'_> {
        index.content(&self.contents.active(), &self.static_controls, id)
    }
    pub fn content_at(&self, slot: Option<ContentSlot>) -> IndexedContent<'_> {
        content_at(&self.contents.active(), &self.static_controls, slot)
    }
    pub fn indexed_appearance<'a>(
        &'a self,
        content: &IndexedContent<'a>,
        id: &GeneratedId,
    ) -> &'a [NodeAppearance] {
        match content.content {
            Some(c) if content.active => &c.content.appearance,
            _ => self.contents.cursor_appearance(id),
        }
    }
    pub fn lamp_button_at(&self, index: usize) -> Option<(&SourceId, Transform)> {
        self.lamp_bindings
            .get(index)?
            .button
            .as_ref()
            .map(|b| (&b.node, b.pose))
    }
    pub fn presentation_contents(&self) -> impl Iterator<Item = ContentSlot> + '_ {
        self.presentation_changes.iter().copied()
    }
    pub fn presentation_lamps(&self) -> impl Iterator<Item = usize> + '_ {
        self.presentation_lamp_changes.iter().copied()
    }
    pub fn clear_presentation_changes(&mut self) {
        self.presentation_changes.clear();
        self.presentation_lamp_changes.clear();
        self.cobwebs.changes.clear();
    }
}
/// Reused dense selection queue. Producers can name the same instance more than once.
#[derive(Default)]
pub struct InstanceSelection {
    queued: Vec<bool>,
    indices: Vec<usize>,
}
impl InstanceSelection {
    pub fn retained_bytes(&self) -> usize {
        self.queued.capacity() * std::mem::size_of::<bool>()
            + self.indices.capacity() * std::mem::size_of::<usize>()
    }
    pub fn reset(&mut self, count: usize) {
        self.queued.clear();
        self.queued.resize(count, false);
        self.indices.clear();
        self.indices.reserve(count);
    }
    pub fn push(&mut self, index: usize) {
        if let Some(queued) = self.queued.get_mut(index) {
            if !*queued {
                *queued = true;
                self.indices.push(index);
            }
        }
    }
    pub fn len(&self) -> usize {
        self.indices.len()
    }
    pub fn at(&self, index: usize) -> usize {
        self.indices[index]
    }
    pub fn clear(&mut self) {
        for index in self.indices.drain(..) {
            self.queued[index] = false;
        }
    }
}
