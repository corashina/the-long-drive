//! The "Reflections" display setting. Off turns every live reflection probe off.
use bevy::{ecs as bevy_ecs, prelude::*};

#[derive(Resource)]
pub struct Reflections(pub bool);

impl Default for Reflections {
    fn default() -> Self {
        Self(true)
    }
}

/// Read the setting that the browser sends at start and after each change.
pub(crate) fn poll(mut reflections: ResMut<Reflections>) {
    if let Some(on) = crate::browser::game_reflections() {
        reflections.0 = on;
    }
}
