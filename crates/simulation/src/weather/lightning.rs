//! Thunderstorm lightning. A new feature: the source has no lightning.
//! A strike flashes the light. Its thunder plays after the sound travel time.
use super::StormRandom;

/// Seconds between strikes at full thunderstorm weight.
const WAIT_SECONDS: [f32; 2] = [6., 25.];
/// Strike distance from the player, in meters.
pub const STRIKE_DISTANCE: [f32; 2] = [300., 3000.];
/// Seconds of the flash decay from 1 to 0.
const FLASH_SECONDS: f32 = 0.15;
/// Speed of sound in meters per second.
const SOUND_SPEED: f32 = 343.;
/// Thunder that waits in the queue. A full queue drops the new thunder.
const QUEUE: usize = 4;

/// One queued thunder. `variant` is 0 to 1 and selects the clip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thunder {
    pub distance: f32,
    pub variant: f32,
    delay: f32,
}

#[derive(Debug)]
pub struct Lightning {
    random: StormRandom,
    /// Seconds at full thunderstorm weight until the next strike.
    wait: f32,
    flash: f32,
    queue: [Thunder; QUEUE],
    count: usize,
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
        }
    }

    /// 0 to 1. A strike sets 1.
    pub fn flash(&self) -> f32 {
        self.flash
    }

    /// Decays the flash and moves the thunder delays. `draw` lets this step draw a strike.
    /// `storm` is the thunderstorm weight at the player, 0 to 1.
    /// Returns the absolute strike position when a strike occurs.
    pub fn advance(
        &mut self,
        dt: f32,
        storm: f32,
        player: [f64; 3],
        draw: bool,
    ) -> Option<[f64; 3]> {
        self.flash = (self.flash - dt / FLASH_SECONDS).max(0.);
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
        self.strike(position, player);
        Some(position)
    }

    /// Flashes and queues thunder at the player distance to `position`.
    pub fn strike(&mut self, position: [f64; 3], player: [f64; 3]) {
        self.flash = 1.;
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

    /// A load starts with no flash and no thunder. The random stream keeps its state.
    pub fn clear(&mut self) {
        self.flash = 0.;
        self.count = 0;
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
            lightning.advance(dt, 0., [0.; 3], false);
            seconds += dt;
            if let Some(thunder) = lightning.pop_thunder() {
                break thunder;
            }
            assert!(seconds < 3.);
        };
        assert!((seconds - 2.).abs() < 0.02);
        assert_eq!(thunder.distance, 686.);
        assert_eq!(lightning.flash(), 0.);
        lightning.advance(dt, 0., [0.; 3], false);
        assert_eq!(lightning.pop_thunder(), None);
    }
}
