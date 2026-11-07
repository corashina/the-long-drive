//! Thunderstorm lightning. A new feature: the source has no lightning.
//! A strike flashes the light and shows a bolt. Its thunder plays after the sound travel time.
use super::StormRandom;

/// Seconds between strikes at full thunderstorm weight.
const WAIT_SECONDS: [f32; 2] = [3., 12.];
/// Strike distance from the player, in meters.
pub const STRIKE_DISTANCE: [f32; 2] = [300., 3000.];
/// Seconds of the flash decay from 1 to 0.
const FLASH_SECONDS: f32 = 0.3;
/// Speed of sound in meters per second.
const SOUND_SPEED: f32 = 343.;
/// Thunder that waits in the queue. A full queue drops the new thunder.
const QUEUE: usize = 4;
/// Seconds that a bolt shows.
pub const BOLT_SECONDS: f32 = 0.2;
/// Bolt start height above the strike point, in meters.
const CLOUD_HEIGHT: [f32; 2] = [500., 900.];
/// Thunder nearer than this distance in meters plays the close crack.
pub const CLOSE_DISTANCE: f32 = 600.;

/// One queued thunder. `variant` is 0 to 1 and selects the clip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thunder {
    pub distance: f32,
    pub variant: f32,
    delay: f32,
}

/// The visible bolt of the last strike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bolt {
    /// Absolute strike point on the ground.
    pub position: [f64; 3],
    /// Start height above `position`, in meters.
    pub height: f32,
    /// Selects the jagged shape. The position gives it, so all peers draw the same bolt.
    pub seed: u32,
    /// Seconds since the strike.
    pub age: f32,
}

#[derive(Debug)]
pub struct Lightning {
    random: StormRandom,
    /// Seconds at full thunderstorm weight until the next strike.
    wait: f32,
    flash: f32,
    queue: [Thunder; QUEUE],
    count: usize,
    bolt: Option<Bolt>,
}

impl Lightning {
    pub fn new(seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x6c69_6768_746e_0001);
        let wait = random.range(WAIT_SECONDS[0], WAIT_SECONDS[1]);
        Self {
            random,
            wait,
            flash: 0.,
            queue: [Thunder::default(); QUEUE],
            count: 0,
            bolt: None,
        }
    }

    /// 0 to 1. A strike sets 1.
    pub fn flash(&self) -> f32 {
        self.flash
    }

    /// The bolt while it shows.
    pub fn bolt(&self) -> Option<Bolt> {
        self.bolt
    }

    /// Decays the flash and moves the thunder delays. `draw` lets this step draw a strike.
    /// `storm` is the thunderstorm weight at the player, 0 to 1.
    /// `height_at` gives the ground height at absolute `x`, `z`.
    /// Returns the absolute strike position when a strike occurs.
    pub fn advance(
        &mut self,
        dt: f32,
        storm: f32,
        player: [f64; 3],
        draw: bool,
        height_at: impl FnOnce([f64; 2]) -> f32,
    ) -> Option<[f64; 3]> {
        self.flash = (self.flash - dt / FLASH_SECONDS).max(0.);
        if let Some(bolt) = &mut self.bolt {
            bolt.age += dt;
            if bolt.age >= BOLT_SECONDS {
                self.bolt = None;
            }
        }
        for thunder in &mut self.queue[..self.count] {
            thunder.delay -= dt;
        }
        if !draw {
            return None;
        }
        if storm <= 0. {
            return None;
        }
        self.wait -= dt * storm;
        if self.wait > 0. {
            return None;
        }
        self.wait = self.random.range(WAIT_SECONDS[0], WAIT_SECONDS[1]);
        let angle = self.random.range(0., std::f32::consts::TAU);
        let distance = self.random.range(STRIKE_DISTANCE[0], STRIKE_DISTANCE[1]);
        let mut position = player;
        position[0] += (angle.cos() * distance) as f64;
        position[2] += (angle.sin() * distance) as f64;
        position[1] = height_at([position[0], position[2]]) as f64;
        self.strike(position, player);
        Some(position)
    }

    /// Flashes, shows a bolt and queues thunder at the player distance to `position`.
    pub fn strike(&mut self, position: [f64; 3], player: [f64; 3]) {
        self.flash = 1.;
        let bits = position[0].to_bits() ^ position[2].to_bits().rotate_left(32);
        let seed = crate::world::hash(bits) as u32;
        let t = (seed >> 8) as f32 / 16_777_216.;
        self.bolt = Some(Bolt {
            position,
            height: CLOUD_HEIGHT[0] + (CLOUD_HEIGHT[1] - CLOUD_HEIGHT[0]) * t,
            seed,
            age: 0.,
        });
        let distance = position
            .iter()
            .zip(player)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt() as f32;
        if self.count < QUEUE {
            self.queue[self.count] = Thunder {
                distance,
                variant: self.random.unit(),
                delay: distance / SOUND_SPEED,
            };
            self.count += 1;
        }
    }

    /// Removes and returns one thunder whose delay is over.
    pub fn pop_thunder(&mut self) -> Option<Thunder> {
        let index = self.queue[..self.count]
            .iter()
            .position(|t| t.delay <= 0.)?;
        let thunder = self.queue[index];
        self.count -= 1;
        self.queue[index] = self.queue[self.count];
        Some(thunder)
    }

    /// A load starts with no flash, no bolt and no thunder. The random stream keeps its state.
    pub fn clear(&mut self) {
        self.flash = 0.;
        self.count = 0;
        self.bolt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_strike_gives_one_thunder_at_the_delay() {
        let mut lightning = Lightning::new(1);
        lightning.strike([686., 0., 0.], [0.; 3]);
        assert_eq!(lightning.flash(), 1.);
        let dt = 0.01;
        let mut seconds = 0.;
        let thunder = loop {
            lightning.advance(dt, 0., [0.; 3], false, |_| 0.);
            seconds += dt;
            if let Some(thunder) = lightning.pop_thunder() {
                break thunder;
            }
            assert!(seconds < 3.);
        };
        assert!((seconds - 2.).abs() < 0.02);
        assert_eq!(thunder.distance, 686.);
        assert_eq!(lightning.flash(), 0.);
        assert_eq!(lightning.bolt(), None);
        lightning.advance(dt, 0., [0.; 3], false, |_| 0.);
        assert_eq!(lightning.pop_thunder(), None);
    }
}
