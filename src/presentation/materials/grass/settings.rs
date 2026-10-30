use crate::reference::{ReferenceError, WorldSourceMaterial};
use grass_mesh::GrassSettings;

/// "Grass" setting at page load as (blade spacing, fade distance) factors.
/// High keeps the source. Lower steps make blades sparser and fade them nearer.
pub fn quality_scale() -> (f32, f32) {
    match crate::browser::game_grass_quality() {
        0 => (1., 1.),
        1 => (1.5, 0.75),
        _ => (2., 0.5),
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
    let (spacing, distance) = quality_scale();
    Ok(GrassSettings {
        edge_length: get("_EdgeLength")? * spacing,
        max_tessellation: max_tessellation as u32,
        max_height: get("_MaxHeight00")?,
        fade_end: get("_GrassFadeEnd")? * distance,
        lod_start: get("_LODStart")?,
        lod_end: get("_LODEnd")?,
        lod_max: lod_max as u32,
        smooth_domain: material
            .valid_keywords
            .iter()
            .any(|x| x == "GRASS_HEIGHT_SMOOTHING" || x == "GRASS_WIDTH_SMOOTHING"),
    })
}
