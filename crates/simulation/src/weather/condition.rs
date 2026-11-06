//! Weather kinds and the mix. A new feature: the source has only the sandstorm.
use serde::{Deserialize, Serialize};

/// Wetness per second at full rain.
pub(super) const WET_RATE: f32 = 1. / 60.;
/// Wetness per second without rain.
pub(super) const DRY_RATE: f32 = 1. / 300.;

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
    /// The full mix of this kind. Wetness and flash are 0.
    pub fn target(self) -> WeatherMix {
        TARGETS[self as usize]
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

/// One row per kind, in `WeatherKind` order.
const TARGETS: [WeatherMix; 5] = [
    target(0., 0., 0.),
    target(0.7, 0., 0.1),
    target(0.9, 0.7, 0.3),
    target(1., 1., 0.4),
    target(0.5, 0., 1.),
];
