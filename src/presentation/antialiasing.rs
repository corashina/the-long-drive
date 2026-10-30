//! The "Antialiasing" display setting: 4x MSAA on, 1 sample off. Default on.
//! Pipelines are prepared once for the view sample count, so a change applies after a reload.
use bevy::{
    ecs as bevy_ecs, prelude::*, render as bevy_render, render::extract_resource::ExtractResource,
};

/// Read once at startup. It never changes while the game runs.
#[derive(Resource, Clone, Copy, ExtractResource)]
pub struct Antialiasing(pub bool);

impl Antialiasing {
    /// World view sample count.
    pub fn samples(self) -> u32 {
        if self.0 {
            super::render_targets::MSAA_SAMPLES
        } else {
            1
        }
    }
    pub fn msaa(self) -> Msaa {
        Msaa::from_samples(self.samples())
    }
}

pub fn install(app: &mut App) {
    app.insert_resource(Antialiasing(crate::browser::game_antialiasing()))
        .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
            Antialiasing,
        >::default());
}
