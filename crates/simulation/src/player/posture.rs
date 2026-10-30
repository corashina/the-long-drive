//! Source duck transitions, jump order and retained ordered clearance contacts.
use super::{
    sensor::SensorContact,
    wall_jump::{WallJump, WallJumpState},
};
use crate::{
    reference::PlayerPostureReference,
    simulation::{FrameInput, keys},
};

/// Source jump force multipliers.
#[derive(Clone, Copy)]
pub(super) struct JumpForces {
    pub(super) duck: f32,
    pub(super) inverse_duck: f32,
    pub(super) wall: f32,
    pub(super) ledge: f32,
}

pub(super) struct PostureState {
    value: f32,
    last_ducked: bool,
    rising: bool,
    space_duck: bool,
    duck_jumped: bool,
    can_duck_jump: bool,
    contacts: [SensorContact; 10],
}

impl PostureState {
    pub(super) fn new(value: f32) -> Self {
        Self {
            value,
            last_ducked: false,
            rising: false,
            space_duck: false,
            duck_jumped: false,
            can_duck_jump: false,
            contacts: [SensorContact::default(); 10],
        }
    }
    pub(super) fn value(&self) -> f32 {
        self.value
    }
    pub(super) fn restore(&mut self, value: f32) {
        *self = Self::new(value);
    }
    pub(super) fn contacts(&mut self, hits: [bool; 10], hold: u8) {
        for (contact, hit) in self.contacts.iter_mut().zip(hits) {
            contact.step(hit, hold);
        }
    }
    fn grow(&self, amount: f32, reference: &PlayerPostureReference) -> f32 {
        let mut value = 0.0;
        for (sensor, contact) in reference.clearance.iter().zip(self.contacts) {
            if contact.sensed() {
                break;
            }
            value = amount.clamp(0.0, (sensor.posture - self.value).max(0.0));
        }
        value.clamp(0.0, 1.0)
    }
    /// Source `Grip` flags.
    fn grip(&mut self, wall: &mut WallJumpState) {
        if wall.grip() {
            self.duck_jumped = false;
            self.can_duck_jump = true;
        }
    }
    /// Returns the source force multiplier for this fixed step.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step(
        &mut self,
        input: FrameInput,
        wheel_allowed: bool,
        grounded: bool,
        wall: &mut WallJumpState,
        reference: &PlayerPostureReference,
        dt: f32,
        forces: JumpForces,
    ) -> f32 {
        if grounded {
            self.can_duck_jump = true;
            self.duck_jumped = false;
            wall.grounded();
        }
        if wall.hold(input.held & keys::JUMP != 0) {
            self.grip(wall);
        }
        if input.released & keys::CROUCH != 0 && self.last_ducked {
            self.rising = true;
        }
        if input.pressed & keys::CROUCH != 0 {
            self.last_ducked = true;
        }
        let mut duck = input.held & keys::CROUCH != 0 || self.space_duck;
        if input.held & keys::CROUCH != 0 {
            self.rising = false;
        }
        let space_released = input.held & keys::JUMP == 0 && self.space_duck;
        if space_released {
            if self.last_ducked {
                self.rising = true;
            }
            self.space_duck = false;
        }
        let mut force = 0.0;
        let ctrl_jump = input.pressed & keys::CROUCH != 0
            && self.can_duck_jump
            && (!grounded || input.held & keys::JUMP != 0);
        if ctrl_jump && self.value >= 0.5 {
            force += forces.duck;
            self.duck_jumped = true;
            self.can_duck_jump = false;
            self.last_ducked = true;
            duck = true;
        }
        if wall.supported(grounded) {
            if input.pressed & keys::JUMP != 0 {
                force += 1.0;
                self.can_duck_jump = true;
                self.duck_jumped = false;
                wall.grounded();
            }
        } else {
            if (input.released & keys::CROUCH != 0 || space_released) && self.duck_jumped {
                force -= forces.inverse_duck;
                self.duck_jumped = false;
            }
            if input.pressed & keys::JUMP != 0 {
                if let Some(kind) = wall.air_jump() {
                    self.grip(wall);
                    force += match kind {
                        WallJump::Wall => forces.wall,
                        WallJump::Ledge => forces.ledge,
                    };
                } else if self.can_duck_jump {
                    self.space_duck = true;
                    if self.value >= 0.5 {
                        self.last_ducked = true;
                        duck = true;
                        self.duck_jumped = true;
                        self.can_duck_jump = false;
                        force += forces.duck;
                    }
                }
            }
        }
        if duck && self.last_ducked {
            self.value = if self.value > reference.ctrl_value {
                reference.ctrl_value
            } else {
                self.value + self.grow(reference.ctrl_value - self.value, reference)
            };
        }
        if wheel_allowed && input.wheel.is_finite() && input.wheel != 0.0 {
            self.last_ducked = false;
            self.rising = false;
            let delta = input.wheel * reference.scroll_step;
            self.value += if delta < 0.0 {
                delta
            } else {
                self.grow(delta, reference)
            };
        }
        if self.rising && !self.space_duck {
            self.value += self.grow(dt * reference.rise_rate, reference);
        }
        self.value = self.value.clamp(0.0, 1.0);
        force
    }
}

pub(super) fn speed_multiplier(value: f32, ctrl: f32, low: f32, duck: f32) -> f32 {
    if value >= ctrl {
        duck + (1.0 - duck) * ((value - ctrl) / (1.0 - ctrl))
    } else {
        low + (duck - low) * (value / ctrl)
    }
}
