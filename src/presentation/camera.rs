use bevy::ecs as bevy_ecs;
use bevy::prelude::*;

pub struct WorldCameraSettings {
    pub near: f32,
    pub far: f32,
    pub fov_degrees: f32,
    pub mask: u32,
}

/// The source draws near and far with Cam1 and Cam2. The port draws with one camera.
/// It uses `Cam2FarPlane` as `far`. The Cam1 far and Cam2 near planes stay unused.
pub fn world_camera_settings(fov_degrees: f32, far: f32) -> WorldCameraSettings {
    WorldCameraSettings {
        near: 0.05,
        far,
        fov_degrees,
        mask: 0x14DA8F33,
    }
}

#[derive(Component)]
pub struct DriveCamera;
