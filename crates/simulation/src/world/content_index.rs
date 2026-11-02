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
    #[cfg(test)]
    pub(super) fn buttons<'a>(
        &'a self,
        lamps: &'a [super::content_lamps::LampBinding],
        parent: &'a GeneratedId,
    ) -> impl Iterator<Item = (&'a SourceId, Transform)> + 'a {
        self.lamp_slots(parent)
            .iter()
            .filter_map(|i| lamps[*i].button.as_ref())
            .map(|b| (&b.node, b.pose))
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
            &self.contents.active,
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
        index.content(&self.contents.active, &self.static_controls, id)
    }
    pub fn content_at(&self, slot: Option<ContentSlot>) -> IndexedContent<'_> {
        content_at(&self.contents.active, &self.static_controls, slot)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{CapVisualReference, ItemId, OpeningId};
    use crate::world::{AbsolutePose, Placement, WorldBounds, WorldLayer};

    #[test]
    fn world_sync_cache_tracks_membership_and_reads_live_content() {
        let source = SourceId {
            file: "test.assets".into(),
            path_id: 1,
        };
        let placement = |ordinal| Placement {
            id: GeneratedId {
                layer: WorldLayer::Building,
                generator: source.clone(),
                unit: [0; 2],
                ordinal,
                children: Vec::new(),
            },
            prototype: source.clone(),
            pose: AbsolutePose {
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
            },
            scale: [1.; 3],
            bounds: WorldBounds {
                min: [0.; 3],
                max: [0.; 3],
            },
            object_seed: 42,
        };
        let instance = |ordinal| ContentInstance::for_structure(placement(ordinal));
        let cap = || GeneratedCapVisual {
            opening: OpeningId(1),
            visual: CapVisualReference {
                open_nodes: Vec::new(),
                closed_nodes: Vec::new(),
                cap_node: None,
                closed_node: None,
                open_node: None,
                euler_rotation: false,
                closed_euler_degrees: [0.; 3],
                open_euler_degrees: [0.; 3],
            },
        };
        let mut visual = instance(8);
        assert!(visual.take_visual_change(None));
        let initial = visual.visual_revision;
        assert!(!visual.take_visual_change(None));
        assert_eq!(visual.visual_revision, initial);
        visual.item = Some(ItemId(1));
        assert!(visual.take_visual_change(None));
        assert!(!visual.take_visual_change(None));
        visual.restored = true;
        assert!(visual.take_visual_change(None));
        visual.caps.push(cap());
        assert!(visual.take_visual_change(None));
        let food = crate::interaction::items::FoodState {
            prototype_index: 0,
            bite: 1,
        };
        assert!(visual.take_visual_change(Some((food, false))));
        assert!(!visual.take_visual_change(Some((food, false))));
        assert!(visual.take_visual_change(Some((
            crate::interaction::items::FoodState { bite: 2, ..food },
            false
        ))));
        assert!(visual.take_visual_change(Some((food, true))));
        assert!(!visual.take_visual_change(Some((food, true))));
        // Mechanism and appearance producers publish one pending revision.
        visual.visual_revision = visual.visual_revision.wrapping_add(1);
        assert!(visual.take_visual_change(None));
        assert!(!visual.take_visual_change(None));
        let mut active = vec![instance(0), instance(1), instance(1)];
        let mut controls = vec![instance(1), instance(2)];
        let first = placement(1).id;
        let static_id = placement(2).id;
        let missing = placement(9).id;
        let mut index = ContentIndex::default();
        index.update_contents((1, 0, 0), &active, &controls);
        assert_eq!(index.rebuilds, [1, 0]);
        assert!(index.retained_bytes() > 0);
        assert!(std::ptr::eq(
            index.content(&active, &controls, &first).content.unwrap(),
            &active[1]
        ));
        assert!(std::ptr::eq(
            index
                .content(&active, &controls, &static_id)
                .content
                .unwrap(),
            &controls[1]
        ));
        assert!(
            index
                .content(&active, &controls, &missing)
                .content
                .is_none()
        );
        assert!(index.content(&active, &controls, &missing).bindings_ready());
        assert!(
            index
                .content(&active, &controls, &missing)
                .caps()
                .is_empty()
        );
        assert_eq!(
            index.content(&active, &controls, &missing).node_pose(0),
            None
        );

        let pose = crate::vehicle::VehiclePose {
            position: [3., 4., 5.],
            rotation: [0., 0., 0., 1.],
        };
        active[1].poses.push(pose);
        active[1].item = Some(ItemId(1));
        active[1].content.appearance.push(NodeAppearance {
            node: source.clone(),
            primary: None,
            scale: Some([2.; 3]),
            active: Some(false),
            material: None,
            color: None,
            color_material: None,
            rotation: None,
        });
        active[1].caps.push(cap());
        controls[1].caps.push(cap());
        controls[1].poses.push(pose);
        index.update_contents((1, 0, 0), &active, &controls);
        assert_eq!(index.rebuilds, [1, 0]);
        let found = index.content(&active, &controls, &first);
        assert_eq!(found.node_pose(0), Some(pose));
        assert!(!found.bindings_ready());
        assert_eq!(found.caps().len(), 1);
        assert!(std::ptr::eq(found.caps(), active[1].caps.as_slice()));
        assert_eq!(
            found.content.unwrap().content.appearance[0].scale,
            Some([2.; 3])
        );
        let found = index.content(&active, &controls, &static_id);
        assert_eq!(found.node_pose(0), Some(pose));
        assert!(found.caps().is_empty());
        assert!(!found.active);
        active[1].restored = true;
        active[1].content.appearance[0].scale = Some([3.; 3]);
        active[1].poses[0].position[0] = 7.;
        let found = index.content(&active, &controls, &first);
        assert!(found.bindings_ready());
        assert_eq!(found.node_pose(0).unwrap().position[0], 7.);
        assert_eq!(
            found.content.unwrap().content.appearance[0].scale,
            Some([3.; 3])
        );

        active.swap_remove(0);
        active.push(instance(3));
        index.update_contents((1, 1, 0), &active, &controls);
        assert_eq!(index.rebuilds, [2, 0]);
        assert!(
            index
                .content(&active, &controls, &placement(0).id)
                .content
                .is_none()
        );
        assert!(std::ptr::eq(
            index.content(&active, &controls, &first).content.unwrap(),
            &active[0]
        ));
        assert!(index.content(&active, &controls, &placement(3).id).active);
        active.swap_remove(2);
        index.update_contents((1, 2, 0), &active, &controls);
        assert_eq!(index.rebuilds, [3, 0]);
        assert!(std::ptr::eq(
            index
                .content(&active, &controls, &static_id)
                .content
                .unwrap(),
            &controls[1]
        ));

        controls.swap_remove(0);
        controls.push(instance(4));
        index.update_contents((1, 2, 1), &active, &controls);
        assert_eq!(index.rebuilds, [4, 0]);
        assert!(
            index
                .content(&active, &controls, &placement(4).id)
                .content
                .is_some()
        );
        active = vec![instance(5), instance(6)];
        index.update_contents((2, 2, 1), &active, &controls);
        assert_eq!(index.rebuilds, [5, 0]);
        assert!(index.content(&active, &controls, &first).content.is_none());
        assert!(
            index
                .content(&active, &controls, &placement(5).id)
                .content
                .is_some()
        );
        index.update_contents((2, 2, 1), &active, &controls);
        assert_eq!(index.rebuilds, [5, 0]);
    }
    #[test]
    fn world_sync_selection_visits_only_named_instances_and_retains_capacity() {
        let mut selection = InstanceSelection::default();
        selection.reset(50_000);
        selection.push(12);
        selection.push(49_999);
        selection.push(12);
        assert_eq!(selection.len(), 2);
        assert_eq!((selection.at(0), selection.at(1)), (12, 49_999));
        let capacity = selection.indices.capacity();
        let bytes = selection.retained_bytes();
        selection.clear();
        assert_eq!(selection.len(), 0);
        selection.push(12);
        assert_eq!(selection.len(), 1);
        assert_eq!(selection.indices.capacity(), capacity);
        assert_eq!(selection.retained_bytes(), bytes);
        // Retirement, swap_remove, and new sessions discard old slot numbers.
        selection.reset(1);
        selection.push(49_999);
        assert_eq!(selection.len(), 0);
        selection.push(0);
        assert_eq!(selection.at(0), 0);
    }
}
