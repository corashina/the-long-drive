//! Flat prop entities. An instance spawns entities only for dynamic glTF nodes.
//! Renderers of static nodes attach to the nearest dynamic ancestor with a baked matrix.
use bevy::math::{DMat4, Mat4, Vec3};
use bevy::transform::components::Transform;

/// Per prototype glTF node plan. Built once per loaded prototype.
pub struct NodePlan {
    /// glTF parent of each glTF node.
    pub parents: Vec<Option<u32>>,
    /// Prototype node of each glTF node.
    pub sources: Vec<Option<u32>>,
    /// glTF nodes, each parent before its children.
    pub order: Vec<u32>,
    has_children: Vec<bool>,
    /// Runtime code writes the transform or visibility of the node.
    dynamic: Vec<bool>,
}
/// The flat layout of one instance.
pub struct FlatNodes {
    /// The instance spawns an entity for the node.
    pub spawned: Vec<bool>,
    /// Nearest spawned ancestor of each node. `None` is the instance root.
    pub anchors: Vec<Option<u32>>,
    /// Static matrix from the anchor to the node.
    pub rest: Vec<Mat4>,
    /// Matrix from the instance root to the node.
    pub chain: Vec<Mat4>,
}
impl NodePlan {
    /// `children` holds the glTF children of each node. `dynamic` holds the dynamic glTF nodes.
    pub fn new(children: &[&[u32]], sources: Vec<Option<u32>>, dynamic: Vec<bool>) -> Self {
        let count = children.len();
        let mut parents = vec![None; count];
        for (parent, list) in children.iter().enumerate() {
            for &child in *list {
                parents[child as usize] = Some(parent as u32);
            }
        }
        let mut order = Vec::with_capacity(count);
        order.extend((0..count as u32).filter(|&n| parents[n as usize].is_none()));
        let mut next = 0;
        while next < order.len() {
            order.extend_from_slice(children[order[next] as usize]);
            next += 1;
        }
        Self {
            parents,
            sources,
            order,
            has_children: children.iter().map(|c| !c.is_empty()).collect(),
            dynamic,
        }
    }
    /// Places the nodes of one instance from its local matrices.
    /// A node with children and a non-uniform scale also spawns an entity.
    /// So each baked matrix stays a pure scale, rotation and translation.
    pub fn flatten(&self, locals: &[Mat4]) -> FlatNodes {
        let count = locals.len();
        let mut flat = FlatNodes {
            spawned: vec![false; count],
            anchors: vec![None; count],
            rest: vec![Mat4::IDENTITY; count],
            chain: vec![Mat4::IDENTITY; count],
        };
        for &node in &self.order {
            let node = node as usize;
            let local = locals[node];
            flat.spawned[node] = self.dynamic[node] || (self.has_children[node] && !uniform(local));
            match self.parents[node] {
                None => {
                    flat.rest[node] = local;
                    flat.chain[node] = local;
                }
                Some(parent) => {
                    let parent = parent as usize;
                    flat.chain[node] = flat.chain[parent] * local;
                    if flat.spawned[parent] {
                        flat.anchors[node] = Some(parent as u32);
                        flat.rest[node] = local;
                    } else {
                        flat.anchors[node] = flat.anchors[parent];
                        flat.rest[node] = flat.rest[parent] * local;
                    }
                }
            }
        }
        flat
    }
}
impl FlatNodes {
    /// A static node under the instance root. It never moves after spawn.
    pub fn on_root(&self, node: usize) -> bool {
        !self.spawned[node] && self.anchors[node].is_none()
    }
}
fn uniform(local: Mat4) -> bool {
    let scale = local.to_scale_rotation_translation().0;
    (scale - Vec3::splat(scale.x)).abs().max_element() <= 1e-5 * scale.abs().max_element()
}
/// Transform of a spawned node from its pose relative to its glTF parent.
/// `parent` holds the anchor and glTF parent world matrices. It is `None` when the glTF parent is the anchor.
pub fn anchor_local(parent: Option<(DMat4, DMat4)>, local: Transform) -> Transform {
    match parent {
        Some((anchor, parent)) => {
            Transform::from_matrix((anchor.inverse() * parent).as_mat4() * local.to_matrix())
        }
        None => local,
    }
}
