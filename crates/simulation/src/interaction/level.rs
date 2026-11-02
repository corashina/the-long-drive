//! bubbleLevelScript: each spirit level bubble follows the vial tilt.
use crate::reference::{BubbleReference, ReferenceError, Transform, WorldPrototypeReference};
use crate::simulation::engine::torque;
use rapier3d::math::{Matrix, Rotation, Vector};

/// Fixed vial geometry of one bubble.
#[derive(Clone, Copy, Debug)]
pub struct Vial {
    /// t1 and t2 in the item node space.
    t1: Vector,
    t2: Vector,
    /// t1 and t2 in the bubble parent space.
    local_t1: Vector,
    local_t2: Vector,
    /// Item node space to bubble parent space, for directions.
    to_local: Matrix,
}

/// Map from `node` space to `anchor` space, as a linear part and an offset.
fn chain(
    prototype: &WorldPrototypeReference,
    node: usize,
    anchor: usize,
) -> Result<(Matrix, Vector), ReferenceError> {
    let (mut linear, mut offset) = (Matrix::IDENTITY, Vector::ZERO);
    let mut current = node;
    while current != anchor {
        let n = &prototype.nodes[current];
        let t = n.transform;
        let m = Matrix::from_quat(Rotation::from_array(t.rotation))
            * Matrix::from_diagonal(Vector::from_array(t.scale));
        linear = m * linear;
        offset = Vector::from_array(t.translation) + m * offset;
        current = n
            .parent
            .ok_or_else(|| ReferenceError("Spirit level node outside its item".into()))?
            as usize;
    }
    Ok((linear, offset))
}

fn descends(prototype: &WorldPrototypeReference, node: usize, ancestor: usize) -> bool {
    let mut current = Some(node);
    while let Some(i) = current {
        if i == ancestor {
            return true;
        }
        current = prototype.nodes[i].parent.map(|p| p as usize);
    }
    false
}

impl Vial {
    /// `anchor` is the item node. The other indices are prototype nodes.
    pub fn new(
        prototype: &WorldPrototypeReference,
        anchor: usize,
        bubble: usize,
        t1: usize,
        t2: usize,
    ) -> Result<Self, ReferenceError> {
        if bubble == anchor || descends(prototype, t1, bubble) || descends(prototype, t2, bubble) {
            return Err(ReferenceError("Unsupported spirit level bubble".into()));
        }
        let point = |n| chain(prototype, n, anchor).map(|(_, offset)| offset);
        let parent = prototype.nodes[bubble]
            .parent
            .ok_or_else(|| ReferenceError("Spirit level bubble has no parent".into()))?;
        let (linear, offset) = chain(prototype, parent as usize, anchor)?;
        if linear.determinant() == 0. {
            return Err(ReferenceError(
                "Spirit level bubble parent has zero scale".into(),
            ));
        }
        let to_local = linear.inverse();
        let (t1, t2) = (point(t1)?, point(t2)?);
        Ok(Self {
            t1,
            t2,
            local_t1: to_local * (t1 - offset),
            local_t2: to_local * (t2 - offset),
            to_local,
        })
    }
}

pub fn inverse_lerp(a: f32, b: f32, v: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((v - a) / (b - a)).clamp(0., 1.)
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}

/// Unity `Vector3.normalized`.
fn normalized(v: Vector) -> Vector {
    let length = v.length();
    if length > 1e-5 {
        v / length
    } else {
        Vector::ZERO
    }
}

/// One Update: GetValue, then Upd. `rotation` and `scale` place the item node in the world.
/// Returns the bubble local translation and local scale.
pub fn step(
    value: &mut f32,
    b: &BubbleReference,
    vial: &Vial,
    rotation: Rotation,
    scale: Vector,
    dt: f32,
) -> ([f32; 3], [f32; 3]) {
    let axis = normalized(rotation * (scale * (vial.t2 - vial.t1)));
    let tilt = torque(&b.bubble_curve, inverse_lerp(-1., 1., axis.dot(Vector::Y)));
    *value = lerp(*value, tilt, b.lerp * dt);
    let v = *value;
    // Vector3.ProjectOnPlane(Vector3.up, axis).
    let up = if axis.length_squared() < f32::EPSILON {
        Vector::Y
    } else {
        Vector::Y - axis * axis.dot(Vector::Y) / axis.length_squared()
    };
    let offset = up * b.move_xy * torque(&b.move_xy_curve, v);
    let offset = (rotation.inverse() * offset) / scale;
    let position = vial.local_t1.lerp(vial.local_t2, v.clamp(0., 1.)) + vial.to_local * offset;
    let xy = lerp(b.min_scale_xy, b.max_scale_xy, torque(&b.scale_xy_curve, v));
    let z = lerp(b.min_scale_z, b.max_scale_z, torque(&b.scale_z_curve, v));
    (position.to_array(), [xy, xy, z])
}

/// The bubble node pose with its source rotation kept.
pub fn pose(rest: Transform, (translation, scale): ([f32; 3], [f32; 3])) -> Transform {
    Transform {
        translation,
        rotation: rest.rotation,
        scale,
    }
}
