//! Source per-view lighting bindings. Scene and probe views never share writable uniforms.

use super::materials::SourceMaterial;
use super::readiness::RendererStateHandle;
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Transparent3d};
use bevy::ecs as bevy_ecs;
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::SystemParamItem;
use bevy::material::labels::DrawFunctionLabel;
use bevy::pbr::{
    DrawMesh, MainPassAlphaMaskDrawFunction, MainPassOpaqueDrawFunction,
    MainPassTransmissiveDrawFunction, MainPassTransparentDrawFunction, PreparedMaterial,
    Transmissive3d,
};
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    erased_render_asset::ErasedRenderAssets,
    render_phase::{
        AddRenderCommand, DrawFunctions, PhaseItem, RenderCommand, RenderCommandResult,
        SetItemPipeline, TrackedRenderPass,
    },
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
use std::{
    any::TypeId,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

/// Same layout as source_lighting.wgsl. Source constants are uploaded without unit conversion.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct LightingUniform {
    pub ambient_sh: [Vec4; 7],
    pub sun_radiance: Vec4,
    pub direction_to_sun: Vec4,
    pub cookie_from_world: Mat4,
    pub fog_color_density: Vec4,
    pub shadow_fade: Vec4,
    pub shadow_fade_center: Vec4,
    pub probe_center: [Vec4; super::lighting_scene::PROBE_COUNT],
    pub probe_extents: [Vec4; super::lighting_scene::PROBE_COUNT],
    pub source_time: Vec4,
    pub clip_from_world: Mat4,
    pub near_far: Vec4,
    pub sun_shadow: super::shadow_collector::SunShadowUniform,
    /// Lamp shadow slots. Only the world view fills them.
    pub lamp_shadows:
        [super::lamp_shadows::LampShadowUniform; crate::reference::lamp_shadows::LAMP_SLOTS],
}

/// Layout specialization happens only when a pipeline is prepared.
/// Task5 uses a distinct glass layout; it cannot append a seventeenth texture.
pub fn surface_lighting_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_view_lighting",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<LightingUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_cube_array(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                sampler(SamplerBindingType::Comparison),
                texture_depth_2d(),
            ),
        ),
    )
}

/// One immutable bind group and one stable buffer allocation for each queued view.
#[derive(Component)]
pub struct SourceViewLighting {
    uniform: UniformBuffer<LightingUniform>,
    bind_group: BindGroup,
    draw_epoch: AtomicU32,
    draw_counts: [AtomicU32; 4],
    instance_counts: [AtomicU32; 4],
}
impl SourceViewLighting {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        value: LightingUniform,
        cookie: (&TextureView, &Sampler),
        probe: (&TextureView, &Sampler),
        shadow: (&TextureView, &Sampler),
        lamp_atlas: &TextureView,
    ) -> Self {
        let mut uniform = UniformBuffer::from(value);
        uniform.set_label(Some("source_view_lighting_uniform"));
        uniform.write_buffer(device, queue);
        let bind_group = device.create_bind_group(
            "source_view_lighting",
            &cache.get_bind_group_layout(&surface_lighting_layout()),
            &BindGroupEntries::sequential((
                uniform.binding().expect("allocated source view uniform"),
                cookie.0,
                cookie.1,
                probe.0,
                probe.1,
                shadow.0,
                shadow.1,
                lamp_atlas,
            )),
        );
        Self {
            uniform,
            bind_group,
            draw_epoch: AtomicU32::new(0),
            draw_counts: std::array::from_fn(|_| AtomicU32::new(0)),
            instance_counts: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    pub fn bind_group(&self) -> &BindGroup {
        &self.bind_group
    }
    /// Reset before each view invocation. Earlier draws cannot satisfy this submission.
    pub fn begin_draws(&self) {
        for count in self.draw_counts.iter().chain(self.instance_counts.iter()) {
            count.store(0, Ordering::Relaxed);
        }
        self.draw_epoch.fetch_add(1, Ordering::Relaxed);
    }
    /// Epoch, base attempts/successes, additive attempts/successes.
    pub fn instances(&self) -> [u32; 4] {
        self.instance_counts
            .each_ref()
            .map(|value| value.load(Ordering::Relaxed))
    }
    pub fn draws(&self) -> [u32; 5] {
        [
            self.draw_epoch.load(Ordering::Relaxed),
            self.draw_counts[0].load(Ordering::Relaxed),
            self.draw_counts[1].load(Ordering::Relaxed),
            self.draw_counts[2].load(Ordering::Relaxed),
            self.draw_counts[3].load(Ordering::Relaxed),
        ]
    }

    /// Update every view before graph execution. This does not allocate GPU resources.
    pub fn unchanged(&self, value: &LightingUniform) -> bool {
        self.uniform.get() == value
    }
    /// Update every view before graph execution. This does not allocate GPU resources.
    pub fn write(&mut self, value: LightingUniform, device: &RenderDevice, queue: &RenderQueue) {
        if self.unchanged(&value) {
            return;
        }
        self.uniform.set(value);
        self.uniform.write_buffer(device, queue);
    }
}

pub struct SetSourceViewLighting;
impl<P: PhaseItem> RenderCommand<P> for SetSourceViewLighting {
    type Param = ();
    type ViewQuery = &'static SourceViewLighting;
    type ItemQuery = ();
    fn render<'w>(
        _: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(1, &view.bind_group, &[]);
        RenderCommandResult::Success
    }
}

pub struct SourceDrawMark<const ADD: bool, const DONE: bool>;
impl<P: PhaseItem, const ADD: bool, const DONE: bool> RenderCommand<P>
    for SourceDrawMark<ADD, DONE>
{
    type Param = ();
    type ViewQuery = &'static SourceViewLighting;
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        _: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        view.draw_counts[usize::from(ADD) * 2 + usize::from(DONE)].fetch_add(1, Ordering::Relaxed);
        let count = item.batch_range().end - item.batch_range().start;
        view.instance_counts[usize::from(ADD) * 2 + usize::from(DONE)]
            .fetch_add(count, Ordering::Relaxed);
        RenderCommandResult::Success
    }
}

pub(crate) type DrawSourceMaterial = (
    SourceDrawMark<false, false>,
    super::surface_pass::SourcePairReady,
    SetItemPipeline,
    SetSourceViewCamera,
    SetSourceViewLighting,
    super::static_draws::SetSourceMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    DrawMesh,
    SourceDrawMark<false, true>,
    super::surface_pass::SetSourceAdditivePipeline,
    SourceDrawMark<true, false>,
    DrawMesh,
    SourceDrawMark<true, true>,
);

/// Transparent passes are dispatched separately through the source comparator.
pub(crate) type DrawSourceBase = (
    SourceDrawMark<false, false>,
    super::surface_pass::SourcePairReady,
    SetItemPipeline,
    SetSourceViewCamera,
    SetSourceViewLighting,
    super::static_draws::SetSourceMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    DrawMesh,
    SourceDrawMark<false, true>,
);
pub(crate) type DrawSourceWorld = (
    SetItemPipeline,
    SetSourceViewCamera,
    SetSourceViewLighting,
    super::static_draws::SetSourceMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    DrawMesh,
);

pub(crate) type DrawSourcePlate = (
    SourceDrawMark<false, false>,
    SetItemPipeline,
    SetSourceViewCamera,
    SetSourceViewLighting,
    super::static_draws::SetSourceMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    DrawMesh,
    SourceDrawMark<false, true>,
);
pub(crate) type DrawSourceGlass = (
    SourceDrawMark<false, false>,
    super::surface_pass::SourcePairReady,
    SetItemPipeline,
    SetSourceViewCamera,
    super::stained_glass::SetSourceGlassView,
    super::static_draws::SetSourceMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    DrawMesh,
    SourceDrawMark<false, true>,
);

pub(crate) type DrawSourceAdd = (
    SourceDrawMark<true, false>,
    super::surface_pass::SourcePairReady,
    super::surface_pass::SetSourceAdditivePipeline,
    SetSourceViewCamera,
    SetSourceViewLighting,
    super::static_draws::SetSourceMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    DrawMesh,
    SourceDrawMark<true, true>,
);

pub struct SourceLightingPlugin;
impl Plugin for SourceLightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::render::extract_component::ExtractComponentPlugin::<
            super::surface_views::SourceSurfaceView,
        >::default())
            .init_resource::<super::lighting_scene::SourceSceneLighting>()
            .init_resource::<super::lighting_scene::SourceWorldOrigin>()
            .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
                super::lighting_scene::SourceSceneLighting,
            >::default())
            .add_systems(
                PostUpdate,
                super::lighting_scene::sync_lighting
                    .after(bevy::transform::TransformSystems::Propagate)
                    .after(super::lighting::sync_lighting)
                    .after(super::sky::sync_sky),
            )
            .init_asset::<super::lighting_assets::CasterDitherBytes>()
            .init_asset_loader::<super::lighting_assets::CasterDitherLoader>()
            .add_systems(Update, super::lighting_assets::lighting_asset_readiness);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            super::surface_graph::install(render_app);
            super::depth_resolve::install(render_app);
            super::prop_draws::install(render_app);
            super::static_draws::install(render_app);
            render_app
                .init_resource::<SourceViewCamera>()
                .add_systems(
                    Render,
                    prepare_source_camera
                        .in_set(RenderSystems::PrepareBindGroups)
                        .after(bevy::render::view::prepare_view_uniforms),
                )
                .init_resource::<super::surface_pass::SurfacePasses>()
                .init_resource::<super::caster::CasterPasses>()
                .init_resource::<super::transparent::SourceSortIndex>()
                .add_systems(
                    bevy::render::ExtractSchedule,
                    super::transparent::extract_sort_index,
                )
                .add_systems(
                    bevy::render::ExtractSchedule,
                    super::lighting_assets::extract_lighting_assets,
                )
                .add_systems(
                    Render,
                    (
                        super::surface_views::prepare_views,
                        super::caster::prepare_views,
                    )
                        .in_set(RenderSystems::PrepareResources),
                )
                .add_systems(
                    Render,
                    (
                        super::clouds::prepare_clouds,
                        super::source_sky::prepare_sky,
                    )
                        .in_set(RenderSystems::PrepareResources),
                )
                .add_systems(
                    Render,
                    (
                        super::surface_pass::prepare_surface_passes,
                        super::caster::prepare_passes,
                    )
                        .chain()
                        .in_set(RenderSystems::Prepare),
                )
                // WebGPU creates queued pipelines in the same frame. Inspect them before the
                // graph runs, so a new base waits for its partner instead of skipping its draw.
                .add_systems(
                    Render,
                    (
                        super::surface_pass::inspect_created_surface_passes,
                        super::caster::inspect_created_passes,
                    )
                        .chain()
                        .in_set(RenderSystems::Render)
                        .after(PipelineCache::process_pipeline_queue_system)
                        .before(bevy::render::renderer::render_system),
                )
                .add_render_command::<Opaque3d, DrawSourceWorld>()
                .add_render_command::<AlphaMask3d, DrawSourceWorld>()
                .add_render_command::<Transmissive3d, DrawSourceWorld>()
                .add_render_command::<Transparent3d, DrawSourceWorld>()
                .add_render_command::<Opaque3d, DrawSourceMaterial>()
                .add_render_command::<AlphaMask3d, DrawSourceMaterial>()
                .add_render_command::<Transmissive3d, DrawSourceMaterial>()
                .add_render_command::<Transparent3d, DrawSourceBase>()
                .add_render_command::<Transparent3d, DrawSourcePlate>()
                .add_render_command::<Transparent3d, DrawSourceAdd>()
                .add_render_command::<Transparent3d, DrawSourceGlass>()
                .add_systems(
                    Render,
                    super::transparent::prepare_pass_order
                        .after(RenderSystems::PhaseSort)
                        .after(super::surface_pass::prepare_surface_passes)
                        .before(RenderSystems::Render),
                )
                .add_systems(
                    Render,
                    prepare_source_draws
                        .after(RenderSystems::PrepareAssets)
                        .before(RenderSystems::Specialize),
                );
        }
    }
}

fn prepare_source_draws(
    mut materials: ResMut<ErasedRenderAssets<PreparedMaterial>>,
    status: Res<RendererStateHandle>,
    opaque: Res<DrawFunctions<Opaque3d>>,
    masked: Res<DrawFunctions<AlphaMask3d>>,
    transmissive: Res<DrawFunctions<Transmissive3d>>,
    transparent: Res<DrawFunctions<Transparent3d>>,
) {
    if !materials.is_changed() {
        return;
    }
    let draws = [
        (
            MainPassOpaqueDrawFunction.intern(),
            opaque.read().id::<DrawSourceMaterial>(),
        ),
        (
            MainPassAlphaMaskDrawFunction.intern(),
            masked.read().id::<DrawSourceMaterial>(),
        ),
        (
            MainPassTransmissiveDrawFunction.intern(),
            transmissive.read().id::<DrawSourceMaterial>(),
        ),
        (
            MainPassTransparentDrawFunction.intern(),
            transparent.read().id::<DrawSourceBase>(),
        ),
    ];
    for (id, material) in materials.iter_mut() {
        let world_material = [
            TypeId::of::<super::materials::terrain::TerrainMaterial>(),
            TypeId::of::<super::materials::impostor::ImpostorMaterial>(),
            TypeId::of::<super::materials::grass::material::GrassMaterial>(),
        ]
        .contains(&id.type_id());
        let specular = id.type_id() == TypeId::of::<super::materials::specular::SpecularMaterial>();
        let draws = if world_material {
            [
                (
                    MainPassOpaqueDrawFunction.intern(),
                    opaque.read().id::<DrawSourceWorld>(),
                ),
                (
                    MainPassAlphaMaskDrawFunction.intern(),
                    masked.read().id::<DrawSourceWorld>(),
                ),
                (
                    MainPassTransmissiveDrawFunction.intern(),
                    transmissive.read().id::<DrawSourceWorld>(),
                ),
                (
                    MainPassTransparentDrawFunction.intern(),
                    transparent.read().id::<DrawSourceWorld>(),
                ),
            ]
        } else {
            draws
        };
        if id.type_id() == TypeId::of::<super::plates::PlateTextMaterial>() {
            let label = MainPassTransparentDrawFunction.intern();
            let draw = transparent.read().id::<DrawSourcePlate>();
            if material
                .properties
                .draw_functions
                .iter()
                .any(|(key, current)| *key == label && *current != draw)
            {
                let Some(properties) = Arc::get_mut(&mut material.properties) else {
                    status.fail("Plate draw properties were shared before specialization".into());
                    return;
                };
                properties
                    .draw_functions
                    .iter_mut()
                    .find(|(key, _)| *key == label)
                    .expect("plate transparent draw label")
                    .1 = draw;
            }
            continue;
        }
        if id.type_id() != TypeId::of::<SourceMaterial>() && !world_material && !specular {
            continue;
        }
        let needs_update = draws.iter().any(|(label, draw)| {
            material
                .properties
                .draw_functions
                .iter()
                .find(|(key, _)| key == label)
                .is_none_or(|(_, current)| current != draw)
        });
        if !needs_update {
            continue;
        }
        // Preparation creates this Arc. Modify it before specialization can retain it.
        let Some(properties) = Arc::get_mut(&mut material.properties) else {
            status.fail("Source material draw setup failed: prepared properties were shared before specialization".into());
            return;
        };
        for (label, draw) in draws {
            let entry = properties
                .draw_functions
                .iter_mut()
                .find(|(key, _)| *key == label)
                .expect("source material has every forward draw label");
            entry.1 = draw;
        }
    }
}

/// Source mesh shaders use Bevy camera/global buffers and perform their own lighting.
/// This layout has no sampled textures or samplers.
pub fn source_camera_layout() -> BindGroupLayoutDescriptor {
    use bevy::render::{globals::GlobalsUniform, view::ViewUniform};
    BindGroupLayoutDescriptor::new(
        "source_camera",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::VERTEX_FRAGMENT,
            (
                (0, uniform_buffer::<ViewUniform>(true)),
                (11, uniform_buffer::<GlobalsUniform>(false)),
            ),
        ),
    )
}

#[derive(Resource, Default)]
pub struct SourceViewCamera {
    bind_group: Option<BindGroup>,
    buffers: Option<(BufferId, BufferId)>,
}
impl SourceViewCamera {
    pub fn bind_group(&self) -> Option<&BindGroup> {
        self.bind_group.as_ref()
    }
    pub fn ready(&self) -> bool {
        self.bind_group.is_some()
    }
}

/// Rebind only when Bevy replaces a backing buffer. Steady frames retain the group.
pub fn prepare_source_camera(
    mut camera: ResMut<SourceViewCamera>,
    uniforms: Res<bevy::render::view::ViewUniforms>,
    globals: Res<bevy::render::globals::GlobalsBuffer>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
) {
    let (Some(view_buffer), Some(global_buffer)) =
        (uniforms.uniforms.buffer(), globals.buffer.buffer())
    else {
        camera.bind_group = None;
        camera.buffers = None;
        return;
    };
    let identities = (view_buffer.id(), global_buffer.id());
    if camera.buffers == Some(identities) {
        return;
    }
    let (Some(view), Some(global)) = (uniforms.uniforms.binding(), globals.buffer.binding()) else {
        return;
    };
    camera.bind_group = Some(device.create_bind_group(
        "source_camera",
        &cache.get_bind_group_layout(&source_camera_layout()),
        &BindGroupEntries::with_indices(((0, view), (11, global))),
    ));
    camera.buffers = Some(identities);
}

pub struct SetSourceViewCamera;
impl<P: PhaseItem> RenderCommand<P> for SetSourceViewCamera {
    type Param = bevy::ecs::system::lifetimeless::SRes<SourceViewCamera>;
    type ViewQuery = &'static bevy::render::view::ViewUniformOffset;
    type ItemQuery = ();
    fn render<'w>(
        _: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        camera: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(group) = &camera.into_inner().bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, group, &[view.offset]);
        RenderCommandResult::Success
    }
}
