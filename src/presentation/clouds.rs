//! Source cloud geometry and transparent draws. No full-screen cloud approximation.
use super::render_targets::{DEPTH_FORMAT, HDR_FORMAT};
use crate::reference::lighting_scene::LightingRecord;
use bevy::asset::AssetId;
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::render::{
    mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};

#[derive(Clone, Copy, ShaderType)]
struct CloudParams {
    world_from_local: Mat4,
    clip_from_world: Mat4,
    uv: [Vec4; 4],
    color: Vec4,
    controls: Vec4,
}
struct CloudBinding {
    uniform: UniformBuffer<CloudParams>,
    group: BindGroup,
}
#[derive(Resource)]
pub struct CloudPass {
    pipeline: CachedRenderPipelineId,
    bindings: [CloudBinding; 2],
    models: [Mat4; 2],
    order: [usize; 2],
    mesh: AssetId<Mesh>,
    vertices: u32,
}
impl CloudPass {
    /// Called once after the cloud mesh and source images reach the render world.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        record: &LightingRecord,
        mesh_id: AssetId<Mesh>,
        mesh: &RenderMesh,
        textures: [[&GpuImage; 4]; 2],
        samples: u32,
    ) -> Result<Self, String> {
        // The source Ball has no normals. Bevy's glTF loader expands its indices
        // in order before generating unused flat normals. Position and UV are unchanged.
        if !matches!(mesh.buffer_info, RenderMeshBufferInfo::NonIndexed)
            || mesh.vertex_count != record.sky.cloud_mesh.index_count
        {
            return Err("Source cloud expanded triangle stream changed".into());
        }
        let vertex_layout = mesh
            .layout
            .0
            .get_layout(&[
                Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            ])
            .map_err(|error| error.to_string())?;
        let layout = BindGroupLayoutDescriptor::new(
            "source_cloud",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX_FRAGMENT,
                (
                    uniform_buffer::<CloudParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let blend = BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        };
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("source_cloud".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                buffers: vec![vertex_layout.clone()],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(BlendState {
                        color: blend,
                        alpha: blend,
                    }),
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
                count: samples,
                ..default()
            },
            ..default()
        });
        let root = &record.sky.cloud_root;
        // Makeskystuffvisible recenters this root at the sky camera before culling.
        // The selected non-XR sky camera has world position zero.
        let root_matrix = Mat4::from_scale_rotation_translation(
            Vec3::from_array(root.scale),
            Quat::from_array(root.rotation),
            Vec3::ZERO,
        );
        let mut distances = [0.; 2];
        let mut models = [Mat4::IDENTITY; 2];
        let bindings = std::array::from_fn(|index| {
            let cloud = &record.sky.clouds[index];
            let pose = &cloud.transform;
            let model = root_matrix
                * Mat4::from_scale_rotation_translation(
                    Vec3::from_array(pose.scale),
                    Quat::from_array(pose.rotation),
                    Vec3::from_array(pose.translation),
                );
            models[index] = model;
            let center =
                model.transform_point3(Vec3::from_array(record.sky.cloud_mesh.bounds_center));
            distances[index] = -((center.y * center.y + center.x * center.x) + center.z * center.z);
            let mut uniform = UniformBuffer::from(CloudParams {
                world_from_local: model,
                clip_from_world: Mat4::IDENTITY,
                uv: std::array::from_fn(|slot| {
                    let texture = &cloud.textures[slot];
                    Vec4::new(
                        texture.scale[0],
                        texture.scale[1],
                        texture.offset[0],
                        texture.offset[1],
                    )
                }),
                color: Vec4::from_array(cloud.color),
                controls: Vec4::new(cloud.speed, record.sample.shader_time[0], cloud.density, 0.),
            });
            uniform.write_buffer(device, queue);
            let t = textures[index];
            let group = device.create_bind_group(
                "source_cloud",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::sequential((
                    uniform.binding().unwrap(),
                    &t[0].texture_view,
                    &t[0].sampler,
                    &t[1].texture_view,
                    &t[1].sampler,
                    &t[2].texture_view,
                    &t[2].sampler,
                    &t[3].texture_view,
                    &t[3].sampler,
                )),
            );
            CloudBinding { uniform, group }
        });
        if distances[0] == distances[1] {
            return Err("Source cloud sort requires distinct startup distances".into());
        }
        let order = if distances[0] < distances[1] {
            [0, 1]
        } else {
            [1, 0]
        };
        Ok(Self {
            pipeline,
            bindings,
            models,
            order,
            mesh: mesh_id,
            vertices: mesh.vertex_count,
        })
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.pipeline).is_some()
    }
    /// Encode after the procedural sky into the same source MSAA attachments.
    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        allocator: &MeshAllocator,
        color: &TextureView,
        resolve: Option<&TextureView>,
        depth: &TextureView,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return false;
        };
        let Some(vertices) = allocator.mesh_vertex_slice(&self.mesh) else {
            return false;
        };
        let colors = [Some(RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: resolve.map(|view| &**view),
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_clouds_high_then_low"),
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
        pass.set_vertex_buffer(0, *vertices.buffer.slice(..));
        for index in self.order {
            pass.set_bind_group(0, &self.bindings[index].group, &[]);
            pass.draw(
                vertices.range.start..vertices.range.start + self.vertices,
                0..1,
            );
        }
        true
    }
}

/// Prepare source cloud bindings once their mesh and images have uploaded.
pub(crate) fn prepare_clouds(
    mut commands: Commands,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    existing: Option<Res<CloudPass>>,
    meshes: Res<bevy::render::render_asset::RenderAssets<RenderMesh>>,
    images: Res<bevy::render::render_asset::RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<super::readiness::RendererStateHandle>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
) {
    if let Some(pass) = existing {
        use bevy::shader::ShaderCacheError;
        match cache.get_render_pipeline_state(pass.pipeline) {
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => {}
            CachedPipelineState::Err(error) => {
                status.fail(format!("Source cloud pipeline: {error}"));
            }
            _ => {}
        }
        return;
    }
    if status.failure().is_some() {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let Some(mesh) = meshes.get(assets.cloud_mesh.id()) else {
        return;
    };
    let mut textures = [[None; 4]; 2];
    for (index, cloud) in assets.record.sky.clouds.iter().enumerate() {
        for (slot, texture) in cloud.textures.iter().enumerate() {
            let Some(handle) = assets.textures.get(&texture.texture) else {
                status.fail("Source cloud texture link is missing".into());
                return;
            };
            let Some(image) = images.get(handle.id()) else {
                return;
            };
            textures[index][slot] = Some(image);
        }
    }
    let result = CloudPass::new(
        &device,
        &queue,
        &cache,
        assets.shaders.cloud.clone(),
        &assets.record,
        assets.cloud_mesh.id(),
        mesh,
        textures.map(|row| row.map(|texture| texture.expect("validated cloud slots"))),
        antialiasing.samples(),
    );
    match result {
        Ok(pass) => {
            commands.insert_resource(pass);
        }
        Err(error) => {
            status.fail(format!("Source cloud resources: {error}"));
        }
    }
}

impl CloudPass {
    pub fn write_live(
        &mut self,
        position: Vec3,
        clip_from_world: Mat4,
        scene: &super::lighting_scene::SceneLighting,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) {
        for (index, binding) in self.bindings.iter_mut().enumerate() {
            let value = binding.uniform.get_mut();
            value.world_from_local = Mat4::from_translation(position) * self.models[index];
            value.clip_from_world = clip_from_world;
            value.color = scene.sky.clouds[index];
            value.controls = Vec4::new(
                scene.cloud_speed[index],
                scene.source_time.x,
                scene.sky.cloud_motion[index],
                0.,
            );
            binding.uniform.write_buffer(device, queue);
        }
    }
}
