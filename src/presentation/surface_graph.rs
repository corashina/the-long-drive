//! Source surface phases. AO and ordered glass capture join the explicit phase boundary.
use super::{
    materials::MaterialLoading, readiness::RendererStateHandle, render_targets::SourceViewTargets,
    source_lighting::SourceViewLighting, surface_pass::SurfacePasses,
    transparent::SourceTransparentPasses,
};
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Transparent3d};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::render::{
    render_phase::{ViewBinnedRenderPhases, ViewSortedRenderPhases},
    render_resource::*,
    renderer::{RenderContext, ViewQuery},
    view::ExtractedView,
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::Arc;

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSurface;
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SourceSurfaceStage {
    ShadowAtlas,
    Collector,
    Opaque,
    AfterOpaque,
    Transparent,
    Postprocessing,
    AfterTransparent,
}

/// Insert once when a view's targets are prepared. Replaced only on resize.
#[derive(Component)]
pub struct SurfaceViewTargets(pub Arc<SourceViewTargets>);

pub fn install(app: &mut SubApp) {
    let mut original = app
        .take_extract()
        .expect("Source surface requires Bevy extraction");
    app.set_extract(move |main, render| {
        if stop_failed_renderer(render) {
            return;
        }
        original(main, render);
        // Extract commands run with Schedules removed. Perform setup outside that scope.
        if !stop_failed_renderer(render) && render.resource::<SourcePhaseSetup>().requested {
            remove_builtin_phases(render);
            stop_failed_renderer(render);
        }
    });
    app.init_resource::<SourcePhaseSetup>()
        .add_systems(bevy::render::ExtractSchedule, validate_source_cameras)
        .add_schedule(Schedule::new(SourceSurface))
        .configure_sets(
            SourceSurface,
            (
                SourceSurfaceStage::ShadowAtlas,
                SourceSurfaceStage::Collector,
                SourceSurfaceStage::Opaque,
                SourceSurfaceStage::AfterOpaque,
                SourceSurfaceStage::Transparent,
                SourceSurfaceStage::Postprocessing,
                SourceSurfaceStage::AfterTransparent,
            )
                .chain(),
        )
        // Source views do not run Core3d. Build this view's mesh uniforms before any source pass draws.
        .add_systems(
            SourceSurface,
            bevy::pbr::early_gpu_preprocess
                .before(SourceSurfaceStage::ShadowAtlas)
                .run_if(view_active),
        )
        .add_systems(
            SourceSurface,
            (
                opaque
                    .in_set(SourceSurfaceStage::Opaque)
                    .run_if(super::frame_graph::surface_ready)
                    .run_if(super::shadow_queue::collector_encoded),
                transparent
                    .in_set(SourceSurfaceStage::Transparent)
                    .run_if(super::frame_graph::surface_ready)
                    .run_if(super::shadow_queue::collector_encoded),
            ),
        );
}

/// Idle probe faces have no frame view and draw nothing.
fn view_active(
    view: ViewQuery<&super::surface_views::SourceSurfaceView>,
    frame: Res<super::view_frame::SourceViewFrame>,
) -> bool {
    frame
        .views
        .get(view.into_inner().index)
        .is_some_and(Option::is_some)
}

fn stop_failed_renderer(render: &mut World) -> bool {
    let failed = render.resource::<RendererStateHandle>().failure().is_some();
    if failed {
        // Stop extraction and render schedules. Bevy still sends time to the main app.
        render
            .resource_mut::<bevy::render::RenderScheduleOrder>()
            .labels
            .clear();
    }
    failed
}

fn opaque(
    world: &World,
    view: ViewQuery<(
        &ExtractedView,
        &SurfaceViewTargets,
        &SourceViewLighting,
        &super::surface_views::SourceSurfaceView,
    )>,
    opaque: Res<ViewBinnedRenderPhases<Opaque3d>>,
    masked: Res<ViewBinnedRenderPhases<AlphaMask3d>>,
    pipelines: Res<SurfacePasses>,
    casters: Res<super::caster::CasterPasses>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    loading: Res<MaterialLoading>,
    mut context: RenderContext,
) {
    if !loading.is_ready()
        || !pipelines.ready(&cache, &owners)
        || !casters.ready(&cache, &owners)
        || status.failure().is_some()
    {
        return;
    }
    let entity = view.entity();
    let (view, targets, lighting, source) = view.into_inner();
    lighting.begin_draws();
    let (Some(opaque), Some(masked)) = (
        opaque.get(&view.retained_view_entity),
        masked.get(&view.retained_view_entity),
    ) else {
        return;
    };
    let targets = &targets.0;
    let colors = [Some(RenderPassColorAttachment {
        view: targets.colors.attachment(),
        depth_slice: None,
        resolve_target: targets.colors.resolve_target().map(|view| &**view),
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let depth = targets.multisample_depth.as_ref().unwrap_or(&targets.depth);
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("source_opaque"),
        color_attachments: &colors,
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: &depth.attachment,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..default()
    });
    // Static draws run as one bundle, the others directly.
    let draws = super::static_draws::StaticDraws::new(world);
    let target = super::render_bundles::BundleTarget {
        color: Some(super::render_targets::HDR_FORMAT),
        depth: super::render_targets::DEPTH_FORMAT,
        samples: targets.colors.samples,
    };
    let result = world.resource::<super::static_draws::StaticBundles>().0[source.index]
        .lock()
        .unwrap()
        .draw(world.resource(), target, &mut pass, |pass| {
            opaque
                .render_filtered(pass, world, entity, |e| draws.contains(e))
                .and_then(|_| masked.render_filtered(pass, world, entity, |e| draws.contains(e)))
        });
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            status.fail(format!("Source static opaque bundle: {error}"));
            return;
        }
    }
    .and_then(|_| opaque.render_filtered(&mut pass, world, entity, |e| !draws.contains(e)))
    .and_then(|_| masked.render_filtered(&mut pass, world, entity, |e| !draws.contains(e)));
    if let Err(error) = result {
        status.fail(format!("Source opaque draw: {error:?}"));
        return;
    }
    let (attempted, issued) = super::prop_draws::draw_camera(
        world,
        &mut pass,
        entity,
        source.index,
        targets.colors.samples,
    );
    if attempted != issued {
        status.fail(format!(
            "Source GPU prop draw skipped in view {entity:?}: {issued}/{attempted}"
        ));
    }
}
fn transparent(
    world: &World,
    view: ViewQuery<(
        &ExtractedView,
        &SurfaceViewTargets,
        &SourceTransparentPasses,
        &SourceViewLighting,
        &super::stained_glass::SourceGlassView,
        &super::surface_views::SourceSurfaceView,
    )>,
    frame: Res<super::view_frame::SourceViewFrame>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    pipelines: Res<SurfacePasses>,
    casters: Res<super::caster::CasterPasses>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    loading: Res<MaterialLoading>,
    mut context: RenderContext,
) {
    if !loading.is_ready()
        || !pipelines.ready(&cache, &owners)
        || !casters.ready(&cache, &owners)
        || status.failure().is_some()
    {
        return;
    }
    let entity = view.entity();
    let (view, targets, order, lighting, glass, source_view) = view.into_inner();
    glass.cleanup();
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return;
    };
    let targets = &targets.0;
    let colors = [Some(RenderPassColorAttachment {
        view: targets.colors.attachment(),
        depth_slice: None,
        resolve_target: targets.colors.resolve_target().map(|view| &**view),
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let depth = targets.multisample_depth.as_ref().unwrap_or(&targets.depth);
    SourceTransparentPasses::prepare_draws(world);
    let mut start = 0;
    while start < order.len() {
        if order.capture_at(start) {
            let captured = if order.named_at(start) {
                glass.capture_named(context.command_encoder(), &targets.colors)
            } else {
                glass.capture(context.command_encoder(), &targets.colors)
            };
            if let Err(error) = captured {
                status.fail(error.into());
                glass.cleanup();
                return;
            }
            start += 1;
            continue;
        }
        let end = order.next_capture(start + 1);
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("source_transparent"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &depth.attachment,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..default()
        });
        if let Err(error) = order.render_range(start..end, phase, &mut pass, world, entity) {
            status.fail(format!("Source transparent draw: {error:?}"));
        }
        drop(pass);
        start = end;
    }
    let [epoch, base_attempts, base_done, add_attempts, add_done] = lighting.draws();
    if base_attempts != base_done || add_attempts != add_done {
        status.fail(format!(
            "Source surface draw skipped in view {entity:?}, epoch {epoch}: base {base_done}/{base_attempts}, additive {add_done}/{add_attempts}, last skip {}",
            super::surface_pass::last_skip(&cache)
        ));
    }
    glass.completed(
        frame.frame,
        // Mirror views draw in the main submission. Probe faces have their own.
        if !super::surface_views::probe_view(source_view.index) {
            frame.main_token
        } else {
            frame.probe_token
        },
        order.glass_counts(),
    );
}

#[derive(Resource, Default)]
pub struct SourcePhaseSetup {
    pub depth_removed: bool,
    pub sort_removed: bool,
    requested: bool,
}

/// Validate active camera ownership before the one-time global depth replacement.
/// Figure stage cameras are the Core3d cameras. Each has its own depth texture.
fn validate_source_cameras(
    cameras: bevy::render::Extract<
        Query<
            (&Camera, &bevy::render::camera::CameraRenderGraph),
            (With<Camera3d>, Without<super::figure_stage::StageCamera>),
        >,
    >,
    mut setup: ResMut<SourcePhaseSetup>,
    status: Res<RendererStateHandle>,
) {
    let mut active = false;
    for (camera, graph) in &cameras {
        if !camera.is_active {
            continue;
        }
        active = true;
        if graph.0 != SourceSurface.intern()
            && graph.0 != super::frame_graph::SourceSkySurface.intern()
        {
            status.fail("Active 3D camera does not use the source schedule".into());
            return;
        }
    }
    if active && (!setup.depth_removed || !setup.sort_removed) {
        setup.requested = true;
    }
}

/// Source cameras own their D24S8 depth. Keep Camera3d for Bevy mesh/light preparation.
fn remove_builtin_phases(world: &mut World) {
    let (depth, sort) = world.resource_scope(
        |world, mut schedules: Mut<bevy::ecs::schedule::Schedules>| {
            let schedule = schedules
                .get_mut(bevy::render::Render)
                .expect("render schedule");
            let depth = schedule.remove_systems_in_set(
                bevy::core_pipeline::core_3d::prepare_core_3d_depth_textures,
                world,
                bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            let sort = schedule.remove_systems_in_set(
                bevy::render::render_phase::sort_phase_system::<Transparent3d>,
                world,
                bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
            );
            (depth, sort)
        },
    );
    if matches!(depth, Ok(1)) && matches!(sort, Ok(1)) {
        *world.resource_mut::<SourcePhaseSetup>() = SourcePhaseSetup {
            depth_removed: true,
            sort_removed: true,
            requested: false,
        };
    } else {
        world.resource::<RendererStateHandle>().fail(format!(
            "Source phase schedule setup: depth {depth:?}, sort {sort:?}"
        ));
    }
}
