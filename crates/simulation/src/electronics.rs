//! Source `electronicsscript` jam state and `empscript` sources.
use crate::reference::{ReferenceError, TorqueCurve};
use crate::simulation::dials::inverse_lerp;
use crate::world::spawn_rules::{FieldAccess, number};
use rapier3d::math::Vector;
use serde::{Deserialize, Serialize};

/// `mainscript.jamIncrease`: jam per second that an EMP adds.
pub const JAM_INCREASE: f32 = 0.3;
/// `mainscript.jamDecrease`: jam per second that electronics lose.
const JAM_DECREASE: f32 = 0.1;
/// `mainscript.maxJamValue`: jam that turns output to zero.
pub const MAX_JAM: f32 = 0.3;

/// Source `electronicsscript`.
#[derive(Clone, Debug, PartialEq)]
pub struct Electronics {
    pub max_output: f32,
    /// `overRideMaxJamValue`. Zero or less uses `MAX_JAM`.
    pub override_max_jam: f32,
    pub jam: f32,
    /// `TLastEmp`: position of the last EMP that raised the jam.
    pub last_emp: Option<Vector>,
}

impl Electronics {
    pub fn new(max_output: f32) -> Self {
        Self {
            max_output,
            override_max_jam: 0.,
            jam: 0.,
            last_emp: None,
        }
    }
    /// Reads `maxOutput`, `overRideMaxJamValue` and `jamValue`.
    pub fn from_fields<'a>(f: impl FieldAccess<'a>) -> Result<Self, ReferenceError> {
        Ok(Self {
            max_output: number(f, "maxOutput")?,
            override_max_jam: number(f, "overRideMaxJamValue")?,
            jam: number(f, "jamValue")?,
            last_emp: None,
        })
    }
    /// Share of the maximum that the jam leaves: `InverseLerp(max, 0, jam)`.
    pub fn factor(&self) -> f32 {
        let max = if self.override_max_jam > 0. {
            self.override_max_jam
        } else {
            MAX_JAM
        };
        inverse_lerp(max, 0., self.jam)
    }
    /// `UpdOutPut`.
    pub fn output(&self) -> f32 {
        self.max_output * self.factor()
    }
    /// `Update` decay.
    pub fn update(&mut self, dt: f32) {
        if self.jam > 0. {
            self.jam = (self.jam - JAM_DECREASE * dt).max(0.);
        }
    }
    /// Runs one frame: the decay, then every EMP that holds `at` on collider layer `layer`.
    pub fn expose(&mut self, emps: &[Emp], at: Vector, layer: u32, dt: f32) {
        self.update(dt);
        for emp in emps {
            emp.jam(at, layer, self, dt);
        }
    }
}

/// An `electronicsscript` on a prototype node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeElectronics {
    pub node: u32,
    /// Collider layer of the node.
    pub layer: u32,
    pub state: Electronics,
}

/// Exported `empscript` fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmpReference {
    pub affect_ambient_sound: bool,
    pub make_rabbits_evil: bool,
    pub range: f32,
    pub value: f32,
    pub range_spread: f32,
    pub range_speed: f32,
    pub value_spread: f32,
    pub value_speed: f32,
    pub fall_off: TorqueCurve,
    /// `lm` layer bits.
    pub layers: u32,
}

/// Live source `empscript`.
#[derive(Clone, Debug)]
pub struct Emp {
    pub position: Vector,
    pub affect_ambient_sound: bool,
    pub make_rabbits_evil: bool,
    range: f32,
    value: f32,
    range_spread: f32,
    range_speed: f32,
    value_spread: f32,
    value_speed: f32,
    fall_off: TorqueCurve,
    layers: u32,
    noise_range: f32,
    noise_value: f32,
    local_range: f32,
    local_value: f32,
}

impl Emp {
    pub fn new(r: &EmpReference, position: Vector) -> Self {
        let mut emp = Self {
            position,
            affect_ambient_sound: r.affect_ambient_sound,
            make_rabbits_evil: r.make_rabbits_evil,
            range: r.range,
            value: r.value,
            range_spread: r.range_spread,
            range_speed: r.range_speed,
            value_spread: r.value_spread,
            value_speed: r.value_speed,
            fall_off: r.fall_off.clone(),
            layers: r.layers,
            noise_range: 0.,
            noise_value: 0.,
            local_range: 0.,
            local_value: 0.,
        };
        emp.advance(0.);
        emp
    }
    /// `Update` noise: range and value of this frame.
    pub fn advance(&mut self, dt: f32) {
        self.local_range = self.range;
        self.local_value = self.value;
        if self.range_spread > 0. {
            self.noise_range += self.range_speed * dt;
            if self.noise_range > 1. {
                self.noise_range -= 1.;
            }
            let n = terrain::source_perlin(self.noise_range, self.noise_range);
            self.local_range += (n - 0.5) * self.range_spread;
        }
        if self.value_spread > 0. {
            self.noise_value += self.value_speed * dt;
            if self.noise_value > 1. {
                self.noise_value -= 1.;
            }
            let n = terrain::source_perlin(self.noise_value, -self.noise_value);
            self.local_value += (n - 0.5) * self.value_spread;
        }
    }
    /// `OverlapSphere` hit of electronics at `at` on collider layer `layer`. Raises the jam
    /// toward the fall-off value. Returns true when the sphere holds the target.
    pub fn jam(&self, at: Vector, layer: u32, e: &mut Electronics, dt: f32) -> bool {
        let distance = (at - self.position).length();
        if layer >= 32 || self.layers & (1 << layer) == 0 || distance > self.local_range {
            return false;
        }
        let t = inverse_lerp(0., self.local_range, distance);
        let target = self.local_value * crate::simulation::engine::torque(&self.fall_off, t);
        if e.jam < target {
            e.last_emp = Some(self.position);
            e.jam = (e.jam + JAM_INCREASE * dt).min(target);
        }
        true
    }
}
