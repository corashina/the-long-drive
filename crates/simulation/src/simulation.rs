pub const FIXED_DT: f32 = 0.01999999955;
pub mod balance;
pub mod car_lights;
pub mod dials;
pub mod drivetrain;
pub mod engine;
pub mod gauges;
pub mod ignition;
pub mod input;
pub mod instruments;
pub mod lever;
pub mod metal_detector;
pub mod music_box;
pub mod pedals;
pub mod powertrain;
pub mod radio;
pub mod reach;
pub mod sled;
const MAX_FRAME_SECONDS: f64 = 0.1;
const STEP_SECONDS: f64 = FIXED_DT as f64;
/// Steps per frame at time scale 1. Slow frames drop extra time, so step cost cannot grow frame time.
const STEP_BUDGET: usize = 2;

pub mod keys {
    pub const FORWARD: u32 = 1;
    pub const BACK: u32 = 2;
    pub const LEFT: u32 = 4;
    pub const RIGHT: u32 = 8;
    /// Source `IN.jump`.
    pub const JUMP: u32 = 16;
    pub const USE: u32 = 32;
    pub const RUN: u32 = 64;
    pub const CROUCH: u32 = 128;
    pub const GRAB: u32 = 256;
    pub const PRIMARY: u32 = 512;
    pub const IGNITION: u32 = 1024;
    pub const CAMERA: u32 = 2048;
    pub const ROTATE: u32 = 4096;
    /// Source `input.reload` (R).
    pub const RELOAD: u32 = 8192;
    pub const TRANSLATE: u32 = 16384;
    pub const RECENTER: u32 = 32768;
    pub const LIGHTS: u32 = 65536;
    pub const HORN: u32 = 1 << 17;
    pub const SLEEP: u32 = 1 << 18;
    pub const URINATE: u32 = 1 << 19;
    pub const BURP: u32 = 1 << 20;
    pub const CLUTCH: u32 = 1 << 21;
    pub const SHIFT_UP: u32 = 1 << 22;
    pub const SHIFT_DOWN: u32 = 1 << 23;
    pub const REVERSE: u32 = 1 << 24;
    pub const NEUTRAL: u32 = 1 << 25;
    pub const GEAR_1: u32 = 1 << 26;
    pub const GEAR_2: u32 = 1 << 27;
    pub const GEAR_3: u32 = 1 << 28;
    pub const GEAR_4: u32 = 1 << 29;
    pub const GEAR_5: u32 = 1 << 30;
    pub const GEAR_6: u32 = 1 << 31;
}

pub mod player_keys {
    pub const LEAN: u16 = 1;
    pub const ZOOM: u16 = 2;
    /// Source `input.smoke` (Caps Lock).
    pub const SMOKE: u16 = 4;
    /// Source `input.removeSunglasses` (X).
    pub const TAKE_OFF: u16 = 8;
    /// Source `IN.togglemousedrive` (U).
    pub const MOUSE_DRIVE: u16 = 16;
    /// Source `input.point` (`IN.pointrubwave`, Left Alt): point, rub and wave.
    pub const POINT: u16 = 32;
    /// Source `IN.map` (M): map view.
    pub const MAP: u16 = 64;
    /// Source `IN.keylook` (J): seated look with the move keys.
    pub const KEY_LOOK: u16 = 128;
    /// Source `IN.gas`, `IN.brake`, `IN.steerleft`, `IN.steerright` and `IN.handbrake`.
    pub const GAS: u16 = 256;
    pub const BRAKE: u16 = 512;
    pub const STEER_LEFT: u16 = 1024;
    pub const STEER_RIGHT: u16 = 2048;
    pub const HANDBRAKE: u16 = 4096;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CarryDeltas {
    pub owner: Option<crate::reference::ItemId>,
    pub mouse: [f32; 2],
    pub wheel: f32,
}

/// Keyboard input while the player types on a computer (web/input.js typing keys).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TypingInput {
    /// Input.inputString bytes: printable ASCII, 8 (Backspace) and 13 (Enter).
    #[serde(with = "typing_text")]
    pub text: [u8; TypingInput::CAPACITY],
    pub len: u8,
    /// Held piano keys (bits 0-16), held arrows (17-20), pressed A S X (21-23), any key (24).
    pub keys: u32,
}
impl TypingInput {
    pub const CAPACITY: usize = 64;
    const PRESSED: u32 = 7 << 21;
    pub fn text(&self) -> &[u8] {
        &self.text[..usize::from(self.len)]
    }
    fn merge(&mut self, frame: &Self) {
        let free = Self::CAPACITY - usize::from(self.len);
        let n = frame.text().len().min(free);
        let start = usize::from(self.len);
        self.text[start..start + n].copy_from_slice(&frame.text()[..n]);
        self.len += n as u8;
        self.keys = frame.keys | (self.keys & Self::PRESSED);
    }
}
mod typing_text {
    use super::TypingInput;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(
        text: &[u8; TypingInput::CAPACITY],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(text)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<[u8; TypingInput::CAPACITY], D::Error> {
        <&[u8]>::deserialize(d)?
            .try_into()
            .map_err(D::Error::custom)
    }
}
impl Default for TypingInput {
    fn default() -> Self {
        Self {
            text: [0; Self::CAPACITY],
            len: 0,
            keys: 0,
        }
    }
}

/// Raw browser gamepad state: 4 pads, 8 axes and 32 button bits per pad.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GamepadFrame {
    /// Pad `p` axis `a` is at `p * GamepadFrame::AXES + a`.
    pub axes: [f32; 32],
    /// Bit `b` of pad `p` is set while button `b` is pressed.
    pub buttons: [u32; 4],
}

impl GamepadFrame {
    pub const PADS: usize = 4;
    pub const AXES: usize = 8;
    pub const BUTTONS: usize = 32;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameInput {
    pub held: u32,
    pub pressed: u32,
    pub released: u32,
    pub player_held: u16,
    pub player_pressed: u16,
    pub player_released: u16,
    pub inventory_held: u8,
    pub inventory_released: u8,
    pub mouse: [f32; 2],
    /// Source `Mouse X`, `Mouse Y` axes, without look sensitivity. Y is up.
    pub drive_mouse: [f32; 2],
    pub wheel: f32,
    pub carry: CarryDeltas,
    pub typing: TypingInput,
    pub gamepad: GamepadFrame,
}

impl FrameInput {
    /// Source `otherView()`: head look, lean, zoom, recenter, use, pickup and get-out do not run.
    /// Releases stay, so a held action can end.
    pub fn without_view_input(mut self) -> Self {
        const VIEW_KEYS: u32 = keys::USE
            | keys::GRAB
            | keys::PRIMARY
            | keys::ROTATE
            | keys::TRANSLATE
            | keys::RECENTER;
        const VIEW_PLAYER_KEYS: u16 = player_keys::LEAN | player_keys::ZOOM | player_keys::POINT;
        self.held &= !VIEW_KEYS;
        self.pressed &= !VIEW_KEYS;
        self.player_held &= !VIEW_PLAYER_KEYS;
        self.player_pressed &= !VIEW_PLAYER_KEYS;
        self.mouse = [0.; 2];
        self.wheel = 0.;
        self.carry.mouse = [0.; 2];
        self.carry.wheel = 0.;
        self
    }
    pub fn merge(&mut self, frame: Self) {
        self.held = frame.held;
        self.pressed |= frame.pressed;
        self.released |= frame.released;
        self.player_held = frame.player_held;
        self.player_pressed |= frame.player_pressed;
        self.player_released |= frame.player_released;
        self.inventory_held = frame.inventory_held;
        self.inventory_released |= frame.inventory_released;
        self.gamepad = frame.gamepad;
        for (pending, delta) in self
            .mouse
            .iter_mut()
            .chain(&mut self.drive_mouse)
            .zip(frame.mouse.into_iter().chain(frame.drive_mouse))
        {
            if delta.is_finite() {
                *pending += delta;
            }
        }
        if frame.wheel.is_finite() {
            self.wheel += frame.wheel;
        }
        if self.carry.owner != frame.carry.owner {
            self.carry = CarryDeltas {
                owner: frame.carry.owner,
                ..CarryDeltas::default()
            };
        }
        for (pending, delta) in self.carry.mouse.iter_mut().zip(frame.carry.mouse) {
            if delta.is_finite() {
                *pending += delta;
            }
        }
        if frame.carry.wheel.is_finite() {
            self.carry.wheel += frame.carry.wheel;
        }
        self.typing.merge(&frame.typing);
    }

    /// Retain active skipped input, cancel blocked skipped input, or consume a frame.
    pub fn take_frame(&mut self, skip_elapsed: &mut bool, blocked: bool) -> Option<Self> {
        if std::mem::take(skip_elapsed) {
            if blocked {
                self.clear();
            }
            return None;
        }
        Some(std::mem::take(self))
    }

    pub fn take_step(&mut self) -> Self {
        let step = *self;
        *self = Self {
            held: step.held,
            player_held: step.player_held,
            inventory_held: step.inventory_held,
            typing: TypingInput {
                keys: step.typing.keys & !TypingInput::PRESSED,
                ..TypingInput::default()
            },
            gamepad: step.gamepad,
            ..Self::default()
        };
        step
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DriveInput {
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub handbrake: f32,
}

/// Stores accepted real time. Each step spends its sampled scale once.
/// Below 25 FPS the simulation runs slower than real time.
#[derive(Default)]
pub struct StepClock {
    accumulator: f64,
    steps: usize,
    touched_sleep: bool,
}

impl StepClock {
    pub fn accept_frame(&mut self, elapsed_seconds: f64) {
        self.steps = 0;
        self.touched_sleep = false;
        if elapsed_seconds.is_finite() && elapsed_seconds >= 0. {
            self.accumulator += elapsed_seconds.min(MAX_FRAME_SECONDS);
        }
    }

    pub fn take_step(&mut self, time_scale: f32) -> bool {
        if !time_scale.is_finite() || time_scale <= 0. {
            return false;
        }
        self.touched_sleep |= time_scale > 1.;
        let cost = STEP_SECONDS / f64::from(time_scale);
        let budget = if self.touched_sleep {
            STEP_BUDGET * 2
        } else {
            STEP_BUDGET
        };
        if self.steps >= budget {
            self.accumulator %= cost;
            return false;
        }
        if self.accumulator + 1.0e-12 < cost {
            return false;
        }
        self.accumulator = (self.accumulator - cost).max(0.);
        self.steps += 1;
        true
    }

    pub fn alpha(&self, time_scale: f32) -> f32 {
        (self.accumulator * f64::from(time_scale) / STEP_SECONDS).clamp(0., 1.) as f32
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod step_clock_tests {
    use super::{FIXED_DT, StepClock};

    #[test]
    fn slow_frame_runs_two_steps_and_drops_extra_time() {
        let mut clock = StepClock::default();
        clock.accept_frame(0.1);

        for _ in 0..2 {
            assert!(clock.take_step(1.));
        }
        assert!(!clock.take_step(1.));
        clock.accept_frame(0.);
        assert!(!clock.take_step(1.));
    }

    #[test]
    fn sleep_clock_keeps_double_time_scale_bounded() {
        let mut clock = StepClock::default();
        clock.accept_frame(0.1);

        for _ in 0..4 {
            assert!(clock.take_step(2.));
        }
        assert!(!clock.take_step(2.));
    }

    #[test]
    fn fixed_steps_cover_one_simulation_second() {
        let mut clock = StepClock::default();
        let mut steps = 0;
        for _ in 0..50 {
            clock.accept_frame(f64::from(FIXED_DT));
            steps += usize::from(clock.take_step(1.));
        }

        assert_eq!(steps, 50);
        assert!((f64::from(FIXED_DT) * steps as f64 - 1.).abs() < 1.0e-6);
    }
}

#[cfg(test)]
mod typing_tests {
    use super::{FrameInput, TypingInput};

    #[test]
    fn typed_text_and_music_keys_reach_one_step() {
        let frame = |text: &[u8], keys: u32| {
            let mut typing = TypingInput {
                keys,
                ..TypingInput::default()
            };
            typing.text[..text.len()].copy_from_slice(text);
            typing.len = text.len() as u8;
            FrameInput {
                typing,
                ..FrameInput::default()
            }
        };
        let mut pending = FrameInput::default();
        pending.merge(frame(b"ab", 1 | 1 << 21));
        pending.merge(frame(b"c", 1));
        let step = pending.take_step();
        assert_eq!(step.typing.text(), b"abc");
        assert_eq!(step.typing.keys, 1 | 1 << 21);
        let next = pending.take_step();
        assert!(next.typing.text().is_empty());
        assert_eq!(next.typing.keys, 1, "held keys stay, pressed keys do not");
    }
}
