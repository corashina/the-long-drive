//! Source `movecreaturescript`: `Move`, `Rot`, `Jump` and `CheckGround`.
use rapier3d::math::{Rotation, Vector};

/// Source `Move`. The horizontal velocity lerps to `move_dir * speed` times `modifier`. The
/// vertical part stays. The source lerp starts from `(v.x, 0, v.y)`. Unity z takes the vertical
/// speed.
pub fn velocity(
    v: Vector,
    move_dir: Vector,
    speed: f32,
    modifier: f32,
    acceleration: f32,
    dt: f32,
) -> Vector {
    let goal = Vector::new(move_dir.x, 0., move_dir.z) * speed;
    let goal = goal.clamp_length_max(speed) * modifier;
    // Converted z is Unity -z.
    let start = Vector::new(v.x, 0., -v.y);
    start.lerp(goal, (acceleration * dt).clamp(0., 1.)) + Vector::new(0., v.y, 0.)
}

/// Source `Rot`: `transform.Rotate(Vector3.up, clamp(moveRot) * dt)`.
/// Positive Unity degrees turn right, which is a negative converted yaw.
pub fn turn(rotation: Rotation, rot: f32, rot_speed: f32, dt: f32) -> Rotation {
    let degrees = rot.clamp(-rot_speed, rot_speed) * dt;
    (rotation * Rotation::from_rotation_y(-degrees.to_radians())).normalize()
}
