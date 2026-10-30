//! Cached source ForwardBase and ForwardAdd pipelines. Transparent dispatch uses source pass order.
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
use std::sync::atomic::{AtomicU64, Ordering};

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
        _ => "additive partner",
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

#[derive(Resource, Default)]
pub struct SurfacePasses {
    inspected: usize,
    partners: HashMap<CachedRenderPipelineId, Option<CachedRenderPipelineId>>,
}
impl SurfacePasses {
    pub fn revisit(&mut self, id: CachedRenderPipelineId) {
        self.inspected = self.inspected.min(id.id());
    }
    pub fn companion_state(
        &self,
        base: CachedRenderPipelineId,
        cache: &PipelineCache,
    ) -> Option<Result<bool, String>> {
        self.partners.get(&base).map(|partner| {
            let Some(id) = partner else {
                return Ok(true);
            };
            match cache.get_render_pipeline_state(*id) {
                CachedPipelineState::Ok(_) => Ok(true),
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => Ok(false),
                CachedPipelineState::Err(error) => {
                    Err(format!("Source additive pipeline: {error}"))
                }
                _ => Ok(false),
            }
        })
    }
    /// The additive pipeline of an inspected base.
    pub fn partner(&self, base: CachedRenderPipelineId) -> Option<CachedRenderPipelineId> {
        self.partners.get(&base).copied().flatten()
    }
    pub fn counts(&self) -> (usize, usize) {
        (
            self.partners.len(),
            self.partners
                .values()
                .filter(|value| value.is_some())
                .count(),
        )
    }
    pub fn ready(
        &self,
        cache: &PipelineCache,
        owners: &super::pipeline_ownership::PipelineOwners,
    ) -> bool {
        // A base that the inspection has not seen has no partner yet.
        !self.partners.is_empty()
            && self.inspected >= cache.pipelines().count()
            && self
                .partners
                .iter()
                .filter(|(base, _)| !owners.base_isolated(**base))
                .all(|(base, add)| {
                    cache.get_render_pipeline(*base).is_some()
                        && add.is_none_or(|id| cache.get_render_pipeline(id).is_some())
                })
    }
}
fn flag(definitions: &[ShaderDefVal], name: &str) -> bool {
    definitions
        .iter()
        .any(|value| matches!(value,ShaderDefVal::Bool(key,true) if key==name))
}
/// The same inspection after the pipeline queue. A separate system: one system type
/// cannot sit in both the Prepare and Render sets.
pub(crate) fn inspect_created_surface_passes(
    passes: ResMut<SurfacePasses>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
) {
    prepare_surface_passes(passes, cache, status, owners);
}
/// Inspect only newly prepared descriptors. Pipeline clones occur once per new source variant.
pub(crate) fn prepare_surface_passes(
    mut passes: ResMut<SurfacePasses>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
) {
    if status.failure().is_some() {
        return;
    }
    let first = passes.inspected;
    for (index, pipeline) in cache.pipelines().enumerate().skip(first) {
        passes.inspected = index + 1;
        let base_id = CachedRenderPipelineId::new(index);
        if passes.partners.contains_key(&base_id) || owners.cancelled(base_id) {
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
        {
            continue;
        }
        let base = CachedRenderPipelineId::new(index);
        // Installed Unlit/Color and stained glass have no ForwardAdd pass. Single-pass surfaces include it.
        let partner = if flag(&fragment.shader_defs, "SOURCE_STANDARD")
            && !flag(&fragment.shader_defs, "SOURCE_FORWARD_ONE")
        {
            match owners.queue_companion(&cache, base, || {
                let mut add = descriptor.clone();
                add.label = Some("source_surface_add".into());
                add.vertex.shader_defs.extend([
                    ShaderDefVal::Bool("SOURCE_FORWARD_ADD".into(), true),
                    ShaderDefVal::Bool("SOURCE_LAMPS".into(), true),
                ]);
                let fragment = add.fragment.as_mut().unwrap();
                fragment.shader_defs.retain(
                |value| !matches!(value,ShaderDefVal::Bool(name,_) if name == "SOURCE_REPLACEMENT"),
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
                Some(*add)
            }) {
                Ok(id) => id,
                Err(error) => {
                    if !owners.fail(base, error.0.clone()) {
                        status.fail(error.0);
                    }
                    continue;
                }
            }
        } else {
            None
        };
        passes.partners.insert(base, partner);
    }
    for (base, add) in passes
        .partners
        .iter()
        .filter(|(base, _)| !owners.base_isolated(**base))
    {
        for id in std::iter::once(*base).chain(*add) {
            match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => {}
                CachedPipelineState::Err(error) => {
                    status.fail(format!(
                        "Source surface {} pipeline: {error}",
                        if id == *base { "base" } else { "additive" }
                    ));
                    return;
                }
                _ => {}
            }
        }
    }
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
        let Some(add) = passes.partners.get(&base) else {
            return skip(1, base);
        };
        if cache.get_render_pipeline(base).is_none() {
            return skip(2, base);
        }
        if let Some(id) = add.filter(|id| cache.get_render_pipeline(*id).is_none()) {
            return skip(3, id);
        }
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
        let Some(Some(id)) = passes.partners.get(&item.cached_pipeline()) else {
            return skip(4, item.cached_pipeline());
        };
        let Some(pipeline) = cache.into_inner().get_render_pipeline(*id) else {
            return skip(3, *id);
        };
        pass.set_render_pipeline(pipeline);
        RenderCommandResult::Success
    }
}
