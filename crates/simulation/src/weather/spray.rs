//! Wheel spray amount on wet ground.

/// Most spray emitters in one frame. The renderer has the same value.
pub const MAX_SPRAY: usize = 32;
/// Spray needs a faster car, in meters per second.
pub const SPRAY_SPEED: f32 = 8.;
/// Spray needs more wetness.
pub const SPRAY_GROUND: f32 = 0.3;
/// Cars farther from the player give no spray, in meters.
pub const SPRAY_DISTANCE: f32 = 250.;

/// Spray of one wheel, 0..1, by the car speed in m/s and the ground state.
pub fn spray_amount(speed: f32, wetness: f32) -> f32 {
    if speed <= SPRAY_SPEED || wetness <= SPRAY_GROUND {
        return 0.;
    }
    let ground = (wetness - SPRAY_GROUND) / (1. - SPRAY_GROUND);
    let speed = ((speed - SPRAY_SPEED) / 20.).min(1.);
    ground * (0.3 + 0.7 * speed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spray_needs_speed_and_wet_ground() {
        assert_eq!(spray_amount(7., 1.), 0.);
        assert_eq!(spray_amount(20., 0.3), 0.);
        assert!(spray_amount(20., 0.8) > 0.);
        assert_eq!(spray_amount(60., 1.), 1.);
    }
}
