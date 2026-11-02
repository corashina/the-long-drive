//! Source needle (`mutatoscript`), digit wheel (`forgoszamlap`), compass and clock maths.
use rapier3d::math::{Rotation, Vector};

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}
/// Unity `Mathf.LerpAngle`.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut delta = (b - a).rem_euclid(360.);
    if delta > 180. {
        delta -= 360.;
    }
    a + delta * t.clamp(0., 1.)
}
/// Unity `Mathf.InverseLerp`.
pub fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((value - a) / (b - a)).clamp(0., 1.)
    }
}

/// Source digit wheel angle: units turn freely, higher places roll over with the units.
pub fn digit_angle(values: &[f32], index: usize, continuous: bool, base: f32) -> f32 {
    let step = 360. / base;
    if index == 0 || continuous {
        values[index] * step
    } else if values[..index].iter().all(|v| *v >= base - 1.) {
        let floor = values[index].trunc();
        lerp_angle(
            floor * step,
            (floor + 1.) * step,
            inverse_lerp(base - 1., base, values[0]),
        )
    } else {
        values[index].trunc() * step
    }
}

/// Unity `Vector3.SignedAngle` in degrees.
pub fn signed_angle(from: Vector, to: Vector, axis: Vector) -> f32 {
    let (from, to) = (from.normalize_or_zero(), to.normalize_or_zero());
    let angle = from.dot(to).clamp(-1., 1.).acos().to_degrees();
    if axis.dot(from.cross(to)) < 0. {
        -angle
    } else {
        angle
    }
}

/// `compassscript.GetNorthValue` from converted vectors. The mirrored Z flips the source sign.
pub fn compass_north(forward: Vector, north: Vector) -> f32 {
    let flat = Vector::new(forward.x, 0., forward.z);
    -signed_angle(flat, Vector::new(north.x, 0., north.z), Vector::Y)
}

/// `compassscript.GetNorthValue2`: a jammed compass points at the last EMP, then spins.
/// `spin` keeps the source `jam` turn.
pub fn compass_value(
    forward: Vector,
    north: Vector,
    position: Vector,
    electronics: Option<&crate::electronics::Electronics>,
    spin: &mut f32,
    dt: f32,
) -> f32 {
    let Some(e) = electronics.filter(|e| e.jam >= 0.05) else {
        return compass_north(forward, north);
    };
    if e.jam < 0.75
        && let Some(emp) = e.last_emp
    {
        return -compass_north(forward, emp - position);
    }
    *spin -= dt * 360. * e.jam;
    if *spin < -180. {
        *spin = 180.;
    }
    *spin
}

/// Source `localEulerAngles` in degrees as a converted rotation.
pub fn unity_euler(degrees: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = degrees.map(f32::to_radians);
    let q =
        Rotation::from_rotation_y(y) * Rotation::from_rotation_x(x) * Rotation::from_rotation_z(z);
    [-q.x, -q.y, q.z, q.w]
}

/// `clockscript`: dial turn in degrees for an offset and the cycle fraction.
pub fn clock_degrees(offset: f32, cycle_fraction: f32) -> f32 {
    (offset + CLOCK_ADD + cycle_fraction).rem_euclid(1.) * 360.
}
/// `clockscript.add`.
pub const CLOCK_ADD: f32 = 0.0625;
