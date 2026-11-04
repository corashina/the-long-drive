//! Source boxes: spawnInBoxScript content of generated boxes and its release on a box break.
//! Content rolls at generation. It publishes only after its box breaks, at the box pose.
use super::WorldStream;
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, SourceId};
use crate::world::{AbsolutePose, GeneratedId, Placement};
use rapier3d::math::{Rotation, Vector};
use serde::{Deserialize, Serialize};

/// One broken box. `origin` is its generated pose and `pose` its pose at the break.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxRelease {
    pub id: GeneratedId,
    pub origin: AbsolutePose,
    pub pose: AbsolutePose,
}

/// Broken boxes. Box content keys are `box id + (spawnInBoxScript, copy)`.
#[derive(Default)]
pub struct BoxReleases {
    spawner: Option<SourceId>,
    released: Vec<BoxRelease>,
}
impl BoxReleases {
    pub fn new(spawner: SourceId) -> Self {
        Self {
            spawner: Some(spawner),
            released: Vec::new(),
        }
    }
    /// True when `child` is a content copy of the box `parent`.
    fn content_of(&self, parent: &GeneratedId, child: &GeneratedId) -> bool {
        child.layer == parent.layer
            && child.generator == parent.generator
            && child.unit == parent.unit
            && child.ordinal == parent.ordinal
            && child.children.len() == parent.children.len() + 1
            && child.children.starts_with(&parent.children)
            && child.children.last().map(|(s, _)| s) == self.spawner.as_ref()
    }
    fn is_content(&self, id: &GeneratedId) -> bool {
        id.children.last().map(|(s, _)| s) == self.spawner.as_ref()
    }
    fn release_of(&self, id: &GeneratedId) -> Option<&BoxRelease> {
        self.released.iter().find(|r| self.content_of(&r.id, id))
    }
    /// Source spawned flag. The content of an intact box does not publish.
    pub fn withheld(&self, id: &GeneratedId) -> bool {
        self.is_content(id) && self.release_of(id).is_none()
    }
    /// Returns false when the box is already broken, so content spawns once.
    pub fn release(&mut self, release: BoxRelease) -> bool {
        if self.released.iter().any(|r| r.id == release.id) {
            return false;
        }
        self.released.push(release);
        true
    }
    /// Moves released content from the generated box pose to the break pose.
    pub fn place(&self, placement: &mut Placement) {
        if let Some(release) = self.release_of(&placement.id) {
            relocate(placement, &release.origin, &release.pose);
        }
    }
    pub fn released(&self) -> &[BoxRelease] {
        &self.released
    }
    pub fn retain(&mut self, keep: impl FnMut(&BoxRelease) -> bool) {
        self.released.retain(keep);
    }
    pub fn restore(&mut self, released: &[BoxRelease]) {
        self.released.clear();
        for release in released {
            self.release(release.clone());
        }
    }
}

fn rotation(pose: &AbsolutePose) -> Rotation {
    Rotation::from_array(pose.rotation)
}
/// Applies `pose * inverse(origin)` to a placement and its bounds.
pub fn relocate(placement: &mut Placement, origin: &AbsolutePose, pose: &AbsolutePose) {
    let turn = rotation(pose) * rotation(origin).inverse();
    let map = |p: [f64; 3]| -> [f64; 3] {
        let local = Vector::new(
            (p[0] - origin.position[0]) as f32,
            (p[1] - origin.position[1]) as f32,
            (p[2] - origin.position[2]) as f32,
        );
        let moved = turn * local;
        [
            pose.position[0] + moved.x as f64,
            pose.position[1] + moved.y as f64,
            pose.position[2] + moved.z as f64,
        ]
    };
    let [min, max] = [placement.bounds.min, placement.bounds.max];
    let mut bounds = crate::world::WorldBounds {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };
    for corner in 0..8 {
        let p = map(std::array::from_fn(|a| {
            if corner & (1 << a) == 0 {
                min[a]
            } else {
                max[a]
            }
        }));
        bounds.min = std::array::from_fn(|a| bounds.min[a].min(p[a]));
        bounds.max = std::array::from_fn(|a| bounds.max[a].max(p[a]));
    }
    placement.pose.position = map(placement.pose.position);
    placement.pose.rotation = (turn * Rotation::from_array(placement.pose.rotation))
        .normalize()
        .to_array();
    placement.bounds = bounds;
}

/// Change revision whose forgotten box removals are applied.
#[derive(Default)]
pub struct BoxRuntime {
    changes: Option<u64>,
}

impl WorldStream {
    /// Box content that waits for its box to break.
    pub(super) fn box_content_withheld(&self, id: &GeneratedId) -> bool {
        self.contents.boxes.withheld(id)
    }
    pub(super) fn refresh_boxes(&mut self) {
        if self.boxes.changes == Some(self.changes.revision()) {
            return;
        }
        // A forgotten box removal respawns the box and withholds its content again.
        let changes = &self.changes;
        self.contents.boxes.retain(|r| {
            changes
                .get_generated(&r.id)
                .is_some_and(|c| c.snapshot.is_none())
        });
        self.boxes.changes = Some(self.changes.revision());
    }
    /// mountStuff.Unstore. A broken box releases its content at its current pose.
    /// Run before `remove_broken_item`. A box releases once.
    pub(super) fn break_box(&mut self, item: ItemId, items: &ItemStore) {
        let Some(state) = items.get(item) else {
            return;
        };
        let Some(content) = self.contents.active().iter().find(|c| c.item == Some(item)) else {
            return;
        };
        let release = BoxRelease {
            id: content.content.placement.id.clone(),
            origin: content.content.placement.pose,
            pose: AbsolutePose {
                position: self.origin.absolute(state.pose.position),
                rotation: state.pose.rotation,
            },
        };
        if !self.contents.boxes.release(release.clone()) {
            return;
        }
        let (active, _, boxes) = self.contents.active_parts();
        for content in active {
            if boxes.content_of(&release.id, &content.content.placement.id) {
                relocate(
                    &mut content.content.placement,
                    &release.origin,
                    &release.pose,
                );
            }
        }
    }
}
