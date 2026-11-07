//! Source procedural sky. All seven view bindings are allocated during loading.
use super::render_targets::{DEPTH_FORMAT, HDR_FORMAT};
use crate::reference::{lighting_scene::LightingRecord, sky::source_sky_vertices};
use bevy::mesh::VertexBufferLayout;
use bevy::render::{
    render_phase::TrackedRenderPass,
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
    /// Weather: fog color in rgb, sky share in a.
    overcast: Vec4,
    /// Weather: xz world direction to the wet cells, w darkening.
    storm: Vec4,
}
#[derive(Resource)]
pub struct SkyPass {
    pipelines: [CachedRenderPipelineId; 2],
    vertices: Buffer,
    params: [SkyParams; super::surface_views::VIEW_COUNT],
    /// One slot for each view. `upload` sends the frame's changed views in one write.
    uniforms: super::uniform_slots::UniformSlots<SkyParams>,
    groups: [BindGroup; super::surface_views::VIEW_COUNT],
}
impl SkyPass {
    pub fn new(
        device: &RenderDevice,
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
        let params = [SkyParams {
            world_from_source: Mat4::IDENTITY,
            clip_from_world: Mat4::IDENTITY,
            light_direction: source_direction.extend(0.),
            light_radiance: Vec3::from_array(record.lights[0].radiance).extend(0.),
            tint_thickness: tint,
            ground_exposure: ground,
            sun: Vec4::new(record.sky.sun_size, record.sky.sun_convergence, 0., 0.),
            overcast: Vec4::ZERO,
            storm: Vec4::ZERO,
        }; super::surface_views::VIEW_COUNT];
        let mut uniforms =
            super::uniform_slots::UniformSlots::new(device, "source_sky", params.len());
        for (index, value) in params.iter().enumerate() {
            uniforms.set(index, value);
        }
        let layout = cache.get_bind_group_layout(&layout);
        let groups = std::array::from_fn(|index| {
            device.create_bind_group(
                "source_sky",
                &layout,
                &BindGroupEntries::single(uniforms.binding(index)),
            )
        });
        Self {
            pipelines,
            vertices,
            params,
            uniforms,
            groups,
        }
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|id| cache.get_render_pipeline(*id).is_some())
    }
    /// View zero is the sky camera. Other indices are the surface views of the same index.
    /// The caller supplies its established source-to-target clip conversion.
    fn write_view(&mut self, index: usize, position: Vec3, far: f32, clip_from_world: Mat4) {
        let scale = far * 10.;
        let value = &mut self.params[index];
        value.world_from_source = Mat4::from_scale_rotation_translation(
            Vec3::new(scale, scale, -scale),
            Quat::IDENTITY,
            position,
        );
        value.clip_from_world = clip_from_world;
        self.uniforms.set(index, value);
    }
    /// One write for the views that `write_live` changed this frame.
    pub fn upload(&mut self, queue: &RenderQueue) {
        self.uniforms.upload(queue);
    }
    /// Draw the sky into a pass that the caller owns.
    pub fn draw<'a>(
        &'a self,
        index: usize,
        pass: &mut TrackedRenderPass<'a>,
        cache: &'a PipelineCache,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipelines[usize::from(index != 0)])
        else {
            return false;
        };
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, &self.groups[index], &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
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
    cache: Res<PipelineCache>,
    status: Res<super::readiness::RendererStateHandle>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
) {
    if status.failure().is_some() {
        return;
    }
    if let Some(pass) = existing {
        if let Some(error) =
            super::fullscreen::failure(&cache, &pass.pipelines, "Source sky pipeline")
        {
            status.fail(error);
        }
        return;
    }
    let Some(assets) = assets else { return };
    commands.insert_resource(SkyPass::new(
        &device,
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
    ) {
        let value = &mut self.params[index];
        let direction = scene.direction_to_sun;
        value.light_direction = Vec4::new(direction.x, direction.y, -direction.z, 0.);
        value.light_radiance = scene.sun_radiance;
        value.tint_thickness = scene.sky.tint.truncate().extend(scene.sky.parameters.y);
        value.ground_exposure = scene.sky.ground.truncate().extend(scene.sky.parameters.x);
        value.overcast = scene.fog_color_density.truncate().extend(scene.overcast);
        value.storm = scene.storm;
        self.write_view(index, position, far, clip_from_world);
    }
}
