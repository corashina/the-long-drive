//! Source motion vectors for TAA and motion blur: camera motion from resolved depth, then rigid renderers that moved.
//! Every source renderer uses `m_MotionVectors` 1 (per object). Skinned meshes keep camera motion.
use super::{
    lighting_assets::LightingRenderAssets,
    lighting_scene::SourceWorldOrigin,
    readiness::RendererStateHandle,
    render_targets::{DEPTH_FORMAT, SourceColorTargets},
    renderer_visibility::SourceRenderer,
    surface_graph::{SourceSurface, SourceSurfaceStage, SurfaceViewTargets},
    surface_views::{SourceSurfaceView, SurfaceResources},
    view_frame::SourceViewFrame,
};
use bevy::material::AlphaMode;
use bevy::mesh::{MeshVertexBufferLayoutRef, skinning::SkinnedMesh};
use bevy::pbr::{PreparedMaterial, RenderMaterialInstances, RenderMeshInstances};
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    erased_render_asset::ErasedRenderAssets,
    mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
    render_asset::RenderAssets,
    render_resource::{binding_types::*, *},
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
    sync_world::MainEntity,
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::Arc;

pub const MOTION_FORMAT: TextureFormat = TextureFormat::Rg16Float;

#[derive(Clone, Copy, ShaderType)]
struct MotionCamera {
    raster: Mat4,
    world_from_raster: Mat4,
    current: Mat4,
    previous: Mat4,
}
#[derive(Clone, Copy, ShaderType)]
struct MotionObject {
    current_model: Mat4,
    previous_model: Mat4,
}

/// The model of the last frame that changed the transform.
#[derive(Component)]
pub struct MotionModel(Mat4);

#[derive(Clone, Copy)]
struct MotionRow {
    entity: MainEntity,
    object: MotionObject,
}
/// Renderers that moved since the previous main frame.
#[derive(Resource, Default)]
pub struct MotionRows {
    rows: Vec<MotionRow>,
    frame: u32,
}

/// An unchanged transform keeps its model. Its previous model is therefore the stored one.
/// A reset refreshes every model: transforms can change while motion is off.
fn collect(
    mut commands: Commands,
    frame: Res<SourceViewFrame>,
    origin: Res<SourceWorldOrigin>,
    mut last_origin: Local<Option<[f64; 3]>>,
    added: Query<
        (Entity, &GlobalTransform),
        (
            With<SourceRenderer>,
            With<Mesh3d>,
            Without<MotionModel>,
            Without<SkinnedMesh>,
        ),
    >,
    mut models: ParamSet<(
        Query<
            (Entity, &GlobalTransform, &ViewVisibility, &mut MotionModel),
            Changed<GlobalTransform>,
        >,
        Query<(&GlobalTransform, &mut MotionModel)>,
    )>,
    mut rows: ResMut<MotionRows>,
) {
    rows.rows.clear();
    rows.frame = 0;
    let Some(motion) = frame.main_motion else {
        return;
    };
    for (entity, transform) in &added {
        commands
            .entity(entity)
            .insert(MotionModel(transform.to_matrix()));
    }
    if motion.reset {
        *last_origin = Some(origin.0);
        for (transform, mut model) in &mut models.p1() {
            model.0 = transform.to_matrix();
        }
        rows.frame = frame.frame;
        return;
    }
    // Runtime poses subtract the origin. Move stored models into the current origin.
    let delta = last_origin.replace(origin.0).map_or(Vec3::ZERO, |old| {
        Vec3::from_array(std::array::from_fn(|i| (origin.0[i] - old[i]) as f32))
    });
    let rebase = Mat4::from_translation(-delta);
    for (entity, transform, visibility, mut model) in &mut models.p0() {
        let current = transform.to_matrix();
        let previous = rebase * model.0;
        model.0 = current;
        if !visibility.get() || current.abs_diff_eq(previous, 1e-4) {
            continue;
        }
        rows.rows.push(MotionRow {
            entity: MainEntity::from(entity),
            object: MotionObject {
                current_model: current,
                previous_model: previous,
            },
        });
    }
    rows.frame = frame.frame;
}
fn extract(input: Extract<Res<MotionRows>>, mut output: ResMut<MotionRows>) {
    output.rows.clear();
    output.rows.extend_from_slice(&input.rows);
    output.frame = input.frame;
}

struct ObjectPipeline {
    shader: Handle<Shader>,
    camera: BindGroupLayoutDescriptor,
    object: BindGroupLayoutDescriptor,
}
impl SpecializedMeshPipeline for ObjectPipeline {
    type Key = ();
    fn specialize(
        &self,
        _: (),
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let vertex = layout
            .0
            .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        Ok(RenderPipelineDescriptor {
            label: Some("source_motion_object".into()),
            layout: vec![self.camera.clone(), self.object.clone()],
            vertex: VertexState {
                shader: self.shader.clone(),
                entry_point: Some("object_vertex".into()),
                buffers: vec![vertex],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some("object_fragment".into()),
                targets: vec![Some(MOTION_FORMAT.into())],
                ..default()
            }),
            primitive: PrimitiveState {
                cull_mode: None,
                ..default()
            },
            // Resolved depth holds MSAA sample 0. The bias keeps the pixel center in front of it.
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: DepthBiasState {
                    constant: 16,
                    slope_scale: 1.,
                    clamp: 0.,
                },
            }),
            ..default()
        })
    }
}

struct MotionDraw {
    pipeline: CachedRenderPipelineId,
    offset: u32,
    mesh: AssetId<Mesh>,
}
#[derive(Resource, Default)]
pub struct SourceMotion {
    object_pipeline: Option<ObjectPipeline>,
    pass: Option<MotionPass>,
}
impl SourceMotion {
    /// The pass for these world colors, if it exists.
    pub fn pass(&self, colors: &Arc<SourceColorTargets>) -> Option<&MotionPass> {
        self.pass
            .as_ref()
            .filter(|pass| Arc::ptr_eq(&pass.colors, colors))
    }
}
/// The world view's motion target. Allocate only at startup and physical resize.
pub struct MotionPass {
    colors: Arc<SourceColorTargets>,
    _texture: Texture,
    view: TextureView,
    camera: UniformBuffer<MotionCamera>,
    camera_group: BindGroup,
    object_camera_group: BindGroup,
    camera_pipeline: CachedRenderPipelineId,
    objects: DynamicUniformBuffer<MotionObject>,
    object_group: Option<(BufferId, BindGroup)>,
    draws: Vec<MotionDraw>,
    frame: u32,
    encoded: std::sync::atomic::AtomicU32,
}
impl MotionPass {
    /// Rg16Float uv delta, current minus previous.
    pub fn view(&self) -> &TextureView {
        &self.view
    }
    /// True when the camera pass ran for this frame.
    pub fn encoded(&self, frame: u32) -> bool {
        self.encoded.load(std::sync::atomic::Ordering::Acquire) == frame
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::postprocessing::pipeline_failure(cache, &[self.camera_pipeline])
    }
}

fn layouts() -> [BindGroupLayoutDescriptor; 3] {
    [
        BindGroupLayoutDescriptor::new(
            "source_motion_camera",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (uniform_buffer::<MotionCamera>(false), texture_depth_2d()),
            ),
        ),
        BindGroupLayoutDescriptor::new(
            "source_motion_object_camera",
            &BindGroupLayoutEntries::single(
                ShaderStages::VERTEX,
                uniform_buffer::<MotionCamera>(false),
            ),
        ),
        BindGroupLayoutDescriptor::new(
            "source_motion_object",
            &BindGroupLayoutEntries::single(
                ShaderStages::VERTEX,
                uniform_buffer::<MotionObject>(true),
            ),
        ),
    ]
}

fn prepare(
    mut motion: ResMut<SourceMotion>,
    mut specialized: ResMut<SpecializedMeshPipelines<ObjectPipeline>>,
    surfaces: Option<Res<SurfaceResources>>,
    assets: Option<Res<LightingRenderAssets>>,
    frame: Res<SourceViewFrame>,
    rows: Res<MotionRows>,
    (mesh_instances, material_instances, materials, meshes): (
        Res<RenderMeshInstances>,
        Res<RenderMaterialInstances>,
        Res<ErasedRenderAssets<PreparedMaterial>>,
        Res<RenderAssets<RenderMesh>>,
    ),
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    if frame.main_motion.is_none() || status.failure().is_some() {
        return;
    }
    let (Some(surfaces), Some(assets)) = (surfaces, assets) else {
        return;
    };
    let SourceMotion {
        object_pipeline,
        pass,
    } = &mut *motion;
    let [camera_layout, object_camera_layout, object_layout] = layouts();
    let object_pipeline = object_pipeline.get_or_insert_with(|| ObjectPipeline {
        shader: assets.shaders.motion.clone(),
        camera: object_camera_layout.clone(),
        object: object_layout.clone(),
    });
    let colors = &surfaces.targets.world;
    if pass
        .as_ref()
        .is_none_or(|pass| !Arc::ptr_eq(&pass.colors, colors))
    {
        let depth = surfaces.targets.views[0].depth.sampled.as_ref().unwrap();
        *pass = Some(new_pass(
            &device,
            &queue,
            &cache,
            &assets.shaders.motion,
            colors.clone(),
            depth,
            [camera_layout, object_camera_layout],
        ));
    }
    let pass = pass.as_mut().unwrap();
    if let Some(error) = pass.failure(&cache) {
        status.fail(error);
        return;
    }
    pass.frame = 0;
    pass.draws.clear();
    let (Some(motion), Some(snapshot)) = (frame.main_motion, frame.views[0]) else {
        return;
    };
    let raster = snapshot.raster_projection * snapshot.view_from_world;
    pass.camera.set(MotionCamera {
        raster,
        world_from_raster: raster.inverse(),
        current: motion.clip_from_world,
        previous: motion.previous_clip_from_world,
    });
    pass.camera.write_buffer(&device, &queue);
    pass.objects.clear();
    if rows.frame == frame.frame {
        for row in &rows.rows {
            let Some(mesh) = mesh_instances.mesh_asset_id(row.entity) else {
                continue;
            };
            let opaque = material_instances
                .instances
                .get(&row.entity)
                .and_then(|instance| materials.get(instance.asset_id))
                .is_some_and(|material| {
                    matches!(
                        material.properties.alpha_mode,
                        AlphaMode::Opaque | AlphaMode::Mask(_)
                    )
                });
            let Some(render_mesh) = meshes
                .get(mesh)
                .filter(|m| opaque && m.primitive_topology() == PrimitiveTopology::TriangleList)
            else {
                continue;
            };
            let pipeline =
                match specialized.specialize(&cache, object_pipeline, (), &render_mesh.layout) {
                    Ok(pipeline) => pipeline,
                    Err(error) => {
                        status.fail(format!("Source motion pipeline: {error}"));
                        return;
                    }
                };
            let offset = pass.objects.push(&row.object);
            pass.draws.push(MotionDraw {
                pipeline,
                offset,
                mesh,
            });
        }
    }
    pass.objects.write_buffer(&device, &queue);
    if let Some(buffer) = pass.objects.buffer() {
        let id = buffer.id();
        if pass.object_group.as_ref().is_none_or(|(old, _)| *old != id) {
            let group = device.create_bind_group(
                "source_motion_object",
                &cache.get_bind_group_layout(&object_layout),
                &BindGroupEntries::single(pass.objects.binding().unwrap()),
            );
            pass.object_group = Some((id, group));
        }
    }
    pass.frame = frame.frame;
}

fn new_pass(
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
    shader: &Handle<Shader>,
    colors: Arc<SourceColorTargets>,
    depth: &TextureView,
    [camera_layout, object_camera_layout]: [BindGroupLayoutDescriptor; 2],
) -> MotionPass {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("source_motion"),
        size: Extent3d {
            width: colors.width,
            height: colors.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: MOTION_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&default());
    let mut camera = UniformBuffer::from(MotionCamera {
        raster: Mat4::IDENTITY,
        world_from_raster: Mat4::IDENTITY,
        current: Mat4::IDENTITY,
        previous: Mat4::IDENTITY,
    });
    camera.write_buffer(device, queue);
    let camera_group = device.create_bind_group(
        "source_motion_camera",
        &cache.get_bind_group_layout(&camera_layout),
        &BindGroupEntries::sequential((camera.binding().unwrap(), depth)),
    );
    let object_camera_group = device.create_bind_group(
        "source_motion_object_camera",
        &cache.get_bind_group_layout(&object_camera_layout),
        &BindGroupEntries::single(camera.binding().unwrap()),
    );
    let camera_pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("source_motion_camera".into()),
        layout: vec![camera_layout],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("camera_vertex".into()),
            ..default()
        },
        fragment: Some(FragmentState {
            shader: shader.clone(),
            entry_point: Some("camera_fragment".into()),
            targets: vec![Some(MOTION_FORMAT.into())],
            ..default()
        }),
        ..default()
    });
    MotionPass {
        colors,
        _texture: texture,
        view,
        camera,
        camera_group,
        object_camera_group,
        camera_pipeline,
        objects: DynamicUniformBuffer::default(),
        object_group: None,
        draws: Vec::new(),
        frame: 0,
        encoded: Default::default(),
    }
}

/// Runs after the depth resolve. The object pass reads resolved depth as a read-only attachment.
fn encode(
    view: ViewQuery<(&SourceSurfaceView, &SurfaceViewTargets)>,
    motion: Res<SourceMotion>,
    frame: Res<SourceViewFrame>,
    allocator: Res<MeshAllocator>,
    meshes: Res<RenderAssets<RenderMesh>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (source, targets) = view.into_inner();
    let Some(pass) = motion
        .pass(&targets.0.colors)
        .filter(|pass| source.index == 0 && pass.frame == frame.frame)
    else {
        return;
    };
    let Some(camera_pipeline) = cache.get_render_pipeline(pass.camera_pipeline) else {
        return;
    };
    let encoder = context.command_encoder();
    let colors = [Some(RenderPassColorAttachment {
        view: &pass.view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::NONE.into()),
            store: StoreOp::Store,
        },
    })];
    {
        let mut camera = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_motion_camera"),
            color_attachments: &colors,
            ..default()
        });
        camera.set_pipeline(camera_pipeline);
        camera.set_bind_group(0, &pass.camera_group, &[]);
        camera.draw(0..3, 0..1);
    }
    pass.encoded
        .store(frame.frame, std::sync::atomic::Ordering::Release);
    let Some((_, object_group)) = pass
        .object_group
        .as_ref()
        .filter(|_| !pass.draws.is_empty())
    else {
        return;
    };
    let colors = [Some(RenderPassColorAttachment {
        view: &pass.view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
    })];
    let mut objects = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("source_motion_objects"),
        color_attachments: &colors,
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: &targets.0.depth.attachment,
            depth_ops: None,
            stencil_ops: None,
        }),
        ..default()
    });
    objects.set_bind_group(0, &pass.object_camera_group, &[]);
    for draw in &pass.draws {
        let (Some(pipeline), Some(mesh), Some(vertices)) = (
            cache.get_render_pipeline(draw.pipeline),
            meshes.get(draw.mesh),
            allocator.mesh_vertex_slice(&draw.mesh),
        ) else {
            continue;
        };
        objects.set_pipeline(pipeline);
        objects.set_bind_group(1, object_group, &[draw.offset]);
        objects.set_vertex_buffer(0, *vertices.buffer.slice(..));
        match mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                count,
                index_format,
            } => {
                let Some(indices) = allocator.mesh_index_slice(&draw.mesh) else {
                    continue;
                };
                objects.set_index_buffer(*indices.buffer.slice(..), index_format);
                objects.draw_indexed(
                    indices.range.start..indices.range.start + count,
                    vertices.range.start as i32,
                    0..1,
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                objects.draw(
                    vertices.range.start..vertices.range.start + mesh.vertex_count,
                    0..1,
                );
            }
        }
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<MotionRows>()
        .add_systems(PostUpdate, collect.after(super::view_frame::snapshot));
    app.sub_app_mut(RenderApp)
        .init_resource::<MotionRows>()
        .init_resource::<SourceMotion>()
        .init_resource::<SpecializedMeshPipelines<ObjectPipeline>>()
        .add_systems(ExtractSchedule, extract)
        .add_systems(
            Render,
            prepare
                .after(RenderSystems::PrepareResources)
                .before(super::source_effects::prepare)
                .before(RenderSystems::Render),
        )
        .add_systems(
            SourceSurface,
            encode
                .in_set(SourceSurfaceStage::AfterOpaque)
                .after(super::depth_resolve::resolve)
                .run_if(super::frame_graph::surface_ready),
        );
}
