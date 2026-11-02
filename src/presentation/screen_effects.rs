//! The "Heat haze", "Old heat haze", "LUT" and "Grayscale" display settings.
//! Source `settingsscript` BHeatHaze, BOldHeatHaze, ILUT and BGrayscale.
use bevy::{ecs as bevy_ecs, prelude::*};

/// Source ILUT: 0 is the HDR grading, 1 and 2 are LDR grading with `LutC64` and `LutSepia`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lut {
    #[default]
    Default,
    C64,
    Sepia,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenEffects {
    pub heat_haze: bool,
    pub old_heat_haze: bool,
    pub lut: Lut,
    pub grayscale: bool,
}

impl Default for ScreenEffects {
    fn default() -> Self {
        Self {
            heat_haze: true,
            old_heat_haze: false,
            lut: Lut::Default,
            grayscale: false,
        }
    }
}

/// Read the settings that the browser sends at start and after each change.
pub(crate) fn poll(mut effects: ResMut<ScreenEffects>) {
    if let Some(value) = crate::browser::game_screen_effects()
        .and_then(|json| serde_json::from_str::<ScreenEffects>(&json).ok())
    {
        *effects = value;
    }
}
