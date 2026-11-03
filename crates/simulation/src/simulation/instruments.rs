use super::dials::{digit_angle, lerp, lerp_angle};
use crate::reference::{GaugeLimits, InstrumentReference, LampReference, NeedleReference};
use rapier3d::math::Rotation;
#[derive(Clone, Copy, Default)]
pub struct InstrumentSample {
    pub speed_kmh: f32,
    pub fuel_liters: f32,
    pub oil_liters: f32,
    pub temperature: f32,
    pub rpm: f32,
    /// The engine is a two-stroke.
    pub two_stroke: bool,
    pub oil_penalty: f32,
    /// Source `carscript.ledOilTestTwoStrokeDefault`.
    pub oil_led_two_stroke: bool,
    pub local_ignition: bool,
    /// Live `carscript.electronics` output.
    pub electronics_output: f32,
    pub headlights: u8,
    /// Source `carscript.handbrake`.
    pub handbrake: f32,
    /// The car has an engine.
    pub engine: bool,
    /// Engine fuel use in liters per second.
    pub fuel_rate: f32,
    pub distance_m: f64,
}
/// Gauge state of one car, for any needle, lamp and digit count. Lamp bits hold up to 32 lamps.
pub struct InstrumentState {
    signals: Vec<f32>,
    values: Vec<f32>,
    digit_values: Vec<f32>,
    pub needle_angles: Vec<f32>,
    pub digit_angles: Vec<f32>,
    pub lamp_bits: u32,
    lamps: LampValues,
    pub digits_lit: bool,
    pub distance_km: f32,
}
pub fn needle_angle(r: &NeedleReference, value: f32) -> f32 {
    let mut t = ((value + r.offset - r.min_value) / (r.max_value - r.min_value)).clamp(0., 1.);
    if r.inv {
        t = 1. - t;
    }
    lerp(r.min_angle, r.max_angle, t)
}
/// Source `carscript` oil LED. The two-stroke rule reads the engine oil penalty.
fn oil_lamp(limits: &GaugeLimits, s: &InstrumentSample) -> bool {
    let low_oil = if s.oil_led_two_stroke {
        s.two_stroke && s.oil_penalty > 0.4
    } else {
        s.oil_liters < limits.low_oil
    };
    s.local_ignition && (low_oil || s.rpm < limits.oil_test_rpm)
}
/// Source `ledkijelzoscript.value` of each lamp, set by `carscript.LEDUPD`.
#[derive(Clone, Debug, Default)]
pub struct LampValues(Vec<f32>);
impl LampValues {
    /// Source `LEDUPD` and `Upd`: bit i is lamp i on. `dt` None sets each value to its target.
    pub fn update(
        &mut self,
        lamps: &[LampReference],
        limits: &GaugeLimits,
        s: &InstrumentSample,
        dt: Option<f32>,
    ) -> u32 {
        self.0.resize(lamps.len(), 0.);
        let on = |on: bool| if on { 1. } else { 0. };
        let electronics = s.electronics_output > limits.electronics_off_threshold;
        let mut bits = 0;
        for (i, (l, value)) in lamps.iter().zip(&mut self.0).enumerate() {
            *value = match l.signal.as_str() {
                "headlight" => on(matches!(s.headlights, 1 | 2) && electronics),
                "headlight2" => on(s.headlights == 2 && electronics),
                "fuel" => on(s.local_ignition && s.fuel_liters < limits.low_fuel),
                "oil" => on(oil_lamp(limits, s)),
                "battery" => on(s.local_ignition && s.rpm < limits.battery_test_rpm),
                "tank" if s.local_ignition => s.fuel_liters,
                "handbrake" if s.local_ignition => s.handbrake,
                "temp" if s.local_ignition && s.engine => s.temperature,
                "temp" => -1000.,
                "consumption" if electronics => {
                    let target = s.fuel_rate * l.multiplier;
                    dt.map_or(target, |dt| lerp(*value, target, l.smoothing * dt))
                }
                _ => 0.,
            };
            if let Some([min, max]) = l.clamp {
                *value = value.clamp(min, max);
            }
            if *value >= l.min_value && (*value <= l.max_value || l.stay_on) {
                bits |= 1 << i;
            }
        }
        bits
    }
}
/// Source METERUPD2 `lampak`: the meter lights with low or high headlights and local ignition.
pub fn meter_lit(s: &InstrumentSample) -> bool {
    s.local_ignition && matches!(s.headlights, 1 | 2)
}
pub fn source_rotation(e: [f32; 3]) -> [f32; 4] {
    let q = (Rotation::from_rotation_y(e[1].to_radians())
        * Rotation::from_rotation_x(e[0].to_radians())
        * Rotation::from_rotation_z(e[2].to_radians()))
    .to_array();
    [-q[0], -q[1], q[2], q[3]]
}
impl InstrumentState {
    pub fn new(r: &InstrumentReference) -> Self {
        let mut state = Self {
            signals: vec![0.; r.needles.len()],
            values: vec![0.; r.needles.len()],
            digit_values: vec![0.; r.digits().len()],
            needle_angles: vec![0.; r.needles.len()],
            digit_angles: vec![0.; r.digits().len()],
            lamp_bits: 0,
            lamps: LampValues::default(),
            digits_lit: false,
            distance_km: 0.,
        };
        state.reset(r);
        state
    }
    /// Startup values. Keeps the buffers.
    pub fn reset(&mut self, r: &InstrumentReference) {
        self.signals.fill(0.);
        self.values.fill(0.);
        for (i, d) in r.digits().iter().enumerate() {
            self.digit_values[i] = d.initial_value;
            self.digit_angles[i] = d.initial_angle;
        }
        for (i, n) in r.needles.iter().enumerate() {
            self.needle_angles[i] = needle_angle(n, 0.);
        }
        self.lamp_bits = 0;
        self.digits_lit = false;
        self.distance_km = r.odometer.as_ref().map_or(0., |o| o.initial_value);
    }
    pub fn initialize_from_sample(
        &mut self,
        r: &InstrumentReference,
        limits: &GaugeLimits,
        s: InstrumentSample,
    ) {
        self.reset(r);
        self.evaluate(r, limits, s, 0., true);
    }
    pub fn update(
        &mut self,
        r: &InstrumentReference,
        limits: &GaugeLimits,
        s: InstrumentSample,
        dt: f32,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        self.evaluate(r, limits, s, dt, false);
    }
    fn evaluate(
        &mut self,
        r: &InstrumentReference,
        limits: &GaugeLimits,
        s: InstrumentSample,
        dt: f32,
        initialize: bool,
    ) {
        for (i, n) in r.needles.iter().enumerate() {
            let target = match n.signal.as_str() {
                "speed" => s.speed_kmh.abs(),
                "rpm" => s.rpm,
                "fuel" => {
                    if s.local_ignition {
                        s.fuel_liters
                    } else {
                        0.
                    }
                }
                "temp" => {
                    if s.local_ignition {
                        s.temperature
                    } else {
                        0.
                    }
                }
                _ => 0.,
            };
            self.signals[i] = if !initialize && n.signal_smoothing > 0. {
                lerp(self.signals[i], target, dt * n.signal_smoothing)
            } else {
                target
            };
            self.values[i] = if n.no_value_lerp {
                self.signals[i] + n.offset
            } else if initialize {
                self.signals[i]
            } else if n.lerp_angle {
                lerp_angle(self.values[i], self.signals[i], dt * n.value_smoothing)
            } else {
                lerp(self.values[i], self.signals[i], dt * n.value_smoothing)
            };
            self.needle_angles[i] = needle_angle(n, self.values[i]);
        }
        self.lamp_bits = self
            .lamps
            .update(&r.lamps, limits, &s, (!initialize).then_some(dt));
        self.digits_lit = meter_lit(&s);
        self.distance_km =
            r.odometer.as_ref().map_or(0., |o| o.initial_value) + (s.distance_m / 1000.) as f32;
        let Some(o) = &r.odometer else {
            return;
        };
        let value = if o.use_multiplier {
            self.distance_km * o.multiplier
        } else {
            self.distance_km
        };
        let base = o.base as f32;
        for (i, d) in o.digits.iter().enumerate() {
            if !d.direct {
                let place = base.powf(d.place as f32 - o.decimals + 1.);
                self.digit_values[i] = if place == 0. {
                    0.
                } else {
                    value % place / place * base
                };
            }
        }
        for (i, d) in o.digits.iter().enumerate() {
            let angle = digit_angle(&self.digit_values, i, d.continuous, base);
            self.digit_angles[i] = if initialize {
                angle
            } else {
                lerp_angle(self.digit_angles[i], angle, dt * 20.)
            };
        }
    }
}
