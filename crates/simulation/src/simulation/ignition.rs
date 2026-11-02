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
