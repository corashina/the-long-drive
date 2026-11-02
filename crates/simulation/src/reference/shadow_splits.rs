//! Installed StableFit caster split masks; separate from projection-bound membership.
use super::shadow_candidates::{Bounds, DirectionalHull, Plane};

pub struct SplitCull {
    pub bit: u8,
    pub sphere: [f32; 4],
    pub light_center: [f32; 3],
    pub hull: DirectionalHull,
}
impl SplitCull {
    /// ExtractShadowSplitCullInfos2da560 preserves plane indices and source float addition order.
    pub fn new(
        index: usize,
        sphere: [f32; 4],
        light_basis: [[f32; 3]; 3],
        mut receiver: [Plane; 6],
        camera_near: f32,
        shadow_distance: f32,
        min_view_distance: f32,
        max_view_distance: f32,
    ) -> Result<Self, &'static str> {
        if index >= 4 {
            return Err("Source shadow split index exceeds four cascades");
        }
        receiver[4][3] += min_view_distance - camera_near;
        receiver[5][3] += max_view_distance - shadow_distance;
        let center = [sphere[0], sphere[1], sphere[2]];
        Ok(Self {
            bit: 1 << index,
            sphere,
            light_center: light_basis.map(|row| dot_yxz(row, center)),
            hull: DirectionalHull::with_tested_planes(
                receiver,
                light_basis[2],
                [true, true, true, true, index == 0, index == 3],
            )?,
        })
    }
}
fn dot_yxz(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[1] * b[1] + a[0] * b[0]) + a[2] * b[2]
}
fn squared(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]
}
fn normal(p: Plane) -> [f32; 3] {
    [p[0], p[1], p[2]]
}
/// RayDistanceToFrustumOriented361fc0: nearest positive exit, with strict distance tie handling.
fn exit_plane(origin: [f32; 3], direction: [f32; 3], planes: &[Plane; 6]) -> Option<(usize, f32)> {
    let mut best = f32::INFINITY;
    let mut index = None;
    for (i, p) in planes.iter().enumerate() {
        let denominator = dot_yxz(normal(*p), direction);
        if denominator > 0. || denominator.abs() <= f32::from_bits(0x358637bd) {
            continue;
        }
        let distance = (-dot_yxz(normal(*p), origin) - p[3]) / denominator;
        if distance > 0. && distance < best {
            best = distance;
            index = Some(i);
        }
    }
    index.map(|i| (i, best))
}
/// IsCapsuleInsideSphere3558f0: both endpoints must lie strictly within the shrunken sphere.
fn capsule_inside(start: [f32; 3], end: [f32; 3], radius: f32, sphere: [f32; 4]) -> bool {
    if radius >= sphere[3] {
        return false;
    }
    let r = sphere[3] - radius;
    let distance = |p: [f32; 3]| squared(std::array::from_fn(|i| sphere[i] - p[i]));
    r * r > distance(start) && r * r > distance(end)
}
/// IntersectAABBPlaneBoundsOptimized360090 has its own add order, unlike the first candidate kernel.
fn split_hull_intersects(bounds: Bounds, hull: &DirectionalHull) -> bool {
    hull.planes().iter().all(|p| {
        let c = bounds.center;
        let e = bounds.extents;
        let radius = (p[1].abs() * e[1] + p[0].abs() * e[0]) + p[2].abs() * e[2];
        let distance = ((p[0] * c[0] + p[3]) + p[1] * c[1]) + p[2] * c[2];
        !(radius + distance < 0.)
    })
}
/// Inputs use source world coordinates and the recovered light basis rows.
/// Enabled splits retain source order; their sphere radius is not squared.
pub fn visible_split_mask(
    bounds: Bounds,
    light_basis: [[f32; 3]; 3],
    receiver: &[Plane; 6],
    splits: &[SplitCull],
) -> u8 {
    let radius = squared(bounds.extents).sqrt();
    let center = light_basis.map(|row| dot_yxz(row, bounds.center));
    let epsilon = f32::from_bits(0x38d1b717);
    let length = match exit_plane(bounds.center, light_basis[2], receiver) {
        None => 0.,
        Some((i, distance)) => {
            let plane = receiver[i];
            let cosine =
                (dot_yxz(normal(plane), bounds.center) + plane[3]).abs() / (distance + epsilon);
            let tangent = cosine / (1. - cosine * cosine).sqrt();
            radius / (tangent + epsilon) + distance
        }
    };
    let endpoint = std::array::from_fn(|i| bounds.center[i] + length * light_basis[2][i]);
    let mut mask = 0;
    for split in splits {
        let dx = center[0] - split.light_center[0];
        let dy = center[1] - split.light_center[1];
        let radius_sum = radius + split.sphere[3];
        let radius_squared = radius_sum * radius_sum;
        if radius_squared < dx * dx + dy * dy {
            continue;
        }
        let depth = ((radius_squared - dx * dx) - dy * dy).sqrt() + split.light_center[2];
        if center[2] > depth {
            continue;
        }
        mask |= split.bit;
        if capsule_inside(bounds.center, endpoint, radius, split.sphere) {
            break;
        }
    }
    if splits.len() > 1 {
        for split in splits {
            if mask & split.bit != 0 && !split_hull_intersects(bounds, &split.hull) {
                mask &= !split.bit;
            }
        }
    }
    mask
}
