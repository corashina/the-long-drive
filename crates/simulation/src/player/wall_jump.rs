//! Source `fpscontroller` wall jump (`AWallJump`), ledge jump (`AWallJump2`) and `Grip`.
use super::sensor::SensorContact;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WallJump {
    Wall,
    Ledge,
}

#[derive(Default)]
pub(super) struct WallJumpState {
    /// Source `canwalljump`.
    wall: SensorContact,
    /// Source `canwalljump2`.
    ledge: SensorContact,
    /// Source `BcanWallJump`.
    can_wall_jump: bool,
    /// Source `Bwalljump2ed`.
    ledge_jumped: bool,
    /// Source `griping`.
    gripping: bool,
    /// `Grip` can clamp the fall this step.
    grip_ready: bool,
    /// `Grip` clamped the fall this step.
    gripped: bool,
}

/// Source `Grip` condition: falling and slower than `max_speed`.
pub(super) fn grip_ready(vertical: f32, speed: f32, max_speed: f32) -> bool {
    vertical < 0.0 && speed <= max_speed
}

/// Source `Grip` vertical clamp to `[grip_loose, max_fall]`.
pub(super) fn grip_vertical(vertical: f32, grip_loose: f32, max_fall: f32) -> f32 {
    vertical.max(grip_loose).min(max_fall)
}

impl WallJumpState {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
    /// Run once per step before the jump decision.
    pub(super) fn contacts(&mut self, wall: bool, ledge: bool, hold: u8, grip_ready: bool) {
        self.wall.step(wall, hold);
        self.ledge.step(ledge, hold);
        self.grip_ready = grip_ready;
        self.gripped = false;
    }
    pub(super) fn gripping(&self) -> bool {
        self.gripping
    }
    /// True when `Grip` clamped the fall this step.
    pub(super) fn gripped(&self) -> bool {
        self.gripped
    }
    /// Source `Move` first jump check. True when `Grip` runs.
    pub(super) fn hold(&mut self, jump_held: bool) -> bool {
        let grip = jump_held && !self.ledge.sensed() && self.wall.sensed() && self.ledge_jumped;
        if !grip {
            self.gripping = false;
        }
        grip
    }
    /// Source `Grip`. True when it clamps the fall. The caller then enables the duck jump.
    pub(super) fn grip(&mut self) -> bool {
        if !self.grip_ready {
            return false;
        }
        self.grip_ready = false;
        self.gripped = true;
        self.gripping = true;
        true
    }
    /// Source grounded sensor and `AJump`.
    pub(super) fn grounded(&mut self) {
        self.can_wall_jump = true;
    }
    /// Source ground or grip branch. True when the ground jump is allowed.
    pub(super) fn supported(&mut self, grounded: bool) -> bool {
        if !self.gripping {
            self.ledge_jumped = false;
        }
        grounded || self.gripping
    }
    /// Source air jump order. The caller runs `Grip` on a result.
    pub(super) fn air_jump(&mut self) -> Option<WallJump> {
        if !self.wall.sensed() {
            None
        } else if self.can_wall_jump {
            self.can_wall_jump = false;
            Some(WallJump::Wall)
        } else if !self.ledge.sensed() {
            self.ledge_jumped = true;
            Some(WallJump::Ledge)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touching(state: &mut WallJumpState, wall: bool, ledge: bool, ready: bool) {
        state.contacts(wall, ledge, 3, ready);
    }

    #[test]
    fn wall_jump_once_then_ledge_then_grip() {
        let mut state = WallJumpState::default();
        state.grounded();
        touching(&mut state, true, false, true);
        assert_eq!(state.air_jump(), Some(WallJump::Wall));
        assert!(state.grip());
        touching(&mut state, true, true, false);
        assert_eq!(state.air_jump(), None);
        // The ledge contact holds for three more steps.
        for _ in 0..4 {
            touching(&mut state, true, false, true);
        }
        assert_eq!(state.air_jump(), Some(WallJump::Ledge));
        // Jump held at the wall below the ledge: grip again.
        touching(&mut state, true, false, true);
        assert!(state.hold(true));
        assert!(state.grip() && state.gripping() && state.gripped());
        assert!(state.supported(false));
        // Release jump: grip ends.
        touching(&mut state, true, false, true);
        assert!(!state.hold(false) && !state.gripping());
        assert!(!state.supported(false));
    }

    #[test]
    fn no_wall_no_wall_jump() {
        let mut state = WallJumpState::default();
        state.grounded();
        touching(&mut state, false, false, true);
        assert_eq!(state.air_jump(), None);
        assert!(!state.hold(true));
    }

    #[test]
    fn sensor_holds_three_steps() {
        let mut state = WallJumpState::default();
        state.grounded();
        touching(&mut state, true, false, false);
        for _ in 0..3 {
            touching(&mut state, false, false, false);
        }
        assert_eq!(state.air_jump(), Some(WallJump::Wall));
        state.grounded();
        touching(&mut state, false, false, false);
        assert_eq!(state.air_jump(), None);
    }

    #[test]
    fn grip_only_when_falling_slowly() {
        assert!(grip_ready(-1.0, 4.0, 5.0));
        assert!(!grip_ready(-6.0, 6.0, 5.0));
        assert!(!grip_ready(1.0, 1.0, 5.0));
        let mut state = WallJumpState::default();
        touching(&mut state, true, false, false);
        assert!(!state.grip() && !state.gripping());
        assert_eq!(grip_vertical(-3.0, 0.0, 30.0), 0.0);
        assert_eq!(grip_vertical(-3.0, -1.0, 30.0), -1.0);
    }
}
