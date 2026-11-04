//! Player projections share source planes. Probe projections remain fixed.
use bevy::camera::{PerspectiveProjection, Projection};

/// The player view takes the field of view and the far plane of each simulation frame.
pub fn sync_player_projection(projection: &mut Projection, fov_degrees: f32, far: f32) {
    if let Projection::Perspective(perspective) = projection {
        perspective.fov = fov_degrees.to_radians();
        perspective.far = far;
    }
}

/// `far` is the session `Cam2FarPlane`.
pub fn player_perspective(fov_degrees: f32, aspect_ratio: f32, far: f32) -> PerspectiveProjection {
    PerspectiveProjection {
        fov: fov_degrees.to_radians(),
        aspect_ratio,
        near: 0.05,
        far,
        ..Default::default()
    }
}
