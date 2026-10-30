//! Persistent source directional-shadow caster state.
use super::{readiness::RendererStateHandle, render_targets::SHADOW_FORMAT};
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
use bevy::shader::{ShaderCacheError, ShaderDefVal};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, ShaderType)]
pub struct CasterUniform {
    pub clip_from_world: Mat4,
    pub direction_to_light: Vec4,
    pub bias: Vec4,
    pub view_from_world: Mat4,
    pub world_from_view: Mat4,
    pub clip_from_view: Mat4,
}
pub fn caster_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_caster_view",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<CasterUniform>(false),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    )
}
struct CasterBinding {
    uniform: UniformBuffer<CasterUniform>,
    binding: BindGroup,
}
/// Slots 0..3 are the four source cascades. Then 6 faces for each lamp slot; only the world view uses them.
/// Each source view owns this component; queued views never overwrite another view's state.
#[derive(Component)]
pub struct CasterViews {
    slots: [CasterBinding; super::lamp_shadows::TILES],
    pub(crate) shadow_key: Option<super::shadow_preparation::CascadeKey>,
}
impl CasterViews {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        dither: &TextureView,
        sampler: &Sampler,
    ) -> Self {
        let slots = std::array::from_fn(|_| {
            let mut uniform = UniformBuffer::from(CasterUniform {
                clip_from_world: Mat4::IDENTITY,
                direction_to_light: Vec4::Y,
                bias: Vec4::ZERO,
                view_from_world: Mat4::IDENTITY,
                world_from_view: Mat4::IDENTITY,
                clip_from_view: Mat4::IDENTITY,
            });
            uniform.set_label(Some("source_caster_uniform"));
            uniform.write_buffer(device, queue);
            let binding = device.create_bind_group(
                "source_caster_view",
                &cache.get_bind_group_layout(&caster_layout()),
                &BindGroupEntries::sequential((uniform.binding().unwrap(), dither, sampler)),
            );
            CasterBinding { uniform, binding }
        });
        Self {
            slots,
            shadow_key: None,
        }
    }
    pub fn write(
        &mut self,
        index: usize,
        value: CasterUniform,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let words = |v: &CasterUniform| {
            v.clip_from_world
                .to_cols_array()
                .into_iter()
                .chain(v.direction_to_light.to_array())
                .chain(v.bias.to_array())
                .chain(v.view_from_world.to_cols_array())
                .chain(v.world_from_view.to_cols_array())
                .chain(v.clip_from_view.to_cols_array())
        };
        if super::shadow_preparation::same_floats(
            words(self.slots[index].uniform.get()),
            words(&value),
        ) {
            return;
        }
        self.slots[index].uniform.set(value);
        self.slots[index].uniform.write_buffer(device, queue);
    }
    pub fn uniform(&self, index: usize) -> &CasterUniform {
        self.slots[index].uniform.get()
    }
    pub fn binding(&self, index: usize) -> &BindGroup {
        &self.slots[index].binding
    }
}
#[derive(Resource, Default)]
pub struct CasterPasses {
    inspected: usize,
    classified: HashSet<CachedRenderPipelineId>,
    variants: HashMap<CachedRenderPipelineId, Option<CachedRenderPipelineId>>,
}
fn flag(defs: &[ShaderDefVal], name: &str) -> Option<bool> {
    defs.iter().find_map(|d| match d {
        ShaderDefVal::Bool(k, v) if k == name => Some(*v),
        _ => None,
    })
}
impl CasterPasses {
    pub fn revisit(&mut self, id: CachedRenderPipelineId) {
        self.inspected = self.inspected.min(id.id());
    }
    pub(crate) fn physically_settled(
        &self,
        base: CachedRenderPipelineId,
        cache: &PipelineCache,
    ) -> bool {
        crate::ownership::renderer_pending::pipeline_states_drained(
            self.variants.get(&base).into_iter().flatten().map(|id| {
                super::world::pipelines::physical_pipeline_state(
                    cache.get_render_pipeline_state(*id),
                )
            }),
        )
    }
    pub fn companion_state(
        &self,
        base: CachedRenderPipelineId,
        cache: &PipelineCache,
    ) -> Option<Result<bool, String>> {
        self.variants.get(&base).map(|id| {
            let Some(id) = id else {
                return Ok(true);
            };
            match cache.get_render_pipeline_state(*id) {
                CachedPipelineState::Ok(_) => Ok(true),
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => Ok(false),
                CachedPipelineState::Err(error) => Err(format!("Source caster pipeline: {error}")),
                _ => Ok(false),
            }
        })
    }
    pub fn counts(&self) -> (usize, usize) {
        (
            self.variants.len(),
            self.variants.values().filter(|id| id.is_some()).count(),
        )
    }
    pub fn classified(&self, base: CachedRenderPipelineId) -> bool {
        self.classified.contains(&base)
    }
    pub fn pipeline(&self, base: CachedRenderPipelineId) -> Option<CachedRenderPipelineId> {
        self.variants.get(&base).copied().flatten()
    }
    pub fn ready(
        &self,
        cache: &PipelineCache,
        owners: &super::pipeline_ownership::PipelineOwners,
    ) -> bool {
        !self.variants.is_empty()
            && self
                .variants
                .iter()
                .filter(|(base, _)| !owners.base_isolated(**base))
                .filter_map(|(_, id)| *id)
                .all(|id| cache.get_render_pipeline(id).is_some())
    }
    /// Called during preparation. Descriptors are cloned only once per new source surface variant.
    pub(crate) fn prepare(
        &mut self,
        cache: &PipelineCache,
        shader: &Handle<Shader>,
        status: &RendererStateHandle,
        owners: &super::pipeline_ownership::PipelineOwners,
    ) {
        if status.failure().is_some() {
            return;
        }
        let first = self.inspected;
        for (index, entry) in cache.pipelines().enumerate().skip(first) {
            self.inspected = index + 1;
            let base_id = CachedRenderPipelineId::new(index);
            if self.classified.contains(&base_id) || owners.cancelled(base_id) {
                continue;
            }
            let PipelineDescriptor::RenderPipelineDescriptor(base) = &entry.descriptor else {
                continue;
            };
            let Some(fragment) = &base.fragment else {
                continue;
            };
            let world_shader = flag(&fragment.shader_defs, "SOURCE_WORLD") == Some(true);
            if flag(&fragment.shader_defs, "SOURCE_SURFACE") != Some(true) && !world_shader
                || flag(&fragment.shader_defs, "SOURCE_FORWARD_ADD") == Some(true)
            {
                continue;
            }
            self.classified.insert(CachedRenderPipelineId::new(index));
            if flag(&fragment.shader_defs, "SOURCE_STANDARD") != Some(true) && !world_shader {
                continue;
            }
            let Some(reflected) = flag(&fragment.shader_defs, "SOURCE_OBJECT_REFLECTED") else {
                let error = "Source caster requires object transform parity metadata".to_string();
                if !owners.fail(base_id, error.clone()) {
                    status.fail(error);
                }
                return;
            };
            let id = match owners.queue_companion(cache, base_id, || {
                let mut descriptor = base.clone();
                descriptor.label = Some("source_shadow_caster".into());
                descriptor.layout[0] = super::source_lighting::source_camera_layout();
                descriptor.layout[1] = caster_layout();
                if !world_shader
                    && flag(&fragment.shader_defs, "WORLD_TEXTURE_ARRAYS") != Some(true)
                {
                    descriptor.layout[3] = super::caster_material::layout();
                }
                descriptor.multisample = MultisampleState::default();
                if !world_shader {
                    descriptor.vertex.shader = shader.clone();
                }
                descriptor.vertex.entry_point = Some("vertex".into());
                let fragment = descriptor.fragment.as_mut().unwrap();
                if !world_shader {
                    fragment.shader = shader.clone();
                }
                fragment.entry_point = Some("fragment".into());
                fragment.targets.clear();
                for defs in [
                    &mut descriptor.vertex.shader_defs,
                    &mut fragment.shader_defs,
                ] {
                    defs.retain(|v|!matches!(v,ShaderDefVal::Bool(n,_)if n=="SOURCE_SURFACE"||n=="SOURCE_FORWARD_ADD"||n=="SOURCE_FORWARD_ONE"||n=="SOURCE_LAMPS"||n=="SOURCE_WORLD"));
                    defs.push(ShaderDefVal::Bool("SOURCE_CASTER".into(), true));
                    if world_shader {
                        defs.push(ShaderDefVal::Bool("PREPASS_PIPELINE".into(), true));
                    }
                }
                if !world_shader {
                    descriptor.primitive.cull_mode = Some(Face::Back);
                }
                // RenderShadowMaps sets userBackfaceMode=false for Directional1, even inside cube capture.
                descriptor.primitive.front_face = if reflected {
                    FrontFace::Ccw
                } else {
                    FrontFace::Cw
                };
                descriptor.depth_stencil = Some(DepthStencilState {
                    format: SHADOW_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(CompareFunction::GreaterEqual),
                    stencil: StencilState::default(),
                    bias: DepthBiasState::default(),
                });
                Some(*descriptor)
            }) {
                Ok(id) => id,
                Err(error) => {
                    if !owners.fail(base_id, error.0.clone()) {
                        status.fail(error.0);
                    }
                    None
                }
            };
            self.variants.insert(base_id, id);
        }
        for id in self
            .variants
            .iter()
            .filter(|(base, _)| !owners.base_isolated(**base))
            .filter_map(|(_, id)| *id)
        {
            match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => {}
                CachedPipelineState::Err(error) => {
                    status.fail(format!("Source caster pipeline: {error}"));
                    return;
                }
                _ => {}
            }
        }
    }
}

/// Create the caster slots once per view. Cascade writes fill them later.
/// Shadow slots remain untouched until the source candidate set and cascade fit are ready.
pub(crate) fn prepare_views(
    mut commands: Commands,
    frame: Res<super::view_frame::SourceViewFrame>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    views: Query<(Entity, &super::surface_views::SourceSurfaceView), Without<CasterViews>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if status.failure().is_some() {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let Some(dither) = images.get(assets.caster_dither.id()) else {
        return;
    };
    for (entity, source) in &views {
        if frame.skip_preparation(source.index) {
            continue;
        }
        commands.entity(entity).insert(CasterViews::new(
            &device,
            &queue,
            &cache,
            &dither.texture_view,
            &dither.sampler,
        ));
    }
}

/// The same caster inspection after the pipeline queue.
pub(crate) fn inspect_created_passes(
    passes: ResMut<CasterPasses>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
) {
    prepare_passes(passes, assets, cache, status, owners);
}
pub(crate) fn prepare_passes(
    mut passes: ResMut<CasterPasses>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
) {
    let Some(assets) = assets else {
        return;
    };
    passes.prepare(&cache, &assets.shaders.caster, &status, &owners);
}
