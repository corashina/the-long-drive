//! Validated source effects, camera order, and exact histogram bounds.
#[derive(Clone, Debug)]
pub struct HistogramLayout {
    pub source: [u32; 2],
    pub samples: [u32; 2],
    pub sample_count: u32,
    pub tiles: u32,
    pub columns: u32,
    pub scatter: [u32; 2],
    pub reduction_rows: Vec<u32>,
    pub total_bound: u32,
}
impl HistogramLayout {
    pub fn new(width: u32, height: u32, max_dimension: u32) -> Result<Self, String> {
        if width == 0
            || height == 0
            || width > max_dimension
            || height > max_dimension
            || max_dimension < 32
        {
            return Err("Invalid source histogram extent".into());
        }
        let samples = [width.div_ceil(2), height.div_ceil(2)];
        let count = u64::from(samples[0]) * u64::from(samples[1]);
        let bound = (64 * u64::from(samples[0]) - 8 * u64::from(width.div_ceil(8)))
            .checked_mul(u64::from(samples[1]))
            .ok_or("Source histogram exceeds uint32 transport")?;
        if bound > u64::from(u32::MAX) || count * 6 > u64::from(u32::MAX) {
            return Err("Source histogram exceeds uint32 transport".into());
        }
        let tiles = (count as u32).div_ceil(32);
        let mut columns = 1u32;
        while u64::from(columns) * u64::from(columns) * 32 < u64::from(tiles) {
            columns += 1;
        }
        columns = columns.min(max_dimension / 32);
        let scatter = [columns * 32, tiles.div_ceil(columns)];
        if scatter[1] > max_dimension {
            return Err("Source histogram scatter exceeds device extent".into());
        }
        let mut reduction_rows = Vec::new();
        let mut rows = tiles;
        loop {
            rows = rows.div_ceil(256);
            if rows > max_dimension {
                return Err("Source histogram reduction exceeds device extent".into());
            }
            reduction_rows.push(rows);
            if rows == 1 {
                break;
            }
        }
        Ok(Self {
            source: [width, height],
            samples,
            sample_count: count as u32,
            tiles,
            columns,
            scatter,
            reduction_rows,
            total_bound: bound as u32,
        })
    }
    pub fn logical_bytes(&self) -> u64 {
        u64::from(self.scatter[0]) * u64::from(self.scatter[1]) * 8
            + self
                .reduction_rows
                .iter()
                .map(|rows| 32 * u64::from(*rows) * 16)
                .sum::<u64>()
    }
}

/// Active source exposure settings. Percent filtering is converted only at GPU binding.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureSettings {
    pub filtering_percent: [f32; 2],
    pub luminance_ev: [f32; 2],
    pub key: f32,
    pub speed_up: f32,
    pub speed_down: f32,
}
impl ExposureSettings {
    pub fn validate(&self) -> bool {
        self.filtering_percent
            .iter()
            .chain(self.luminance_ev.iter())
            .all(|v| v.is_finite())
            && self.filtering_percent[0] >= 1.
            && self.filtering_percent[1] <= 99.
            && self.filtering_percent[1] - self.filtering_percent[0] >= 0.01
            && self.luminance_ev[0] <= self.luminance_ev[1]
            && self
                .luminance_ev
                .iter()
                .all(|v| v.exp2().is_finite() && v.exp2() > 0.)
            && self.key.is_finite()
            && self.key > 0.
            && self.speed_up.is_finite()
            && self.speed_up >= 0.
            && self.speed_down.is_finite()
            && self.speed_down >= 0.
    }
}

/// Main-camera effects after source startup controls and volume blending.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostprocessingRecord {
    pub main_layer: super::SourceId,
    pub stop_nan: bool,
    pub taa: TaaSettings,
    pub motion_blur: MotionBlurSettings,
    pub exposure: ExposureSettings,
    pub bloom: BloomSettings,
    pub occlusion: OcclusionSettings,
    pub grading: GradingSettings,
    pub blue_noise: Vec<super::SourceId>,
    pub binoculars: BinocularProfile,
    pub depth_of_field: FocusControl,
    /// `postprocesscontrol` day and night saturation.
    pub saturation: GradingSaturation,
    /// `postprocesscontrol` LDR LUT strips.
    pub luts: LdrLuts,
    pub heat_haze: HeatHaze,
}
/// `postprocesscontrol.Update` sets the main grading saturation by the time of day.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradingSaturation {
    pub day: f32,
    pub night: f32,
}
impl GradingSaturation {
    /// `postprocesscontrol.Update`: night (`este`), Lerp toward night at dusk and toward day at dawn.
    pub fn at(&self, phase: crate::lighting::CyclePhase, progress: f32) -> f32 {
        use crate::lighting::CyclePhase;
        let lerp = |a: f32, b: f32| a + (b - a) * progress.clamp(0., 1.);
        match phase {
            CyclePhase::Night => self.night,
            CyclePhase::Dusk => lerp(self.day, self.night),
            CyclePhase::Dawn => lerp(self.night, self.day),
            CyclePhase::Day => self.day,
        }
    }
}
/// `LutC64` and `LutSepia`: linear strips with width = height * height.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LdrLuts {
    pub c64: super::SourceId,
    pub sepia: super::SourceId,
}
/// The main camera `Custom/heathazeshader` material.
/// `napszakvaltakozas` sets `_Weight` = InverseLerp(temperature[0], temperature[1], temp).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeatHaze {
    pub material: super::SourceId,
    pub noise: super::SourceId,
    pub temperature: [f32; 2],
    pub min_dist: f32,
    pub distance_m: f32,
    /// `_UpSpeed` and `_UpSpeed2`.
    pub up_speed: [f32; 2],
    pub up_strength: f32,
    /// Offset in xy, scale in zw.
    pub up_offset_scale: [f32; 4],
    /// `_DownSpeed` and `_DownSpeed2`.
    pub down_speed: [f32; 2],
    pub down_strength: f32,
    /// Offset in xy, scale in zw.
    pub down_offset_scale: [f32; 4],
}
/// The binocular volume profile `tavcso`. It overrides the `DOF` volume.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinocularProfile {
    pub profile: super::SourceId,
    /// Masked vignette color and opacity.
    pub color: [f32; 4],
    pub opacity: f32,
    /// `tavcsoscript.defVignette`. The shader reads its alpha.
    pub mask: super::SourceId,
    /// Focus distance comes from `tavcsoscript.focusDistance`.
    pub depth_of_field: Lens,
    pub distortion: LensDistortion,
    /// Fast mode with the internal red, green, blue spectrum.
    pub chromatic_intensity: f32,
}
/// DepthOfField lens with the Small kernel.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lens {
    pub aperture: f32,
    /// Millimeters.
    pub focal_length: f32,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensDistortion {
    pub intensity: f32,
    pub intensity_xy: [f32; 2],
    pub center: [f32; 2],
    pub scale: f32,
}
/// The `DOF` volume lens and `postprocesscontrol` focus ray.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusControl {
    pub lens: Lens,
    /// Profile focus before the first update.
    pub focus_distance: f32,
    pub layer_mask: u32,
    /// `minFocusDistance`, `maxFocusDistance`.
    pub focus: [f32; 2],
    pub night_focus: f32,
    pub focus_lerp: f32,
    /// `minFocusDistanceRay`, `maxFocusDistanceRay`.
    pub ray: [f32; 2],
}
impl Lens {
    fn validate(&self) -> bool {
        self.aperture.is_finite()
            && self.aperture > 0.
            && self.focal_length.is_finite()
            && self.focal_length > 0.
    }
}
/// `pplayermelle.TurnOn` temporal antialiasing settings.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaaSettings {
    pub jitter_spread: f32,
    pub stationary_blending: f32,
    pub motion_blending: f32,
    pub sharpness: f32,
}
impl TaaSettings {
    pub fn validate(&self) -> bool {
        [
            self.jitter_spread,
            self.stationary_blending,
            self.motion_blending,
        ]
        .iter()
        .all(|value| value.is_finite() && (0. ..=1.).contains(value))
            && self.sharpness.is_finite()
            && self.sharpness >= 0.
    }
}
/// Main `MotionBlur` effect and the `postprocesscontrol` shutter controller.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionBlurSettings {
    pub samples: u32,
    /// Degrees at the low and high speed.
    pub shutter_range: [f32; 2],
    /// km/h.
    pub speed_range: [f32; 2],
    pub shutter_lerp: f32,
}
impl MotionBlurSettings {
    pub fn validate(&self) -> bool {
        let [low, high] = self.shutter_range;
        self.samples > 0
            && [low, high, self.shutter_lerp]
                .iter()
                .chain(&self.speed_range)
                .all(|v| v.is_finite())
            && 0. <= low
            && low <= high
            && high <= 360.
            && self.speed_range[0] < self.speed_range[1]
            && self.shutter_lerp >= 0.
    }
    /// Source `postprocesscontrol.Update`: lerp from the current angle to the speed target.
    pub fn shutter(&self, current: f32, speed_kmh: f32, delta_seconds: f32) -> f32 {
        let [min_speed, max_speed] = self.speed_range;
        let t = ((speed_kmh - min_speed) / (max_speed - min_speed)).clamp(0., 1.);
        let target = self.shutter_range[0] + (self.shutter_range[1] - self.shutter_range[0]) * t;
        current + (target - current) * (self.shutter_lerp * delta_seconds).clamp(0., 1.)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BloomSettings {
    pub intensity: f32,
    pub threshold: f32,
    pub soft_knee: f32,
    pub clamp: f32,
    pub diffusion: f32,
    pub anamorphic_ratio: f32,
    pub color: [f32; 4],
    pub fast: bool,
    pub dirt: super::SourceId,
    pub dirt_intensity: f32,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcclusionSettings {
    pub intensity: f32,
    pub thickness: f32,
    pub noise_tolerance: f32,
    pub blur_tolerance: f32,
    pub upsample_tolerance: f32,
    pub direct_lighting_strength: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradingCurves {
    SourceDefault,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradingSettings {
    pub lut_size: u32,
    pub curves: GradingCurves,
    pub temperature: f32,
    pub tint: f32,
    pub hue_shift: f32,
    pub saturation: f32,
    pub post_exposure: f32,
    pub contrast: f32,
    pub color_filter: [f32; 4],
    pub channel_mixer: [[f32; 3]; 3],
    pub trackballs: [[f32; 4]; 3],
}
impl PostprocessingRecord {
    pub fn validate(
        &self,
        textures: &std::collections::HashSet<super::SourceId>,
    ) -> Result<(), super::ReferenceError> {
        use super::{finite, require};
        require(
            !self.main_layer.file.is_empty() && self.main_layer.path_id != 0,
            "postprocessing main layer",
        )?;
        require(self.taa.validate(), "postprocessing temporal antialiasing")?;
        require(self.motion_blur.validate(), "postprocessing motion blur")?;
        require(self.exposure.validate(), "postprocessing exposure")?;
        let bloom = &self.bloom;
        require(
            finite(&[
                bloom.intensity,
                bloom.threshold,
                bloom.soft_knee,
                bloom.clamp,
                bloom.diffusion,
                bloom.anamorphic_ratio,
                bloom.dirt_intensity,
            ]) && finite(&bloom.color)
                && bloom.color.iter().all(|v| *v >= 0.)
                && bloom.intensity >= 0.
                && bloom.threshold >= 0.
                && bloom.clamp > 0.
                && (0. ..=1.).contains(&bloom.soft_knee)
                && (1. ..=10.).contains(&bloom.diffusion)
                && bloom.anamorphic_ratio == 0.
                && bloom.fast
                && bloom.dirt_intensity >= 0.
                && textures.contains(&bloom.dirt),
            "postprocessing source bloom variant or dirt texture",
        )?;
        let ao = self.occlusion;
        require(
            finite(&[
                ao.intensity,
                ao.thickness,
                ao.noise_tolerance,
                ao.blur_tolerance,
                ao.upsample_tolerance,
                ao.direct_lighting_strength,
            ]) && ao.intensity >= 0.
                && ao.thickness > 0.
                && (0. ..=1.).contains(&ao.direct_lighting_strength),
            "postprocessing multiscale occlusion",
        )?;
        let grading = &self.grading;
        require(
            grading.lut_size == 33
                && finite(&[
                    grading.temperature,
                    grading.tint,
                    grading.hue_shift,
                    grading.saturation,
                    grading.post_exposure,
                    grading.contrast,
                ])
                && finite(&grading.color_filter)
                && grading.channel_mixer.iter().all(|values| finite(values))
                && grading.trackballs.iter().all(|values| finite(values)),
            "postprocessing Neutral grading",
        )?;
        require(
            self.blue_noise.len() == 64
                && self
                    .blue_noise
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == 64
                && self
                    .blue_noise
                    .iter()
                    .all(|source| textures.contains(source)),
            "postprocessing blue noise resources",
        )?;
        let vignette = &self.binoculars;
        require(
            finite(&vignette.color)
                && (0. ..=1.).contains(&vignette.opacity)
                && textures.contains(&vignette.mask),
            "postprocessing binocular vignette",
        )?;
        let d = vignette.distortion;
        require(
            vignette.depth_of_field.validate()
                && finite(&[d.intensity, d.scale, vignette.chromatic_intensity])
                && finite(&d.intensity_xy)
                && finite(&d.center)
                && d.scale > 0.
                && vignette.chromatic_intensity >= 0.,
            "postprocessing binocular lens",
        )?;
        let f = &self.depth_of_field;
        require(
            f.lens.validate()
                && finite(&[f.focus_distance, f.night_focus, f.focus_lerp])
                && finite(&f.focus)
                && finite(&f.ray)
                && f.focus_lerp >= 0.
                && f.ray[0] < f.ray[1]
                && f.layer_mask != 0,
            "postprocessing focus control",
        )?;
        require(
            finite(&[self.saturation.day, self.saturation.night])
                && textures.contains(&self.luts.c64)
                && textures.contains(&self.luts.sepia),
            "postprocessing LDR LUTs and saturation",
        )?;
        let haze = &self.heat_haze;
        require(
            textures.contains(&haze.noise)
                && haze.temperature[0] < haze.temperature[1]
                && finite(&haze.temperature)
                && finite(&[
                    haze.min_dist,
                    haze.distance_m,
                    haze.up_strength,
                    haze.down_strength,
                ])
                && finite(&haze.up_speed)
                && finite(&haze.down_speed)
                && finite(&haze.up_offset_scale)
                && finite(&haze.down_offset_scale),
            "postprocessing heat haze",
        )
    }
}

/// Source camera order. Probe captures have a separate scene path and no player effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraRole {
    Sky,
    Far,
    Main,
    Foreground,
}
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraClear {
    Skybox,
    Depth,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraEffectScope {
    MainStack,
    DitherOnly,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraEffects {
    pub source: super::SourceId,
    pub volume_mask: u32,
    pub scope: CameraEffectScope,
    pub stop_nan: bool,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraStage {
    pub role: CameraRole,
    pub source: super::SourceId,
    pub order: f32,
    pub clear: CameraClear,
    pub near: f32,
    pub far: f32,
    pub field_of_view: f32,
    pub mask: u32,
    pub hdr: bool,
    pub allow_msaa: bool,
    pub postprocessing: Option<CameraEffects>,
}
impl CameraStage {
    pub fn validate_stack(stages: &[Self]) -> bool {
        stages.len() == 4
            && stages.iter().map(|s| s.role).eq([
                CameraRole::Sky,
                CameraRole::Far,
                CameraRole::Main,
                CameraRole::Foreground,
            ])
            && stages.windows(2).all(|pair| pair[0].order < pair[1].order)
            && stages.iter().all(|stage| {
                let valid_source =
                    |source: &super::SourceId| !source.file.is_empty() && source.path_id != 0;
                valid_source(&stage.source)
                    && super::finite(&[stage.order, stage.near, stage.far, stage.field_of_view])
                    && stage.near > 0.
                    && stage.far > stage.near
                    && stage.field_of_view > 0.
                    && stage.field_of_view < 180.
                    && stage.hdr
                    && stage.allow_msaa
                    && match (&stage.role, &stage.postprocessing) {
                        (CameraRole::Sky, None) => matches!(stage.clear, CameraClear::Skybox),
                        (CameraRole::Main, Some(effect)) => {
                            valid_source(&effect.source)
                                && matches!(stage.clear, CameraClear::Depth)
                                && effect.scope == CameraEffectScope::MainStack
                                && effect.volume_mask == 2097408
                        }
                        (CameraRole::Far, Some(effect)) => {
                            valid_source(&effect.source)
                                && matches!(stage.clear, CameraClear::Depth)
                                && effect.scope == CameraEffectScope::DitherOnly
                                && effect.volume_mask == 0
                        }
                        (CameraRole::Foreground, Some(effect)) => {
                            valid_source(&effect.source)
                                && matches!(stage.clear, CameraClear::Depth)
                                && effect.scope == CameraEffectScope::DitherOnly
                                && effect.volume_mask == 32
                        }
                        _ => false,
                    }
            })
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceParameter<T> {
    pub override_state: bool,
    pub value: T,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceTextureParameter {
    pub override_state: bool,
    pub value: Option<super::SourceId>,
    pub default_state: u32,
    pub texture: Option<u32>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceGradingSpline {
    pub curve: super::TorqueCurve,
    pub cached_data: Vec<f32>,
    pub r#loop: bool,
    pub range: f32,
    pub zero_value: f32,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlassesGradingParameters {
    #[serde(rename = "blueCurve")]
    pub blue_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "brightness")]
    pub brightness: SourceParameter<f32>,
    #[serde(rename = "colorFilter")]
    pub color_filter: SourceParameter<[f32; 4]>,
    #[serde(rename = "contrast")]
    pub contrast: SourceParameter<f32>,
    #[serde(rename = "enabled")]
    pub enabled: SourceParameter<bool>,
    #[serde(rename = "externalLut")]
    pub external_lut: SourceTextureParameter,
    #[serde(rename = "gain")]
    pub gain: SourceParameter<[f32; 4]>,
    #[serde(rename = "gamma")]
    pub gamma: SourceParameter<[f32; 4]>,
    #[serde(rename = "gradingMode")]
    pub grading_mode: SourceParameter<u32>,
    #[serde(rename = "greenCurve")]
    pub green_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "hueShift")]
    pub hue_shift: SourceParameter<f32>,
    #[serde(rename = "hueVsHueCurve")]
    pub hue_vs_hue_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "hueVsSatCurve")]
    pub hue_vs_sat_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "ldrLut")]
    pub ldr_lut: SourceTextureParameter,
    #[serde(rename = "ldrLutContribution")]
    pub ldr_lut_contribution: SourceParameter<f32>,
    #[serde(rename = "lift")]
    pub lift: SourceParameter<[f32; 4]>,
    #[serde(rename = "lumVsSatCurve")]
    pub lum_vs_sat_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "masterCurve")]
    pub master_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "mixerBlueOutBlueIn")]
    pub mixer_blue_out_blue_in: SourceParameter<f32>,
    #[serde(rename = "mixerBlueOutGreenIn")]
    pub mixer_blue_out_green_in: SourceParameter<f32>,
    #[serde(rename = "mixerBlueOutRedIn")]
    pub mixer_blue_out_red_in: SourceParameter<f32>,
    #[serde(rename = "mixerGreenOutBlueIn")]
    pub mixer_green_out_blue_in: SourceParameter<f32>,
    #[serde(rename = "mixerGreenOutGreenIn")]
    pub mixer_green_out_green_in: SourceParameter<f32>,
    #[serde(rename = "mixerGreenOutRedIn")]
    pub mixer_green_out_red_in: SourceParameter<f32>,
    #[serde(rename = "mixerRedOutBlueIn")]
    pub mixer_red_out_blue_in: SourceParameter<f32>,
    #[serde(rename = "mixerRedOutGreenIn")]
    pub mixer_red_out_green_in: SourceParameter<f32>,
    #[serde(rename = "mixerRedOutRedIn")]
    pub mixer_red_out_red_in: SourceParameter<f32>,
    #[serde(rename = "postExposure")]
    pub post_exposure: SourceParameter<f32>,
    #[serde(rename = "redCurve")]
    pub red_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "satVsSatCurve")]
    pub sat_vs_sat_curve: SourceParameter<SourceGradingSpline>,
    #[serde(rename = "saturation")]
    pub saturation: SourceParameter<f32>,
    #[serde(rename = "temperature")]
    pub temperature: SourceParameter<f32>,
    #[serde(rename = "tint")]
    pub tint: SourceParameter<f32>,
    #[serde(rename = "toneCurveGamma")]
    pub tone_curve_gamma: SourceParameter<f32>,
    #[serde(rename = "toneCurveShoulderAngle")]
    pub tone_curve_shoulder_angle: SourceParameter<f32>,
    #[serde(rename = "toneCurveShoulderLength")]
    pub tone_curve_shoulder_length: SourceParameter<f32>,
    #[serde(rename = "toneCurveShoulderStrength")]
    pub tone_curve_shoulder_strength: SourceParameter<f32>,
    #[serde(rename = "toneCurveToeLength")]
    pub tone_curve_toe_length: SourceParameter<f32>,
    #[serde(rename = "toneCurveToeStrength")]
    pub tone_curve_toe_strength: SourceParameter<f32>,
    #[serde(rename = "tonemapper")]
    pub tonemapper: SourceParameter<u32>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GlassesEffect {
    ColorGrading {
        source: super::SourceId,
        active: bool,
        parameters: GlassesGradingParameters,
    },
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlassesProfile {
    pub source: super::SourceId,
    pub effects: Vec<GlassesEffect>,
}
impl SourceGradingSpline {
    fn valid(&self) -> bool {
        super::particles::curve_valid(&self.curve)
            && super::finite(&self.cached_data)
            && self.cached_data.len() == 128
            && self.range.is_finite()
            && self.zero_value.is_finite()
    }
}
impl GlassesGradingParameters {
    fn finite(&self) -> bool {
        self.blue_curve.value.valid()
            && self.brightness.value.is_finite()
            && super::finite(&self.color_filter.value)
            && self.contrast.value.is_finite()
            && super::finite(&self.gain.value)
            && super::finite(&self.gamma.value)
            && self.green_curve.value.valid()
            && self.hue_shift.value.is_finite()
            && self.hue_vs_hue_curve.value.valid()
            && self.hue_vs_sat_curve.value.valid()
            && self.ldr_lut_contribution.value.is_finite()
            && super::finite(&self.lift.value)
            && self.lum_vs_sat_curve.value.valid()
            && self.master_curve.value.valid()
            && self.mixer_blue_out_blue_in.value.is_finite()
            && self.mixer_blue_out_green_in.value.is_finite()
            && self.mixer_blue_out_red_in.value.is_finite()
            && self.mixer_green_out_blue_in.value.is_finite()
            && self.mixer_green_out_green_in.value.is_finite()
            && self.mixer_green_out_red_in.value.is_finite()
            && self.mixer_red_out_blue_in.value.is_finite()
            && self.mixer_red_out_green_in.value.is_finite()
            && self.mixer_red_out_red_in.value.is_finite()
            && self.post_exposure.value.is_finite()
            && self.red_curve.value.valid()
            && self.sat_vs_sat_curve.value.valid()
            && self.saturation.value.is_finite()
            && self.temperature.value.is_finite()
            && self.tint.value.is_finite()
            && self.tone_curve_gamma.value.is_finite()
            && self.tone_curve_shoulder_angle.value.is_finite()
            && self.tone_curve_shoulder_length.value.is_finite()
            && self.tone_curve_shoulder_strength.value.is_finite()
            && self.tone_curve_toe_length.value.is_finite()
            && self.tone_curve_toe_strength.value.is_finite()
    }
}
impl GlassesProfile {
    pub fn validate(
        &self,
        index: usize,
        m: &super::ReferenceManifest,
    ) -> Result<(), super::ReferenceError> {
        use super::{SourceId, require};
        let sid = |file: &str, path_id| SourceId {
            file: file.into(),
            path_id,
        };
        require(
            self.source == sid("sharedassets1.assets", 130128 + index as i64)
                && self.effects.len() == 1,
            "glasses profile identity",
        )?;
        let GlassesEffect::ColorGrading {
            source,
            active,
            parameters: p,
        } = &self.effects[0];
        require(
            *source == sid("sharedassets1.assets", 130142 + index as i64) && *active && p.finite(),
            "glasses effect source",
        )?;
        require(
            p.enabled.override_state
                && p.enabled.value
                && p.grading_mode.override_state
                && p.grading_mode.value == 1
                && p.tonemapper.override_state
                && p.tonemapper.value == 0
                && p.post_exposure.override_state
                && p.post_exposure.value == if index == 0 { -2. } else { 0. }
                && p.tone_curve_toe_length.override_state
                && p.tone_curve_toe_length.value == 0.5
                && p.tone_curve_toe_strength.override_state
                && p.tone_curve_toe_strength.value == 0.,
            "glasses grading source overrides",
        )?;
        require(
            !p.blue_curve.override_state
                && !p.brightness.override_state
                && !p.color_filter.override_state
                && !p.contrast.override_state
                && !p.external_lut.override_state
                && !p.gain.override_state
                && !p.gamma.override_state
                && !p.green_curve.override_state
                && !p.hue_shift.override_state
                && !p.hue_vs_hue_curve.override_state
                && !p.hue_vs_sat_curve.override_state
                && !p.ldr_lut.override_state
                && !p.ldr_lut_contribution.override_state
                && !p.lift.override_state
                && !p.lum_vs_sat_curve.override_state
                && !p.master_curve.override_state
                && !p.mixer_blue_out_green_in.override_state
                && !p.mixer_blue_out_red_in.override_state
                && !p.mixer_green_out_blue_in.override_state
                && !p.mixer_green_out_red_in.override_state
                && !p.mixer_red_out_blue_in.override_state
                && !p.mixer_red_out_green_in.override_state
                && !p.red_curve.override_state
                && !p.sat_vs_sat_curve.override_state
                && !p.saturation.override_state
                && !p.temperature.override_state
                && !p.tint.override_state
                && !p.tone_curve_gamma.override_state
                && !p.tone_curve_shoulder_angle.override_state
                && !p.tone_curve_shoulder_length.override_state
                && !p.tone_curve_shoulder_strength.override_state,
            "unexpected glasses override",
        )?;
        let rgb = [
            [100., 100., 100.],
            [50., 100., 200.],
            [75., 200., 75.],
            [200., 150., 80.],
            [150., 50., 50.],
            [150., 150., 50.],
        ][index];
        for (p, value) in [
            &p.mixer_red_out_red_in,
            &p.mixer_green_out_green_in,
            &p.mixer_blue_out_blue_in,
        ]
        .into_iter()
        .zip(rgb)
        {
            require(
                p.override_state == (index > 0) && p.value == value,
                "glasses source channel mixer",
            )?;
        }
        for (p, expected, default) in [
            (&p.ldr_lut, Some(sid("sharedassets0.assets", 255)), 4),
            (&p.external_lut, None, 1),
        ] {
            require(
                p.value == expected
                    && !p.override_state
                    && p.default_state == default
                    && match (&p.value, p.texture) {
                        (Some(s), Some(id)) => m
                            .lighting
                            .textures
                            .iter()
                            .any(|t| t.id == id && &t.source == s),
                        (None, None) => true,
                        _ => false,
                    },
                "glasses profile resource",
            )?;
        }
        Ok(())
    }
}

/// Main camera `PostProcessLayer.volumeLayer` (layers 8 and 21).
pub const MAIN_VOLUME_MASK: u32 = 2097408;

/// `fpscontroller.DeadV`: a global volume over the main stack. `mainscript.Died` enables it.
/// While dead, its weight is `InverseLerp(0, 14, t - deathTime)`; `survival` holds both constants.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeadProfile {
    /// `PostProcessVolume`.
    pub source: super::SourceId,
    pub enabled: bool,
    pub global: bool,
    pub blend_distance: f32,
    /// Authored weight. The death fade replaces it.
    pub weight: f32,
    pub priority: f32,
    pub layer: u32,
    /// `PostProcessVolume.sharedProfile`.
    pub profile: super::SourceId,
    pub effects: Vec<VolumeEffect>,
    pub evidence: super::ManagedEvidence,
}

/// One profile effect with every serialized parameter, by source field name.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeEffect {
    /// Snake case class name, such as `color_grading` or `vignette`.
    pub kind: String,
    pub source: super::SourceId,
    pub active: bool,
    pub parameters: std::collections::BTreeMap<String, VolumeParameter>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum VolumeParameter {
    Texture(SourceTextureParameter),
    Spline(SourceParameter<SourceGradingSpline>),
    Bool(SourceParameter<bool>),
    Number(SourceParameter<f32>),
    /// Color (rgba) or vector (xy, xyz or xyzw).
    Vector(SourceParameter<Vec<f32>>),
}

impl VolumeParameter {
    pub fn override_state(&self) -> bool {
        match self {
            Self::Texture(p) => p.override_state,
            Self::Spline(p) => p.override_state,
            Self::Bool(p) => p.override_state,
            Self::Number(p) => p.override_state,
            Self::Vector(p) => p.override_state,
        }
    }
}

impl DeadProfile {
    pub fn validate(&self, m: &super::ReferenceManifest) -> Result<(), super::ReferenceError> {
        use super::{finite, ids, require};
        require(
            self.source.file == "level1"
                && self.global
                && finite(&[self.blend_distance, self.weight, self.priority])
                && (0. ..=1.).contains(&self.weight)
                && self.layer < 32
                && MAIN_VOLUME_MASK & (1 << self.layer) != 0
                && !self.effects.is_empty()
                && self.evidence.valid(),
            "dead volume",
        )?;
        ids(
            self.effects.iter().map(|e| &e.source),
            "duplicate dead effect",
        )?;
        for e in &self.effects {
            require(
                !e.kind.is_empty()
                    && e.kind
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                    && matches!(e.parameters.get("enabled"), Some(VolumeParameter::Bool(_))),
                "dead effect",
            )?;
            for p in e.parameters.values() {
                require(
                    match p {
                        VolumeParameter::Texture(t) => match (&t.value, t.texture) {
                            (Some(s), Some(id)) => m
                                .lighting
                                .textures
                                .iter()
                                .any(|v| v.id == id && &v.source == s),
                            (None, None) => true,
                            _ => false,
                        },
                        VolumeParameter::Spline(s) => s.value.valid(),
                        VolumeParameter::Bool(_) => true,
                        VolumeParameter::Number(n) => n.value.is_finite(),
                        VolumeParameter::Vector(v) => {
                            (2..=4).contains(&v.value.len()) && finite(&v.value)
                        }
                    },
                    "dead effect parameter",
                )?;
            }
        }
        Ok(())
    }
}

/// `ColorGrading` parameters of the main grading, by source field name, in `GradingSettings::values` order.
/// `colorFilter`, `lift`, `gamma` and `gain` are four values each. The mixer names come from `MIXER`.
const GRADING_FIELDS: [(&str, usize); 10] = [
    ("temperature", 0),
    ("tint", 1),
    ("hueShift", 2),
    ("saturation", 3),
    ("postExposure", 4),
    ("contrast", 5),
    ("colorFilter", 6),
    ("lift", 19),
    ("gamma", 23),
    ("gain", 27),
];
/// `mixer{out}Out{in}In` starts at value 10, row by output channel.
const MIXER: [&str; 3] = ["Red", "Green", "Blue"];

/// The `GradingSettings::values` index of a source field. A vector field gives its first index.
fn grading_index(name: &str) -> Option<usize> {
    fn channel(name: &str) -> Option<(usize, &str)> {
        MIXER
            .iter()
            .enumerate()
            .find_map(|(i, c)| name.strip_prefix(c).map(|rest| (i, rest)))
    }
    let mixer = || {
        let (output, rest) = channel(name.strip_prefix("mixer")?)?;
        let (input, rest) = channel(rest.strip_prefix("Out")?)?;
        (rest == "In").then_some(10 + output * 3 + input)
    };
    GRADING_FIELDS
        .iter()
        .find(|(field, _)| *field == name)
        .map(|(_, index)| *index)
        .or_else(mixer)
}

/// `ColorGrading.tonemapper` of the HDR LUT kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tonemapper {
    /// Source value 0: `KGenLut3D_NoTonemap`.
    None,
    /// Source value 1: `KGenLut3D_NeutralTonemap`. The main grading uses it.
    Neutral,
}

impl GlassesGradingParameters {
    /// The number parameters that `GradingSettings` holds, by source field name.
    fn numbers(&self) -> [(&'static str, &SourceParameter<f32>); 15] {
        [
            ("temperature", &self.temperature),
            ("tint", &self.tint),
            ("hueShift", &self.hue_shift),
            ("saturation", &self.saturation),
            ("postExposure", &self.post_exposure),
            ("contrast", &self.contrast),
            ("mixerRedOutRedIn", &self.mixer_red_out_red_in),
            ("mixerRedOutGreenIn", &self.mixer_red_out_green_in),
            ("mixerRedOutBlueIn", &self.mixer_red_out_blue_in),
            ("mixerGreenOutRedIn", &self.mixer_green_out_red_in),
            ("mixerGreenOutGreenIn", &self.mixer_green_out_green_in),
            ("mixerGreenOutBlueIn", &self.mixer_green_out_blue_in),
            ("mixerBlueOutRedIn", &self.mixer_blue_out_red_in),
            ("mixerBlueOutGreenIn", &self.mixer_blue_out_green_in),
            ("mixerBlueOutBlueIn", &self.mixer_blue_out_blue_in),
        ]
    }
    /// The four-value parameters that `GradingSettings` holds, by source field name.
    fn vectors(&self) -> [(&'static str, &SourceParameter<[f32; 4]>); 4] {
        [
            ("colorFilter", &self.color_filter),
            ("lift", &self.lift),
            ("gamma", &self.gamma),
            ("gain", &self.gain),
        ]
    }
}

impl GradingSettings {
    /// Source `PPVSunGlasses` over the main grading: PPv2 volume blending at weight 1.
    /// An overridden number takes the lens value. An overridden tonemapper switches.
    /// `GlassesProfile::validate` allows only these overrides, the HDR grading mode and the toe values.
    /// The toe values change only the Custom tonemapper. Thus they have no effect here.
    pub fn with_glasses(&self, profile: &GlassesProfile) -> (Self, Tonemapper) {
        let mut grading = *self;
        let mut tonemapper = Tonemapper::Neutral;
        for effect in &profile.effects {
            let GlassesEffect::ColorGrading {
                active: true,
                parameters: p,
                ..
            } = effect
            else {
                continue;
            };
            let values = grading.values();
            for (name, parameter) in p.numbers() {
                if parameter.override_state {
                    *values[grading_index(name).unwrap()] = parameter.value;
                }
            }
            for (name, parameter) in p.vectors() {
                if parameter.override_state {
                    let index = grading_index(name).unwrap();
                    for (i, value) in parameter.value.iter().enumerate() {
                        *values[index + i] = *value;
                    }
                }
            }
            if p.tonemapper.override_state && p.tonemapper.value == 0 {
                tonemapper = Tonemapper::None;
            }
        }
        (grading, tonemapper)
    }
    /// Every blended number: the six scalars, color filter, channel mixer rows and the lift, gamma and gain trackballs.
    fn values(&mut self) -> [&mut f32; 31] {
        let Self {
            temperature,
            tint,
            hue_shift,
            saturation,
            post_exposure,
            contrast,
            color_filter: [f0, f1, f2, f3],
            channel_mixer: [[m0, m1, m2], [m3, m4, m5], [m6, m7, m8]],
            trackballs: [[l0, l1, l2, l3], [g0, g1, g2, g3], [n0, n1, n2, n3]],
            ..
        } = self;
        [
            temperature,
            tint,
            hue_shift,
            saturation,
            post_exposure,
            contrast,
            f0,
            f1,
            f2,
            f3,
            m0,
            m1,
            m2,
            m3,
            m4,
            m5,
            m6,
            m7,
            m8,
            l0,
            l1,
            l2,
            l3,
            g0,
            g1,
            g2,
            g3,
            n0,
            n1,
            n2,
            n3,
        ]
    }
}

/// The `DeadV` overrides that the main grading supports.
/// Source PostProcessManager blends each overridden parameter as `Lerp(main, dead, weight)`.
/// The port main stack has only `ColorGrading` in its HDR Neutral variant with default curves.
/// Thus other effects, curve and texture parameters, and a different grading mode or tonemapper are ignored.
/// `new` names each ignored overridden parameter.
#[derive(Clone, Debug, Default)]
pub struct DeadGrading {
    /// `GradingSettings::values` index and its `DeadV` value.
    overrides: Vec<(usize, f32)>,
}

impl DeadGrading {
    /// The supported overrides of the active `DeadV` effects, and the ignored ones as `effect.parameter`.
    pub fn new(profile: &DeadProfile) -> (Self, Vec<String>) {
        let mut overrides = Vec::new();
        let mut ignored = Vec::new();
        for effect in profile.effects.iter().filter(|e| e.active) {
            for (name, parameter) in &effect.parameters {
                if !parameter.override_state() {
                    continue;
                }
                let grading = effect.kind == "color_grading";
                let index = grading_index(name);
                match (grading, index, parameter) {
                    (true, Some(index), VolumeParameter::Number(p))
                        if index < 6 || (10..19).contains(&index) =>
                    {
                        overrides.push((index, p.value));
                    }
                    (true, Some(index), VolumeParameter::Vector(p))
                        if (index == 6 || index >= 19) && p.value.len() == 4 =>
                    {
                        overrides.extend(p.value.iter().enumerate().map(|(i, v)| (index + i, *v)));
                    }
                    // The main grading is enabled, HDR (gradingMode 1) and Neutral (tonemapper 1).
                    (true, None, VolumeParameter::Bool(p)) if name == "enabled" && p.value => {}
                    (true, None, VolumeParameter::Number(p))
                        if (name == "gradingMode" || name == "tonemapper") && p.value == 1. => {}
                    _ => ignored.push(format!("{}.{name}", effect.kind)),
                }
            }
        }
        (Self { overrides }, ignored)
    }
    /// Blends the overrides over `grading` with `weight` from 0 to 1.
    pub fn blend(&self, grading: &mut GradingSettings, weight: f32) {
        let values = grading.values();
        for (index, target) in &self.overrides {
            let value = &mut *values[*index];
            *value += (target - *value) * weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MotionBlurSettings;

    #[test]
    fn shutter_follows_speed_and_lerps() {
        let blur = MotionBlurSettings {
            samples: 10,
            shutter_range: [0., 180.],
            speed_range: [50., 150.],
            shutter_lerp: 2.,
        };
        // Below the speed range, a full lerp step reaches the low angle.
        assert_eq!(blur.shutter(90., 20., 0.5), 0.);
        // Half the speed range, a quarter step: 0 + (90 - 0) * 0.25.
        assert_eq!(blur.shutter(0., 100., 0.125), 22.5);
        // Above the range, the target clamps to the high angle.
        assert_eq!(blur.shutter(0., 400., 1.), 180.);
    }
}
