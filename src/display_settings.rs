//! The display settings are one record from the browser settings store (`web/display-settings.js`).
//! The browser sends the record at start and after each change with `window.gameDisplaySettings()`.
//! Antialiasing and Grass apply at page load. Their consumers read `loaded()`.
#[cfg(target_arch = "wasm32")]
use bevy::{ecs as bevy_ecs, prelude::*};
use simulation::render_scale::SceneScale;

/// Source ILUT: 0 is the HDR grading, 1 and 2 are LDR grading with `LutC64` and `LutSepia`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lut {
    #[default]
    Default,
    C64,
    Sepia,
}

/// "Shadows": halvings of the world view atlas side. 0 high, 1 medium, 2 low.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(transparent)]
pub struct ShadowQuality(pub u32);

impl ShadowQuality {
    /// World view atlas side for the source atlas side.
    pub fn atlas_size(self, source: u32) -> u32 {
        (source >> self.0).max(32)
    }
}

#[cfg_attr(target_arch = "wasm32", derive(Resource))]
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySettings {
    /// "Scene resolution" percent. 0 is Auto.
    pub scene_scale: u32,
    pub shadows: ShadowQuality,
    /// "Grass": 0 high, 1 medium, 2 low.
    pub grass: u32,
    /// "Sunglasses effects". Off removes the lens grading. The sun flare rule stays.
    pub sunglasses: bool,
    /// Source `settingsscript` BHeatHaze, BOldHeatHaze, ILUT and BGrayscale.
    pub heat_haze: bool,
    pub old_heat_haze: bool,
    pub lut: Lut,
    pub grayscale: bool,
    /// Source `BDof`. Off disables the `DOF` volume. Binoculars keep their DoF.
    pub depth_of_field: bool,
    /// "Mirrors": 0 high, 1 low, 2 off. Low draws 1 mirror per frame, not 2.
    pub mirrors: u32,
    /// Source `FMirrorDrawDistance`, in meters.
    pub mirror_distance: f32,
    /// Off skips the blur pass. The source has no such setting.
    pub motion_blur: bool,
    /// Off removes the shadows of point and spot lights.
    pub lamp_shadows: bool,
    /// "Antialiasing": 0 high, 1 low, 2 off.
    pub antialiasing: u32,
    /// Off skips the AO passes.
    pub ambient_occlusion: bool,
    /// "Reflections": 0 high, 1 low, 2 off. Low captures a probe job every 2nd frame.
    /// Off turns every live reflection probe off.
    pub reflections: u32,
}

/// The menu value for Auto scene resolution.
pub const AUTO_SCALE: u32 = 0;

impl DisplaySettings {
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if value.scene_scale != AUTO_SCALE {
            SceneScale::from_percent(value.scene_scale)?;
        }
        let levels = [
            value.shadows.0,
            value.grass,
            value.antialiasing,
            value.reflections,
            value.mirrors,
        ];
        if levels.iter().any(|level| *level > 2) {
            return Err(
                "Shadows, Grass, Antialiasing, Reflections and Mirrors must be 0, 1 or 2".into(),
            );
        }
        if !(value.mirror_distance.is_finite() && value.mirror_distance >= 0.) {
            return Err("Mirror draw distance must be a finite distance".into());
        }
        Ok(value)
    }
}

#[cfg(target_arch = "wasm32")]
fn take() -> Option<DisplaySettings> {
    let json = crate::browser::game_display_settings()?;
    Some(DisplaySettings::parse(&json).unwrap_or_else(|error| panic!("Display settings: {error}")))
}

/// The record at page load.
#[cfg(target_arch = "wasm32")]
pub fn loaded() -> DisplaySettings {
    static LOADED: std::sync::OnceLock<DisplaySettings> = std::sync::OnceLock::new();
    *LOADED.get_or_init(|| take().expect("The browser sends display settings at start"))
}

#[cfg(target_arch = "wasm32")]
fn poll(mut settings: ResMut<DisplaySettings>) {
    if let Some(value) = take() {
        *settings = value;
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn install(app: &mut App) {
    app.insert_resource(loaded())
        .add_systems(PreUpdate, poll.before(crate::render_scale::update));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `displayRecord` of the `web/display-settings.js` defaults.
    const DEFAULTS: &str = r#"{"scene_scale":0,"shadows":0,"grass":0,"sunglasses":true,"heat_haze":true,"old_heat_haze":false,"lut":"default","grayscale":false,"depth_of_field":false,"mirrors":0,"mirror_distance":150,"motion_blur":true,"lamp_shadows":true,"antialiasing":1,"ambient_occlusion":true,"reflections":0}"#;

    #[test]
    fn browser_record_parses_and_checks_values() {
        let value = DisplaySettings::parse(DEFAULTS).unwrap();
        assert_eq!(value.scene_scale, AUTO_SCALE);
        assert_eq!(value.lut, Lut::Default);
        assert!(
            DisplaySettings::parse(&DEFAULTS.replace(r#""shadows":0"#, r#""shadows":3"#)).is_err()
        );
        assert!(
            DisplaySettings::parse(&DEFAULTS.replace(r#""scene_scale":0"#, r#""scene_scale":90"#))
                .is_err()
        );
        assert!(
            DisplaySettings::parse(&DEFAULTS.replace(r#""scene_scale":0"#, r#""scene_scale":100"#))
                .is_ok()
        );
        assert!(DisplaySettings::parse(&DEFAULTS.replace(r#","reflections":0"#, "")).is_err());
    }
}
