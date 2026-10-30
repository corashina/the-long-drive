//! Source carscript AutoShift, ControlStuff, and engine input mapping.
use super::{
    engine::{EngineOutput, torque},
    input::DrivingControls,
    keys,
};
use crate::reference::{
    DrivingAssists, DrivingReference, DrivingVehicleReference, EnginePartReference,
    WorldVehicleReference,
};
use crate::vehicle::DriveTorques;

/// Gear values of one car record.
#[derive(Clone, Copy, Debug)]
pub struct Gearbox<'a> {
    pub driving: &'a DrivingVehicleReference,
    pub gear_ratios: &'a [f32],
    pub gear_free_run: &'a [bool],
    pub final_drive_ratio: f32,
    pub auto_move_min: f32,
    pub dont_ignite_with_throttle_in_automatic: bool,
}
impl<'a> From<&'a WorldVehicleReference> for Gearbox<'a> {
    fn from(v: &'a WorldVehicleReference) -> Self {
        Self {
            driving: &v.driving,
            gear_ratios: &v.gear_ratios,
            gear_free_run: &v.gear_free_run,
            final_drive_ratio: v.final_drive_ratio,
            auto_move_min: v.auto_move_min,
            dont_ignite_with_throttle_in_automatic: v.dont_ignite_with_throttle_in_automatic,
        }
    }
}
#[derive(Clone, Debug)]
pub struct DrivetrainState {
    gear: usize,
    auto_shift_to: usize,
    go_forward: bool,
    go_backward: bool,
    wheel_rpm: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct DrivetrainOutput {
    pub gear: usize,
    pub throttle: f32,
    pub brake: f32,
    pub engagement: f32,
    /// Source `clutch`: pedal depression.
    pub clutch: f32,
    /// Source `IsAutoShift`.
    pub automatic: bool,
    pub wheel_rpm: f32,
    pub free_run: bool,
    pub ignition_request: bool,
}
impl DrivetrainState {
    pub fn new(vehicle: Gearbox<'_>) -> Self {
        Self {
            gear: vehicle.driving.initial_gear,
            auto_shift_to: 1,
            go_forward: false,
            go_backward: false,
            wheel_rpm: 0.,
        }
    }
    pub fn gear(&self) -> usize {
        self.gear
    }
    pub fn restore_gear(&mut self, gear: usize) {
        self.gear = gear;
    }
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        controls: DrivingControls,
        edges: u32,
        assists: DrivingAssists,
        engine: Option<(EngineOutput, &EnginePartReference)>,
        ignition: bool,
        wheel_rpm: f32,
        direction: f32,
        vehicle: Gearbox<'_>,
        driving: &DrivingReference,
        controlled: bool,
    ) -> DrivetrainOutput {
        let controls = if controlled {
            controls
        } else {
            DrivingControls::default()
        };
        let mut throttle = controls.throttle;
        let mut brake = controls.brake;
        let mut depression = controls.clutch;
        let automatic =
            (vehicle.driving.force_automatic || assists.automatic_gears) && !driving.force_manual;
        let mut ignition_request = false;
        if controlled
            && automatic
            && let Some((engine, part)) = engine
        {
            // AutoShift: direction and input inversion precede torque selection.
            let direction =
                if direction > -vehicle.auto_move_min && direction < vehicle.auto_move_min {
                    0.
                } else {
                    direction
                };
            let pressed_forward = throttle > 0.;
            if pressed_forward {
                if direction >= 0. {
                    self.go_forward = true;
                }
            } else {
                self.go_forward = false;
            }
            if brake > 0. && throttle <= 0.25 {
                if direction <= 0. {
                    self.go_backward = true;
                }
            } else {
                self.go_backward = false;
            }
            let inverse = (vehicle.gear_ratios[self.gear] < 0.
                || (!self.go_forward && direction <= 0.))
                && vehicle.gear_ratios.iter().any(|ratio| *ratio < 0.);
            if inverse {
                std::mem::swap(&mut throttle, &mut brake);
            }
            if throttle > 0. || self.go_forward || self.go_backward {
                ignition_request = !vehicle.dont_ignite_with_throttle_in_automatic
                    && throttle > 0.25
                    && !engine.running;
                if let Some(gear) = auto_gear(
                    vehicle.gear_ratios,
                    vehicle.final_drive_ratio,
                    part,
                    wheel_rpm,
                    pressed_forward,
                ) {
                    self.auto_shift_to = gear;
                }
                self.gear = self.auto_shift_to;
            } else if vehicle.gear_ratios[self.gear] != 0. && !pressed_forward {
                self.gear = vehicle
                    .gear_ratios
                    .iter()
                    .position(|ratio| *ratio == 0.)
                    .expect("validated neutral");
            }
            if brake > 0. && throttle > 0. {
                depression = depression.max(brake);
            }
            depression = depression.max(
                ((part.auto_shift_clutch_rpm - engine.rpm) / part.auto_shift_clutch_rpm_range)
                    .clamp(0., 1.),
            );
            if part.kickstart && !engine.running && self.wheel_rpm > part.shutdown_rpm && ignition {
                depression = 0.;
            }
        }
        // `edges` come from `DrivingInput`, which clears them for automatic gears.
        // ControlStuff applies direct keys in source order, then up, then down.
        if controlled
            && (depression > 0.
                || assists.automatic_clutch
                || (vehicle.driving.force_automatic && !driving.force_manual))
        {
            let mut requested = self.gear as i32 - 1;
            let mut shifted = false;
            for (key, gear) in [
                (keys::REVERSE, -1),
                (keys::NEUTRAL, 0),
                (keys::GEAR_1, 1),
                (keys::GEAR_2, 2),
                (keys::GEAR_3, 3),
                (keys::GEAR_4, 4),
                (keys::GEAR_5, 5),
                (keys::GEAR_6, 6),
            ] {
                if edges & key != 0 {
                    requested = gear;
                    shifted = true;
                }
            }
            if vehicle.gear_ratios[0] == 0. {
                requested -= 1;
            } else if vehicle.gear_ratios[0] > 0. {
                requested -= 2;
            }
            if edges & keys::SHIFT_UP != 0 {
                requested = self.gear as i32;
                shifted = true;
            }
            if edges & keys::SHIFT_DOWN != 0 {
                requested = self.gear as i32 - 2;
                shifted = true;
            }
            if shifted
                && requested >= -1
                && requested < vehicle.gear_ratios.len() as i32 - 1
                && (depression > 0.5
                    || ((assists.automatic_clutch || vehicle.driving.force_automatic)
                        && !driving.force_manual))
            {
                self.gear = (requested + 1) as usize;
            }
        }
        let ratio = vehicle.gear_ratios[self.gear];
        self.wheel_rpm = wheel_rpm * vehicle.final_drive_ratio * ratio;
        DrivetrainOutput {
            gear: self.gear,
            throttle,
            brake,
            engagement: if ratio == 0. { 0. } else { 1. - depression },
            clutch: depression,
            automatic,
            wheel_rpm: self.wheel_rpm,
            free_run: vehicle.gear_free_run[self.gear],
            ignition_request,
        }
    }
}
/// Source AutoShift gear pick: the gear with the most engine torque at the wheel speed.
/// Gears with equal torque keep the lowest one, so a car at rest starts in first.
fn auto_gear(
    ratios: &[f32],
    final_drive_ratio: f32,
    part: &EnginePartReference,
    wheel_rpm: f32,
    forward: bool,
) -> Option<usize> {
    let mut maximum = 0.;
    let mut gear = None;
    for (index, ratio) in ratios.iter().enumerate() {
        if *ratio != 0. && (*ratio > 0.) == forward {
            let mut current = torque(
                &part.torque_curve,
                wheel_rpm.abs() * final_drive_ratio * ratio.abs(),
            );
            if current == 0. {
                current = torque(
                    &part.torque_curve,
                    part.max_rpm.abs() * 0.5 * final_drive_ratio * ratio.abs(),
                );
            }
            if current > maximum {
                maximum = current;
                gear = Some(index);
            }
        }
    }
    gear
}
/// EngineOutput already includes clutch engagement. These are axle torques in Nm.
pub fn transmitted_torques(
    engine: EngineOutput,
    vehicle: Gearbox<'_>,
    gear: usize,
) -> DriveTorques {
    let ratio = vehicle.gear_ratios[gear];
    DriveTorques {
        motor_nm: engine.torque_nm * ratio * vehicle.final_drive_ratio,
        engine_brake_nm: engine.brake_torque_nm * ratio.abs() * vehicle.final_drive_ratio,
        pedal_nm: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_gear_at_rest_is_first() {
        let part: EnginePartReference =
            serde_json::from_value(crate::reference::engine_part_fixture()).unwrap();
        let ratios = [-3.5, 0., 3.7, 2.1, 1.4, 1.];
        assert_eq!(auto_gear(&ratios, 4.1, &part, 0., true), Some(2));
        assert_eq!(auto_gear(&ratios, 4.1, &part, 0., false), Some(0));
    }
}
