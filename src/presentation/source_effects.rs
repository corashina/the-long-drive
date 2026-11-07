//! The world camera effect chain commits history after frame submission.
use super::{
    lighting_assets::{LightingAssets, LightingRenderAssets},
    lighting_scene::SourceSceneLighting,
    postprocessing::chain::{
        FrameMotion, MotionInputs, PostFrame, PostShaders, PostTextures, SourceAoChain,
        SourcePostChain,
    },
    readiness::{RendererStateHandle, SourceExecutionGate},
    render_targets::SourceColorTargets,
    surface_graph::{SourceSurface, SourceSurfaceStage, SurfaceViewTargets},
    surface_views::{SourceSurfaceView, SurfaceResources},
    view_frame::SourceViewFrame,
};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_asset::RenderAssets,
    render_resource::*,
    renderer::{
        RenderAdapter, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
        ViewQuery,
    },
    texture::GpuImage,
};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use std::sync::Arc;

#[derive(Resource, Clone, Default, ExtractResource)]
pub(crate) struct EffectInput {
    scene: u32,
    frame: u32,
    delta_seconds: f32,
    grading_temperature: Option<f32>,
    grade: simulation::lighting::WeatherGrade,
    dead_weight: f32,
    glasses: Option<u32>,
    binoculars: bool,
    /// AO scales. 0 is AO off.
    ao_scales: usize,
    lut: crate::display_settings::Lut,
    grayscale: bool,
    heat_haze: bool,
    cycle: (simulation::lighting::CyclePhase, f32),
    temperature: f32,
    time: f32,
    /// DoF focus distance. None is DoF off.
    focus: Option<f32>,
}
fn snapshot_input(
    frame: Res<SourceViewFrame>,
    assets: Option<Res<LightingAssets>>,
    sim: Res<super::sim_frame::SimFrameResource>,
    display: Res<crate::display_settings::DisplaySettings>,
    mut input: ResMut<EffectInput>,
) {
    if input.scene != frame.scene {
        *input = EffectInput {
            scene: frame.scene,
            ..default()
        };
    }
    input.ao_scales = match display.ambient_occlusion {
        0 => 4,
        1 => 2,
        _ => 0,
    };
    input.frame = 0;
    let Some(assets) = assets else { return };
    if !assets.loaded || frame.views[0].is_none() {
        return;
    }
    input.delta_seconds = sim.frame.state.perf.frame_seconds();
    input.grading_temperature = sim.frame.lighting.cycle.grading_temperature;
    input.grade = sim.frame.lighting.cycle.grade;
    input.dead_weight = sim.frame.state.dead_weight;
    // Source `PPVSunGlasses.enabled = !otherView()`.
    let other_view = sim.frame.state.flags & simulation::boundary::StateBlock::OTHER_VIEW != 0;
    input.glasses = (display.sunglasses && !other_view)
        .then(|| sim.frame.state.glasses())
        .flatten()
        .map(|g| g.profile);
    // The binocular `PP` volume holds the camera while the player uses binoculars.
    input.binoculars = sim.frame.state.flags & simulation::boundary::StateBlock::BINOCULARS != 0;
    let cycle = &sim.frame.lighting.cycle;
    input.lut = display.lut;
    input.grayscale = display.grayscale;
    input.heat_haze = display.heat_haze;
    input.cycle = (cycle.phase, cycle.progress);
    input.temperature = cycle.ambient_temperature;
    input.time = cycle.shader_time_seconds as f32;
    // The binocular volume overrides the `DOF` volume, `enabled` included.
    input.focus = if input.binoculars {
        Some(sim.frame.state.binocular_focus)
    } else {
        display
            .depth_of_field
            .then_some(sim.frame.state.focus_distance)
    };
    input.frame = frame.frame;
}

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) struct EffectsAudit {
    scene: u32,
    first_token: u32,
    first_before: [u32; 3],
    first_after: [u32; 3],
    history: [u32; 3],
    frame: u32,
    token: u32,
    ao_frame: u32,
    ao_token: u32,
    submitted_token: u32,
    pipelines_ready: bool,
    inputs_ready: bool,
    encoded_token: u32,
    completed_token: u32,
    extent: [u32; 2],
    chain_bytes: u64,
    ao_bytes: u64,
    total_bytes: u64,
    startup_count: u32,
    resize_count: u32,
    ao_startup_count: u32,
    ao_resize_count: u32,
    validation: u32,
    validated: bool,
    delta_seconds: f32,
}
/// The extent of TAA output and every pass after it. TAA upscales the scene to the native extent.
/// Without TAA, the post passes run at the scene extent and the canvas copy stretches.
pub(crate) fn post_extent(taa: bool, scene: [u32; 2], native: UVec2) -> [u32; 2] {
    if taa { native.to_array() } else { scene }
}
struct EffectTargets {
    colors: Arc<SourceColorTargets>,
    ao: Option<SourceAoChain>,
    chain: Option<SourcePostChain>,
}
#[derive(Resource, Default)]
pub(crate) struct SourceEffects {
    motion_revision: u32,
    targets: Option<EffectTargets>,
    audit: EffectsAudit,
}
impl SourceEffects {
    pub(crate) fn ready(&self, cache: &PipelineCache) -> bool {
        self.targets.as_ref().is_some_and(|targets| {
            targets.ao.as_ref().unwrap().ready(cache)
                && targets.chain.as_ref().unwrap().ready(cache)
        })
    }
    pub(crate) fn encoded(&self, frame: u32, token: u32) -> bool {
        self.audit.frame == frame && self.audit.token == token && self.audit.encoded_token == token
    }
    fn failure(&self, cache: &PipelineCache) -> Option<String> {
        let targets = self.targets.as_ref()?;
        targets
            .ao
            .as_ref()
            .unwrap()
            .failure(cache)
            .or_else(|| targets.chain.as_ref().unwrap().failure(cache))
    }
}
pub fn install(app: &mut App) {
    app.init_resource::<EffectInput>()
        .add_plugins(ExtractResourcePlugin::<EffectInput>::default())
        .add_systems(
            PostUpdate,
            snapshot_input.after(super::view_frame::snapshot),
        );
    app.sub_app_mut(RenderApp)
        .init_resource::<SourceEffects>()
        .add_systems(
            Render,
            prepare
                .after(RenderSystems::PrepareResources)
                .before(RenderSystems::Render),
        )
        .add_systems(RenderGraph, begin_frame.in_set(RenderGraphSystems::Begin))
        .add_systems(
            SourceSurface,
            (
                encode_ao
                    .in_set(SourceSurfaceStage::AfterOpaque)
                    .after(super::depth_resolve::resolve)
                    .run_if(super::frame_graph::surface_ready)
                    .run_if(super::shadow_queue::collector_encoded),
                encode_post
                    .in_set(SourceSurfaceStage::Postprocessing)
                    .run_if(super::frame_graph::surface_ready)
                    .run_if(super::shadow_queue::collector_encoded),
            ),
        );
}
fn shaders(assets: &LightingRenderAssets) -> PostShaders {
    let s = &assets.shaders;
    PostShaders {
        copy: s.copy.clone(),
        dof: s.dof.clone(),
        histogram: s.histogram.clone(),
        exposure: s.exposure.clone(),
        bloom: s.bloom.clone(),
        lut: s.lut.clone(),
        lut_ldr: s.lut_ldr.clone(),
        uber: s.uber.clone(),
        grain: s.grain.clone(),
        ao_depth: s.ao_depth.clone(),
        ao_render: s.ao_render.clone(),
        ao_upsample: s.ao_upsample.clone(),
        ao_composite: s.ao_composite.clone(),
        taa: s.taa.clone(),
    }
}
pub(crate) fn prepare(
    mut effects: ResMut<SourceEffects>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
    motion: Res<super::motion_vectors::SourceMotion>,
    frame: Res<SourceViewFrame>,
    input: Res<EffectInput>,
    surfaces: Option<Res<SurfaceResources>>,
    assets: Option<Res<LightingRenderAssets>>,
    images: Res<RenderAssets<GpuImage>>,
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if status.failure().is_some() {
        return;
    }
    if effects.motion_revision != frame.motion_revision {
        effects.motion_revision = frame.motion_revision;
        if let Some(targets) = effects.targets.as_mut() {
            targets.chain.as_mut().unwrap().reset_history();
        }
    }
    if effects.audit.scene != frame.scene {
        if let Some(targets) = effects.targets.as_mut() {
            targets.chain.as_mut().unwrap().recreate();
        }
        let a = &mut effects.audit;
        a.scene = frame.scene;
        a.first_token = 0;
        a.first_before = [0; 3];
        a.first_after = [0; 3];
        a.submitted_token = 0;
        a.completed_token = 0;
        a.validation = 0;
        a.validated = false;
    }
    if let Some(error) = effects.failure(&cache) {
        status.fail(error);
        return;
    }
    let (Some(surfaces), Some(assets), Some(view)) = (surfaces, assets, frame.views[0]) else {
        return;
    };
    let colors = &surfaces.targets.world;
    let scene_extent = [colors.width, colors.height];
    let extent = post_extent(antialiasing.taa(), scene_extent, view.native_extent);
    // The AO setting changes the chains without a resize.
    // With AO off, the AO chain keeps its scales.
    if effects.targets.as_ref().is_some_and(|old| {
        let chain = old.chain.as_ref().unwrap();
        Arc::ptr_eq(&old.colors, colors)
            && chain.extent == extent
            && (input.ao_scales == 0 || old.ao.as_ref().unwrap().scales() == input.ao_scales)
    }) {
        return;
    }
    let record = &assets.record.postprocessing;
    let image = |id| {
        assets
            .textures
            .get(id)
            .and_then(|handle| images.get(handle.id()))
    };
    let (Some(dirt), Some(vignette_mask), Some(c64), Some(sepia), Some(haze_noise)) = (
        image(&record.bloom.dirt),
        image(&record.binoculars.mask),
        image(&record.luts.c64),
        image(&record.luts.sepia),
        image(&record.heat_haze.noise),
    ) else {
        return;
    };
    fn lut(image: &GpuImage) -> (&TextureView, [u32; 2]) {
        let size = image.texture_descriptor.size;
        (&image.texture_view, [size.width, size.height])
    }
    if record.blue_noise.iter().any(|id| image(id).is_none()) {
        return;
    }
    let textures = PostTextures {
        noise: std::array::from_fn(|i| &image(&record.blue_noise[i]).unwrap().texture_view),
        dirt: &dirt.texture_view,
        dirt_extent: [
            dirt.texture_descriptor.size.width,
            dirt.texture_descriptor.size.height,
        ],
        vignette_mask: &vignette_mask.texture_view,
        luts: [lut(c64), lut(sepia)],
        heat_haze_noise: &haze_noise.texture_view,
        camera_depth: surfaces.targets.views[0].depth.sampled.as_ref().unwrap(),
    };
    let colors = colors.clone();
    let motion = if antialiasing.taa() {
        let Some(pass) = motion.pass(&colors) else {
            return;
        };
        Some(MotionInputs {
            taa: Some(record.taa),
            motion: pass.view(),
        })
    } else {
        None
    };
    let shaders = shaders(&assets);
    let mut previous = effects.targets.take();
    let result = (|| -> Result<EffectTargets, String> {
        let ao = SourceAoChain::new(
            &adapter,
            &device,
            &queue,
            &cache,
            &shaders,
            surfaces.targets.views[0].depth.sampled.as_ref().unwrap(),
            scene_extent,
            colors.samples,
            record,
            input.ao_scales.max(2),
            previous.as_mut().and_then(|old| old.ao.take()),
        )?;
        let layer = assets.record.cameras[2].postprocessing.as_ref().unwrap();
        let chain = SourcePostChain::new(
            &adapter,
            &device,
            &queue,
            &cache,
            &shaders,
            &textures,
            colors.resolved.sampled.as_ref().unwrap(),
            surfaces.targets.views[0].depth.sampled.as_ref().unwrap(),
            scene_extent,
            extent,
            record,
            &assets.record.0.clothing.dead_profile,
            &assets.record.0.clothing.glasses_profiles,
            layer.stop_nan,
            motion,
            previous.as_mut().and_then(|old| old.chain.take()),
        )?;
        Ok(EffectTargets {
            colors,
            ao: Some(ao),
            chain: Some(chain),
        })
    })();
    match result {
        Ok(targets) => {
            effects.audit.extent = extent;
            let chain = targets.chain.as_ref().unwrap();
            effects.audit.chain_bytes = chain.logical_bytes;
            effects.audit.startup_count = chain.startup_count;
            effects.audit.resize_count = chain.resize_count;
            let ao = targets.ao.as_ref().unwrap();
            effects.audit.ao_bytes = ao.logical_bytes;
            effects.audit.ao_startup_count = ao.startup_count;
            effects.audit.ao_resize_count = ao.resize_count;
            effects.audit.total_bytes = effects.audit.chain_bytes + effects.audit.ao_bytes;
            effects.audit.validation = 0;
            effects.audit.validated = false;
            effects.targets = Some(targets);
        }
        Err(error) => status.fail(error),
    }
}
fn begin_frame(
    mut effects: ResMut<SourceEffects>,
    frame: Res<SourceViewFrame>,
    input: Res<EffectInput>,
    cache: Res<PipelineCache>,
) {
    effects.audit.frame = frame.frame;
    effects.audit.token = frame.main_token;
    effects.audit.ao_frame = 0;
    effects.audit.ao_token = 0;
    effects.audit.encoded_token = 0;
    effects.audit.pipelines_ready = effects.ready(&cache);
    effects.audit.inputs_ready = input.frame == frame.frame;
    effects.audit.delta_seconds = input.delta_seconds;
}
pub(crate) fn encode_ao(
    view: ViewQuery<(&SourceSurfaceView, &SurfaceViewTargets)>,
    mut effects: ResMut<SourceEffects>,
    frame: Res<SourceViewFrame>,
    input: Res<EffectInput>,
    scene: Res<SourceSceneLighting>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    mut context: RenderContext,
) {
    let (view, targets) = view.into_inner();
    if view.index != 0 || status.failure().is_some() {
        return;
    }
    // With the setting off, no AO passes run. The AO composite never resolves: the transparent stage does.
    if input.ao_scales == 0 {
        effects.audit.ao_frame = frame.frame;
        effects.audit.ao_token = frame.main_token;
        return;
    }
    let (Some(snapshot), Some(scene), Some(storage)) =
        (frame.views[0].as_ref(), scene.0, effects.targets.as_mut())
    else {
        return;
    };
    match storage.ao.as_mut().unwrap().encode(
        context.command_encoder(),
        &device,
        &queue,
        &cache,
        targets.0.colors.attachment(),
        snapshot.near,
        snapshot.far,
        snapshot.implicit_projection.x_axis.x,
        scene.fog_color_density.w,
    ) {
        Ok(true) => {
            effects.audit.ao_frame = frame.frame;
            effects.audit.ao_token = frame.main_token;
        }
        Ok(false) => {}
        Err(error) => status.fail(error),
    }
}
fn encode_post(
    view: ViewQuery<&SourceSurfaceView>,
    motion: Res<super::motion_vectors::SourceMotion>,
    mut effects: ResMut<SourceEffects>,
    frame: Res<SourceViewFrame>,
    input: Res<EffectInput>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    display: Option<Res<super::frame_graph::FrameResources>>,
    mut context: RenderContext,
) {
    let index = view.into_inner().index;
    if index != 0 || input.frame != frame.frame || status.failure().is_some() {
        return;
    }
    if effects.audit.ao_frame != frame.frame {
        return;
    }
    let Some(snapshot) = frame.views[index].as_ref() else {
        return;
    };
    let Some(targets) = effects.targets.as_mut() else {
        return;
    };
    let Some(held) = display.as_ref().and_then(|display| {
        display.held_target(
            &targets.colors,
            targets.chain.as_ref().unwrap().extent,
            &status,
        )
    }) else {
        return;
    };
    // TAA reads this frame's motion target.
    if frame.main_motion.is_some()
        && motion
            .pass(&targets.colors)
            .is_none_or(|pass| !pass.encoded(frame.frame))
    {
        return;
    }
    let thumbnail = super::frame_graph::thumbnail_requested();
    let encoded = targets.chain.as_mut().unwrap().encode(
        context.command_encoder(),
        &device,
        &queue,
        &cache,
        PostFrame {
            token: frame.main_token,
            delta_seconds: input.delta_seconds,
            grading_temperature: input.grading_temperature,
            grade: input.grade,
            dead_weight: input.dead_weight,
            glasses: input.glasses,
            binoculars: input.binoculars,
            cycle: input.cycle,
            lut: input.lut,
            grayscale: input.grayscale,
            heat_haze: input.heat_haze.then_some(input.temperature),
            time: input.time,
            motion: frame.main_motion.map(|motion| FrameMotion {
                jitter: motion.jitter,
                reset: motion.reset,
            }),
            focus: input.focus,
            near: snapshot.near,
            far: snapshot.far,
        },
        held,
    );
    match encoded {
        Ok(true) => {
            effects.audit.encoded_token = frame.main_token;
            if thumbnail {
                super::frame_graph::thumbnail_encoded(frame.main_token);
            }
        }
        Ok(false) => {}
        Err(error) => status.fail(error),
    }
}
pub(crate) fn submitted(
    mut effects: ResMut<SourceEffects>,
    frame: Res<SourceViewFrame>,
    progress: Res<RendererStateHandle>,
    gate: Res<SourceExecutionGate>,
    queue: Res<RenderQueue>,
    status: Res<RendererStateHandle>,
) {
    if status.failure().is_some() {
        return;
    }
    let mut shared = progress.lock();
    let (stamp, token, presented) = (
        shared.audit.frame,
        shared.audit.token,
        shared.audit.presented,
    );
    if presented
        && stamp == frame.frame
        && token == frame.main_token
        && effects.audit.encoded_token == token
        && token > effects.audit.submitted_token
    {
        let targets = effects.targets.as_mut().unwrap();
        if targets.chain.as_ref().unwrap().pending_token() != Some(token) {
            shared
                .proof
                .fail("Source effect submission token mismatch".into());
            return;
        }
        let before = targets.chain.as_ref().unwrap().history();
        if !targets.chain.as_mut().unwrap().completed(token) {
            shared
                .proof
                .fail("Source effect history did not commit".into());
            return;
        }
        let chain = targets.chain.as_ref().unwrap();
        let after = chain.history();
        effects.audit.completed_token = chain.completed_token();
        effects.audit.history = after;
        if effects.audit.first_token == 0 {
            effects.audit.first_token = token;
            effects.audit.first_before = before;
            effects.audit.first_after = after;
        }
        effects.audit.submitted_token = token;
        if effects.audit.validation == 0 {
            effects.audit.validation = gate.watch_submission(&queue);
        }
    }
    let validation = gate.snapshot(effects.audit.validation);
    effects.audit.validated =
        validation.requested != 0 && validation.validated >= validation.requested;
    if let Some(identity) = frame.render_identity() {
        shared
            .proof
            .observe_effects(crate::render_readiness::EffectsObservation {
                identity,
                history_revision: frame.motion_revision,
                frame: effects.audit.frame,
                token: effects.audit.token,
                pipelines_ready: effects.audit.pipelines_ready,
                inputs_ready: effects.audit.inputs_ready,
                encoded_token: effects.audit.encoded_token,
                submitted_token: effects.audit.submitted_token,
                committed_token: effects.audit.completed_token,
                validation,
            });
    } else {
        shared.proof.invalidate_frame();
    }
    shared.audit.effects = effects.audit;
}
