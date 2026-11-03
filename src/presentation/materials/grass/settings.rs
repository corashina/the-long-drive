use crate::reference::{ReferenceError, WorldSourceMaterial};
use grass_mesh::GrassSettings;

/// "Grass" setting at page load.
pub struct GrassQuality {
    /// Blade spacing factor.
    pub spacing: f32,
    /// Fade distance factor.
    pub distance: f32,
    /// Thinning start as a share of the fade end.
    pub thin_start: f32,
    /// Share of blades that stay at the fade end.
    pub thin_floor: f32,
}
/// High keeps the source spacing and fade. Lower steps make blades sparser, fade them nearer
/// and thin them out sooner with distance.
pub fn quality() -> GrassQuality {
    let (spacing, distance, thin_start, thin_floor) = match crate::browser::game_grass_quality() {
        0 => (1., 1., 0.5, 0.5),
        1 => (1.5, 0.75, 0.4, 0.35),
        _ => (2., 0.5, 0.3, 0.2),
    };
    GrassQuality {
        spacing,
        distance,
        thin_start,
        thin_floor,
    }
}

pub fn settings_from_source(
    material: &WorldSourceMaterial,
) -> Result<GrassSettings, ReferenceError> {
    let get = |name: &str| {
        material
            .floats
            .get(name)
            .copied()
            .ok_or_else(|| ReferenceError(format!("Grass material is missing {name}")))
    };
    let max_tessellation = get("_MaxTessellation")?;
    let lod_max = get("_LODMax")?;
    if !matches!(max_tessellation, 2.0 | 4.0)
        || !(1.0..=6.0).contains(&lod_max)
        || lod_max.fract() != 0.0
    {
        return Err(ReferenceError(
            "Unsupported checked Grass tessellation or blade segment range".into(),
        ));
    }
    let quality = quality();
    let fade_end = get("_GrassFadeEnd")? * quality.distance;
    Ok(GrassSettings {
        edge_length: get("_EdgeLength")? * quality.spacing,
        max_tessellation: max_tessellation as u32,
        max_height: get("_MaxHeight00")?,
        fade_end,
        lod_start: get("_LODStart")?,
        lod_end: get("_LODEnd")?,
        lod_max: lod_max as u32,
        smooth_domain: material
            .valid_keywords
            .iter()
            .any(|x| x == "GRASS_HEIGHT_SMOOTHING" || x == "GRASS_WIDTH_SMOOTHING"),
        thin_start: fade_end * quality.thin_start,
        thin_floor: quality.thin_floor,
    })
}
