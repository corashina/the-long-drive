//! Post stack v2 effects that the binocular and `DeadV` volumes blend over the main camera stack.
use super::postprocessing::{BloomSettings, DeadProfile, PostprocessingRecord, VolumeParameter};

/// Bloom values that a volume can change without a new pyramid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomBlend {
    pub intensity: f32,
    pub threshold: f32,
    pub soft_knee: f32,
    /// Gamma color.
    pub color: [f32; 4],
    pub dirt_intensity: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grain {
    pub enabled: bool,
    pub intensity: f32,
    pub size: f32,
    pub lum_contrib: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chromatic {
    pub enabled: bool,
    pub intensity: f32,
    pub fast: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vignette {
    pub enabled: bool,
    /// Masked mode with the binocular mask. False is the classic mode.
    pub masked: bool,
    /// Gamma color.
    pub color: [f32; 4],
    pub center: [f32; 2],
    pub intensity: f32,
    pub smoothness: f32,
    pub roundness: f32,
    pub rounded: bool,
    pub opacity: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distortion {
    pub enabled: bool,
    pub intensity: f32,
    pub intensity_x: f32,
    pub intensity_y: f32,
    pub center_x: f32,
    pub center_y: f32,
    pub scale: f32,
}

/// The blended settings of one frame. The `on` methods are the source `IsEnabledAndSupported`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackEffects {
    pub bloom: BloomBlend,
    pub grain: Grain,
    pub chromatic: Chromatic,
    pub vignette: Vignette,
    pub distortion: Distortion,
}

impl Grain {
    pub fn on(&self) -> bool {
        self.enabled && self.intensity > 0.
    }
}
impl Chromatic {
    pub fn on(&self) -> bool {
        self.enabled && self.intensity > 0.
    }
}
impl Vignette {
    pub fn on(&self) -> bool {
        self.enabled
            && if self.masked {
                self.opacity > 0.
            } else {
                self.intensity > 0.
            }
    }
}
impl Distortion {
    pub fn on(&self) -> bool {
        // Mathf.Approximately(intensity, 0) is false for every value that the stack blends.
        self.enabled && self.intensity != 0. && (self.intensity_x > 0. || self.intensity_y > 0.)
    }
}

impl StackEffects {
    /// The main stack, with the binocular volume at weight 1 when `binoculars`.
    pub fn main(record: &PostprocessingRecord, binoculars: bool) -> Self {
        let b = &record.bloom;
        let mut effects = Self::with_bloom(BloomBlend {
            intensity: b.intensity,
            threshold: b.threshold,
            soft_knee: b.soft_knee,
            color: b.color,
            dirt_intensity: b.dirt_intensity,
        });
        if binoculars {
            let p = &record.binoculars;
            effects.vignette.enabled = true;
            effects.vignette.masked = true;
            effects.vignette.color = p.color;
            effects.vignette.opacity = p.opacity;
            let d = p.distortion;
            effects.distortion = Distortion {
                enabled: true,
                intensity: d.intensity,
                intensity_x: d.intensity_xy[0],
                intensity_y: d.intensity_xy[1],
                center_x: d.center[0],
                center_y: d.center[1],
                scale: d.scale,
            };
            effects.chromatic = Chromatic {
                enabled: true,
                intensity: p.chromatic_intensity,
                fast: true,
            };
        }
        effects
    }
    /// `bloom` and the source defaults of the effects that the main stack does not have.
    fn with_bloom(bloom: BloomBlend) -> Self {
        Self {
            bloom,
            grain: Grain {
                enabled: false,
                intensity: 0.,
                size: 1.,
                lum_contrib: 0.8,
            },
            chromatic: Chromatic {
                enabled: false,
                intensity: 0.,
                fast: false,
            },
            vignette: Vignette {
                enabled: false,
                masked: false,
                color: [0., 0., 0., 1.],
                center: [0.5, 0.5],
                intensity: 0.,
                smoothness: 0.2,
                roundness: 1.,
                rounded: false,
                opacity: 1.,
            },
            distortion: Distortion {
                enabled: false,
                intensity: 0.,
                intensity_x: 1.,
                intensity_y: 1.,
                center_x: 0.,
                center_y: 0.,
                scale: 1.,
            },
        }
    }
    fn slot(&mut self, field: Field) -> Slot<'_> {
        use Field::*;
        let (b, g, c, v, d) = (
            &mut self.bloom,
            &mut self.grain,
            &mut self.chromatic,
            &mut self.vignette,
            &mut self.distortion,
        );
        match field {
            BloomIntensity => Slot::Number(&mut b.intensity),
            BloomThreshold => Slot::Number(&mut b.threshold),
            BloomSoftKnee => Slot::Number(&mut b.soft_knee),
            BloomColor => Slot::Vector(&mut b.color),
            BloomDirtIntensity => Slot::Number(&mut b.dirt_intensity),
            GrainEnabled => Slot::Switch(&mut g.enabled),
            GrainIntensity => Slot::Number(&mut g.intensity),
            GrainSize => Slot::Number(&mut g.size),
            GrainLumContrib => Slot::Number(&mut g.lum_contrib),
            ChromaticEnabled => Slot::Switch(&mut c.enabled),
            ChromaticIntensity => Slot::Number(&mut c.intensity),
            ChromaticFast => Slot::Switch(&mut c.fast),
            VignetteEnabled => Slot::Switch(&mut v.enabled),
            VignetteMasked => Slot::Switch(&mut v.masked),
            VignetteColor => Slot::Vector(&mut v.color),
            VignetteCenter => Slot::Vector(&mut v.center),
            VignetteIntensity => Slot::Number(&mut v.intensity),
            VignetteSmoothness => Slot::Number(&mut v.smoothness),
            VignetteRoundness => Slot::Number(&mut v.roundness),
            VignetteRounded => Slot::Switch(&mut v.rounded),
            DistortionEnabled => Slot::Switch(&mut d.enabled),
            DistortionIntensity => Slot::Number(&mut d.intensity),
            DistortionX => Slot::Number(&mut d.intensity_x),
            DistortionY => Slot::Number(&mut d.intensity_y),
            DistortionCenterX => Slot::Number(&mut d.center_x),
            DistortionCenterY => Slot::Number(&mut d.center_y),
            DistortionScale => Slot::Number(&mut d.scale),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Field {
    BloomIntensity,
    BloomThreshold,
    BloomSoftKnee,
    BloomColor,
    BloomDirtIntensity,
    GrainEnabled,
    GrainIntensity,
    GrainSize,
    GrainLumContrib,
    ChromaticEnabled,
    ChromaticIntensity,
    ChromaticFast,
    VignetteEnabled,
    VignetteMasked,
    VignetteColor,
    VignetteCenter,
    VignetteIntensity,
    VignetteSmoothness,
    VignetteRoundness,
    VignetteRounded,
    DistortionEnabled,
    DistortionIntensity,
    DistortionX,
    DistortionY,
    DistortionCenterX,
    DistortionCenterY,
    DistortionScale,
}

/// Source effect kind and field name of each blended parameter.
const FIELDS: [(&str, &str, Field); 27] = [
    ("bloom", "intensity", Field::BloomIntensity),
    ("bloom", "threshold", Field::BloomThreshold),
    ("bloom", "softKnee", Field::BloomSoftKnee),
    ("bloom", "color", Field::BloomColor),
    ("bloom", "dirtIntensity", Field::BloomDirtIntensity),
    ("grain", "enabled", Field::GrainEnabled),
    ("grain", "intensity", Field::GrainIntensity),
    ("grain", "size", Field::GrainSize),
    ("grain", "lumContrib", Field::GrainLumContrib),
    ("chromatic_aberration", "enabled", Field::ChromaticEnabled),
    (
        "chromatic_aberration",
        "intensity",
        Field::ChromaticIntensity,
    ),
    ("chromatic_aberration", "fastMode", Field::ChromaticFast),
    ("vignette", "enabled", Field::VignetteEnabled),
    ("vignette", "mode", Field::VignetteMasked),
    ("vignette", "color", Field::VignetteColor),
    ("vignette", "center", Field::VignetteCenter),
    ("vignette", "intensity", Field::VignetteIntensity),
    ("vignette", "smoothness", Field::VignetteSmoothness),
    ("vignette", "roundness", Field::VignetteRoundness),
    ("vignette", "rounded", Field::VignetteRounded),
    ("lens_distortion", "enabled", Field::DistortionEnabled),
    ("lens_distortion", "intensity", Field::DistortionIntensity),
    ("lens_distortion", "intensityX", Field::DistortionX),
    ("lens_distortion", "intensityY", Field::DistortionY),
    ("lens_distortion", "centerX", Field::DistortionCenterX),
    ("lens_distortion", "centerY", Field::DistortionCenterY),
    ("lens_distortion", "scale", Field::DistortionScale),
];

enum Slot<'a> {
    Switch(&'a mut bool),
    Number(&'a mut f32),
    Vector(&'a mut [f32]),
}

#[derive(Clone, Copy, Debug)]
enum Value {
    Switch(bool),
    Number(f32),
    /// The first `len` values.
    Vector([f32; 4], usize),
}

/// The `DeadV` overrides. Source PostProcessManager blends a volume only at weight above 0.
/// Then each overridden number is `Lerp(from, to, weight)`, and each bool or mode is `to`.
#[derive(Clone, Debug, Default)]
pub struct DeadEffects {
    overrides: Vec<(Field, Value)>,
}

impl DeadEffects {
    /// The overrides of the active `DeadV` effects, and the ignored ones as `effect.parameter`.
    pub fn new(profile: &DeadProfile, record: &PostprocessingRecord) -> (Self, Vec<String>) {
        let mut overrides = Vec::new();
        let mut ignored = Vec::new();
        for effect in profile.effects.iter().filter(|e| e.active) {
            for (name, parameter) in &effect.parameters {
                if !parameter.override_state() {
                    continue;
                }
                match parse(&effect.kind, name, parameter, &record.bloom) {
                    Ok(Some(o)) => overrides.push(o),
                    Ok(None) => {}
                    Err(()) => ignored.push(format!("{}.{name}", effect.kind)),
                }
            }
        }
        (Self { overrides }, ignored)
    }
    /// Blends the overrides over `effects` with `weight` from 0 to 1.
    pub fn blend(&self, effects: &mut StackEffects, weight: f32) {
        if weight <= 0. {
            return;
        }
        let lerp = |from: &mut f32, to: f32| *from += (to - *from) * weight;
        for (field, value) in &self.overrides {
            match (effects.slot(*field), value) {
                (Slot::Switch(s), Value::Switch(v)) => *s = *v,
                (Slot::Number(s), Value::Number(v)) => lerp(s, *v),
                (Slot::Vector(s), Value::Vector(v, len)) => {
                    for (from, to) in s.iter_mut().zip(&v[..*len]) {
                        lerp(from, *to);
                    }
                }
                _ => unreachable!("DeadEffects::new matches each value to its slot"),
            }
        }
    }
}

/// One overridden parameter: an override, None at the main value, or an error if the port does not blend it.
/// Bloom parameters that need a new pyramid are accepted only at the main value.
fn parse(
    kind: &str,
    name: &str,
    parameter: &VolumeParameter,
    bloom: &BloomSettings,
) -> Result<Option<(Field, Value)>, ()> {
    let unchanged = match (kind, name, parameter) {
        ("bloom", "enabled", VolumeParameter::Bool(p)) => p.value,
        ("bloom", "fastMode", VolumeParameter::Bool(p)) => p.value == bloom.fast,
        ("bloom", "fastMode", VolumeParameter::Number(p)) => {
            p.value == f32::from(u8::from(bloom.fast))
        }
        ("bloom", "clamp", VolumeParameter::Number(p)) => p.value == bloom.clamp,
        ("bloom", "diffusion", VolumeParameter::Number(p)) => p.value == bloom.diffusion,
        ("bloom", "anamorphicRatio", VolumeParameter::Number(p)) => {
            p.value == bloom.anamorphic_ratio
        }
        ("bloom", "dirtTexture", VolumeParameter::Texture(p)) => {
            p.value.as_ref() == Some(&bloom.dirt)
        }
        _ => false,
    };
    if unchanged {
        return Ok(None);
    }
    let field = FIELDS
        .iter()
        .find(|(k, n, _)| *k == kind && *n == name)
        .map(|(_, _, f)| *f)
        .ok_or(())?;
    let mut probe = StackEffects::with_bloom(BloomBlend {
        intensity: 0.,
        threshold: 0.,
        soft_knee: 0.,
        color: [0.; 4],
        dirt_intensity: 0.,
    });
    let value = match (probe.slot(field), parameter) {
        (Slot::Switch(_), VolumeParameter::Bool(p)) => Value::Switch(p.value),
        // Bools and two-value enums can serialize as 0 or 1.
        (Slot::Switch(_), VolumeParameter::Number(p)) if p.value == 0. || p.value == 1. => {
            Value::Switch(p.value == 1.)
        }
        (Slot::Number(_), VolumeParameter::Number(p)) => Value::Number(p.value),
        (Slot::Vector(s), VolumeParameter::Vector(p)) if s.len() == p.value.len() => {
            let mut v = [0.; 4];
            v[..s.len()].copy_from_slice(&p.value);
            Value::Vector(v, s.len())
        }
        _ => return Err(()),
    };
    Ok(Some((field, value)))
}

#[cfg(test)]
mod tests {
    use super::super::postprocessing::SourceParameter;
    use super::*;

    #[test]
    fn dead_volume_blends_numbers_and_switches_modes() {
        let number = |value| {
            VolumeParameter::Number(SourceParameter {
                override_state: true,
                value,
            })
        };
        let bloom = BloomSettings {
            intensity: 1.,
            threshold: 1.2,
            soft_knee: 0.5,
            clamp: 65472.,
            diffusion: 7.,
            anamorphic_ratio: 0.,
            color: [1.; 4],
            fast: true,
            dirt: super::super::SourceId {
                file: "sharedassets0.assets".into(),
                path_id: 265,
            },
            dirt_intensity: 2.,
        };
        let parse = |kind, name, value| parse(kind, name, &number(value), &bloom);
        assert_eq!(parse("bloom", "diffusion", 7.).unwrap().map(|_| ()), None);
        assert!(parse("bloom", "diffusion", 6.).is_err());
        assert!(parse("vignette", "mode", 2.).is_err());
        let dead = DeadEffects {
            overrides: [
                ("vignette", "intensity", 1.),
                ("vignette", "mode", 0.),
                ("chromatic_aberration", "fastMode", 0.),
                ("bloom", "intensity", 20.),
            ]
            .map(|(k, n, v)| parse(k, n, v).unwrap().unwrap())
            .to_vec(),
        };
        let mut e = StackEffects::with_bloom(BloomBlend {
            intensity: 1.,
            threshold: 1.2,
            soft_knee: 0.5,
            color: [1.; 4],
            dirt_intensity: 2.,
        });
        e.vignette.masked = true;
        e.chromatic.fast = true;
        let base = e;
        dead.blend(&mut e, 0.);
        assert_eq!(e, base);
        dead.blend(&mut e, 0.25);
        assert_eq!(e.vignette.intensity, 0.25);
        assert!(!e.vignette.masked && !e.chromatic.fast);
        assert_eq!(e.bloom.intensity, 1. + 19. * 0.25);
    }
}
