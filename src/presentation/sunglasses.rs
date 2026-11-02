//! The "Sunglasses effects" display setting. Off removes the lens grading. The sun flare rule stays.
use bevy::{ecs as bevy_ecs, prelude::*};

#[derive(Resource)]
pub struct SunglassesEffects(pub bool);

impl Default for SunglassesEffects {
    fn default() -> Self {
        Self(true)
    }
}

/// Read the setting that the browser sends at start and after each change.
pub(crate) fn poll(mut effects: ResMut<SunglassesEffects>) {
    if let Some(on) = crate::browser::game_sunglasses_effects() {
        effects.0 = on;
    }
}
