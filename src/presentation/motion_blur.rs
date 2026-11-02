//! The "Motion blur" display setting. Off skips the blur pass. Default on. The source has no such setting.
use bevy::{ecs as bevy_ecs, prelude::*};

#[derive(Resource)]
pub struct MotionBlur(pub bool);

impl Default for MotionBlur {
    fn default() -> Self {
        Self(true)
    }
}

/// Read the setting that the browser sends at start and after each change.
pub(crate) fn poll(mut blur: ResMut<MotionBlur>) {
    if let Some(on) = crate::browser::game_motion_blur() {
        blur.0 = on;
    }
}
