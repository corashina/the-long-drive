//! Source road skinning and frame calculation.
mod codec;
pub use codec::{
    decode_pose, decode_prototype, encode_pose, encode_prototype, prototype_encoded_len,
};

#[derive(Clone, Copy, Debug)]
pub struct RoadNode {
    pub parent: Option<u32>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct RoadPrototype {
    pub nodes: Vec<RoadNode>,
    pub controls: Vec<u32>,
    pub skins: Vec<RoadSkin>,
}

#[derive(Clone, Debug)]
pub struct RoadSkin {
    pub bones: Vec<u32>,
    pub bind_poses: Vec<[f32; 16]>,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uv: Vec<[f32; 2]>,
    pub joints: Vec<[u32; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub triangles: Vec<Vec<[u32; 3]>>,
}

#[derive(Clone, Copy, Debug)]
pub struct RoadPose {
    pub origin: [f64; 3],
    pub rotations: [[f32; 3]; 10],
}

#[derive(Debug, Default)]
pub struct RoadMesh {
    pub origin: [f64; 3],
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uv: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
    pub submeshes: Vec<std::ops::Range<usize>>,
}

/// Deform the admitted road skin in its authored bone and influence order.
/// Source mesh inputs already contain the Z reflection.
pub fn deform(prototype: &RoadPrototype, pose: &RoadPose) -> RoadMesh {
    use rapier3d::math::Vector;
    let frames = frames(
        prototype.nodes.iter().copied(),
        &prototype.controls,
        &pose.rotations,
    );
    let mut output = RoadMesh {
        origin: pose.origin,
        ..Default::default()
    };
    output
        .vertices
        .reserve_exact(prototype.skins.iter().map(|s| s.positions.len()).sum());
    output
        .normals
        .reserve_exact(prototype.skins.iter().map(|s| s.normals.len()).sum());
    output
        .tangents
        .reserve_exact(prototype.skins.iter().map(|s| s.tangents.len()).sum());
    output
        .uv
        .reserve_exact(prototype.skins.iter().map(|s| s.uv.len()).sum());
    output.triangles.reserve_exact(
        prototype
            .skins
            .iter()
            .flat_map(|s| &s.triangles)
            .map(Vec::len)
            .sum(),
    );
    output
        .submeshes
        .reserve_exact(prototype.skins.iter().map(|s| s.triangles.len()).sum());
    for skin in &prototype.skins {
        let base = output.vertices.len() as u32;
        output.vertices.reserve(skin.positions.len());
        output.normals.reserve(skin.positions.len());
        for index in 0..skin.positions.len() {
            let p = skin.positions[index];
            let normal = skin.normals[index];
            let tangent = skin.tangents.get(index);
            let mut point = Vector::ZERO;
            let mut n = Vector::ZERO;
            let mut t = Vector::ZERO;
            for influence in 0..4 {
                let weight = skin.weights[index][influence];
                if weight == 0.0 {
                    continue;
                }
                let bone = skin.joints[index][influence] as usize;
                let bind = skin.bind_poses[bone];
                let frame = frames[skin.bones[bone] as usize];
                let transform = |v: [f32; 3], w: f32| {
                    Vector::from_array(std::array::from_fn(|a| {
                        bind[a] * v[0] + bind[4 + a] * v[1] + bind[8 + a] * v[2] + bind[12 + a] * w
                    }))
                };
                point +=
                    (frame.position + frame.rotation * (frame.scale * transform(p, 1.0))) * weight;
                n += (frame.rotation * (frame.scale * transform(normal, 0.0))) * weight;
                if let Some(tangent) = tangent {
                    t += (frame.rotation
                        * (frame.scale * transform([tangent[0], tangent[1], tangent[2]], 0.0)))
                        * weight;
                }
            }
            output.vertices.push(point.to_array());
            output.normals.push(n.normalize_or_zero().to_array());
            if let Some(tangent) = tangent {
                let t = t.normalize_or_zero();
                output.tangents.push([t.x, t.y, t.z, tangent[3]]);
            }
        }
        output.uv.extend_from_slice(&skin.uv);
        for group in &skin.triangles {
            let first = output.triangles.len();
            output
                .triangles
                .extend(group.iter().map(|triangle| triangle.map(|i| i + base)));
            output.submeshes.push(first..output.triangles.len());
        }
    }
    output
}

#[derive(Clone, Copy)]
pub struct RoadFrame {
    pub position: rapier3d::math::Vector,
    pub rotation: rapier3d::math::Rotation,
    pub scale: rapier3d::math::Vector,
}

pub fn frames(
    nodes: impl ExactSizeIterator<Item = RoadNode>,
    controls: &[u32],
    rotations: &[[f32; 3]; 10],
) -> Vec<RoadFrame> {
    use rapier3d::math::{Rotation, Vector};
    let mut frames = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.enumerate() {
        let parent = node
            .parent
            .map(|i| frames[i as usize])
            .unwrap_or(RoadFrame {
                position: Vector::ZERO,
                rotation: Rotation::IDENTITY,
                scale: Vector::ONE,
            });
        let mut frame = if index == 0 {
            parent
        } else {
            RoadFrame {
                position: parent.position
                    + parent.rotation * (Vector::from_array(node.translation) * parent.scale),
                rotation: parent.rotation * Rotation::from_array(node.rotation),
                scale: parent.scale * Vector::from_array(node.scale),
            }
        };
        if let Some(control) = controls.iter().position(|&i| i as usize == index) {
            let angles = rotations[control];
            let axes = [
                -(parent.rotation * Vector::X),
                -Vector::Y,
                -(parent.rotation * Vector::Y),
            ];
            for (angle, axis) in angles.into_iter().zip(axes) {
                let (sin, cos) = half_angle(angle);
                let axis = axis.normalize() * sin;
                frame.rotation = Rotation::from_xyzw(axis.x, axis.y, axis.z, cos) * frame.rotation;
            }
        }
        frames.push(frame);
    }
    frames
}

// UnityPlayer's source mode-one finite-angle polynomials. Road angles stay in [-90, 90].
fn sin_poly(x: f64) -> f64 {
    let square = x * x;
    let p = 2.7557319223985893e-6_f64.mul_add(square, -0.0001984126984126984);
    let p = p
        .mul_add(square, 0.008333333333333333)
        .mul_add(square, -0.16666666666666666);
    p.mul_add(x * square, x)
}
fn cos_poly(x: f64, reduced_sine: bool) -> f64 {
    let square = x * x;
    let base = if reduced_sine {
        (-0.5_f64).mul_add(square, 1.0)
    } else {
        1.0 - square * 0.5
    };
    let p = (-2.755731922398589e-7_f64).mul_add(square, 2.4801587301587298e-5);
    let p = p
        .mul_add(square, -0.0013888888888888887)
        .mul_add(square, 0.041666666666666664);
    p.mul_add(square * square, base)
}
pub fn half_angle(degrees: f32) -> (f32, f32) {
    let radians = (degrees as f64 * 0.0174532924_f32 as f64) as f32;
    let angle = radians * 0.5;
    let x = angle as f64;
    let abs = x.abs();
    if abs > std::f64::consts::FRAC_PI_4 {
        // Only the first quadrant reduction can occur in the checked road domain.
        let reduced = (-1.0_f64).mul_add(1.5707963267341256, abs) - 6.077100506506192e-11;
        return (
            (cos_poly(reduced, true).copysign(x)) as f32,
            (-sin_poly(reduced)) as f32,
        );
    }
    if abs < 0.0001220703125 {
        return (angle, 1.0);
    }
    if abs < 0.0078125 {
        return (
            (-(x * x * x)).mul_add(0.16666666666666666, x) as f32,
            (-(x * 0.5)).mul_add(x, 1.0) as f32,
        );
    }
    (sin_poly(x) as f32, cos_poly(x, false) as f32)
}

impl RoadPrototype {
    pub fn retained_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<RoadNode>()
            + self.controls.capacity() * 4
            + self.skins.capacity() * std::mem::size_of::<RoadSkin>()
            + self
                .skins
                .iter()
                .map(|s| {
                    s.bones.capacity() * 4
                        + s.bind_poses.capacity() * 64
                        + s.positions.capacity() * 12
                        + s.normals.capacity() * 12
                        + s.tangents.capacity() * 16
                        + s.uv.capacity() * 8
                        + s.joints.capacity() * 16
                        + s.weights.capacity() * 16
                        + s.triangles.capacity() * std::mem::size_of::<Vec<[u32; 3]>>()
                        + s.triangles
                            .iter()
                            .map(|group| group.capacity() * 12)
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
}
