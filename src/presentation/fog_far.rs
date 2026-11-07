//! The world view far plane stops where the weather fog is full. The GPU then draws no
//! geometry that shows only the fog color. The world step selection keeps the full far plane.

/// The fog factor exp2(-(1.2011 density d)^2) = exp(-(density d)^2) is below 1/1000 past this
/// density d. 1/1000, not 1/255, because HDR emission can be bright.
const FULL_FOG: f32 = 2.63;
/// Behind the cut the sky shows. Below this sky share of the fog color the far plane is not cut.
const MIN_OVERCAST: f32 = 0.9;
/// Ratio of two far plane steps. A step keeps bundles and visibility caches for many frames.
const STEP: f32 = 1.1;
/// The fog must be this much thinner than a step before the far plane moves in.
const HYSTERESIS: f32 = 1.05;
/// The far plane moves out by one step at most each this many seconds, so it eases out.
const GROW_SECONDS: f32 = 0.1;

#[derive(Default)]
pub struct FogFar {
    /// The far plane is `far_plane / STEP^level`.
    level: u32,
    wait: f32,
}

/// The largest level whose far plane is not nearer than `limit`.
fn level(far_plane: f32, limit: f32) -> u32 {
    if limit >= far_plane {
        return 0;
    }
    ((far_plane / limit).ln() / STEP.ln()) as u32
}

impl FogFar {
    /// The world view far plane of this frame. `floor` is the nearest far plane, for example
    /// the shadow distance, so the shadow cascades do not change.
    pub fn advance(
        &mut self,
        far_plane: f32,
        floor: f32,
        density: f32,
        overcast: f32,
        seconds: f32,
    ) -> f32 {
        let limit = if density > 0. && overcast >= MIN_OVERCAST {
            (FULL_FOG / density).max(floor)
        } else {
            far_plane
        };
        let need = level(far_plane, limit);
        self.wait = (self.wait - seconds).max(0.);
        if need < self.level {
            if self.wait <= 0. {
                self.level -= 1;
                self.wait = GROW_SECONDS;
            }
        } else {
            self.level = self.level.max(level(far_plane, limit * HYSTERESIS));
        }
        far_plane / STEP.powi(self.level as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_in_fog_and_eases_out() {
        let mut far = FogFar::default();
        let density = 1.73 / 120.;
        let cut = far.advance(3000., 100., density, 1., 0.016);
        assert!(cut >= FULL_FOG / density && cut < FULL_FOG / density * STEP * HYSTERESIS);
        assert_eq!(far.advance(3000., 100., density * 1.01, 1., 0.016), cut);
        // The sky goes back to blue: the far plane moves out one step each GROW_SECONDS.
        let next = far.advance(3000., 100., density, 0.5, 0.05);
        assert!((next / cut - STEP).abs() < 1e-3);
        assert_eq!(far.advance(3000., 100., density, 0.5, 0.05), next);
        let mut last = next;
        for _ in 0..100 {
            last = far.advance(3000., 100., density, 0.5, 0.1);
        }
        assert_eq!(last, 3000.);
        // The shadow distance is the floor.
        assert!(far.advance(3000., 500., density, 1., 0.1) >= 500.);
    }
}
