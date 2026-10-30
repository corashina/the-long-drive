//! Source `carscript.Balance()`: the roll PID that holds a bike upright.

use crate::reference::BalanceReference;
use crate::simulation::dials::{inverse_lerp, lerp};
use crate::simulation::engine::torque as evaluate;
use rapier3d::math::Vector;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BalanceState {
    pub actual_set_angle: f32,
    pub lean_angle: f32,
    pub integral: f32,
    pub last_error: f32,
}

/// Unity `Mathf.Sign`: 0 gives 1.
fn sign(v: f32) -> f32 {
    if v >= 0. { 1. } else { -1. }
}

/// Unity `Vector3.SignedAngle` in degrees, in source (left-handed) coordinates.
fn signed_angle(from: Vector, to: Vector, axis: Vector) -> f32 {
    let n = (from.length_squared() * to.length_squared()).sqrt();
    if n < 1e-15 {
        return 0.;
    }
    let angle = (from.dot(to) / n).clamp(-1., 1.).acos().to_degrees();
    angle * sign(axis.dot(from.cross(to)))
}

/// Converted (right-handed) direction back to source coordinates.
fn source(v: Vector) -> Vector {
    Vector::new(v.x, v.y, -v.z)
}

/// `speed`: source km/h, `|project(velocity, forward)| * 3.6`. `steer`: -1..1.
/// `up` and `forward`: chassis axes in converted world space.
/// Returns the torque in converted chassis-local axes. Source `AddRelativeTorque(-(0, 0, z))`
/// keeps its z under handedness conversion (axial vector: z negates twice).
pub fn step(
    r: &BalanceReference,
    s: &mut BalanceState,
    speed: f32,
    steer: f32,
    up: Vector,
    forward: Vector,
    dt: f32,
) -> Vector {
    if dt == 0. {
        return Vector::ZERO;
    }
    let target = r.set_angle
        + lerp(
            r.steer_angle_min,
            r.steer_angle_max,
            inverse_lerp(r.steer_angle_speed_min, r.steer_angle_speed_max, speed),
        ) * steer;
    s.actual_set_angle = lerp(s.actual_set_angle, target, dt * r.steer_angle_lerp);
    let mut current = signed_angle(source(up), Vector::Y, source(forward));
    if r.lean {
        let lean = lerp(
            r.lean_min_angle,
            r.lean_max_angle,
            inverse_lerp(r.lean_min_speed, r.lean_max_speed, speed),
        ) * sign(steer)
            * evaluate(&r.lean_curve, steer.abs());
        s.lean_angle = lerp(s.lean_angle, lean, r.lean_angle_lerp * dt);
        current += s.lean_angle;
    }
    let mut output = 0.;
    if (current > r.min_angle && current < r.max_angle) || !r.use_angle_limit {
        let e = s.actual_set_angle - current;
        s.integral += e * dt;
        let derivative = (e - s.last_error) / dt;
        s.last_error = e;
        output = e * r.p + s.integral * r.i + derivative * r.d;
    }
    Vector::new(0., 0., -output.clamp(-r.max_force, r.max_force))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::reference::{CurveKey, TorqueCurve};
    use rapier3d::math::Rotation;

    const DT: f32 = 0.02;
    /// Converted forward is -Z.
    const FORWARD: Vector = Vector::NEG_Z;

    pub fn reference() -> BalanceReference {
        let key = |time, value| CurveKey {
            time,
            value,
            in_tangent: 0.,
            out_tangent: 0.,
            in_weight: 0.,
            out_weight: 0.,
            weighted_mode: 0,
        };
        BalanceReference {
            lean: false,
            lean_min_speed: 0.,
            lean_max_speed: 50.,
            lean_min_angle: 5.,
            lean_max_angle: 20.,
            lean_angle_lerp: 5.,
            lean_curve: TorqueCurve {
                keys: vec![key(0., 0.), key(1., 1.)],
                pre_wrap_mode: 8,
                post_wrap_mode: 8,
            },
            steer_angle_min: 10.,
            steer_angle_max: 10.,
            steer_angle_speed_min: 0.,
            steer_angle_speed_max: 50.,
            steer_angle_lerp: 1000.,
            p: 1.,
            i: 0.,
            d: 0.,
            set_angle: 0.,
            use_angle_limit: true,
            min_angle: -45.,
            max_angle: 45.,
            max_force: 1000.,
        }
    }

    /// Chassis rolled about its forward axis.
    fn rolled(degrees: f32) -> Rotation {
        Rotation::from_axis_angle(FORWARD, degrees.to_radians())
    }

    fn run(r: &BalanceReference, rot: Rotation, steer: f32) -> Vector {
        step(
            r,
            &mut BalanceState::default(),
            0.,
            steer,
            rot * Vector::Y,
            rot * FORWARD,
            DT,
        )
    }

    /// Tilt of up from world up after a small turn along the world torque.
    fn tilt_after(rot: Rotation, local: Vector) -> f32 {
        let world = rot * local;
        let turned = Rotation::from_scaled_axis(world.normalize_or_zero() * 0.01) * rot;
        (turned * Vector::Y).angle_between(Vector::Y)
    }

    #[test]
    fn upright_gives_zero() {
        assert!(run(&reference(), Rotation::IDENTITY, 0.).length() < 1e-3);
    }

    #[test]
    fn tilt_restores() {
        let r = reference();
        for deg in [-20., 20.] {
            let rot = rolled(deg);
            let t = run(&r, rot, 0.);
            assert!(t.z.abs() > 1.);
            assert!(tilt_after(rot, t) < (rot * Vector::Y).angle_between(Vector::Y));
        }
    }

    #[test]
    fn steer_right_leans_right() {
        let r = reference();
        let right = run(&r, Rotation::IDENTITY, 1.);
        // Up tips toward converted +X (right) after the torque.
        let up = Rotation::from_scaled_axis(right.normalize() * 0.01) * Vector::Y;
        assert!(up.x > 0.);
        let left = run(&r, Rotation::IDENTITY, -1.);
        let up = Rotation::from_scaled_axis(left.normalize() * 0.01) * Vector::Y;
        assert!(up.x < 0.);

        let mut lean = reference();
        lean.lean = true;
        for (steer, expected) in [(1., 1.), (-1., -1.)] {
            let mut s = BalanceState::default();
            step(&lean, &mut s, 0., steer, Vector::Y, FORWARD, DT);
            assert_eq!(s.lean_angle.signum(), expected);
        }
    }

    #[test]
    fn outside_limit_gives_zero() {
        let r = reference();
        assert_eq!(run(&r, rolled(60.), 0.), Vector::ZERO);
        let mut free = r;
        free.use_angle_limit = false;
        assert!(run(&free, rolled(60.), 0.).z.abs() > 1.);
    }

    #[test]
    fn output_clamps() {
        let mut r = reference();
        r.max_force = 3.;
        assert_eq!(run(&r, rolled(30.), 0.).z.abs(), 3.);
    }

    #[test]
    fn zero_dt_gives_zero() {
        let mut s = BalanceState {
            integral: 2.,
            ..Default::default()
        };
        let t = step(
            &reference(),
            &mut s,
            0.,
            1.,
            rolled(20.) * Vector::Y,
            FORWARD,
            0.,
        );
        assert_eq!(t, Vector::ZERO);
        assert_eq!(s.integral, 2.);
    }
}
