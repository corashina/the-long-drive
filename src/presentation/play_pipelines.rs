//! Logs each pipeline that the cache first queues while the renderer is ready.
//! Such a pipeline compiles during play and can cause a frame spike. Add it to the prewarm.
use super::readiness::RendererStateHandle;
use crate::render_readiness::RendererStatus;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    render_resource::{PipelineCache, PipelineDescriptor},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::atomic::{AtomicU32, Ordering};

/// Pipelines queued while the renderer was ready, since the page loaded.
static PLAY_PIPELINES: AtomicU32 = AtomicU32::new(0);

pub(crate) fn count() -> u32 {
    PLAY_PIPELINES.load(Ordering::Relaxed)
}

/// Cache length at the last check.
#[derive(Resource, Default)]
struct Inspected(usize);

pub(crate) fn install(app: &mut App) {
    app.sub_app_mut(RenderApp)
        .init_resource::<Inspected>()
        .add_systems(Render, log_play_pipelines.in_set(RenderSystems::Cleanup));
}

fn log_play_pipelines(
    cache: Res<PipelineCache>,
    state: Res<RendererStateHandle>,
    mut inspected: ResMut<Inspected>,
) {
    let len = cache.pipelines().count();
    if len == inspected.0 {
        return;
    }
    let first = std::mem::replace(&mut inspected.0, len);
    // Pipelines queued while loading compile behind the loading screen.
    if state.lock().proof.status() != RendererStatus::Ready {
        return;
    }
    PLAY_PIPELINES.fetch_add((len - first) as u32, Ordering::Relaxed);
    for (index, entry) in cache.pipelines().enumerate().skip(first) {
        let (kind, label, shader, defs) = match &entry.descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(descriptor) => {
                let stage = descriptor.fragment.as_ref();
                (
                    "render",
                    &descriptor.label,
                    stage.map_or(&descriptor.vertex.shader, |fragment| &fragment.shader),
                    stage.map_or(&descriptor.vertex.shader_defs, |fragment| {
                        &fragment.shader_defs
                    }),
                )
            }
            PipelineDescriptor::ComputePipelineDescriptor(descriptor) => (
                "compute",
                &descriptor.label,
                &descriptor.shader,
                &descriptor.shader_defs,
            ),
        };
        warn!(
            "Pipeline compiled during play: {kind} #{index} {:?}, shader {:?}, defs {defs:?}",
            label.as_deref().unwrap_or("unlabeled"),
            shader.path().map(ToString::to_string),
        );
    }
}
