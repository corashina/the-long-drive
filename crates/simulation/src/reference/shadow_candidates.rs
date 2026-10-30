//! Installed directional hull and projection-union rules. No main-view Umbra assumption is made here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub center: [f32; 3],
    pub extents: [f32; 3],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinMax {
    pub min: [f32; 3],
    pub max: [f32; 3],
}
impl MinMax {
    fn from_bounds(b: Bounds) -> Self {
        Self {
            min: std::array::from_fn(|i| b.center[i] - b.extents[i]),
            max: std::array::from_fn(|i| b.center[i] + b.extents[i]),
        }
    }
    fn include(&mut self, b: Bounds) {
        let b = Self::from_bounds(b);
        for i in 0..3 {
            self.min[i] = self.min[i].min(b.min[i]);
            self.max[i] = self.max[i].max(b.max[i]);
        }
    }
}
pub type Plane = [f32; 4];
fn dot_yxz(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[1] * b[1] + a[0] * b[0]) + a[2] * b[2]
}
fn dot_xyz(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
fn normal(p: Plane) -> [f32; 3] {
    [p[0], p[1], p[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// IntersectThreePlanes2db3d0: preserve separate products, reciprocal and final multiply.
fn intersect(a: Plane, b: Plane, c: Plane) -> [f32; 3] {
    let ab = cross(normal(a), normal(b));
    let ac = cross(normal(a), normal(c));
    let cb = cross(normal(c), normal(b));
    let reciprocal = 1.0 / dot_xyz(normal(c), ab);
    std::array::from_fn(|i| ((b[3] * ac[i] + a[3] * cb[i]) - c[3] * ab[i]) * reciprocal)
}
/// Fixed capacity matches CalculateShadowCasterCull2d5300. No heap storage or padding changes the result.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DirectionalHull {
    planes: [Plane; 10],
    len: usize,
}
impl DirectionalHull {
    pub fn planes(&self) -> &[Plane] {
        &self.planes[..self.len]
    }
    pub fn new(receiver: [Plane; 6], light_forward: [f32; 3]) -> Result<Self, &'static str> {
        Self::with_tested_planes(receiver, light_forward, [false; 6])
    }
    /// Split hulls omit retained planes already tested by the full candidate hull.
    /// Their silhouette edges still use every receiver-plane classification.
    pub fn with_tested_planes(
        receiver: [Plane; 6],
        light_forward: [f32; 3],
        tested: [bool; 6],
    ) -> Result<Self, &'static str> {
        if receiver
            .iter()
            .flatten()
            .chain(light_forward.iter())
            .any(|x| !x.is_finite())
        {
            return Err("Source shadow hull requires finite planes and light direction");
        }
        let triples = [
            [4, 1, 2],
            [4, 1, 3],
            [4, 0, 3],
            [4, 0, 2],
            [5, 1, 2],
            [5, 1, 3],
            [5, 0, 3],
            [5, 0, 2],
        ];
        let corners = triples.map(|[a, b, c]| intersect(receiver[a], receiver[b], receiver[c]));
        if corners.iter().flatten().any(|x| !x.is_finite()) {
            return Err("Source shadow receiver planes do not form a finite frustum");
        }
        let opposite = light_forward.map(|x| -x);
        let facing = receiver.map(|p| dot_yxz(normal(p), opposite) > 0.0);
        let mut hull = Self {
            planes: [[0.; 4]; 10],
            len: 0,
        };
        // PrepareShadowedLight2dc133 passes s_AlreadyTestedNone, six zero bytes.
        for ((p, keep), tested) in receiver.into_iter().zip(facing).zip(tested) {
            if keep && !tested {
                hull.push(p);
            }
        }
        let mut center = [0.; 3];
        for corner in corners {
            for i in 0..3 {
                center[i] += corner[i];
            }
        }
        center = center.map(|x| x * 0.125);
        let edges = [
            [0, 1],
            [1, 2],
            [2, 3],
            [3, 0],
            [4, 5],
            [5, 6],
            [6, 7],
            [7, 4],
            [0, 4],
            [1, 5],
            [2, 6],
            [3, 7],
        ];
        let adjacent = [
            [4, 1],
            [4, 3],
            [4, 0],
            [4, 2],
            [5, 1],
            [5, 3],
            [5, 0],
            [5, 2],
            [2, 1],
            [1, 3],
            [3, 0],
            [0, 2],
        ];
        for ([a, b], [p, q]) in edges.into_iter().zip(adjacent) {
            if facing[p] == facing[q] {
                continue;
            }
            let edge = std::array::from_fn(|i| corners[b][i] - corners[a][i]);
            let value = cross(light_forward, edge);
            let squared = dot_yxz(value, value);
            if squared <= f32::from_bits(0x34000000) {
                continue;
            }
            let offset = std::array::from_fn(|i| center[i] - corners[a][i]);
            let sign = if dot_yxz(offset, value) < 0.0 {
                -1.0
            } else {
                1.0
            };
            let scale = sign * (1.0 / squared.sqrt());
            let n = value.map(|x| x * scale);
            hull.push([n[0], n[1], n[2], -dot_yxz(n, corners[a])]);
            if hull.len == 10 {
                break;
            }
        }
        Ok(hull)
    }
    fn push(&mut self, plane: Plane) {
        self.planes[self.len] = plane;
        self.len += 1;
    }
    /// Kernel317640 negates planes, then compares radius against distance. Contact remains inside.
    pub fn intersects(&self, bounds: Bounds) -> bool {
        self.planes().iter().all(|p| {
            let neg = [-p[0], -p[1], -p[2]];
            let distance = dot_xyz(neg, bounds.center) + (-p[3]);
            let radius = dot_xyz(neg.map(f32::abs), bounds.extents);
            !(radius < distance)
        })
    }
}
/// Inputs are per-renderer state, before camera-visible phase batching.
/// Active LOD state and renderer-array membership must come from their proved target contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateState {
    pub shadow_casting_mode: u8,
    pub layer: u8,
    pub renderer_present: bool,
    pub disabled: bool,
    pub force_off: bool,
    pub lod_group: u32,
    pub lod_index_mask: u8,
    pub active_lod: u8,
}
pub fn no_umbra_candidate(
    state: &CandidateState,
    bounds: Bounds,
    hull: &DirectionalHull,
    light_mask: u32,
    view_mask: u32,
) -> bool {
    state.shadow_casting_mode != 0
        && state.layer < 32
        && (light_mask & view_mask) & (1u32 << state.layer) != 0
        && state.renderer_present
        && !state.disabled
        && !state.force_off
        && (state.lod_group == 0 || state.active_lod & state.lod_index_mask != 0)
        && hull.intersects(bounds)
}
/// IntersectAABBFrustumFull35fe20 uses a different scalar addition boundary from the hull kernel.
pub fn intersects_camera(bounds: Bounds, camera_planes: &[Plane; 6]) -> bool {
    camera_planes.iter().all(|p| {
        let n = normal(*p);
        let center = dot_xyz(n, bounds.center) + p[3];
        let radius = dot_xyz(n.map(f32::abs), bounds.extents);
        !(center + radius < 0.0)
    })
}
#[derive(Default)]
pub struct ProjectionUnion {
    preferred: Option<MinMax>,
    fallback: Option<MinMax>,
}
impl ProjectionUnion {
    /// Call once per complete renderer AABB only after at least one valid caster material part was added.
    pub fn include(&mut self, bounds: Bounds, camera_planes: &[Plane; 6]) {
        let slot = if intersects_camera(bounds, camera_planes) {
            &mut self.preferred
        } else {
            &mut self.fallback
        };
        match slot {
            Some(value) => value.include(bounds),
            None => *slot = Some(MinMax::from_bounds(bounds)),
        }
    }
    pub fn bounds(&self) -> Option<MinMax> {
        self.preferred.or(self.fallback)
    }
}
