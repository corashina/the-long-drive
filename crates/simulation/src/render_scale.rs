//! Scene pixel sizing. Display coordinates and source projections stay native.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneScale(u32);

impl Default for SceneScale {
    fn default() -> Self {
        Self(100)
    }
}

impl SceneScale {
    /// Menu steps in percent of the native extent.
    pub const STEPS: [u32; 5] = [100, 85, 75, 67, 50];

    pub fn from_percent(percent: u32) -> Result<Self, &'static str> {
        if Self::STEPS.contains(&percent) {
            Ok(Self(percent))
        } else {
            Err("Scene resolution must be 100%, 85%, 75%, 67% or 50%")
        }
    }

    pub fn percent(self) -> u32 {
        self.0
    }

    pub fn extent(self, native: [u32; 2]) -> [u32; 2] {
        native.map(|side| {
            if side == 0 {
                0
            } else {
                ((u64::from(side) * u64::from(self.0) / 100) as u32).max(1)
            }
        })
    }
}

/// Windows past a threshold before Auto changes one step.
const CONFIRM: u8 = 2;
/// Auto raises only when the predicted GPU time at the next step is under this part of the budget.
const RAISE_HEADROOM: f32 = 0.75;

/// Auto scene scale. It steps on the GPU world time of each timing window.
#[derive(Clone, Copy, Debug, Default)]
pub struct AutoScale {
    step: usize,
    over: u8,
    under: u8,
    /// The first window after a change mixes two scales.
    settling: bool,
}

impl AutoScale {
    pub fn scale(&self) -> SceneScale {
        SceneScale(SceneScale::STEPS[self.step])
    }

    /// Takes the average GPU ms of one window. Returns the scale for the next frames.
    pub fn window(&mut self, gpu_ms: f32, budget_ms: f32) -> SceneScale {
        if std::mem::take(&mut self.settling) {
            return self.scale();
        }
        let steps = SceneScale::STEPS;
        let current = steps[self.step] as f32;
        let over = self.step + 1 < steps.len() && gpu_ms > budget_ms;
        // GPU time follows the pixel count, so it grows with the square of the scale.
        let under = self.step > 0
            && gpu_ms * (steps[self.step - 1] as f32 / current).powi(2)
                < budget_ms * RAISE_HEADROOM;
        self.over = if over { self.over + 1 } else { 0 };
        self.under = if under { self.under + 1 } else { 0 };
        if self.over >= CONFIRM {
            self.step += 1;
        } else if self.under >= CONFIRM {
            self.step -= 1;
        } else {
            return self.scale();
        }
        self.over = 0;
        self.under = 0;
        self.settling = true;
        self.scale()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_steps_with_hysteresis() {
        let budget = 16.;
        let mut auto = AutoScale::default();
        // One slow window does not change the scale.
        assert_eq!(auto.window(20., budget).percent(), 100);
        assert_eq!(auto.window(20., budget).percent(), 85);
        // The step ignores the settling window.
        assert_eq!(auto.window(30., budget).percent(), 85);
        for _ in 0..20 {
            auto.window(30., budget);
        }
        assert_eq!(auto.scale().percent(), 50);
        // The time is under budget but too close for a raise. Hold the scale so it does not flip.
        for _ in 0..10 {
            assert_eq!(auto.window(10., budget).percent(), 50);
        }
        assert_eq!(auto.window(5., budget).percent(), 50);
        assert_eq!(auto.window(5., budget).percent(), 67);
        auto.window(1., budget);
        for _ in 0..20 {
            auto.window(1., budget);
        }
        assert_eq!(auto.scale().percent(), 100);
    }
}
