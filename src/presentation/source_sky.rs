//! Source procedural sky. All seven view bindings are allocated during loading.
use super::render_targets::{DEPTH_FORMAT, HDR_FORMAT};
use crate::reference::{lighting_scene::LightingRecord, sky::source_sky_vertices};
use bevy::mesh::VertexBufferLayout;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
use bevy::{ecs as bevy_ecs, prelude::*};
#[derive(Clone, Copy, ShaderType)]
struct SkyParams {
    world_from_source: Mat4,
    clip_from_world: Mat4,
    light_direction: Vec4,
    light_radiance: Vec4,
    tint_thickness: Vec4,
    ground_exposure: Vec4,
    sun: Vec4,
}
struct SkyBinding {
    uniform: UniformBuffer<SkyParams>,
    group: BindGroup,
}
#[derive(Resource)]
pub struct SkyPass {
    pipelines: [CachedRenderPipelineId; 2],
    vertices: Buffer,
    bindings: [SkyBinding; super::surface_views::VIEW_COUNT],
}
impl SkyPass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        record: &LightingRecord,
        samples: u32,
    ) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "source_sky",
            &BindGroupLayoutEntries::single(
                ShaderStages::VERTEX_FRAGMENT,
                uniform_buffer::<SkyParams>(false),
            ),
        );
        let pipelines = std::array::from_fn(|index| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(
                    if index == 0 {
                        "source_sky_main"
                    } else {
                        "source_sky_capture"
                    }
                    .into(),
                ),
                layout: vec![layout.clone()],
                vertex: VertexState {
                    shader: shader.clone(),
                    entry_point: Some(
                        if index == 0 {
                            "vertex_main"
                        } else {
                            "vertex_capture"
                        }
                        .into(),
                    ),
                    buffers: vec![VertexBufferLayout {
                        array_stride: 28,
                        step_mode: VertexStepMode::Vertex,
                        attributes: vec![VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        }],
                    }],
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    entry_point: Some(
                        if index == 0 {
                            "fragment_main"
                        } else {
                            "fragment_capture"
                        }
                        .into(),
                    ),
                    targets: vec![Some(ColorTargetState {
                        format: HDR_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                primitive: PrimitiveState {
                    cull_mode: None,
                    ..default()
                },
                depth_stencil: Some(DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(CompareFunction::GreaterEqual),
                    stencil: default(),
                    bias: default(),
                }),
                multisample: MultisampleState {
                    // Native probe scratch inherits its one-sample destination descriptor.
                    count: if index == 0 { samples } else { 1 },
                    ..default()
                },
                ..default()
            })
        });
        let vertices = source_sky_vertices();
        let mut bytes = Vec::with_capacity(vertices.len() * 28);
        for vertex in &vertices {
            for word in vertex
                .position
                .map(f32::to_bits)
                .into_iter()
                .chain([vertex.color])
                .chain(vertex.direction.map(f32::to_bits))
            {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        let vertices = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("source_sky_vertices"),
            contents: &bytes,
            usage: BufferUsages::VERTEX,
        });
        let linear = |value: [f32; 4]| {
            Color::srgba(value[0], value[1], value[2], value[3])
                .to_linear()
                .to_vec4()
        };
        let tint = linear(record.sky.tint)
            .truncate()
            .extend(record.sky.atmosphere_thickness);
        let ground = linear(record.sky.ground)
            .truncate()
            .extend(record.sky.exposure);
        let target_direction = Quat::from_array(record.lights[0].world_pose.rotation) * Vec3::Z;
        let source_direction =
            Vec3::new(target_direction.x, target_direction.y, -target_direction.z);
        let bindings = std::array::from_fn(|_| {
            let mut uniform = UniformBuffer::from(SkyParams {
                world_from_source: Mat4::IDENTITY,
                clip_from_world: Mat4::IDENTITY,
                light_direction: source_direction.extend(0.),
                light_radiance: Vec3::from_array(record.lights[0].radiance).extend(0.),
                tint_thickness: tint,
                ground_exposure: ground,
                sun: Vec4::new(record.sky.sun_size, record.sky.sun_convergence, 0., 0.),
            });
            uniform.write_buffer(device, queue);
            let group = device.create_bind_group(
                "source_sky",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::single(uniform.binding().unwrap()),
            );
            SkyBinding { uniform, group }
        });
        Self {
            pipelines,
            vertices,
            bindings,
        }
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|id| cache.get_render_pipeline(*id).is_some())
    }
    /// View zero is the sky camera. Other indices are the surface views of the same index.
    /// The caller supplies its established source-to-target clip conversion.
    pub fn write_view(
        &mut self,
        index: usize,
        position: Vec3,
        far: f32,
        clip_from_world: Mat4,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let scale = far * 10.;
        let value = self.bindings[index].uniform.get_mut();
        value.world_from_source = Mat4::from_scale_rotation_translation(
            Vec3::new(scale, scale, -scale),
            Quat::IDENTITY,
            position,
        );
        value.clip_from_world = clip_from_world;
        self.bindings[index].uniform.write_buffer(device, queue);
    }
    pub fn encode(
        &self,
        index: usize,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        color: &TextureView,
        resolved: Option<&TextureView>,
        depth: &TextureView,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipelines[usize::from(index != 0)])
        else {
            return false;
        };
        let colors = [Some(RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: resolved.map(|view| &**view),
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_sky"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bindings[index].group, &[]);
        pass.set_vertex_buffer(0, *self.vertices.slice(..));
        pass.draw(0..5040, 0..1);
        true
    }
}

/// Allocate the source sky once. Main and capture pipeline failures remain named.
pub(crate) fn prepare_sky(
    mut commands: Commands,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    existing: Option<Res<SkyPass>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<super::readiness::RendererStateHandle>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
) {
    if status.failure().is_some() {
        return;
    }
    if let Some(pass) = existing {
        for pipeline in pass.pipelines {
            use bevy::shader::ShaderCacheError;
            match cache.get_render_pipeline_state(pipeline) {
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => {}
                CachedPipelineState::Err(error) => {
                    status.fail(format!("Source sky pipeline: {error}"));
                    return;
                }
                _ => {}
            }
        }
        return;
    }
    let Some(assets) = assets else { return };
    commands.insert_resource(SkyPass::new(
        &device,
        &queue,
        &cache,
        assets.shaders.sky.clone(),
        &assets.record,
        antialiasing.samples(),
    ));
}

impl SkyPass {
    pub fn write_live(
        &mut self,
        index: usize,
        scene: &super::lighting_scene::SceneLighting,
        position: Vec3,
        far: f32,
        clip_from_world: Mat4,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        let value = self.bindings[index].uniform.get_mut();
        let direction = scene.direction_to_sun;
        value.light_direction = Vec4::new(direction.x, direction.y, -direction.z, 0.);
        value.light_radiance = scene.sun_radiance;
        value.tint_thickness = scene.sky.tint.truncate().extend(scene.sky.parameters.y);
        value.ground_exposure = scene.sky.ground.truncate().extend(scene.sky.parameters.x);
        self.write_view(index, position, far, clip_from_world, device, queue);
    }
}
