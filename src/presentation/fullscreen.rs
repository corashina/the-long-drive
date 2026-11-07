//! Shared parts of the fullscreen triangle passes: pipelines, readiness, draws and samplers.
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::render::{render_resource::*, renderer::RenderDevice};
use std::borrow::Cow;

/// Clear each target to zero. `None` loads the target.
pub const CLEAR: Option<LinearRgba> = Some(LinearRgba::NONE);
pub const EFFECT: &str = "Source effect pipeline";

/// One linear and one point sampler for all passes. Both clamp to the edge and use mip 0.
#[derive(Resource, Clone)]
pub struct Samplers {
    pub linear: Sampler,
    pub point: Sampler,
}
impl FromWorld for Samplers {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        Self {
            linear: device.create_sampler(&SamplerDescriptor {
                label: Some("source_linear_clamp"),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..default()
            }),
            point: device.create_sampler(&SamplerDescriptor {
                label: Some("source_point_clamp"),
                ..default()
            }),
        }
    }
}

/// A pass whose readiness is the readiness of its pipelines.
pub trait FullscreenPass {
    /// The prefix of the failure text.
    const NAME: &'static str = EFFECT;
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId];
    fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipeline_ids()
            .iter()
            .all(|&id| cache.get_render_pipeline(id).is_some())
    }
    fn failure(&self, cache: &PipelineCache) -> Option<String> {
        failure(cache, self.pipeline_ids(), Self::NAME)
    }
}

/// The first compile error of `ids`, as "`name`: error". A shader that still loads is no error.
pub fn failure(
    cache: &PipelineCache,
    ids: &[CachedRenderPipelineId],
    name: &str,
) -> Option<String> {
    ids.iter()
        .find_map(|id| match cache.get_render_pipeline_state(*id) {
            CachedPipelineState::Err(
                bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
            ) => None,
            CachedPipelineState::Err(error) => Some(format!("{name}: {error}")),
            _ => None,
        })
}

/// A target without blend.
pub fn target(format: TextureFormat, write_mask: ColorWrites) -> Option<ColorTargetState> {
    Some(ColorTargetState {
        format,
        blend: None,
        write_mask,
    })
}

/// The shader `vertex` entry, then `fragment` into `targets`.
pub fn descriptor(
    label: impl Into<Cow<'static, str>>,
    layout: &BindGroupLayoutDescriptor,
    shader: &Handle<Shader>,
    fragment: &'static str,
    targets: Vec<Option<ColorTargetState>>,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some(label.into()),
        layout: vec![layout.clone()],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader: shader.clone(),
            entry_point: Some(fragment.into()),
            targets,
            ..default()
        }),
        ..default()
    }
}

/// `descriptor` into one target of `format` without blend, all channels.
pub fn queue(
    cache: &PipelineCache,
    label: impl Into<Cow<'static, str>>,
    layout: &BindGroupLayoutDescriptor,
    shader: &Handle<Shader>,
    fragment: &'static str,
    format: TextureFormat,
) -> CachedRenderPipelineId {
    cache.queue_render_pipeline(descriptor(
        label,
        layout,
        shader,
        fragment,
        vec![target(format, ColorWrites::ALL)],
    ))
}

/// One fullscreen triangle into up to 4 targets.
pub fn draw(
    encoder: &mut CommandEncoder,
    label: &'static str,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    targets: &[&WgpuTextureView],
    clear: Option<LinearRgba>,
) {
    draw_vertices(encoder, label, pipeline, group, targets, clear, 3);
}

pub fn draw_vertices(
    encoder: &mut CommandEncoder,
    label: &'static str,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    targets: &[&WgpuTextureView],
    clear: Option<LinearRgba>,
    vertices: u32,
) {
    let load = clear.map_or(LoadOp::Load, |color| LoadOp::Clear(color.into()));
    let attachments: [Option<RenderPassColorAttachment>; 4] = std::array::from_fn(|index| {
        targets.get(index).map(|&view| RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load,
                store: StoreOp::Store,
            },
        })
    });
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &attachments[..targets.len()],
        ..default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..vertices, 0..1);
}
