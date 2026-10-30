//! Renderer proof access, completion validation, and terminal errors.
use super::{
    frame_graph::FrameAudit,
    lighting_scene::ProbeActivation,
    surface_views::SourceSurfaceView,
    view_frame::{SourceSceneEpoch, SourceSubmissionAcks, SourceViewClock, SourceViewFrame},
};
use crate::{render_readiness::*, render_scale::SceneScaleSetting};
use bevy::ecs::system::SystemParam;
use bevy::render::{RenderApp, renderer::RenderQueue};
use bevy::window::PrimaryWindow;
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicU32, Ordering},
};

#[derive(Default)]
pub(crate) struct RendererState {
    pub proof: RendererReadiness,
    pub audit: FrameAudit,
}
#[derive(Resource, Clone, Default)]
pub(crate) struct RendererStateHandle(Arc<Mutex<RendererState>>);
impl RendererStateHandle {
    pub(crate) fn lock(&self) -> MutexGuard<'_, RendererState> {
        self.0.lock().expect("renderer state lock")
    }
    pub(crate) fn failure(&self) -> Option<Arc<str>> {
        self.lock().proof.failure()
    }
    pub(crate) fn fail(&self, error: String) {
        self.lock().proof.fail(error);
    }
}
#[derive(Clone, Copy)]
pub(crate) struct RendererContentCheck {
    pub preparation: ContentPreparation,
    pub history_revision: u32,
}
#[derive(SystemParam)]
pub(crate) struct RendererReadinessAccess<'w, 's> {
    state: Res<'w, RendererStateHandle>,
    epoch: Res<'w, SourceSceneEpoch>,
    clock: Res<'w, SourceViewClock>,
    submissions: Res<'w, SourceSubmissionAcks>,
    activation: Res<'w, ProbeActivation>,
    scale: Res<'w, SceneScaleSetting>,
    cameras: Query<'w, 's, (&'static SourceSurfaceView, &'static Camera)>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
}
impl RendererReadinessAccess<'_, '_> {
    pub(crate) fn failure(&self) -> Option<Arc<str>> {
        self.state.failure()
    }
    pub(crate) fn request(&self, content: RendererContentCheck) -> Option<ReadinessRequest> {
        let (ContentPreparation::Pending(key) | ContentPreparation::Prepared(key)) =
            content.preparation;
        let mut cameras = self.cameras.iter().filter(|(view, _)| view.index == 0);
        let (_, camera) = cameras.next()?;
        if cameras.next().is_some() || !camera.is_active {
            return None;
        }
        let native = camera.physical_viewport_size()?;
        let target = camera.physical_target_size()?;
        let window = self.windows.single().ok()?;
        if native.x == 0
            || native.y == 0
            || native != target
            || window.physical_width() != target.x
            || window.physical_height() != target.y
        {
            return None;
        }
        Some(ReadinessRequest {
            render: RenderIdentity {
                session: key.session,
                scene: self.epoch.0,
                native_extent: native.to_array(),
                scene_extent: self.scale.0.extent(native.to_array()),
                scale: self.scale.0,
            },
            selection_revision: key.selection_revision,
            probe_mask: self
                .activation
                .enabled
                .iter()
                .enumerate()
                .fold(0, |mask, (index, enabled)| {
                    mask | (u32::from(*enabled) << index)
                }),
            history_revision: content.history_revision,
        })
    }
    pub(crate) fn check(&mut self, content: RendererContentCheck) -> RendererStatus {
        let request = self.request(content);
        let mut state = self.state.lock();
        state.proof.content_checked(content.preparation);
        if let Some(error) = state.proof.failure() {
            return RendererStatus::Failed(error);
        }
        let Some(request) = request else {
            state.proof.invalidate_submissions();
            return RendererStatus::Loading;
        };
        state.proof.check(
            request,
            self.clock.main_token(),
            self.submissions.main_token(),
        )
    }
    pub(crate) fn graph_ready(&self, content: RendererContentCheck) -> bool {
        self.request(content)
            .is_some_and(|request| self.state.lock().proof.graph_ready(request))
    }
}
pub(crate) fn control_bytes() -> usize {
    std::alloc::Layout::new::<[usize; 2]>()
        .extend(std::alloc::Layout::new::<Mutex<RendererState>>())
        .unwrap()
        .0
        .pad_to_align()
        .size()
        + 3 * std::mem::size_of::<RendererStateHandle>()
        + 2 * std::mem::size_of::<SourceViewFrame>()
        + std::mem::size_of::<Option<crate::world::RequiredViews>>()
}
pub(crate) fn observe_main_readiness(
    state: Res<RendererStateHandle>,
    materials: Option<Res<super::materials::SourceMaterials>>,
    master: Option<Res<super::lighting::SourceLighting>>,
) {
    if state.failure().is_some() {
        return;
    }
    if let Some(master) = master {
        let status = master.status.lock().expect("master lighting status lock");
        let failure = match &*status {
            super::lighting::RenderingState::Failed(error) => Some(error),
            _ => match &master.state {
                super::lighting::RenderingState::Failed(error) => Some(error),
                _ => None,
            },
        };
        if let Some(error) = failure {
            let error = error.clone();
            drop(status);
            state.fail(error);
            return;
        }
    }
    if let Some(materials) = materials {
        if let super::materials::RenderingState::Failed(error) = &materials.state {
            state.fail(error.clone());
        }
    }
}
pub(crate) fn install(app: &mut App) {
    let state = app.world().resource::<RendererStateHandle>().clone();
    let gate = SourceExecutionGate::default();
    app.insert_resource(gate.clone()).insert_resource(
        bevy::render::error_handler::RenderErrorHandler(source_render_error),
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(state)
        .insert_resource(gate)
        .add_systems(First, validate_completed_submissions);
}
fn source_render_error(
    error: &bevy::render::error_handler::RenderError,
    main: &mut World,
    _: &mut World,
) -> bevy::render::error_handler::RenderErrorPolicy {
    let failure = {
        let mut state = main.resource::<RendererStateHandle>().lock();
        if state.proof.failure().is_none() {
            state.proof.fail(format!(
                "Source rendering {:?}: {}",
                error.ty, error.description
            ));
        }
        state.proof.failure().unwrap()
    };
    if let Some(mut materials) = main.get_resource_mut::<super::materials::SourceMaterials>() {
        materials.state = super::materials::RenderingState::Failed(failure.to_string());
    }
    bevy::render::error_handler::RenderErrorPolicy::StopRendering
}

#[derive(Default)]
struct ExecutionProgress {
    requested: AtomicU32,
    completed: AtomicU32,
    validated: AtomicU32,
}
#[derive(Resource, Clone, Default)]
pub(crate) struct SourceExecutionGate(Arc<ExecutionProgress>);
impl SourceExecutionGate {
    pub fn watch_submission(&self, queue: &RenderQueue) -> u32 {
        let generation = self.0.requested.fetch_add(1, Ordering::AcqRel) + 1;
        assert_ne!(generation, 0, "source execution generation overflow");
        let progress = self.0.clone();
        queue.on_submitted_work_done(move || {
            progress.completed.fetch_max(generation, Ordering::AcqRel);
        });
        generation
    }
    pub fn validated_generation(&self) -> u32 {
        self.0.validated.load(Ordering::Acquire)
    }
    pub fn submission_validated(&self, generation: u32) -> bool {
        generation != 0 && self.validated_generation() >= generation
    }
    pub(crate) fn snapshot(&self, requested: u32) -> Validation {
        Validation {
            requested,
            completed: self.0.completed.load(Ordering::Acquire),
            validated: self.0.validated.load(Ordering::Acquire),
        }
    }
}
fn validate_completed_submissions(gate: Res<SourceExecutionGate>) {
    let requested = gate.0.requested.load(Ordering::Acquire);
    let completed = gate.0.completed.load(Ordering::Acquire).min(requested);
    if completed != 0 {
        gate.0.validated.store(completed, Ordering::Release);
    }
}
