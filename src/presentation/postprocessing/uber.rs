//! Camera-local Uber output and persistent noise/exposure bindings.
use super::{dither::DitherSequence, pipeline_failure};
use crate::reference::postprocessing::PostprocessingRecord;
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
    /// Masked vignette: linear color, then opacity. Opacity 0 is off.
    vignette: Vec4,
    /// Post stack v2 `_Distortion_Amount` and `_Distortion_CenterScale`.
    distortion: Vec4,
    distortion_center: Vec4,
    /// Chromatic aberration amount, binoculars, distortion. 0 is off.
    lens: Vec4,
    /// LDR `_Lut2D_Params`.
    strip: Vec4,
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
    pub sample_scale: f32,
}
pub struct UberPass {
    uniform: UniformBuffer<Params>,
    bindings: Vec<BindGroup>,
    /// HDR then LDR grading, each for the world target and for the single-sample heat haze input.
    pipelines: [CachedRenderPipelineId; 4],
    sequence: DitherSequence,
    pending: Option<DitherSequence>,
    extent: [u32; 2],
    /// The binocular `lens` row.
    binocular_lens: Vec4,
    vignette_opacity: f32,
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
        samples: u32,
        record: &PostprocessingRecord,
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
                ),
            )
            .to_vec(),
        );
        let layout = BindGroupLayoutDescriptor::new("source_uber", &entries);
        let pipelines = previous.as_ref().map_or_else(
            || {
                [(false, samples), (false, 1), (true, samples), (true, 1)].map(|(ldr, count)| {
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
                                format: TextureFormat::Rgba16Float,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            })],
                            ..default()
                        }),
                        multisample: MultisampleState { count, ..default() },
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
            bloom: Vec4::ZERO,
            tint: Vec4::ONE,
            dirt: Vec4::new(1., 1., 0., 0.),
            grading: Vec4::new(1. / 33., 32., record.grading.post_exposure.exp2(), 0.),
            vignette: Vec4::ZERO,
            distortion: Vec4::ZERO,
            distortion_center: Vec4::ZERO,
            lens: Vec4::ZERO,
            strip: super::lut_ldr::STRIP_PARAMS,
        };
        // LensDistortionRenderer.Render.
        let d = record.binoculars.distortion;
        let theta = (1.6 * d.intensity.abs().max(1.)).min(160.).to_radians();
        params.distortion = Vec4::new(
            if d.intensity >= 0. { theta } else { 1. / theta },
            2. * (theta * 0.5).tan(),
            1. / d.scale,
            d.intensity,
        );
        params.distortion_center = Vec4::new(
            d.center[0],
            d.center[1],
            d.intensity_xy[0].max(1e-4),
            d.intensity_xy[1].max(1e-4),
        );
        // IsEnabledAndSupported of each effect.
        let distortion_on = d.intensity != 0. && (d.intensity_xy[0] > 0. || d.intensity_xy[1] > 0.);
        let binocular_lens = Vec4::new(
            record.binoculars.chromatic_intensity * 0.05,
            1.,
            f32::from(u8::from(distortion_on)),
            0.,
        );
        if input.dirt_extent.contains(&0) {
            return Err("Source bloom dirt extent is invalid".into());
        }
        params.bloom = Vec4::new(
            input.sample_scale,
            (record.bloom.intensity / 10.).exp2() - 1.,
            record.bloom.dirt_intensity,
            0.,
        );
        let c = record.bloom.color;
        params.tint = Vec4::from_array([
            super::bloom::gamma_to_linear(c[0]),
            super::bloom::gamma_to_linear(c[1]),
            super::bloom::gamma_to_linear(c[2]),
            c[3],
        ]);
        let v = record.binoculars.color;
        params.vignette = Vec4::new(
            super::bloom::gamma_to_linear(v[0]),
            super::bloom::gamma_to_linear(v[1]),
            super::bloom::gamma_to_linear(v[2]),
            0.,
        );
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
            binocular_lens,
            vignette_opacity: record.binoculars.opacity,
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
    /// The binocular volume applies with weight 1, or not at all. `encode` writes it.
    pub fn set_binoculars(&mut self, on: bool) {
        let params = self.uniform.get_mut();
        params.vignette.w = if on { self.vignette_opacity } else { 0. };
        params.lens = if on { self.binocular_lens } else { Vec4::ZERO };
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
        resolve: Option<&TextureView>,
        exposure_index: usize,
        ldr: bool,
        single_sample: bool,
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
            resolve_target: resolve.map(|view| &**view),
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
        let pipeline = self.pipelines[usize::from(ldr) * 2 + usize::from(single_sample)];
        pass.set_pipeline(cache.get_render_pipeline(pipeline).unwrap());
        pass.set_bind_group(0, &self.bindings[exposure_index * 64 + index], &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
