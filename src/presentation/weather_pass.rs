//! One render pass of the world view for rain, wheel spray and the lightning bolt.
//! It begins only when one of them has something to draw. Each module keeps its pipeline and prepare.
//! No weather pipeline writes depth, so the depth attachment is read-only.
use super::{
    lightning_bolt::BoltPass,
    precipitation::PrecipitationPass,
    surface_graph::{SourceSurface, SourceSurfaceStage, SurfaceViewTargets},
    surface_views::{SourceSurfaceView, WORLD_VIEWS},
    wheel_spray::SprayPass,
};
use bevy::prelude::*;
use bevy::render::{
    RenderApp,
    render_resource::*,
    renderer::{RenderContext, ViewQuery},
};

pub fn install(app: &mut App) {
    app.sub_app_mut(RenderApp).add_systems(
        SourceSurface,
        draw.in_set(SourceSurfaceStage::Weather)
            .run_if(super::frame_graph::surface_ready)
            .run_if(super::shadow_queue::collector_encoded),
    );
}

/// Draws into the world view color after AO, in the order rain, spray, bolt.
/// The last transparent pass resolves it.
fn draw(
    view: ViewQuery<(&SourceSurfaceView, &SurfaceViewTargets)>,
    rain: Option<Res<PrecipitationPass>>,
    spray: Option<Res<SprayPass>>,
    bolt: Option<Res<BoltPass>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (source, targets) = view.into_inner();
    if source.index >= WORLD_VIEWS {
        return;
    }
    let rain = rain.as_deref().and_then(|p| Some((p, p.pipeline(&cache)?)));
    let spray = spray
        .as_deref()
        .and_then(|p| Some((p, p.pipeline(&cache)?)));
    let bolt = bolt.as_deref().and_then(|p| Some((p, p.pipeline(&cache)?)));
    if rain.is_none() && spray.is_none() && bolt.is_none() {
        return;
    }
    let targets = &targets.0;
    let depth = targets.multisample_depth.as_ref().unwrap_or(&targets.depth);
    let mut render = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("weather"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: targets.colors.attachment(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &depth.attachment,
                depth_ops: None,
                stencil_ops: None,
            }),
            ..default()
        });
    if let Some((pass, pipeline)) = rain {
        pass.encode(&mut render, pipeline);
    }
    if let Some((pass, pipeline)) = spray {
        pass.encode(&mut render, pipeline);
    }
    if let Some((pass, pipeline)) = bolt {
        pass.encode(&mut render, pipeline);
    }
}
