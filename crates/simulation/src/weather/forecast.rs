//! The forecast: the next weather kind and the game time until it is full at the player. A new feature.
//! The time of day and the biome give the kind weights.
use super::{StormRandom, WeatherKind};
use crate::lighting::CyclePhase;
use serde::{Deserialize, Serialize};

const KINDS: usize = WeatherKind::ALL.len();
/// Game seconds from one change to the next.
pub const WAIT: [f32; 2] = [600., 1500.];
/// Kind weights by the biome, in `WeatherKind` order. Rows: desert, snow, grass (`terrain::biome`).
const BIOME_WEIGHTS: [[f32; KINDS]; 3] = [
    [0.5, 0.4, 0.1, 0., 0.],
    [0.3, 0.35, 0.15, 0., 0.2],
    [0.35, 0.25, 0.18, 0.1, 0.12],
];
/// Kind factors by the time of day, in `WeatherKind` order. Rows: `DayPart` order.
const PART_WEIGHTS: [[f32; KINDS]; 5] = [
    [0.6, 0.8, 0.5, 0.2, 5.],
    [1.5, 1.2, 0.7, 0.4, 0.5],
    [1.2, 1.2, 0.8, 2.5, 0.2],
    [0.7, 1.6, 1.6, 0.8, 0.6],
    [2., 1., 0.7, 0.4, 0.8],
];

/// The time of day for the weights. The day has two halves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DayPart {
    Dawn,
    #[default]
    Morning,
    Afternoon,
    Dusk,
    Night,
}

impl DayPart {
    /// The part of a cycle phase. `progress` is the phase progress, 0 to 1.
    pub fn new(phase: CyclePhase, progress: f32) -> Self {
        match phase {
            CyclePhase::Dawn => Self::Dawn,
            CyclePhase::Day if progress < 0.5 => Self::Morning,
            CyclePhase::Day => Self::Afternoon,
            CyclePhase::Dusk => Self::Dusk,
            CyclePhase::Night => Self::Night,
        }
    }
}

/// The kind weights at a time of day over a biome.
pub fn weights(part: DayPart, biome: u8) -> [f32; KINDS] {
    let biome = &BIOME_WEIGHTS[usize::from(biome.min(2))];
    let part = &PART_WEIGHTS[part as usize];
    std::array::from_fn(|i| biome[i] * part[i])
}

/// A weighted kind. `not` has no weight, unless no other kind has weight.
pub(super) fn pick(
    random: &mut StormRandom,
    weights: &[f32; KINDS],
    not: Option<WeatherKind>,
) -> WeatherKind {
    let mut weights = *weights;
    if let Some(not) = not
        && weights
            .iter()
            .enumerate()
            .any(|(i, &w)| w > 0. && i != not as usize)
    {
        weights[not as usize] = 0.;
    }
    let mut draw = random.unit() * weights.iter().sum::<f32>();
    for (kind, &w) in WeatherKind::ALL.iter().zip(&weights) {
        if draw < w {
            return *kind;
        }
        draw -= w;
    }
    WeatherKind::ALL[weights
        .iter()
        .rposition(|&w| w > 0.)
        .expect("one kind has weight")]
}

#[derive(Debug)]
pub struct Forecast {
    /// The kind of the last change.
    pub current: WeatherKind,
    pub next: WeatherKind,
    /// Game seconds until `next` is full at the player.
    pub wait: f32,
    random: StormRandom,
    /// The last time of day and the biome at the last change. `resume` picks with them.
    part: DayPart,
    biome: u8,
}

/// The saved forecast. A guest gets `next` and `wait` from the host.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForecastSnapshot {
    pub current: WeatherKind,
    pub next: WeatherKind,
    pub wait: f32,
    pub random: u64,
}

impl Forecast {
    pub fn new(seed: u64) -> Self {
        let mut forecast = Self {
            current: WeatherKind::Clear,
            next: WeatherKind::Clear,
            wait: 0.,
            random: StormRandom(seed ^ 0x666f_7265_6361_7374),
            part: DayPart::default(),
            biome: 2,
        };
        forecast.resume(WeatherKind::Clear);
        forecast
    }

    /// Counts down `dt` game seconds. At 0 it returns the kind that is now full and picks the next.
    /// One call gives one change at most. It calls `biome` only at a change.
    pub fn advance(
        &mut self,
        dt: f32,
        part: DayPart,
        biome: impl FnOnce() -> u8,
    ) -> Option<WeatherKind> {
        self.part = part;
        self.wait -= dt;
        if self.wait > 0. {
            return None;
        }
        self.biome = biome();
        let arrived = self.next;
        self.resume(arrived);
        Some(arrived)
    }

    /// Starts again from `current` with a new next kind and a new wait.
    pub fn resume(&mut self, current: WeatherKind) {
        self.current = current;
        let weights = weights(self.part, self.biome);
        self.next = pick(&mut self.random, &weights, Some(current));
        self.wait = self.random.range(WAIT[0], WAIT[1]);
    }

    pub fn snapshot(&self) -> ForecastSnapshot {
        ForecastSnapshot {
            current: self.current,
            next: self.next,
            wait: self.wait,
            random: self.random.0,
        }
    }

    pub fn restore(&mut self, saved: &ForecastSnapshot) {
        self.current = saved.current;
        self.next = saved.next;
        self.wait = saved.wait;
        self.random.0 = saved.random;
    }

    /// A saved or host wait in range.
    pub fn valid_wait(wait: f32) -> bool {
        wait.is_finite() && (0. ..=WAIT[1]).contains(&wait)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_forecast_picks_the_same_kinds() {
        let mut forecast = Forecast::new(4);
        let mut copy = Forecast::new(5);
        copy.restore(&forecast.snapshot());
        for _ in 0..20 {
            forecast.advance(2000., DayPart::Dusk, || 1);
            copy.advance(2000., DayPart::Dusk, || 1);
            assert_eq!(forecast.snapshot(), copy.snapshot());
        }
    }

    #[test]
    fn dawn_gives_more_fog_and_the_desert_none() {
        let mut random = StormRandom(9);
        let mut count = |part, biome, kind| {
            let weights = weights(part, biome);
            (0..1000)
                .filter(|_| pick(&mut random, &weights, None) == kind)
                .count()
        };
        assert!(
            count(DayPart::Dawn, 2, WeatherKind::Fog)
                > 2 * count(DayPart::Afternoon, 2, WeatherKind::Fog)
        );
        for part in [DayPart::Dawn, DayPart::Afternoon, DayPart::Night] {
            assert_eq!(count(part, 0, WeatherKind::Fog), 0);
            assert_eq!(count(part, 0, WeatherKind::Thunderstorm), 0);
        }
    }
}
