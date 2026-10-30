//! Source live mirror globals: `mainscript` ranges, `defMirrorTexture` and the Mirror draw distance slider.
use super::{ReferenceError, SourceId, require};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MirrorsReference {
    pub source: SourceId,
    /// `mainscript.minFov` and `maxFov`, degrees.
    pub min_fov: f32,
    pub max_fov: f32,
    /// `mainscript.minDist` and `maxDist`, meters.
    pub min_distance: f32,
    pub max_distance: f32,
    /// `defMirrorTexture` size.
    pub texture: [u32; 2],
    pub draw_distance: MirrorDrawDistance,
}

/// Source `tabGraphics.sliders[1]` (`FMirrorDrawDistance`). The slider middle shows `half`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MirrorDrawDistance {
    pub source: SourceId,
    pub default: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub half: f32,
    /// The typed value clamps to `minimum..=limit`.
    pub limit: f32,
    /// The value rounds to a multiple of `step`.
    pub step: f32,
}

impl MirrorsReference {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let d = &self.draw_distance;
        require(
            self.min_fov > 0.
                && self.min_fov < self.max_fov
                && self.max_fov < 180.
                && self.min_distance >= 0.
                && self.min_distance < self.max_distance
                && self.texture.iter().all(|&side| side > 0 && side <= 2048),
            "world mirrors",
        )?;
        require(
            d.step > 0.
                && d.minimum > 0.
                && d.minimum < d.half
                && d.half < d.maximum
                && d.maximum <= d.limit
                && (d.minimum..=d.limit).contains(&d.default),
            "world mirrors.draw_distance",
        )
    }
}
