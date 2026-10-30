//! Persistent fullscreen copies: the final display and the mirror images.
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::RenderDevice,
};

pub struct FrameCopy {
    pipeline: CachedRenderPipelineId,
    binding: BindGroup,
    sampler: Sampler,
    layout: BindGroupLayoutDescriptor,
}
impl FrameCopy {
    pub fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
        format: TextureFormat,
        entry: &'static str,
    ) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "source_frame_copy",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("source_frame_copy".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        });
        let binding = device.create_bind_group(
            "source_frame_copy",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((source, &sampler)),
        );
        Self {
            pipeline,
            binding,
            sampler,
            layout,
        }
    }
    pub fn rebind(&mut self, device: &RenderDevice, cache: &PipelineCache, source: &TextureView) {
        self.binding = device.create_bind_group(
            "source_frame_copy",
            &cache.get_bind_group_layout(&self.layout),
            &BindGroupEntries::sequential((source, &self.sampler)),
        );
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.pipeline).is_some()
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::postprocessing::pipeline_failure(cache, &[self.pipeline])
    }
    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        output: &WgpuTextureView,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return false;
        };
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
            label: Some("source_frame_copy"),
            color_attachments: &colors,
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
