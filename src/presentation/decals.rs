//! Bullet-hole decals: one instanced quad batch per `ShootDecals` prefab. The simulation
//! frame carries the live decals.
use super::lighting::SourceLighting;
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use crate::lighting::source_linear;
use crate::reference::{GUN_DECALS, GunDecal};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    render_asset::RenderAssets,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};
use bevy::shader::ShaderRef;
use bevy::{
    asset::{self as bevy_asset, RenderAssetUsages},
    ecs::{
        self as bevy_ecs,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::*,
    reflect as bevy_reflect,
};
use simulation::interaction::decals::DECAL_CAP;

#[derive(Clone, Copy, ShaderType)]
pub struct DecalUniform {
    /// xyz: stream position, w: root scale.
    poses: [Vec4; DECAL_CAP],
    rotations: [Vec4; DECAL_CAP],
    /// Unit quad to the prefab mesh plane under the root.
    node: Mat4,
    tint: Vec4,
    st: Vec4,
    fog: Vec4,
    ambient: Vec4,
    /// Direction to the sun.
    sun: Vec4,
    sun_color: Vec4,
    /// x: 1 lit, 0 unlit. w: decal count.
    settings: Vec4,
}
impl Default for DecalUniform {
    fn default() -> Self {
        Self {
            poses: [Vec4::ZERO; DECAL_CAP],
            rotations: [Vec4::ZERO; DECAL_CAP],
            node: Mat4::IDENTITY,
            tint: Vec4::ONE,
            st: Vec4::new(1., 1., 0., 0.),
            fog: Vec4::ZERO,
            ambient: Vec4::ZERO,
            sun: Vec4::ZERO,
            sun_color: Vec4::ZERO,
            settings: Vec4::ZERO,
        }
    }
}

#[derive(Asset, TypePath, Clone, Debug)]
pub struct DecalMaterial {
    kind: usize,
    texture: Handle<Image>,
}
impl Material for DecalMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_decals.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_decals.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _: &MaterialPipeline,
        d: &mut RenderPipelineDescriptor,
        l: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The transparent pass draws decals with the particle commands.
        d.layout[1] = super::stained_glass::particle_view_layout();
        d.vertex.buffers = vec![l.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ])?];
        d.primitive.cull_mode = None;
        if let Some(depth) = d.depth_stencil.as_mut() {
            depth.format = super::render_targets::DEPTH_FORMAT;
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}
impl AsBindGroup for DecalMaterial {
    type Data = ();
    type Param = (SRes<DecalGpu>, SRes<RenderAssets<GpuImage>>);
    fn label() -> &'static str {
        "source decals"
    }
    fn bind_group_data(&self) {}
    fn unprepared_bind_group(
        &self,
        _: &BindGroupLayout,
        _: &RenderDevice,
        p: &mut SystemParamItem<Self::Param>,
        _: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let buffer =
            p.0.buffers
                .get(self.kind)
                .and_then(|b| b.buffer())
                .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let image =
            p.1.get(self.texture.id())
                .ok_or(AsBindGroupError::RetryNextUpdate)?;
        Ok(UnpreparedBindGroup {
            bindings: BindingResources(vec![
                (0, OwnedBindingResource::Buffer(buffer.clone())),
                (
                    1,
                    OwnedBindingResource::TextureView(
                        TextureViewDimension::D2,
                        image.texture_view.clone(),
                    ),
                ),
                (
                    2,
                    OwnedBindingResource::Sampler(
                        SamplerBindingType::Filtering,
                        image.sampler.clone(),
                    ),
                ),
            ]),
        })
    }
    fn bind_group_layout_entries(_: &RenderDevice, _: bool) -> Vec<BindGroupLayoutEntry> {
        use binding_types::*;
        BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<DecalUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        )
        .to_vec()
    }
}

#[derive(Resource, Default)]
pub struct DecalGpu {
    buffers: Vec<UniformBuffer<DecalUniform>>,
}

/// One batch per decal kind. A kind without a texture has no batch.
#[derive(Resource, Default)]
pub(crate) struct DecalRender {
    prepared: bool,
    entities: [Option<Entity>; GUN_DECALS],
    uniforms: Vec<DecalUniform>,
}

#[derive(Resource, Default)]
pub(crate) struct ExtractedDecals {
    uniforms: Vec<DecalUniform>,
}

pub struct SourceDecalPlugin;
impl Plugin for SourceDecalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DecalRender>()
            .add_plugins(MaterialPlugin::<DecalMaterial>::default())
            .add_systems(
                PostUpdate,
                sync_decals
                    .after(bevy::transform::TransformSystems::Propagate)
                    .after(super::lighting::sync_lighting)
                    .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
            );
        app.sub_app_mut(RenderApp)
            .init_resource::<DecalGpu>()
            .init_resource::<ExtractedDecals>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(Render, upload.in_set(RenderSystems::PrepareResources));
    }
}

fn extract(state: Extract<Res<DecalRender>>, mut extracted: ResMut<ExtractedDecals>) {
    extracted
        .uniforms
        .resize(state.uniforms.len(), DecalUniform::default());
    extracted.uniforms.copy_from_slice(&state.uniforms);
}

fn upload(
    extracted: Res<ExtractedDecals>,
    mut gpu: ResMut<DecalGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    while gpu.buffers.len() < extracted.uniforms.len() {
        gpu.buffers.push(UniformBuffer::default());
    }
    for (i, value) in extracted.uniforms.iter().enumerate() {
        // Hidden empty batches need no upload after their initial buffer creation.
        if value.settings.w == 0. && gpu.buffers[i].buffer().is_some() {
            continue;
        }
        gpu.buffers[i].set(*value);
        gpu.buffers[i].write_buffer(&device, &queue);
    }
}

/// `DECAL_CAP` unit quads. Vertex z is the instance index.
fn mesh() -> Mesh {
    let mut positions = Vec::with_capacity(DECAL_CAP * 4);
    let mut uv = Vec::with_capacity(DECAL_CAP * 4);
    let mut indices = Vec::with_capacity(DECAL_CAP * 6);
    for i in 0..DECAL_CAP {
        let z = i as f32;
        positions.extend([
            [-0.5, -0.5, z],
            [0.5, -0.5, z],
            [0.5, 0.5, z],
            [-0.5, 0.5, z],
        ]);
        uv.extend([[0., 1.], [1., 1.], [1., 0.], [0., 0.]]);
        let k = i as u32 * 4;
        indices.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(indices))
}

/// The unit quad in the plane of the two widest mesh bounds axes, under the prefab root.
fn node_matrix(d: &GunDecal) -> Mat4 {
    let e = Vec3::from_array(d.mesh.bounds_extents) * 2.;
    let (a, b) = if e.z <= e.x && e.z <= e.y {
        (Vec3::X * e.x, Vec3::Y * e.y)
    } else if e.y <= e.x {
        (Vec3::X * e.x, Vec3::Z * e.z)
    } else {
        (Vec3::Z * e.z, Vec3::Y * e.y)
    };
    let quad = Mat4::from_cols(
        a.extend(0.),
        b.extend(0.),
        Vec4::ZERO,
        Vec3::from_array(d.mesh.bounds_center).extend(1.),
    );
    let t = &d.local_pose;
    Mat4::from_scale_rotation_translation(
        Vec3::from_array(t.scale),
        Quat::from_array(t.rotation),
        Vec3::from_array(t.translation),
    ) * quad
}

fn linear(c: [f32; 4]) -> Vec4 {
    Vec4::new(
        source_linear(c[0]),
        source_linear(c[1]),
        source_linear(c[2]),
        c[3],
    )
}

fn setup(
    state: &mut DecalRender,
    decals: &[GunDecal; GUN_DECALS],
    lighting: &SourceLighting,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<DecalMaterial>,
) {
    let mesh = meshes.add(mesh());
    state.uniforms = vec![DecalUniform::default(); GUN_DECALS];
    for (kind, d) in decals.iter().enumerate() {
        let m = &d.material;
        let Some(t) = m
            .textures
            .iter()
            .find(|t| t.property == "_MainTex")
            .or(m.textures.first())
        else {
            continue;
        };
        let Some((_, _, texture)) = lighting.textures.iter().find(|(id, _, _)| *id == t.texture)
        else {
            continue;
        };
        let shader = m.shader.name.as_str();
        // Legacy particle shaders double `_TintColor`.
        let tint = match (m.colors.get("_Color"), m.colors.get("_TintColor")) {
            (Some(c), _) => linear(*c),
            (None, Some(c)) => linear(*c) * Vec4::new(2., 2., 2., 2.),
            _ => Vec4::ONE,
        };
        let lit = !(shader.contains("Particles") || shader.contains("Unlit"));
        let u = &mut state.uniforms[kind];
        u.node = node_matrix(d);
        u.tint = tint;
        u.st = Vec4::new(t.scale[0], t.scale[1], t.offset[0], t.offset[1]);
        u.settings.x = f32::from(u8::from(lit));
        let material = materials.add(DecalMaterial {
            kind,
            texture: texture.clone(),
        });
        let identity = 0x4000_0000 | kind as u32;
        state.entities[kind] = Some(
            commands
                .spawn((
                    super::renderer_visibility::SourceSortData {
                        material_key: identity,
                        material_identity: identity,
                        glass_source: 0,
                        shader_identity: m.shader.source.path_id as i32,
                        source_material_index: 0,
                        layer_and_order: 0,
                        queue: 3000,
                        additive: false,
                        glass: false,
                        grab: false,
                    },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    bevy::light::NotShadowCaster,
                    bevy::camera::visibility::NoFrustumCulling,
                    bevy::camera::primitives::Aabb {
                        center: bevy::math::Vec3A::ZERO,
                        half_extents: bevy::math::Vec3A::ZERO,
                    },
                ))
                .id(),
        );
    }
    state.prepared = true;
}

#[allow(clippy::too_many_arguments)]
fn sync_decals(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    sim: Res<SimFrameResource>,
    lighting: Res<SourceLighting>,
    mut state: ResMut<DecalRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DecalMaterial>>,
    mut visibility: Query<(&mut Visibility, &mut GlobalTransform)>,
) {
    if !lighting.bound {
        return;
    }
    let state = &mut *state;
    if !state.prepared {
        setup(
            state,
            &reference.0.guns.decals,
            &lighting,
            &mut commands,
            &mut meshes,
            &mut materials,
        );
    }
    let frame = &sim.frame;
    let cycle = &frame.lighting.cycle;
    let [x, y, z] = cycle.directional_euler_degrees.map(f32::to_radians);
    let sun = Quat::from_euler(EulerRot::YXZ, -y, -x, z) * Vec3::Z;
    let sun_color = linear(
        cycle
            .directional_color
            .map(|c| c * cycle.directional_intensity),
    );
    let fog = linear(cycle.fog_color).truncate().extend(cycle.fog_density);
    let ambient = lighting.uniform.ambient.truncate().extend(0.);
    for u in &mut state.uniforms {
        u.settings.w = 0.;
        u.fog = fog;
        u.ambient = ambient;
        u.sun = sun.extend(0.);
        u.sun_color = sun_color;
    }
    // The newest decal of each kind places its batch for the transparent sort.
    let mut newest = [Vec3::ZERO; GUN_DECALS];
    for row in &frame.decals {
        let kind = row.kind as usize;
        let Some(u) = state.uniforms.get_mut(kind) else {
            continue;
        };
        let pose = super::interpolate(row.previous.pose(), row.current.pose(), frame.state.alpha);
        let n = u.settings.w as usize;
        let position = Vec3::from_array(pose.position);
        u.poses[n] = position.extend(row.scale);
        u.rotations[n] = Vec4::from_array(pose.rotation);
        u.settings.w += 1.;
        newest[kind] = position;
    }
    for (kind, entity) in state.entities.iter().enumerate() {
        let Some(entity) = entity else { continue };
        let Ok((mut v, mut transform)) = visibility.get_mut(*entity) else {
            continue;
        };
        let shown = state.uniforms[kind].settings.w > 0.;
        if shown {
            *transform = GlobalTransform::from_translation(newest[kind]);
        }
        v.set_if_neq(if shown {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }
}
