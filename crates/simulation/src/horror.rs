//! Source `mainscript.RandomHorrorSound`: every few nights the player horror source plays for a
//! few seconds. The night count is not saved, as in the source.
use crate::weather::StormRandom;

/// Source `minHorrorNight` and `maxHorrorNight`.
const NIGHTS: [u32; 2] = [1, 5];
/// Source play length range in seconds.
const DURATION: [f32; 2] = [5., 15.];

#[derive(Debug)]
pub struct HorrorSound {
    added: bool,
    current_night: u32,
    selected_night: u32,
    duration: f32,
    start: f64,
    playing: bool,
    random: StormRandom,
}

impl HorrorSound {
    pub fn new(seed: u64) -> Self {
        Self {
            added: false,
            current_night: 0,
            selected_night: 0,
            duration: 0.,
            start: 0.,
            playing: false,
            random: StormRandom(seed ^ 0x686f_7272_6f72_0001),
        }
    }

    /// One source `Update`. Returns true while the horror source plays.
    pub fn step(&mut self, night: bool, now: f64) -> bool {
        if night {
            if !self.added {
                self.added = true;
                self.current_night += 1;
                if self.current_night >= self.selected_night {
                    // Random.Range(int, int) excludes the maximum, so the source adds 1.
                    let nights = NIGHTS[1] - NIGHTS[0] + 1;
                    let pick = ((self.random.unit() * nights as f32) as u32).min(nights - 1);
                    self.selected_night = self.current_night + NIGHTS[0] + pick;
                    self.duration = self.random.range(DURATION[0], DURATION[1]);
                    self.playing = true;
                    self.start = now;
                }
            }
        } else {
            self.added = false;
            self.playing = false;
        }
        if now - self.start > f64::from(self.duration) {
            self.playing = false;
        }
        self.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps one night and one day of 100 s each. Returns the play length of the night.
    fn night_and_day(h: &mut HorrorSound, now: &mut f64) -> f64 {
        let mut played = 0.;
        for night in [true, false] {
            for _ in 0..1000 {
                if h.step(night, *now) {
                    assert!(night, "the sound stops at day");
                    played += 0.1;
                }
                *now += 0.1;
            }
        }
        played
    }

    #[test]
    fn plays_5_to_15_s_every_1_to_5_nights() {
        let mut h = HorrorSound::new(7);
        let mut now = 0.;
        let mut nights = Vec::new();
        for night in 1..=60 {
            let played = night_and_day(&mut h, &mut now);
            if played > 0. {
                assert!((4.9..=15.1).contains(&played), "play length {played}");
                nights.push(night);
            }
        }
        assert_eq!(nights[0], 1, "the first night plays");
        assert!(nights.windows(2).all(|w| (1..=5).contains(&(w[1] - w[0]))));
        assert!(nights.windows(2).any(|w| w[1] - w[0] > 1));
    }
}
