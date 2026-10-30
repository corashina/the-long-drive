//! Complete renderer proof and loading policy.
use crate::render_scale::SceneScale;
use std::sync::Arc;

/// Fixed live probe slots. The nearest enabled probes take them.
pub const PROBE_COUNT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentKey {
    pub session: u64,
    pub selection_revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentPreparation {
    Pending(ContentKey),
    Prepared(ContentKey),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderIdentity {
    pub session: u64,
    pub scene: u32,
    pub native_extent: [u32; 2],
    pub scene_extent: [u32; 2],
    pub scale: SceneScale,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadinessRequest {
    pub render: RenderIdentity,
    pub selection_revision: u64,
    pub probe_mask: u32,
    pub history_revision: u32,
}
impl ReadinessRequest {
    fn content(self) -> ContentKey {
        ContentKey {
            session: self.render.session,
            selection_revision: self.selection_revision,
        }
    }
    fn capture(self) -> CaptureKey {
        CaptureKey {
            session: self.render.session,
            scene: self.render.scene,
            extent: self.render.scene_extent,
            native_extent: self.render.native_extent,
            scale: self.render.scale,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadingDecision {
    pub captures_allowed: bool,
    pub restart_scene: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RendererStatus {
    Loading,
    Ready,
    Failed(Arc<str>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalidation {
    Content,
    Scene,
    Extent,
    Scale,
    History,
    Failure,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Validation {
    pub requested: u32,
    pub completed: u32,
    pub validated: u32,
}
impl Validation {
    fn passed(self) -> bool {
        self.requested != 0 && self.completed >= self.requested && self.validated >= self.requested
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeObservation {
    pub pipelines_ready: bool,
    pub published: bool,
    pub validation: Validation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphObservation {
    pub identity: RenderIdentity,
    pub frame: u32,
    pub token: u32,
    pub sky: bool,
    pub world_mask: u16,
    pub display_encoded: bool,
    pub submitted_token: u32,
    pub targets_ready: bool,
    pub lighting_textures: u32,
    pub lighting_gpu_textures: u32,
    pub surface_ready: bool,
    pub camera_ready: bool,
    pub sky_ready: bool,
    pub cloud_ready: bool,
    pub caster_ready: bool,
    pub probes: [ProbeObservation; PROBE_COUNT],
    pub validation: Validation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectsObservation {
    pub identity: RenderIdentity,
    pub history_revision: u32,
    pub frame: u32,
    pub token: u32,
    pub pipelines_ready: bool,
    pub inputs_ready: bool,
    pub encoded_token: u32,
    pub submitted_token: u32,
    pub committed_token: u32,
    pub validation: Validation,
}
#[derive(Default)]
pub struct RendererReadiness {
    request: Option<ReadinessRequest>,
    content: Option<ContentPreparation>,
    graph: Option<GraphObservation>,
    effects: Option<EffectsObservation>,
    loading: LoadingPolicy<PROBE_COUNT>,
    presentations: PresentationCount,
    failure: Option<Arc<str>>,
}
impl RendererReadiness {
    pub fn invalidate_submissions(&mut self) {
        self.presentations.reset();
    }
    pub fn invalidate_frame(&mut self) {
        self.graph = None;
        self.effects = None;
        self.invalidate_submissions();
    }
    pub fn content_checked(&mut self, content: ContentPreparation) {
        if self.content != Some(content) {
            self.invalidate(Invalidation::Content);
        }
        self.content = Some(content);
    }
    pub fn snapshot_loading(
        &mut self,
        request: ReadinessRequest,
        frame: u32,
        bypass: bool,
    ) -> LoadingDecision {
        if let Some(old) = self.request.filter(|old| *old != request) {
            let cause = if old.render.session != request.render.session
                || old.render.scene != request.render.scene
            {
                Invalidation::Scene
            } else if old.render.scale != request.render.scale {
                Invalidation::Scale
            } else if old.render.native_extent != request.render.native_extent
                || old.render.scene_extent != request.render.scene_extent
            {
                Invalidation::Extent
            } else if old.history_revision != request.history_revision {
                Invalidation::History
            } else {
                Invalidation::Content
            };
            self.invalidate(cause);
        }
        self.request = Some(request);
        let prepared = self.content == Some(ContentPreparation::Prepared(request.content()))
            && self.failure.is_none();
        let restart_scene = self.loading.snapshot(
            request.capture(),
            frame,
            bypass,
            prepared,
            request.selection_revision,
            request.probe_mask,
        );
        LoadingDecision {
            captures_allowed: self.failure.is_none() && self.loading.captures_allowed(),
            restart_scene,
        }
    }
    pub fn allows_frame(&self, identity: RenderIdentity, frame: u32) -> bool {
        self.failure.is_none()
            && self.loading.allows(
                CaptureKey {
                    session: identity.session,
                    scene: identity.scene,
                    extent: identity.scene_extent,
                    native_extent: identity.native_extent,
                    scale: identity.scale,
                },
                frame,
            )
    }
    pub fn capture_started(&mut self, request: ReadinessRequest, frame: u32, probe: usize) {
        if self.request.is_some_and(|current| {
            current.render == request.render
                && current.selection_revision == request.selection_revision
                && current.probe_mask == request.probe_mask
        }) {
            self.loading
                .capture_started(request.capture(), frame, probe);
        }
    }
    pub fn publication_validated(&mut self, request: ReadinessRequest, probe: usize) {
        if self.request.is_some_and(|current| {
            current.render == request.render
                && current.selection_revision == request.selection_revision
                && current.probe_mask == request.probe_mask
        }) {
            self.loading.publication_validated(request.capture(), probe);
        }
    }
    pub fn observe_graph(&mut self, observation: GraphObservation) {
        self.graph = Some(observation);
    }
    pub fn observe_effects(&mut self, observation: EffectsObservation) {
        self.effects = Some(observation);
    }
    pub fn invalidate(&mut self, cause: Invalidation) {
        self.invalidate_submissions();
        match cause {
            Invalidation::History => self.effects = None,
            Invalidation::Content | Invalidation::Failure => self.loading.invalidate(),
            Invalidation::Scene | Invalidation::Extent | Invalidation::Scale => {
                self.graph = None;
                self.effects = None;
                self.loading.invalidate();
            }
        }
    }
    pub fn fail(&mut self, error: String) {
        if self.failure.is_none() {
            self.failure = Some(Arc::from(error));
        }
        self.invalidate(Invalidation::Failure);
    }
    pub fn failure(&self) -> Option<Arc<str>> {
        self.failure.clone()
    }
    pub fn status(&self) -> RendererStatus {
        if let Some(error) = self.failure() {
            return RendererStatus::Failed(error);
        }
        if self.presentations.count >= 2
            && self
                .request
                .is_some_and(|request| self.graph_ready(request))
        {
            RendererStatus::Ready
        } else {
            RendererStatus::Loading
        }
    }
    pub fn graph_ready(&self, request: ReadinessRequest) -> bool {
        let (Some(graph), Some(effects)) = (self.graph, self.effects) else {
            return false;
        };
        self.failure.is_none()
            && self.content == Some(ContentPreparation::Prepared(request.content()))
            && self.request == Some(request)
            && self.loading.fresh(
                request.capture(),
                request.selection_revision,
                request.probe_mask,
            )
            && graph.identity == request.render
            && effects.identity == request.render
            && graph.frame != 0
            && graph.token != 0
            && graph.sky
            && graph.world_mask == 1
            && graph.display_encoded
            && graph.submitted_token == graph.token
            && graph.targets_ready
            && graph.lighting_textures != 0
            && graph.lighting_gpu_textures == graph.lighting_textures
            && graph.surface_ready
            && graph.camera_ready
            && graph.sky_ready
            && graph.cloud_ready
            && graph.caster_ready
            && graph.validation.passed()
            && graph.probes.iter().enumerate().all(|(index, probe)| {
                probe.pipelines_ready
                    && (request.probe_mask & (1 << index) == 0
                        || (probe.published && probe.validation.passed()))
            })
            && effects.history_revision == request.history_revision
            && effects.frame == graph.frame
            && effects.token == graph.token
            && effects.pipelines_ready
            && effects.inputs_ready
            && effects.encoded_token == graph.token
            && effects.submitted_token == graph.token
            && effects.committed_token == graph.token
            && effects.validation.passed()
    }
    pub fn check(
        &mut self,
        request: ReadinessRequest,
        issued: u32,
        submitted: u32,
    ) -> RendererStatus {
        if let Some(error) = self.failure() {
            return RendererStatus::Failed(error);
        }
        if !self.graph_ready(request) {
            self.invalidate_submissions();
            return RendererStatus::Loading;
        }
        if self.presentations.observe(
            request.render.session,
            request.render.scene,
            issued,
            submitted,
        ) {
            RendererStatus::Ready
        } else {
            RendererStatus::Loading
        }
    }
}

impl serde::Serialize for RendererReadiness {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.loading.serialize(serializer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct CaptureKey {
    session: u64,
    scene: u32,
    extent: [u32; 2],
    #[serde(skip)]
    native_extent: [u32; 2],
    #[serde(skip)]
    scale: SceneScale,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
enum Mode {
    #[default]
    Cadenced,
    Full,
    Bypass,
}
#[derive(Clone, Copy, Debug)]
struct LoadingPolicy<const N: usize> {
    mode: Mode,
    key: Option<CaptureKey>,
    decision_frame: u32,
    content: Option<(u64, u32, u32)>,
    capture_start: [u32; N],
    published_start: [u32; N],
}
impl<const N: usize> Default for LoadingPolicy<N> {
    fn default() -> Self {
        Self {
            mode: Mode::Cadenced,
            key: None,
            decision_frame: 0,
            content: None,
            capture_start: [0; N],
            published_start: [0; N],
        }
    }
}
impl<const N: usize> LoadingPolicy<N> {
    fn snapshot(
        &mut self,
        key: CaptureKey,
        frame: u32,
        bypass: bool,
        content: bool,
        revision: u64,
        enabled: u32,
    ) -> bool {
        let restart = !bypass
            && self.mode == Mode::Full
            && self.key.is_some_and(|previous| previous.scene == key.scene)
            && (self.key != Some(key)
                || !content
                || self
                    .content
                    .is_none_or(|(r, mask, _)| r != revision || mask != enabled));
        if self.key != Some(key) {
            self.key = Some(key);
            self.content = None;
            self.capture_start = [0; N];
            self.published_start = [0; N];
            if self.mode != Mode::Bypass {
                self.mode = Mode::Cadenced;
            }
        }
        self.decision_frame = frame;
        if bypass {
            self.mode = Mode::Bypass;
            self.content = None;
            return false;
        }
        if self.mode == Mode::Bypass {
            return false;
        }
        if !content {
            self.content = None;
            return restart;
        }
        self.mode = Mode::Full;
        if self
            .content
            .is_none_or(|(r, mask, _)| r != revision || mask != enabled)
        {
            self.content = Some((revision, enabled, frame));
        }
        restart
    }
    fn captures_allowed(&self) -> bool {
        self.mode == Mode::Bypass || self.content.is_some()
    }
    fn allows(&self, key: CaptureKey, frame: u32) -> bool {
        self.mode != Mode::Cadenced
            || self.key != Some(key)
            || self.decision_frame != frame
            || frame % 4 == 0
    }
    fn invalidate(&mut self) {
        self.content = None;
    }
    fn capture_started(&mut self, key: CaptureKey, frame: u32, index: usize) {
        if self.key == Some(key) {
            self.capture_start[index] = frame;
        }
    }
    fn publication_validated(&mut self, key: CaptureKey, index: usize) {
        if self.key == Some(key) {
            self.published_start[index] = self.capture_start[index];
        }
    }
    fn fresh(&self, key: CaptureKey, revision: u64, enabled: u32) -> bool {
        if self.key != Some(key) {
            return false;
        }
        if self.mode == Mode::Bypass {
            return true;
        }
        self.mode == Mode::Full
            && self.key == Some(key)
            && self.content.is_some_and(|(r, mask, floor)| {
                r == revision
                    && mask == enabled
                    && self.published_start.iter().enumerate().all(|(i, start)| {
                        enabled & (1 << i) == 0 || (*start != 0 && *start >= floor)
                    })
            })
    }
}
impl<const N: usize> serde::Serialize for LoadingPolicy<N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut audit = serializer.serialize_struct("LoadingPolicy", 9)?;
        audit.serialize_field("mode", &self.mode)?;
        audit.serialize_field("key", &self.key)?;
        audit.serialize_field("decision_frame", &self.decision_frame)?;
        audit.serialize_field("required_revision", &self.content.map(|v| v.0))?;
        audit.serialize_field("enabled_mask", &self.content.map(|v| v.1))?;
        audit.serialize_field("minimum_capture_frame", &self.content.map(|v| v.2))?;
        audit.serialize_field("capture_start", &self.capture_start.as_slice())?;
        audit.serialize_field("published_start", &self.published_start.as_slice())?;
        audit.serialize_field(
            "fresh_complete_scene",
            &self
                .key
                .zip(self.content)
                .is_some_and(|(key, (revision, enabled, _))| self.fresh(key, revision, enabled)),
        )?;
        audit.end()
    }
}

#[derive(Default)]
struct PresentationCount {
    scene: Option<(u64, u32)>,
    last: u32,
    count: u8,
}
impl PresentationCount {
    fn reset(&mut self) {
        *self = Self::default();
    }
    /// Start above every token issued before this complete scene became ready.
    fn observe(&mut self, session: u64, generation: u32, issued: u32, submitted: u32) -> bool {
        if self.scene != Some((session, generation)) {
            self.scene = Some((session, generation));
            self.last = issued;
            self.count = 0;
        }
        if submitted > self.last {
            self.last = submitted;
            self.count = self.count.saturating_add(1).min(2);
        }
        self.count >= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> CaptureKey {
        CaptureKey {
            session: 1,
            scene: 1,
            extent: [800, 600],
            native_extent: [800, 600],
            scale: SceneScale::default(),
        }
    }
    #[test]
    fn startup_probes_wait_for_complete_content_then_publish_once() {
        use crate::reference::reflections::{ProbeJob, ProbeSchedule};
        let k = key();
        let mut policy = LoadingPolicy::<2>::default();
        let mut probes = [ProbeSchedule::default(), ProbeSchedule::default()];
        for frame in 1..=8 {
            assert!(!policy.snapshot(k, frame, false, false, 1, 3));
            assert!(!policy.captures_allowed());
            assert_eq!(policy.allows(k, frame), frame % 4 == 0);
            assert!(probes.iter().all(|probe| probe.job() == ProbeJob::Face(0)));
        }
        let mut frame = 9;
        for (index, probe) in probes.iter_mut().enumerate() {
            for _ in 0..8 {
                assert!(!policy.snapshot(k, frame, false, true, 1, 3));
                assert!(policy.captures_allowed());
                let job = probe.job();
                if job == ProbeJob::Face(0) {
                    policy.capture_started(k, frame, index);
                }
                probe.complete(job).unwrap();
                if probe.published() {
                    policy.publication_validated(k, index);
                }
                frame += 1;
            }
        }
        assert!(probes.iter().all(ProbeSchedule::published));
        assert!(policy.fresh(k, 1, 3));
    }
    #[test]
    fn loading_changes_restart_both_capture_owners_but_bypass_does_not() {
        let k = key();
        let resized = CaptureKey {
            extent: [900, 600],
            ..k
        };
        for (next, content, revision, mask) in [
            (resized, true, 1, 3),
            (k, true, 2, 3),
            (k, true, 1, 1),
            (k, false, 1, 3),
        ] {
            let mut p = LoadingPolicy::<2>::default();
            assert!(!p.snapshot(k, 1, false, true, 1, 3));
            p.capture_started(k, 1, 0);
            assert!(p.snapshot(next, 2, false, content, revision, mask));
            assert!(!p.snapshot(
                CaptureKey { scene: 2, ..next },
                3,
                false,
                content,
                revision,
                mask
            ));
        }
        let mut p = LoadingPolicy::<2>::default();
        p.snapshot(k, 1, false, true, 1, 3);
        p.invalidate();
        assert!(!p.captures_allowed());
        assert!(p.snapshot(k, 2, false, true, 1, 3));
        assert!(!p.snapshot(k, 3, true, false, 1, 3));
        assert!(!p.snapshot(resized, 4, false, false, 2, 1));
        assert!(p.captures_allowed());
        assert!(p.fresh(resized, 2, 1));
    }
    #[test]
    fn keys_masks_failures_and_later_presentations_reset_the_proof() {
        let k = key();
        let mut p = LoadingPolicy::<2>::default();
        p.snapshot(k, 4, false, true, 1, 1);
        p.capture_started(k, 4, 0);
        p.publication_validated(k, 0);
        assert!(p.fresh(k, 1, 1));
        let mut frames = PresentationCount::default();
        assert!(!frames.observe(k.session, k.scene, 8, 8));
        assert!(!frames.observe(k.session, k.scene, 8, 8));
        assert!(!frames.observe(k.session, k.scene, 9, 9));
        assert!(frames.observe(k.session, k.scene, 10, 10));
        p.snapshot(k, 5, false, true, 1, 3);
        assert!(!p.fresh(k, 1, 3));
        p.capture_started(k, 5, 0);
        p.publication_validated(k, 0);
        p.capture_started(k, 6, 1);
        p.publication_validated(k, 1);
        assert!(p.fresh(k, 1, 3));
        let resized = CaptureKey {
            extent: [900, 600],
            ..k
        };
        assert!(!p.fresh(resized, 1, 3));
        assert!(p.allows(resized, 7));
        p.snapshot(resized, 7, false, true, 1, 3);
        assert!(!p.fresh(resized, 1, 3));
        p.capture_started(k, 8, 0);
        p.publication_validated(k, 0);
        assert!(!p.fresh(resized, 1, 3));
        p.snapshot(resized, 8, false, true, 1, 0);
        assert!(p.fresh(resized, 1, 0));
        p.invalidate();
        assert!(!p.fresh(resized, 1, 0));
        assert!(p.allows(resized, 9));
        for next in [
            CaptureKey {
                scene: 2,
                ..resized
            },
            CaptureKey {
                session: 2,
                ..resized
            },
        ] {
            p.snapshot(next, 9, false, false, 1, 3);
            assert!(!p.fresh(next, 1, 3));
            assert!(!p.allows(next, 9));
        }
    }
    #[test]
    fn loading_cadence_and_fresh_publications_use_snapshot_frames() {
        let k = key();
        let mut p = LoadingPolicy::<2>::default();
        for frame in 1..=4 {
            p.snapshot(k, frame, false, false, 2, 3);
            assert_eq!(p.allows(k, frame), frame == 4);
            assert!(p.allows(k, frame + 1));
        }
        p.capture_started(k, 4, 0);
        p.snapshot(k, 5, false, true, 2, 3);
        assert!(p.allows(k, 5));
        p.publication_validated(k, 0);
        assert!(!p.fresh(k, 2, 3));
        p.capture_started(k, 6, 0);
        p.publication_validated(k, 0);
        assert!(!p.fresh(k, 2, 3));
        p.capture_started(k, 7, 1);
        p.publication_validated(k, 1);
        assert!(p.fresh(k, 2, 3));
        p.snapshot(k, 8, false, false, 2, 3);
        assert!(p.allows(k, 8));
        assert!(!p.fresh(k, 2, 3));
        p.snapshot(k, 9, false, false, 2, 3);
        assert!(p.allows(k, 9));
        p.snapshot(k, 10, false, true, 3, 1);
        assert!(!p.fresh(k, 3, 1));
        p.capture_started(k, 10, 0);
        p.publication_validated(k, 0);
        assert!(p.fresh(k, 3, 1));
        p.invalidate();
        assert!(!p.fresh(k, 3, 1));
        p.snapshot(k, 11, true, false, 3, 1);
        let resized = CaptureKey {
            extent: [900, 600],
            ..k
        };
        p.snapshot(resized, 12, false, false, 3, 1);
        assert!(p.allows(resized, 12));
        assert!(p.fresh(resized, 3, 1));
    }

    #[test]
    fn readiness_requires_two_distinct_new_submissions() {
        let mut frames = PresentationCount::default();
        assert!(!frames.observe(11, 3, 9, 8));
        assert!(!frames.observe(11, 3, 9, 8));
        assert!(!frames.observe(11, 3, 10, 10));
        assert!(!frames.observe(11, 3, 10, 10));
        assert!(frames.observe(11, 3, 11, 11));
        frames.reset();
        assert!(!frames.observe(11, 3, 11, 11));
        assert!(!frames.observe(12, 4, 14, 12));
        assert!(!frames.observe(12, 4, 14, 13));
        assert!(!frames.observe(12, 4, 15, 15));
        assert!(frames.observe(12, 4, 16, 16));
    }
}
