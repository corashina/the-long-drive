//! Camera-local Uber output and persistent noise and exposure bindings.
//! It also does the installed main camera heat haze (`postprocessingscript` with `Custom/heathazeshader`).
//! The source haze moves the Uber output. Here the Uber runs at the moved UV, so no extra pass is necessary.
use super::grain::GRAIN_SIZE;
use super::{dither::DitherSequence, pipeline_failure};
use crate::presentation::render_targets::HELD_FORMAT;
use crate::reference::{postprocessing::HeatHaze, stack_effects::StackEffects};
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct Params {
    extent: Vec4,
    noise: Vec4,
    bloom: Vec4,
    tint: Vec4,
    dirt: Vec4,
    grading: Vec4,
    /// `_Vignette_Color` linear, then `_Vignette_Opacity`.
    vignette: Vec4,
    /// `_Vignette_Settings`.
    vignette_shape: Vec4,
    /// `_Vignette_Center`, then mode (0 off, 1 classic, 2 masked) and the screen aspect.
    vignette_center: Vec4,
    /// Post stack v2 `_Distortion_Amount` and `_Distortion_CenterScale`.
    distortion: Vec4,
    distortion_center: Vec4,
    /// Chromatic aberration amount and mode (0 off, 1 low, 2 full), then distortion on.
    lens: Vec4,
    /// `_Grain_Params1`. Intensity 0 is off.
    grain: Vec4,
    /// `_Grain_Params2`.
    grain_tile: Vec4,
    /// LDR `_Lut2D_Params`.
    strip: Vec4,
    /// Heat haze `_Weight` (0 is off), `_MinDist`, `_DistanceM`, `_Time.y`.
    haze: Vec4,
    haze_up_offset_scale: Vec4,
    haze_down_offset_scale: Vec4,
    /// `_UpSpeed`, `_UpSpeed2`, `_UpStrength`.
    haze_up: Vec4,
    /// `_DownSpeed`, `_DownSpeed2`, `_DownStrength`.
    haze_down: Vec4,
}
pub struct MainInputs<'a> {
    pub bloom: &'a TextureView,
    pub exposure: [&'a TextureView; 2],
    pub dirt: &'a TextureView,
    pub dirt_extent: [u32; 2],
    pub vignette_mask: &'a TextureView,
    pub lut: &'a TextureView,
    /// The LDR grading strip.
    pub strip: &'a TextureView,
    pub grain: &'a TextureView,
    pub sample_scale: f32,
    /// The main view depth.
    pub camera_depth: &'a TextureView,
    /// `HeatHaze::noise`.
    pub haze_noise: &'a TextureView,
    pub haze: &'a HeatHaze,
}
pub struct UberPass {
    uniform: UniformBuffer<Params>,
    bindings: Vec<BindGroup>,
    /// HDR then LDR grading.
    pipelines: [CachedRenderPipelineId; 2],
    sequence: DitherSequence,
    pending: Option<DitherSequence>,
    extent: [u32; 2],
    haze_temperature: [f32; 2],
}
impl UberPass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
        noise: [&TextureView; 64],
        extent: [u32; 2],
        input: MainInputs<'_>,
        previous: Option<Self>,
    ) -> Result<Self, String> {
        if extent.contains(&0) {
            return Err("Source Uber requires a positive extent".into());
        }
        let mut entries = BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<Params>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                sampler(SamplerBindingType::NonFiltering),
            ),
        )
        .to_vec();
        entries.extend(
            BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                (
                    (5, texture_2d(TextureSampleType::Float { filterable: true })),
                    (
                        6,
                        texture_2d(TextureSampleType::Float { filterable: false }),
                    ),
                    (7, texture_2d(TextureSampleType::Float { filterable: true })),
                    (8, texture_3d(TextureSampleType::Float { filterable: true })),
                    (9, sampler(SamplerBindingType::Filtering)),
                    (
                        10,
                        texture_2d(TextureSampleType::Float { filterable: true }),
                    ),
                    (
                        11,
                        texture_2d(TextureSampleType::Float { filterable: true }),
                    ),
                    (
                        12,
                        texture_2d(TextureSampleType::Float { filterable: true }),
                    ),
                    (
                        13,
                        texture_2d(TextureSampleType::Float { filterable: false }),
                    ),
                    (
                        14,
                        texture_2d(TextureSampleType::Float { filterable: true }),
                    ),
                ),
            )
            .to_vec(),
        );
        let layout = BindGroupLayoutDescriptor::new("source_uber", &entries);
        let pipelines = previous.as_ref().map_or_else(
            || {
                [false, true].map(|ldr| {
                    let mut shader_defs = vec!["SOURCE_UBER_MAIN".into()];
                    if ldr {
                        shader_defs.push("SOURCE_UBER_LDR".into());
                    }
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some("source_uber".into()),
                        layout: vec![layout.clone()],
                        vertex: VertexState {
                            shader: shader.clone(),
                            entry_point: Some("vertex".into()),
                            ..default()
                        },
                        fragment: Some(FragmentState {
                            shader: shader.clone(),
                            entry_point: Some("uber".into()),
                            shader_defs,
                            targets: vec![Some(ColorTargetState {
                                format: HELD_FORMAT,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            })],
                            ..default()
                        }),
                        ..default()
                    })
                })
            },
            |old| old.pipelines,
        );
        let mut params = Params {
            extent: Vec4::new(
                extent[0] as f32,
                extent[1] as f32,
                1. / extent[0] as f32,
                1. / extent[1] as f32,
            ),
            noise: Vec4::ZERO,
            bloom: Vec4::new(input.sample_scale, 0., 0., 0.),
            tint: Vec4::ONE,
            dirt: Vec4::new(1., 1., 0., 0.),
            grading: Vec4::new(1. / 33., 32., 1., 0.),
            vignette: Vec4::ZERO,
            vignette_shape: Vec4::ZERO,
            vignette_center: Vec4::ZERO,
            distortion: Vec4::ZERO,
            distortion_center: Vec4::ZERO,
            lens: Vec4::ZERO,
            grain: Vec4::ZERO,
            grain_tile: Vec4::ZERO,
            strip: super::lut_ldr::STRIP_PARAMS,
            haze: Vec4::new(0., input.haze.min_dist, input.haze.distance_m, 0.),
            haze_up_offset_scale: Vec4::from_array(input.haze.up_offset_scale),
            haze_down_offset_scale: Vec4::from_array(input.haze.down_offset_scale),
            haze_up: Vec4::new(
                input.haze.up_speed[0],
                input.haze.up_speed[1],
                input.haze.up_strength,
                0.,
            ),
            haze_down: Vec4::new(
                input.haze.down_speed[0],
                input.haze.down_speed[1],
                input.haze.down_strength,
                0.,
            ),
        };
        if input.dirt_extent.contains(&0) {
            return Err("Source bloom dirt extent is invalid".into());
        }
        let image_aspect = input.dirt_extent[0] as f32 / input.dirt_extent[1] as f32;
        let screen_aspect = extent[0] as f32 / extent[1] as f32;
        if image_aspect > screen_aspect {
            params.dirt.x = screen_aspect / image_aspect;
            params.dirt.z = (1. - params.dirt.x) / 2.;
        } else {
            params.dirt.y = image_aspect / screen_aspect;
            params.dirt.w = (1. - params.dirt.y) / 2.;
        }
        let mut uniform = UniformBuffer::from(params);
        uniform.write_buffer(device, queue);
        let linear = device.create_sampler(&SamplerDescriptor {
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            ..default()
        });
        let point = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            ..default()
        });
        let repeat = device.create_sampler(&SamplerDescriptor {
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            ..default()
        });
        let bindings = (0..128)
            .map(|i| {
                let mut entries = BindGroupEntries::sequential((
                    uniform.binding().unwrap(),
                    source,
                    noise[i % 64],
                    &linear,
                    &point,
                ))
                .to_vec();
                entries.extend(
                    BindGroupEntries::with_indices((
                        (5, input.bloom),
                        (6, input.exposure[i / 64]),
                        (7, input.dirt),
                        (8, input.lut),
                        (9, &repeat),
                        (10, input.vignette_mask),
                        (11, input.strip),
                        (12, input.grain),
                        (13, input.camera_depth),
                        (14, input.haze_noise),
                    ))
                    .to_vec(),
                );
                device.create_bind_group(
                    "source_uber",
                    &cache.get_bind_group_layout(&layout),
                    &entries,
                )
            })
            .collect();
        Ok(Self {
            uniform,
            bindings,
            pipelines,
            sequence: previous.map_or_else(DitherSequence::default, |old| old.sequence),
            pending: None,
            extent,
            haze_temperature: input.haze.temperature,
        })
    }
    pub fn recreate(&mut self) {
        self.sequence = DitherSequence::default();
        self.pending = None;
    }
    /// Grading post exposure in EV. `encode` writes it.
    pub fn set_post_exposure(&mut self, post_exposure: f32) {
        self.uniform.get_mut().grading.z = post_exposure.exp2();
    }
    /// The blended stack effects. `encode` writes them.
    pub fn set_effects(&mut self, e: &StackEffects) {
        let linear = |c: [f32; 4]| {
            let g = super::bloom::gamma_to_linear;
            Vec4::new(g(c[0]), g(c[1]), g(c[2]), c[3])
        };
        let p = self.uniform.get_mut();
        // BloomRenderer.Render.
        p.bloom.y = (e.bloom.intensity / 10.).exp2() - 1.;
        p.bloom.z = e.bloom.dirt_intensity;
        p.tint = linear(e.bloom.color);
        // ChromaticAberrationRenderer.Render.
        let c = e.chromatic;
        p.lens = Vec4::new(
            if c.on() { c.intensity * 0.05 } else { 0. },
            match (c.on(), c.fast) {
                (false, _) => 0.,
                (true, true) => 1.,
                (true, false) => 2.,
            },
            f32::from(u8::from(e.distortion.on())),
            0.,
        );
        // LensDistortionRenderer.Render.
        let d = e.distortion;
        let theta = (1.6 * d.intensity.abs().max(1.)).min(160.).to_radians();
        p.distortion = Vec4::new(
            if d.intensity >= 0. { theta } else { 1. / theta },
            2. * (theta * 0.5).tan(),
            1. / d.scale,
            d.intensity,
        );
        p.distortion_center = Vec4::new(
            d.center_x,
            d.center_y,
            d.intensity_x.max(1e-4),
            d.intensity_y.max(1e-4),
        );
        // VignetteRenderer.Render.
        let v = e.vignette;
        p.vignette = linear(v.color).with_w(v.opacity.clamp(0., 1.));
        p.vignette_shape = Vec4::new(
            v.intensity * 3.,
            v.smoothness * 5.,
            (1. - v.roundness) * 6. + v.roundness,
            f32::from(u8::from(v.rounded)),
        );
        p.vignette_center = Vec4::new(
            v.center[0],
            v.center[1],
            match (v.on(), v.masked) {
                (false, _) => 0.,
                (true, false) => 1.,
                (true, true) => 2.,
            },
            self.extent[0] as f32 / self.extent[1] as f32,
        );
        // GrainRenderer.Render.
        let g = e.grain;
        p.grain = if g.on() {
            Vec4::new(g.lum_contrib, g.intensity * 20., 0., 0.)
        } else {
            Vec4::ZERO
        };
        let tile = UVec2::from_array(self.extent).as_vec2() / GRAIN_SIZE as f32 / g.size;
        p.grain_tile = tile.extend(p.grain_tile.z).extend(p.grain_tile.w);
    }
    /// `temperature` is the ambient temperature while the heat haze setting is on. `time` is the shader `_Time.y`.
    /// `napszakvaltakozas`: `_Weight` = InverseLerp(heatHazeTempMin, heatHazeTempMax, temp).
    /// At weight 0 the haze does not move the image, so the shader skips it.
    pub fn set_haze(&mut self, temperature: Option<f32>, time: f32) {
        let [min, max] = self.haze_temperature;
        let haze = &mut self.uniform.get_mut().haze;
        haze.x = temperature.map_or(0., |t| ((t - min) / (max - min)).clamp(0., 1.));
        haze.w = time;
    }
    /// The Halton offset of this frame's grain lookup.
    pub fn set_grain_offset(&mut self, offset: [f32; 2]) {
        let p = self.uniform.get_mut();
        p.grain_tile.z = offset[0];
        p.grain_tile.w = offset[1];
    }
    pub fn noise_index(&self) -> u32 {
        self.sequence.noise_index()
    }
    pub fn completed(&mut self) {
        if let Some(sequence) = self.pending.take() {
            self.sequence = sequence;
        }
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|p| cache.get_render_pipeline(*p).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        pipeline_failure(cache, &self.pipelines)
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        output: &TextureView,
        exposure_index: usize,
        ldr: bool,
    ) -> bool {
        if !self.ready(cache) || exposure_index > 1 {
            return false;
        }
        let mut sequence = self.sequence.clone();
        let (index, noise) = sequence.render(self.extent);
        self.pending = Some(sequence);
        self.uniform.get_mut().noise = Vec4::from_array(noise);
        self.uniform.write_buffer(device, queue);
        let colors = [Some(RenderPassColorAttachment {
            view: output,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_uber"),
            color_attachments: &colors,
            ..default()
        });
        let pipeline = self.pipelines[usize::from(ldr)];
        pass.set_pipeline(cache.get_render_pipeline(pipeline).unwrap());
        pass.set_bind_group(0, &self.bindings[exposure_index * 64 + index], &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
