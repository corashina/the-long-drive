//! Source `szankoScript`: sled use state, input, slide forces and slide sound.
use crate::simulation::dials::{inverse_lerp, lerp};
use rapier3d::math::Vector;

/// Source `szankoScript` values. The binding maps the sled reference into this.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SledParams {
    pub slide_above_mass: f32,
    pub move_force: f32,
    pub max_speed: f32,
    pub velocity_forward_lerp: f32,
    pub turn_force: f32,
    pub angular_velocity_lerp: f32,
    pub sound_min_speed: f32,
    pub sound_max_speed: f32,
    pub sound_max_volume: f32,
    pub sound_pitch_min_speed: f32,
    pub sound_pitch_max_speed: f32,
    pub sound_min_pitch: f32,
    pub sound_max_pitch: f32,
}

/// In use when a seat is occupied, or when the mass is above the slide mass.
pub fn in_use(seated: bool, mass_kg: f32, slide_above_mass: f32) -> bool {
    seated || mass_kg > slide_above_mass
}

/// -1 when the sled moves backward above 0.2 m/s, else 1.
pub fn direction(forward: Vector, velocity: Vector) -> f32 {
    if forward.dot(velocity) < 0. && velocity.length() > 0.2 {
        -1.
    } else {
        1.
    }
}

/// Returns `(move, turn)`. Not seated (load only) gives no input.
pub fn input(throttle: f32, brake: f32, steer: f32, direction: f32, seated: bool) -> (f32, f32) {
    if !seated {
        return (0., 0.);
    }
    let turn = (steer * 10.).clamp(-1., 1.);
    (throttle - brake, if direction < 0. { -turn } else { turn })
}

/// Push force. It fades to 0 at `max_speed * move`.
pub fn forward_force(
    forward: Vector,
    move_: f32,
    move_force: f32,
    max_speed: f32,
    speed: f32,
    direction: f32,
) -> Vector {
    forward * (move_ * move_force * inverse_lerp(max_speed * move_, 0., speed * direction))
}

/// Unity `Vector3.Project`.
fn project(v: Vector, normal: Vector) -> Vector {
    let sqr = normal.length_squared();
    if sqr < f32::MIN_POSITIVE {
        Vector::ZERO
    } else {
        normal * (v.dot(normal) / sqr)
    }
}

/// Unity `Vector3.ProjectOnPlane`.
fn project_on_plane(v: Vector, normal: Vector) -> Vector {
    let sqr = normal.length_squared();
    if sqr < f32::MIN_POSITIVE {
        v
    } else {
        v - normal * (v.dot(normal) / sqr)
    }
}

/// Velocity lerp toward flat forward. Side speed goes, vertical speed stays.
pub fn velocity_after_lerp(velocity: Vector, forward: Vector, t: f32) -> Vector {
    let horizontal = project_on_plane(velocity, Vector::Y);
    let vertical = project(velocity, Vector::Y);
    let target = project(horizontal, project_on_plane(forward, Vector::Y)) + vertical;
    velocity.lerp(target, t.clamp(0., 1.))
}

/// Angular velocity lerp toward a yaw rate of `turn_force * turn`.
pub fn angular_after_lerp(angular: Vector, turn_force: f32, turn: f32, t: f32) -> Vector {
    angular.lerp(Vector::new(0., turn_force * turn, 0.), t.clamp(0., 1.))
}

/// Returns `(volume, pitch)`. In the air the volume is 0 and the pitch is `None`.
/// The source does not set the pitch then, so the caller keeps the last pitch.
pub fn sound(speed: f32, grounded: bool, params: &SledParams) -> (f32, Option<f32>) {
    if !grounded {
        return (0., None);
    }
    let volume = inverse_lerp(params.sound_min_speed, params.sound_max_speed, speed)
        * params.sound_max_volume;
    let pitch = lerp(
        params.sound_min_pitch,
        params.sound_max_pitch,
        inverse_lerp(
            params.sound_pitch_min_speed,
            params.sound_pitch_max_speed,
            speed,
        ),
    );
    (volume, Some(pitch))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORWARD: Vector = Vector::Z;

    #[test]
    fn in_use_by_seat_and_by_mass() {
        assert!(in_use(true, 20., 50.));
        assert!(!in_use(false, 20., 50.));
        assert!(!in_use(false, 50., 50.));
        assert!(in_use(false, 51., 50.));
    }

    #[test]
    fn force_fades_to_zero_at_top_speed() {
        let full = forward_force(FORWARD, 1., 100., 10., 0., 1.);
        assert!((full.z - 100.).abs() < 1e-4);
        let half = forward_force(FORWARD, 1., 100., 10., 5., 1.);
        assert!((half.z - 50.).abs() < 1e-4);
        assert_eq!(forward_force(FORWARD, 1., 100., 10., 10., 1.), Vector::ZERO);
        assert_eq!(forward_force(FORWARD, 1., 100., 10., 15., 1.), Vector::ZERO);
        assert_eq!(forward_force(FORWARD, 0., 100., 10., 0., 1.), Vector::ZERO);
    }

    #[test]
    fn backward_turn_reverses_above_threshold() {
        let slow = direction(FORWARD, Vector::new(0., 0., -0.1));
        let back = direction(FORWARD, Vector::new(0., 0., -1.));
        assert_eq!(slow, 1.);
        assert_eq!(back, -1.);
        assert_eq!(input(1., 0., 0.05, slow, true), (1., 0.5));
        assert_eq!(input(1., 0., 0.05, back, true), (1., -0.5));
        assert_eq!(input(1., 0., 1., back, false), (0., 0.));
    }

    #[test]
    fn velocity_lerp_removes_side_speed_and_keeps_vertical() {
        let velocity = Vector::new(3., -2., 4.);
        let after = velocity_after_lerp(velocity, FORWARD, 1.);
        assert!((after - Vector::new(0., -2., 4.)).length() < 1e-5);
        assert_eq!(velocity_after_lerp(velocity, FORWARD, 0.), velocity);
        let spin = angular_after_lerp(Vector::new(1., 2., 3.), 4., 0.5, 1.);
        assert!((spin - Vector::new(0., 2., 0.)).length() < 1e-5);
    }

    #[test]
    fn sound_silent_in_air_and_scales_with_speed() {
        let params = SledParams {
            sound_min_speed: 1.,
            sound_max_speed: 5.,
            sound_max_volume: 0.8,
            sound_pitch_min_speed: 1.,
            sound_pitch_max_speed: 5.,
            sound_min_pitch: 0.5,
            sound_max_pitch: 1.5,
            ..SledParams::default()
        };
        assert_eq!(sound(3., false, &params), (0., None));
        let (slow, _) = sound(2., true, &params);
        let (fast, pitch) = sound(5., true, &params);
        assert!(slow < fast);
        assert!((fast - 0.8).abs() < 1e-6);
        assert!((pitch.unwrap() - 1.5).abs() < 1e-6);
    }
}
