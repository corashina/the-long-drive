//! Source `lightdither`. Perlin dithers set the intensity of one light each frame.
use crate::reference::LightDitherReference;
use crate::simulation::dials::inverse_lerp;
use crate::world::unity_random::UnityRandom;

/// One `lightdither` instance: the `Start` offsets and the current values.
#[derive(Clone, Debug)]
pub struct LightDither {
    offsets: Vec<[f32; 2]>,
    values: Vec<f32>,
}
impl LightDither {
    /// `Start`: `offsetX` and `offsetY` are `Random.Range(0, 1)` per dither.
    pub fn new(r: &LightDitherReference, seed: i32) -> Self {
        let mut random = UnityRandom::new(seed);
        Self {
            offsets: r
                .dithers
                .iter()
                .map(|_| [random.range(0., 1.), random.range(0., 1.)])
                .collect(),
            values: r.dithers.iter().map(|d| d.value).collect(),
        }
    }
    /// `Update` with the frame `deltaTime`. Returns the light intensity.
    /// The source samples the noise at `deltaTime`, not at the time. This copies it.
    pub fn step(&mut self, r: &LightDitherReference, dt: f32) -> f32 {
        for ((d, [x, y]), value) in r.dithers.iter().zip(&self.offsets).zip(&mut self.values) {
            let f = terrain::source_perlin(x + dt * d.speed * d.speed2, *y);
            // Below or above the thresholds the value does not change.
            if f >= d.min_threshold && f <= d.max_threshold {
                let t = inverse_lerp(d.min_threshold, d.max_threshold, f);
                *value = d.min_value + (d.max_value - d.min_value) * t;
            }
        }
        let values = self.values.iter().copied();
        if r.max {
            values.fold(f32::NEG_INFINITY, f32::max)
        } else if r.min {
            values.fold(f32::INFINITY, f32::min)
        } else {
            values.sum()
        }
    }
}
