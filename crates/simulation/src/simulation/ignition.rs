//! Chassis ignition switch and transient starter commands.
#[derive(Clone, Copy, Debug, Default)]
pub struct IgnitionInput {
    pub held: bool,
    pub pressed: bool,
    pub released: bool,
}

/// Ignition key position. Source `ignition` and `starting`: the starter turns only with ignition on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Key {
    #[default]
    Off,
    On,
    Cranking,
}

#[derive(Clone, Debug, Default)]
pub struct IgnitionState {
    pub key: Key,
    hold_seconds: f32,
    last_held: bool,
}

impl IgnitionState {
    /// Source `ignition`.
    pub fn on(&self) -> bool {
        self.key != Key::Off
    }
    /// Source `starting`.
    pub fn cranking(&self) -> bool {
        self.key == Key::Cranking
    }
    /// Sets the ignition from a save or a peer. A turning starter keeps turning.
    pub fn set_on(&mut self, on: bool) {
        self.key = match (on, self.key) {
            (false, _) => Key::Off,
            (true, Key::Off) => Key::On,
            (true, key) => key,
        };
    }
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
                self.key = if electronics_enabled {
                    Key::Cranking
                } else {
                    Key::On
                };
            }
        } else if input.released || self.last_held {
            if self.hold_seconds < delay_seconds {
                self.key = if self.on() { Key::Off } else { Key::On };
                self.hold_seconds = 0.;
            }
        } else {
            self.stop_starter();
            self.hold_seconds = 0.;
        }
        self.last_held = input.held;
    }

    pub fn clear_input(&mut self) {
        self.hold_seconds = 0.;
        self.last_held = false;
        self.stop_starter();
    }

    fn stop_starter(&mut self) {
        if self.key == Key::Cranking {
            self.key = Key::On;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_without_electronics_turns_on_without_cranking() {
        let mut state = IgnitionState::default();
        let held = IgnitionInput {
            held: true,
            ..Default::default()
        };
        state.step(
            IgnitionInput {
                pressed: true,
                ..held
            },
            0.1,
            false,
            0.2,
        );
        state.step(held, 0.1, false, 0.2);
        assert_eq!(state.key, Key::On);
    }
}
