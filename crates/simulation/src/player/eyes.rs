//! Source `bulgingeyesScript.Update`: the eyes of a player figure grow while its player zooms.
use crate::reference::BulgingEyesReference;
use crate::simulation::engine::torque;

#[derive(Clone, Copy, Debug, Default)]
pub struct BulgingEyes {
    /// `active`: the `zooming` of the player at the last step.
    pub active: bool,
    /// `current`: 0 is the start scale, 1 the end of the curves.
    current: f32,
}

impl BulgingEyes {
    /// `active` is the `zooming` of the player.
    pub fn step(&mut self, r: &BulgingEyesReference, active: bool, dt: f32) {
        self.active = active;
        let (target, speed) = if active {
            (1., r.speed_forward)
        } else {
            (0., r.speed_back)
        };
        self.current += (target - self.current) * (dt * speed).clamp(0., 1.);
    }

    /// `Refresh`: the local scale of each eye.
    pub fn scale(&self, r: &BulgingEyesReference) -> [f32; 3] {
        std::array::from_fn(|i| {
            let t = torque(&r.curves[i], self.current);
            r.start_scale[i] + (r.end_scale[i] - r.start_scale[i]) * t
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{CurveKey, ManagedEvidence, SourceId, TorqueCurve};

    fn key(time: f32, value: f32) -> CurveKey {
        CurveKey {
            time,
            value,
            in_tangent: 0.,
            out_tangent: 0.,
            in_weight: 1. / 3.,
            out_weight: 1. / 3.,
            weighted_mode: 0,
        }
    }

    /// The source `animx` curve.
    fn reference() -> BulgingEyesReference {
        let curve = TorqueCurve {
            keys: vec![key(0., 0.), key(0.5, 0.), key(0.8, 1.25), key(1., 1.)],
            pre_wrap_mode: 2,
            post_wrap_mode: 2,
        };
        BulgingEyesReference {
            source: SourceId {
                file: "level1".into(),
                path_id: 66030,
            },
            eyes: Vec::new(),
            start_scale: [1.; 3],
            end_scale: [2., 2.6, 2.],
            curves: [curve.clone(), curve.clone(), curve],
            speed_forward: 6.,
            speed_back: 8.,
            evidence: ManagedEvidence {
                assembly_sha256: String::new(),
                methods: Vec::new(),
            },
        }
    }

    #[test]
    fn eyes_grow_while_zooming_and_return_after() {
        let r = reference();
        let mut eyes = BulgingEyes::default();
        assert_eq!(eyes.scale(&r), [1.; 3]);
        for _ in 0..200 {
            eyes.step(&r, true, 0.02);
        }
        let full = eyes.scale(&r);
        assert!((full[0] - 2.).abs() < 1e-3 && (full[1] - 2.6).abs() < 1e-3);
        // The curve overshoots on the way: at 0.8 the eye is 1.25 of the bulge.
        eyes.current = 0.8;
        assert!((eyes.scale(&r)[0] - 2.25).abs() < 1e-5);
        for _ in 0..200 {
            eyes.step(&r, false, 0.02);
        }
        assert!((eyes.scale(&r)[0] - 1.).abs() < 1e-3);
    }
}
