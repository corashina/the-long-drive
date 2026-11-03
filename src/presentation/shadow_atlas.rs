//! Source four-cascade atlas dispatch over the persistent caster phase of each tile bit.
use super::{
    lamp_shadows::{LampShadowUniform, LampShadows, TILES, tile_bit},
    lighting_assets::LightingRenderAssets,
    materials::MaterialLoading,
    readiness::RendererStateHandle,
    render_targets::{SHADOW_FORMAT, SourceViewTargets},
    shadow_phase::{CasterSet, ShadowDrawState, SourceShadow},
    shadow_queue::ShadowQueues,
    shadow_scene::ShadowScene,
    shadow_tiles::{ShadowTileViews, TILE_BITS},
    surface_graph::SurfaceViewTargets,
    surface_views::{SourceSurfaceView, WORLD_VIEWS},
    view_frame::SourceViewFrame,
};
use crate::reference::{
    lamp_shadows::{LAMP_FACES, LampFace, LampHistory, LampPlan},
    shadow_cadence::{
        CascadeEpoch, CascadeHistory, CascadeInputs, CascadePlan, plan_cascades as plan_tiles,
    },
};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    render_phase::{TrackedRenderPass, ViewBinnedRenderPhases},
    render_resource::{binding_types::texture_depth_2d, *},
    renderer::{RenderContext, RenderDevice, ViewQuery},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::Arc;

/// The cascade plan of one view for one frame. Atlas, caster slots and collector read it.
#[derive(Component)]
pub struct ShadowCadence {
    targets: Arc<SourceViewTargets>,
    /// The static atlas of `targets`, for the tile copy.
    restore: BindGroup,
    history: CascadeHistory,
    pub plan: CascadePlan,
    /// The static lamp atlas, for the face copy. Only the world view has it.
    lamp_restore: Option<BindGroup>,
    lamp_history: LampHistory,
    /// Lamp face tiles that draw or refresh this frame. Only the world view draws lamp faces.
    pub lamp: LampPlan,
    pub frame: u32,
    /// Changes when a tile gets a different fit.
    pub revision: u64,
}
impl ShadowCadence {
    fn update(
        &mut self,
        frame: u32,
        scene: &ShadowScene,
        index: usize,
        epoch: CascadeEpoch,
        encoded: u32,
        lamps: &LampShadows,
    ) {
        if index < WORLD_VIEWS && self.lamp_restore.is_some() {
            let current = std::array::from_fn(|tile| {
                let (slot, face) = (tile / 6, tile % 6);
                let uniform = &lamps.uniforms[slot];
                (face < lamps.faces(slot)).then(|| LampFace {
                    clip_from_world: uniform.faces[face].to_cols_array().map(f32::to_bits),
                    point: uniform.point(),
                    contents: scene.lamp_contents[tile],
                })
            });
            self.lamp =
                self.lamp_history
                    .plan(encoded != self.frame, &current, &scene.lamp_animated);
        }
        self.plan = CascadePlan::ALL;
        let Some(fit) = &scene.fits[index] else {
            // The next fit renders all cascades.
            if self.history.epoch.is_some() {
                self.history = CascadeHistory::default();
                self.revision += 1;
            }
            self.frame = frame;
            return;
        };
        let current = CascadeInputs {
            epoch,
            cascades: fit.cascades,
            sun: scene.sun.to_array().map(f32::to_bits),
            contents: scene.contents[index],
            animated: std::array::from_fn(|i| scene.animated_tiles[index] >> i & 1 != 0),
            forced: encoded != self.frame,
        };
        if index < WORLD_VIEWS {
            self.plan = plan_tiles(frame, &self.history, &current);
        }
        self.revision += u64::from(self.history.commit(&self.plan, &current));
        self.frame = frame;
    }
}
/// Runs once per frame before the caster slots and the collector.
pub(crate) fn plan_cascades(
    mut commands: Commands,
    scene: Res<ShadowScene>,
    frame: Res<SourceViewFrame>,
    assets: Option<Res<LightingRenderAssets>>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    lamps: Res<LampShadows>,
    resources: Option<Res<super::surface_views::SurfaceResources>>,
    mut views: Query<(
        Entity,
        &SourceSurfaceView,
        &SurfaceViewTargets,
        Option<&ShadowDrawState>,
        Option<&mut ShadowCadence>,
    )>,
) {
    if scene.frame == 0 || scene.frame != frame.frame {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let shadows = &assets.record.lights[0].shadows;
    let split = assets.record.shadows.split_fractions;
    let epoch = CascadeEpoch {
        session: frame.session,
        scene: frame.scene,
        origin: scene.origin.map(f64::to_bits),
        settings: [
            shadows.depth_bias,
            shadows.normal_bias,
            shadows.strength,
            split[0],
            split[1],
            split[2],
        ]
        .map(f32::to_bits),
    };
    for (entity, view, targets, state, cadence) in &mut views {
        if frame.views[view.index].is_none() {
            continue;
        }
        let encoded = state.map_or(0, ShadowDrawState::encoded_frame);
        if let Some(mut cadence) = cadence.filter(|c| Arc::ptr_eq(&c.targets, &targets.0)) {
            cadence.update(frame.frame, &scene, view.index, epoch, encoded, &lamps);
            continue;
        }
        // A new atlas has no history, so all cascades render.
        let layout = cache.get_bind_group_layout(&restore_layout());
        let restore = device.create_bind_group(
            "source_shadow_restore",
            &layout,
            &BindGroupEntries::with_indices(((2, &targets.0.static_atlas.attachment),)),
        );
        let lamp_restore = resources
            .as_ref()
            .filter(|_| view.index < WORLD_VIEWS)
            .map(|r| {
                device.create_bind_group(
                    "source_lamp_shadow_restore",
                    &layout,
                    &BindGroupEntries::with_indices(
                        ((2, &r.targets.lamp_static_atlas.attachment),),
                    ),
                )
            });
        let mut cadence = ShadowCadence {
            targets: targets.0.clone(),
            restore,
            history: CascadeHistory::default(),
            plan: CascadePlan::ALL,
            lamp_restore,
            lamp_history: LampHistory::default(),
            lamp: LampPlan {
                draw: [false; LAMP_FACES],
                refresh: [false; LAMP_FACES],
            },
            frame: 0,
            revision: 0,
        };
        cadence.update(frame.frame, &scene, view.index, epoch, encoded, &lamps);
        commands.entity(entity).insert(cadence);
    }
}
fn restore_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_shadow_restore",
        &BindGroupLayoutEntries::with_indices(ShaderStages::FRAGMENT, ((2, texture_depth_2d()),)),
    )
}
/// Fullscreen triangles inside the tile viewport.
/// `clear` writes depth 0 (reversed Z). `restore` copies the static tile into the atlas tile.
#[derive(Resource)]
pub struct AtlasPipelines {
    clear: CachedRenderPipelineId,
    restore: CachedRenderPipelineId,
}
fn prepare_pipelines(
    mut commands: Commands,
    pipelines: Option<Res<AtlasPipelines>>,
    assets: Option<Res<LightingRenderAssets>>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if let Some(pipelines) = pipelines {
        let ids = [pipelines.clear, pipelines.restore];
        if let Some(error) = super::postprocessing::pipeline_failure(&cache, &ids) {
            status.fail(error);
        }
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let depth = Some(DepthStencilState {
        format: SHADOW_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(CompareFunction::Always),
        stencil: default(),
        bias: default(),
    });
    let vertex = VertexState {
        shader: assets.shaders.copy.clone(),
        entry_point: Some("vertex".into()),
        ..default()
    };
    // The copy vertex stage writes z=0. No fragment stage is necessary for depth.
    let clear = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("source_shadow_atlas_clear".into()),
        vertex: vertex.clone(),
        depth_stencil: depth.clone(),
        ..default()
    });
    let restore = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("source_shadow_atlas_restore".into()),
        layout: vec![restore_layout()],
        vertex,
        fragment: Some(FragmentState {
            shader: assets.shaders.copy.clone(),
            entry_point: Some("restore_shadow".into()),
            targets: vec![],
            ..default()
        }),
        depth_stencil: depth,
        ..default()
    });
    commands.insert_resource(AtlasPipelines { clear, restore });
}
pub fn install(app: &mut App) {
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        (
            prepare_pipelines.in_set(RenderSystems::PrepareResources),
            plan_cascades
                .in_set(RenderSystems::PrepareResources)
                .before(super::shadow_queue::write_cascades),
        ),
    );
}

/// Target atlas placement. The collector uses the same rectangle for its atlas transform.
/// Coordinates use wgpu's top-left viewport convention; source projection conversion is separate.
pub fn tile_rect(atlas_size: u32, cascade: usize) -> [u32; 4] {
    assert!(atlas_size >= 2 && atlas_size % 2 == 0 && cascade < 4);
    let side = atlas_size / 2;
    [
        (cascade as u32 & 1) * side,
        (cascade as u32 >> 1) * side,
        side,
        side,
    ]
}

/// Tiles without a draw keep their depth.
fn tile_pass<'a>(label: &'static str, attachment: &'a TextureView) -> RenderPassDescriptor<'a> {
    RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: attachment,
            depth_ops: Some(Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..default()
    }
}
pub(crate) fn render_atlas(
    world: &World,
    view: ViewQuery<(
        &SurfaceViewTargets,
        &ShadowDrawState,
        &SourceSurfaceView,
        &ShadowCadence,
        &ShadowTileViews,
    )>,
    pipelines: Option<Res<AtlasPipelines>>,
    phases: Res<ViewBinnedRenderPhases<SourceShadow>>,
    cache: Res<PipelineCache>,
    frame: Res<SourceViewFrame>,
    queues: Res<ShadowQueues>,
    status: Res<RendererStateHandle>,
    loading: Res<MaterialLoading>,
    lamps: Res<LampShadows>,
    resources: Res<super::surface_views::SurfaceResources>,
    mut context: RenderContext,
) {
    if !loading.is_ready() || status.failure().is_some() {
        return;
    }
    let entity = view.entity();
    let (targets, state, source, cadence, tiles) = view.into_inner();
    if frame.views[source.index].is_none()
        || cadence.frame != frame.frame
        || queues.frame != frame.frame
        || !queues.ready[source.index]
    {
        return;
    }
    // Tile bit N draws from phase N. Lamp faces of one slot share the phase of the slot.
    let mut tile_phases = [None; TILE_BITS];
    for bit in 0..tiles.count {
        let Some(phase) = phases.get(&tiles.retained[bit]) else {
            return;
        };
        tile_phases[bit] = Some(phase);
    }
    let phase_of = |tile: usize| tile_phases[tile_bit(tile)];
    let Some((clear, restore)) = pipelines.and_then(|p| {
        Some((
            cache.get_render_pipeline(p.clear)?,
            cache.get_render_pipeline(p.restore)?,
        ))
    }) else {
        return;
    };
    // Pipeline preparation is asynchronous. No partial atlas can be published.
    if tile_phases.iter().flatten().any(|phase| {
        phase
            .batchable_meshes
            .keys()
            .chain(phase.unbatchable_meshes.keys())
            .any(|(key, _)| cache.get_render_pipeline(key.pipeline).is_none())
    }) {
        return;
    }
    state.begin();
    let viewport = |pass: &mut TrackedRenderPass, cascade| {
        let [x, y, width, height] = tile_rect(targets.0.atlas_size, cascade);
        pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0., 1.);
        pass.set_scissor_rect(x, y, width, height);
    };
    // Static casters and GPU props draw into the static tile.
    if cadence.plan.draw.contains(&true) {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_shadow_static_atlas",
            &targets.0.static_atlas.attachment,
        ));
        for cascade in (0..4).filter(|&c| cadence.plan.draw[c]) {
            state.select(cascade, CasterSet::Static);
            viewport(&mut pass, cascade);
            pass.set_render_pipeline(clear);
            pass.draw(0..3, 0..1);
            if let Some(phase) = phase_of(cascade)
                && let Err(error) = phase.render(&mut pass, world, entity)
            {
                status.fail(format!("Source shadow atlas draw: {error:?}"));
                return;
            }
            let (attempted, issued) = super::prop_draws::draw_cascade(
                world,
                &mut pass,
                entity,
                source.index,
                cascade,
                cascade,
            );
            if attempted != issued {
                status.fail(format!(
                    "Source GPU prop caster draw skipped in cascade {cascade}: {issued}/{attempted}"
                ));
                return;
            }
        }
    }
    // The atlas tile copies the static tile, then the animated casters draw.
    if cadence.plan.refresh.contains(&true) {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_shadow_atlas",
            &targets.0.atlas.attachment,
        ));
        for cascade in (0..4).filter(|&c| cadence.plan.refresh[c]) {
            state.select(cascade, CasterSet::Animated);
            viewport(&mut pass, cascade);
            pass.set_render_pipeline(restore);
            pass.set_bind_group(0, &cadence.restore, &[]);
            pass.draw(0..3, 0..1);
            if let Some(phase) = phase_of(cascade)
                && let Err(error) = phase.render(&mut pass, world, entity)
            {
                status.fail(format!("Source shadow atlas draw: {error:?}"));
                return;
            }
        }
    }
    let lamp_draw = |tile: usize| tile >= 4 && cadence.lamp.draw[tile - 4];
    let lamp_refresh = |tile: usize| tile >= 4 && cadence.lamp.refresh[tile - 4];
    let lamp_viewport = |pass: &mut TrackedRenderPass, tile: usize| {
        let slot = (tile - 4) / 6;
        let [x, y, width, height] =
            LampShadowUniform::viewport(slot, (tile - 4) % 6, lamps.uniforms[slot].point());
        pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0., 1.);
        pass.set_scissor_rect(x, y, width, height);
    };
    // Static casters and GPU props draw into the static face.
    if (4..TILES).any(lamp_draw) {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_lamp_shadow_static_atlas",
            &resources.targets.lamp_static_atlas.attachment,
        ));
        for tile in (4..TILES).filter(|tile| lamp_draw(*tile)) {
            state.select(tile, CasterSet::Static);
            lamp_viewport(&mut pass, tile);
            pass.set_render_pipeline(clear);
            pass.draw(0..3, 0..1);
            if let Some(phase) = phase_of(tile)
                && let Err(error) = phase.render(&mut pass, world, entity)
            {
                status.fail(format!("Source lamp shadow draw: {error:?}"));
                return;
            }
            let (attempted, issued) = super::prop_draws::draw_cascade(
                world,
                &mut pass,
                entity,
                source.index,
                tile_bit(tile),
                tile,
            );
            if attempted != issued {
                status.fail(format!(
                    "Source GPU prop caster draw skipped in lamp tile {tile}: {issued}/{attempted}"
                ));
                return;
            }
        }
    }
    // The atlas face copies the static face, then the animated casters draw.
    if let Some(lamp_restore) = &cadence.lamp_restore
        && (4..TILES).any(lamp_refresh)
    {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_lamp_shadow_atlas",
            &resources.targets.lamp_atlas.attachment,
        ));
        for tile in (4..TILES).filter(|tile| lamp_refresh(*tile)) {
            state.select(tile, CasterSet::Animated);
            lamp_viewport(&mut pass, tile);
            pass.set_render_pipeline(restore);
            pass.set_bind_group(0, lamp_restore, &[]);
            pass.draw(0..3, 0..1);
            if let Some(phase) = phase_of(tile)
                && let Err(error) = phase.render(&mut pass, world, entity)
            {
                status.fail(format!("Source lamp shadow draw: {error:?}"));
                return;
            }
        }
    }
    let (epoch, attempted, issued) = state.counts();
    let instances = state.instance_counts();
    // Skipped cascades and skipped lamp faces draw no instances.
    let required: [u32; TILES] = std::array::from_fn(|tile| {
        let bit = tile_bit(tile);
        let all = queues.instances[source.index][bit];
        let animated = queues.animated[source.index][bit];
        let (draw, refresh) = if tile < 4 {
            (cadence.plan.draw[tile], cadence.plan.refresh[tile])
        } else {
            (lamp_draw(tile), lamp_refresh(tile))
        };
        (all - animated) * u32::from(draw) + animated * u32::from(refresh)
    });
    if attempted != issued || instances[0] != required || instances[1] != required {
        status.fail(format!(
            "Source shadow atlas skipped draw at epoch{epoch}: attempted{attempted:?}, issued{issued:?}, instances{instances:?}, required{required:?}"
        ));
        return;
    }
    state.encoded(frame.frame);
}
