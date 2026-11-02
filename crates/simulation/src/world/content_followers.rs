//! Source node followers (`followspringscript`, `followavgposrot`, `followposrotscript`) on
//! Bus02 and its trailers. Poses solve in the unscaled prototype root frame. Each write refreshes
//! the descendants of the written node, so later followers read moved children, as in Unity.
use super::WorldStream;
use crate::interaction::items::ItemStore;
use crate::reference::{
    FollowerReference, ReferenceError, Transform, WorldFollowersReference, WorldPrototypeReference,
};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::ObjectKey;
use rapier3d::math::{Rotation, Vector};

/// One node in the root frame. Scale is the lossy product of parent scales.
#[derive(Clone, Copy, Debug)]
struct NodeFrame {
    position: Vector,
    rotation: Rotation,
    scale: Vector,
}

/// Root-frame node poses of one prototype instance and its followers.
pub struct FollowerPoses {
    parents: Vec<Option<usize>>,
    /// Local pose of each node. Followers and wheels change it.
    local: Vec<Transform>,
    frames: Vec<NodeFrame>,
    /// Descendants of each node in `writes`, in hierarchy order: `refresh[range]`.
    refresh: Vec<usize>,
    /// Written node and its `refresh` range: wheels first, then followers.
    writes: Vec<(usize, std::ops::Range<usize>)>,
    /// Spring `maxDist` from the spawn pose, per follower. Others hold 0.
    spring_lengths: Vec<f32>,
    /// Root-frame wheel centers of the last step, in wheel order.
    pub wheel_positions: Vec<Vector>,
}

fn vector(v: [f32; 3]) -> Vector {
    Vector::from_array(v)
}

/// Unity Quaternion.LookRotation(direction, up) in the converted frame: local -Z faces `direction`.
fn look_rotation(direction: Vector) -> Rotation {
    let back = -direction.normalize_or_zero();
    if back == Vector::ZERO {
        return Rotation::IDENTITY;
    }
    let right = Vector::Y.cross(back).normalize_or(Vector::X);
    let up = back.cross(right);
    Rotation::from_mat3(&rapier3d::glamx::Mat3::from_cols(right, up, back))
}

impl FollowerPoses {
    pub fn new(prototype: &WorldPrototypeReference, record: &WorldFollowersReference) -> Self {
        let parents: Vec<_> = prototype
            .nodes
            .iter()
            .map(|n| n.parent.map(|p| p as usize))
            .collect();
        let local: Vec<_> = prototype.nodes.iter().map(|n| n.transform).collect();
        let writes_nodes = record
            .wheels
            .iter()
            .copied()
            .chain(record.followers.iter().map(FollowerReference::node));
        let mut refresh = Vec::new();
        let mut writes = Vec::new();
        for node in writes_nodes {
            let node = node as usize;
            let start = refresh.len();
            // Node order is hierarchy order: a descendant comes after its parent.
            for n in node + 1..parents.len() {
                let mut p = parents[n];
                while let Some(i) = p {
                    if i == node {
                        refresh.push(n);
                        break;
                    }
                    p = parents[i];
                }
            }
            writes.push((node, start..refresh.len()));
        }
        let mut poses = Self {
            parents,
            local,
            frames: vec![
                NodeFrame {
                    position: Vector::ZERO,
                    rotation: Rotation::IDENTITY,
                    scale: Vector::ONE,
                };
                prototype.nodes.len()
            ],
            refresh,
            writes,
            spring_lengths: Vec::with_capacity(record.followers.len()),
            wheel_positions: vec![Vector::ZERO; record.wheels.len()],
        };
        for n in 0..poses.frames.len() {
            poses.update(n);
        }
        // Source followspringscript.Start.
        for f in &record.followers {
            let length = match *f {
                FollowerReference::Spring { start, end, .. } => {
                    poses.position(start).distance(poses.position(end))
                }
                _ => 0.,
            };
            poses.spring_lengths.push(length);
        }
        poses
    }

    fn position(&self, node: u32) -> Vector {
        self.frames[node as usize].position
    }

    /// Root frame of node `n` from its parent frame and local pose.
    fn update(&mut self, n: usize) {
        let l = self.local[n];
        self.frames[n] = match self.parents[n] {
            None => NodeFrame {
                position: vector(l.translation),
                rotation: Rotation::from_array(l.rotation),
                scale: vector(l.scale),
            },
            Some(p) => {
                let parent = self.frames[p];
                NodeFrame {
                    position: parent.position
                        + parent.rotation * (parent.scale * vector(l.translation)),
                    rotation: parent.rotation * Rotation::from_array(l.rotation),
                    scale: parent.scale * vector(l.scale),
                }
            }
        };
    }

    /// Writes the root-frame pose of write `index` and refreshes its descendants.
    fn write(&mut self, index: usize, position: Vector, rotation: Rotation) {
        let (node, range) = self.writes[index].clone();
        let (parent_position, parent_rotation, parent_scale) = match self.parents[node] {
            Some(p) => (
                self.frames[p].position,
                self.frames[p].rotation,
                self.frames[p].scale,
            ),
            None => (Vector::ZERO, Rotation::IDENTITY, Vector::ONE),
        };
        let inverse = parent_rotation.inverse();
        self.local[node].translation =
            ((inverse * (position - parent_position)) / parent_scale).to_array();
        self.local[node].rotation = (inverse * rotation).normalize().to_array();
        self.update(node);
        for i in range {
            let n = self.refresh[i];
            self.update(n);
        }
    }

    /// One source Update of every follower, after the wheel poses of this step.
    pub fn step(&mut self, record: &WorldFollowersReference) {
        for (wheel, &node) in record.wheels.iter().enumerate() {
            let rotation = self.frames[node as usize].rotation;
            self.write(wheel, self.wheel_positions[wheel], rotation);
        }
        let first = record.wheels.len();
        for (i, f) in record.followers.iter().enumerate() {
            let node = f.node() as usize;
            let current = self.frames[node];
            let (position, rotation) = match *f {
                FollowerReference::Spring {
                    target,
                    from,
                    to,
                    start,
                    ..
                } => {
                    let t = self.position(start).distance(self.position(target))
                        / self.spring_lengths[i];
                    // Vector3.LerpUnclamped.
                    let (a, b) = (self.position(from), self.position(to));
                    (a + (b - a) * t, current.rotation)
                }
                FollowerReference::Average {
                    position,
                    rotation,
                    offset,
                    axis,
                    first: a,
                    second: b,
                    look,
                    ..
                } => {
                    let (middle, new_position) = if position {
                        let middle = (self.position(a.unwrap()) + self.position(b.unwrap())) * 0.5;
                        // T.TransformVector uses the rotation and scale before this write.
                        (
                            middle,
                            middle + current.rotation * (current.scale * vector(offset)),
                        )
                    } else {
                        (current.position, current.position)
                    };
                    let new_rotation = if rotation {
                        look_rotation(self.position(look.unwrap()) - middle)
                            * Rotation::from_rotation_arc(Vector::NEG_Z, vector(axis).normalize())
                    } else {
                        current.rotation
                    };
                    (new_position, new_rotation)
                }
                FollowerReference::Pose {
                    copy_position,
                    lerp,
                    copy_rotation,
                    look,
                    lerp_amount,
                    look_rotation: extra,
                    position_target,
                    from,
                    to,
                    rotation_target,
                    look_target,
                    ..
                } => {
                    let position = if copy_position {
                        self.position(position_target.unwrap())
                    } else if lerp {
                        self.position(from.unwrap())
                            .lerp(self.position(to.unwrap()), lerp_amount.clamp(0., 1.))
                    } else {
                        current.position
                    };
                    let rotation = if copy_rotation {
                        self.frames[rotation_target.unwrap() as usize].rotation
                    } else if look {
                        look_rotation(self.position(look_target.unwrap()) - position)
                            * Rotation::from_array(extra)
                    } else {
                        current.rotation
                    };
                    (position, rotation)
                }
            };
            self.write(first + i, position, rotation);
        }
    }

    /// Local pose of each follower node after the last step.
    pub fn rows<'a>(
        &'a self,
        record: &'a WorldFollowersReference,
    ) -> impl Iterator<Item = (usize, Transform)> + 'a {
        record.followers.iter().map(|f| {
            let n = f.node() as usize;
            (n, self.local[n])
        })
    }
}

/// Followers of one spawned vehicle or trailer.
pub struct FollowerBinding {
    pub(super) parent: GeneratedId,
    key: ObjectKey,
    prototype: usize,
    poses: FollowerPoses,
}

impl FollowerBinding {
    pub fn heap_bytes(&self) -> usize {
        let p = &self.poses;
        crate::world::contents::identity_heap(&self.parent)
            + p.parents.capacity() * std::mem::size_of::<Option<usize>>()
            + p.local.capacity() * std::mem::size_of::<Transform>()
            + p.frames.capacity() * std::mem::size_of::<NodeFrame>()
            + p.refresh.capacity() * std::mem::size_of::<usize>()
            + p.writes.capacity() * std::mem::size_of::<(usize, std::ops::Range<usize>)>()
            + p.spring_lengths.capacity() * std::mem::size_of::<f32>()
            + p.wheel_positions.capacity() * std::mem::size_of::<Vector>()
    }
}

impl WorldStream {
    /// Binds the followers of active prototypes once their bodies exist.
    pub(super) fn sync_follower_bindings(
        &mut self,
        items: &ItemStore,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.follower_scan == Some(revision) {
            return Ok(());
        }
        let active = &self.contents.active;
        self.follower_bindings
            .retain(|b| active.iter().any(|c| c.content.placement.id == b.parent));
        let mut complete = true;
        for content in &self.contents.active {
            let Some(index) = self.follower_prototypes.iter().copied().find(|&p| {
                self.reference.prototypes[p].source == content.content.placement.prototype
            }) else {
                continue;
            };
            let parent = &content.content.placement.id;
            if self.follower_bindings.iter().any(|b| &b.parent == parent) {
                continue;
            }
            if !content.restored {
                complete = false;
                continue;
            }
            if content
                .item
                .and_then(|i| items.get(i))
                .and_then(|i| i.body)
                .is_none()
            {
                continue;
            }
            let prototype = &self.reference.prototypes[index];
            let record = prototype.followers.as_ref().unwrap();
            self.follower_bindings.push(FollowerBinding {
                parent: parent.clone(),
                key: ObjectKey::Generated(parent.clone()),
                prototype: index,
                poses: FollowerPoses::new(prototype, record),
            });
        }
        self.follower_scan = complete.then_some(revision);
        Ok(())
    }

    /// Source Update of every follower. Run after the wheel steps.
    pub fn step_followers(&mut self, physics: &PhysicsWorld) {
        for b in &mut self.follower_bindings {
            let record = self.reference.prototypes[b.prototype]
                .followers
                .as_ref()
                .unwrap();
            let positions = &mut b.poses.wheel_positions;
            if let Some(trailer) = self.trailer_bindings.iter().find(|t| t.parent == b.parent) {
                for (p, root) in positions.iter_mut().zip(trailer.graphic_positions()) {
                    *p = root;
                }
            } else if let Some(centers) = physics.wheel_centers(&b.key) {
                // The chassis body frame is the car root node.
                for (p, center) in positions.iter_mut().zip(centers) {
                    *p = center;
                }
            } else {
                continue;
            }
            b.poses.step(record);
        }
    }

    /// Follower node poses of a spawned prototype, in their parent frames.
    pub fn followers(
        &self,
        parent: &GeneratedId,
    ) -> Option<impl Iterator<Item = (usize, Transform)> + '_> {
        let b = self
            .follower_bindings
            .iter()
            .find(|b| &b.parent == parent)?;
        let record = self.reference.prototypes[b.prototype].followers.as_ref()?;
        Some(b.poses.rows(record))
    }
}
