//! Source sky geometry and persistent GPU values.
use super::sim_frame::SimFrameResource;
use super::{lighting::SourceLighting, reference::ReferenceVisuals};
use crate::{app::RuntimeReference, lighting::source_linear, reference::SourceId};
use bevy::asset::RenderAssetUsages;
use bevy::ecs::{
    self as bevy_ecs,
    system::{SystemParamItem, lifetimeless::SRes},
};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
    render_asset::RenderAssets,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::{asset as bevy_asset, reflect as bevy_reflect};

#[derive(Clone, Copy, ShaderType, Default)]
pub struct SkyUniform {
    pub tint: Vec4,
    pub ground: Vec4,
    pub sun_direction: Vec4,
    pub sun_color: Vec4,
    pub parameters: Vec4,
    pub clouds: [Vec4; 2],
    pub cloud_motion: Vec4,
    pub star_color: Vec4,
    pub star_motion: Vec4,
    pub flares: [Vec4; 3],
}
#[derive(Clone)]
pub struct SkyState {
    pub shader: Handle<Shader>,
    pub uniform: SkyUniform,
    pub spawned: bool,
    pub clouds_bound: [bool; 2],
    pub materials: Vec<Handle<SkyMaterial>>,
    /// Source `mainscript.disableSunFlare`: the worn lens hides `sunFlare`.
    pub hide_flare: bool,
}
impl SkyState {
    pub fn new(lighting: &crate::reference::LightingRecord) -> Self {
        Self {
            shader: default(),
            uniform: SkyUniform {
                parameters: Vec4::new(
                    0.,
                    0.,
                    lighting.skybox.floats["_SunSize"],
                    lighting.skybox.floats["_SunSizeConvergence"],
                ),
                ..default()
            },
            spawned: false,
            clouds_bound: [false; 2],
            materials: Vec::with_capacity(8),
            hide_flare: false,
        }
    }
    pub fn bound(&self) -> bool {
        self.spawned && self.clouds_bound.iter().all(|v| *v)
    }
}
#[derive(Resource, Default)]
struct ExtractedSky(SkyUniform);
#[derive(Resource, Default)]
pub struct SkyGpu {
    pub uniform: UniformBuffer<SkyUniform>,
}
#[derive(Component)]
pub struct SkyMesh;
#[derive(Clone, Copy, ShaderType, Default, Debug)]
pub struct SkySettings {
    pub kind: UVec4,
    pub st: [Vec4; 4],
    pub color: Vec4,
}
#[derive(Asset, TypePath, Clone, Debug)]
pub struct SkyMaterial {
    pub settings: SkySettings,
    pub textures: [Handle<Image>; 4],
    pub additive: bool,
    pub order: f32,
}
impl Material for SkyMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_sky.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_sky.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn alpha_mode(&self) -> AlphaMode {
        if self.additive {
            AlphaMode::Add
        } else {
            AlphaMode::Blend
        }
    }
    fn depth_bias(&self) -> f32 {
        // Bevy sorts ascending. Wide gaps keep sky, stars, clouds, and flares in order.
        self.order
    }
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
        ])?];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.format = super::render_targets::DEPTH_FORMAT;
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}
impl AsBindGroup for SkyMaterial {
    type Data = ();
    type Param = (SRes<SkyGpu>, SRes<RenderAssets<GpuImage>>);
    fn label() -> &'static str {
        "source sky"
    }
    fn bind_group_data(&self) {}
    fn unprepared_bind_group(
        &self,
        _: &BindGroupLayout,
        device: &RenderDevice,
        param: &mut SystemParamItem<Self::Param>,
        _: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let (gpu, images) = param;
        // Immutable per-material values are uploaded only when this material is prepared.
        let mut encoded = encase::UniformBuffer::new(Vec::new());
        encoded.write(&self.settings).expect("sky settings layout");
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("source sky settings"),
            contents: encoded.as_ref(),
            usage: BufferUsages::UNIFORM,
        });
        let mut bindings = vec![
            (
                0,
                OwnedBindingResource::Buffer(
                    gpu.uniform
                        .buffer()
                        .ok_or(AsBindGroupError::RetryNextUpdate)?
                        .clone(),
                ),
            ),
            (1, OwnedBindingResource::Buffer(buffer)),
        ];
        for (index, handle) in self.textures.iter().enumerate() {
            let image = images
                .get(handle.id())
                .ok_or(AsBindGroupError::RetryNextUpdate)?;
            bindings.push((
                2 + index as u32 * 2,
                OwnedBindingResource::TextureView(
                    TextureViewDimension::D2,
                    image.texture_view.clone(),
                ),
            ));
            bindings.push((
                3 + index as u32 * 2,
                OwnedBindingResource::Sampler(SamplerBindingType::Filtering, image.sampler.clone()),
            ));
        }
        Ok(UnpreparedBindGroup {
            bindings: BindingResources(bindings),
        })
    }
    fn bind_group_layout_entries(_: &RenderDevice, _: bool) -> Vec<BindGroupLayoutEntry> {
        use binding_types::*;
        BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<SkyUniform>(false),
                uniform_buffer::<SkySettings>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        )
        .to_vec()
    }
}

pub struct SourceSkyPlugin;
impl Plugin for SourceSkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(Update, bind_sky.before(super::materials::bind_materials))
            .add_systems(PostUpdate, sync_sky);
        app.sub_app_mut(RenderApp)
            .init_resource::<ExtractedSky>()
            .add_systems(RenderStartup, init_gpu)
            .add_systems(ExtractSchedule, extract_sky)
            .add_systems(Render, upload_sky.in_set(RenderSystems::PrepareResources));
    }
}
fn init_gpu(mut commands: Commands, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let mut gpu = SkyGpu::default();
    gpu.uniform.write_buffer(&device, &queue);
    commands.insert_resource(gpu);
}
fn extract_sky(source: Extract<Option<Res<SourceLighting>>>, mut sky: ResMut<ExtractedSky>) {
    if let Some(source) = source.as_ref() {
        sky.0 = source.sky.uniform;
    }
}
fn upload_sky(
    sky: Res<ExtractedSky>,
    mut gpu: ResMut<SkyGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    gpu.uniform.set(sky.0);
    gpu.uniform.write_buffer(&device, &queue);
}
#[derive(serde::Deserialize)]
struct Identity {
    reference_id: Option<SourceId>,
}
fn bind_sky(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut source: ResMut<SourceLighting>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    extras: Query<(Entity, &bevy::gltf::GltfExtras)>,
    primitives: Query<(Entity, &ChildOf, &Mesh3d), Without<SkyMesh>>,
) {
    if !visuals.bound || !source.bound {
        return;
    }
    let r = &reference.0.lighting.cycle;
    if !source.sky.spawned {
        let texture = |id| {
            source
                .textures
                .iter()
                .find(|(key, _, _)| *key == id)
                .unwrap()
                .2
                .clone()
        };
        let dummy = texture(r.star_material.textures[0].texture);
        let sky = materials.add(SkyMaterial {
            settings: SkySettings {
                kind: UVec4::new(0, 0, 0, 0),
                ..default()
            },
            textures: std::array::from_fn(|_| dummy.clone()),
            additive: false,
            order: -400000.,
        });
        let mut dome = Sphere::new(1.).mesh().uv(64, 32);
        complete_attributes(&mut dome);
        spawn_mesh(
            &mut commands,
            meshes.add(dome),
            sky.clone(),
            Transform::default(),
        );
        source.sky.materials.push(sky);
        for (index, star) in r.stars.iter().enumerate() {
            let material = materials.add(SkyMaterial {
                settings: SkySettings {
                    kind: UVec4::new(3, index as u32, 0, 0),
                    color: Vec4::from_array(star.start_color),
                    ..default()
                },
                textures: std::array::from_fn(|_| dummy.clone()),
                additive: false,
                order: -300000.,
            });
            spawn_mesh(
                &mut commands,
                meshes.add(star_mesh(star, index as u64 + 1)),
                material.clone(),
                source_pose(&star.pose),
            );
            source.sky.materials.push(material);
        }
        for (index, flare) in r.sun_flares.iter().enumerate() {
            let tex = source
                .textures
                .iter()
                .find(|(id, _, _)| *id == flare.atlas)
                .unwrap()
                .2
                .clone();
            let material = materials.add(SkyMaterial {
                settings: SkySettings {
                    kind: UVec4::new(4, index as u32, 0, 0),
                    color: Vec4::from_array(flare.color),
                    ..default()
                },
                textures: std::array::from_fn(|_| tex.clone()),
                additive: true,
                order: 100000.,
            });
            let entity = spawn_mesh(
                &mut commands,
                meshes.add(flare_mesh(flare)),
                material.clone(),
                Transform::default(),
            );
            commands
                .entity(entity)
                .insert((SunFlare, flare_visibility(source.sky.hide_flare)));
            source.sky.materials.push(material);
        }
        let asset = reference
            .0
            .assets
            .iter()
            .find(|a| a.id == r.sky_geometry.asset)
            .unwrap();
        let scene = visuals
            .handles
            .iter()
            .find(|(path, _)| *path == asset.path)
            .unwrap()
            .1
            .clone();
        commands.spawn((
            bevy::world_serialization::WorldAssetRoot(gltfs.get(&scene).unwrap().scenes[0].clone()),
            source_pose(&r.sky_geometry.pose),
            Visibility::Visible,
        ));
        source.sky.spawned = true;
    }
    for (index, cloud) in r.clouds.iter().enumerate() {
        if source.sky.clouds_bound[index] {
            continue;
        }
        let identity = &reference
            .0
            .nodes
            .iter()
            .find(|n| n.id == cloud.node)
            .unwrap()
            .source;
        let Some((node, _)) = extras.iter().find(|(_, extra)| {
            serde_json::from_str::<Identity>(&extra.value)
                .is_ok_and(|data| data.reference_id.as_ref() == Some(identity))
        }) else {
            continue;
        };
        for (entity, parent, mesh) in &primitives {
            if parent.parent() != node {
                continue;
            }
            let Some(original) = meshes.get(&mesh.0) else {
                continue;
            };
            let mut mesh = original.clone();
            complete_attributes(&mut mesh);
            let names = ["_ScatterMap0", "_ScatterMap1", "_DensityMap", "_TextureMap"];
            let links = names.map(|name| {
                cloud
                    .material
                    .textures
                    .iter()
                    .find(|t| t.property == name)
                    .unwrap()
            });
            let material = materials.add(SkyMaterial {
                settings: SkySettings {
                    kind: UVec4::new(1, index as u32, 0, 0),
                    st: links.map(|t| Vec4::new(t.scale[0], t.scale[1], t.offset[0], t.offset[1])),
                    ..default()
                },
                textures: links.map(|link| {
                    source
                        .textures
                        .iter()
                        .find(|(id, _, _)| *id == link.texture)
                        .unwrap()
                        .2
                        .clone()
                }),
                additive: false,
                order: -100000. * (index + 1) as f32,
            });
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((
                    SkyMesh,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material.clone()),
                    bevy::light::NotShadowCaster,
                    bevy::camera::visibility::NoFrustumCulling,
                    bevy::camera::visibility::RenderLayers::layer(0),
                ));
            source.sky.materials.push(material);
            source.sky.clouds_bound[index] = true;
        }
    }
}
fn complete_attributes(mesh: &mut Mesh) {
    let count = mesh.count_vertices();
    if !mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; count]);
    }
    if !mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; count]);
    }
    if !mesh.contains_attribute(Mesh::ATTRIBUTE_COLOR) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.; 4]; count]);
    }
}
fn spawn_mesh(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    material: Handle<SkyMaterial>,
    transform: Transform,
) -> Entity {
    commands
        .spawn((
            SkyMesh,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            bevy::light::NotShadowCaster,
            bevy::camera::visibility::NoFrustumCulling,
        ))
        .id()
}
/// A source `sunFlare` element.
#[derive(Component)]
pub struct SunFlare;
fn flare_visibility(hide: bool) -> Visibility {
    if hide {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    }
}
pub(super) fn source_pose(p: &crate::reference::Transform) -> Transform {
    Transform {
        translation: Vec3::from_array(p.translation),
        rotation: Quat::from_array(p.rotation),
        scale: Vec3::from_array(p.scale),
    }
}
fn star_mesh(r: &crate::reference::StarReference, mut seed: u64) -> Mesh {
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u32 << 24) as f32
    };
    let count = r.max_particles as usize;
    let mut p = Vec::with_capacity(count * 4);
    let mut n = Vec::with_capacity(count * 4);
    let mut uv = Vec::with_capacity(count * 4);
    let mut colors = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count {
        let y = random() * 2. - 1.;
        let angle = random() * std::f32::consts::TAU;
        let radius = (1. - y * y).sqrt();
        let center = Vec3::new(radius * angle.cos(), y, radius * angle.sin())
            * r.radius
            * Vec3::from_array(r.shape_scale);
        let size = r.size_min + random() * (r.size_max - r.size_min);
        for corner in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
            p.push(center.to_array());
            n.push([size, 0., 0.]);
            uv.push(corner);
            colors.push([1.; 4]);
        }
        let b = i as u32 * 4;
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    quad_mesh(p, n, uv, colors, indices)
}
fn flare_mesh(r: &crate::reference::SunFlareReference) -> Mesh {
    let mut p = Vec::new();
    let mut n = Vec::new();
    let mut uv = Vec::new();
    let mut c = Vec::new();
    let mut indices = Vec::new();
    for (index, e) in r.elements.iter().enumerate() {
        let rect = match e.image {
            0 => [0., 0., 1., 0.5],
            1 => [0., 0.5, 0.5, 0.75],
            2 => [0., 0.75, 0.5, 1.],
            i => {
                let i = i - 3;
                let x = 0.5 + (i % 2) as f32 * 0.25;
                let y = 0.5 + (i / 2) as f32 * 0.125;
                [x, y, x + 0.25, y + 0.125]
            }
        };
        let flags = e.rotate as u32
            | (e.zoom as u32) << 1
            | (e.fade as u32) << 2
            | (e.use_light_color as u32) << 3;
        for (corner, [x, y]) in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .into_iter()
            .enumerate()
        {
            p.push([e.position, e.size, flags as f32]);
            n.push([x * 2. - 1., y * 2. - 1., corner as f32]);
            uv.push([
                rect[0] + x * (rect[2] - rect[0]),
                1. - (rect[1] + y * (rect[3] - rect[1])),
            ]);
            c.push(e.color);
        }
        let b = index as u32 * 4;
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    quad_mesh(p, n, uv, c, indices)
}
fn quad_mesh(
    p: Vec<[f32; 3]>,
    n: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    c: Vec<[f32; 4]>,
    i: Vec<u32>,
) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, c)
    .with_inserted_indices(Indices::U32(i))
}
pub(crate) fn sync_sky(
    sim: Res<SimFrameResource>,
    mut source: ResMut<SourceLighting>,
    mut flares: Query<&mut Visibility, With<SunFlare>>,
) {
    let output = &sim.frame.lighting.cycle;
    let sky = &mut source.sky;
    // Source `sunFlare.SetActive(!disableSunFlare)`. The sunglasses setting does not change it.
    let hide_flare = sim.frame.state.glasses().is_some_and(|g| g.hide_flare);
    if hide_flare != sky.hide_flare {
        sky.hide_flare = hide_flare;
        for mut visibility in &mut flares {
            *visibility = flare_visibility(hide_flare);
        }
    }
    let [x, y, z] = output.directional_euler_degrees.map(f32::to_radians);
    let sun = Quat::from_euler(EulerRot::YXZ, -y, -x, z) * Vec3::Z;
    let active = sim.frame.state.cycle_elapsed_seconds;
    let u = &mut sky.uniform;
    u.tint = Vec4::from_array(output.sky_tint.map(source_linear));
    u.ground = Vec4::from_array(output.ground_color.map(source_linear));
    u.sun_direction = sun.extend(0.);
    u.sun_color = Vec4::from_array(
        output
            .directional_color
            .map(|c| source_linear(c * output.directional_intensity)),
    );
    u.parameters.x = output.sky_exposure;
    u.parameters.y = output.atmosphere_thickness;
    u.clouds = output.clouds.map(|c| {
        Vec4::new(
            source_linear(c.color[0]),
            source_linear(c.color[1]),
            source_linear(c.color[2]),
            c.color[3],
        )
    });
    u.cloud_motion = Vec4::new(
        output.clouds[0].density,
        output.clouds[1].density,
        output.clouds[0].speed * (output.shader_time_seconds / 20.) as f32,
        output.clouds[1].speed * (output.shader_time_seconds / 20.) as f32,
    );
    u.star_color = Vec4::from_array(output.star_color);
    u.star_motion = Vec4::new(
        output.star_rotation_degrees.to_radians(),
        active.min(1.) as f32,
        0.,
        0.,
    );
}
