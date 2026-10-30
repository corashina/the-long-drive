//! Source CopyStd draws use a stable input binding and both precompiled pass variants.
use crate::presentation::render_targets::HDR_FORMAT;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::RenderDevice,
};
pub struct CopyPass {
    binding: BindGroup,
    sampler: Sampler,
    pipelines: [CachedRenderPipelineId; 2],
}
impl CopyPass {
    /// The input and output must have equal physical extents and distinct storage.
    pub fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
    ) -> Self {
        let layout = copy_layout();
        let pipelines = ["copy", "copy_kill_nan"].map(|entry| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(entry.into()),
                layout: vec![layout.clone()],
                vertex: VertexState {
                    shader: shader.clone(),
                    entry_point: Some("vertex".into()),
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    entry_point: Some(entry.into()),
                    targets: vec![Some(ColorTargetState {
                        format: HDR_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("source_copy_linear_clamp"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        });
        let binding = device.create_bind_group(
            "source_copy",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((source, &sampler)),
        );
        Self {
            binding,
            pipelines,
            sampler,
        }
    }
    /// Resize changes the input binding, not either compiled pipeline.
    pub fn rebind(&mut self, device: &RenderDevice, cache: &PipelineCache, source: &TextureView) {
        self.binding = device.create_bind_group(
            "source_copy",
            &cache.get_bind_group_layout(&copy_layout()),
            &BindGroupEntries::sequential((source, &self.sampler)),
        );
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
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        output: &TextureView,
        kill_nan: bool,
    ) -> bool {
        if !self.ready(cache) {
            return false;
        }
        let pipeline = cache
            .get_render_pipeline(self.pipelines[usize::from(kill_nan)])
            .unwrap();
        let attachments = [Some(RenderPassColorAttachment {
            view: output,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::NONE.into()),
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_copy"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

fn copy_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_copy",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    )
}
