//! Source `ladderscript` contact countdown and `fpscontroller.LadderMove`.
use crate::simulation::{FrameInput, keys};
use rapier3d::math::Vector;
use rapier3d::prelude::ColliderHandle;

const CONTACT_STEPS: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LadderContact {
    pub collider: ColliderHandle,
    pub speed: f32,
    /// World space.
    pub up: Vector,
}

/// Source `ladderV`, `TLadder` and `releasedJump`.
#[derive(Clone, Copy, Debug)]
pub(super) struct LadderState {
    countdown: u8,
    ladder: Option<LadderContact>,
    active: bool,
    jump_released: bool,
    climbed: bool,
}
impl Default for LadderState {
    fn default() -> Self {
        Self {
            countdown: 0,
            ladder: None,
            active: false,
            jump_released: true,
            climbed: false,
        }
    }
}
impl LadderState {
    /// Source `OnTriggerStay`. Keeps the first ladder and refreshes its pose.
    pub(super) fn touch(&mut self, contact: LadderContact) {
        self.countdown = CONTACT_STEPS;
        if self.ladder.is_none_or(|l| l.collider == contact.collider) {
            self.ladder = Some(contact);
        }
    }
    /// One player step on foot.
    pub(super) fn tick(&mut self, input: FrameInput) {
        if input.released & keys::JUMP != 0 {
            self.jump_released = true;
        }
        self.active = self.countdown > 0;
        if self.active {
            self.countdown -= 1;
        } else {
            self.ladder = None;
        }
    }
    pub(super) fn ladder(&self) -> Option<LadderContact> {
        self.ladder.filter(|_| self.active)
    }
    /// The last `climb_velocity` call climbed.
    pub(super) fn climbed(&self) -> bool {
        self.climbed
    }
    /// Source `useGravity`.
    pub(super) fn gravity(&self) -> bool {
        self.ladder().is_none() || !self.jump_released
    }
    /// Source `LadderMove`. `None` walks normally. `speed` is `FladderSpeed`.
    pub(super) fn climb_velocity(
        &mut self,
        input: FrameInput,
        forward: Vector,
        right: Vector,
        grounded: bool,
        speed: f32,
        run_multiplier: Option<f32>,
    ) -> Option<Vector> {
        self.climbed = false;
        let ladder = self.ladder()?;
        let angle = forward.angle_between(ladder.up).to_degrees();
        if (angle > 70.0 && grounded) || !self.jump_released {
            return None;
        }
        let up = if angle < 120.0 { ladder.up } else { -ladder.up };
        let mut sum = Vector::ZERO;
        let mut vertical = Vector::ZERO;
        if input.held & keys::FORWARD != 0 {
            sum += forward;
            vertical = up;
        }
        if input.held & keys::BACK != 0 {
            vertical = -up;
        }
        if input.held & keys::LEFT != 0 {
            sum -= right;
        }
        if input.held & keys::RIGHT != 0 {
            sum += right;
        }
        sum += vertical.normalize_or_zero();
        if input.pressed & keys::JUMP != 0 {
            self.jump_released = false;
            return None;
        }
        self.climbed = true;
        Some(sum * speed * ladder.speed * run_multiplier.unwrap_or(1.0))
    }
}
