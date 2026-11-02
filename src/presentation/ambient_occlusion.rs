//! The "Ambient occlusion" display setting. Off skips the AO passes. Default on.
use bevy::{ecs as bevy_ecs, prelude::*};

#[derive(Resource)]
pub struct AmbientOcclusion(pub bool);

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self(true)
    }
}

/// Read the setting that the browser sends at start and after each change.
pub(crate) fn poll(mut occlusion: ResMut<AmbientOcclusion>) {
    if let Some(on) = crate::browser::game_ambient_occlusion() {
        occlusion.0 = on;
    }
}
