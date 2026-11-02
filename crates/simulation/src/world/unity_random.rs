//! Checked UnityPlayer random state. This stream is distinct from System.Random.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnityRandomState {
    pub words: [u32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct UnityRandom {
    state: UnityRandomState,
}
impl UnityRandom {
    pub fn restore(state: UnityRandomState) -> Self {
        Self { state }
    }
    pub fn snapshot(&self) -> UnityRandomState {
        self.state
    }
    /// Native InitState uses three identical additions, without the MT seed xor.
    pub fn new(seed: i32) -> Self {
        let mut words = [seed as u32; 4];
        for i in 1..4 {
            words[i] = words[i - 1].wrapping_mul(0x6c078965).wrapping_add(1);
        }
        Self::restore(UnityRandomState { words })
    }
    pub fn word(&mut self) -> u32 {
        let [a, b, c, d] = self.state.words;
        let t = a ^ a.wrapping_shl(11);
        let next = d ^ t ^ (((d >> 11) ^ t) >> 8);
        self.state.words = [b, c, d, next];
        next
    }
    pub fn value(&mut self) -> f32 {
        (self.word() & 0x7fffff) as f32 * f32::from_bits(0x34000001)
    }
    /// Native interpolation reverses its weight and advances even for equal bounds.
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        let u = self.value();
        let high = (1.0 - u) * max;
        let low = u * min;
        high + low
    }
    pub fn range_int(&mut self, min: i32, max: i32) -> i32 {
        if min == max {
            return min;
        }
        let word = self.word();
        if min < max {
            min.wrapping_add((word % (max as u32).wrapping_sub(min as u32)) as i32)
        } else {
            min.wrapping_sub((word % (min as u32).wrapping_sub(max as u32)) as i32)
        }
    }
    /// Uniform point in the unit disc (`insideUnitCircle`). The native draw order is not checked.
    pub fn inside_unit_circle(&mut self) -> [f32; 2] {
        let angle = self.value() * std::f32::consts::TAU;
        let radius = self.value().sqrt();
        [radius * angle.cos(), radius * angle.sin()]
    }
    /// Uniform point in the unit ball (`insideUnitSphere`). The native draw order is not checked.
    pub fn inside_unit_sphere(&mut self) -> [f32; 3] {
        let radius = self.value().cbrt();
        self.on_unit_sphere().map(|v| v * radius)
    }
    /// Native RVA ffc80 has two draws and no rejection branch.
    pub fn on_unit_sphere(&mut self) -> [f32; 3] {
        let u = self.value();
        let z = (1.0 - u) - u;
        let v = self.value();
        let angle = (1.0 - v) * std::f32::consts::TAU + v * 0.0;
        let radius = (1.0 - z * z).sqrt();
        [angle.cos() * radius, angle.sin() * radius, z]
    }
}
