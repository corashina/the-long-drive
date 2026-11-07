//! AO multiplies each original scene sample before transparent drawing.
use crate::presentation::fullscreen::{self, FullscreenPass, Samplers};
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
        samplers: &Samplers,
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
        let binding = device.create_bind_group(
            "source_ao_composite",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((
                uniform.binding().unwrap(),
                depth,
                visibility,
                &samplers.point,
            )),
        );
        // Pipeline descriptors do not depend on the physical extent.
        let pipeline = previous.map_or_else(
            || {
                let mut descriptor = fullscreen::descriptor(
                    "source_ao_composite",
                    &layout,
                    &shader,
                    "composite",
                    vec![Some(ColorTargetState {
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
                );
                descriptor.multisample.count = samples;
                cache.queue_render_pipeline(descriptor)
            },
            |pass| pass.pipeline,
        );
        Self {
            uniform,
            binding,
            pipeline,
        }
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        color: &TextureView,
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
        fullscreen::draw(
            encoder,
            "source_ao_composite",
            pipeline,
            &self.binding,
            &[color],
            None,
        );
        Ok(true)
    }
}
impl FullscreenPass for AoCompositePass {
    const NAME: &'static str = "Source AO composite pipeline";
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        std::slice::from_ref(&self.pipeline)
    }
}
