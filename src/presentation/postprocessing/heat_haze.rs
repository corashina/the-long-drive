//! Installed main camera heat haze: `postprocessingscript` with `Custom/heathazeshader`.
//! It runs as OnRenderImage after the main stack. The Uber writes `input`, then this pass writes the world target.
use crate::presentation::render_targets::HDR_FORMAT;
use crate::reference::postprocessing::HeatHaze;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct Params {
    extent: Vec4,
    up_offset_scale: Vec4,
    down_offset_scale: Vec4,
    values: Vec4,
    up: Vec4,
    down: Vec4,
}
pub struct HeatHazePass {
    _input: Texture,
    /// The single-sample Uber output that this pass reads.
    pub input: TextureView,
    uniform: UniformBuffer<Params>,
    group: BindGroup,
    pipeline: CachedRenderPipelineId,
    temperature: [f32; 2],
}
impl HeatHazePass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        camera_depth: &TextureView,
        noise: &TextureView,
        extent: [u32; 2],
        samples: u32,
        record: &HeatHaze,
        previous: Option<&Self>,
    ) -> Self {
        let input = device.create_texture(&TextureDescriptor {
            label: Some("source_heat_haze_input"),
            size: Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: HDR_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let input_view = input.create_view(&default());
        let layout = BindGroupLayoutDescriptor::new(
            "source_heat_haze",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<Params>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let pipeline = previous.map_or_else(
            || {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("source_heat_haze".into()),
                    layout: vec![layout.clone()],
                    vertex: VertexState {
                        shader: shader.clone(),
                        entry_point: Some("vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        shader,
                        entry_point: Some("haze".into()),
                        targets: vec![Some(ColorTargetState {
                            format: HDR_FORMAT,
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                        ..default()
                    }),
                    multisample: MultisampleState {
                        count: samples,
                        ..default()
                    },
                    ..default()
                })
            },
            |old| old.pipeline,
        );
        // The OnRenderImage source: bilinear clamp. `noise`: bilinear repeat with mips.
        let linear_clamp = device.create_sampler(&SamplerDescriptor {
            label: Some("source_heat_haze_linear_clamp"),
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            ..default()
        });
        let noise_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("source_heat_haze_noise"),
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            ..default()
        });
        let mut uniform = UniformBuffer::from(Params {
            extent: Vec4::new(
                extent[0] as f32,
                extent[1] as f32,
                1. / extent[0] as f32,
                1. / extent[1] as f32,
            ),
            up_offset_scale: Vec4::from_array(record.up_offset_scale),
            down_offset_scale: Vec4::from_array(record.down_offset_scale),
            values: Vec4::new(0., record.min_dist, record.distance_m, 0.),
            up: Vec4::new(
                record.up_speed[0],
                record.up_speed[1],
                record.up_strength,
                0.,
            ),
            down: Vec4::new(
                record.down_speed[0],
                record.down_speed[1],
                record.down_strength,
                0.,
            ),
        });
        uniform.write_buffer(device, queue);
        let group = device.create_bind_group(
            "source_heat_haze",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((
                uniform.binding().unwrap(),
                &input_view,
                &linear_clamp,
                camera_depth,
                noise,
                &noise_sampler,
            )),
        );
        Self {
            _input: input,
            input: input_view,
            uniform,
            group,
            pipeline,
            temperature: record.temperature,
        }
    }
    /// `napszakvaltakozas`: `_Weight` = InverseLerp(heatHazeTempMin, heatHazeTempMax, temp).
    pub fn weight(&self, temperature: f32) -> f32 {
        let [min, max] = self.temperature;
        ((temperature - min) / (max - min)).clamp(0., 1.)
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.pipeline).is_some()
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::pipeline_failure(cache, &[self.pipeline])
    }
    /// `time` is the shader `_Time.y`.
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        output: &TextureView,
        resolve: Option<&TextureView>,
        weight: f32,
        time: f32,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return false;
        };
        let values = &mut self.uniform.get_mut().values;
        values.x = weight;
        values.w = time;
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
            label: Some("source_heat_haze"),
            color_attachments: &colors,
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
