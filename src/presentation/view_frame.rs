use super::lighting_scene::PROBE_COUNT;
use simulation::boundary::StateBlock;
// One main-world view snapshot for raster, culling, and shadow fitting.
use super::surface_views::{
    MIRROR_EXTENT, MIRROR_START, PROBE_START, VIEW_COUNT, WORLD_VIEWS, mirror_view, probe_view,
};
use super::{
    lighting_assets::LightingAssets, lighting_scene::SourceSceneLighting,
    readiness::RendererStateHandle, surface_views::SourceSurfaceView,
};
use crate::reference::{
    reflections::{
        ProbeJob, ProbeSchedule, capture_view_from_world, next_probe_index, prepare_probe_face,
    },
    shadow_planes::{CullPlanes, implicit_cull_planes},
};
use bevy::render::{extract_resource::ExtractResource, view::ExtractedView};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// CPU submission acknowledgements. GPU completion remains SourceExecutionGate's responsibility.
#[derive(Resource, Clone, Default)]
pub struct SourceSubmissionAcks(Arc<SubmissionAcks>);
#[derive(Default)]
struct SubmissionAcks {
    main: AtomicU32,
    probe: AtomicU32,
    publication: AtomicU32,
}
impl SourceSubmissionAcks {
    pub fn main_token(&self) -> u32 {
        self.0.main.load(Ordering::Acquire)
    }
    /// Called only after the owner's complete commands have entered the submitted frame.
    pub fn main_submitted(&self, token: u32) {
        self.0.main.fetch_max(token, Ordering::Release);
    }
    pub fn probe_submitted(&self, token: u32) {
        self.0.probe.fetch_max(token, Ordering::Release);
    }
}
#[derive(Resource)]
pub struct SourceSceneEpoch(pub u32);
impl Default for SourceSceneEpoch {
    fn default() -> Self {
        Self(1)
    }
}
/// The source restart caller recreates its scene. GPU allocations remain owned by the renderer.
pub fn restart_scene(world: &mut World) {
    let acks = world.resource::<SourceSubmissionAcks>().clone();
    if let Err(error) = world.resource_mut::<SourceViewClock>().restart(&acks) {
        world.resource::<RendererStateHandle>().fail(error.into());
        return;
    }
    let next = world
        .resource::<SourceSceneEpoch>()
        .0
        .checked_add(1)
        .expect("Source scene generation exhausted");
    world.resource_mut::<SourceSceneEpoch>().0 = next;
    world
        .resource::<RendererStateHandle>()
        .lock()
        .proof
        .invalidate(crate::render_readiness::Invalidation::Scene);
}
#[derive(Resource, Default)]
pub struct SourceViewClock {
    preparation_ready: bool,
    probe: [ProbeSchedule; PROBE_COUNT],
    probe_index: usize,
    main_token: u32,
    probe_token: u32,
    frame: u32,
}
impl SourceViewClock {
    pub fn main_token(&self) -> u32 {
        self.main_token
    }
    /// The slot of the active capture. Its probe must not change.
    pub(crate) fn probe_index(&self) -> usize {
        self.probe_index
    }
    pub(crate) fn probe_published(&self, index: usize) -> bool {
        self.probe[index].published()
    }
    /// A new probe in a slot starts unpublished. Only idle slots may reset.
    pub(crate) fn reset_probe(&mut self, index: usize) {
        debug_assert_ne!(index, self.probe_index);
        self.probe[index] = ProbeSchedule::default();
    }
    fn probe_job(&self, enabled: [bool; PROBE_COUNT]) -> Option<ProbeJob> {
        self.probe[self.probe_index].pending_job(enabled[self.probe_index])
    }
    fn probe_cameras_active(&self, enabled: [bool; PROBE_COUNT]) -> bool {
        self.probe[self.probe_index]
            .cameras_active(enabled[self.probe_index], !self.preparation_ready)
    }
    fn consume(
        &mut self,
        acks: &SourceSubmissionAcks,
        probe_enabled: [bool; PROBE_COUNT],
    ) -> Result<(), &'static str> {
        if self.main_token == 0 {
            self.main_token = 1;
            self.probe_token = 1;
        }
        let main = acks.0.main.load(Ordering::Acquire);
        let probe = acks.0.probe.load(Ordering::Acquire);
        if main > self.main_token || probe > self.probe_token {
            return Err("Source view acknowledgement is ahead of its token");
        }
        if main == self.main_token {
            self.main_token = self
                .main_token
                .checked_add(1)
                .ok_or("Source main render token exhausted")?;
        }
        let mut published = false;
        if probe == self.probe_token {
            if self.probe[self.probe_index].job() == ProbeJob::RemainingMipsAndPublish
                && acks.0.publication.load(Ordering::Acquire) != probe
            {
                return Err("Source probe final job lacks publication acknowledgement");
            }
            let completed = self.probe[self.probe_index].job();
            self.probe[self.probe_index].complete(completed)?;
            published = completed == ProbeJob::RemainingMipsAndPublish;
            self.probe_token = self
                .probe_token
                .checked_add(1)
                .ok_or("Source probe job token exhausted")?;
        }
        // Enabled probes capture in turn. A published probe keeps its cubemap.
        let current = self.probe_index;
        let next = if published
            || self.probe[current]
                .pending_job(probe_enabled[current])
                .is_none()
        {
            next_probe_index(current, probe_enabled)
        } else {
            current
        };
        if next != self.probe_index {
            self.probe_index = next;
            if !published {
                self.probe_token = self
                    .probe_token
                    .checked_add(1)
                    .ok_or("Source probe job token exhausted")?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct ViewSnapshot {
    pub extent: UVec2,
    pub native_extent: UVec2,
    pub view_from_world: Mat4,
    pub implicit_projection: Mat4,
    pub raster_projection: Mat4,
    pub source_world_from_view: Mat4,
    pub source_camera_planes: [[f32; 4]; 6],
    pub source_shadow_planes: [[f32; 4]; 6],
    pub frustum_points: [[f32; 3]; 8],
    pub near: f32,
    pub far: f32,
    pub fov_degrees: f32,
    pub mask: u32,
    pub shadow_distance: f32,
}
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct SourceViewFrame {
    pub session: u64,
    pub selection_revision: u64,
    pub(crate) scale: crate::render_scale::SceneScale,
    pub(crate) shadow_quality: super::shadow_quality::ShadowQuality,
    pub preparation_warmup: bool,
    pub motion_revision: u32,
    pub scene: u32,
    pub probe_enabled: [bool; PROBE_COUNT],
    pub probe_position: [Vec3; PROBE_COUNT],
    pub probe_checks: u32,
    pub probe_distance_squared: [f32; PROBE_COUNT],
    pub probe_index: usize,
    pub sky_clip_from_world: Option<Mat4>,
    pub skybox_clip_from_world: Option<Mat4>,
    pub frame: u32,
    pub main_token: u32,
    pub probe_token: u32,
    pub probe_job: Option<ProbeJob>,
    pub views: [Option<ViewSnapshot>; VIEW_COUNT],
    /// Main-view motion state. None when antialiasing and motion blur are off.
    pub main_motion: Option<super::temporal::MainMotion>,
}
impl SourceViewFrame {
    /// Shadow atlas side of a view. The shadow quality shrinks only world views.
    pub fn atlas_size(&self, index: usize) -> Option<u32> {
        let native = self.views[index]?.native_extent;
        let source = crate::reference::shadows::source_atlas_size(native.x, native.y)?;
        Some(if index < WORLD_VIEWS {
            self.shadow_quality.atlas_size(source)
        } else {
            source
        })
    }
    /// A probe face or mirror view without a snapshot prepares and draws nothing this frame.
    pub fn skip_preparation(&self, index: usize) -> bool {
        index >= PROBE_START && self.views[index].is_none()
    }
    pub(crate) fn render_identity(&self) -> Option<crate::render_readiness::RenderIdentity> {
        let view = self.views[0]?;
        Some(crate::render_readiness::RenderIdentity {
            session: self.session,
            scene: self.scene,
            native_extent: view.native_extent.to_array(),
            scene_extent: view.extent.to_array(),
            scale: self.scale,
        })
    }
    pub(crate) fn readiness_request(&self) -> Option<crate::render_readiness::ReadinessRequest> {
        Some(crate::render_readiness::ReadinessRequest {
            render: self.render_identity()?,
            selection_revision: self.selection_revision,
            probe_mask: self.probe_mask(),
            history_revision: self.motion_revision,
        })
    }
    pub(crate) fn probe_mask(&self) -> u32 {
        self.probe_enabled
            .iter()
            .enumerate()
            .fold(0, |mask, (i, enabled)| mask | (u32::from(*enabled) << i))
    }
    /// The world view is mandatory. Probe snapshots follow warmup and the capture job.
    pub fn required_views_ready(&self) -> bool {
        self.frame != 0
            && self.views[..PROBE_START].iter().all(Option::is_some)
            && (0..6).all(|face| {
                !prepare_probe_face(self.probe_job, face, self.preparation_warmup)
                    || self.views[PROBE_START + face].is_some()
            })
    }
}
fn source_transform(m: Mat4, p: Vec4) -> Vec4 {
    // Matrix4x4::PerspectiveMultiplyPoint3 sums Y, X, Z, then W.
    ((m.y_axis * p.y + m.x_axis * p.x) + m.z_axis * p.z) + m.w_axis * p.w
}
fn corners(inverse: Mat4) -> [[f32; 3]; 8] {
    [
        (-1., -1., -1.),
        (1., -1., -1.),
        (1., 1., -1.),
        (-1., 1., -1.),
        (-1., -1., 1.),
        (1., -1., 1.),
        (1., 1., 1.),
        (-1., 1., 1.),
    ]
    .map(|(x, y, z)| {
        let v = source_transform(inverse, Vec4::new(x, y, z, 1.));
        (v.truncate() * (1. / v.w)).to_array()
    })
}
pub fn gpu_projection(source: Mat4) -> Mat4 {
    let mut p = source.to_cols_array_2d();
    for c in &mut p {
        c[1] = -c[1];
        c[2] = 0.5 * c[3] - 0.5 * c[2];
    }
    Mat4::from_cols_array_2d(&p)
}
/// Consume submitted jobs before camera update, visibility, and extraction.
fn sync_probe_cameras(
    assets: Option<Res<LightingAssets>>,
    scene: Res<SourceSceneLighting>,
    activation: Res<super::lighting_scene::ProbeActivation>,
    acks: Res<SourceSubmissionAcks>,
    mut clock: ResMut<SourceViewClock>,
    status: Res<RendererStateHandle>,
    mut views: Query<(&SourceSurfaceView, &mut Camera)>,
) {
    if status.failure().is_some() || assets.is_none_or(|assets| !assets.loaded) || scene.0.is_none()
    {
        return;
    }
    if let Err(error) = clock.consume(&acks, activation.enabled) {
        status.fail(error.into());
        status
            .lock()
            .proof
            .invalidate(crate::render_readiness::Invalidation::Content);
        return;
    }
    let active = clock.probe_cameras_active(activation.enabled);
    for (view, mut camera) in &mut views {
        if probe_view(view.index) && camera.is_active != active {
            camera.is_active = active;
        }
    }
}
/// Runs after current physical extents and transforms, before source frustum publication.
pub(crate) fn snapshot(
    mut commands: Commands,
    drive: Res<crate::app::Drive>,
    sim: Res<super::sim_frame::SimFrameResource>,
    requests: Res<crate::app::save::SaveRequests>,
    graph: Res<super::readiness::RendererStateHandle>,
    assets: Option<Res<LightingAssets>>,
    scene: Res<SourceSceneLighting>,
    epoch: Res<SourceSceneEpoch>,
    activation: Res<super::lighting_scene::ProbeActivation>,
    mut clock: ResMut<SourceViewClock>,
    mut frame: ResMut<SourceViewFrame>,
    status: Res<RendererStateHandle>,
    settings: (
        Res<crate::render_scale::SceneScaleSetting>,
        Res<super::shadow_quality::ShadowQuality>,
        Res<super::mirrors::MirrorFrame>,
    ),
    views: Query<(&SourceSurfaceView, &Camera, &GlobalTransform)>,
    sky_view: Query<(&Camera, &GlobalTransform), With<super::frame_graph::SourceSkyCamera>>,
    (antialiasing, blur, mut temporal, origin): (
        Res<super::antialiasing::Antialiasing>,
        Res<super::motion_blur::MotionBlur>,
        ResMut<super::temporal::TemporalView>,
        Res<super::lighting_scene::SourceWorldOrigin>,
    ),
) {
    frame.session = sim.frame.state.session;
    frame.selection_revision = sim.frame.state.selection_revision;
    let (scale, shadow_quality, mirrors) = settings;
    frame.scale = scale.0;
    frame.shadow_quality = *shadow_quality;
    frame.scene = epoch.0;
    frame.probe_job = None;
    frame.views = [None; VIEW_COUNT];
    frame.main_motion = None;
    frame.sky_clip_from_world = None;
    frame.skybox_clip_from_world = None;
    if sim.frame.state.flags & StateBlock::WORLD_FAILED != 0 {
        graph
            .lock()
            .proof
            .invalidate(crate::render_readiness::Invalidation::Content);
    }
    if status.failure().is_some() {
        graph
            .lock()
            .proof
            .invalidate(crate::render_readiness::Invalidation::Content);
        return;
    }
    let (Some(assets), Some(scene)) = (assets, scene.0) else {
        return;
    };
    if !assets.loaded {
        return;
    }
    let mut input = [None; VIEW_COUNT];
    let mut identities = [false; VIEW_COUNT];
    let probe_job = clock.probe_job(activation.enabled);
    let probes_active = clock.probe_cameras_active(activation.enabled);
    for (view, camera, pose) in &views {
        if view.index >= VIEW_COUNT || identities[view.index] {
            status.fail("Source snapshot view identity is invalid".into());
            graph
                .lock()
                .proof
                .invalidate(crate::render_readiness::Invalidation::Content);
            return;
        }
        identities[view.index] = true;
        // Mirror views take their pose and extent from the mirror frame.
        if mirror_view(view.index) || (probe_view(view.index) && !probes_active) {
            continue;
        }
        if !camera.is_active {
            return;
        }
        let Some(extent) = camera.physical_viewport_size() else {
            return;
        };
        if extent.x == 0 || extent.y == 0 {
            return;
        }
        input[view.index] = Some((extent, pose.to_matrix()));
    }
    if identities.contains(&false) {
        return;
    }
    let Ok((sky_camera, sky_pose)) = sky_view.single() else {
        return;
    };
    if !sky_camera.is_active
        || sky_camera
            .physical_viewport_size()
            .is_none_or(|size| size.x == 0 || size.y == 0)
    {
        return;
    }
    let main = input[0].unwrap().0;
    let Some(next_frame) = clock.frame.checked_add(1) else {
        status.fail("Source view frame stamp exhausted".into());
        graph
            .lock()
            .proof
            .invalidate(crate::render_readiness::Invalidation::Content);
        return;
    };
    clock.frame = next_frame;
    let request = crate::render_readiness::ReadinessRequest {
        render: crate::render_readiness::RenderIdentity {
            session: frame.session,
            scene: frame.scene,
            native_extent: main.to_array(),
            scene_extent: scale.0.extent(main.to_array()),
            scale: scale.0,
        },
        selection_revision: frame.selection_revision,
        probe_mask: activation
            .enabled
            .iter()
            .enumerate()
            .fold(0, |mask, (index, enabled)| {
                mask | (u32::from(*enabled) << index)
            }),
        history_revision: sim.frame.state.motion_revision,
    };
    clock.preparation_ready |= graph.lock().proof.graph_ready(request);
    let warmup = !clock.preparation_ready;
    let job = probe_job;
    let fov_degrees = sim.frame.state.fov_degrees;
    let mut snapshots: [Option<ViewSnapshot>; VIEW_COUNT] = std::array::from_fn(|index| {
        if mirror_view(index) {
            let view = mirrors.views[index - MIRROR_START]?;
            return Some(camera_snapshot(
                UVec2::splat(MIRROR_EXTENT),
                view.world_from_view.inverse(),
                view.near,
                view.far,
                view.fov_degrees,
                view.mask,
                assets.record.shadows.distance,
            ));
        }
        if index >= PROBE_START && !prepare_probe_face(job, index - PROBE_START, warmup) {
            return None;
        }
        let (extent, world_from_view) = input[index]?;
        let (near, far, fov, mask, distance) = if index < WORLD_VIEWS {
            let world =
                super::camera::world_camera_settings(fov_degrees, sim.frame.state.far_plane);
            (
                world.near,
                world.far,
                world.fov_degrees,
                world.mask,
                assets.record.shadows.distance,
            )
        } else {
            let p = &assets.record.probes[0];
            (p.near, p.far, 90., p.mask, p.shadow_distance)
        };
        let view = if index < WORLD_VIEWS {
            world_from_view.inverse()
        } else {
            Mat4::from_cols_array_2d(&capture_view_from_world(
                index - PROBE_START,
                scene.probe_position[clock.probe_index].to_array(),
            ))
        };
        let mut snapshot = camera_snapshot(extent, view, near, far, fov, mask, distance);
        if index < PROBE_START {
            snapshot.extent = UVec2::from_array(scale.0.extent(extent.to_array()));
        }
        Some(snapshot)
    });
    let main_motion = (antialiasing.0 || blur.0).then(|| {
        temporal.advance(
            snapshots[0].as_mut().unwrap(),
            antialiasing.0.then_some(&assets.record.postprocessing.taa),
            origin.0,
            sim.frame.state.motion_revision,
            epoch.0,
        )
    });
    if main_motion.is_none() {
        // A later start has no valid history.
        temporal.clear();
    }
    let jitter = main_motion.map_or(Mat4::IDENTITY, |motion| {
        super::temporal::jitter_clip(motion.jitter)
    });
    let sky_clips = Some(sky_pose).map(|pose| {
        let sky = &assets.record.cameras[0];
        let projection = Mat4::perspective_rh_gl(
            fov_degrees.to_radians(),
            main.x as f32 / main.y as f32,
            sky.near,
            sky.far,
        );
        let view = pose.to_matrix().inverse();
        let skybox_projection =
            Mat4::from_cols_array_2d(&crate::reference::sky::source_sky_gpu_projection(
                projection.to_cols_array_2d(),
                sky.near,
            ));
        (
            jitter * gpu_projection(projection) * view,
            jitter * skybox_projection * view,
        )
    });
    *frame = SourceViewFrame {
        session: sim.frame.state.session,
        selection_revision: sim.frame.state.selection_revision,
        scale: scale.0,
        shadow_quality: *shadow_quality,
        preparation_warmup: warmup,
        scene: epoch.0,
        motion_revision: sim.frame.state.motion_revision,
        probe_enabled: activation.enabled,
        probe_position: scene.probe_position,
        probe_checks: activation.checks,
        probe_index: clock.probe_index,
        probe_distance_squared: activation.distance_squared,
        sky_clip_from_world: sky_clips.map(|(camera, _)| camera),
        skybox_clip_from_world: sky_clips.map(|(_, skybox)| skybox),
        frame: clock.frame,
        main_token: clock.main_token,
        probe_token: clock.probe_token,
        probe_job: job,
        views: snapshots,
        main_motion,
    };
    let decision = {
        let mut state = graph.lock();
        state.proof.snapshot_loading(
            frame.readiness_request().unwrap(),
            frame.frame,
            drive.ready || sim.frame.state.flags & StateBlock::STARTED != 0 || requests.pending(),
        )
    };
    if !decision.captures_allowed || decision.restart_scene {
        frame.probe_job = None;
    }
    if decision.restart_scene {
        commands.queue(restart_scene);
    }
}
/// Replace Bevy's frusta after its update, before either camera visibility system runs.
pub fn publish_frusta(
    frame: Res<SourceViewFrame>,
    mut views: Query<(&SourceSurfaceView, &mut bevy::camera::primitives::Frustum)>,
) {
    for (view, mut frustum) in &mut views {
        if let Some(value) = frame.views.get(view.index).copied().flatten() {
            for (output, p) in frustum
                .half_spaces
                .iter_mut()
                .zip(value.source_camera_planes)
            {
                *output =
                    bevy::math::primitives::HalfSpace::new(Vec4::new(p[0], p[1], -p[2], p[3]));
            }
        }
    }
}
/// Runs before render preparation. No pose, projection, or extent is recomputed here.
pub fn apply_render_snapshot(
    frame: Res<SourceViewFrame>,
    mut views: Query<(&mut SourceSurfaceView, &mut ExtractedView)>,
) {
    for (mut state, mut view) in &mut views {
        if let Some(snapshot) = frame.views.get(state.index).copied().flatten() {
            view.world_from_view = GlobalTransform::from(snapshot.view_from_world.inverse());
            view.clip_from_view = snapshot.raster_projection;
            view.clip_from_world = Some(snapshot.raster_projection * snapshot.view_from_world);
            // World and mirror poses are converted transforms. Probe faces are source matrices.
            view.invert_culling = !probe_view(state.index);
            view.viewport = UVec4::new(0, 0, snapshot.extent.x, snapshot.extent.y);
            state.clip_from_world = snapshot.raster_projection * snapshot.view_from_world;
            state.near = snapshot.near;
            state.far = snapshot.far;
        }
    }
}
/// Explicit graph setup. Final application registration must also install source restart semantics.
pub fn install(app: &mut App) {
    use bevy::camera::visibility::VisibilitySystems;
    use bevy::render::{Render, RenderApp, RenderSystems};
    let acks = SourceSubmissionAcks::default();
    app.insert_resource(acks.clone())
        .init_resource::<SourceViewClock>()
        .init_resource::<SourceSceneEpoch>()
        .init_resource::<super::lighting_scene::ProbeActivation>()
        .init_resource::<super::reflections::Reflections>()
        .init_resource::<SourceViewFrame>()
        .init_resource::<super::temporal::TemporalView>()
        .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
            SourceViewFrame,
        >::default())
        .add_systems(Update, super::reflections::poll)
        .add_systems(
            PostUpdate,
            sync_probe_cameras
                .after(super::lighting_scene::sync_lighting)
                .before(bevy::camera::CameraUpdateSystems),
        )
        .add_systems(
            PostUpdate,
            snapshot
                .after(bevy::camera::CameraUpdateSystems)
                .after(bevy::transform::TransformSystems::Propagate)
                .after(super::lighting_scene::sync_lighting)
                .before(VisibilitySystems::UpdateFrusta),
        )
        .add_systems(
            PostUpdate,
            publish_frusta
                .after(VisibilitySystems::UpdateFrusta)
                .before(VisibilitySystems::CheckVisibility),
        );
    app.sub_app_mut(RenderApp)
        .insert_resource(acks)
        .add_systems(
            Render,
            (apply_render_snapshot, apply_sky_snapshot)
                .chain()
                .after(RenderSystems::ExtractCommands)
                .before(RenderSystems::PrepareAssets),
        );
}

impl SourceSubmissionAcks {
    /// Call after the validated complete cube copy enters its submission.
    pub fn probe_publication_submitted(&self, token: u32) {
        self.0.publication.fetch_max(token, Ordering::Release);
        self.probe_submitted(token);
    }
}

impl SourceViewClock {
    /// Retained GPU resources require monotonic tokens across source scene recreation.
    pub fn restart(&mut self, acks: &SourceSubmissionAcks) -> Result<(), &'static str> {
        self.main_token = self
            .main_token
            .checked_add(1)
            .ok_or("Source main render token exhausted")?;
        self.probe_token = self
            .probe_token
            .checked_add(1)
            .ok_or("Source probe render token exhausted")?;
        self.probe = std::array::from_fn(|_| ProbeSchedule::default());
        self.probe_index = 0;
        self.preparation_ready = false;
        acks.0.main.store(0, Ordering::Release);
        acks.0.probe.store(0, Ordering::Release);
        acks.0.publication.store(0, Ordering::Release);
        Ok(())
    }
    /// The clock stores no world-space history. Preserve probe job order.
    pub fn rebase(&mut self, delta: Vec3) -> Result<(), &'static str> {
        if !delta.is_finite() {
            return Err("Source view rebase delta is invalid");
        }
        Ok(())
    }
}
pub fn apply_sky_snapshot(
    frame: Res<SourceViewFrame>,
    mut views: Query<&mut ExtractedView, With<super::frame_graph::SourceSkyCamera>>,
) {
    if let Some(clip) = frame.sky_clip_from_world {
        let Some(main) = frame.views[0] else { return };
        for mut view in &mut views {
            view.clip_from_view = clip * view.world_from_view.to_matrix();
            view.clip_from_world = Some(clip);
            view.invert_culling = true;
            view.viewport = UVec4::new(0, 0, main.extent.x, main.extent.y);
        }
    }
}

/// Shared source projection and culling math for live and private candidate views.
pub(crate) fn camera_snapshot(
    extent: UVec2,
    view: Mat4,
    near: f32,
    far: f32,
    fov: f32,
    mask: u32,
    distance: f32,
) -> ViewSnapshot {
    let implicit = Mat4::perspective_rh_gl(
        fov.to_radians(),
        extent.x as f32 / extent.y as f32,
        near,
        far,
    );
    let source_view = Mat4::from_cols_array_2d(&crate::reference::view_math::source_culling_view(
        view.to_cols_array_2d(),
    ));
    let source_world_from_view = source_view.inverse();
    // GetScriptableCullingParameters450659..450663 sets shadowNearOffset=0.
    // This is separate from the quality projection near_offset.
    let CullPlanes { camera, shadow } = implicit_cull_planes(
        (implicit * source_view).to_cols_array(),
        source_world_from_view.to_cols_array(),
        near,
        far,
        distance,
        0.,
    );
    ViewSnapshot {
        extent,
        native_extent: extent,
        view_from_world: view,
        implicit_projection: implicit,
        raster_projection: gpu_projection(implicit),
        source_world_from_view,
        source_camera_planes: camera,
        source_shadow_planes: shadow,
        frustum_points: corners(implicit.inverse()),
        near,
        far,
        fov_degrees: fov,
        mask,
        shadow_distance: distance,
    }
}

pub(crate) fn required_view(
    snapshot: &ViewSnapshot,
    origin: [f64; 3],
    light_forward: [f32; 3],
    light_mask: u32,
) -> Result<crate::world::RequiredView, crate::reference::ReferenceError> {
    Ok(crate::world::RequiredView {
        camera: snapshot.source_camera_planes,
        shadow: crate::reference::shadow_candidates::DirectionalHull::new(
            snapshot.source_shadow_planes,
            light_forward,
        )
        .map_err(|e| crate::reference::ReferenceError(e.into()))?,
        mask: snapshot.mask,
        light_mask,
        origin,
    })
}

/// Live and private candidates use the same complete probe frontier and source camera math.
pub(crate) fn append_probe_required_views(
    views: &mut crate::world::RequiredViews,
    records: &[crate::reference::render_lighting::ProbeRecord],
    positions: &[Vec3; PROBE_COUNT],
    enabled: &[bool; PROBE_COUNT],
    origin: [f64; 3],
    light_forward: [f32; 3],
    light_mask: u32,
) -> Result<(), crate::reference::ReferenceError> {
    const _: () = assert!(crate::world::REQUIRED_VIEW_CAPACITY == WORLD_VIEWS + PROBE_COUNT * 6);
    // Every car probe shares the capture settings of the one probe record.
    let probe = &records[0];
    views.append_probe_faces(enabled, |index, face| {
        let snapshot = camera_snapshot(
            UVec2::splat(probe.resolution),
            Mat4::from_cols_array_2d(&capture_view_from_world(face, positions[index].to_array())),
            probe.near,
            probe.far,
            90.,
            probe.mask,
            probe.shadow_distance,
        );
        required_view(&snapshot, origin, light_forward, light_mask)
    })
}
