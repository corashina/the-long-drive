//! Weather kinds and the mix. A new feature: the source has only the sandstorm.
use serde::{Deserialize, Serialize};

/// Wetness per second at full rain.
pub(super) const WET_RATE: f32 = 1. / 60.;
/// Wetness per second without rain.
pub(super) const DRY_RATE: f32 = 1. / 300.;
/// Temperature offset in °C at full wetness.
pub const WET_TEMPERATURE: f32 = -2.;

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
    /// The look of this kind at full weight.
    pub fn look(self) -> WeatherLook {
        LOOKS[self as usize]
    }
}

/// The weather that presentation, audio and physics read. All values are 0 to 1, but
/// `temperature` is an offset in °C.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherMix {
    pub rain: f32,
    pub wetness: f32,
    pub flash: f32,
    pub temperature: f32,
}

const fn target(rain: f32, temperature: f32) -> WeatherMix {
    WeatherMix {
        rain,
        wetness: 0.,
        flash: 0.,
        temperature,
    }
}

/// One row per kind, in `WeatherKind` order. Columns: rain, temperature offset in °C.
const TARGETS: [WeatherMix; 5] = [
    target(0., 0.),
    target(0., -2.),
    target(0.7, -5.),
    target(1., -6.),
    target(0., -2.),
];

/// What a kind looks like. The rows multiply the source light and color, so night stays night.
/// Factors are 1 for no change.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherLook {
    /// Sun light factor.
    pub sun: f32,
    /// Ambient light factor.
    pub ambient: f32,
    /// Sky share that shows the fog color. A low share keeps blue sky between the clouds.
    pub sky: f32,
    /// Cloud cover, 0 to 1. 1 is a full sheet.
    pub cloud_cover: f32,
    /// Cloud grey share, 0 to 1.
    pub cloud_grey: f32,
    /// Cloud brightness factor of the grey.
    pub cloud_shade: f32,
    /// Cloud alpha removed, 0 to 1. Fog hides the clouds.
    pub cloud_hide: f32,
    /// Exp2 fog density from the visibility. 0 keeps the source fog.
    pub fog_density: f32,
    /// Fog color grey share, 0 to 1. The sky overcast also shows the fog color.
    pub fog_grey: f32,
    /// RGB factor of the grey fog color.
    pub fog_shade: [f32; 3],
    /// Grade saturation factor.
    pub saturation: f32,
    /// Grade contrast factor.
    pub contrast: f32,
    /// Grade warmth, -1 (cool) to 1 (warm).
    pub warmth: f32,
    /// Sun flare factor.
    pub flare: f32,
}

/// Exp2 fog density that leaves 5 % contrast at `meters`: exp(-(1.73)^2) = 0.05.
pub const fn visibility_density(meters: f32) -> f32 {
    1.73 / meters
}

/// One row per kind, in `WeatherKind` order. Tune the look here.
pub const LOOKS: [WeatherLook; 5] = [
    // Clear: deep blue sky, few white clouds, full sun and flare.
    WeatherLook {
        sun: 1.,
        ambient: 1.,
        sky: 0.,
        cloud_cover: 0.,
        cloud_grey: 0.,
        cloud_shade: 1.,
        cloud_hide: 0.,
        fog_density: 0.,
        fog_grey: 0.,
        fog_shade: [1.; 3],
        saturation: 1.15,
        contrast: 1.05,
        warmth: 0.,
        flare: 1.,
    },
    // Cloudy: big white clouds with blue gaps.
    WeatherLook {
        sun: 0.7,
        ambient: 0.9,
        sky: 0.15,
        cloud_cover: 0.6,
        cloud_grey: 0.5,
        cloud_shade: 0.95,
        cloud_hide: 0.,
        fog_density: 0.,
        fog_grey: 0.,
        fog_shade: [1.; 3],
        saturation: 1.,
        contrast: 1.,
        warmth: 0.,
        flare: 0.4,
    },
    // Rain: flat mid grey sky, full grey cloud sheet, 1500 m.
    WeatherLook {
        sun: 0.3,
        ambient: 0.65,
        sky: 1.,
        cloud_cover: 1.,
        cloud_grey: 1.,
        cloud_shade: 0.6,
        cloud_hide: 0.,
        fog_density: visibility_density(1500.),
        fog_grey: 1.,
        fog_shade: [0.55, 0.58, 0.63],
        saturation: 0.7,
        contrast: 0.9,
        warmth: -0.15,
        flare: 0.,
    },
    // Thunderstorm: very dark slate blue-grey sky and clouds, 600 m.
    WeatherLook {
        sun: 0.15,
        ambient: 0.45,
        sky: 1.,
        cloud_cover: 1.,
        cloud_grey: 1.,
        cloud_shade: 0.3,
        cloud_hide: 0.,
        fog_density: visibility_density(600.),
        fog_grey: 1.,
        fog_shade: [0.22, 0.27, 0.3],
        saturation: 0.55,
        contrast: 1.,
        warmth: -0.35,
        flare: 0.,
    },
    // Fog: bright milky white, clouds hidden, 120 m.
    WeatherLook {
        sun: 0.35,
        ambient: 0.85,
        sky: 0.95,
        cloud_cover: 0.,
        cloud_grey: 1.,
        cloud_shade: 1.2,
        cloud_hide: 0.9,
        fog_density: visibility_density(120.),
        fog_grey: 1.,
        fog_shade: [1.25; 3],
        saturation: 0.6,
        contrast: 0.75,
        warmth: 0.,
        flare: 0.,
    },
];
