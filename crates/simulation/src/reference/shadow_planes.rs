//! Native culling planes before backend projection and framebuffer conversion.
use super::shadow_candidates::Plane;

/// ExtractProjectionPlanesHelper<0>::Apply2a78e0 takes a column-major source clip matrix.
pub fn extract_planes(matrix: [f32; 16]) -> [Plane; 6] {
    std::array::from_fn(|index| {
        let axis = index / 2;
        let plane: Plane = std::array::from_fn(|column| {
            let row3 = matrix[column * 4 + 3];
            let row = matrix[column * 4 + axis];
            if index % 2 == 0 {
                row3 + row
            } else {
                row3 - row
            }
        });
        // The first installed normalization sums Y then X. The later five sum X then Y.
        let xy = if index == 0 {
            plane[1] * plane[1] + plane[0] * plane[0]
        } else {
            plane[0] * plane[0] + plane[1] * plane[1]
        };
        let reciprocal = 1. / (xy + plane[2] * plane[2]).sqrt();
        plane.map(|value| value * reciprocal)
    })
}

#[derive(Clone, Copy)]
pub struct CullPlanes {
    pub camera: [Plane; 6],
    pub shadow: [Plane; 6],
}
/// Selected implicit-culling path; explicit capture views still force parallel near/far planes.
/// world_from_view is the source inverse view, before world handedness conversion.
pub fn implicit_cull_planes(
    source_clip_from_world: [f32; 16],
    world_from_view: [f32; 16],
    camera_near: f32,
    camera_far: f32,
    shadow_distance: f32,
    shadow_near_offset: f32,
) -> CullPlanes {
    let mut shadow = extract_planes(source_clip_from_world);
    let mut backward = [world_from_view[8], world_from_view[9], world_from_view[10]];
    let length = ((backward[1] * backward[1] + backward[0] * backward[0])
        + backward[2] * backward[2])
        .sqrt();
    if length > f32::from_bits(0x3727c5ac) {
        backward = backward.map(|value| value / length);
    } else {
        backward = [0.; 3];
    }
    let position = [
        world_from_view[12],
        world_from_view[13],
        world_from_view[14],
    ];
    let dot = |normal: [f32; 3]| {
        (position[1] * normal[1] + position[0] * normal[0]) + position[2] * normal[2]
    };
    let forward = backward.map(|value| -value);
    let near_d = -dot(forward) - camera_near;
    let base_far = -dot(backward);
    shadow[4] = [forward[0], forward[1], forward[2], near_d];
    shadow[5] = [
        backward[0],
        backward[1],
        backward[2],
        base_far + camera_far.min(shadow_distance),
    ];
    let mut camera = shadow;
    // SetupShadowCullData2dd537 restores the camera far plane independently of shadow range.
    camera[5][3] = camera_far + base_far;
    shadow[5][3] = camera_far.min(shadow_distance) + base_far;
    shadow[4][3] -= shadow_near_offset;
    CullPlanes { camera, shadow }
}
