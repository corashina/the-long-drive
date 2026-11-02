//! Source `electronicsscript` jam state and `empscript` sources.
use crate::reference::{ReferenceError, TorqueCurve};
use crate::simulation::dials::inverse_lerp;
use crate::world::spawn_rules::{FieldAccess, curve, fields, integer, number, value};
use rapier3d::math::Vector;

/// `mainscript.jamIncrease`: jam per second that an EMP adds.
pub const JAM_INCREASE: f32 = 0.3;
/// `mainscript.jamDecrease`: jam per second that electronics lose.
pub const JAM_DECREASE: f32 = 0.1;
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
}

/// Source `empscript`.
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
    /// `lm` layer bits.
    layers: u32,
    noise_range: f32,
    noise_value: f32,
    local_range: f32,
    local_value: f32,
}

impl Emp {
    pub fn from_fields<'a>(
        f: impl FieldAccess<'a>,
        position: Vector,
    ) -> Result<Self, ReferenceError> {
        let fall_off = curve(f, "fallOff")?;
        let mut emp = Self {
            position,
            affect_ambient_sound: number(f, "affectAmbientSound")? != 0.,
            make_rabbits_evil: number(f, "makeRabbitsEvil")? != 0.,
            range: number(f, "range")?,
            value: number(f, "value")?,
            range_spread: number(f, "rangeSpread")?,
            range_speed: number(f, "rangeSpeed")?,
            value_spread: number(f, "valueSpread")?,
            value_speed: number(f, "valueSpeed")?,
            fall_off,
            layers: integer(fields(value(f, "lm")?)?, "m_Bits")? as u32,
            noise_range: number(f, "noiseRange")?,
            noise_value: number(f, "noiseValue")?,
            local_range: 0.,
            local_value: 0.,
        };
        emp.advance(0.);
        Ok(emp)
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
    /// `OverlapSphere` hit of electronics at `at` on collider layer `layer`: raises the jam
    /// toward the fall-off value. Returns true when the sphere holds the target.
    pub fn jam(&self, at: Vector, layer: u32, e: &mut Electronics, dt: f32) -> bool {
        let distance = (at - self.position).length();
        if layer >= 32 || self.layers & (1 << layer) == 0 || !(distance <= self.local_range) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::CurveKey;

    fn emp(value: f32) -> Emp {
        let key = |time, value| CurveKey {
            time,
            value,
            in_tangent: 0.,
            out_tangent: 0.,
            in_weight: 0.,
            out_weight: 0.,
            weighted_mode: 0,
        };
        Emp {
            position: Vector::ZERO,
            affect_ambient_sound: false,
            make_rabbits_evil: false,
            range: 10.,
            value,
            range_spread: 0.,
            range_speed: 0.,
            value_spread: 0.,
            value_speed: 0.,
            fall_off: TorqueCurve {
                keys: vec![key(0., 1.), key(1., 1.)],
                pre_wrap_mode: 0,
                post_wrap_mode: 0,
            },
            layers: 1,
            noise_range: 0.,
            noise_value: 0.,
            local_range: 10.,
            local_value: value,
        }
    }

    #[test]
    fn emp_jams_up_to_its_value_and_decays() {
        let emp = emp(0.6);
        let mut e = Electronics::new(1.);
        for _ in 0..100 {
            assert!(emp.jam(Vector::new(3., 0., 0.), 0, &mut e, 0.1));
        }
        assert_eq!(e.jam, 0.6);
        assert_eq!(e.output(), 0.);
        assert_eq!(e.last_emp, Some(Vector::ZERO));
        // Outside the sphere or off the layer mask nothing changes.
        assert!(!emp.jam(Vector::new(11., 0., 0.), 0, &mut e, 0.1));
        assert!(!emp.jam(Vector::ZERO, 1, &mut e, 0.1));
        e.jam = 0.15;
        assert!((e.output() - 0.5).abs() < 1e-6);
        e.update(1.);
        assert!((e.jam - 0.05).abs() < 1e-6);
        e.update(1.);
        assert_eq!(e.jam, 0.);
    }
}
