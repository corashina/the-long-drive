//! Source four-cascade atlas dispatch over the persistent caster phase of each tile bit.
//! Only the world view draws the sun atlas. Probe faces and mirrors sample it.
use super::{
    lamp_shadows::{LampShadowUniform, LampShadows, TILES, tile_bit},
    lighting_assets::LightingRenderAssets,
    materials::MaterialLoading,
    readiness::RendererStateHandle,
    render_bundles::BundleTarget,
    render_targets::{SHADOW_FORMAT, SunAtlas},
    shadow_phase::{CasterSet, ShadowBundles, ShadowDrawState, SourceShadow},
    shadow_queue::ShadowQueues,
    shadow_scene::ShadowScene,
    shadow_tiles::{ShadowTileViews, TILE_BITS},
    static_draws::StaticDraws,
    surface_views::{SourceSurfaceView, SurfaceResources, WORLD_VIEWS},
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
    render_phase::{BinnedRenderPhase, TrackedRenderPass, ViewBinnedRenderPhases},
    render_resource::{binding_types::texture_depth_2d, *},
    renderer::{RenderContext, RenderDevice, ViewQuery},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::Arc;

/// The cascade plan of the world view for one frame. Atlas, caster slots and collector read it.
#[derive(Component)]
pub struct ShadowCadence {
    atlas: Arc<SunAtlas>,
    /// The static atlas of `atlas`, for the tile copy.
    restore: BindGroup,
    history: CascadeHistory,
    pub plan: CascadePlan,
    /// The static lamp atlas, for the face copy.
    lamp_restore: BindGroup,
    lamp_history: LampHistory,
    /// Lamp face tiles that draw or refresh this frame.
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
        let current = std::array::from_fn(|tile| {
            let (slot, face) = (tile / 6, tile % 6);
            let uniform = &lamps.uniforms[slot];
            (face < lamps.faces(slot)).then(|| LampFace {
                clip_from_world: uniform.faces[face].to_cols_array().map(f32::to_bits),
                point: uniform.point(),
                contents: scene.lamp_contents[tile],
            })
        });
        self.lamp = self
            .lamp_history
            .plan(encoded != self.frame, &current, &scene.lamp_animated);
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
        self.plan = plan_tiles(frame, &self.history, &current);
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
    resources: Option<Res<SurfaceResources>>,
    mut views: Query<(
        Entity,
        &SourceSurfaceView,
        Option<&ShadowDrawState>,
        Option<&mut ShadowCadence>,
    )>,
) {
    if scene.frame == 0 || scene.frame != frame.frame {
        return;
    }
    let (Some(assets), Some(resources)) = (assets, resources) else {
        return;
    };
    let atlas = &resources.targets.sun;
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
    for (entity, view, state, cadence) in &mut views {
        if view.index >= WORLD_VIEWS || frame.views[view.index].is_none() {
            continue;
        }
        let encoded = state.map_or(0, ShadowDrawState::encoded_frame);
        if let Some(mut cadence) = cadence.filter(|c| Arc::ptr_eq(&c.atlas, atlas)) {
            cadence.update(frame.frame, &scene, view.index, epoch, encoded, &lamps);
            continue;
        }
        // A new atlas has no history, so all cascades render.
        let layout = cache.get_bind_group_layout(&restore_layout());
        let restore = device.create_bind_group(
            "source_shadow_restore",
            &layout,
            &BindGroupEntries::with_indices(((2, &atlas.static_atlas.attachment),)),
        );
        let lamp_restore = device.create_bind_group(
            "source_lamp_shadow_restore",
            &layout,
            &BindGroupEntries::with_indices(
                ((2, &resources.targets.lamp_static_atlas.attachment),),
            ),
        );
        let mut cadence = ShadowCadence {
            atlas: atlas.clone(),
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
/// Coordinates use wgpu's top-left viewport convention. The source projection conversion is separate.
pub fn tile_rect(atlas_size: u32, cascade: usize) -> [u32; 4] {
    assert!(atlas_size >= 2 && atlas_size.is_multiple_of(2) && cascade < 4);
    let side = atlas_size / 2;
    [
        (cascade as u32 & 1) * side,
        (cascade as u32 >> 1) * side,
        side,
        side,
    ]
}

/// Draws only inside `[x, y, width, height]`.
fn set_rect(pass: &mut TrackedRenderPass, [x, y, width, height]: [u32; 4]) {
    pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0., 1.);
    pass.set_scissor_rect(x, y, width, height);
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
/// The view inputs of `draw_casters`.
type Casters<'a> = (
    &'a [Option<&'a BinnedRenderPhase<SourceShadow>>; TILE_BITS],
    Entity,
    &'a StaticDraws<'a>,
    &'a ShadowDrawState,
    &'a ShadowBundles,
);
/// Draw the entity casters of one tile. Static draws run as the tile bundle, the others after it.
/// The draw commands run each frame, so `MarkDraw` counts bundled draws too.
fn draw_casters<'w>(
    world: &'w World,
    pass: &mut TrackedRenderPass<'w>,
    (phases, entity, draws, state, bundles): Casters,
    tile: usize,
    set: CasterSet,
) -> Result<(), String> {
    // Tile bit N draws from phase N. Lamp faces of one slot share the phase of the slot.
    let phase = phases[tile_bit(tile)].ok_or_else(|| format!("no caster phase for tile {tile}"))?;
    state.select(tile, set);
    let bundle = &bundles.0[tile][usize::from(set == CasterSet::Animated)];
    let target = BundleTarget {
        color: None,
        depth: SHADOW_FORMAT,
        samples: 1,
    };
    bundle
        .lock()
        .unwrap()
        .draw(world.resource(), target, pass, |pass| {
            phase.render_filtered(pass, world, entity, |e| draws.contains(e), Some)
        })?
        .and_then(|_| phase.render_filtered(pass, world, entity, |e| !draws.contains(e), Some))
        .map_err(|error| format!("{error:?}"))
}
pub(crate) fn render_atlas(
    world: &World,
    view: ViewQuery<(
        &ShadowDrawState,
        &SourceSurfaceView,
        &ShadowCadence,
        &ShadowBundles,
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
    resources: Res<SurfaceResources>,
    mut context: RenderContext,
) {
    if !loading.is_ready() || status.failure().is_some() {
        return;
    }
    let entity = view.entity();
    let (state, source, cadence, bundles, tiles) = view.into_inner();
    if frame.views[source.index].is_none()
        || cadence.frame != frame.frame
        // A resize replaces the atlas after the plan. The surfaces bind the new one.
        || !Arc::ptr_eq(&cadence.atlas, &resources.targets.sun)
        || queues.frame != frame.frame
        || !queues.ready[source.index]
    {
        return;
    }
    let mut tile_phases = [None; TILE_BITS];
    for bit in 0..TILE_BITS {
        let Some(phase) = phases.get(&tiles.retained[bit]) else {
            return;
        };
        tile_phases[bit] = Some(phase);
    }
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
    let draws = StaticDraws::new(world);
    let casters = (&tile_phases, entity, &draws, state, bundles);
    let viewport = |pass: &mut TrackedRenderPass, cascade| {
        set_rect(pass, tile_rect(cadence.atlas.size, cascade));
    };
    // Static casters and GPU props draw into the static tile.
    if cadence.plan.draw.contains(&true) {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_shadow_static_atlas",
            &cadence.atlas.static_atlas.attachment,
        ));
        for cascade in (0..4).filter(|&c| cadence.plan.draw[c]) {
            viewport(&mut pass, cascade);
            pass.set_render_pipeline(clear);
            pass.draw(0..3, 0..1);
            if let Err(error) = draw_casters(world, &mut pass, casters, cascade, CasterSet::Static)
            {
                status.fail(format!("Source shadow atlas draw: {error}"));
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
            &cadence.atlas.atlas.attachment,
        ));
        for cascade in (0..4).filter(|&c| cadence.plan.refresh[c]) {
            viewport(&mut pass, cascade);
            pass.set_render_pipeline(restore);
            pass.set_bind_group(0, &cadence.restore, &[]);
            pass.draw(0..3, 0..1);
            if let Err(error) =
                draw_casters(world, &mut pass, casters, cascade, CasterSet::Animated)
            {
                status.fail(format!("Source shadow atlas draw: {error}"));
                return;
            }
        }
    }
    let lamp_draw = |tile: usize| tile >= 4 && cadence.lamp.draw[tile - 4];
    let lamp_refresh = |tile: usize| tile >= 4 && cadence.lamp.refresh[tile - 4];
    let lamp_viewport = |pass: &mut TrackedRenderPass, tile: usize| {
        let slot = (tile - 4) / 6;
        set_rect(
            pass,
            LampShadowUniform::viewport(slot, (tile - 4) % 6, lamps.uniforms[slot].point()),
        );
    };
    // Static casters and GPU props draw into the static face.
    if (4..TILES).any(lamp_draw) {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_lamp_shadow_static_atlas",
            &resources.targets.lamp_static_atlas.attachment,
        ));
        for tile in (4..TILES).filter(|tile| lamp_draw(*tile)) {
            lamp_viewport(&mut pass, tile);
            pass.set_render_pipeline(clear);
            pass.draw(0..3, 0..1);
            if let Err(error) = draw_casters(world, &mut pass, casters, tile, CasterSet::Static) {
                status.fail(format!("Source lamp shadow draw: {error}"));
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
    if (4..TILES).any(lamp_refresh) {
        let mut pass = context.begin_tracked_render_pass(tile_pass(
            "source_lamp_shadow_atlas",
            &resources.targets.lamp_atlas.attachment,
        ));
        for tile in (4..TILES).filter(|tile| lamp_refresh(*tile)) {
            lamp_viewport(&mut pass, tile);
            pass.set_render_pipeline(restore);
            pass.set_bind_group(0, &cadence.lamp_restore, &[]);
            pass.draw(0..3, 0..1);
            if let Err(error) = draw_casters(world, &mut pass, casters, tile, CasterSet::Animated) {
                status.fail(format!("Source lamp shadow draw: {error}"));
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
