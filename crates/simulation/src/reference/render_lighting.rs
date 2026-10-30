//! Recovered source light and realtime capture records.
use super::reflections::CUBE_BASES;
use super::{NodeId, NodeRecord, ReferenceError, SourceId, Transform, finite, require};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightShadows {
    pub mode: u8,
    pub strength: f32,
    pub depth_bias: f32,
    pub normal_bias: f32,
    pub near: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightRecord {
    pub source: SourceId,
    pub node: Option<NodeId>,
    pub world_pose: Transform,
    pub kind: LightKind,
    pub color: [f32; 4],
    pub intensity: f32,
    pub radiance: [f32; 3],
    pub range: f32,
    pub spot_angles: [f32; 2],
    pub mask: u32,
    pub enabled: bool,
    pub cookie: Option<SourceId>,
    pub cookie_size: f32,
    pub shadows: LightShadows,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeRefresh {
    IndividualFacesEveryFrame,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureFace {
    pub index: u8,
    pub basis: [[f32; 3]; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRecord {
    pub source: SourceId,
    pub enabled: bool,
    pub bounds_size: [f32; 3],
    pub bounds_offset: [f32; 3],
    pub box_projection: bool,
    pub blend_distance: f32,
    pub intensity: f32,
    pub importance: i32,
    pub near: f32,
    pub far: f32,
    pub shadow_distance: f32,
    pub mask: u32,
    pub resolution: u32,
    pub refresh: ProbeRefresh,
    pub decode: [f32; 4],
    pub faces: Vec<CaptureFace>,
    pub mip_extents: Vec<u32>,
}
pub(super) fn valid_source(source: &SourceId) -> bool {
    !source.file.is_empty() && source.path_id != 0
}
pub(super) fn valid_pose(pose: &Transform) -> bool {
    finite(&pose.translation)
        && finite(&pose.rotation)
        && finite(&pose.scale)
        && (pose.rotation.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.001
        && pose.scale.iter().all(|v| *v != 0.)
}
impl LightRecord {
    pub fn validate(
        &self,
        nodes: &[NodeRecord],
        textures: &HashSet<SourceId>,
    ) -> Result<(), ReferenceError> {
        let label = format!("light {}:{}", self.source.file, self.source.path_id);
        require(
            valid_source(&self.source) && valid_pose(&self.world_pose),
            &label,
        )?;
        require(
            self.node
                .is_none_or(|id| nodes.iter().any(|node| node.id == id)),
            &format!("{label}: node"),
        )?;
        require(
            self.node.is_some() || self.kind == LightKind::Directional,
            &format!("{label}: unbound local light"),
        )?;
        require(
            finite(&self.color)
                && self.color.iter().all(|v| *v >= 0.)
                && self.intensity.is_finite()
                && self.intensity >= 0.
                && finite(&self.radiance)
                && self.radiance.iter().all(|v| *v >= 0.)
                && self.range.is_finite()
                && self.range > 0.
                && finite(&self.spot_angles)
                && self.spot_angles[0] >= 0.
                && self.spot_angles[1] >= self.spot_angles[0]
                && self.spot_angles[1] < 180.,
            &label,
        )?;
        require(
            self.cookie
                .as_ref()
                .is_none_or(|id| valid_source(id) && (!self.enabled || textures.contains(id)))
                && self.cookie_size.is_finite()
                && self.cookie_size > 0.,
            &format!("{label}: cookie"),
        )?;
        let s = &self.shadows;
        require(
            s.mode <= 2
                && s.strength.is_finite()
                && (0. ..=1.).contains(&s.strength)
                && s.depth_bias.is_finite()
                && s.depth_bias >= 0.
                && s.normal_bias.is_finite()
                && s.normal_bias >= 0.
                && s.near.is_finite()
                && s.near > 0.,
            &format!("{label}: shadows"),
        )
    }
}
impl ProbeRecord {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let label = format!("probe {}:{}", self.source.file, self.source.path_id);
        require(valid_source(&self.source), &format!("{label}: source"))?;
        require(
            self.bounds_size == [6., 2.5, 6.]
                && self.importance == 1
                && finite(&self.bounds_offset)
                && !self.box_projection
                && self.blend_distance.is_finite()
                && self.blend_distance >= 0.
                && self.intensity.is_finite()
                && self.intensity >= 0.
                && self.near.is_finite()
                && self.near > 0.
                && self.far.is_finite()
                && self.far > self.near
                && self.shadow_distance.is_finite()
                && self.shadow_distance > self.near
                && self.decode == [1., 1., 0., 0.],
            &label,
        )?;
        require(
            self.resolution == 64
                && self.faces.len() == 6
                && self.mip_extents == [64, 32, 16, 8, 4, 2, 1],
            &format!("{label}: capture faces or mips"),
        )?;
        let mut indices = HashSet::new();
        for face in &self.faces {
            require(
                face.index < 6
                    && indices.insert(face.index)
                    && face.basis == CUBE_BASES[usize::from(face.index)],
                &format!("{label}: face basis"),
            )?;
        }
        Ok(())
    }
}

/// Renderer state belongs to an instance, even when its geometry is shared.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RendererRecord {
    pub source: SourceId,
    pub bounds_center: [f32; 3],
    pub bounds_extents: [f32; 3],
    pub cast_shadows: u8,
    pub receive_shadows: bool,
    pub reflection_probe_usage: u8,
    pub motion_vectors: u8,
    pub sorting_layer: i16,
    pub sorting_order: i16,
}
impl RendererRecord {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        require(
            valid_source(&self.source)
                && finite(&self.bounds_center)
                && finite(&self.bounds_extents)
                && self.bounds_extents.iter().all(|v| *v >= 0.)
                && self.cast_shadows <= 3
                && self.reflection_probe_usage <= 1
                && self.motion_vectors <= 2
                && self.sorting_layer == 0
                && self.sorting_order == 0,
            &format!("renderer {}:{}", self.source.file, self.source.path_id),
        )
    }
    pub fn validate_template(&self) -> Result<(), ReferenceError> {
        self.validate()?;
        let shadow_only = self.source.file == "sharedassets1.assets"
            && [98477, 98194, 93128, 91870, 98218].contains(&self.source.path_id);
        require(
            if shadow_only {
                self.cast_shadows == 3
            } else {
                self.cast_shadows <= 1
            },
            "template shadow source mode",
        )
    }
    /// Two constant vertex attributes. Flags are small integers, exactly represented by f32.
    pub fn attributes(&self) -> [[f32; 4]; 2] {
        [
            [
                self.bounds_center[0],
                self.bounds_center[1],
                self.bounds_center[2],
                f32::from(self.reflection_probe_usage),
            ],
            [
                self.bounds_extents[0],
                self.bounds_extents[1],
                self.bounds_extents[2],
                f32::from(
                    u8::from(self.cast_shadows != 0)
                        | ((self.receive_shadows as u8) << 1)
                        | (self.motion_vectors << 2),
                ),
            ],
        ]
    }
    /// Include every shader-visible field when caching startup mesh variants.
    pub fn mesh_variant_key(&self) -> [u32; 8] {
        let attributes = self.attributes();
        std::array::from_fn(|i| attributes[i / 4][i % 4].to_bits())
    }
}
