//! Persistent fullscreen copies: the post chain input, the final display and the mirror images.
use super::fullscreen::{self, FullscreenPass, Samplers};
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::RenderDevice,
};

/// One pipeline per copy entry. Each reads the same source binding.
pub struct FrameCopy {
    pipelines: Vec<CachedRenderPipelineId>,
    binding: BindGroup,
    sampler: Sampler,
}
impl FrameCopy {
    pub fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        samplers: &Samplers,
        shader: Handle<Shader>,
        source: &TextureView,
        format: TextureFormat,
        entries: &[&'static str],
    ) -> Self {
        let pipelines = entries
            .iter()
            .map(|&entry| fullscreen::queue(cache, entry, &layout(), &shader, entry, format))
            .collect();
        let sampler = samplers.linear.clone();
        let binding = bind(device, cache, source, &sampler);
        Self {
            pipelines,
            binding,
            sampler,
        }
    }
    /// Resize changes the input binding, not the compiled pipelines.
    pub fn rebind(&mut self, device: &RenderDevice, cache: &PipelineCache, source: &TextureView) {
        self.binding = bind(device, cache, source, &self.sampler);
    }
    /// `entry` indexes the entries of `new`.
    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        output: &WgpuTextureView,
        entry: usize,
        clear: Option<LinearRgba>,
    ) -> bool {
        if !self.ready(cache) {
            return false;
        }
        let pipeline = cache.get_render_pipeline(self.pipelines[entry]).unwrap();
        fullscreen::draw(
            encoder,
            "source_frame_copy",
            pipeline,
            &self.binding,
            &[output],
            clear,
        );
        true
    }
}
impl FullscreenPass for FrameCopy {
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        &self.pipelines
    }
}

fn bind(
    device: &RenderDevice,
    cache: &PipelineCache,
    source: &TextureView,
    sampler: &Sampler,
) -> BindGroup {
    device.create_bind_group(
        "source_frame_copy",
        &cache.get_bind_group_layout(&layout()),
        &BindGroupEntries::sequential((source, sampler)),
    )
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_frame_copy",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    )
}
