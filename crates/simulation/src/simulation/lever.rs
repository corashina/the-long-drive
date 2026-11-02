//! Source `carscript.Update` gear lever motion.
use crate::reference::{LeverReference, LeverTiming, Transform};
use rapier3d::math::{Rotation, Vector};

/// The lever pose in its parent space, and the shift in progress.
#[derive(Clone, Copy, Debug)]
pub struct LeverState {
    pub translation: Vector,
    pub rotation: Rotation,
    last: usize,
    gear: usize,
    remaining: f32,
}

fn parts(t: &Transform) -> (Vector, Rotation) {
    (
        Vector::from_array(t.translation),
        Rotation::from_array(t.rotation),
    )
}

/// Unity `Mathf.InverseLerp`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    ((value - a) / (b - a)).clamp(0., 1.)
}

impl LeverState {
    /// At rest on `gear`.
    pub fn rest(reference: &LeverReference, gear: usize) -> Self {
        let (translation, rotation) = parts(&reference.gears[gear].pos);
        Self {
            translation,
            rotation,
            last: gear,
            gear,
            remaining: 0.,
        }
    }

    /// Source `ShiftTo` and `Shifted` when the gear changes.
    pub fn shift(&mut self, timing: &LeverTiming, last: usize, gear: usize) {
        self.last = last;
        self.gear = gear;
        self.remaining = timing.seconds;
    }

    /// Each stage lerps from the current pose with weight = stage progress, as the source does.
    pub fn step(&mut self, reference: &LeverReference, timing: &LeverTiming, dt: f32) {
        let gears = &reference.gears;
        let [first, second] = timing.stages;
        let (target, weight) = if self.remaining > 0. {
            self.remaining -= dt;
            let t = inverse_lerp(timing.seconds, 0., self.remaining);
            if t < first {
                (&gears[self.last].path, inverse_lerp(0., first, t))
            } else if t < second {
                (&gears[self.gear].path, inverse_lerp(first, second, t))
            } else {
                (&gears[self.gear].pos, inverse_lerp(second, 1., t))
            }
        } else {
            (&gears[self.gear].pos, 1.)
        };
        let (translation, rotation) = parts(target);
        self.translation = self.translation.lerp(translation, weight);
        self.rotation = self.rotation.lerp(rotation, weight);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::LeverGear;

    fn pose(x: f32, rotation: [f32; 4]) -> Transform {
        Transform {
            translation: [x, 0., 0.],
            rotation,
            scale: [1.; 3],
        }
    }

    #[test]
    fn shift_moves_through_three_stages_to_rest() {
        let turned = [
            0.,
            0.,
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        ];
        let reference = LeverReference {
            node: 1,
            gears: vec![
                LeverGear {
                    path: pose(1., [0., 0., 0., 1.]),
                    pos: pose(0., [0., 0., 0., 1.]),
                },
                LeverGear {
                    path: pose(2., [0., 0., 0., 1.]),
                    pos: pose(4., turned),
                },
            ],
        };
        let timing = LeverTiming {
            seconds: 0.25,
            stages: [0.33333, 0.66666],
        };
        let mut lever = LeverState::rest(&reference, 0);
        lever.step(&reference, &timing, 0.02);
        assert_eq!(lever.translation.x, 0.);
        lever.shift(&timing, 0, 1);
        let x = |lever: &LeverState| lever.translation.x;
        // t = 0.2: 0.6 of the way to the old gear path.
        lever.step(&reference, &timing, 0.05);
        assert!((x(&lever) - 0.6).abs() < 1e-4);
        // t = 0.5: half way from the current pose to the new gear path.
        lever.step(&reference, &timing, 0.075);
        assert!((x(&lever) - 1.3).abs() < 1e-3);
        // t = 0.9: 0.7 of the way to the new gear pos.
        lever.step(&reference, &timing, 0.1);
        assert!((x(&lever) - (1.3 + 0.7 * 2.7)).abs() < 1e-3);
        assert!(lever.rotation.z > 0.3 && lever.rotation.z < 0.7);
        // The last stage step ends on the new gear pos and stays at rest.
        for _ in 0..2 {
            lever.step(&reference, &timing, 0.05);
            assert!((x(&lever) - 4.).abs() < 1e-6);
            assert!((lever.rotation.z - turned[2]).abs() < 1e-6);
        }
    }
}
