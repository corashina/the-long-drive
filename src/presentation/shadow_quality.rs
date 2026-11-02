//! The "Shadows" display setting. It shrinks the world view shadow atlas. Default high.
use bevy::{ecs as bevy_ecs, prelude::*};

/// Halvings of the world view atlas side: 0 high, 1 medium, 2 low.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShadowQuality(pub u32);

impl ShadowQuality {
    /// World view atlas side for the source atlas side.
    pub fn atlas_size(self, source: u32) -> u32 {
        (source >> self.0).max(32)
    }
}

/// Read the setting from the browser each frame.
pub(crate) fn poll(mut quality: ResMut<ShadowQuality>) {
    let value = crate::browser::game_shadow_quality().min(2);
    if quality.0 != value {
        quality.0 = value;
    }
}
