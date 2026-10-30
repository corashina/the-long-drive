//! Cached source shadow specialization and offscreen mesh queueing.
use super::shadow_cache::SpecializationKey;
use super::surface_views::{PROBE_START, VIEW_COUNT};
use super::{
    caster::{CasterPasses, CasterUniform, CasterViews},
    readiness::RendererStateHandle,
    shadow_phase::{self, ShadowBatchKey, ShadowDrawState, SourceShadow},
    shadow_scene::ShadowScene,
    surface_views::SourceSurfaceView,
    view_frame::SourceViewFrame,
};
use bevy::material::{
    MaterialProperties,
    key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey},
};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    MeshPipeline, MeshPipelineKey, PreparedMaterial, RenderMaterialInstances, RenderMeshInstances,
    ViewKeyCache, alpha_mode_pipeline_key, base_specialize,
};
use bevy::render::{
    batching::gpu_preprocessing::GpuPreprocessingMode,
    erased_render_asset::ErasedRenderAssets,
    mesh::{RenderMesh, allocator::MeshAllocator},
    render_asset::RenderAssets,
    render_phase::*,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    sync_world::{MainEntity, MainEntityHashMap},
    view::{ExtractedView, RetainedViewEntity},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::Arc;
#[derive(Clone, PartialEq)]
struct QueuedShadow {
    key: ShadowBatchKey,
    mesh: bevy::asset::AssetId<Mesh>,
    input: InputUniformIndex,
    batchable: bool,
    specialization: SpecializationKey,
    base: CachedRenderPipelineId,
    seen: u64,
}
struct CachedSpecialization {
    key: SpecializationKey,
    // Retain both allocations for as long as their pointer identities remain keys.
    _layout: MeshVertexBufferLayoutRef,
    _properties: Arc<MaterialProperties>,
    base: CachedRenderPipelineId,
}
#[derive(Resource)]
pub struct ShadowQueues {
    query: QueryState<(Entity, &'static SourceSurfaceView, &'static ExtractedView)>,
    specializations: Vec<CachedSpecialization>,
    queued: [MainEntityHashMap<QueuedShadow>; VIEW_COUNT],
    retained: [Option<RetainedViewEntity>; VIEW_COUNT],
    // Keep idle face and mirror bins out of Bevy's per-frame instance preparation.
    idle_probe_phases: [Option<BinnedRenderPhase<SourceShadow>>; VIEW_COUNT - PROBE_START],
    epoch: Option<(u64, u32)>,
    revision: u64,
    /// Queued instances of each caster mask bit: 4 cascades, then 4 lamp slots.
    pub instances: [[u32; 8]; VIEW_COUNT],
    pub ready: [bool; VIEW_COUNT],
    pub frame: u32,
}
impl FromWorld for ShadowQueues {
    fn from_world(world: &mut World) -> Self {
        Self {
            query: world.query(),
            specializations: Vec::new(),
            queued: std::array::from_fn(|_| MainEntityHashMap::default()),
            retained: [None; VIEW_COUNT],
            idle_probe_phases: std::array::from_fn(|_| None),
            epoch: None,
            revision: 0,
            instances: [[0; 8]; VIEW_COUNT],
            ready: [false; VIEW_COUNT],
            frame: 0,
        }
    }
}
impl ShadowQueues {
    pub fn cached_pipelines(&self) -> usize {
        self.specializations.len()
    }
}
fn base_pipeline(
    world: &mut World,
    state: &mut ShadowQueues,
    entity: MainEntity,
    view_key: MeshPipelineKey,
    previous: Option<&QueuedShadow>,
) -> Result<Option<(CachedRenderPipelineId, bool, SpecializationKey)>, String> {
    let Some(mesh_id) = world
        .resource::<RenderMeshInstances>()
        .mesh_asset_id(entity)
    else {
        return Ok(None);
    };
    let Some(instance) = world
        .resource::<RenderMaterialInstances>()
        .instances
        .get(&entity)
    else {
        return Ok(None);
    };
    let material_type = instance.asset_id.type_id();
    let Some(mesh) = world.resource::<RenderAssets<RenderMesh>>().get(mesh_id) else {
        return Ok(None);
    };
    let Some(material) = world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(instance.asset_id)
    else {
        return Ok(None);
    };
    let Some(slabs) = world.resource::<MeshAllocator>().mesh_slabs(&mesh_id) else {
        return Ok(None);
    };
    let indexed = slabs.index_slab_id.is_some();
    let bits: MeshPipelineKey = material.properties.mesh_pipeline_key_bits.downcast();
    let key = view_key
        | MeshPipelineKey::from_bits_retain(mesh.key_bits.bits())
        | bits
        | alpha_mode_pipeline_key(
            material.properties.alpha_mode,
            &Msaa::from_samples(view_key.msaa_samples()),
        );
    let cache_key = SpecializationKey::new(key.bits(), &mesh.layout.0, &material.properties);
    if let Some(previous) = previous {
        if previous.specialization == cache_key {
            return Ok(Some((previous.base, indexed, cache_key)));
        }
    }
    let mut index = match state
        .specializations
        .binary_search_by_key(&cache_key, |entry| entry.key)
    {
        Ok(index) => {
            return Ok(Some((
                state.specializations[index].base,
                indexed,
                cache_key,
            )));
        }
        Err(index) => index,
    };
    if state.specializations.len() == state.specializations.capacity() {
        // Re-prepared materials get new pointer keys, so drop keys no queued item uses.
        let ShadowQueues {
            specializations,
            queued,
            ..
        } = &mut *state;
        specializations.retain(|entry| {
            queued
                .iter()
                .any(|view| view.values().any(|item| item.specialization == entry.key))
        });
        if specializations.len() == specializations.capacity() {
            specializations.reserve_exact(specializations.capacity().max(1));
        }
        index = specializations
            .binary_search_by_key(&cache_key, |entry| entry.key)
            .unwrap_err();
    }
    // Full immutable material key is passed on cache misses. No Arc clone occurs on steady hits.
    let properties = material.properties.clone();
    let layout = mesh.layout.clone();
    let full = ErasedMaterialPipelineKey {
        type_id: material_type,
        mesh_key: ErasedMeshPipelineKey::new(key),
        material_key: properties.material_key.clone(),
    };
    let base = base_specialize(world, full, &layout, &properties)
        .map_err(|e| format!("Source shadow specialization: {e}"))?;
    state.specializations.insert(
        index,
        CachedSpecialization {
            key: cache_key,
            _layout: layout,
            _properties: properties,
            base,
        },
    );
    Ok(Some((base, indexed, cache_key)))
}
pub fn queue(world: &mut World) {
    world.resource_scope(|world, mut state: Mut<ShadowQueues>| {
        state.ready = [false; VIEW_COUNT];
        state.instances = [[0; 8]; VIEW_COUNT];
        state.revision += 1;
        let revision = state.revision;
        state.frame = 0;
        if world.resource::<RendererStateHandle>().failure().is_some() {
            return;
        }
        world.resource_scope(|world, scene: Mut<ShadowScene>| {
            if scene.frame == 0 || scene.frame != world.resource::<SourceViewFrame>().frame {
                return;
            }
            if state.specializations.capacity() < scene.capacity() * VIEW_COUNT {
                state
                    .specializations
                    .reserve_exact(scene.capacity() * VIEW_COUNT);
            }
            let mut views: [Option<(Entity, RetainedViewEntity, MeshPipelineKey)>; VIEW_COUNT] =
                [None; VIEW_COUNT];
            for (entity, source, view) in state.query.iter(world) {
                if world.resource::<SourceViewFrame>().views[source.index].is_none() {
                    continue;
                }
                let Some(key) = world
                    .resource::<ViewKeyCache>()
                    .get(&view.retained_view_entity)
                    .copied()
                else {
                    return;
                };
                views[source.index] = Some((entity, view.retained_view_entity, key));
            }
            let draw = shadow_phase::draw_id(world.resource::<DrawFunctions<SourceShadow>>());
            world.resource_scope(
                |world, mut phases: Mut<ViewBinnedRenderPhases<SourceShadow>>| {
                    let frame = world.resource::<SourceViewFrame>();
                    let epoch = (frame.session, frame.scene);
                    if state.epoch != Some(epoch) {
                        // Old bins and pointer keys belong to the previous scene.
                        for retained in state.retained.iter().flatten() {
                            phases.remove(retained);
                        }
                        for queued in &mut state.queued {
                            queued.clear();
                        }
                        state.specializations.clear();
                        state.retained = [None; VIEW_COUNT];
                        state.idle_probe_phases = std::array::from_fn(|_| None);
                        state.epoch = Some(epoch);
                    }
                    for index in PROBE_START..VIEW_COUNT {
                        if world.resource::<SourceViewFrame>().skip_preparation(index) {
                            if let Some(phase) =
                                state.retained[index].and_then(|retained| phases.remove(&retained))
                            {
                                state.idle_probe_phases[index - PROBE_START] = Some(phase);
                            }
                        }
                    }
                    for (index, view) in views.into_iter().enumerate() {
                        let Some((entity, retained, key)) = view else {
                            continue;
                        };
                        if state.retained[index] != Some(retained) {
                            if let Some(previous) = state.retained[index].replace(retained) {
                                phases.remove(&previous);
                            }
                            phases.remove(&retained);
                            if index >= PROBE_START {
                                state.idle_probe_phases[index - PROBE_START] = None;
                            }
                        }
                        if index >= PROBE_START {
                            if let Some(phase) = state.idle_probe_phases[index - PROBE_START].take()
                            {
                                phases.insert(retained, phase);
                            }
                        }
                        if !phases.contains_key(&retained) {
                            state.queued[index].clear();
                            world.entity_mut(entity).insert(ShadowDrawState::default());
                        }
                        // Source views carry NoIndirectDrawing, so their work items are direct.
                        phases.prepare_for_new_frame(
                            retained,
                            GpuPreprocessingMode::PreprocessingOnly,
                        );
                        let phase = phases.get_mut(&retained).unwrap();
                        let queued = &mut state.queued[index];
                        queued.reserve(scene.capacity().saturating_sub(queued.len()));
                        let mut ready = true;
                        for (item, caster) in scene.caster_data.entities.iter().enumerate() {
                            let mask = scene.masks[index][item];
                            // GPU prop rows draw from their own buffers.
                            let crate::presentation::shadow_scene::CasterKey::Entity(main) =
                                *caster
                            else {
                                continue;
                            };
                            if mask == 0 {
                                continue;
                            }
                            let main = MainEntity::from(main);
                            let previous = state.queued[index].get(&main).cloned();
                            let base = match base_pipeline(
                                world,
                                &mut state,
                                main,
                                key,
                                previous.as_ref(),
                            ) {
                                Ok(Some(v)) => v,
                                Ok(None) => {
                                    ready = false;
                                    continue;
                                }
                                Err(e) => {
                                    world.resource::<RendererStateHandle>().fail(e);
                                    return;
                                }
                            };
                            let casters = world.resource::<CasterPasses>();
                            let Some(pipeline) = casters.pipeline(base.0) else {
                                ready = false;
                                continue;
                            };
                            if world
                                .resource::<PipelineCache>()
                                .get_render_pipeline(pipeline)
                                .is_none()
                            {
                                ready = false;
                            }
                            let instances = world.resource::<RenderMeshInstances>();
                            let Some(instance) = instances.render_mesh_queue_data(main) else {
                                ready = false;
                                continue;
                            };
                            let mesh = instance.mesh_asset_id();
                            // A re-extracted mesh can get a new input slot. Rebin it on change.
                            let input = instance.current_uniform_index;
                            let batchable = instance.should_batch();
                            let material = world.resource::<RenderMaterialInstances>().instances
                                [&main]
                                .asset_id;
                            let material = super::caster_material::shadow_material(world, material);
                            let next = QueuedShadow {
                                key: ShadowBatchKey {
                                    pipeline,
                                    draw,
                                    material,
                                    split_mask: mask,
                                    indexed: base.1,
                                },
                                mesh,
                                input,
                                batchable,
                                specialization: base.2,
                                base: base.0,
                                seen: revision,
                            };
                            if previous.as_ref().is_none_or(|old| {
                                old.key != next.key
                                    || old.mesh != mesh
                                    || old.input != input
                                    || old.batchable != batchable
                            }) {
                                phase.remove(main);
                                phase.add(
                                    next.key.clone(),
                                    mesh,
                                    (Entity::PLACEHOLDER, main),
                                    input,
                                    if batchable {
                                        BinnedRenderPhaseType::BatchableMesh
                                    } else {
                                        BinnedRenderPhaseType::UnbatchableMesh
                                    },
                                );
                            }
                            if let Some(previous) = state.queued[index].get_mut(&main) {
                                *previous = next;
                            } else {
                                state.queued[index].insert(main, next);
                            }
                            for bit in 0..8 {
                                state.instances[index][bit] += u32::from(mask & (1 << bit) != 0);
                            }
                        }
                        state.queued[index].retain(|main, queued| {
                            if queued.seen == revision {
                                return true;
                            }
                            phase.remove(*main);
                            false
                        });
                        state.ready[index] = ready;
                    }
                },
            );
            state.frame = scene.frame;
        });
    });
}
/// The native light camera negates its forward column before inversion.
pub(crate) fn cascade_projection(
    cascade: &crate::reference::shadows::ShadowCascade,
    light_basis: [Vec3; 3],
) -> (Mat4, f32) {
    let position = Vec3::from_array(cascade.light_position);
    let source_world = Mat4::from_cols(
        light_basis[0].extend(0.),
        light_basis[1].extend(0.),
        (-light_basis[2]).extend(0.),
        position.extend(1.),
    );
    let [width, height] = cascade.width_height;
    let mut p = Mat4::orthographic_rh_gl(
        -width * 0.5,
        width * 0.5,
        -height * 0.5,
        height * 0.5,
        cascade.near,
        cascade.far,
    )
    .to_cols_array_2d();
    for c in &mut p {
        c[1] = -c[1];
        c[2] = 0.5 * c[3] - 0.5 * c[2];
    }
    (
        Mat4::from_cols_array_2d(&p)
            * source_world.inverse()
            * Mat4::from_scale(Vec3::new(1., 1., -1.)),
        p[2][2],
    )
}
fn cascade_world_from_view(
    cascade: &crate::reference::shadows::ShadowCascade,
    basis: [Vec3; 3],
) -> Mat4 {
    Mat4::from_scale(Vec3::new(1., 1., -1.))
        * Mat4::from_cols(
            basis[0].extend(0.),
            basis[1].extend(0.),
            (-basis[2]).extend(0.),
            Vec3::from_array(cascade.light_position).extend(1.),
        )
}
pub fn write_cascades(
    scene: Res<ShadowScene>,
    frame: Res<SourceViewFrame>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    mut views: Query<(
        &mut SourceSurfaceView,
        &mut CasterViews,
        &super::shadow_atlas::ShadowCadence,
    )>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if scene.frame == 0 || scene.frame != frame.frame {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let light = &assets.record.lights[0];
    let rotation = scene.sun;
    let h = |v: Vec3| Vec3::new(v.x, v.y, -v.z);
    let basis = [
        h(rotation * Vec3::X),
        h(rotation * Vec3::Y),
        h(rotation * -Vec3::Z),
    ];
    for (mut view, mut slots, cadence) in &mut views {
        if frame.skip_preparation(view.index) {
            continue;
        }
        let key = super::shadow_preparation::CascadeKey {
            view: scene.view_revision(view.index, &frame),
            cadence: cadence.revision,
            bias: [
                light.shadows.depth_bias.to_bits(),
                light.shadows.normal_bias.to_bits(),
            ],
        };
        let Some(fit) = scene.fits[view.index] else {
            view.shadow_fade = Vec4::ZERO;
            view.shadow_fade_center = Vec4::ZERO;
            slots.shadow_key = Some(key);
            continue;
        };
        view.shadow_fade = Vec4::from_array(fit.fade);
        view.shadow_fade_center = Vec4::new(
            fit.fade_center[0],
            fit.fade_center[1],
            -fit.fade_center[2],
            fit.fade_center[3],
        );
        // SourceSurfaceView is extracted again each frame. Always restore its fade values.
        if slots.shadow_key == Some(key) {
            continue;
        }
        slots.shadow_key = Some(key);
        for (index, cascade) in fit.cascades.iter().enumerate() {
            // A stale slot keeps the values of its atlas tile.
            if !cadence.plan.render[index] {
                continue;
            }
            let (projection, device_e22) = cascade_projection(cascade, basis);
            // Selected D3D reversed-depth orthographic device e22 = 1/(far-near).
            let depth_bias = -light.shadows.depth_bias * device_e22;
            let normal_bias = (light.shadows.normal_bias * 3.6500000953674316_f32)
                * cascade.texel_size[0].max(cascade.texel_size[1]);
            let world_from_view = cascade_world_from_view(cascade, basis);
            slots.write(
                index,
                CasterUniform {
                    clip_from_world: projection,
                    world_from_view,
                    view_from_world: world_from_view.inverse(),
                    clip_from_view: projection * world_from_view,
                    direction_to_light: (rotation * Vec3::Z).extend(0.),
                    bias: Vec4::new(
                        depth_bias,
                        1.,
                        normal_bias,
                        super::surface_views::player_dist(view.index),
                    ),
                },
                &device,
                &queue,
            );
        }
    }
}
pub fn install(app: &mut App) {
    use bevy::camera::visibility::VisibilitySystems;
    use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};
    app.init_resource::<ShadowScene>()
        .add_systems(
            PostUpdate,
            super::shadow_scene::prepare_scene
                .after(VisibilitySystems::CheckVisibility)
                .before(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible),
        )
        .add_plugins(BinnedRenderPhasePlugin::<SourceShadow, MeshPipeline>::new(
            default(),
        ));
    let render = app.sub_app_mut(RenderApp);
    shadow_phase::register(render);
    render
        .init_resource::<ShadowScene>()
        .init_resource::<ShadowQueues>()
        .add_systems(ExtractSchedule, super::shadow_scene::extract_scene)
        .add_systems(Render, queue.in_set(RenderSystems::Queue))
        .add_systems(
            Render,
            write_cascades
                .in_set(RenderSystems::PrepareResources)
                .after(super::caster::prepare_views)
                .before(super::surface_views::prepare_views),
        );
}

/// Gates source dispatch before any pass consumes a stale view or partial pipeline set.
pub(crate) fn graph_ready(
    view: bevy::render::renderer::ViewQuery<(
        &SourceSurfaceView,
        Option<&super::shadow_collector::CollectorView>,
    )>,
    frame: Res<SourceViewFrame>,
    queues: Res<ShadowQueues>,
    cache: Res<PipelineCache>,
    surface: Res<super::surface_pass::SurfacePasses>,
    caster: Res<CasterPasses>,
    resolve: Res<super::depth_resolve::DepthResolve>,
    owners: Res<super::pipeline_ownership::PipelineOwners>,
    loading: Res<super::materials::MaterialLoading>,
    status: Res<RendererStateHandle>,
    props: Res<super::prop_draws::PropDraws>,
) -> bool {
    let (view, collector) = view.into_inner();
    frame.frame != 0
        && frame.views[view.index].is_some()
        && queues.frame == frame.frame
        && queues.ready[view.index]
        && props.frame == frame.frame
        && props.ready[view.index]
        && loading.is_ready()
        && surface.ready(&cache, &owners)
        && caster.ready(&cache, &owners)
        && resolve.ready(&cache)
        && collector.is_some_and(|c| c.prepared_frame == frame.frame)
        && !status.failure().is_some()
}
pub fn collector_encoded(
    view: bevy::render::renderer::ViewQuery<&super::shadow_collector::CollectorView>,
    frame: Res<SourceViewFrame>,
) -> bool {
    view.into_inner().encoded_frame() == frame.frame
}
