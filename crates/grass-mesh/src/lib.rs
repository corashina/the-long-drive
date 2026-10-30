//! Source Grass calculation in the frozen source coordinate frame.
mod codec;
mod domain;
mod expansion;
pub use codec::*;
pub use domain::GrassDomain;
pub use expansion::{GrassAllocation, GrassExpansion, GrassMeshData};
use glam::{Vec2, Vec3, Vec4};

#[derive(Clone, Copy)]
pub struct GrassPatch {
    pub positions: [Vec3; 3],
    pub uvs: [Vec2; 3],
}
#[derive(Clone, Copy, Default, PartialEq)]
pub struct GrassView {
    pub position: Vec3,
    pub screen_height: f32,
    pub clip_planes: [Vec4; 4],
}
#[derive(Clone, Copy, PartialEq)]
pub struct GrassSettings {
    pub edge_length: f32,
    pub max_tessellation: u32,
    pub max_height: f32,
    pub fade_end: f32,
    pub lod_start: f32,
    pub lod_end: f32,
    pub lod_max: u32,
    pub smooth_domain: bool,
}
#[derive(Clone, Copy)]
pub struct GrassAnchor {
    pub position: Vec3,
    pub uv: Vec2,
    pub smoothing: f32,
}
impl GrassSettings {
    /// Source hull returns both REALTESS and the equal power-of-two partition factor.
    pub fn tessellation(&self, patch: &GrassPatch, view: GrassView) -> (f32, u32) {
        let nearest = patch
            .positions
            .iter()
            .map(|p| p.distance(view.position))
            .fold(f32::INFINITY, f32::min);
        if nearest >= self.fade_end {
            return (0.0, 0);
        }
        let visible = view.clip_planes.iter().all(|plane| {
            patch
                .positions
                .iter()
                .any(|position| plane.dot(position.extend(1.0)) > -self.max_height)
        });
        let mut mean = 0.0;
        if visible {
            let cap = (1u32 << self.max_tessellation) as f32;
            for (a, b) in [(1, 2), (2, 0), (0, 1)] {
                let edge = patch.positions[a].distance(patch.positions[b]);
                let distance =
                    ((patch.positions[a] + patch.positions[b]) * 0.5).distance(view.position);
                mean += (edge * view.screen_height / (distance * self.edge_length))
                    .max(1.0)
                    .min(cap);
            }
            mean *= 0.333330;
        }
        // The checked hull applies this minimum after its plane test.
        let real = mean.max(2.0);
        (real, real.log2().ceil().exp2() as u32)
    }
    pub fn segments(&self, distance: f32) -> u32 {
        ((self.lod_max as f32
            * smooth((distance - self.lod_start) / (self.lod_end - self.lod_start)))
        .max(1.0)) as u32
    }
    pub fn anchor(
        &self,
        patch: &GrassPatch,
        bary: [f32; 3],
        real: f32,
        factor: u32,
    ) -> GrassAnchor {
        let point = patch.positions[1] * bary[1]
            + patch.positions[0] * bary[0]
            + patch.positions[2] * bary[2];
        let root = hash(point.xz() + Vec2::new(8888.0, -5300.0)).sqrt();
        let second = hash(point.xz() + Vec2::new(4400.0, 2000.0));
        let weights = [1.0 - root, root * (1.0 - second), root * second];
        let smoothing = if self.smooth_domain {
            domain_smoothing(patch, point, bary, real, factor)
        } else {
            1.0
        };
        GrassAnchor {
            position: patch.positions[1] * weights[1]
                + patch.positions[0] * weights[0]
                + patch.positions[2] * weights[2],
            uv: patch.uvs[1] * weights[1] + patch.uvs[0] * weights[0] + patch.uvs[2] * weights[2],
            smoothing,
        }
    }
}
use glam::Vec3Swizzles;
fn smooth(value: f32) -> f32 {
    let t = value.max(0.0).min(1.0);
    t * t * (3.0 - 2.0 * t)
}
fn hash(point: Vec2) -> f32 {
    let value = point.dot(Vec2::new(12.9898, 78.233002)).sin() * 43758.546875;
    value - value.floor()
}
fn domain_smoothing(
    patch: &GrassPatch,
    point: Vec3,
    bary: [f32; 3],
    real: f32,
    factor: u32,
) -> f32 {
    let center = (patch.positions[0] + patch.positions[1] + patch.positions[2]) * (1.0 / 3.0);
    let relative = point - center;
    for (a, b, x, y) in [(2, 1, 1, 0), (2, 0, 0, 1), (1, 0, 0, 2)] {
        let a = patch.positions[a] - center;
        let b = patch.positions[b] - center;
        let aa = a.dot(a);
        let ab = a.dot(b);
        let bb = b.dot(b);
        let ap = a.dot(relative);
        let bp = b.dot(relative);
        let inverse = 1.0 / (aa * bb - ab * ab);
        let u = (bb * ap - ab * bp) * inverse;
        let v = (aa * bp - ab * ap) * inverse;
        if u >= -0.01 && v >= -0.01 && u + v < 1.01 {
            let x = factor as f32 * 1.5 * bary[x];
            let y = factor as f32 * 0.75 * bary[y];
            if (x - x.round_ties_even()).abs() <= 0.1 && (y - y.round_ties_even()).abs() <= 0.1 {
                return 1.0;
            }
            let half = factor as f32 * 0.5;
            let exponent = hash(point.xz() + Vec2::new(97869.0, 152.0)) * 0.5 + 0.25;
            return (((real - half) / half).log2() * exponent).exp2();
        }
    }
    0.0
}
