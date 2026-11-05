//! Native culling planes before backend projection and framebuffer conversion.
use super::shadow_candidates::{Bounds, Plane, intersects_camera};

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
/// Camera forward, `-dot(position, forward)` and `-dot(position, backward)`.
/// world_from_view is the source inverse view, before world handedness conversion.
fn view_axis(world_from_view: [f32; 16]) -> ([f32; 3], f32, f32) {
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
    (forward, -dot(forward), -dot(backward))
}
/// Selected implicit-culling path. Explicit capture views still force parallel near/far planes.
pub fn implicit_cull_planes(
    source_clip_from_world: [f32; 16],
    world_from_view: [f32; 16],
    camera_near: f32,
    camera_far: f32,
    shadow_distance: f32,
    shadow_near_offset: f32,
) -> CullPlanes {
    let mut shadow = extract_planes(source_clip_from_world);
    let (forward, base_near, base_far) = view_axis(world_from_view);
    let backward = forward.map(|value| -value);
    shadow[4] = [forward[0], forward[1], forward[2], base_near - camera_near];
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

/// The source draws the world with two cameras. Cam and mainCam draw `near_mask` to `Cam1FarPlane`.
/// farCamera draws `far_mask` from `Cam2NearPlane`. The port draws with one camera and tests both ranges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerRanges {
    pub near_mask: u32,
    pub far_mask: u32,
    /// Far plane at `Cam1FarPlane` and near plane at `Cam2NearPlane`, in the frame of `implicit_cull_planes`.
    pub planes: [Plane; 2],
}
impl LayerRanges {
    pub fn new(
        world_from_view: [f32; 16],
        near_mask: u32,
        far_mask: u32,
        cam1_far: f32,
        cam2_near: f32,
    ) -> Self {
        let (forward, base_near, base_far) = view_axis(world_from_view);
        let [x, y, z] = forward;
        Self {
            near_mask,
            far_mask,
            planes: [
                [-x, -y, -z, cam1_far + base_far],
                [x, y, z, base_near - cam2_near],
            ],
        }
    }
    /// One range for every layer of `mask`, cut at the camera far plane.
    pub fn single(world_from_view: [f32; 16], mask: u32, far: f32) -> Self {
        Self::new(world_from_view, mask, 0, far, 0.)
    }
    /// Test the six camera planes separately. `layers` holds the renderer layer bits.
    pub fn visible(&self, layers: u32, bounds: Bounds) -> bool {
        (layers & self.near_mask != 0 && intersects_camera(bounds, &self.planes[..1]))
            || (layers & self.far_mask != 0 && intersects_camera(bounds, &self.planes[1..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_ranges_follow_source_cameras() {
        // Camera at the origin looks down source -Z: backward is +Z.
        let mut world_from_view = [0.; 16];
        for i in 0..4 {
            world_from_view[i * 5] = 1.;
        }
        let ranges = LayerRanges::new(world_from_view, 0b011, 0b101, 300., 200.);
        let at = |depth: f32| Bounds {
            center: [0., 0., -depth],
            extents: [1.; 3],
        };
        // Near layer: drawn before Cam1FarPlane, culled after it.
        assert!(ranges.visible(0b010, at(250.)));
        assert!(!ranges.visible(0b010, at(400.)));
        // Far layer: culled before Cam2NearPlane, drawn after it.
        assert!(!ranges.visible(0b100, at(150.)));
        assert!(ranges.visible(0b100, at(400.)));
        // A layer in both masks draws at any depth.
        assert!(ranges.visible(0b001, at(150.)) && ranges.visible(0b001, at(400.)));
        // Bounds that cross a plane stay visible.
        assert!(ranges.visible(0b010, at(300.5)));
    }
}
