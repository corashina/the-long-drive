//! Source world camera. GPU tokens are acknowledged after the render queue submission.
use super::lighting_scene::PROBE_COUNT;
use super::surface_views::{
    MIRROR_EXTENT, MIRROR_START, PROBE_START, VIEW_COUNT, WORLD_VIEWS, mirror_view, probe_view,
};
use super::{
    frame_copy::FrameCopy,
    lighting_assets::LightingRenderAssets,
    materials::RenderingState,
    probes::{ProbeFilter, ProbePublication},
    readiness::{RendererState, RendererStateHandle, SourceExecutionGate},
    source_lighting::SourceViewCamera,
    source_sky::SkyPass,
    surface_graph::{SourceSurface, SourceSurfaceStage},
    surface_views::{SourceSurfaceView, SurfaceResources},
    view_frame::{SourceSubmissionAcks, SourceViewFrame},
};
use crate::presentation_logic::view_gate::{FrameFacts, ViewFacts, view_may_draw};
use crate::reference::reflections::ProbeJob;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::render::camera::CameraRenderGraph;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    extract_component::ExtractComponent,
    render_phase::{
        DrawFunctions, DrawFunctionsInternal, PhaseItem, SortedPhaseItem, TrackedRenderPass,
        ViewSortedRenderPhases,
    },
    render_resource::*,
    renderer::{
        RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue, ViewQuery,
    },
    sync_world::MainEntity,
    view::{ExtractedView, ViewTarget},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSkySurface;
#[derive(Component, Clone, Copy)]
pub struct SourceSkyCamera;
impl bevy::render::sync_component::SyncComponent for SourceSkyCamera {
    type Target = Self;
}
impl ExtractComponent for SourceSkyCamera {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(_: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(Self)
    }
}
#[derive(Component, Clone, Copy)]
pub struct SourceSkyExtra(pub u32);
impl bevy::render::sync_component::SyncComponent for SourceSkyExtra {
    type Target = Self;
}
impl ExtractComponent for SourceSkyExtra {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(
        item: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>,
    ) -> Option<Self> {
        Some(*item)
    }
}
#[derive(Resource, Default)]
struct SkyExtras(bevy::ecs::entity::EntityHashMap<(Entity, u32)>);
#[derive(Default, serde::Serialize)]
pub(crate) struct FrameAudit {
    #[serde(skip)]
    identity: Option<crate::render_readiness::RenderIdentity>,
    scene: u32,
    extent: [u32; 2],
    first_token: u32,
    pub(crate) frame: u32,
    mask: u16,
    sky: bool,
    pub(crate) presented: bool,
    pub(crate) token: u32,
    validation: u32,
    validated: bool,
    views: [ViewAudit; VIEW_COUNT],
    passes: PassReadiness,
    cpu_boundary: CpuBoundary,
    gpu_boundary: GpuBoundary,
    probes: [ProbeAudit; PROBE_COUNT],
    pub(crate) effects: super::source_effects::EffectsAudit,
}
#[derive(Resource)]
pub(crate) struct FrameResources {
    colors: Arc<super::render_targets::SourceColorTargets>,
    _sky_depth: Texture,
    sky_depth: TextureView,
    display_format: TextureFormat,
    held: HeldImage,
}
impl FrameResources {
    /// The post chain writes its last pass into the held image.
    /// A frame without the sky does not present, so it keeps the last image.
    pub(crate) fn held_target(
        &self,
        colors: &Arc<super::render_targets::SourceColorTargets>,
        extent: [u32; 2],
        progress: &RendererStateHandle,
    ) -> Option<&TextureView> {
        (Arc::ptr_eq(&self.colors, colors)
            && self.held.extent() == extent
            && progress.lock().audit.sky)
            .then_some(&self.held.view)
    }
}
/// The last presented image. The canvas gets a new cleared texture each frame.
/// A frame that presents no world image shows this image again.
/// Rows are bottom-up, as source textures. It has the post chain extent.
struct HeldImage {
    texture: Texture,
    view: TextureView,
    /// Stretches `held` over the canvas.
    copy: FrameCopy,
    filled: AtomicBool,
}
impl HeldImage {
    fn texture(device: &RenderDevice, [width, height]: [u32; 2]) -> (Texture, TextureView) {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_held_display"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: super::render_targets::HELD_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        (texture, view)
    }
    fn extent(&self) -> [u32; 2] {
        [self.texture.width(), self.texture.height()]
    }
    /// A new size starts empty. A new target of the same size keeps the last image.
    fn fit(&mut self, device: &RenderDevice, cache: &PipelineCache, extent: [u32; 2]) {
        if self.extent() == extent {
            return;
        }
        (self.texture, self.view) = Self::texture(device, extent);
        self.copy.rebind(device, cache, &self.view);
        self.filled.store(false, Ordering::Relaxed);
    }
    fn show(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        output: &WgpuTextureView,
    ) -> bool {
        self.copy.encode(encoder, cache, output)
    }
}

#[derive(Clone, Copy, Default, serde::Serialize)]
struct ProbeAudit {
    enabled: bool,
    checks: u32,
    distance_squared: f32,
    token: u32,
    job: u8,
    face_mask: u8,
    mip_mask: u8,
    pipelines_ready: bool,
    filter_validation: u32,
    publication_validation: u32,
    published: bool,
    generation: u32,
    submitted_token: u32,
    published_token: u32,
}
#[derive(Resource)]
pub(crate) struct ProbeFrame {
    scene: u32,
    probes: [ProbeState; PROBE_COUNT],
}
struct ProbeState {
    filter: ProbeFilter,
    publication: ProbePublication,
    encoded_token: Option<u32>,
    publication_encoded: Option<u32>,
    filter_validation: u32,
    publication_validation: u32,
    published_validation: u32,
    publication_token: u32,
    submitted_token: u32,
    published_token: u32,
    sky_frames: [AtomicU32; 6],
}
impl ProbeState {
    fn accepts(&self, token: u32) -> bool {
        token != 0
            && token > self.submitted_token
            && !self.publication.waiting()
            && self.publication_validation == 0
    }
}

pub struct SourceFramePlugin;
impl Plugin for SourceFramePlugin {
    fn build(&self, app: &mut App) {
        let progress = app.world().resource::<RendererStateHandle>().clone();
        let gate = app.world().resource::<SourceExecutionGate>().clone();
        let _ = SOURCE_DIAGNOSTICS.set(SourceDiagnostics {
            state: progress.clone(),
            gate,
        });
        app.insert_resource(progress.clone())
            .add_plugins((
                bevy::render::extract_component::ExtractComponentPlugin::<SourceSkyCamera>::default(
                ),
                bevy::render::extract_component::ExtractComponentPlugin::<SourceSkyExtra>::default(
                ),
            ))
            .add_systems(
                Update,
                bind_sky_extras.after(super::materials::bind_materials),
            );
        app.add_systems(Startup, cpu_clustering);
        app.add_systems(
            Update,
            cpu_boundaries
                .after(super::materials::bind_authored_materials)
                .run_if(audit_watched),
        );
        super::view_frame::install(app);
        super::mirrors::install(app);
        super::shadow_queue::install(app);
        super::shadow_tiles::install(app);
        super::lamp_shadows::install(app);
        super::caster_material::install(app);
        super::shadow_atlas::install(app);
        super::shadow_collector::install(app);
        super::antialiasing::install(app);
        super::source_effects::install(app);
        super::precipitation::install(app);
        super::lightning_bolt::install(app);
        super::weather_shafts::install(app);
        super::wheel_spray::install(app);
        super::weather_pass::install(app);
        super::motion_vectors::install(app);
        super::gpu_timing::install(app);
        super::play_pipelines::install(app);
        super::prewarm_list::install(app);
        let render = app.sub_app_mut(RenderApp);
        render
            .insert_resource(progress)
            .init_resource::<SkyExtras>()
            .add_systems(
                Render,
                (pass_readiness, gpu_boundaries.run_if(audit_watched))
                    .in_set(RenderSystems::Cleanup),
            )
            .add_systems(Render, release_canvas_texture.after(RenderSystems::Render))
            .add_systems(
                Render,
                (
                    update_sky_extras,
                    prepare_frame,
                    prepare_probe,
                    prepare_mirror_copy,
                )
                    .after(RenderSystems::PrepareResources)
                    .before(RenderSystems::Render),
            )
            .add_systems(
                Render,
                sort_sky
                    .after(RenderSystems::PhaseSort)
                    .before(super::transparent::prepare_pass_order),
            )
            .add_schedule(Schedule::new(SourceSkySurface))
            .add_systems(
                SourceSkySurface,
                bevy::pbr::early_gpu_preprocess.before(render_sky),
            )
            .add_systems(SourceSkySurface, render_sky)
            .add_systems(
                SourceSurface,
                (
                    clear_capture
                        .before(SourceSurfaceStage::Opaque)
                        .run_if(surface_ready)
                        .run_if(super::shadow_queue::graph_ready),
                    render_capture_sky
                        .in_set(SourceSurfaceStage::AfterOpaque)
                        .after(super::depth_resolve::resolve)
                        .run_if(surface_ready)
                        .run_if(super::shadow_queue::collector_encoded),
                    super::shadow_atlas::render_atlas
                        .in_set(SourceSurfaceStage::ShadowAtlas)
                        .run_if(surface_ready)
                        .run_if(super::shadow_queue::graph_ready),
                    finish_surface
                        .in_set(SourceSurfaceStage::AfterTransparent)
                        .run_if(surface_ready),
                    hold_display
                        .in_set(SourceSurfaceStage::AfterTransparent)
                        .after(finish_surface),
                ),
            )
            .add_systems(RenderGraph, begin_frame.in_set(RenderGraphSystems::Begin))
            .add_systems(
                RenderGraph,
                filter_probe
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            )
            .add_systems(
                RenderGraph,
                (submitted, super::source_effects::submitted)
                    .chain()
                    .in_set(RenderGraphSystems::Finish),
            );
    }
}

/// A WebGPU canvas texture expires when the browser task ends.
/// Bevy keeps a texture that no view presented, so release it before the next frame.
fn release_canvas_texture(mut windows: ResMut<bevy::render::view::ExtractedWindows>) {
    for window in windows.values_mut() {
        window.swap_chain_texture_view = None;
        window.swap_chain_texture = None;
    }
}
pub fn spawn_cameras(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    reference: &crate::reference::ReferenceManifest,
    antialiasing: super::antialiasing::Antialiasing,
) {
    // Each simulation frame sets the pose, field of view and far plane of the player views
    // (`update_camera`, `sync_sky_camera`). Until the first frame they are 0.
    let (fov_degrees, far) = (0f32, 0f32);
    let record = &reference.render_lighting;
    let sky = &record.cameras[0];
    commands.spawn((
        SourceSkyCamera,
        Camera3d::default(),
        // Source passes draw directly. GPU preprocessing builds mesh uniforms only.
        bevy::render::view::NoIndirectDrawing,
        bevy::light::cluster::ClusterConfig::None,
        bevy::camera::Hdr,
        Camera {
            order: sky.order as isize,
            ..default()
        },
        CameraRenderGraph::new(SourceSkySurface),
        RenderLayers::layer(16),
        Projection::Perspective(PerspectiveProjection {
            fov: fov_degrees.to_radians(),
            near: sky.near,
            far: sky.far,
            ..default()
        }),
        Transform::default(),
        antialiasing.msaa(),
        bevy::core_pipeline::tonemapping::Tonemapping::None,
    ));
    let mut image = Image::new_fill(
        Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let capture = images.add(image);
    // Mirror views draw into their own scratch. Their slot images take a copy.
    let mut image = Image::new_fill(
        Extent3d {
            width: MIRROR_EXTENT,
            height: MIRROR_EXTENT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let mirror_capture = images.add(image);
    commands.insert_resource(super::mirrors::MirrorFrame {
        views: [None; super::surface_views::MIRROR_COUNT],
        images: super::mirrors::slot_images(images),
    });
    // Probe faces and mirrors (order 10 + index) sample the sun atlas of the world view.
    assert!(
        (record.cameras[2].order as isize) < 10 + PROBE_START as isize,
        "The world view must render before the probe faces and mirrors"
    );
    for index in 0..VIEW_COUNT {
        let (near, far, fov, mask, order, samples) = if index < WORLD_VIEWS {
            let world = super::camera::world_camera_settings(fov_degrees, far);
            (
                world.near,
                world.far,
                world.fov_degrees,
                world.mask,
                record.cameras[2].order as isize,
                antialiasing.msaa(),
            )
        } else if probe_view(index) {
            let p = &record.probes[0];
            (p.near, p.far, 90., p.mask, 10 + index as isize, Msaa::Off)
        } else {
            // The mirror selection sets the pose, projection, mask and activity each frame.
            (0.1, 1., 60., 0, 10 + index as isize, Msaa::Off)
        };
        let mut entity = commands.spawn((
            Camera3d::default(),
            bevy::render::view::NoIndirectDrawing,
            bevy::light::cluster::ClusterConfig::Single,
            bevy::camera::Hdr,
            Camera {
                order,
                is_active: !mirror_view(index),
                ..default()
            },
            CameraRenderGraph::new(SourceSurface),
            (0..32)
                .filter(|layer| mask & (1u32 << layer) != 0)
                .collect::<RenderLayers>(),
            Projection::Perspective(if index < WORLD_VIEWS {
                super::camera_projection::player_perspective(fov, 1., far)
            } else {
                PerspectiveProjection {
                    fov: fov.to_radians(),
                    near,
                    far,
                    ..default()
                }
            }),
            Transform::default(),
            samples,
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            SourceSurfaceView {
                index,
                near,
                far,
                clip_from_world: Mat4::IDENTITY,
            },
        ));
        if index == 0 {
            entity.insert(super::camera::DriveCamera);
        }
        if probe_view(index) {
            entity.insert(RenderTarget::Image(capture.clone().into()));
        } else if mirror_view(index) {
            entity.insert(RenderTarget::Image(mirror_capture.clone().into()));
        }
    }
}

pub(crate) fn sync_sky_camera(
    sim: Res<super::sim_frame::SimFrameResource>,
    main: Query<&Transform, (With<super::camera::DriveCamera>, Without<SourceSkyCamera>)>,
    mut sky: Query<
        (&mut Transform, &mut Projection),
        (With<SourceSkyCamera>, Without<super::camera::DriveCamera>),
    >,
) {
    let Ok(main) = main.single() else { return };
    let fov = sim.frame.state.fov_degrees;
    for (mut transform, mut projection) in &mut sky {
        *transform = Transform::from_rotation(main.rotation);
        // The sky camera keeps its own far plane.
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = fov.to_radians();
        }
    }
}
fn bind_sky_extras(
    mut commands: Commands,
    assets: Res<Assets<super::sky::SkyMaterial>>,
    meshes: Query<(Entity, &MeshMaterial3d<super::sky::SkyMaterial>), Without<SourceSkyExtra>>,
) {
    for (entity, material) in &meshes {
        let Some(material) = assets.get(&material.0) else {
            continue;
        };
        match material.settings.kind.x {
            3 | 4 => {
                commands.entity(entity).insert((
                    SourceSkyExtra(material.settings.kind.x),
                    RenderLayers::layer(16),
                ));
            }
            _ => {
                commands.entity(entity).despawn();
            }
        }
    }
}
fn update_sky_extras(
    changed: Query<
        (Entity, &MainEntity, &SourceSkyExtra),
        Or<(Changed<SourceSkyExtra>, Changed<MainEntity>)>,
    >,
    mut removed: RemovedComponents<SourceSkyExtra>,
    mut extras: ResMut<SkyExtras>,
) {
    for entity in removed.read() {
        extras.0.retain(|_, (render, _)| *render != entity);
    }
    for (render, main, extra) in &changed {
        extras.0.insert(main.id(), (render, extra.0));
    }
}
fn sort_sky(
    views: Query<&ExtractedView, With<SourceSkyCamera>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
) {
    for view in &views {
        if let Some(phase) = phases.get_mut(&view.retained_view_entity) {
            Transparent3d::sort(&mut phase.items);
        }
    }
}
fn prepare_frame(
    mut commands: Commands,
    resources: Option<Res<SurfaceResources>>,
    assets: Option<Res<LightingRenderAssets>>,
    existing: Option<ResMut<FrameResources>>,
    mut sky: Option<ResMut<SkyPass>>,
    mut clouds: Option<ResMut<super::clouds::CloudPass>>,
    scene: Res<super::lighting_scene::SourceSceneLighting>,
    views: Query<(&SourceSurfaceView, &ViewTarget)>,
    sky_views: Query<&ExtractedView, With<SourceSkyCamera>>,
    frame: Res<SourceViewFrame>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    let (Some(resources), Some(assets), Some(scene)) = (resources, assets, scene.0) else {
        return;
    };
    let (Some(sky_clip), Some(skybox_clip)) =
        (frame.sky_clip_from_world, frame.skybox_clip_from_world)
    else {
        return;
    };
    if let Some(sky) = sky.as_mut()
        && sky_views.single().is_ok()
    {
        sky.write_live(
            0,
            &scene,
            Vec3::ZERO,
            assets.record.cameras[0].far,
            skybox_clip,
        );
    }
    if let (Some(sky), Some(ProbeJob::Face(face))) = (sky.as_mut(), frame.probe_job)
        && let Some(snapshot) = frame.views[PROBE_START + usize::from(face)]
    {
        let projection =
            Mat4::from_cols_array_2d(&crate::reference::sky::source_sky_gpu_projection(
                snapshot.implicit_projection.to_cols_array_2d(),
                snapshot.near,
            ));
        sky.write_live(
            1 + usize::from(face),
            &scene,
            scene.probe_position[frame.probe_index],
            snapshot.far,
            projection * snapshot.view_from_world,
        );
    }
    if let Some(sky) = sky.as_mut() {
        for index in MIRROR_START..VIEW_COUNT {
            let Some(snapshot) = frame.views[index] else {
                continue;
            };
            let projection =
                Mat4::from_cols_array_2d(&crate::reference::sky::source_sky_gpu_projection(
                    snapshot.implicit_projection.to_cols_array_2d(),
                    snapshot.near,
                ));
            sky.write_live(
                index,
                &scene,
                snapshot.view_from_world.inverse().w_axis.truncate(),
                snapshot.far,
                projection * snapshot.view_from_world,
            );
        }
        sky.upload(&queue);
    }
    if let Some(clouds) = clouds.as_mut()
        && sky_views.single().is_ok()
    {
        clouds.write_live(Vec3::ZERO, sky_clip, &scene, &queue);
    }
    let Some((_, target)) = views.iter().find(|(source, _)| source.index == 0) else {
        return;
    };
    let Some(format) = target.out_texture_view_format() else {
        return;
    };
    if !format.is_srgb() {
        status.fail("Source final display requires an sRGB view".into());
        return;
    }
    let colors = resources.targets.world.clone();
    let extent = frame.views[0].map(|view| {
        super::source_effects::post_extent(
            antialiasing.taa(),
            [colors.width, colors.height],
            view.native_extent,
        )
    });
    if let Some(mut existing) = existing {
        if !Arc::ptr_eq(&existing.colors, &colors) {
            let texture = sky_depth(&device, &colors);
            existing.sky_depth = texture.create_view(&TextureViewDescriptor::default());
            existing._sky_depth = texture;
            existing.colors = colors;
        }
        if let Some(extent) = extent {
            existing.held.fit(&device, &cache, extent);
        }
        if existing.display_format != format {
            status.fail("Source display format changed".into());
        }
        if let Some(error) = existing.held.copy.failure(&cache) {
            status.fail(error);
        }
        return;
    }
    let Some(extent) = extent else {
        return;
    };
    let texture = sky_depth(&device, &colors);
    let depth = texture.create_view(&TextureViewDescriptor::default());
    let (held_texture, held_view) = HeldImage::texture(&device, extent);
    let held = HeldImage {
        copy: FrameCopy::new(
            &device,
            &cache,
            assets.shaders.copy.clone(),
            &held_view,
            format,
            "present",
        ),
        texture: held_texture,
        view: held_view,
        filled: AtomicBool::new(false),
    };
    commands.insert_resource(FrameResources {
        colors,
        _sky_depth: texture,
        sky_depth: depth,
        display_format: format,
        held,
    });
}
/// Copies the mirror scratch color into a slot image. Rows stay bottom-up, as source textures.
#[derive(Resource)]
pub(crate) struct MirrorCopy {
    copy: FrameCopy,
}
fn prepare_mirror_copy(
    mut commands: Commands,
    resources: Option<Res<SurfaceResources>>,
    assets: Option<Res<LightingRenderAssets>>,
    existing: Option<Res<MirrorCopy>>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if let Some(existing) = existing {
        if let Some(error) = existing.copy.failure(&cache) {
            status.fail(error);
        }
        return;
    }
    let (Some(resources), Some(assets)) = (resources, assets) else {
        return;
    };
    // The mirror scratch keeps its storage across window resizes.
    commands.insert_resource(MirrorCopy {
        copy: FrameCopy::new(
            &device,
            &cache,
            assets.shaders.copy.clone(),
            resources.targets.mirror.resolved.sampled.as_ref().unwrap(),
            TextureFormat::Rgba8Unorm,
            "copy",
        ),
    });
}
fn sky_depth(device: &RenderDevice, colors: &super::render_targets::SourceColorTargets) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some("source_sky_depth"),
        size: Extent3d {
            width: colors.width,
            height: colors.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: colors.samples,
        dimension: TextureDimension::D2,
        format: super::render_targets::DEPTH_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
/// Loading draws one frame of four. A skipped frame draws no view and no sky.
fn frame_allowed(frame: &SourceViewFrame, state: &RendererState) -> bool {
    frame
        .render_identity()
        .is_none_or(|key| state.proof.allows_frame(key, frame.frame))
}

/// `DrawMesh` skips every draw until both direct and frustum preprocess pipelines exist.
fn preprocess_ready(pipelines: &bevy::pbr::PreprocessPipelines, cache: &PipelineCache) -> bool {
    [
        &pipelines.direct_preprocess,
        &pipelines.gpu_frustum_culling_preprocess,
    ]
    .iter()
    .all(|p| {
        p.pipeline_id
            .is_some_and(|id| cache.get_compute_pipeline(id).is_some())
    })
}

/// Run condition of every surface view pass.
pub(crate) fn surface_ready(
    view: ViewQuery<&SourceSurfaceView>,
    frame: Res<SourceViewFrame>,
    progress: Res<RendererStateHandle>,
    camera: Res<SourceViewCamera>,
    resources: Option<Res<FrameResources>>,
    probe: Option<Res<ProbeFrame>>,
    cache: Res<PipelineCache>,
    effects: Option<Res<super::source_effects::SourceEffects>>,
    preprocess: Res<bevy::pbr::PreprocessPipelines>,
    mirrors: Option<Res<MirrorCopy>>,
) -> bool {
    let index = view.into_inner().index;
    if index >= VIEW_COUNT {
        return false;
    }
    let state = progress.lock();
    let facts = FrameFacts {
        view: frame.views[index].is_some(),
        allowed: frame_allowed(&frame, &state),
        camera: camera.ready(),
        display: resources.is_some_and(|r| r.held.copy.ready(&cache)),
        effects: effects.is_some_and(|e| e.ready(&cache)),
        preprocess: preprocess_ready(&preprocess, &cache),
        sky: state.audit.sky,
        presented: state.audit.presented,
    };
    let kind = if index < WORLD_VIEWS {
        ViewFacts::World
    } else if mirror_view(index) {
        ViewFacts::Mirror {
            copy: mirrors.is_some_and(|m| m.copy.ready(&cache)),
        }
    } else if probe_view(index) {
        ViewFacts::Probe {
            face: frame.probe_job == Some(ProbeJob::Face((index - PROBE_START) as u8)),
            probe: probe.is_some_and(|p| {
                let probe = &p.probes[frame.probe_index];
                probe.accepts(frame.probe_token) && probe.filter.ready(&cache)
            }),
        }
    } else {
        return false;
    };
    view_may_draw(facts, kind)
}

/// A frame without a presented world image shows the held image again.
/// Skipped loading frames, late pipelines and new targets then do not show a black canvas.
fn hold_display(
    view: ViewQuery<(&SourceSurfaceView, &ViewTarget)>,
    progress: Res<RendererStateHandle>,
    resources: Option<Res<FrameResources>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (source, target) = view.into_inner();
    if source.index != 0 || progress.lock().audit.presented {
        return;
    }
    let Some(resources) = resources else { return };
    if !resources.held.filled.load(Ordering::Relaxed) {
        return;
    }
    let Some(output) = target.out_texture_color_attachment(None) else {
        return;
    };
    resources
        .held
        .show(context.command_encoder(), &cache, output.view);
}
fn begin_frame(frame: Res<SourceViewFrame>, progress: Res<RendererStateHandle>) {
    let mut state = progress.lock();
    if state.audit.scene != frame.scene {
        let passes = state.audit.passes;
        let cpu = state.audit.cpu_boundary;
        let gpu = state.audit.gpu_boundary;
        state.audit = FrameAudit {
            scene: frame.scene,
            passes,
            cpu_boundary: cpu,
            gpu_boundary: gpu,
            ..default()
        };
    }
    if let Some(view) = frame.views[0] {
        let extent = view.extent.to_array();
        if state.audit.extent != extent {
            state.audit.extent = extent;
            state.audit.validation = 0;
            state.audit.validated = false;
        }
    }
    let identity = frame.render_identity();
    if state.audit.identity != identity {
        state.audit.identity = identity;
        state.audit.validation = 0;
        state.audit.validated = false;
    }
    state.audit.frame = frame.frame;
    state.audit.mask = 0;
    state.audit.sky = false;
    state.audit.presented = false;
}
fn draw_extras<'w>(
    kind: u32,
    phase: &'w bevy::render::render_phase::SortedRenderPhase<Transparent3d>,
    world: &'w World,
    view: Entity,
    pass: &mut TrackedRenderPass<'w>,
    functions: &mut DrawFunctionsInternal<Transparent3d>,
    extras: &SkyExtras,
) -> Result<(), String> {
    for item in phase.items.values() {
        if extras
            .0
            .get(&item.main_entity().id())
            .is_some_and(|(_, k)| *k == kind)
        {
            functions
                .get_mut(item.draw_function())
                .ok_or("Source sky draw function missing")?
                .draw(world, pass, view, item)
                .map_err(|e| format!("Source sky extra: {e:?}"))?;
        }
    }
    Ok(())
}
fn render_sky(
    world: &World,
    view: ViewQuery<&ExtractedView>,
    resources: Option<Res<FrameResources>>,
    sky: Option<Res<SkyPass>>,
    clouds: Option<Res<super::clouds::CloudPass>>,
    extras: Res<SkyExtras>,
    phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    allocator: Res<bevy::render::mesh::allocator::MeshAllocator>,
    cache: Res<PipelineCache>,
    frame: Res<SourceViewFrame>,
    loading: Res<super::materials::MaterialLoading>,
    status: Res<RendererStateHandle>,
    progress: Res<RendererStateHandle>,
    preprocess: Res<bevy::pbr::PreprocessPipelines>,
    mut context: RenderContext,
) {
    let entity = view.entity();
    let view = view.into_inner();
    if frame.sky_clip_from_world.is_none()
        || !frame.required_views_ready()
        || !preprocess_ready(&preprocess, &cache)
        || !frame_allowed(&frame, &progress.lock())
    {
        return;
    }
    let (Some(resources), Some(sky), Some(clouds)) = (resources, sky, clouds) else {
        return;
    };
    if !sky.ready(&cache)
        || !clouds.ready(&cache)
        || !resources.held.copy.ready(&cache)
        || !loading.is_ready()
    {
        return;
    }
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return;
    };
    // One pass for clear, sky, extras and clouds. The last transparent pass resolves it.
    let colors = [Some(RenderPassColorAttachment {
        view: resources.colors.attachment(),
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::BLACK.into()),
            store: StoreOp::Store,
        },
    })];
    let mut functions = world.resource::<DrawFunctions<Transparent3d>>().write();
    functions.prepare(world);
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("source_sky"),
        color_attachments: &colors,
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: &resources.sky_depth,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(0.),
                store: StoreOp::Discard,
            }),
            stencil_ops: Some(Operations {
                load: LoadOp::Clear(0),
                store: StoreOp::Discard,
            }),
        }),
        ..default()
    });
    if !sky.draw(0, &mut pass, &cache) {
        return;
    }
    if let Err(error) = draw_extras(3, phase, world, entity, &mut pass, &mut functions, &extras) {
        status.fail(error);
        return;
    }
    if !clouds.draw(&mut pass, &cache, &allocator) {
        return;
    }
    if let Err(error) = draw_extras(4, phase, world, entity, &mut pass, &mut functions, &extras) {
        status.fail(error);
        return;
    }
    drop(pass);
    progress.lock().audit.sky = true;
}
fn finish_surface(
    view: ViewQuery<(
        &SourceSurfaceView,
        &ViewTarget,
        &super::source_lighting::SourceViewLighting,
        Option<&super::shadow_phase::ShadowDrawState>,
        &super::surface_graph::SurfaceViewTargets,
        &super::stained_glass::SourceGlassView,
    )>,
    frame: Res<SourceViewFrame>,
    progress: Res<RendererStateHandle>,
    resources: Res<FrameResources>,
    surfaces: Res<SurfaceResources>,
    mut probe: Option<ResMut<ProbeFrame>>,
    cache: Res<PipelineCache>,
    effects: Res<super::source_effects::SourceEffects>,
    status: Res<RendererStateHandle>,
    props: Res<super::prop_draws::PropDraws>,
    sun: Res<super::shadow_collector::SunShadow>,
    mirrors: Option<Res<MirrorCopy>>,
    mirror_frame: Res<super::mirrors::MirrorFrame>,
    gpu_images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    mut context: RenderContext,
) {
    let (source, target, lighting, shadow, targets, glass) = view.into_inner();
    if sun.encoded_frame() != frame.frame || glass.audit().frame != frame.frame {
        return;
    }
    if status.failure().is_some() {
        return;
    }
    let mut state = progress.lock();
    if audit_watched() {
        // Only world views draw shadows. The other views sample them.
        let (shadow_epoch, shadow_attempts, shadow_issued) =
            shadow.map_or_else(Default::default, |s| s.counts());
        state.audit.views[source.index] = ViewAudit {
            frame: frame.frame,
            glass: glass.audit(),
            surface: lighting.draws(),
            surface_instances: lighting.instances(),
            shadow_epoch,
            shadow_attempts,
            shadow_issued,
            shadow_instances: shadow.map_or_else(Default::default, |s| s.instance_counts()),
            props: props.audit(source.index),
            collector_frame: sun.encoded_frame(),
            atlas_size: surfaces.targets.sun.size,
            physical_extent: [targets.0.colors.width, targets.0.colors.height],
            native_extent: frame.views[source.index].unwrap().native_extent.to_array(),
            projection: frame.views[source.index]
                .unwrap()
                .raster_projection
                .to_cols_array(),
        };
    }
    if mirror_view(source.index) {
        // The slot image is the source `rt`. Its R8G8B8A8_UNorm format clamps the HDR color.
        let (Some(mirrors), Some(image)) = (
            mirrors,
            mirror_frame
                .images
                .get(source.index - MIRROR_START)
                .and_then(|handle| gpu_images.get(handle)),
        ) else {
            return;
        };
        mirrors
            .copy
            .encode(context.command_encoder(), &cache, &image.texture_view);
        return;
    }
    if source.index >= PROBE_START {
        let Some(probes) = probe.as_mut() else { return };
        let probe = &mut probes.probes[frame.probe_index];
        let face = (source.index - PROBE_START) as u8;
        if probe.sky_frames[usize::from(face)].load(Ordering::Acquire) != frame.frame {
            return;
        }
        if !probe.filter.capture_face(
            context.command_encoder(),
            &cache,
            &surfaces.probes[frame.probe_index],
            usize::from(face),
        ) {
            return;
        }
        let job = ProbeJob::Face(face);
        if let Err(error) = probe.publication.encoded(frame.probe_token, job) {
            state.proof.fail(error.into());
            return;
        }
        if face == 0
            && let Some(request) = frame.readiness_request()
        {
            state
                .proof
                .capture_started(request, frame.frame, frame.probe_index);
        }
        probe.encoded_token = Some(frame.probe_token);
        return;
    }
    if !effects.encoded(frame.frame, frame.main_token) {
        return;
    }
    state.audit.mask |= 1 << source.index;
    if source.index == 0 && state.audit.mask == 1 && state.audit.sky {
        let Some(output) = target.out_texture_color_attachment(None) else {
            return;
        };
        if resources
            .held
            .show(context.command_encoder(), &cache, output.view)
        {
            resources.held.filled.store(true, Ordering::Relaxed);
            state.audit.presented = true;
            state.audit.token = frame.main_token;
        }
    }
}
fn submitted(
    progress: Res<RendererStateHandle>,
    frame: Res<SourceViewFrame>,
    mut probe: Option<ResMut<ProbeFrame>>,
    cache: Res<PipelineCache>,
    acks: Res<SourceSubmissionAcks>,
    gate: Res<SourceExecutionGate>,
    queue: Res<RenderQueue>,
    status: Res<RendererStateHandle>,
) {
    if status.failure().is_some() {
        return;
    }
    let mut state = progress.lock();
    if state.audit.presented {
        if state.audit.first_token == 0 {
            state.audit.first_token = state.audit.token;
        }
        acks.main_submitted(state.audit.token);
        let thumbnail = THUMBNAIL_TOKEN.load(Ordering::Relaxed);
        if thumbnail != 0 && state.audit.token >= thumbnail {
            THUMBNAIL_TOKEN.store(0, Ordering::Relaxed);
            crate::browser::game_thumbnail_frame();
        }
        if crate::app::cpu_profile::enabled() {
            crate::browser::game_cpu_submitted(state.audit.token);
        }
        if state.audit.validation == 0 {
            state.audit.validation = gate.watch_submission(&queue);
        }
    }
    let validation = gate.snapshot(state.audit.validation);
    state.audit.validated =
        validation.requested != 0 && validation.validated >= validation.requested;
    let mut observations = [crate::render_readiness::ProbeObservation {
        pipelines_ready: false,
        published: false,
        validation: crate::render_readiness::Validation::default(),
    }; PROBE_COUNT];
    if let Some(probes) = probe.as_mut().filter(|probes| probes.scene == frame.scene) {
        for (index, probe) in probes.probes.iter_mut().enumerate() {
            let result = (|| -> Result<(), &'static str> {
                if let Some(token) = probe.encoded_token.take() {
                    let advance = probe.publication.submitted(token)?;
                    probe.submitted_token = token;
                    if advance {
                        acks.probe_submitted(token);
                    } else {
                        probe.filter_validation = gate.watch_submission(&queue);
                    }
                }
                if let Some(token) = probe.publication_encoded.take() {
                    probe.publication_token = token;
                    probe.publication_validation = gate.watch_submission(&queue);
                }
                if probe.publication_validation != 0
                    && validation.validated >= probe.publication_validation
                {
                    let token = probe.publication_token;
                    probe.publication.publication_submitted(token)?;
                    if let Some(request) = frame.readiness_request() {
                        state.proof.publication_validated(request, index);
                    }
                    acks.probe_publication_submitted(token);
                    probe.published_token = token;
                    probe.published_validation = probe.publication_validation;
                    probe.publication_validation = 0;
                    probe.publication_token = 0;
                }
                Ok(())
            })();
            if let Err(error) = result {
                state.proof.fail(error.into());
                return;
            }
            observations[index] = crate::render_readiness::ProbeObservation {
                pipelines_ready: probe.filter.ready(&cache),
                published: probe.publication.published(),
                validation: crate::render_readiness::Validation {
                    requested: probe.published_validation,
                    ..validation
                },
            };
            if !audit_watched() {
                continue;
            }
            state.audit.probes[index] = ProbeAudit {
                enabled: frame.probe_enabled[index],
                checks: frame.probe_checks,
                distance_squared: frame.probe_distance_squared[index],
                token: frame.probe_token,
                job: match if index == frame.probe_index {
                    frame.probe_job
                } else {
                    None
                } {
                    Some(ProbeJob::Face(face)) => face + 1,
                    Some(ProbeJob::FirstMip) => 7,
                    Some(ProbeJob::RemainingMipsAndPublish) => 8,
                    None => 0,
                },
                face_mask: probe.publication.face_mask(),
                mip_mask: probe.publication.mip_mask(),
                pipelines_ready: probe.filter.ready(&cache),
                filter_validation: probe.filter_validation,
                publication_validation: probe.publication_validation,
                published: probe.publication.published(),
                generation: probe.publication.generation(),
                submitted_token: probe.submitted_token,
                published_token: probe.published_token,
            };
        }
    }
    if let Some(identity) = frame.render_identity() {
        let audit = &state.audit;
        let passes = &audit.passes;
        let observation = crate::render_readiness::GraphObservation {
            identity,
            frame: audit.frame,
            token: audit.token,
            sky: audit.sky,
            world_mask: audit.mask,
            display_encoded: audit.presented,
            submitted_token: if audit.presented { audit.token } else { 0 },
            targets_ready: passes.targets,
            lighting_textures: passes.lighting_textures,
            lighting_gpu_textures: passes.lighting_gpu_textures,
            surface_ready: passes.surface,
            camera_ready: passes.camera,
            sky_ready: passes.sky,
            cloud_ready: passes.cloud,
            caster_ready: passes.caster,
            probes: observations,
            validation,
        };
        state.proof.observe_graph(observation);
    } else {
        state.proof.invalidate_frame();
    }
}

#[derive(Clone, Copy, Default, serde::Serialize)]
struct ViewAudit {
    frame: u32,
    glass: super::stained_glass::GlassAudit,
    surface: [u32; 5],
    surface_instances: [u32; 4],
    shadow_epoch: u32,
    shadow_attempts: [u32; super::lamp_shadows::TILES],
    shadow_issued: [u32; super::lamp_shadows::TILES],
    shadow_instances: [[u32; super::lamp_shadows::TILES]; 2],
    /// Counts of GPU prop rows, camera-visible rows and issued indirect draws.
    props: [u32; 3],
    collector_frame: u32,
    atlas_size: u32,
    physical_extent: [u32; 2],
    native_extent: [u32; 2],
    projection: [f32; 16],
}
struct SourceDiagnostics {
    state: RendererStateHandle,
    gate: SourceExecutionGate,
}
static SOURCE_DIAGNOSTICS: OnceLock<SourceDiagnostics> = OnceLock::new();

static THUMBNAIL: AtomicBool = AtomicBool::new(false);
/// Token of the frame encoded for the thumbnail. 0 is none.
static THUMBNAIL_TOKEN: AtomicU32 = AtomicU32::new(0);

/// The canvas holds a frame only in the task that drew it, so the thumbnail frame calls back when presented.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn request_thumbnail() {
    THUMBNAIL.store(true, Ordering::Relaxed);
}
/// The next encoded main frame is the thumbnail.
pub(crate) fn thumbnail_requested() -> bool {
    THUMBNAIL.load(Ordering::Relaxed)
}
pub(crate) fn thumbnail_encoded(token: u32) {
    THUMBNAIL.store(false, Ordering::Relaxed);
    THUMBNAIL_TOKEN.store(token, Ordering::Relaxed);
}

/// Browser graphics failures use the same terminal state as native device errors.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn source_render_failure(message: &str) {
    let Some(renderer) = SOURCE_DIAGNOSTICS.get() else {
        return;
    };
    renderer.state.fail(message.to_owned());
}

/// Serialize only on explicit diagnostic calls. Render frames copy fixed-size counters.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn source_render_audit() -> String {
    let Some(audit) = SOURCE_DIAGNOSTICS.get() else {
        return "null".into();
    };
    let state = audit.state.lock();
    let status = match state.proof.status() {
        crate::render_readiness::RendererStatus::Loading => "Loading".to_owned(),
        crate::render_readiness::RendererStatus::Ready => "Ready".to_owned(),
        crate::render_readiness::RendererStatus::Failed(error) => format!("Failed: {error}"),
    };
    #[derive(serde::Serialize)]
    struct GraphDiagnostic<'a> {
        loading: &'a crate::render_readiness::RendererReadiness,
        #[serde(flatten)]
        audit: &'a FrameAudit,
    }
    serde_json::to_string(&serde_json::json!({
        "state": status, "graph": GraphDiagnostic { loading: &state.proof, audit: &state.audit },
        "gpu_validated_generation": audit.gate.validated_generation(),
        "gpu_ms": super::gpu_timing::stage_ms(),
        "play_pipelines": super::play_pipelines::count(),
        "scene_scale": crate::render_scale::percent(),
    }))
    .expect("source diagnostic values serialize")
}

#[derive(Clone, Copy, Default, serde::Serialize)]
struct CpuBoundary {
    lighting_assets_present: bool,
    lighting_assets_loaded: bool,
    lighting_texture_bytes: u64,
    source_materials_present: bool,
    source_materials_bound: bool,
    source_material_state: u8,
    source_texture_bytes: u64,
    material_loading_ready: bool,
    requests_sealed: bool,
    shadow_capacity: u32,
    transparent_capacity: u32,
    master_bound: bool,
    master_materials_prepared: bool,
    master_pending_lamp_targets: u32,
    master_sky_bound: bool,
    master_warming: bool,
    master_cookie_handles: bool,
    visuals_bound: bool,
}
#[derive(Clone, Copy, Default, serde::Serialize)]
struct GpuBoundary {
    asset_audit_refreshes: u32,
    view_keys: u32,
    gpu_meshes: u32,
    prepared_materials: u32,
    source_prepared_materials: u32,
    master_cookies: bool,
    master_uniform: bool,
    master_state: u8,
    master_pipeline_audit: super::lighting::LightingPipelineAudit,
    lighting_render_assets: bool,
    cloud_gpu_mesh: bool,
    frame_resources: bool,
    frame_copy_ready: bool,
    source_surface_pipelines: u32,
    source_add_pipelines: u32,
    source_views: u32,
    source_lighting_bindings: u32,
    source_glass_bindings: u32,
    opaque_phases: u32,
    masked_phases: u32,
    transparent_phases: u32,
    shadow_phases: u32,
    shadow_scene_frame: u32,
    shadow_candidates: [usize; super::surface_views::WORLD_VIEWS],
    shadow_split_counts: [[usize; 4]; super::surface_views::WORLD_VIEWS],
    shadow_offscreen_casters: usize,
    shadow_queue_frame: u32,
    shadow_specializations: usize,
    shadow_ready_mask: u16,
    snapshot_frame: u32,
    snapshot_views: u16,
    sky_snapshot: bool,
    prewarm: super::materials::MaterialPrewarmAudit,
}
fn state_code(state: &RenderingState) -> u8 {
    match state {
        RenderingState::Loading => 0,
        RenderingState::Ready => 1,
        RenderingState::Failed(_) => 2,
    }
}
fn cpu_boundaries(
    progress: Res<RendererStateHandle>,
    assets: Option<Res<super::lighting_assets::LightingAssets>>,
    materials: Option<Res<super::materials::SourceMaterials>>,
    loading: Option<Res<super::materials::MaterialLoading>>,
    master: Option<Res<super::lighting::SourceLighting>>,
    visuals: Option<Res<super::reference::ReferenceVisuals>>,
) {
    let mut boundary = CpuBoundary::default();
    if let Some(assets) = assets {
        boundary.lighting_assets_present = true;
        boundary.lighting_assets_loaded = assets.loaded;
        boundary.lighting_texture_bytes = assets.texture_bytes as u64;
    }
    if let Some(materials) = materials {
        boundary.source_materials_present = true;
        boundary.source_materials_bound = materials.bound;
        boundary.source_material_state = state_code(&materials.state);
        boundary.source_texture_bytes = materials.texture_bytes as u64;
    }
    if let Some(loading) = loading {
        boundary.material_loading_ready = loading.is_ready();
        boundary.requests_sealed = loading.source_shadow_capacity().is_some();
        boundary.shadow_capacity = loading.source_shadow_capacity().unwrap_or(0) as u32;
        boundary.transparent_capacity = loading.transparent_pass_capacity().unwrap_or(0) as u32;
    }
    if let Some(master) = master {
        boundary.master_bound = master.bound;
        boundary.master_materials_prepared = master.materials_prepared;
        boundary.master_pending_lamp_targets = master
            .lamp_materials
            .iter()
            .filter(|target| target.current.is_none())
            .count() as u32;
        boundary.master_sky_bound = master.sky.bound();
        boundary.master_warming = master.warming;
        boundary.master_cookie_handles = master.cookie_handles.is_some();
    }
    boundary.visuals_bound = visuals.is_some_and(|v| v.bound);
    progress.lock().audit.cpu_boundary = boundary;
}
/// Render passes and inputs that loading readiness checks. Filled each frame.
#[derive(Clone, Copy, Default, serde::Serialize)]
struct PassReadiness {
    targets: bool,
    lighting_textures: u32,
    lighting_gpu_textures: u32,
    camera: bool,
    surface: bool,
    caster: bool,
    sky: bool,
    cloud: bool,
}
#[derive(bevy::ecs::system::SystemParam)]
struct PassInputs<'w> {
    targets: Option<Res<'w, SurfaceResources>>,
    lighting: Option<Res<'w, LightingRenderAssets>>,
    images: Res<'w, bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    camera: Option<Res<'w, SourceViewCamera>>,
    surface: Option<Res<'w, super::surface_pass::SurfacePasses>>,
    caster: Option<Res<'w, super::caster::CasterPasses>>,
    sky: Option<Res<'w, SkyPass>>,
    cloud: Option<Res<'w, super::clouds::CloudPass>>,
    owners: Res<'w, super::pipeline_ownership::PipelineOwners>,
    cache: Res<'w, PipelineCache>,
}
/// Lighting texture counts change only when lighting assets or GPU images change.
#[derive(Default)]
struct LightingTextureCache {
    stamps: Option<[u32; 2]>,
    counts: [u32; 2],
}
fn pass_readiness(
    inputs: PassInputs,
    mut textures: Local<LightingTextureCache>,
    progress: Res<RendererStateHandle>,
) {
    let cache = &*inputs.cache;
    let mut passes = PassReadiness {
        targets: inputs.targets.is_some(),
        camera: inputs.camera.is_some_and(|c| c.ready()),
        surface: inputs
            .surface
            .is_some_and(|p| p.ready(cache, &inputs.owners)),
        caster: inputs
            .caster
            .is_some_and(|p| p.ready(cache, &inputs.owners)),
        sky: inputs.sky.is_some_and(|p| p.ready(cache)),
        cloud: inputs.cloud.is_some_and(|p| p.ready(cache)),
        ..default()
    };
    if let Some(lighting) = &inputs.lighting {
        let stamps = [
            lighting.last_changed().get(),
            inputs.images.last_changed().get(),
        ];
        if textures.stamps != Some(stamps) {
            textures.stamps = Some(stamps);
            textures.counts = [
                lighting.textures.len() as u32,
                lighting
                    .textures
                    .values()
                    .filter(|image| inputs.images.get(image.id()).is_some())
                    .count() as u32,
            ];
        }
        [passes.lighting_textures, passes.lighting_gpu_textures] = textures.counts;
    }
    progress.lock().audit.passes = passes;
}

/// Only F3 and the bench read the renderer audit, so frames fill it only while one watches.
static AUDIT_WATCHED: AtomicBool = AtomicBool::new(false);
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn watch_render_audit(on: bool) {
    AUDIT_WATCHED.store(on, Ordering::Relaxed);
}
fn audit_watched() -> bool {
    AUDIT_WATCHED.load(Ordering::Relaxed)
}
#[derive(Default)]
struct AssetAuditCache {
    stamps: Option<[Option<u32>; 2]>,
    counts: [u32; 3],
    refreshes: u32,
}
fn resource_stamp<T: Resource>(world: &World) -> Option<u32> {
    world
        .get_resource_ref::<T>()
        .map(|resource| resource.last_changed().get())
}
/// Source cameras cluster lights on the CPU on every backend. GPU clustering fails on the zero-cluster sky camera.
fn cpu_clustering(mut settings: ResMut<bevy::light::cluster::GlobalClusterSettings>) {
    settings.gpu_clustering = None;
}
fn gpu_boundaries(
    world: &World,
    mut audit: Local<AssetAuditCache>,
    views: Query<(
        &SourceSurfaceView,
        Option<&super::source_lighting::SourceViewLighting>,
        Option<&super::stained_glass::SourceGlassView>,
    )>,
) {
    use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d};
    use bevy::pbr::{PreparedMaterial, ViewKeyCache};
    use bevy::render::mesh::RenderMesh;
    use bevy::render::render_phase::ViewBinnedRenderPhases;
    use bevy::render::{erased_render_asset::ErasedRenderAssets, render_asset::RenderAssets};
    let mut boundary = GpuBoundary::default();
    if let Some(keys) = world.get_resource::<ViewKeyCache>() {
        boundary.view_keys = keys.len() as u32;
    }
    let stamps = [
        resource_stamp::<RenderAssets<RenderMesh>>(world),
        resource_stamp::<ErasedRenderAssets<PreparedMaterial>>(world),
    ];
    if audit.stamps != Some(stamps) {
        audit.stamps = Some(stamps);
        audit.counts = [0; 3];
        audit.refreshes += 1;
        if let Some(meshes) = world.get_resource::<RenderAssets<RenderMesh>>() {
            audit.counts[0] = meshes.iter().count() as u32;
        }
        if let Some(materials) = world.get_resource::<ErasedRenderAssets<PreparedMaterial>>() {
            audit.counts[1] = materials.iter().count() as u32;
            audit.counts[2] = materials
                .iter()
                .filter(|(id, _)| {
                    id.type_id() == std::any::TypeId::of::<super::materials::SourceMaterial>()
                })
                .count() as u32;
        }
    }
    boundary.gpu_meshes = audit.counts[0];
    boundary.prepared_materials = audit.counts[1];
    boundary.source_prepared_materials = audit.counts[2];
    boundary.asset_audit_refreshes = audit.refreshes;
    if let Some(assets) = world.get_resource::<LightingRenderAssets>() {
        boundary.lighting_render_assets = true;
        boundary.cloud_gpu_mesh = world
            .get_resource::<RenderAssets<RenderMesh>>()
            .is_some_and(|meshes| meshes.get(assets.cloud_mesh.id()).is_some());
    }
    if let Some(master) = world.get_resource::<super::lighting::LightingGpu>() {
        boundary.master_cookies = master.cookies.is_some();
        boundary.master_uniform = master.uniform.buffer().is_some();
        boundary.master_state = match &*master.status.lock().unwrap() {
            super::lighting::RenderingState::Loading => 0,
            super::lighting::RenderingState::Ready => 1,
            super::lighting::RenderingState::Failed(_) => 2,
        };
    }
    if let Some(audit) = world.get_resource::<super::lighting::LightingPipelineAudit>() {
        boundary.master_pipeline_audit = *audit;
    }
    let cache = world.resource::<PipelineCache>();
    if let Some(resources) = world.get_resource::<FrameResources>() {
        boundary.frame_resources = true;
        boundary.frame_copy_ready = resources.held.copy.ready(cache);
    }
    if let Some(surface) = world.get_resource::<super::surface_pass::SurfacePasses>() {
        let (base, add) = surface.counts();
        boundary.source_surface_pipelines = base as u32;
        boundary.source_add_pipelines = add as u32;
    }
    for (_, lighting, glass) in &views {
        boundary.source_views += 1;
        boundary.source_lighting_bindings += u32::from(lighting.is_some());
        boundary.source_glass_bindings += u32::from(glass.is_some());
    }
    boundary.opaque_phases = world
        .get_resource::<ViewBinnedRenderPhases<Opaque3d>>()
        .map_or(0, |p| p.len() as u32);
    boundary.masked_phases = world
        .get_resource::<ViewBinnedRenderPhases<AlphaMask3d>>()
        .map_or(0, |p| p.len() as u32);
    boundary.transparent_phases = world
        .get_resource::<ViewSortedRenderPhases<Transparent3d>>()
        .map_or(0, |p| p.len() as u32);
    boundary.shadow_phases = world
        .get_resource::<ViewBinnedRenderPhases<super::shadow_phase::SourceShadow>>()
        .map_or(0, |p| p.len() as u32);
    if let Some(scene) = world.get_resource::<super::shadow_scene::ShadowScene>() {
        boundary.shadow_candidates = scene.candidates;
        boundary.shadow_split_counts = scene.split_counts;
        boundary.shadow_offscreen_casters = scene.shadow_only;
    }
    boundary.shadow_scene_frame = world
        .get_resource::<super::shadow_scene::ShadowScene>()
        .map_or(0, |s| s.frame);
    if let Some(queues) = world.get_resource::<super::shadow_queue::ShadowQueues>() {
        boundary.shadow_queue_frame = queues.frame;
        boundary.shadow_specializations = queues.cached_pipelines();
        for (index, ready) in queues.ready.iter().enumerate() {
            if *ready {
                boundary.shadow_ready_mask |= 1 << index;
            }
        }
    }
    if let Some(frame) = world.get_resource::<SourceViewFrame>() {
        boundary.snapshot_frame = frame.frame;
        boundary.sky_snapshot = frame.sky_clip_from_world.is_some();
        for (index, view) in frame.views.iter().enumerate() {
            if view.is_some() {
                boundary.snapshot_views |= 1 << index;
            }
        }
    }
    if let Some(loading) = world.get_resource::<super::materials::MaterialLoading>() {
        boundary.prewarm = loading.prewarm_audit(world);
    }
    world
        .resource::<RendererStateHandle>()
        .lock()
        .audit
        .gpu_boundary = boundary;
}

fn prepare_probe(
    mut commands: Commands,
    surfaces: Option<Res<SurfaceResources>>,
    assets: Option<Res<LightingRenderAssets>>,
    existing: Option<ResMut<ProbeFrame>>,
    frame: Res<SourceViewFrame>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if status.failure().is_some() {
        return;
    }
    if let Some(mut existing) = existing {
        if existing.scene != frame.scene {
            let Some(surfaces) = surfaces else {
                return;
            };
            existing.scene = frame.scene;
            for (index, probe) in existing.probes.iter_mut().enumerate() {
                surfaces.probes[index].clear_publication(&queue);
                probe.publication = ProbePublication::default();
                probe.encoded_token = None;
                probe.publication_encoded = None;
                probe.filter_validation = 0;
                probe.publication_validation = 0;
                probe.published_validation = 0;
                probe.publication_token = 0;
                probe.submitted_token = 0;
                probe.published_token = 0;
                for stamp in &probe.sky_frames {
                    stamp.store(0, Ordering::Release);
                }
            }
        }
        for probe in &existing.probes {
            if let Some(error) = probe.filter.failure(&cache) {
                status.fail(error);
            }
        }
        return;
    }
    let (Some(surfaces), Some(assets)) = (surfaces, assets) else {
        return;
    };
    let mut pipelines = None;
    let probes = std::array::from_fn(|index| {
        let filter = ProbeFilter::new(
            &device,
            &queue,
            &cache,
            assets.shaders.probe.clone(),
            &surfaces.probes[index],
            surfaces.targets.capture.resolved.sampled.as_ref().unwrap(),
            pipelines,
        );
        pipelines = Some(filter.pipelines());
        ProbeState {
            filter,
            publication: ProbePublication::default(),
            encoded_token: None,
            publication_encoded: None,
            filter_validation: 0,
            publication_validation: 0,
            published_validation: 0,
            publication_token: 0,
            submitted_token: 0,
            published_token: 0,
            sky_frames: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    });
    commands.insert_resource(ProbeFrame {
        scene: frame.scene,
        probes,
    });
}

fn clear_capture(
    view: ViewQuery<(
        &SourceSurfaceView,
        &super::surface_graph::SurfaceViewTargets,
    )>,
    mut context: RenderContext,
) {
    let (source, targets) = view.into_inner();
    if source.index < WORLD_VIEWS {
        return;
    }
    let colors = [Some(RenderPassColorAttachment {
        view: targets.0.colors.attachment(),
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::BLACK.into()),
            store: StoreOp::Store,
        },
    })];
    drop(
        context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("source_probe_face_clear"),
                color_attachments: &colors,
                ..default()
            }),
    );
}
fn render_capture_sky(
    view: ViewQuery<(
        &SourceSurfaceView,
        &super::surface_graph::SurfaceViewTargets,
    )>,
    sky: Res<SkyPass>,
    probe: Option<Res<ProbeFrame>>,
    frame: Res<SourceViewFrame>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    mut context: RenderContext,
) {
    let (source, targets) = view.into_inner();
    if source.index < WORLD_VIEWS || status.failure().is_some() {
        return;
    }
    // Sky bindings share the surface view index.
    let colors = [Some(RenderPassColorAttachment {
        view: targets.0.colors.attachment(),
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("source_capture_sky"),
        color_attachments: &colors,
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: &targets.0.depth.attachment,
            depth_ops: Some(Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..default()
    });
    let drawn = sky.draw(source.index, &mut pass, &cache);
    drop(pass);
    if drawn
        && probe_view(source.index)
        && let Some(probe) = probe.as_ref()
    {
        probe.probes[frame.probe_index].sky_frames[source.index - PROBE_START]
            .store(frame.frame, Ordering::Release);
    }
}
fn filter_probe(
    mut probe: Option<ResMut<ProbeFrame>>,
    surfaces: Option<Res<SurfaceResources>>,
    frame: Res<SourceViewFrame>,
    progress: Res<RendererStateHandle>,
    gate: Res<SourceExecutionGate>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    mut context: RenderContext,
) {
    if status.failure().is_some() || !progress.lock().audit.presented {
        return;
    }
    let (Some(probe), Some(surfaces)) = (probe.as_mut(), surfaces) else {
        return;
    };
    let probe = &mut probe.probes[frame.probe_index];
    if probe.publication_validation != 0 {
        return;
    }
    if probe.filter_validation != 0 {
        if !gate.submission_validated(probe.filter_validation) {
            return;
        }
        let Some(token) = probe.publication.validation_token() else {
            status.fail("Source probe validation lacks its token".into());
            return;
        };
        let result = probe.publication.validated(token, true).and_then(|()| {
            probe.publication.encode_publication(
                context.command_encoder(),
                &surfaces.probes[frame.probe_index],
                &probe.filter,
                &cache,
            )
        });
        match result {
            Ok(token) => {
                probe.filter_validation = 0;
                probe.publication_encoded = Some(token);
            }
            Err(error) => status.fail(error.into()),
        }
        return;
    }
    if !probe.accepts(frame.probe_token) || !probe.filter.ready(&cache) {
        return;
    }
    let Some(job) = frame.probe_job else { return };
    let encoded = match job {
        ProbeJob::FirstMip => probe.filter.first_mip(
            context.command_encoder(),
            &cache,
            &surfaces.probes[frame.probe_index],
        ),
        ProbeJob::RemainingMipsAndPublish => probe.filter.remaining_mips(
            context.command_encoder(),
            &cache,
            &surfaces.probes[frame.probe_index],
        ),
        ProbeJob::Face(_) => false,
    };
    if encoded {
        if let Err(error) = probe.publication.encoded(frame.probe_token, job) {
            status.fail(error.into());
            return;
        }
        probe.encoded_token = Some(frame.probe_token);
    }
}
