//! MSAA camera-depth resolve. WebGPU has no depth resolve attachment.
//! A fullscreen pass copies depth sample 0 into the single-sample camera depth.
use super::{
    readiness::RendererStateHandle,
    render_targets::{DEPTH_FORMAT, SourceViewTargets},
    surface_graph::{SourceSurface, SourceSurfaceStage, SurfaceViewTargets},
};
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::render::{
    Render, RenderSystems,
    render_resource::{binding_types::*, *},
    renderer::{RenderContext, RenderDevice, ViewQuery},
};
use bevy::shader::ShaderCacheError;

#[derive(Resource, Default)]
pub struct DepthResolve {
    pipeline: Option<CachedRenderPipelineId>,
}
impl DepthResolve {
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipeline
            .is_some_and(|id| cache.get_render_pipeline(id).is_some())
    }
}

/// Only MSAA views have this binding. Replace it with the view targets on resize.
#[derive(Component)]
pub struct DepthResolveView(BindGroup);
impl DepthResolveView {
    pub fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        targets: &SourceViewTargets,
    ) -> Option<Self> {
        let source = targets.multisample_depth.as_ref()?.sampled.as_ref()?;
        Some(Self(device.create_bind_group(
            "source_depth_resolve",
            &cache.get_bind_group_layout(&layout()),
            &BindGroupEntries::single(source),
        )))
    }
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_depth_resolve",
        &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, texture_depth_2d_multisampled()),
    )
}

pub fn install(app: &mut SubApp) {
    app.init_resource::<DepthResolve>()
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
        .add_systems(
            SourceSurface,
            resolve
                .in_set(SourceSurfaceStage::AfterOpaque)
                .run_if(super::frame_graph::surface_ready)
                .run_if(super::shadow_queue::collector_encoded),
        );
}

fn prepare(
    mut resolve: ResMut<DepthResolve>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    let Some(id) = resolve.pipeline else {
        let Some(assets) = assets else {
            return;
        };
        let shader = assets.shaders.depth_resolve.clone();
        resolve.pipeline = Some(cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("source_depth_resolve".into()),
            layout: vec![layout()],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader,
                entry_point: Some("fragment".into()),
                targets: vec![],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Always),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            ..default()
        }));
        return;
    };
    match cache.get_render_pipeline_state(id) {
        CachedPipelineState::Err(
            ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable,
        ) => {}
        CachedPipelineState::Err(error) => {
            status.fail(format!("Source depth resolve pipeline: {error}"));
        }
        _ => {}
    }
}

/// Runs first in AfterOpaque. AO, glass and soft particles read the resolved depth.
pub(crate) fn resolve(
    view: ViewQuery<(&SurfaceViewTargets, &DepthResolveView)>,
    resolve: Res<DepthResolve>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    mut context: RenderContext,
) {
    if status.failure().is_some() {
        return;
    }
    let (targets, binding) = view.into_inner();
    // The graph gate waits for this pipeline before any source pass runs.
    let Some(pipeline) = resolve
        .pipeline
        .and_then(|id| cache.get_render_pipeline(id))
    else {
        status.fail("Source depth resolve pipeline is missing".into());
        return;
    };
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("source_depth_resolve"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &targets.0.depth.attachment,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(0.),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..default()
        });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &binding.0, &[]);
    pass.draw(0..3, 0..1);
}
