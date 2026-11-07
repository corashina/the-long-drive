//! Cached source ForwardBase and ForwardAdd pipelines. Transparent dispatch uses source pass order.
//! An opaque base also gets a depth prepass pipeline and an Equal pipeline for the world view.
use super::readiness::RendererStateHandle;
use bevy::ecs::{
    query::ROQueryItem,
    system::{SystemParamItem, lifetimeless::SRes},
};
use bevy::render::{
    render_phase::{
        CachedRenderPipelinePhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass,
    },
    render_resource::*,
};
use bevy::shader::{ShaderCacheError, ShaderDefVal};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Hidden bench toggle (`?prepass=0`). Set it before `run`: pipelines queue at load.
static DEPTH_PREPASS: AtomicBool = AtomicBool::new(true);
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn set_depth_prepass(on: bool) {
    DEPTH_PREPASS.store(on, Ordering::Relaxed);
}
/// The world view draws a depth prepass, then the opaque pass with Equal depth.
pub(crate) fn depth_prepass_enabled() -> bool {
    DEPTH_PREPASS.load(Ordering::Relaxed)
}

/// The last skipped surface draw: reason in the high bits, pipeline id in the low bits.
static LAST_SKIP: AtomicU64 = AtomicU64::new(0);
fn skip(reason: u64, pipeline: CachedRenderPipelineId) -> RenderCommandResult {
    LAST_SKIP.store(reason << 32 | pipeline.id() as u64, Ordering::Relaxed);
    RenderCommandResult::Skip
}
/// Why the last source surface draw was skipped, for the skipped-draw failure.
pub fn last_skip(cache: &PipelineCache) -> String {
    let value = LAST_SKIP.load(Ordering::Relaxed);
    let reason = match value >> 32 {
        0 => return "bind group or mesh not ready".into(),
        1 => "no pass partner",
        2 => "base pipeline",
        3 => "additive pipeline",
        4 => "additive partner",
        _ => "prepass pipeline",
    };
    let id = (value & 0xffff_ffff) as usize;
    let state = match cache.get_render_pipeline_state(CachedRenderPipelineId::new(id)) {
        CachedPipelineState::Queued => "queued".into(),
        CachedPipelineState::Creating(_) => "creating".into(),
        CachedPipelineState::Ok(_) => "ok".into(),
        CachedPipelineState::Err(error) => format!("error {error}"),
    };
    format!("{reason} {id} {state}")
}

/// The companion pipelines of one base.
#[derive(Clone, Copy, Default)]
struct Companions {
    /// ForwardAdd.
    add: Option<CachedRenderPipelineId>,
    /// Depth prepass, then the opaque draw with Equal depth and no depth write. Only the world view uses them.
    prepass: Option<[CachedRenderPipelineId; 2]>,
}
impl Companions {
    fn ids(self) -> impl Iterator<Item = CachedRenderPipelineId> {
        self.add
            .into_iter()
            .chain(self.prepass.into_iter().flatten())
    }
}
#[derive(Resource, Default)]
pub struct SurfacePasses {
    inspected: usize,
    /// Pipeline count when every base and companion was last Ok. A new pipeline clears it.
    settled: usize,
    partners: HashMap<CachedRenderPipelineId, Companions>,
}
impl SurfacePasses {
    pub fn companion_state(
        &self,
        base: CachedRenderPipelineId,
        cache: &PipelineCache,
    ) -> Option<Result<bool, String>> {
        self.partners.get(&base).map(|companions| {
            let mut ready = true;
            for id in companions.ids() {
                match cache.get_render_pipeline_state(id) {
                    CachedPipelineState::Ok(_) => {}
                    CachedPipelineState::Err(
                        ShaderCacheError::ShaderNotLoaded(_)
                        | ShaderCacheError::ShaderImportNotYetAvailable,
                    ) => ready = false,
                    CachedPipelineState::Err(error) => {
                        return Err(format!("Source surface companion pipeline: {error}"));
                    }
                    _ => ready = false,
                }
            }
            Ok(ready)
        })
    }
    /// The additive pipeline of an inspected base.
    pub fn partner(&self, base: CachedRenderPipelineId) -> Option<CachedRenderPipelineId> {
        self.partners
            .get(&base)
            .and_then(|companions| companions.add)
    }
    /// The depth prepass and Equal pipelines of an inspected opaque base.
    pub fn prepass(&self, base: CachedRenderPipelineId) -> Option<[CachedRenderPipelineId; 2]> {
        self.partners
            .get(&base)
            .and_then(|companions| companions.prepass)
    }
    pub fn counts(&self) -> (usize, usize) {
        (
            self.partners.len(),
            self.partners
                .values()
                .filter(|value| value.add.is_some())
                .count(),
        )
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        // A base that the inspection has not seen has no partner yet.
        !self.partners.is_empty() && self.settled == cache.pipelines().count()
    }
}
fn flag(definitions: &[ShaderDefVal], name: &str) -> bool {
    definitions
        .iter()
        .any(|value| matches!(value, ShaderDefVal::Bool(key, true) if key == name))
}
/// The same inspection after the pipeline queue. It is a separate system, because one system type
/// cannot sit in both the Prepare and Render sets.
pub(crate) fn inspect_created_surface_passes(
    passes: ResMut<SurfacePasses>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    prepare_surface_passes(passes, cache, status);
}
/// Inspect only newly prepared descriptors. Pipeline clones occur once per new source variant.
pub(crate) fn prepare_surface_passes(
    mut passes: ResMut<SurfacePasses>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if status.failure().is_some() {
        return;
    }
    let first = passes.inspected;
    for (index, pipeline) in cache.pipelines().enumerate().skip(first) {
        passes.inspected = index + 1;
        let base_id = CachedRenderPipelineId::new(index);
        if passes.partners.contains_key(&base_id) {
            continue;
        }
        let PipelineDescriptor::RenderPipelineDescriptor(descriptor) = &pipeline.descriptor else {
            continue;
        };
        let Some(fragment) = &descriptor.fragment else {
            continue;
        };
        if !flag(&fragment.shader_defs, "SOURCE_SURFACE")
            || flag(&fragment.shader_defs, "SOURCE_FORWARD_ADD")
            || flag(&fragment.shader_defs, PREPASS)
        {
            continue;
        }
        let mut companions = Companions::default();
        // Installed Unlit/Color and stained glass have no ForwardAdd pass. Single-pass surfaces include it.
        if flag(&fragment.shader_defs, "SOURCE_STANDARD")
            && !flag(&fragment.shader_defs, "SOURCE_FORWARD_ONE")
        {
            companions.add = Some(cache.queue_render_pipeline(additive(descriptor)));
        }
        if depth_prepass_enabled() && prepass_base(descriptor, &fragment.shader_defs) {
            companions.prepass = Some([
                cache.queue_render_pipeline(depth_prepass(descriptor)),
                cache.queue_render_pipeline(equal_depth(descriptor)),
            ]);
        }
        passes.partners.insert(base_id, companions);
    }
    let count = cache.pipelines().count();
    if passes.settled == count {
        return;
    }
    let mut settled = true;
    for (base, companions) in &passes.partners {
        for id in std::iter::once(*base).chain(companions.ids()) {
            match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Ok(_) => {}
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => settled = false,
                CachedPipelineState::Err(error) => {
                    status.fail(format!(
                        "Source surface {} pipeline: {error}",
                        if id == *base { "base" } else { "companion" }
                    ));
                    return;
                }
                _ => settled = false,
            }
        }
    }
    if settled {
        passes.settled = count;
    }
}
fn additive(base: &RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    let mut add = base.clone();
    add.label = Some("source_surface_add".into());
    add.vertex.shader_defs.extend([
        ShaderDefVal::Bool("SOURCE_FORWARD_ADD".into(), true),
        ShaderDefVal::Bool("SOURCE_LAMPS".into(), true),
    ]);
    let fragment = add.fragment.as_mut().unwrap();
    fragment.shader_defs.retain(
        |value| !matches!(value, ShaderDefVal::Bool(name, _) if name == "SOURCE_REPLACEMENT"),
    );
    fragment.shader_defs.extend([
        ShaderDefVal::Bool("SOURCE_FORWARD_ADD".into(), true),
        ShaderDefVal::Bool("SOURCE_LAMPS".into(), true),
    ]);
    for target in fragment.targets.iter_mut().flatten() {
        let source = target
            .blend
            .map_or(BlendFactor::One, |blend| blend.color.src_factor);
        let blend = BlendComponent {
            src_factor: source,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        target.blend = Some(BlendState {
            color: blend,
            alpha: blend,
        });
        target.write_mask = ColorWrites::ALL;
    }
    add.primitive.cull_mode = Some(Face::Back);
    let depth = add.depth_stencil.as_mut().expect("source surface depth");
    depth.depth_write_enabled = Some(false);
    depth.depth_compare = Some(CompareFunction::GreaterEqual);
    add
}
/// Marks the prepass companions. Inspections skip them as bases.
pub(crate) const PREPASS: &str = "SOURCE_PREPASS";
/// An opaque surface that writes the nearest depth. Its world view draws use the depth prepass.
fn prepass_base(base: &RenderPipelineDescriptor, defs: &[ShaderDefVal]) -> bool {
    flag(defs, "SOURCE_REPLACEMENT")
        && !flag(defs, "SOURCE_TRANSPARENT")
        && !base.multisample.alpha_to_coverage_enabled
        && base.depth_stencil.as_ref().is_some_and(|depth| {
            depth.depth_write_enabled == Some(true)
                && depth.depth_compare == Some(CompareFunction::GreaterEqual)
        })
}
/// It has the same vertex stage and defs. The `depth` fragment runs only the discards of the surface fragment.
fn depth_prepass(base: &RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    let mut depth = base.clone();
    depth.label = Some("source_surface_depth".into());
    let fragment = depth.fragment.as_mut().unwrap();
    fragment.entry_point = Some("depth".into());
    fragment.targets.clear();
    fragment
        .shader_defs
        .push(ShaderDefVal::Bool(PREPASS.into(), true));
    depth
}
/// The opaque draw after the prepass. Each pixel shades once.
fn equal_depth(base: &RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    let mut equal = base.clone();
    equal.label = Some("source_surface_equal".into());
    let fragment = equal.fragment.as_mut().unwrap();
    fragment
        .shader_defs
        .push(ShaderDefVal::Bool(PREPASS.into(), true));
    let depth = equal.depth_stencil.as_mut().expect("source surface depth");
    depth.depth_write_enabled = Some(false);
    depth.depth_compare = Some(CompareFunction::Equal);
    equal
}
/// Prevent a partial pair while its additive pipeline is still being prepared.
pub struct SourcePairReady;
impl<P: CachedRenderPipelinePhaseItem> RenderCommand<P> for SourcePairReady {
    type Param = (SRes<SurfacePasses>, SRes<PipelineCache>);
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        _: ROQueryItem<Self::ViewQuery>,
        _: Option<()>,
        (passes, cache): SystemParamItem<'w, '_, Self::Param>,
        _: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let base = item.cached_pipeline();
        let Some(companions) = passes.partners.get(&base) else {
            return skip(1, base);
        };
        if cache.get_render_pipeline(base).is_none() {
            return skip(2, base);
        }
        if let Some(id) = companions
            .add
            .filter(|id| cache.get_render_pipeline(*id).is_none())
        {
            return skip(3, id);
        }
        if let Some(id) = companions
            .prepass
            .into_iter()
            .flatten()
            .find(|id| cache.get_render_pipeline(*id).is_none())
        {
            return skip(5, id);
        }
        RenderCommandResult::Success
    }
}
/// The depth prepass pipeline. A base without one draws nothing in the prepass.
pub struct SetSourceDepthPipeline;
impl<P: CachedRenderPipelinePhaseItem> RenderCommand<P> for SetSourceDepthPipeline {
    type Param = (SRes<SurfacePasses>, SRes<PipelineCache>);
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        _: ROQueryItem<Self::ViewQuery>,
        _: Option<()>,
        (passes, cache): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(pipeline) = passes
            .prepass(item.cached_pipeline())
            .and_then(|[depth, _]| cache.into_inner().get_render_pipeline(depth))
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_render_pipeline(pipeline);
        RenderCommandResult::Success
    }
}
/// The Equal pipeline after the prepass, else the base pipeline.
pub struct SetSourceEqualPipeline;
impl<P: CachedRenderPipelinePhaseItem> RenderCommand<P> for SetSourceEqualPipeline {
    type Param = (SRes<SurfacePasses>, SRes<PipelineCache>);
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        _: ROQueryItem<Self::ViewQuery>,
        _: Option<()>,
        (passes, cache): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let base = item.cached_pipeline();
        let id = passes.prepass(base).map_or(base, |[_, equal]| equal);
        let Some(pipeline) = cache.into_inner().get_render_pipeline(id) else {
            return skip(5, id);
        };
        pass.set_render_pipeline(pipeline);
        RenderCommandResult::Success
    }
}
pub struct SetSourceAdditivePipeline;
impl<P: CachedRenderPipelinePhaseItem> RenderCommand<P> for SetSourceAdditivePipeline {
    type Param = (SRes<SurfacePasses>, SRes<PipelineCache>);
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        _: ROQueryItem<Self::ViewQuery>,
        _: Option<()>,
        (passes, cache): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(id) = passes.partner(item.cached_pipeline()) else {
            return skip(4, item.cached_pipeline());
        };
        let Some(pipeline) = cache.into_inner().get_render_pipeline(id) else {
            return skip(3, id);
        };
        pass.set_render_pipeline(pipeline);
        RenderCommandResult::Success
    }
}
