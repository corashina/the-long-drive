use super::{ReferenceError, SourceId, Transform, finite, require};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrivingAssists {
    pub automatic_clutch: bool,
    pub automatic_gears: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Source `inputscript.IN` action (by source name) and its `InitDefault` keys. The browser reads
/// them. A key is a `KeyboardEvent.code`, `Mouse0` to `Mouse2`, or a mouse axis named like
/// `oneKey.GetName`. None is null.
pub struct InputBinding {
    pub action: String,
    pub keys: [Option<String>; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrivingReference {
    pub source: SourceId,
    pub pedal_rate: f32,
    pub force_manual: bool,
    pub min_motor_torque_nm: f32,
    pub default_assists: DrivingAssists,
    pub bindings: Vec<InputBinding>,
    pub mouse_drive: MouseDriveReference,
    pub lever: LeverTiming,
    pub steering_wheel: SteeringWheelReference,
}

/// Source `slidersettingscript`: `GetValue` range and default, multiplier applied.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliderRange {
    pub default: f32,
    pub minimum: f32,
    pub maximum: f32,
}

impl SliderRange {
    pub fn contains(&self, value: f32) -> bool {
        (self.minimum..=self.maximum).contains(&value)
    }
}

/// Source `tabControls.sliders[8]`, `[9]` (`FMouseDriveSensX`, `FMouseDriveSensY`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MouseDriveReference {
    pub sensitivity_x: SliderRange,
    pub sensitivity_y: SliderRange,
}

/// Source `settingsscript.tabSteeringWheel` defaults, mapped by `RefreshSettings`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteeringWheelReference {
    /// Source `CBHShifter`.
    pub h_shifter: bool,
    /// Steer, gas, brake, clutch, handbrake.
    pub axes: [AxisReference; 5],
}

/// Source `CBenable*`, `CBinvert*`, `CF*Middle`, `CF*Dead`, `CF*Sens`, `CF*Max`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisReference {
    pub enabled: bool,
    pub invert: bool,
    /// Steer only.
    pub middle: Option<SliderRange>,
    pub dead: SliderRange,
    pub sens: SliderRange,
    pub max: SliderRange,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrivingVehicleReference {
    pub initial_gear: usize,
    pub force_automatic: bool,
    pub play_shift_sounds_in_automatic: bool,
}

/// Source `carscript.gears[i]` lever poses, in the lever parent space.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverGear {
    pub path: Transform,
    pub pos: Transform,
}

/// Source `carscript.Transmission`. `gears` follow `WorldVehicleReference.gear_ratios`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverReference {
    /// The lever prototype node.
    pub node: u32,
    pub gears: Vec<LeverGear>,
}

/// Source `carscript` `TransmissionLerp` and `transmissionStage1`, `transmissionStage2`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverTiming {
    pub seconds: f32,
    pub stages: [f32; 2],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelFrictionCurve {
    pub extremum_slip: f32,
    pub extremum_value: f32,
    pub asymptote_slip: f32,
    pub asymptote_value: f32,
    pub stiffness: f32,
}

impl WheelFrictionCurve {
    pub(super) fn valid(&self) -> bool {
        finite(&[
            self.extremum_slip,
            self.extremum_value,
            self.asymptote_slip,
            self.asymptote_value,
            self.stiffness,
        ]) && self.extremum_slip > 0.0
            && self.asymptote_slip > self.extremum_slip
            && self.extremum_value > 0.0
            && self.asymptote_value >= 0.0
            && self.asymptote_value <= self.extremum_value
            && self.stiffness >= 0.0
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelHubReference {
    pub radius_m: f32,
    pub mass_kg: f32,
    pub forward_stiffness: f32,
    pub sideways_stiffness: f32,
}

impl DrivingReference {
    pub(super) fn validate(&self) -> Result<(), ReferenceError> {
        require(
            self.source.file == "level0" && self.source.path_id == 16865,
            "driving settings source",
        )?;
        require(
            finite(&[self.pedal_rate, self.min_motor_torque_nm])
                && self.pedal_rate > 0.0
                && self.min_motor_torque_nm >= 0.0,
            "driving parameters",
        )?;
        require(
            !self.default_assists.automatic_gears || self.default_assists.automatic_clutch,
            "driving assist dependency",
        )?;
        let mut actions = HashSet::new();
        require(
            self.bindings
                .iter()
                .all(|binding| !binding.action.is_empty() && actions.insert(&binding.action)),
            "input bindings",
        )?;
        require(
            [
                self.mouse_drive.sensitivity_x,
                self.mouse_drive.sensitivity_y,
            ]
            .iter()
            .all(|range| {
                finite(&[range.default, range.minimum, range.maximum])
                    && range.contains(range.default)
            }),
            "mouse driving sensitivity",
        )?;
        let lever = &self.lever;
        require(
            finite(&[lever.seconds, lever.stages[0], lever.stages[1]])
                && lever.seconds > 0.0
                && 0.0 < lever.stages[0]
                && lever.stages[0] < lever.stages[1]
                && lever.stages[1] < 1.0,
            "driving lever timing",
        )?;
        require(
            self.steering_wheel
                .axes
                .iter()
                .enumerate()
                .all(|(index, axis)| {
                    axis.middle.is_some() == (index == 0)
                        && axis
                            .middle
                            .iter()
                            .chain([&axis.dead, &axis.sens, &axis.max])
                            .all(|range| {
                                finite(&[range.default, range.minimum, range.maximum])
                                    && range.contains(range.default)
                            })
                }),
            "steering wheel settings",
        )?;
        Ok(())
    }
}
