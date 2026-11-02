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
