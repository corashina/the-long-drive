//! Persistent source directional-shadow caster state.
use super::{
    readiness::RendererStateHandle, render_targets::SHADOW_FORMAT, uniform_slots::UniformSlots,
};
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
/// Slots 0..3 are the four source cascades. Then 6 faces for each lamp slot. Only the world view uses them.
/// Each world view owns this component. Queued views never overwrite another view's state.
/// All slots share one buffer, so one write uploads the frame's changes.
#[derive(Component)]
pub struct CasterViews {
    uniforms: UniformSlots<CasterUniform>,
    bindings: [BindGroup; super::lamp_shadows::TILES],
    pub(crate) shadow_key: Option<super::shadow_preparation::CascadeKey>,
}
impl CasterViews {
    pub fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        dither: &TextureView,
        sampler: &Sampler,
    ) -> Self {
        let mut uniforms =
            UniformSlots::new(device, "source_caster_uniform", super::lamp_shadows::TILES);
        for slot in 0..super::lamp_shadows::TILES {
            uniforms.set(
                slot,
                &CasterUniform {
                    clip_from_world: Mat4::IDENTITY,
                    direction_to_light: Vec4::Y,
                    bias: Vec4::ZERO,
                    view_from_world: Mat4::IDENTITY,
                    world_from_view: Mat4::IDENTITY,
                    clip_from_view: Mat4::IDENTITY,
                },
            );
        }
        let layout = cache.get_bind_group_layout(&caster_layout());
        let bindings = std::array::from_fn(|slot| {
            device.create_bind_group(
                "source_caster_view",
                &layout,
                &BindGroupEntries::sequential((uniforms.binding(slot), dither, sampler)),
            )
        });
        Self {
            uniforms,
            bindings,
            shadow_key: None,
        }
    }
    /// Stage one slot. `upload` sends the staged slots.
    pub fn write(&mut self, index: usize, value: CasterUniform) {
        self.uniforms.set(index, &value);
    }
    pub fn binding(&self, index: usize) -> &BindGroup {
        &self.bindings[index]
    }
}
/// One write for each world view after the cascade and lamp face writes.
pub(crate) fn upload(mut views: Query<&mut CasterViews>, queue: Res<RenderQueue>) {
    for mut slots in &mut views {
        slots.uniforms.upload(&queue);
    }
}
#[derive(Resource, Default)]
pub struct CasterPasses {
    inspected: usize,
    /// Pipeline count when every variant was last Ok. A new pipeline clears it.
    settled: usize,
    classified: HashSet<CachedRenderPipelineId>,
    variants: HashMap<CachedRenderPipelineId, Option<CachedRenderPipelineId>>,
    /// Active caster pipelines by descriptor. Bases that differ only in surface state share one.
    active: Vec<(RenderPipelineDescriptor, CachedRenderPipelineId)>,
}
/// Shader defs that only the surface shader reads. The caster shader and its imports do not read them.
fn surface_only(def: &ShaderDefVal) -> bool {
    let (ShaderDefVal::Bool(name, _) | ShaderDefVal::Int(name, _) | ShaderDefVal::UInt(name, _)) =
        def;
    name.starts_with("STANDARD_MATERIAL_")
        || name.starts_with("BLEND_")
        || [
            "MAY_DISCARD",
            "PREMULTIPLY_ALPHA",
            "SOURCE_STANDARD",
            "SOURCE_UNLIT",
            "SOURCE_TRANSPARENT",
            "SOURCE_REPLACEMENT",
        ]
        .contains(&name.as_str())
}
fn flag(defs: &[ShaderDefVal], name: &str) -> Option<bool> {
    defs.iter().find_map(|d| match d {
        ShaderDefVal::Bool(k, v) if k == name => Some(*v),
        _ => None,
    })
}
impl CasterPasses {
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
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        !self.variants.is_empty() && self.settled == cache.pipelines().count()
    }
    /// Called during preparation. It clones the descriptors only once for each new source surface variant.
    pub(crate) fn prepare(
        &mut self,
        cache: &PipelineCache,
        shader: &Handle<Shader>,
        status: &RendererStateHandle,
    ) {
        if status.failure().is_some() {
            return;
        }
        let first = self.inspected;
        for (index, entry) in cache.pipelines().enumerate().skip(first) {
            self.inspected = index + 1;
            let base_id = CachedRenderPipelineId::new(index);
            if self.classified.contains(&base_id) {
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
                || flag(&fragment.shader_defs, super::surface_pass::PREPASS) == Some(true)
            {
                continue;
            }
            self.classified.insert(CachedRenderPipelineId::new(index));
            if flag(&fragment.shader_defs, "SOURCE_STANDARD") != Some(true) && !world_shader {
                continue;
            }
            let Some(reflected) = flag(&fragment.shader_defs, "SOURCE_OBJECT_REFLECTED") else {
                status.fail("Source caster requires object transform parity metadata".into());
                return;
            };
            let mut descriptor = base.clone();
            descriptor.label = Some("source_shadow_caster".into());
            descriptor.layout[0] = super::source_lighting::source_camera_layout();
            descriptor.layout[1] = caster_layout();
            if !world_shader && flag(&fragment.shader_defs, "WORLD_TEXTURE_ARRAYS") != Some(true) {
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
                defs.retain(|v| {
                    !matches!(v, ShaderDefVal::Bool(n, _) if matches!(
                        n.as_str(),
                        "SOURCE_SURFACE"
                            | "SOURCE_FORWARD_ADD"
                            | "SOURCE_FORWARD_ONE"
                            | "SOURCE_LAMPS"
                            | "SOURCE_WORLD"
                    ))
                });
                defs.retain(|v| !surface_only(v));
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
            // An active base reuses an equal active caster, so its draws batch with it.
            if let Some((_, id)) = self.active.iter().find(|(d, _)| *d == *descriptor) {
                self.variants.insert(base_id, Some(*id));
                continue;
            }
            let id = cache.queue_render_pipeline((*descriptor).clone());
            self.active.push((*descriptor, id));
            self.variants.insert(base_id, Some(id));
        }
        let count = cache.pipelines().count();
        if self.settled == count {
            return;
        }
        let mut settled = true;
        for id in self.variants.values().flatten() {
            match cache.get_render_pipeline_state(*id) {
                CachedPipelineState::Ok(_) => {}
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => settled = false,
                CachedPipelineState::Err(error) => {
                    status.fail(format!("Source caster pipeline: {error}"));
                    return;
                }
                _ => settled = false,
            }
        }
        if settled {
            self.settled = count;
        }
    }
}

/// Create the caster slots once for each world view. Cascade writes fill them later.
/// Probe faces and mirrors sample the world view shadows, so they have no caster slots.
/// Shadow slots remain untouched until the source candidate set and cascade fit are ready.
pub(crate) fn prepare_views(
    mut commands: Commands,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    views: Query<(Entity, &super::surface_views::SourceSurfaceView), Without<CasterViews>>,
    device: Res<RenderDevice>,
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
        if source.index >= super::surface_views::WORLD_VIEWS {
            continue;
        }
        commands.entity(entity).insert(CasterViews::new(
            &device,
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
) {
    prepare_passes(passes, assets, cache, status);
}
pub(crate) fn prepare_passes(
    mut passes: ResMut<CasterPasses>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    let Some(assets) = assets else {
        return;
    };
    passes.prepare(&cache, &assets.shaders.caster, &status);
}
