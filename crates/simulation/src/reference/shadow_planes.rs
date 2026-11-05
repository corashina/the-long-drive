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
/// Near and far planes of the view depth range. They have the operations of the implicit path.
/// world_from_view is the source inverse view, before world handedness conversion.
pub fn depth_planes(world_from_view: [f32; 16], near: f32, far: f32) -> [Plane; 2] {
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
    let base_far = -dot(backward);
    [
        [forward[0], forward[1], forward[2], -dot(forward) - near],
        [backward[0], backward[1], backward[2], far + base_far],
    ]
}
/// Selected implicit-culling path. Explicit capture views still force parallel near/far planes.
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
    let [near, far] = depth_planes(world_from_view, camera_near, camera_far);
    shadow[4] = near;
    let mut camera = shadow;
    // SetupShadowCullData2dd537 restores the camera far plane independently of shadow range.
    camera[5] = far;
    shadow[5] = depth_planes(
        world_from_view,
        camera_near,
        camera_far.min(shadow_distance),
    )[1];
    shadow[4][3] -= shadow_near_offset;
    CullPlanes { camera, shadow }
}
/// Source camera depth range of each layer, in the source culling frame.
/// Cam and mainCam layers draw nearer than `Cam1FarPlane`. farCamera layers draw beyond `Cam2NearPlane`.
/// A renderer that crosses a plane draws whole. The source clips its pixels at the plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerRanges {
    /// Far plane at `Cam1FarPlane`.
    pub near_far: [f32; 4],
    /// Near plane at `Cam2NearPlane`.
    pub far_near: [f32; 4],
    pub near_mask: u32,
    pub far_mask: u32,
}
impl LayerRanges {
    /// No layer cut. Views other than the world view use it.
    pub const ALL: Self = Self {
        near_far: [0., 0., 0., 1.],
        far_near: [0., 0., 0., 1.],
        near_mask: u32::MAX,
        far_mask: 0,
    };
    /// `world_from_view` is the source inverse view, as `implicit_cull_planes` takes it.
    pub fn world(world_from_view: [f32; 16], cam1_far: f32, cam2_near: f32) -> Self {
        let depth = |near, far| depth_planes(world_from_view, near, far);
        Self {
            near_far: depth(0., cam1_far)[1],
            far_near: depth(cam2_near, 0.)[0],
            near_mask: super::lighting::NEAR_CAMERA_MASK,
            far_mask: super::lighting::FAR_CAMERA_MASK,
        }
    }
    /// A source frame AABB of `layer` lies in the depth range of a camera that draws the layer.
    /// The operation order is that of `layer_visible` in source_props_cull.wgsl.
    pub fn visible(&self, layer: u8, center: [f32; 3], extents: [f32; 3]) -> bool {
        let inside = |p: [f32; 4]| {
            let distance = (p[0] * center[0] + p[1] * center[1]) + p[2] * center[2] + p[3];
            let radius =
                (p[0].abs() * extents[0] + p[1].abs() * extents[1]) + p[2].abs() * extents[2];
            distance + radius >= 0.
        };
        let bit = 1u32 << (layer & 31);
        (self.near_mask & bit != 0 && inside(self.near_far))
            || (self.far_mask & bit != 0 && inside(self.far_near))
    }
}

#[cfg(test)]
mod tests {
    use super::LayerRanges;

    #[test]
    fn source_camera_layer_ranges() {
        // Camera at the origin. The source view looks down -z, so world_from_view column 2 is +z.
        let world_from_view = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let ranges = LayerRanges::world(world_from_view, 400., 300.);
        let at = |depth: f32| [0., 0., -depth];
        let small = [1., 1., 1.];
        // Layer 0 is in Cam and farCamera: it draws at every depth.
        assert!(ranges.visible(0, at(10.), small));
        assert!(ranges.visible(0, at(5000.), small));
        // Layer 23 is in Cam only: it stops at Cam1FarPlane. A crossing bound stays.
        assert!(ranges.visible(23, at(399.5), small));
        assert!(!ranges.visible(23, at(402.), small));
        // Layer 19 is in farCamera only: it starts at Cam2NearPlane.
        assert!(!ranges.visible(19, at(250.), small));
        assert!(ranges.visible(19, at(299.5), small));
        assert!(ranges.visible(19, at(5000.), small));
        // Layer 2 is in no world camera.
        assert!(!ranges.visible(2, at(10.), small));
        assert!(LayerRanges::ALL.visible(2, at(1.0e6), small));
    }
}
