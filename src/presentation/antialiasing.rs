//! The "Antialiasing" display setting at page load.
//! Pipelines are prepared once for the view sample count, so a change applies after a reload.
use bevy::{
    ecs as bevy_ecs, prelude::*, render as bevy_render, render::extract_resource::ExtractResource,
};

/// Read once at startup. It never changes while the game runs.
#[derive(Resource, Clone, Copy, PartialEq, Eq, ExtractResource)]
pub enum Antialiasing {
    /// 4x MSAA and TAA.
    High,
    /// TAA only. One sample removes the multisample targets and their resolve.
    Low,
    Off,
}

impl Antialiasing {
    fn from_setting(value: u32) -> Self {
        match value {
            0 => Self::High,
            1 => Self::Low,
            _ => Self::Off,
        }
    }
    /// World view sample count.
    pub fn samples(self) -> u32 {
        match self {
            Self::High => super::render_targets::MSAA_SAMPLES,
            Self::Low | Self::Off => 1,
        }
    }
    pub fn msaa(self) -> Msaa {
        Msaa::from_samples(self.samples())
    }
    pub fn taa(self) -> bool {
        self != Self::Off
    }
}

pub fn install(app: &mut App) {
    app.insert_resource(Antialiasing::from_setting(
        crate::display_settings::loaded().antialiasing,
    ))
    .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
        Antialiasing,
    >::default());
}
