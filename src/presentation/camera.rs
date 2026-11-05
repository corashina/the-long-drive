use crate::reference::lighting::{FAR_CAMERA_MASK, NEAR_CAMERA_MASK};
pub use crate::reference::shadow_planes::LayerRanges;
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;

pub struct WorldCameraSettings {
    pub near: f32,
    pub far: f32,
    pub fov_degrees: f32,
    pub mask: u32,
}

/// Source culling mask of the world camera.
pub const WORLD_MASK: u32 = NEAR_CAMERA_MASK | FAR_CAMERA_MASK;

/// The source draws near and far with Cam1 and Cam2. The port draws with one camera.
/// It uses `Cam2FarPlane` as `far`. `LayerRanges` applies the Cam1 far and Cam2 near planes per layer.
pub fn world_camera_settings(fov_degrees: f32, far: f32) -> WorldCameraSettings {
    WorldCameraSettings {
        near: 0.05,
        far,
        fov_degrees,
        mask: WORLD_MASK,
    }
}

#[derive(Component)]
pub struct DriveCamera;
