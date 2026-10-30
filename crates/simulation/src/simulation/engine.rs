//! Engine part rules at the fixed gameplay step.

use crate::fluids::{FluidContainer, FluidMixture, VehicleFluids, suitability};
use crate::reference::{EngineGlobals, EnginePartReference, RadiatorPartReference, TorqueCurve};
use crate::save::{EngineSnapshot, SaveError, identity::unsigned};

const RUN_GRACE_SECONDS: f32 = 0.2;

fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.0
    } else {
        ((value - a) / (b - a)).clamp(0.0, 1.0)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// The installed Lada curve has unweighted keys and clamps at both ends.
pub fn torque(curve: &TorqueCurve, rpm: f32) -> f32 {
    let keys = &curve.keys;
    if rpm <= keys[0].time {
        return keys[0].value;
    }
    for pair in keys.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if rpm <= b.time {
            let width = b.time - a.time;
            let t = (rpm - a.time) / width;
            let t2 = t * t;
            let t3 = t2 * t;
            return (2.0 * t3 - 3.0 * t2 + 1.0) * a.value
                + (t3 - 2.0 * t2 + t) * width * a.out_tangent
                + (-2.0 * t3 + 3.0 * t2) * b.value
                + (t3 - t2) * width * b.in_tangent;
        }
    }
    keys.last().unwrap().value
}

fn temperature_change(
    temperature: f32,
    speed: f32,
    transfer: f32,
    input: EngineInput,
    globals: &EngineGlobals,
    dt: f32,
) -> f32 {
    let difference = ((input.ambient_temperature - temperature)
        / globals.temperature_difference_modifier)
        .abs()
        .clamp(1.0, 1000.0);
    let direction = if input.ambient_temperature > temperature {
        1.0
    } else {
        -1.0
    };
    direction
        * dt
        * (1.0
            + globals.temperature_move_modifier
                * inverse_lerp(0.0, globals.max_car_velocity, speed))
        * (transfer / 60.0)
        * difference
        * input.sleep_modifier
}

// Source ChangeWithoutMix is sequential. Tank.Upd removes one depleted entry.
// Coolant changes do not call Tank.Upd. Keep their retained zero entries.
fn drain(container: &mut FluidContainer, liters: f32) -> f32 {
    let quantities = container.mixture().quantities_liters;
    let before = container.total();
    if before > 0.0 && liters > 0.0 {
        container.change_without_mix(-liters);
        // Finite-state correction: overdraw must not publish fluid creation.
        container.bound_engine_drain(quantities);
    }
    before - container.total()
}

/// Source `GetPercent`: the fluid share of the tank. An empty tank gives zero.
fn share(mixture: &FluidMixture, total: f32, fluid: usize) -> f32 {
    if total > 0.0 {
        mixture.amount(fluid) / total
    } else {
        0.0
    }
}

/// Source two-stroke `OilWeak`: the fuel tank oil share against the engine mix.
/// A missing fuel tank gives zero oil.
fn two_stroke_oil_penalty(fuel: Option<&FluidContainer>, reference: &EnginePartReference) -> f32 {
    let oil_fluid = reference.oil_fluid as usize;
    let oil = fuel.map_or(0.0, |fuel| share(fuel.mixture(), fuel.total(), oil_fluid));
    let required = FluidMixture {
        quantities_liters: reference.fuel_consumption,
    };
    let target = share(&required, required.total(), oil_fluid);
    let low = inverse_lerp(target, target - reference.oil_toleration_min, oil);
    let high = inverse_lerp(target, target + reference.oil_toleration_max, oil);
    (low * low).max(high).powi(3)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InstalledEngineInput {
    pub ignition: bool,
    pub starter: bool,
    pub throttle: f32,
    /// Clutch engagement. Zero disconnects the wheels.
    pub clutch: f32,
    pub wheel_rpm: f32,
    pub free_run: bool,
    pub speed_kmh: f32,
    pub radiator_condition: Option<u8>,
    /// The car has a `carscript.CoolantBoil` source. Without it the coolant rule does not run.
    pub coolant_boil: bool,
    /// Car electronics allow ignition.
    pub electronics_enabled: bool,
}

/// The records one engine step reads: the mounted engine and radiator parts and the world globals.
#[derive(Clone, Copy)]
pub struct EngineRecords<'a> {
    pub engine: &'a EnginePartReference,
    pub radiator: Option<&'a RadiatorPartReference>,
    pub globals: &'a EngineGlobals,
}

#[derive(Clone, Copy, Debug)]
pub struct EngineInput {
    pub installed: Option<InstalledEngineInput>,
    pub ambient_temperature: f32,
    pub ambient_heat_modifier: f32,
    pub sleep_modifier: f32,
}

impl Default for EngineInput {
    fn default() -> Self {
        Self {
            installed: None,
            ambient_temperature: 0.,
            ambient_heat_modifier: 0.,
            sleep_modifier: 1.,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EngineOutput {
    pub starter: bool,
    /// The kick start branch ran this step.
    pub kick_start: bool,
    pub running: bool,
    pub rpm: f32,
    pub target_rpm: f32,
    pub torque_nm: f32,
    pub brake_torque_nm: f32,
    pub fuel_suitability: f32,
    pub oil_penalty: f32,
    pub heat_penalty: f32,
    pub consumed_fuel_liters: f32,
    pub temperature: f32,
    /// Source `selectedFanSpeed` in degrees per second while `Cool` spins the fan.
    pub fan_speed: f32,
    /// Source `carscript` coolant boil fraction: steam rate and boil volume scale with it.
    pub boil: f32,
}

#[derive(Clone, Debug)]
pub struct EngineState {
    pub rpm: f32,
    pub temperature: f32,
    elapsed: f32,
    last_run_time: f32,
    last_start_chance_time: f32,
    random_start: f32,
    oil_penalty: f32,
    fuel_suitability: f32,
    heat_penalty: f32,
    start_draw: u64,
    supplied_fuel: f32,
    last_output: EngineOutput,
}

impl EngineState {
    pub fn clear_input(&mut self) {
        self.last_output.starter = false;
        self.last_output.kick_start = false;
    }
    /// Source `engineStartChanceTime * engineStartTimeModifier` draw, shared by starter and kick start.
    fn draw_start_chance(
        &mut self,
        reference: &EnginePartReference,
        running: bool,
        start_sample: &mut impl FnMut(u64) -> f32,
    ) {
        if !running
            && self.elapsed - self.last_start_chance_time
                > reference.start_chance_interval_seconds * reference.engine_start_time_modifier
        {
            self.last_start_chance_time = self.elapsed;
            self.random_start = start_sample(self.start_draw).clamp(0.0, 1.0);
            self.start_draw = self.start_draw.wrapping_add(1);
        }
    }
    pub fn running(&self) -> bool {
        self.elapsed - self.last_run_time < RUN_GRACE_SECONDS
    }
    /// The last step output.
    pub fn output(&self) -> EngineOutput {
        self.last_output
    }
    /// The radiator part saves its own fan state.
    pub fn capture_save(&self) -> crate::save::EngineSnapshot {
        crate::save::EngineSnapshot {
            rpm: self.rpm,
            temperature: self.temperature,
            elapsed: self.elapsed,
            last_run_time: self.last_run_time,
            last_start_chance_time: self.last_start_chance_time,
            random_start: self.random_start,
            supplied_fuel: self.supplied_fuel,
            oil_penalty: self.oil_penalty,
            fuel_suitability: self.fuel_suitability,
            heat_penalty: self.heat_penalty,
            start_draw: self.start_draw.to_string(),
        }
    }
    pub fn restore_save(&mut self, saved: &EngineSnapshot) -> Result<EngineOutput, SaveError> {
        let start_draw = unsigned(&saved.start_draw)?;
        self.rpm = saved.rpm;
        self.temperature = saved.temperature;
        self.elapsed = saved.elapsed;
        self.last_run_time = saved.last_run_time;
        self.last_start_chance_time = saved.last_start_chance_time;
        self.random_start = saved.random_start;
        self.supplied_fuel = saved.supplied_fuel;
        self.oil_penalty = saved.oil_penalty;
        self.fuel_suitability = saved.fuel_suitability;
        self.heat_penalty = saved.heat_penalty;
        self.start_draw = start_draw;
        self.clear_input();
        self.last_output = EngineOutput {
            running: self.running(),
            rpm: self.rpm,
            temperature: self.temperature,
            oil_penalty: self.oil_penalty,
            fuel_suitability: self.fuel_suitability,
            heat_penalty: self.heat_penalty,
            ..Default::default()
        };
        Ok(self.last_output)
    }
    pub fn new(initial_temperature: f32) -> Self {
        Self {
            rpm: 0.0,
            temperature: initial_temperature,
            elapsed: 0.0,
            last_run_time: -1.0,
            last_start_chance_time: 0.0,
            random_start: 1.0,
            oil_penalty: 0.,
            fuel_suitability: 0.,
            heat_penalty: 0.,
            start_draw: 0,
            supplied_fuel: 0.0,
            last_output: EngineOutput::default(),
        }
    }

    /// `fan_on` is the hysteresis state of the mounted radiator part.
    pub fn step(
        &mut self,
        input: EngineInput,
        fluids: VehicleFluids<'_>,
        records: EngineRecords<'_>,
        fan_on: Option<&mut bool>,
        mut start_sample: impl FnMut(u64) -> f32,
        dt: f32,
    ) -> EngineOutput {
        if !dt.is_finite() || dt <= 0.0 {
            return self.last_output;
        }
        self.elapsed += dt;
        let EngineRecords {
            engine: reference,
            radiator,
            globals,
        } = records;
        let VehicleFluids {
            fuel,
            sump,
            mut coolant,
        } = fluids;
        let installed = input.installed.unwrap_or_default();
        let required = FluidMixture {
            quantities_liters: reference.fuel_consumption,
        };
        // Two-strokes read the fuel tank mix. A missing oil tank skips the four-stroke rule.
        if input.installed.is_some() && reference.two_stroke {
            self.oil_penalty = two_stroke_oil_penalty(fuel.as_deref(), reference);
        } else if input.installed.is_some()
            && let Some(sump) = sump.as_deref()
        {
            self.oil_penalty = inverse_lerp(
                sump.capacity_liters * 0.3,
                0.0,
                sump.mixture().amount(reference.oil_fluid as usize),
            )
            .powi(3);
        }
        let oil_penalty = self.oil_penalty;
        let can_run = self.rpm > reference.shutdown_rpm
            && oil_penalty < 0.99
            && self.heat_penalty < 0.99
            && self.fuel_suitability > 0.3
            && self.random_start <= reference.start_chance;
        let ignition = installed.ignition && installed.electronics_enabled;
        let min_supplied = inverse_lerp(0.0, reference.max_rpm, reference.idle_rpm);
        if ignition && can_run {
            self.supplied_fuel = lerp(
                min_supplied,
                1.0,
                installed.throttle * self.fuel_suitability,
            );
            self.last_run_time = self.elapsed;
        } else if self.elapsed - self.last_run_time >= RUN_GRACE_SECONDS {
            self.supplied_fuel = 0.0;
        }
        let running = self.running();
        // A missing fuel tank supplies no fuel.
        self.fuel_suitability = fuel.as_deref().map_or(0.0, |f| suitability(f, &required));
        let fuel_suitability = self.fuel_suitability;
        let consumed_fuel = if running && let Some(fuel) = fuel {
            let removed = drain(
                fuel,
                self.supplied_fuel
                    * dt
                    * input.sleep_modifier
                    * globals.fuel_consumption
                    * lerp(
                        required.total() * 0.2,
                        required.total(),
                        inverse_lerp(0.0, reference.max_rpm, self.rpm),
                    ),
            );
            // Source Fuel.Upd also runs when supplied fuel is zero.
            fuel.cleanup_one();
            removed
        } else {
            0.0
        };
        self.supplied_fuel *= fuel_suitability;
        let target_rpm = reference.max_rpm * self.supplied_fuel;
        let mut clutch = installed.clutch.clamp(0.0, 1.0);
        if installed.free_run
            && (!reference.kickstart || running || !ignition)
            && target_rpm < installed.wheel_rpm
        {
            clutch = 0.0;
        }
        let start_clip = reference
            .audio
            .as_ref()
            .is_some_and(|audio| audio.starter.is_some());
        if installed.starter {
            if self.rpm < reference.starter_rpm {
                self.rpm = lerp(
                    self.rpm,
                    reference.starter_rpm,
                    reference.starter_rpm_lerp * dt,
                );
            }
            if start_clip && !reference.kickstart {
                self.draw_start_chance(reference, running, &mut start_sample);
            }
        }
        // Source draws the kick start chance only when `SStart` is set.
        let kick_start = start_clip
            && reference.kickstart
            && installed.wheel_rpm > reference.shutdown_rpm
            && !running
            && ignition;
        if kick_start {
            self.draw_start_chance(reference, running, &mut start_sample);
        }
        let next_rpm = lerp(target_rpm, installed.wheel_rpm, clutch);
        self.rpm += (next_rpm - self.rpm).clamp(-globals.rpm_change, globals.rpm_change)
            * reference.rpm_change_modifier
            * dt;
        let mut rpm_difference = self.rpm - target_rpm;
        let curve_output = torque(&reference.torque_curve, self.rpm);
        let mut actual_output = if target_rpm > self.rpm && running {
            curve_output
        } else {
            0.0
        };
        if installed.starter && target_rpm <= reference.starter_rpm {
            rpm_difference =
                (self.rpm - reference.starter_rpm) * reference.starter_rpm_lerp * 100.0;
            actual_output =
                (curve_output * reference.starter_rpm_lerp).min(reference.max_torque_nm * 0.5);
        }
        let brake_output = reference.max_torque_nm
            * reference.engine_brake_modifier
            * if self.rpm < 0.0 {
                1.0
            } else {
                inverse_lerp(0.0, reference.max_rpm_difference, rpm_difference)
            };
        let bounds = globals.car_temperature_bounds;
        self.temperature = self.temperature.clamp(bounds[0], bounds[1]);
        if reference.no_overheat {
            self.temperature = self.temperature.min(reference.optimal_temperature[2]);
        }
        self.heat_penalty = inverse_lerp(
            reference.optimal_temperature[2],
            reference.optimal_temperature[3],
            self.temperature,
        )
        .powi(3);
        let heat_penalty = self.heat_penalty;
        actual_output *= (1.0 - heat_penalty) * (1.0 - oil_penalty);
        if running {
            self.temperature += dt
                * (lerp(
                    reference.heat_gain_min,
                    reference.heat_gain_max,
                    inverse_lerp(reference.idle_rpm, reference.max_rpm, self.rpm),
                ) + input.ambient_heat_modifier)
                / 60.0;
        }
        self.temperature += temperature_change(
            self.temperature,
            installed.speed_kmh * reference.heat_loss_speed_modifier,
            reference.heat_loss,
            input,
            globals,
            dt,
        );
        if input.installed.is_some()
            && self.rpm > 10.0
            && let Some(sump) = sump
            && sump.total() > 0.0
        {
            drain(
                sump,
                globals.oil_consumption
                    * reference.oil_consumption_modifier
                    * inverse_lerp(0.0, reference.max_rpm, self.rpm)
                    * dt
                    * input.sleep_modifier,
            );
            sump.cleanup_one();
        }
        // A missing radiator, coolant tank or boil source skips the coolant rules.
        let mut boil = 0.0;
        if installed.coolant_boil
            && installed.radiator_condition.is_some()
            && self.temperature > globals.min_steam_temperature
            && let Some(coolant) = coolant.as_deref_mut()
            && coolant.total() > 0.0
        {
            boil = inverse_lerp(globals.min_steam_temperature, bounds[1], self.temperature);
            drain(
                coolant,
                globals.coolant_evaporate_rate / 60.0 * dt * input.sleep_modifier * boil,
            );
        }
        let mut fan_speed = 0.0;
        if running
            && let Some(condition) = installed.radiator_condition
            && let Some(radiator) = radiator
            && let Some(fan_on) = fan_on
            && let Some(coolant) = coolant
        {
            let condition = usize::from(condition.min(4));
            let value_factor = [1.0, 0.75, 0.5, 0.25, 0.125][condition];
            let fan_value = radiator.max_fan_value * [1.0, 0.9, 0.8, 0.7, 0.6][condition];
            let leak = radiator.max_fluid_loss * [0.0, 0.125, 0.25, 0.5, 1.0][condition];
            let mut speed = installed.speed_kmh;
            if self.temperature > radiator.fan_temperature_on || radiator.fan_on_when_engine_runs {
                *fan_on = true;
                speed = (speed + fan_value).clamp(fan_value, globals.max_car_velocity);
            } else if self.temperature < radiator.fan_temperature_off {
                *fan_on = false;
            }
            if *fan_on {
                fan_speed = radiator.max_fan_speed * [1.0, 0.8, 0.6, 0.4, 0.2][condition];
            }
            let thermostat = inverse_lerp(
                radiator.thermostat_range[0],
                radiator.thermostat_range[1],
                self.temperature,
            );
            let total = coolant.total();
            let water_blood = if total > 0.0 {
                coolant.mixture().amount(3).max(coolant.mixture().amount(4)) / total
            } else {
                0.0
            };
            let cooling = radiator.max_value
                * value_factor
                * thermostat
                * inverse_lerp(0.0, coolant.capacity_liters * 0.5, total)
                * water_blood.clamp(0.5, 1.0);
            self.temperature +=
                temperature_change(self.temperature, speed, cooling, input, globals, dt);
            drain(
                coolant,
                leak / 60.0 * dt * thermostat * input.sleep_modifier,
            );
        }
        self.last_output = EngineOutput {
            starter: installed.starter,
            kick_start,
            running,
            rpm: self.rpm,
            target_rpm,
            torque_nm: actual_output.max(0.0) * reference.torque_modifier * clutch,
            brake_torque_nm: brake_output.max(0.0) * reference.torque_modifier * clutch,
            fuel_suitability,
            oil_penalty,
            heat_penalty,
            consumed_fuel_liters: consumed_fuel,
            temperature: self.temperature,
            fan_speed,
            boil,
        };
        self.last_output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluids::FluidEntry;

    fn globals() -> EngineGlobals {
        serde_json::from_value(serde_json::json!({
            "source": {"file": "level1", "path_id": 64919}, "fuel_consumption": 1.0,
            "oil_consumption": 0.0, "rpm_change": 2000.0, "car_temperature_bounds": [-50.0, 150.0],
            "coolant_evaporate_rate": 0.0, "temperature_difference_modifier": 10.0,
            "temperature_move_modifier": 1.0, "max_car_velocity": 100.0,
            "outside_temperature_modifier": 0.0, "min_steam_temperature": 100.0
        }))
        .unwrap()
    }

    /// Steps a kick start engine for two seconds. Returns the last output, the rpm peak and the draws.
    fn kick(
        ignition: bool,
        starter: bool,
        clutch: f32,
        wheel_rpm: f32,
    ) -> (EngineOutput, f32, u32) {
        let mut record: EnginePartReference =
            serde_json::from_value(crate::reference::engine_part_fixture()).unwrap();
        record.kickstart = true;
        let globals = globals();
        let mut fuel = FluidContainer::new(
            10.,
            &[FluidEntry {
                fluid_type: 2,
                amount: 5.,
            }],
        );
        let mut sump = FluidContainer::new(
            10.,
            &[FluidEntry {
                fluid_type: 3,
                amount: 5.,
            }],
        );
        let mut state = EngineState::new(20.);
        let (mut output, mut peak, mut draws) = (EngineOutput::default(), 0f32, 0);
        for _ in 0..100 {
            output = state.step(
                EngineInput {
                    installed: Some(InstalledEngineInput {
                        ignition,
                        starter,
                        throttle: 0.5,
                        clutch,
                        wheel_rpm,
                        electronics_enabled: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                VehicleFluids {
                    fuel: Some(&mut fuel),
                    sump: Some(&mut sump),
                    coolant: None,
                },
                EngineRecords {
                    engine: &record,
                    radiator: None,
                    globals: &globals,
                },
                None,
                |_| {
                    draws += 1;
                    0.
                },
                0.02,
            );
            peak = peak.max(output.rpm);
        }
        (output, peak, draws)
    }

    #[test]
    fn kick_start_draws_the_start_chance_above_shutdown_rpm() {
        let (output, _, draws) = kick(true, false, 1., 1500.);
        assert!(output.running && draws == 1, "{output:?} {draws}");
        // The kick start stops once the engine runs.
        assert!(!output.kick_start);
        // Below shutdown rpm or without ignition there is no kick start.
        for (ignition, wheel_rpm) in [(true, 150.), (false, 1500.)] {
            let (output, _, draws) = kick(ignition, false, 1., wheel_rpm);
            assert!(
                !output.running && !output.kick_start && draws == 0,
                "{output:?}"
            );
        }
        // Source skips the starter chance draw on a kick start engine.
        let (output, _, draws) = kick(true, true, 0., 0.);
        assert!(!output.running && draws == 0, "{output:?}");
    }

    #[test]
    fn kick_start_does_not_turn_the_starter() {
        // With the clutch open, only the starter can lift the rpm.
        let (output, peak, _) = kick(true, false, 0., 1500.);
        assert!(output.kick_start && peak == 0., "{output:?} {peak}");
    }

    #[test]
    fn coolant_boils_above_steam_temperature_with_a_boil_source() {
        let record: EnginePartReference =
            serde_json::from_value(crate::reference::engine_part_fixture()).unwrap();
        let globals = globals();
        let boil = |temperature: f32, coolant_boil: bool, amount: f32| {
            let mut coolant = FluidContainer::new(
                10.,
                &[FluidEntry {
                    fluid_type: 4,
                    amount,
                }],
            );
            let output = EngineState::new(temperature).step(
                EngineInput {
                    installed: Some(InstalledEngineInput {
                        radiator_condition: Some(0),
                        coolant_boil,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                VehicleFluids {
                    fuel: None,
                    sump: None,
                    coolant: Some(&mut coolant),
                },
                EngineRecords {
                    engine: &record,
                    radiator: None,
                    globals: &globals,
                },
                None,
                |_| 0.,
                0.02,
            );
            (output.boil, output.temperature)
        };
        // f = InverseLerp(minSteamTemp, maxCarTemp, temp).
        let (f, temperature) = boil(125., true, 5.);
        assert!(
            (f - (temperature - 100.) / 50.).abs() < 1e-5 && f > 0.4,
            "{f}"
        );
        // No boil source, no coolant or a cool engine: no boil.
        for (temperature, source, amount) in [(125., false, 5.), (125., true, 0.), (90., true, 5.)]
        {
            assert_eq!(boil(temperature, source, amount).0, 0.);
        }
    }

    #[test]
    fn two_stroke_oil_follows_the_source_formula() {
        let mut record: EnginePartReference =
            serde_json::from_value(crate::reference::engine_part_fixture()).unwrap();
        let oil = record.oil_fluid;
        record.fuel_consumption = [0.; 6];
        record.fuel_consumption[2] = 0.975;
        record.fuel_consumption[oil as usize] = 0.025;
        record.oil_toleration_min = 0.02;
        record.oil_toleration_max = 0.3;
        for (share, expected) in [(0.025, 0.), (0.005, 1.), (0.015, 0.015625), (0.175, 0.125)] {
            let tank = FluidContainer::new(
                10.,
                &[
                    FluidEntry {
                        fluid_type: 2,
                        amount: 1. - share,
                    },
                    FluidEntry {
                        fluid_type: oil,
                        amount: share,
                    },
                ],
            );
            let penalty = two_stroke_oil_penalty(Some(&tank), &record);
            assert!((penalty - expected).abs() < 1e-4, "{share}: {penalty}");
        }
        let empty = FluidContainer::new(10., &[]);
        assert_eq!(two_stroke_oil_penalty(Some(&empty), &record), 1.);
        assert_eq!(two_stroke_oil_penalty(None, &record), 1.);
    }
}
