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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jammed_compass_points_at_the_emp_then_spins() {
        let mut e = crate::electronics::Electronics::new(1.);
        let mut spin = 0.;
        let (north, at) = (Vector::NEG_Z, Vector::ZERO);
        let value = |e: &crate::electronics::Electronics, spin: &mut f32| {
            compass_value(Vector::NEG_Z, north, at, Some(e), spin, 0.5)
        };
        assert_eq!(value(&e, &mut spin), 0.);
        e.jam = 0.5;
        e.last_emp = Some(Vector::X * 10.);
        // Source: -SignedAngle(forward, toEmp). The EMP at source +X reads -90.
        assert!((value(&e, &mut spin) + 90.).abs() < 1e-3);
        e.jam = 1.;
        assert_eq!(value(&e, &mut spin), -180.);
        assert_eq!(value(&e, &mut spin), 180.);
    }

    #[test]
    fn compass_reads_the_signed_heading() {
        // Source forward and north are converted -Z. Facing source +X reads -90.
        let north = Vector::NEG_Z;
        assert_eq!(compass_north(Vector::NEG_Z, north), 0.);
        assert!((compass_north(Vector::X, north) + 90.).abs() < 1e-4);
        assert!((compass_north(Vector::new(-1., 0.3, 0.), north) - 90.).abs() < 1e-3);
    }

    #[test]
    fn euler_angles_convert_like_source_nodes() {
        // Source Y and X turns mirror; Z turns keep their sign.
        let y = unity_euler([0., 90., 0.]);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((y[1] + half).abs() < 1e-6 && (y[3] - half).abs() < 1e-6);
        let z = unity_euler([0., 0., 90.]);
        assert!((z[2] - half).abs() < 1e-6);
    }

    #[test]
    fn clock_hands_follow_the_cycle() {
        // The source multiplies this turn by 2, 24 and 1440 for the hour, minute and second hand.
        assert_eq!(clock_degrees(0., 0.), 22.5);
        assert!((clock_degrees(0., 0.9375) - 0.).abs() < 1e-3);
        assert!((clock_degrees(-0.5, 0.25) - 292.5).abs() < 1e-3);
    }

    #[test]
    fn digit_wheels_roll_over() {
        let values = [9.5, 3.2];
        assert_eq!(digit_angle(&values, 0, false, 10.), 342.);
        assert_eq!(digit_angle(&values, 1, false, 10.), 126.);
        assert_eq!(digit_angle(&[8., 3.2], 1, false, 10.), 108.);
    }
}
