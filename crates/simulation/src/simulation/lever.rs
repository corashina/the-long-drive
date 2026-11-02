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
