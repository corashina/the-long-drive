//! Weather types. A new feature: the source has only the sandstorm.
//! The cycle holds one kind for a time, then blends to the next kind.
use super::StormRandom;
use crate::save::WeatherCycleSnapshot;
use serde::{Deserialize, Serialize};

/// Game seconds that one kind holds.
pub const HOLD_SECONDS: [f32; 2] = [480., 1500.];
/// Game seconds of a blend to the next kind.
pub const BLEND_SECONDS: f32 = 90.;
/// Wetness per second at full rain.
const WET_RATE: f32 = 1. / 60.;
/// Wetness per second without rain.
const DRY_RATE: f32 = 1. / 300.;
/// Rain and thunderstorm weight drop at full desert weight.
const DESERT_RAIN_CUT: f32 = 0.8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeatherKind {
    #[default]
    Clear = 0,
    Cloudy,
    Rain,
    Thunderstorm,
    Fog,
}

impl WeatherKind {
    pub const ALL: [Self; 5] = [
        Self::Clear,
        Self::Cloudy,
        Self::Rain,
        Self::Thunderstorm,
        Self::Fog,
    ];
    /// The sandstorm spawns only in Clear and Cloudy.
    pub fn holds_sandstorm(self) -> bool {
        matches!(self, Self::Rain | Self::Thunderstorm | Self::Fog)
    }
    fn wet(self) -> bool {
        matches!(self, Self::Rain | Self::Thunderstorm)
    }
}

/// The weather that presentation, audio and physics read. All values are 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherMix {
    pub cloud_cover: f32,
    pub rain: f32,
    pub fog: f32,
    pub wetness: f32,
    pub flash: f32,
}

const fn target(cloud_cover: f32, rain: f32, fog: f32) -> WeatherMix {
    WeatherMix {
        cloud_cover,
        rain,
        fog,
        wetness: 0.,
        flash: 0.,
    }
}

/// One row per kind, in `WeatherKind` order. Wetness and flash come from the cycle.
const TARGETS: [WeatherMix; 5] = [
    target(0., 0., 0.),
    target(0.7, 0., 0.1),
    target(0.9, 0.7, 0.3),
    target(1., 1., 0.4),
    target(0.5, 0., 1.),
];
/// Pick weights of the next kind, in `WeatherKind` order.
const WEIGHTS: [f32; 5] = [0.35, 0.25, 0.18, 0.1, 0.12];

#[derive(Debug)]
pub struct WeatherCycle {
    pub current: WeatherKind,
    pub next: WeatherKind,
    /// 0 holds `current`. The blend to `next` starts when `remaining` reaches 0.
    pub blend: f32,
    /// Game seconds until the blend starts.
    pub remaining: f32,
    pub wetness: f32,
    /// The sandbox holds one kind. The cycle then does not change kind.
    pub locked: bool,
    random: StormRandom,
}

impl WeatherCycle {
    pub fn new(seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x7765_6174_6865_7201);
        let remaining = random.range(HOLD_SECONDS[0], HOLD_SECONDS[1]);
        let mut cycle = Self {
            current: WeatherKind::Clear,
            next: WeatherKind::Clear,
            blend: 0.,
            remaining,
            wetness: 0.,
            locked: false,
            random,
        };
        cycle.next = cycle.pick(0.);
        cycle
    }

    /// The kind that has most of the blend.
    pub fn kind(&self) -> WeatherKind {
        if self.blend < 0.5 {
            self.current
        } else {
            self.next
        }
    }

    /// `desert_weight` is 0 to 1: how much of the player biome is the sandstorm biome.
    /// A `guest` does not pick kinds. It holds a full blend until the host sends the next kind.
    pub fn advance(&mut self, dt: f32, desert_weight: f32, guest: bool) -> WeatherMix {
        if !self.locked {
            if self.remaining > 0. {
                self.remaining = (self.remaining - dt).max(0.);
            } else {
                self.blend = (self.blend + dt / BLEND_SECONDS).min(1.);
                if self.blend >= 1. && !guest {
                    self.current = self.next;
                    self.blend = 0.;
                    self.remaining = self.random.range(HOLD_SECONDS[0], HOLD_SECONDS[1]);
                    self.next = self.pick(desert_weight);
                }
            }
        }
        let mut mix = self.look();
        self.wetness = if mix.rain > 0. {
            (self.wetness + mix.rain * WET_RATE * dt).min(1.)
        } else {
            (self.wetness - DRY_RATE * dt).max(0.)
        };
        mix.wetness = self.wetness;
        mix
    }

    /// The blended targets with the current wetness.
    pub fn look(&self) -> WeatherMix {
        let [a, b] = [self.current, self.next].map(|k| TARGETS[k as usize]);
        let t = self.blend;
        WeatherMix {
            cloud_cover: a.cloud_cover + (b.cloud_cover - a.cloud_cover) * t,
            rain: a.rain + (b.rain - a.rain) * t,
            fog: a.fog + (b.fog - a.fog) * t,
            wetness: self.wetness,
            flash: 0.,
        }
    }

    /// `Some` holds the kind. `None` restarts the cycle from the current kind.
    pub fn lock(&mut self, kind: Option<WeatherKind>) {
        self.blend = 0.;
        match kind {
            Some(kind) => {
                self.current = kind;
                self.next = kind;
                self.locked = true;
            }
            None => {
                self.current = self.kind();
                self.locked = false;
                self.remaining = self.random.range(HOLD_SECONDS[0], HOLD_SECONDS[1]);
                self.next = self.pick(0.);
            }
        }
    }

    /// A weighted kind other than `current`. Desert cuts rain and thunderstorm.
    fn pick(&mut self, desert_weight: f32) -> WeatherKind {
        let rain_scale = 1. - DESERT_RAIN_CUT * desert_weight.clamp(0., 1.);
        let current = self.current;
        let weight = |kind: WeatherKind| {
            if kind == current {
                0.
            } else if kind.wet() {
                WEIGHTS[kind as usize] * rain_scale
            } else {
                WEIGHTS[kind as usize]
            }
        };
        let total: f32 = WeatherKind::ALL.into_iter().map(weight).sum();
        let mut draw = self.random.unit() * total;
        for kind in WeatherKind::ALL {
            let w = weight(kind);
            if draw < w {
                return kind;
            }
            draw -= w;
        }
        WeatherKind::ALL
            .into_iter()
            .rfind(|&k| weight(k) > 0.)
            .expect("one kind has weight")
    }

    pub fn snapshot(&self) -> WeatherCycleSnapshot {
        WeatherCycleSnapshot {
            current: self.current,
            next: self.next,
            blend: self.blend,
            remaining: self.remaining,
            wetness: self.wetness,
        }
    }

    /// The random stream keeps its state, as for the sandstorm. A load unlocks the cycle.
    pub fn restore(&mut self, saved: &WeatherCycleSnapshot) {
        self.current = saved.current;
        self.next = saved.next;
        self.blend = saved.blend;
        self.remaining = saved.remaining;
        self.wetness = saved.wetness;
        self.locked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_reaches_the_next_kind() {
        let mut cycle = WeatherCycle::new(3);
        let next = cycle.next;
        let mut seconds = 0.;
        while cycle.current != next {
            cycle.advance(1., 0., false);
            seconds += 1.;
            assert!(seconds <= HOLD_SECONDS[1] + BLEND_SECONDS + 1.);
        }
        assert_eq!(cycle.blend, 0.);
        assert_ne!(cycle.next, next);
        let mix = cycle.advance(0.001, 0., false);
        assert_eq!(mix.rain, TARGETS[next as usize].rain);
    }

    #[test]
    fn one_seed_gives_one_sequence() {
        let sequence = |seed| {
            let mut cycle = WeatherCycle::new(seed);
            let mut kinds = vec![cycle.current];
            while kinds.len() < 11 {
                cycle.advance(10., 0.3, false);
                if *kinds.last().unwrap() != cycle.current {
                    kinds.push(cycle.current);
                }
            }
            kinds
        };
        assert_eq!(sequence(42), sequence(42));
    }

    #[test]
    fn desert_draws_less_rain() {
        let wet = |desert| {
            let mut cycle = WeatherCycle::new(9);
            (0..1000)
                .filter(|_| {
                    cycle.current = WeatherKind::Fog;
                    cycle.pick(desert).wet()
                })
                .count()
        };
        assert!(wet(1.) * 2 < wet(0.));
    }
}
