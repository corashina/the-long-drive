//! Source `lightdither`: Perlin dithers set the intensity of one light each frame.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::DitherReference;

    fn dither(min_threshold: f32, max_threshold: f32, value: f32) -> DitherReference {
        DitherReference {
            speed: 5.,
            speed2: 1.,
            min_threshold,
            max_threshold,
            min_value: 1.,
            max_value: 3.,
            value,
        }
    }

    #[test]
    fn values_follow_the_noise_inside_the_thresholds() {
        let light: crate::reference::LightReference = serde_json::from_value(serde_json::json!({
            "id": 2, "source": {"file": "level1", "path_id": 1}, "node": 1,
            "pose": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]},
            "kind": "point", "color": [1.0, 1.0, 1.0, 1.0], "serialized_intensity": 0.0,
            "startup_intensity": 0.0, "range_meters": 2.5, "full_cone_degrees": 30.0,
            "inner_cone_degrees": 0.0, "enabled": true, "culling_mask": 1,
            "shadows": {"kind": 0, "strength": 1.0, "bias": 0.1, "normal_bias": 0.0,
                "near_plane": 0.1, "resolution": -1, "custom_resolution": -1},
            "cookie": null, "cookie_size": 10.0
        }))
        .unwrap();
        let mut r = LightDitherReference {
            source: light.source.clone(),
            light,
            // Noise is in [0, 1]: the first dither never changes, the second always does.
            dithers: vec![dither(2., 3., 7.), dither(0., 1., 7.)],
            max: false,
            min: false,
        };
        let mut state = LightDither::new(&r, 1);
        let sum = state.step(&r, 1. / 60.);
        let [x, y] = state.offsets[1];
        let f = terrain::source_perlin(x + 5. / 60., y);
        assert!((state.values[1] - (1. + 2. * f)).abs() < 1e-5);
        assert_eq!(state.values[0], 7.);
        assert!((sum - (7. + state.values[1])).abs() < 1e-5);
        r.max = true;
        assert_eq!(state.step(&r, 1. / 60.), 7.);
        r.max = false;
        r.min = true;
        assert!(state.step(&r, 1. / 60.) < 7.);
    }
}
