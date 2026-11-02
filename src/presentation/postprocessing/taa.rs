//! Source TAA: history resolve after the scene copy. Histogram, bloom and Uber read its output.
use crate::presentation::render_targets::HDR_FORMAT;
use crate::reference::postprocessing::TaaSettings;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct TaaParams {
    extent: Vec4,
    jitter: Vec4,
    blend: Vec4,
}
pub struct TaaPass {
    _textures: [Texture; 3],
    /// History pair, then the output.
    views: [TextureView; 3],
    bindings: [BindGroup; 2],
    seed: BindGroup,
    uniform: UniformBuffer<TaaParams>,
    pipelines: [CachedRenderPipelineId; 2],
    next_output: usize,
    reset: bool,
    pending: bool,
    pub logical_bytes: u64,
}
impl TaaPass {
    /// Call only at startup or physical resize. A new pass starts without history.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
        depth: &TextureView,
        motion: &TextureView,
        extent: [u32; 2],
        settings: TaaSettings,
        previous: Option<Self>,
    ) -> Result<Self, String> {
        let [width, height] = extent;
        if !settings.validate() {
            return Err("Invalid source TAA settings".into());
        }
        let layout = BindGroupLayoutDescriptor::new(
            "source_taa",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<TaaParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_depth_2d(),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let seed_layout = BindGroupLayoutDescriptor::new(
            "source_taa_seed",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                (
                    (0, uniform_buffer::<TaaParams>(false)),
                    (1, texture_2d(TextureSampleType::Float { filterable: true })),
                    (5, sampler(SamplerBindingType::Filtering)),
                ),
            ),
        );
        let pipelines = previous.map(|old| old.pipelines).unwrap_or_else(|| {
            std::array::from_fn(|index| {
                let (label, group, entry, targets) = if index == 0 {
                    ("source_taa_seed", &seed_layout, "seed", 1)
                } else {
                    ("source_taa_resolve", &layout, "resolve", 2)
                };
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some(label.into()),
                    layout: vec![group.clone()],
                    vertex: VertexState {
                        shader: shader.clone(),
                        entry_point: Some("vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        shader: shader.clone(),
                        entry_point: Some(entry.into()),
                        targets: vec![
                            Some(ColorTargetState {
                                format: HDR_FORMAT,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            });
                            targets
                        ],
                        ..default()
                    }),
                    ..default()
                })
            })
        });
        let textures = std::array::from_fn(|index| {
            device.create_texture(&TextureDescriptor {
                label: Some(if index < 2 {
                    "source_taa_history"
                } else {
                    "source_taa_output"
                }),
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: HDR_FORMAT,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });
        let views: [TextureView; 3] =
            std::array::from_fn(|index| textures[index].create_view(&default()));
        let linear = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        });
        let mut uniform = UniformBuffer::from(TaaParams {
            extent: Vec4::new(
                width as f32,
                height as f32,
                1. / width as f32,
                1. / height as f32,
            ),
            jitter: Vec4::ZERO,
            blend: Vec4::new(
                settings.stationary_blending,
                settings.motion_blending,
                6000.,
                settings.sharpness,
            ),
        });
        uniform.write_buffer(device, queue);
        // Binding N reads history N and writes history 1 - N.
        let bindings = std::array::from_fn(|index| {
            device.create_bind_group(
                "source_taa",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::sequential((
                    uniform.binding().unwrap(),
                    source,
                    &views[index],
                    depth,
                    motion,
                    &linear,
                )),
            )
        });
        let seed = device.create_bind_group(
            "source_taa_seed",
            &cache.get_bind_group_layout(&seed_layout),
            &BindGroupEntries::with_indices((
                (0, uniform.binding().unwrap()),
                (1, source),
                (5, &linear),
            )),
        );
        Ok(Self {
            _textures: textures,
            views,
            bindings,
            seed,
            uniform,
            pipelines,
            next_output: 1,
            reset: true,
            pending: false,
            logical_bytes: u64::from(width) * u64::from(height) * 24,
        })
    }
    /// Histogram, bloom and Uber read this view.
    pub fn output(&self) -> &TextureView {
        &self.views[2]
    }
    /// Output history index, then 1 while a valid history exists.
    pub fn history(&self) -> [u32; 2] {
        [self.next_output as u32, u32::from(!self.reset)]
    }
    pub fn reset_history(&mut self) {
        self.reset = true;
        self.pending = false;
    }
    pub fn completed(&mut self) {
        if self.pending {
            self.next_output = 1 - self.next_output;
            self.reset = false;
            self.pending = false;
        }
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::pipeline_failure(cache, &self.pipelines)
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|id| cache.get_render_pipeline(*id).is_some())
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        jitter: Vec2,
        reset: bool,
        // DoF input instead of `output`. The history stays in the pair.
        output: Option<&TextureView>,
    ) -> Result<(), String> {
        if !jitter.is_finite() {
            return Err("Invalid source TAA jitter".into());
        }
        self.reset |= reset;
        self.uniform.get_mut().jitter = jitter.extend(0.).extend(0.);
        self.uniform.write_buffer(device, queue);
        let history = 1 - self.next_output;
        if self.reset {
            draw(
                encoder,
                cache.get_render_pipeline(self.pipelines[0]).unwrap(),
                &self.seed,
                &[&self.views[history]],
            );
        }
        draw(
            encoder,
            cache.get_render_pipeline(self.pipelines[1]).unwrap(),
            &self.bindings[history],
            &[
                output.unwrap_or(&self.views[2]),
                &self.views[self.next_output],
            ],
        );
        self.pending = true;
        Ok(())
    }
}
fn draw(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    outputs: &[&TextureView],
) {
    let attachments: [Option<RenderPassColorAttachment>; 2] = std::array::from_fn(|index| {
        outputs.get(index).map(|view| RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::NONE.into()),
                store: StoreOp::Store,
            },
        })
    });
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("source_taa"),
        color_attachments: &attachments[..outputs.len()],
        ..default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}
