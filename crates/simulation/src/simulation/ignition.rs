//! Chassis ignition switch and transient starter commands.
#[derive(Clone, Copy, Debug, Default)]
pub struct IgnitionInput {
    pub held: bool,
    pub pressed: bool,
    pub released: bool,
}

#[derive(Clone, Debug, Default)]
pub struct IgnitionState {
    pub ignition: bool,
    pub starter: bool,
    hold_seconds: f32,
    last_held: bool,
}

impl IgnitionState {
    pub fn held(&self) -> bool {
        self.last_held
    }
    /// Source `startingValue`.
    pub fn hold_seconds(&self) -> f32 {
        self.hold_seconds
    }

    pub fn step(
        &mut self,
        input: IgnitionInput,
        delay_seconds: f32,
        electronics_enabled: bool,
        dt: f32,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        if input.pressed {
            self.hold_seconds = 0.;
        }
        if input.held {
            if self.hold_seconds < delay_seconds {
                self.hold_seconds += dt;
            } else {
                self.ignition = true;
                self.starter = electronics_enabled;
            }
        } else if input.released || self.last_held {
            if self.hold_seconds < delay_seconds {
                self.ignition = !self.ignition;
                self.hold_seconds = 0.;
                self.starter = false;
            }
        } else {
            self.starter = false;
            self.hold_seconds = 0.;
        }
        self.last_held = input.held;
    }

    pub fn clear_input(&mut self) {
        self.hold_seconds = 0.;
        self.last_held = false;
        self.starter = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{IgnitionInput, IgnitionState};

    #[test]
    fn short_tap_toggles_once() {
        let mut ignition = IgnitionState::default();
        ignition.step(
            IgnitionInput {
                held: true,
                pressed: true,
                ..Default::default()
            },
            0.2,
            true,
            0.02,
        );
        assert!(!ignition.ignition);
        ignition.step(
            IgnitionInput {
                released: true,
                ..Default::default()
            },
            0.2,
            true,
            0.02,
        );
        assert!(ignition.ignition);
        assert!(!ignition.starter);
        ignition.step(IgnitionInput::default(), 0.2, true, 0.02);
        assert!(ignition.ignition);
    }

    #[test]
    fn continuous_hold_starts_after_delay() {
        let mut ignition = IgnitionState::default();
        let held = IgnitionInput {
            held: true,
            ..Default::default()
        };
        ignition.step(held, 0.2, true, 0.2);
        assert!(!ignition.starter);
        ignition.step(held, 0.2, true, 0.02);
        assert!(ignition.ignition);
        assert!(ignition.starter);
    }

    #[test]
    fn long_hold_release_retains_one_step() {
        let mut ignition = IgnitionState::default();
        let held = IgnitionInput {
            held: true,
            ..Default::default()
        };
        ignition.step(held, 0.2, true, 0.2);
        ignition.step(held, 0.2, true, 0.02);
        ignition.step(
            IgnitionInput {
                released: true,
                ..Default::default()
            },
            0.2,
            true,
            0.02,
        );
        assert!(ignition.starter);
        ignition.step(IgnitionInput::default(), 0.2, true, 0.02);
        assert!(!ignition.starter);
        assert!(ignition.ignition);
        ignition.clear_input();
        assert!(ignition.ignition);
        assert!(!ignition.starter);
    }

    #[test]
    fn clear_input_cancels_without_release_toggle() {
        let mut ignition = IgnitionState::default();
        let held = IgnitionInput {
            held: true,
            ..Default::default()
        };
        ignition.step(held, 0.2, true, 0.2);
        ignition.step(held, 0.2, true, 0.02);
        ignition.clear_input();
        assert!(ignition.ignition);
        assert!(!ignition.starter);
        ignition.step(IgnitionInput::default(), 0.2, true, 0.02);
        assert!(ignition.ignition);
        let mut short = IgnitionState::default();
        short.step(held, 0.2, true, 0.02);
        short.clear_input();
        short.step(IgnitionInput::default(), 0.2, true, 0.02);
        assert!(!short.ignition);
    }

    #[test]
    fn electronics_disabled_suppresses_starter_only() {
        let mut ignition = IgnitionState::default();
        let held = IgnitionInput {
            held: true,
            ..Default::default()
        };
        ignition.step(held, 0.2, false, 0.2);
        ignition.step(held, 0.2, false, 0.02);
        assert!(ignition.ignition);
        assert!(!ignition.starter);
    }

    #[test]
    fn coalesced_press_release_toggles_once() {
        let mut ignition = IgnitionState::default();
        let tap = IgnitionInput {
            pressed: true,
            released: true,
            ..Default::default()
        };
        ignition.step(tap, 0.2, true, 0.02);
        assert!(ignition.ignition);
        ignition.step(IgnitionInput::default(), 0.2, true, 0.02);
        assert!(ignition.ignition);
        ignition.step(tap, 0.2, true, 0.02);
        assert!(!ignition.ignition);
    }
}
