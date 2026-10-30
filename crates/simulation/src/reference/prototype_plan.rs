//! Exported selected scene metadata. No geometry or image decoding.
use super::{ReferenceError, SourceId};
use serde::Deserialize;
#[derive(Deserialize)]
pub struct PrototypePlan {
    pub json_bytes: usize,
    pub buffer_bytes: usize,
    pub mesh_bytes: usize,
    pub scene_bytes: usize,
    pub nodes: Vec<PlanNode>,
    pub primitives: Vec<PlanPrimitive>,
}
#[derive(Deserialize)]
pub struct PlanNode {
    pub index: u32,
    pub mesh: Option<u32>,
    pub skin: Option<u32>,
    pub children: Vec<u32>,
    pub matrix: Option<[f32; 16]>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub extras: Option<NodeExtras>,
}
#[derive(Deserialize)]
pub struct NodeExtras {
    pub source_renderer: Option<crate::reference::render_lighting::RendererRecord>,
    pub reference_id: Option<SourceId>,
    pub active: bool,
    pub source_active: Option<bool>,
    pub source_renderer_enabled: Option<bool>,
    pub renderer_enabled: Option<bool>,
    pub layer: Option<u8>,
    pub cast_shadows: Option<u8>,
    pub receive_shadows: Option<bool>,
}
#[derive(Clone, Deserialize)]
pub struct PlanPrimitive {
    pub mesh: u32,
    pub primitive: u32,
    pub submesh: u32,
    pub vertices: usize,
    pub indices: usize,
    pub bytes: usize,
    pub attributes: Vec<String>,
    pub source_material: Option<SourceId>,
    pub has_source_material: bool,
    pub material: Option<MaterialIdentity>,
}
#[derive(Clone, Deserialize)]
pub struct MaterialIdentity {
    pub asset: crate::reference::AssetId,
    pub gltf_material: u32,
}
impl PrototypePlan {
    pub fn retained_bytes(&self) -> usize {
        use std::mem::size_of;
        size_of::<Self>()
            + self.nodes.capacity() * size_of::<PlanNode>()
            + self.primitives.capacity() * size_of::<PlanPrimitive>()
            + self
                .nodes
                .iter()
                .map(|node| {
                    node.children.capacity() * 4
                        + node
                            .extras
                            .as_ref()
                            .and_then(|e| e.reference_id.as_ref())
                            .map_or(0, |id| id.file.capacity())
                        + node
                            .extras
                            .as_ref()
                            .and_then(|e| e.source_renderer.as_ref())
                            .map_or(0, |r| r.source.file.capacity())
                })
                .sum::<usize>()
            + self
                .primitives
                .iter()
                .map(|p| {
                    p.attributes.capacity() * size_of::<String>()
                        + p.attributes.iter().map(String::capacity).sum::<usize>()
                        + p.source_material
                            .as_ref()
                            .map_or(0, |id| id.file.capacity())
                })
                .sum::<usize>()
    }
    pub fn mesh_count(&self) -> usize {
        self.nodes
            .iter()
            .filter_map(|n| n.mesh)
            .map(|mesh| self.primitives.iter().filter(|p| p.mesh == mesh).count())
            .sum()
    }
    pub fn reservation(&self) -> Result<(usize, usize), ReferenceError> {
        let cpu = self
            .json_bytes
            .checked_mul(18)
            .and_then(|v| v.checked_add(self.buffer_bytes.checked_mul(2)?))
            .and_then(|v| v.checked_add(self.mesh_bytes.checked_mul(2)?))
            .and_then(|v| v.checked_add(self.scene_bytes))
            .and_then(|v| v.checked_add(self.mesh_count().checked_mul(4096)?))
            .ok_or_else(|| ReferenceError("Prototype reservation overflow".into()))?;
        let gpu = self
            .mesh_count()
            .checked_mul(8)
            .and_then(|bytes| self.mesh_bytes.checked_add(bytes))
            .ok_or_else(|| ReferenceError("Prototype GPU reservation overflow".into()))?;
        Ok((cpu, gpu))
    }
}
