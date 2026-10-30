//! Persistent source shadow items use Bevy's mesh extraction and instance buffers.
use super::caster::CasterViews;
use super::lamp_shadows::{TILES, tile_bit};
use bevy::ecs as bevy_ecs;
use bevy::ecs::{query::ROQueryItem, system::SystemParamItem};
use bevy::pbr::{DrawMesh, SetMeshBindGroup};
use bevy::prelude::*;
use bevy::render::{
    render_phase::*, render_resource::CachedRenderPipelineId, sync_world::MainEntity,
};
use std::{
    ops::Range,
    sync::atomic::{AtomicU32, Ordering},
};

/// All members use the same draw state and recovered cascade membership.
pub type ShadowBatchKey = super::shadow_batch::ShadowBatchKey<
    CachedRenderPipelineId,
    DrawFunctionId,
    bevy::asset::UntypedAssetId,
>;
impl PhaseItemBatchSetKey for ShadowBatchKey {
    fn indexed(&self) -> bool {
        self.indexed
    }
}
pub struct SourceShadow {
    pub entity: (Entity, MainEntity),
    pub key: ShadowBatchKey,
    pub batch_range: Range<u32>,
    pub extra_index: PhaseItemExtraIndex,
}
impl PhaseItem for SourceShadow {
    fn entity(&self) -> Entity {
        self.entity.0
    }
    fn main_entity(&self) -> MainEntity {
        self.entity.1
    }
    fn draw_function(&self) -> DrawFunctionId {
        self.key.draw
    }
    fn batch_range(&self) -> &Range<u32> {
        &self.batch_range
    }
    fn batch_range_mut(&mut self) -> &mut Range<u32> {
        &mut self.batch_range
    }
    fn extra_index(&self) -> PhaseItemExtraIndex {
        self.extra_index.clone()
    }
    fn batch_range_and_extra_index_mut(&mut self) -> (&mut Range<u32>, &mut PhaseItemExtraIndex) {
        (&mut self.batch_range, &mut self.extra_index)
    }
}
impl CachedRenderPipelinePhaseItem for SourceShadow {
    fn cached_pipeline(&self) -> CachedRenderPipelineId {
        self.key.pipeline
    }
}
impl BinnedPhaseItem for SourceShadow {
    type BatchSetKey = ShadowBatchKey;
    type BinKey = bevy::asset::AssetId<Mesh>;
    fn new(
        key: ShadowBatchKey,
        _: Self::BinKey,
        entity: (Entity, MainEntity),
        batch_range: Range<u32>,
        extra_index: PhaseItemExtraIndex,
    ) -> Self {
        Self {
            entity,
            key,
            batch_range,
            extra_index,
        }
    }
}

/// The atlas stage changes this caster slot selector between draws. Uniform slots remain independent.
#[derive(Component)]
pub struct ShadowDrawState {
    cascade: AtomicU32,
    encoded_frame: AtomicU32,
    epoch: AtomicU32,
    attempts: [AtomicU32; TILES],
    successes: [AtomicU32; TILES],
    instance_attempts: [AtomicU32; TILES],
    instance_successes: [AtomicU32; TILES],
}
impl Default for ShadowDrawState {
    fn default() -> Self {
        let counts = || std::array::from_fn(|_| AtomicU32::new(0));
        Self {
            cascade: AtomicU32::new(0),
            encoded_frame: AtomicU32::new(0),
            epoch: AtomicU32::new(0),
            attempts: counts(),
            successes: counts(),
            instance_attempts: counts(),
            instance_successes: counts(),
        }
    }
}
impl ShadowDrawState {
    pub fn encoded_frame(&self) -> u32 {
        self.encoded_frame.load(Ordering::Acquire)
    }
    pub fn encoded(&self, frame: u32) {
        self.encoded_frame.store(frame, Ordering::Release);
    }
    pub fn begin(&self) {
        self.encoded_frame.store(0, Ordering::Release);
        self.epoch.fetch_add(1, Ordering::Relaxed);
        for count in self
            .attempts
            .iter()
            .chain(self.successes.iter())
            .chain(self.instance_attempts.iter())
            .chain(self.instance_successes.iter())
        {
            count.store(0, Ordering::Relaxed);
        }
    }
    pub fn select(&self, cascade: usize) {
        assert!(cascade < TILES);
        self.cascade.store(cascade as u32, Ordering::Relaxed);
    }
    pub fn instance_counts(&self) -> [[u32; TILES]; 2] {
        [
            self.instance_attempts
                .each_ref()
                .map(|v| v.load(Ordering::Relaxed)),
            self.instance_successes
                .each_ref()
                .map(|v| v.load(Ordering::Relaxed)),
        ]
    }
    pub fn counts(&self) -> (u32, [u32; TILES], [u32; TILES]) {
        (
            self.epoch.load(Ordering::Relaxed),
            self.attempts.each_ref().map(|v| v.load(Ordering::Relaxed)),
            self.successes.each_ref().map(|v| v.load(Ordering::Relaxed)),
        )
    }
}
struct SelectCaster;
impl RenderCommand<SourceShadow> for SelectCaster {
    type Param = ();
    type ViewQuery = (&'static ShadowDrawState, &'static CasterViews);
    type ItemQuery = ();
    fn render<'w>(
        item: &SourceShadow,
        (state, views): ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<()>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let cascade = state.cascade.load(Ordering::Relaxed) as usize;
        if item.key.split_mask & (1 << tile_bit(cascade)) == 0 {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(1, views.binding(cascade), &[]);
        RenderCommandResult::Success
    }
}
struct MarkDraw<const DONE: bool>;
impl<const DONE: bool> RenderCommand<SourceShadow> for MarkDraw<DONE> {
    type Param = ();
    type ViewQuery = &'static ShadowDrawState;
    type ItemQuery = ();
    fn render<'w>(
        item: &SourceShadow,
        state: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<()>,
        _: SystemParamItem<'w, '_, Self::Param>,
        _: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let index = state.cascade.load(Ordering::Relaxed) as usize;
        let counts = if DONE {
            &state.successes
        } else {
            &state.attempts
        };
        counts[index].fetch_add(1, Ordering::Relaxed);
        let instances = if DONE {
            &state.instance_successes
        } else {
            &state.instance_attempts
        };
        instances[index].fetch_add(
            item.batch_range.end - item.batch_range.start,
            Ordering::Relaxed,
        );
        RenderCommandResult::Success
    }
}
type DrawSourceShadow = (
    SetItemPipeline,
    SelectCaster,
    MarkDraw<false>,
    super::source_lighting::SetSourceViewCamera,
    SetMeshBindGroup<2>,
    super::caster_material::SetCasterMaterial,
    DrawMesh,
    MarkDraw<true>,
);

/// Register once during renderer setup; draw-table entries remain unchanged during frames.
pub fn register(app: &mut SubApp) {
    app.init_resource::<DrawFunctions<SourceShadow>>()
        .add_render_command::<SourceShadow, DrawSourceShadow>();
}
pub fn draw_id(functions: &DrawFunctions<SourceShadow>) -> DrawFunctionId {
    functions.read().id::<DrawSourceShadow>()
}
