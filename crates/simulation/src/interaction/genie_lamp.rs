//! Source ginlampscript: rubbing its wash trigger lights the lamp.
use crate::reference::GenieLampReference;

/// Seconds after the last rub while the lamp still counts as rubbed.
const RUB_HOLD: f64 = 1.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenieLamp {
    pub value: f32,
    pub rubbing: bool,
    last_rubbed: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampOutput {
    pub intensity: f32,
    pub range: f32,
    pub emission: [f32; 4],
}
impl GenieLamp {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            rubbing: false,
            last_rubbed: f64::NEG_INFINITY,
        }
    }
    /// Update. `rubbed` is a positive wash trigger repairValue, which the caller resets to 0.
    pub fn update(
        &mut self,
        r: &GenieLampReference,
        rubbed: bool,
        time: f64,
        dt: f32,
        sleep_modifier: f32,
    ) -> LampOutput {
        if rubbed {
            self.rubbing = true;
            self.last_rubbed = time;
        } else if time - self.last_rubbed >= RUB_HOLD {
            self.rubbing = false;
        }
        if self.rubbing {
            if self.value < 1. {
                self.value = (self.value + r.gain * dt * sleep_modifier).min(1.);
            }
        } else if self.value > 0. {
            self.value = (self.value - r.lose * dt * sleep_modifier).max(0.);
        }
        output(r, self.value)
    }
}
pub fn output(r: &GenieLampReference, value: f32) -> LampOutput {
    LampOutput {
        intensity: crate::simulation::engine::torque(&r.intensity, value),
        range: crate::simulation::engine::torque(&r.range, value),
        emission: crate::lighting::gradient(&r.emission, value),
    }
}
