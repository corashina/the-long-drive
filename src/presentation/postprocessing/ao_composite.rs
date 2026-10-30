//! AO multiplies each original scene sample before transparent drawing.
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct CompositeParams {
    projection: Vec4,
}
pub struct AoCompositePass {
    uniform: UniformBuffer<CompositeParams>,
    binding: BindGroup,
    pipeline: CachedRenderPipelineId,
}
impl AoCompositePass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        depth: &TextureView,
        visibility: &TextureView,
        samples: u32,
        previous: Option<&Self>,
    ) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "source_ao_composite",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<CompositeParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    sampler(SamplerBindingType::NonFiltering),
                ),
            ),
        );
        let mut uniform = UniformBuffer::from(CompositeParams {
            projection: Vec4::ZERO,
        });
        uniform.write_buffer(device, queue);
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("source_ao_point_clamp"),
            ..default()
        });
        let binding = device.create_bind_group(
            "source_ao_composite",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((
                uniform.binding().unwrap(),
                depth,
                visibility,
                &sampler,
            )),
        );
        // Pipeline descriptors do not depend on the physical extent.
        let pipeline = previous.map_or_else(
            || {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("source_ao_composite".into()),
                    layout: vec![layout],
                    vertex: VertexState {
                        shader: shader.clone(),
                        entry_point: Some("vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        shader,
                        entry_point: Some("composite".into()),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::Rgba16Float,
                            blend: Some(BlendState {
                                color: BlendComponent {
                                    src_factor: BlendFactor::Zero,
                                    dst_factor: BlendFactor::OneMinusSrc,
                                    operation: BlendOperation::Add,
                                },
                                alpha: BlendComponent {
                                    src_factor: BlendFactor::Zero,
                                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                                    operation: BlendOperation::Add,
                                },
                            }),
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
            |pass| pass.pipeline,
        );
        Self {
            uniform,
            binding,
            pipeline,
        }
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.pipeline).is_some()
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        match cache.get_render_pipeline_state(self.pipeline) {
            CachedPipelineState::Err(
                bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
            ) => None,
            CachedPipelineState::Err(e) => Some(format!("Source AO composite pipeline: {e}")),
            _ => None,
        }
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        color: &TextureView,
        resolve: Option<&TextureView>,
        near: f32,
        far: f32,
        density: f32,
    ) -> Result<bool, String> {
        if !near.is_finite()
            || !far.is_finite()
            || !density.is_finite()
            || near <= 0.
            || far <= near
            || density < 0.
        {
            return Err("Invalid source AO composition projection or fog".into());
        }
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return Ok(false);
        };
        self.uniform.get_mut().projection = Vec4::new(near, far, far / near - 1., density);
        self.uniform.write_buffer(device, queue);
        let colors = [Some(RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: resolve.map(|view| &**view),
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_ao_composite"),
            color_attachments: &colors,
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.draw(0..3, 0..1);
        Ok(true)
    }
}
