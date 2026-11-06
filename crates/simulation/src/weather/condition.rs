//! Weather kinds and the mix. A new feature: the source has only the sandstorm.
use serde::{Deserialize, Serialize};

/// Wetness per second at full rain.
pub(super) const WET_RATE: f32 = 1. / 60.;
/// Wetness per second without rain.
pub(super) const DRY_RATE: f32 = 1. / 300.;
/// Temperature offset in °C at full wetness.
pub const WET_TEMPERATURE: f32 = -2.;
/// Snow cover per second at full snow: 1 in 10 minutes.
pub(super) const SNOW_RATE: f32 = 1. / 600.;
/// Snow cover per second above `MELT_TEMPERATURE`: 1 in 15 minutes.
pub(super) const MELT_RATE: f32 = 1. / 900.;
/// Snow cover melts above this ambient temperature in °C.
pub(super) const MELT_TEMPERATURE: f32 = 2.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeatherKind {
    #[default]
    Clear = 0,
    Cloudy,
    Rain,
    Thunderstorm,
    Fog,
    Snow,
}

impl WeatherKind {
    pub const ALL: [Self; 6] = [
        Self::Clear,
        Self::Cloudy,
        Self::Rain,
        Self::Thunderstorm,
        Self::Fog,
        Self::Snow,
    ];
    /// The sandstorm spawns only in Clear and Cloudy.
    pub fn holds_sandstorm(self) -> bool {
        matches!(
            self,
            Self::Rain | Self::Thunderstorm | Self::Fog | Self::Snow
        )
    }
    /// The full mix of this kind. Wetness, snow cover and flash are 0.
    pub fn target(self) -> WeatherMix {
        TARGETS[self as usize]
    }
}

/// The weather that presentation, audio and physics read. All values are 0 to 1, but
/// `temperature` is an offset in °C.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherMix {
    pub cloud_cover: f32,
    pub rain: f32,
    pub snow: f32,
    pub fog: f32,
    pub wetness: f32,
    pub snow_cover: f32,
    pub flash: f32,
    pub temperature: f32,
}

const fn target(cloud_cover: f32, rain: f32, snow: f32, fog: f32, temperature: f32) -> WeatherMix {
    WeatherMix {
        cloud_cover,
        rain,
        snow,
        fog,
        wetness: 0.,
        snow_cover: 0.,
        flash: 0.,
        temperature,
    }
}

/// One row per kind, in `WeatherKind` order. The temperature is an offset in °C.
/// Columns: cloud cover, rain, snow, fog, temperature.
const TARGETS: [WeatherMix; 6] = [
    target(0., 0., 0., 0., 0.),
    target(0.7, 0., 0., 0.1, -2.),
    target(0.9, 0.7, 0., 0.3, -5.),
    target(1., 1., 0., 0.4, -6.),
    target(0.5, 0., 0., 1., -2.),
    target(0.9, 0., 0.8, 0.3, -12.),
];
