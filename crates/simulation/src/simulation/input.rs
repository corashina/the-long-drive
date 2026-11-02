use crate::reference::DrivingReference;
use crate::simulation::{FIXED_DT, FrameInput, GamepadFrame, keys, player_keys};
use rapier3d::prelude::RigidBodyHandle;
use serde::{Deserialize, Serialize};

/// Source `inputscript.DriveStuff` mouse axis scale.
const MOUSE_SCALE: f32 = 0.0125;
/// Source extra scale on `Mouse X`.
const MOUSE_STEER_SCALE: f32 = 0.25;
/// `DrivingSettings.axes` order.
pub const STEER: usize = 0;
pub const GAS: usize = 1;
pub const BRAKE: usize = 2;
pub const CLUTCH: usize = 3;
pub const HANDBRAKE: usize = 4;
/// `DrivingSettings.buttons[i]` acts as keys bit `1 << (BUTTON_SHIFT + i)`: clutch, shift up,
/// shift down, reverse, neutral, gears 1 to 6.
const BUTTON_SHIFT: usize = 21;
const GEARS: u32 = keys::REVERSE
    | keys::NEUTRAL
    | keys::GEAR_1
    | keys::GEAR_2
    | keys::GEAR_3
    | keys::GEAR_4
    | keys::GEAR_5
    | keys::GEAR_6;
/// Source `CBHShifter` held gears, in source priority order.
const H_GEARS: [u32; 7] = [
    keys::REVERSE,
    keys::GEAR_1,
    keys::GEAR_2,
    keys::GEAR_3,
    keys::GEAR_4,
    keys::GEAR_5,
    keys::GEAR_6,
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrivingControls {
    /// Positive steers left.
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub handbrake: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrivingOutput {
    pub controls: DrivingControls,
    /// Gear request bits in `keys` order (source `grDown` to `downshiftDown`).
    pub gear_edges: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisBinding {
    pub pad: u8,
    pub axis: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonBinding {
    pub pad: u8,
    pub button: u8,
}

/// Source `CS*` axis, `CBenable*`, `CBinvert*`, `CF*Middle` (steer only, else 0), `CF*Dead`,
/// `CF*Sens`, `CF*Max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisSetting {
    pub bound: Option<AxisBinding>,
    pub enabled: bool,
    pub invert: bool,
    pub middle: f32,
    pub dead: f32,
    pub sens: f32,
    pub max: f32,
}

impl AxisSetting {
    /// The bound pad axis, or none when the axis is off or unbound.
    fn read(&self, gamepad: &GamepadFrame) -> Option<f32> {
        let bound = self.bound.filter(|_| self.enabled)?;
        let value = gamepad.axes[bound.pad as usize * GamepadFrame::AXES + bound.axis as usize];
        Some(if value.is_finite() { value } else { 0. })
    }

    fn sign(&self) -> f32 {
        if self.invert { -1. } else { 1. }
    }

    /// Source `DriveStuff` steer curve. Positive is source right.
    fn steer(&self, axis: f32) -> f32 {
        let middle = self.middle;
        let num = (axis + middle) * self.sign();
        let num = if num > middle {
            lerp(
                middle,
                1. + middle,
                inverse_lerp(self.dead + middle, 1. + middle, num),
            )
        } else {
            lerp(
                middle,
                -1. + middle,
                inverse_lerp(-self.dead + middle, -1. + middle, num),
            )
        };
        let offset = num - middle;
        if offset > self.max {
            1.
        } else if offset < -self.max {
            -1.
        } else {
            num.clamp(-1., 1.) * self.sens
        }
    }

    /// Source `DriveStuff` pedal curve, as `InverseLerp(-1, 1, value)`.
    fn pedal(&self, axis: f32) -> f32 {
        let value = lerp(
            -1.,
            1.,
            inverse_lerp(self.dead - 1., 1., axis * self.sign()),
        );
        let value = if value > self.max {
            1.
        } else {
            (value + 1.) * self.sens - 1.
        };
        inverse_lerp(-1., 1., value)
    }
}

/// Unity `Mathf.Lerp`.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}

/// Unity `Mathf.InverseLerp`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a != b {
        ((value - a) / (b - a)).clamp(0., 1.)
    } else {
        0.
    }
}

/// Source keyboard ramp: `MoveTowards` for 0 and ±1 targets; other targets apply directly.
fn ramp(value: &mut f32, target: f32, step: f32) {
    *value = if target == 0. || target.abs() == 1. {
        *value + (target - *value).clamp(-step, step)
    } else {
        target
    };
}

/// Player driving settings from the browser (source `settingsscript.S`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrivingSettings {
    /// Source `FMouseDriveSensX`, `FMouseDriveSensY`.
    pub mouse: [f32; 2],
    /// Source `CBHShifter`.
    pub h_shifter: bool,
    /// Steer, gas, brake, clutch, handbrake.
    pub axes: [AxisSetting; 5],
    /// Clutch, shift up, shift down, reverse, neutral, gears 1 to 6.
    pub buttons: [Option<ButtonBinding>; 11],
}

impl DrivingSettings {
    pub fn defaults(reference: &DrivingReference) -> Self {
        let mouse = &reference.mouse_drive;
        let wheel = &reference.steering_wheel;
        Self {
            mouse: [mouse.sensitivity_x.default, mouse.sensitivity_y.default],
            h_shifter: wheel.h_shifter,
            axes: wheel.axes.map(|axis| AxisSetting {
                bound: None,
                enabled: axis.enabled,
                invert: axis.invert,
                middle: axis.middle.map_or(0., |range| range.default),
                dead: axis.dead.default,
                sens: axis.sens.default,
                max: axis.max.default,
            }),
            buttons: [None; 11],
        }
    }

    pub fn valid(&self, reference: &DrivingReference) -> bool {
        let mouse = &reference.mouse_drive;
        mouse.sensitivity_x.contains(self.mouse[0])
            && mouse.sensitivity_y.contains(self.mouse[1])
            && self
                .axes
                .iter()
                .zip(&reference.steering_wheel.axes)
                .all(|(axis, range)| {
                    axis.bound.is_none_or(|bound| {
                        (bound.pad as usize) < GamepadFrame::PADS
                            && (bound.axis as usize) < GamepadFrame::AXES
                    }) && range
                        .middle
                        .map_or(axis.middle == 0., |middle| middle.contains(axis.middle))
                        && range.dead.contains(axis.dead)
                        && range.sens.contains(axis.sens)
                        && range.max.contains(axis.max)
                })
            && self.buttons.iter().flatten().all(|bound| {
                (bound.pad as usize) < GamepadFrame::PADS
                    && (bound.button as usize) < GamepadFrame::BUTTONS
            })
    }
}

/// Source `inputscript.DriveStuff` at the fixed gameplay step: keyboard, pad axes and buttons,
/// and mouse driving.
pub struct DrivingInput {
    keyboard: DrivingControls,
    steering_increment: f32,
    pedal_increment: f32,
    /// The body of the seat that takes the input: the car chassis or the item body.
    seat: Option<RigidBodyHandle>,
    /// The seat is a car driver seat. Only car driver seats take mouse driving and gears.
    car: bool,
    /// Source `mouseDriveToggle`. It stays across seats and pauses.
    mouse_toggle: bool,
    /// Source `currentMouseSteer` (positive steers right) and `currentMouseGas`.
    mouse: [f32; 2],
    /// Bound buttons held at the last step, as keys bits.
    buttons: u32,
    pub settings: DrivingSettings,
    /// The last step or preview controls, for the Controls screen live values.
    pub controls: DrivingControls,
}

impl DrivingInput {
    pub fn new(steering_rate: f32, pedal_rate: f32, settings: DrivingSettings) -> Self {
        Self {
            keyboard: DrivingControls::default(),
            steering_increment: steering_rate * FIXED_DT,
            pedal_increment: pedal_rate * FIXED_DT,
            seat: None,
            car: false,
            mouse_toggle: false,
            mouse: [0.; 2],
            buttons: 0,
            settings,
            controls: DrivingControls::default(),
        }
    }

    /// Source `FKeySteerSens`: keyboard steer change per second.
    pub fn set_steering_rate(&mut self, rate: f32) {
        self.steering_increment = rate * FIXED_DT;
    }

    pub fn clear(&mut self) {
        self.keyboard = DrivingControls::default();
        self.mouse = [0.; 2];
        self.seat = None;
        self.car = false;
        self.controls = DrivingControls::default();
    }

    /// Pad axes only, without a seat, for the live values while the game is paused.
    pub fn preview(&mut self, gamepad: &GamepadFrame) {
        let mut controls = DrivingControls::default();
        self.add_axes(&mut controls, gamepad);
        self.controls = controls;
    }

    /// Source `DriveStuff` axes on top of the keyboard values. The handbrake is the axis only.
    fn add_axes(&self, controls: &mut DrivingControls, gamepad: &GamepadFrame) {
        let axes = &self.settings.axes;
        if let Some(axis) = axes[STEER].read(gamepad) {
            // Source steer is positive right.
            controls.steer = (controls.steer - axes[STEER].steer(axis)).clamp(-1., 1.);
        }
        for (value, index) in [
            (&mut controls.throttle, GAS),
            (&mut controls.brake, BRAKE),
            (&mut controls.clutch, CLUTCH),
        ] {
            if let Some(axis) = axes[index].read(gamepad) {
                *value = (axes[index].pedal(axis) + *value).clamp(0., 1.);
            }
        }
        controls.handbrake = axes[HANDBRAKE]
            .read(gamepad)
            .map_or(0., |axis| axes[HANDBRAKE].pedal(axis));
    }

    pub fn toggle_mouse(&mut self) {
        self.mouse_toggle = !self.mouse_toggle;
    }

    /// Source `mouseDrive`. Mouse look reads zero while it is on.
    pub fn mouse_owned(&self) -> bool {
        self.mouse_toggle && self.car
    }

    /// `input` gives keys, the source `Mouse X`, `Mouse Y` sample and the pads. `car` is the
    /// `keySteerSensMultiplier` of the car on a car driver seat, where zero means none.
    /// None on an item seat: no multiplier, no mouse driving and no gear requests.
    /// `automatic_gears` is source `BAutoShift`, which clears all gear requests.
    pub fn step(
        &mut self,
        seat: Option<RigidBodyHandle>,
        input: &FrameInput,
        car: Option<f32>,
        automatic_gears: bool,
    ) -> DrivingOutput {
        let output = self.output(seat, input, car, automatic_gears);
        self.controls = output.controls;
        output
    }

    fn output(
        &mut self,
        seat: Option<RigidBodyHandle>,
        input: &FrameInput,
        car: Option<f32>,
        automatic_gears: bool,
    ) -> DrivingOutput {
        let gamepad = &input.gamepad;
        let mut buttons = 0;
        for (index, bound) in self.settings.buttons.iter().enumerate() {
            if let Some(bound) = bound
                && gamepad.buttons[bound.pad as usize] & (1 << bound.button) != 0
            {
                buttons |= 1 << (BUTTON_SHIFT + index);
            }
        }
        let pressed = input.pressed | (buttons & !self.buttons);
        self.buttons = buttons;
        let held = input.held | buttons;
        if self.seat != seat {
            self.clear();
            self.seat = seat;
        }
        if seat.is_none() {
            return DrivingOutput::default();
        }
        self.car = car.is_some();
        let steer_multiplier = car.unwrap_or(0.);
        let steering_increment = if steer_multiplier != 0. {
            self.steering_increment * steer_multiplier
        } else {
            self.steering_increment
        };
        let player = |bit: u16| (input.player_held & bit != 0) as u8 as f32;
        let steer = player(player_keys::STEER_LEFT) - player(player_keys::STEER_RIGHT);
        ramp(&mut self.keyboard.steer, steer, steering_increment);
        for (value, pressed) in [
            (&mut self.keyboard.throttle, player(player_keys::GAS)),
            (&mut self.keyboard.brake, player(player_keys::BRAKE)),
            (
                &mut self.keyboard.clutch,
                (held & keys::CLUTCH != 0) as u8 as f32,
            ),
        ] {
            ramp(value, pressed, self.pedal_increment);
        }
        let mut controls = self.keyboard;
        self.add_axes(&mut controls, gamepad);
        let gear_edges = if automatic_gears || !self.car {
            0
        } else {
            let gears = if self.settings.h_shifter {
                H_GEARS
                    .into_iter()
                    .find(|bit| held & bit != 0)
                    .unwrap_or(keys::NEUTRAL)
            } else {
                pressed & GEARS
            };
            gears | (pressed & (keys::SHIFT_UP | keys::SHIFT_DOWN))
        };
        if !self.mouse_owned() {
            self.mouse = [0.; 2];
            return DrivingOutput {
                controls,
                gear_edges,
            };
        }
        let sensitivity = self.settings.mouse;
        let mouse = input.drive_mouse;
        if mouse.iter().all(|v| v.is_finite()) {
            self.mouse[0] = (self.mouse[0]
                + mouse[0] * MOUSE_SCALE * MOUSE_STEER_SCALE * sensitivity[0])
                .clamp(-1., 1.);
            self.mouse[1] =
                (self.mouse[1] + mouse[1] * MOUSE_SCALE * sensitivity[1]).clamp(-1., 1.);
        }
        controls.steer = (controls.steer - self.mouse[0]).clamp(-1., 1.);
        controls.throttle = (controls.throttle + self.mouse[1].clamp(0., 1.)).clamp(0., 1.);
        controls.brake = (controls.brake + (-self.mouse[1]).clamp(0., 1.)).clamp(0., 1.);
        DrivingOutput {
            controls,
            gear_edges,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat() -> Option<RigidBodyHandle> {
        Some(RigidBodyHandle::from_raw_parts(1, 0))
    }
    const CAR: Option<f32> = Some(0.);
    const AXIS: AxisSetting = AxisSetting {
        bound: None,
        enabled: true,
        invert: false,
        middle: 0.,
        dead: 0.,
        sens: 1.,
        max: 1.,
    };

    fn input() -> DrivingInput {
        DrivingInput::new(
            2.,
            5.,
            DrivingSettings {
                mouse: [1., 1.],
                h_shifter: false,
                axes: [AXIS; 5],
                buttons: [None; 11],
            },
        )
    }

    fn held(player_held: u16, mouse: [f32; 2]) -> FrameInput {
        FrameInput {
            player_held,
            drive_mouse: mouse,
            ..FrameInput::default()
        }
    }

    fn step(driving: &mut DrivingInput, frame: FrameInput) -> DrivingControls {
        driving.step(seat(), &frame, CAR, false).controls
    }

    /// Pad 1 axis `axis` reads `value`.
    fn pad_axis(axis: u8, value: f32) -> FrameInput {
        let mut frame = FrameInput::default();
        frame.gamepad.axes[GamepadFrame::AXES + axis as usize] = value;
        frame
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn mouse_drives_after_toggle() {
        let mut driving = input();
        assert_eq!(
            step(&mut driving, held(0, [0., 40.])),
            DrivingControls::default()
        );
        driving.toggle_mouse();
        let up = step(&mut driving, held(0, [0., 40.]));
        assert!(close(up.throttle, 0.5) && up.brake == 0.);
        let down = step(&mut driving, held(0, [0., -120.]));
        assert!(close(down.brake, 1.) && down.throttle == 0.);
        // Mouse X has 0.25 of the Y scale. Right is negative steer.
        let right = step(&mut driving, held(0, [40., 0.]));
        assert!(close(right.steer, -0.125));
        assert!(driving.mouse_owned());
    }

    #[test]
    fn toggle_off_resets_and_seat_gates_mouse() {
        let mut driving = input();
        driving.toggle_mouse();
        assert_eq!(
            driving.step(None, &held(0, [40., 40.]), CAR, false),
            DrivingOutput::default()
        );
        assert!(!driving.mouse_owned());
        // Item seats (sleds) take keyboard and pad input only.
        assert_eq!(
            driving.step(seat(), &held(0, [40., 40.]), None, false),
            DrivingOutput::default()
        );
        assert!(!driving.mouse_owned());
        step(&mut driving, held(0, [40., 40.]));
        driving.toggle_mouse();
        assert_eq!(
            step(&mut driving, held(0, [0.; 2])),
            DrivingControls::default()
        );
        assert!(!driving.mouse_owned());
        driving.toggle_mouse();
        assert_eq!(
            step(&mut driving, held(0, [0.; 2])),
            DrivingControls::default()
        );
    }

    #[test]
    fn keyboard_steer_uses_car_multiplier() {
        let mut driving = input();
        let base = step(&mut driving, held(player_keys::STEER_LEFT, [0.; 2])).steer;
        assert!(close(base, 2. * FIXED_DT));
        driving.clear();
        let scaled = driving.step(
            seat(),
            &held(player_keys::STEER_LEFT, [0.; 2]),
            Some(3.),
            false,
        );
        assert!(close(scaled.controls.steer, 6. * FIXED_DT));
    }

    #[test]
    fn steer_curve_has_middle_dead_zone_and_max_snap() {
        let axis = AxisSetting {
            middle: 0.1,
            dead: 0.2,
            max: 0.5,
            ..AXIS
        };
        // Inside the dead zone the value is the middle.
        assert!(close(axis.steer(0.05), 0.1));
        assert!(close(axis.steer(0.5), 0.475));
        assert!(close(axis.steer(-0.3), -0.025));
        assert!(close(axis.steer(0.55), 0.5375));
        assert_eq!(axis.steer(0.7), 1.);
        assert_eq!(axis.steer(-0.9), -1.);
        let doubled = AxisSetting { sens: 2., ..axis };
        assert!(close(doubled.steer(0.5), 0.95));
        let inverted = AxisSetting {
            invert: true,
            ..axis
        };
        assert!(close(inverted.steer(-0.7), 0.475));
    }

    #[test]
    fn pedal_curve_has_dead_sens_and_max() {
        let axis = AxisSetting {
            dead: 0.2,
            max: 0.5,
            ..AXIS
        };
        assert_eq!(axis.pedal(-1.), 0.);
        assert_eq!(axis.pedal(-0.8), 0.);
        assert!(close(axis.pedal(-0.35), 0.25));
        assert!(close(axis.pedal(0.4), 2. / 3.));
        // Above max the pedal snaps to full.
        assert_eq!(axis.pedal(0.6), 1.);
        let sensitive = AxisSetting { sens: 2., ..axis };
        assert!(close(sensitive.pedal(-0.35), 0.5));
        assert_eq!(sensitive.pedal(0.1), 1.);
        let inverted = AxisSetting {
            invert: true,
            ..axis
        };
        assert!(close(inverted.pedal(0.35), 0.25));
    }

    #[test]
    fn keyboard_and_axes_add_and_clamp() {
        let mut driving = input();
        let bound = |axis| AxisSetting {
            bound: Some(AxisBinding { pad: 1, axis }),
            ..AXIS
        };
        driving.settings.axes = [bound(0), bound(1), bound(1), bound(1), bound(1)];
        let mut frame = pad_axis(0, -0.5);
        frame.gamepad.axes[GamepadFrame::AXES + 1] = 0.;
        frame.player_held = player_keys::STEER_LEFT | player_keys::GAS;
        let mut controls = DrivingControls::default();
        for _ in 0..200 {
            controls = step(&mut driving, frame);
        }
        // Keyboard full left plus pad half left.
        assert_eq!(controls.steer, 1.);
        assert_eq!(controls.throttle, 1.);
        assert!(close(controls.brake, 0.5) && close(controls.handbrake, 0.5));
        frame.player_held = player_keys::STEER_RIGHT;
        let mut controls = DrivingControls::default();
        for _ in 0..200 {
            controls = step(&mut driving, frame);
        }
        assert!(close(controls.steer, -0.5));
        assert!(close(controls.throttle, 0.5));
        // Item seats take the pad axes too.
        let item = driving.step(seat(), &frame, None, false);
        assert!(close(item.controls.throttle, 0.5));
    }

    #[test]
    fn disabled_or_unbound_axis_gives_keyboard_only() {
        let mut driving = input();
        driving.settings.axes[GAS] = AxisSetting {
            bound: Some(AxisBinding { pad: 1, axis: 2 }),
            enabled: false,
            ..AXIS
        };
        let mut frame = pad_axis(2, 1.);
        frame.player_held = player_keys::GAS;
        let controls = step(&mut driving, frame);
        assert!(close(controls.throttle, 5. * FIXED_DT));
        assert_eq!(controls.handbrake, 0.);
    }

    #[test]
    fn bound_button_press_gives_gear_once() {
        let mut driving = input();
        // Button 3 of pad 2 is gear 2; button 0 of pad 0 is the clutch.
        driving.settings.buttons[6] = Some(ButtonBinding { pad: 2, button: 3 });
        driving.settings.buttons[0] = Some(ButtonBinding { pad: 0, button: 0 });
        let mut frame = FrameInput::default();
        frame.gamepad.buttons = [1, 0, 8, 0];
        let first = driving.step(seat(), &frame, CAR, false);
        assert_eq!(first.gear_edges, keys::GEAR_2);
        assert!(close(first.controls.clutch, 5. * FIXED_DT));
        assert_eq!(driving.step(seat(), &frame, CAR, false).gear_edges, 0);
        frame.gamepad.buttons = [0; 4];
        driving.step(seat(), &frame, CAR, false);
        frame.gamepad.buttons[2] = 8;
        assert_eq!(driving.step(seat(), &frame, CAR, true).gear_edges, 0);
    }

    #[test]
    fn h_shifter_holds_gears_and_none_gives_neutral() {
        let mut driving = input();
        driving.settings.h_shifter = true;
        driving.settings.buttons[3] = Some(ButtonBinding { pad: 0, button: 5 });
        let mut frame = FrameInput::default();
        assert_eq!(
            driving.step(seat(), &frame, CAR, false).gear_edges,
            keys::NEUTRAL
        );
        frame.held = keys::GEAR_3;
        for _ in 0..2 {
            assert_eq!(
                driving.step(seat(), &frame, CAR, false).gear_edges,
                keys::GEAR_3
            );
        }
        // Reverse comes first in the source order.
        frame.gamepad.buttons[0] = 32;
        assert_eq!(
            driving.step(seat(), &frame, CAR, false).gear_edges,
            keys::REVERSE
        );
        assert_eq!(driving.step(seat(), &frame, CAR, true).gear_edges, 0);
        // Item seats have no gears.
        assert_eq!(driving.step(seat(), &frame, None, false).gear_edges, 0);
    }
}
