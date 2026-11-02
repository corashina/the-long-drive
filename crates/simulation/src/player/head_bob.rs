//! Source `fpscontroller` walk head bob and `playeranimationscript.CarHeadBob`.
use crate::reference::{PlayerPoseReference, PlayerReference};
use rapier3d::math::{Rotation, Vector};

/// The offset holds its last value when the player stops.
#[derive(Clone, Copy, Debug)]
pub(super) struct WalkBob {
    speed: f32,
    speed_rb: f32,
    max: f32,
    threshold: f32,
    state: f32,
    pub(super) offset: f32,
    wdown: bool,
    /// True on the step a source footstep fires.
    pub(super) beat: bool,
    /// Source `currentAdd`: the state step. 0 when the bob does not advance.
    pub(super) add: f32,
}
impl WalkBob {
    pub(super) fn new(player: &PlayerReference) -> Self {
        Self {
            speed: player.walk_bob_speed,
            speed_rb: player.walk_bob_speed_rb,
            max: player.max_walk_bob,
            threshold: player.walk_bob_sound_threshold,
            state: 0.,
            offset: 0.,
            wdown: false,
            beat: false,
            add: 0.,
        }
    }
    /// `body_speed` is the body speed minus the support body speed.
    pub(super) fn step(&mut self, active: bool, body_speed: f32, dt: f32) {
        self.beat = false;
        self.add = 0.;
        if !active {
            return;
        }
        self.add = dt * self.speed + body_speed * self.speed_rb * dt;
        self.state += self.add;
        self.offset = self.state.sin() * self.max;
        if self.offset <= -self.threshold {
            self.beat = !self.wdown;
            self.wdown = true;
        } else {
            self.wdown = false;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CarHeadBob {
    back: f32,
    back_lerp: f32,
    forward_lerp: f32,
    max: [f32; 3],
    car_v: Vector,
    head_v: Vector,
}
impl CarHeadBob {
    pub(super) fn new(pose: &PlayerPoseReference) -> Self {
        let scalar = |name: &str| pose.animation_scalars[name];
        Self {
            back: scalar("headBackM"),
            back_lerp: scalar("headBackLerp"),
            forward_lerp: scalar("headForwardLerp"),
            max: [
                scalar("maxHeadBobX"),
                scalar("maxHeadBobY"),
                scalar("maxHeadBobZ"),
            ],
            car_v: Vector::ZERO,
            head_v: Vector::ZERO,
        }
    }
    pub(super) fn reset(&mut self) {
        self.car_v = Vector::ZERO;
        self.head_v = Vector::ZERO;
    }
    /// Returns a world offset. `head` gives the right, up, and forward clamp axes.
    pub(super) fn step(
        &mut self,
        dt: f32,
        parent_velocity: Vector,
        dead: bool,
        head: Rotation,
    ) -> Vector {
        if !dead {
            self.car_v = self.car_v.lerp(
                -parent_velocity * self.back,
                (self.back_lerp * dt).clamp(0., 1.),
            );
            self.head_v = self
                .head_v
                .lerp(-self.car_v, (self.forward_lerp * dt).clamp(0., 1.));
        }
        let bob = self.car_v + self.head_v;
        let mut offset = Vector::ZERO;
        for (axis, max) in [Vector::X, Vector::Y, Vector::Z].into_iter().zip(self.max) {
            let axis = head * axis;
            offset += axis * bob.dot(axis).clamp(-max, max);
        }
        offset
    }
}
