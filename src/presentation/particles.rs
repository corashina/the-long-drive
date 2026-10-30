//! Retained particle uniform buffers. The simulation frame carries the live particles.
use super::lighting::SourceLighting;
use super::sim_frame::{ParticleGroupKey, SimFrameResource};
use crate::{
    app::RuntimeReference,
    particles::TextureSheet,
    reference::{ItemId, ParticleReference},
    weather::StormId,
};
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
use simulation::boundary::ParticleGroupRow;
const BATCH: usize = 128;
#[derive(Clone, Copy, ShaderType)]
pub struct ParticleUniform {
    positions: [Vec4; BATCH],
    colors: [Vec4; BATCH],
    angles: [Vec4; BATCH],
    tint: Vec4,
    st: Vec4,
    fog: Vec4,
    settings: Vec4,
    /// x: soft-particle InvFade (0 off), y: distortion strength scaled (0 off), z: distortion blend, w: bump scale.
    effect: Vec4,
    /// FX/Glass/Stained BumpDistort: x `_BumpAmt`, y 1 (0 for the other shaders).
    glass: Vec4,
    /// `_BumpMap_ST` of the glass shader.
    bump_st: Vec4,
}
impl Default for ParticleUniform {
    fn default() -> Self {
        Self {
            positions: [Vec4::ZERO; BATCH],
            colors: [Vec4::ZERO; BATCH],
            angles: [Vec4::ZERO; BATCH],
            tint: Vec4::ONE,
            st: Vec4::new(1., 1., 0., 0.),
            fog: Vec4::ZERO,
            settings: Vec4::ZERO,
            effect: Vec4::ZERO,
            glass: Vec4::ZERO,
            bump_st: Vec4::new(1., 1., 0., 0.),
        }
    }
}
#[derive(Asset, TypePath, Clone, Debug)]
pub struct ParticleMaterial {
    batch: usize,
    texture: Handle<Image>,
    /// Distortion `_BumpMap`; the main texture when the material has none.
    bump: Handle<Image>,
    additive: bool,
    order: f32,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParticleKey {
    additive: bool,
}
/// Source particle shader families selected by the exported material.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParticleShader {
    /// Legacy Shaders/Particles/Alpha Blended.
    AlphaBlended,
    /// Legacy Shaders/Particles/Additive.
    Additive,
    /// Mobile/Particles/Additive: no tint, no soft particles.
    MobileAdditive,
    /// Particles/Standard Unlit and other `_Color` alpha-blended particles.
    Unlit,
    /// FX/Glass/Stained BumpDistort: the grab texture offset by `_BumpMap`, times `_MainTex` and `_Color`.
    StainedBumpDistort,
}
impl ParticleShader {
    fn new(name: &str) -> Self {
        match name {
            "Legacy Shaders/Particles/Alpha Blended" => Self::AlphaBlended,
            "Legacy Shaders/Particles/Additive" => Self::Additive,
            "Mobile/Particles/Additive" => Self::MobileAdditive,
            "FX/Glass/Stained BumpDistort" => Self::StainedBumpDistort,
            _ => Self::Unlit,
        }
    }
    fn legacy(self) -> bool {
        matches!(self, Self::AlphaBlended | Self::Additive)
    }
    fn additive(self) -> bool {
        matches!(self, Self::Additive | Self::MobileAdditive)
    }
}
pub(crate) type DrawSourceParticle = (
    bevy::render::render_phase::SetItemPipeline,
    bevy::pbr::SetMeshViewBindGroup<0>,
    super::stained_glass::SetSourceParticleView,
    bevy::pbr::SetMeshBindGroup<2>,
    bevy::pbr::SetMaterialBindGroup<3>,
    bevy::pbr::DrawMesh,
);
impl Material for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_particles.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_particles.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn depth_bias(&self) -> f32 {
        self.order
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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        d.layout[1] = super::stained_glass::particle_view_layout();
        if key.bind_group_data.additive {
            // Source Blend SrcAlpha One, ColorMask RGB.
            if let Some(target) = d
                .fragment
                .as_mut()
                .and_then(|f| f.targets.first_mut())
                .and_then(|t| t.as_mut())
            {
                target.blend = Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent::OVER,
                });
                target.write_mask = ColorWrites::COLOR;
            }
        }
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
#[derive(Resource, Default)]
pub struct ParticleGpu {
    buffers: Vec<UniformBuffer<ParticleUniform>>,
}
impl AsBindGroup for ParticleMaterial {
    type Data = ParticleKey;
    type Param = (SRes<ParticleGpu>, SRes<RenderAssets<GpuImage>>);
    fn label() -> &'static str {
        "source particles"
    }
    fn bind_group_data(&self) -> ParticleKey {
        ParticleKey {
            additive: self.additive,
        }
    }
    fn unprepared_bind_group(
        &self,
        _: &BindGroupLayout,
        _: &RenderDevice,
        p: &mut SystemParamItem<Self::Param>,
        _: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let buffer =
            p.0.buffers
                .get(self.batch)
                .and_then(|b| b.buffer())
                .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let image =
            p.1.get(self.texture.id())
                .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let bump =
            p.1.get(self.bump.id())
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
                (
                    3,
                    OwnedBindingResource::TextureView(
                        TextureViewDimension::D2,
                        bump.texture_view.clone(),
                    ),
                ),
                (
                    4,
                    OwnedBindingResource::Sampler(
                        SamplerBindingType::Filtering,
                        bump.sampler.clone(),
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
                uniform_buffer::<ParticleUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        )
        .to_vec()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Urine,
    Brush(usize),
    Spray(usize),
    Wash(usize),
    Outlet(u32),
    /// Spray bottle spray (0) and splash (1).
    SprayBottle(usize),
    /// One particle system (`ParticleGroupRow::CAR_*`) of the car with chassis body index `car`.
    Car {
        system: u32,
        prototype: usize,
        car: u32,
    },
    /// Storm dust; `Group::source` is big (0) or small (1).
    Storm(crate::weather::StormId),
    /// `mainscript` melee hit systems; `Group::source` is the system.
    Melee,
    /// `mainscript` shot tracers; `Group::source` is start (0) or end (1).
    Tracer,
    /// `explosionscript.particle`; `Group::source` is the explosion type.
    Explosion,
    /// `mainscript.FoodParticles`; `Group::source` is the food particle type.
    Food,
    /// The contrail of the sky plane with this id; `Group::source` is the prefab.
    Sky(u32),
    /// The `emberScript` particle of one cigarette.
    Ember(crate::reference::ItemId),
    /// A loot tool: its item and `conditions.prototype_tools` index; `Group::source` is the system.
    LootTool {
        item: crate::reference::ItemId,
        tool: usize,
    },
    /// Player mouth smoke (`fpscontroller.smokeParticle`).
    Exhale,
    /// `fpscontroller.heatHaze`: the old particle heat haze.
    HeatHaze,
    /// A washtriggerscript `particleOnDestroy`; the wash effect.
    Cobweb(usize),
}
/// The particle record of one car group.
fn car_particle(
    reference: &crate::reference::ReferenceManifest,
    system: u32,
    prototype: usize,
    source: usize,
) -> &crate::reference::ParticleReference {
    let record = crate::starting_area::world_vehicle(reference, prototype);
    match system {
        ParticleGroupRow::CAR_EXHAUST => &record.exhaust[source],
        ParticleGroupRow::CAR_STEAM => {
            &record
                .coolant_steam
                .as_ref()
                .expect("coolant steam group")
                .particle
        }
        ParticleGroupRow::CAR_WHEEL => {
            let w = &record.wheel_effects[source / 8];
            match source % 8 {
                0 => w.dust.as_ref().expect("wheel dust group"),
                system => &w.grass[system - 1],
            }
        }
        _ => &reference.skid.particles[source],
    }
}
struct Group {
    owner: Owner,
    source: usize,
    start: usize,
    count: usize,
    order: Vec<usize>,
    entities: Vec<Entity>,
    /// Additive source particles fog toward black.
    black_fog: bool,
    sheet: Option<TextureSheet>,
    /// Material `_MainTex` tiling. Scales the per-particle sheet tile offset.
    tiling: Vec2,
    /// False for the retained slots of a removed storm.
    active: bool,
}
#[derive(Resource, Default)]
pub(crate) struct ParticleRender {
    mesh: Option<Handle<Mesh>>,
    groups: Vec<Group>,
    uniforms: Vec<ParticleUniform>,
}
impl ParticleRender {
    pub(crate) fn prepared(&self) -> bool {
        self.mesh.is_some()
    }
}
#[derive(Resource, Default)]
pub(crate) struct ExtractedParticles {
    pub(crate) uniforms: Vec<ParticleUniform>,
}
pub struct SourceParticlePlugin;
impl Plugin for SourceParticlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleRender>()
            .add_plugins(MaterialPlugin::<ParticleMaterial>::default())
            .add_systems(
                PostUpdate,
                sync_particles
                    .after(bevy::transform::TransformSystems::Propagate)
                    .after(super::lighting::sync_lighting)
                    .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
            );
        app.sub_app_mut(RenderApp)
            .init_resource::<ParticleGpu>()
            .init_resource::<ExtractedParticles>()
            .add_systems(ExtractSchedule, extract)
            .add_systems(Render, upload.in_set(RenderSystems::PrepareResources));
        bevy::render::render_phase::AddRenderCommand::add_render_command::<
            bevy::core_pipeline::core_3d::Transparent3d,
            DrawSourceParticle,
        >(app.sub_app_mut(RenderApp));
    }
}
fn extract(state: Extract<Res<ParticleRender>>, mut extracted: ResMut<ExtractedParticles>) {
    extracted
        .uniforms
        .resize(state.uniforms.len(), ParticleUniform::default());
    extracted.uniforms.copy_from_slice(&state.uniforms);
}
fn upload(
    extracted: Res<ExtractedParticles>,
    mut gpu: ResMut<ParticleGpu>,
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
fn mesh() -> Mesh {
    let mut positions = Vec::with_capacity(BATCH * 4);
    let mut uv = Vec::with_capacity(BATCH * 4);
    let mut indices = Vec::with_capacity(BATCH * 6);
    for i in 0..BATCH {
        positions.extend([
            [-0.5, -0.5, i as f32],
            [0.5, -0.5, i as f32],
            [0.5, 0.5, i as f32],
            [-0.5, 0.5, i as f32],
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
fn add_group(
    state: &mut ParticleRender,
    owner: Owner,
    index: usize,
    r: &ParticleReference,
    lighting: &SourceLighting,
    commands: &mut Commands,
    materials: &mut Assets<ParticleMaterial>,
) {
    // A removed storm's retained slots serve the next storm of the same system.
    if let Some(g) = state.groups.iter_mut().find(|g| {
        matches!((g.owner, owner), (Owner::Storm(_), Owner::Storm(_)))
            && !g.active
            && g.source == index
    }) {
        g.owner = owner;
        g.active = true;
        return;
    }
    let start = state.uniforms.len();
    let count = (r.capacity as usize).div_ceil(BATCH);
    let m = &r.materials[0];
    // Without `_MainTex` (the gun tracers) Unity samples its white default texture.
    let t = m.textures.iter().find(|t| t.property == "_MainTex");
    let image = |id: u32| {
        lighting
            .textures
            .iter()
            .find(|(texture, _, _)| *texture == id)
            .unwrap()
            .2
            .clone()
    };
    let texture = t.map_or_else(Handle::default, |t| image(t.texture));
    let shader = ParticleShader::new(&m.shader.name);
    let legacy = shader.legacy();
    let float = |name: &str| m.floats.get(name).copied().unwrap_or(0.);
    let glass = shader == ParticleShader::StainedBumpDistort;
    // Particles/Standard Unlit EFFECT_BUMP reads `_GrabTexture` through `_BumpMap`.
    let bump_texture = m.textures.iter().find(|t| t.property == "_BumpMap");
    let bump = bump_texture.filter(|_| float("_DistortionEnabled") == 1. && !glass);
    let tint = match shader {
        ParticleShader::MobileAdditive => [1.; 4],
        _ if legacy => m.colors["_TintColor"],
        _ => m.colors["_Color"],
    };
    let sheet = TextureSheet::new(r);
    // Unity applies `_MainTex_ST` after the sheet tile: uv * tile * scale + tile offset * scale + offset.
    let tile = sheet.as_ref().map_or(Vec2::ONE, |s| {
        Vec2::ONE / UVec2::from_array(s.tiles).as_vec2()
    });
    let (tiling, offset) = t.map_or((Vec2::ONE, Vec2::ZERO), |t| {
        (Vec2::from_array(t.scale), Vec2::from_array(t.offset))
    });
    let mut uniform = ParticleUniform::default();
    uniform.tint = Vec4::from_array(linear(tint));
    uniform.st = (tiling * tile).extend(offset.x).extend(offset.y);
    uniform.settings = Vec4::new(
        if legacy { 2. } else { 1. },
        r.scalars["renderer.m_MinParticleSize"],
        r.scalars["renderer.m_MaxParticleSize"],
        0.,
    );
    // Source Ultra enables SOFTPARTICLES_ON. Only the legacy shaders compile the fade.
    if glass {
        let t = bump_texture.expect("glass particle _BumpMap");
        uniform.glass = Vec4::new(float("_BumpAmt"), 1., 0., 0.);
        uniform.bump_st = Vec4::new(t.scale[0], t.scale[1], t.offset[0], t.offset[1]);
    }
    uniform.effect = Vec4::new(
        if legacy { float("_InvFade") } else { 0. },
        if bump.is_some() {
            float("_DistortionStrengthScaled")
        } else {
            0.
        },
        float("_DistortionBlend"),
        float("_BumpScale"),
    );
    let bump = bump
        .or(bump_texture.filter(|_| glass))
        .map_or_else(|| texture.clone(), |b| image(b.texture));
    let mut entities = Vec::with_capacity(count);
    for i in 0..count {
        state.uniforms.push(uniform);
        let material = materials.add(ParticleMaterial {
            batch: start + i,
            texture: texture.clone(),
            bump: bump.clone(),
            additive: shader.additive(),
            order: -r.scalars["renderer.m_SortingFudge"],
        });
        entities.push(
            commands
                .spawn((
                    super::renderer_visibility::SourceSortData {
                        material_key: (start + i) as u32,
                        material_identity: (start + i) as u32,
                        glass_source: 0,
                        shader_identity: m.shader.source.path_id as i32,
                        source_material_index: 0,
                        layer_and_order: 0,
                        queue: 3000,
                        additive: false,
                        glass: false,
                        grab: uniform.effect.y != 0. || glass,
                    },
                    Mesh3d(state.mesh.as_ref().unwrap().clone()),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    bevy::light::NotShadowCaster,
                    bevy::camera::visibility::NoFrustumCulling,
                    bevy::camera::primitives::Aabb {
                        center: bevy::math::Vec3A::ZERO,
                        half_extents: bevy::math::Vec3A::ZERO,
                    },
                    bevy::camera::visibility::RenderLayers::layer(
                        r.scalars["gameObject.layer"] as usize,
                    ),
                ))
                .id(),
        );
    }
    state.groups.push(Group {
        owner,
        source: index,
        start,
        count,
        order: Vec::with_capacity(r.capacity as usize),
        entities,
        black_fog: shader.additive(),
        sheet,
        tiling,
        active: true,
    });
}
#[allow(clippy::too_many_arguments)]
fn sync_particles(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    sim: Res<SimFrameResource>,
    lighting: Res<SourceLighting>,
    mut state: ResMut<ParticleRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
    mut visibility: Query<
        (&mut Visibility, &mut GlobalTransform),
        Without<super::camera::DriveCamera>,
    >,
    camera: Query<&GlobalTransform, With<super::camera::DriveCamera>>,
) {
    if !lighting.bound {
        return;
    }
    let references = &reference.0.particles;
    if state.mesh.is_none() {
        state.mesh = Some(meshes.add(mesh()));
        for (i, p) in references.iter().enumerate() {
            let owner = if p.role.starts_with("urine") {
                Owner::Urine
            } else {
                Owner::Outlet(p.role.rsplit('_').next().unwrap().parse().unwrap())
            };
            add_group(
                &mut state,
                owner,
                i,
                p,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
        for (wash, w) in reference.0.conditions.washes.iter().enumerate() {
            for (particle, p) in w.particles.iter().enumerate() {
                add_group(
                    &mut state,
                    Owner::Wash(wash),
                    particle,
                    p,
                    &lighting,
                    &mut commands,
                    &mut materials,
                );
            }
        }
        for (tool, b) in reference.0.conditions.brushes.iter().enumerate() {
            for (particle, p) in b.tool.particles.iter().enumerate() {
                add_group(
                    &mut state,
                    Owner::Brush(tool),
                    particle,
                    p,
                    &lighting,
                    &mut commands,
                    &mut materials,
                );
            }
        }
        let bottle = &reference.0.small_items.spray_bottle;
        for (index, p) in [&bottle.spray, &bottle.splash].into_iter().enumerate() {
            add_group(
                &mut state,
                Owner::SprayBottle(index),
                0,
                p,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
        for (tool, b) in reference.0.conditions.sprays.iter().enumerate() {
            for (particle, p) in b.tool.particles.iter().enumerate() {
                add_group(
                    &mut state,
                    Owner::Spray(tool),
                    particle,
                    p,
                    &lighting,
                    &mut commands,
                    &mut materials,
                );
            }
        }
        for (system, p) in reference.0.melee.particles.iter().enumerate() {
            add_group(
                &mut state,
                Owner::Melee,
                system,
                p,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
        let tracer = &reference.0.guns.tracer;
        for (system, p) in [&tracer.start, &tracer.end].into_iter().enumerate() {
            add_group(
                &mut state,
                Owner::Tracer,
                system,
                p,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
        for (kind, t) in reference.0.explosions.types.iter().enumerate() {
            add_group(
                &mut state,
                Owner::Explosion,
                kind,
                &t.particle,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
        for (kind, p) in reference.0.survival.food_particles.iter().enumerate() {
            add_group(
                &mut state,
                Owner::Food,
                kind,
                p,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
        add_group(
            &mut state,
            Owner::Exhale,
            0,
            &reference.0.small_items.exhale.particle,
            &lighting,
            &mut commands,
            &mut materials,
        );
        add_group(
            &mut state,
            Owner::HeatHaze,
            0,
            &reference.0.small_items.heat_haze,
            &lighting,
            &mut commands,
            &mut materials,
        );
        for (effect, e) in reference.0.small_items.wash_effects.iter().enumerate() {
            add_group(
                &mut state,
                Owner::Cobweb(effect),
                0,
                &e.particle,
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
    }
    let groups = &sim.frame.particle_groups;
    // A dynamic owner keeps its group while the frame has it. Free groups serve the next owner.
    for g in &mut state.groups {
        if matches!(
            g.owner,
            Owner::Ember(_)
                | Owner::Car { .. }
                | Owner::Storm(_)
                | Owner::Sky(_)
                | Owner::LootTool { .. }
        ) {
            g.active = sim
                .particle_group(key(g.owner, g.source))
                .is_some_and(|(row, _)| row.count > 0 || !matches!(g.owner, Owner::Ember(_)));
        }
    }
    let ember = crate::presentation::lighting::ember_particle(&reference.0);
    for row in groups.iter().filter(|r| r.kind == ParticleGroupRow::EMBER) {
        let Some(ember) = ember else {
            break;
        };
        let owner = Owner::Ember(ItemId(row.a));
        if row.count == 0 || state.groups.iter().any(|g| g.active && g.owner == owner) {
            continue;
        }
        if let Some(g) = state
            .groups
            .iter_mut()
            .find(|g| !g.active && matches!(g.owner, Owner::Ember(_)))
        {
            g.owner = owner;
            g.active = true;
            continue;
        }
        add_group(
            &mut state,
            owner,
            0,
            ember,
            &lighting,
            &mut commands,
            &mut materials,
        );
    }
    // Loot tools come and go. A gone tool frees its groups for the next copy of its prototype.
    for row in groups
        .iter()
        .filter(|r| r.kind == ParticleGroupRow::LOOT_TOOL)
    {
        let (tool, source) = (row.b as usize, row.source as usize);
        let owner = Owner::LootTool {
            item: ItemId(row.a),
            tool,
        };
        if state
            .groups
            .iter()
            .any(|g| g.active && g.owner == owner && g.source == source)
        {
            continue;
        }
        if let Some(g) = state.groups.iter_mut().find(|g| {
            !g.active
                && g.source == source
                && matches!(g.owner, Owner::LootTool { tool: t, .. } if t == tool)
        }) {
            g.owner = owner;
            g.active = true;
            continue;
        }
        add_group(
            &mut state,
            owner,
            source,
            loot_tool_particle(&reference.0, tool, source),
            &lighting,
            &mut commands,
            &mut materials,
        );
    }
    // Car copies come and go. A retired car frees its groups for the next copy of its prototype.
    for row in groups.iter().filter(|r| r.kind == ParticleGroupRow::CAR) {
        let (system, prototype, source) = (row.a, row.b as usize, row.source as usize);
        let owner = Owner::Car {
            system,
            prototype,
            car: row.c,
        };
        if state
            .groups
            .iter()
            .any(|g| g.active && g.owner == owner && g.source == source)
        {
            continue;
        }
        if let Some(g) = state.groups.iter_mut().find(|g| {
            !g.active
                && g.source == source
                && matches!(g.owner, Owner::Car { system: s, prototype: p, .. } if s == system && p == prototype)
        }) {
            g.owner = owner;
            g.active = true;
            continue;
        }
        add_group(
            &mut state,
            owner,
            source,
            car_particle(&reference.0, system, prototype, source),
            &lighting,
            &mut commands,
            &mut materials,
        );
    }
    let storms = &reference.0.weather.particles;
    for row in groups.iter().filter(|r| r.kind == ParticleGroupRow::STORM) {
        let owner = Owner::Storm(StormId(row.a));
        let system = row.source as usize;
        if !state
            .groups
            .iter()
            .any(|g| g.active && g.owner == owner && g.source == system)
        {
            add_group(
                &mut state,
                owner,
                system,
                &storms[system],
                &lighting,
                &mut commands,
                &mut materials,
            );
        }
    }
    // A removed plane's retained slots serve the next plane of the same prefab.
    for row in groups.iter().filter(|r| r.kind == ParticleGroupRow::SKY) {
        let prefab = row.source as usize;
        let Some(p) = &reference.0.sky.prefabs[prefab].contrail else {
            continue;
        };
        let owner = Owner::Sky(row.a);
        if state.groups.iter().any(|g| g.active && g.owner == owner) {
            continue;
        }
        if let Some(g) = state
            .groups
            .iter_mut()
            .find(|g| !g.active && g.source == prefab && matches!(g.owner, Owner::Sky(_)))
        {
            g.owner = owner;
            g.active = true;
            continue;
        }
        add_group(
            &mut state,
            owner,
            prefab,
            p,
            &lighting,
            &mut commands,
            &mut materials,
        );
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();
    let cycle = &sim.frame.lighting.cycle;
    let fog = linear(cycle.fog_color);
    let state = &mut *state;
    for g in &mut state.groups {
        let p = match g.owner {
            Owner::Spray(tool) => &reference.0.conditions.sprays[tool].tool.particles[g.source],
            Owner::Brush(tool) => &reference.0.conditions.brushes[tool].tool.particles[g.source],
            Owner::Wash(wash) => &reference.0.conditions.washes[wash].particles[g.source],
            Owner::SprayBottle(0) => &reference.0.small_items.spray_bottle.spray,
            Owner::SprayBottle(_) => &reference.0.small_items.spray_bottle.splash,
            Owner::Car {
                system, prototype, ..
            } => car_particle(&reference.0, system, prototype, g.source),
            Owner::Storm(_) => &storms[g.source],
            Owner::Melee => &reference.0.melee.particles[g.source],
            Owner::Tracer if g.source == 0 => &reference.0.guns.tracer.start,
            Owner::Tracer => &reference.0.guns.tracer.end,
            Owner::Explosion => &reference.0.explosions.types[g.source].particle,
            Owner::Food => &reference.0.survival.food_particles[g.source],
            Owner::Sky(_) => reference.0.sky.prefabs[g.source]
                .contrail
                .as_ref()
                .expect("sky contrail group"),
            Owner::Urine | Owner::Outlet(_) => &references[g.source],
            Owner::Ember(_) => ember.expect("ember group has its record"),
            Owner::LootTool { tool, .. } => loot_tool_particle(&reference.0, tool, g.source),
            Owner::Exhale => &reference.0.small_items.exhale.particle,
            Owner::HeatHaze => &reference.0.small_items.heat_haze,
            Owner::Cobweb(effect) => &reference.0.small_items.wash_effects[effect].particle,
        };
        let (fluid, particles) = sim
            .particle_group(key(g.owner, g.source))
            .map_or((false, &[][..]), |(row, rows)| (row.fluid != 0, rows));
        let count = if !g.active || !p.flags["gameObject.active"] || !p.flags["renderer.enabled"] {
            0
        } else {
            particles.len()
        };
        g.order.clear();
        g.order.extend(0..count);
        let position = |i: usize| Vec3::from_array(particles[i].position);
        if p.scalars["renderer.m_SortMode"] == 1. {
            g.order.sort_unstable_by(|a, b| {
                position(*b)
                    .distance_squared(eye)
                    .total_cmp(&position(*a).distance_squared(eye))
            });
        }
        for batch in 0..g.count {
            let u = &mut state.uniforms[g.start + batch];
            u.fog = if g.black_fog {
                Vec4::new(0., 0., 0., cycle.fog_density)
            } else {
                Vec4::new(fog[0], fog[1], fog[2], cycle.fog_density)
            };
            let n = count.saturating_sub(batch * BATCH).min(BATCH);
            u.settings.w = n as f32;
            if let Ok((mut v, mut transform)) = visibility.get_mut(g.entities[batch]) {
                if n > 0 {
                    *transform =
                        GlobalTransform::from_translation(position(g.order[batch * BATCH + n / 2]));
                }
                *v = if n == 0 {
                    Visibility::Hidden
                } else {
                    Visibility::Visible
                };
            }
            for j in 0..n {
                let index = g.order[batch * BATCH + j];
                let row = &particles[index];
                let tile = if fluid {
                    Vec2::ZERO
                } else {
                    g.sheet.as_ref().map_or(Vec2::ZERO, |s| {
                        sheet_offset(s.frame_at(row.age, row.lifetime, row.uv_draw), s.tiles)
                            * g.tiling
                    })
                };
                u.positions[j] = position(index).extend(row.size);
                u.colors[j] = Vec4::from_array(linear(row.color));
                u.angles[j] = Vec4::new(row.rotation.cos(), row.rotation.sin(), tile.x, tile.y);
            }
        }
    }
}

/// The frame group key of a renderer group.
fn key(owner: Owner, source: usize) -> ParticleGroupKey {
    let (kind, a, b, c) = match owner {
        Owner::Urine => (ParticleGroupRow::URINE, 0, 0, 0),
        Owner::Brush(tool) => (ParticleGroupRow::BRUSH, tool as u32, 0, 0),
        Owner::Spray(tool) => (ParticleGroupRow::SPRAY, tool as u32, 0, 0),
        Owner::Wash(wash) => (ParticleGroupRow::WASH, wash as u32, 0, 0),
        Owner::Outlet(id) => (ParticleGroupRow::OUTLET, id, 0, 0),
        Owner::SprayBottle(index) => (ParticleGroupRow::SPRAY_BOTTLE, index as u32, 0, 0),
        Owner::Car {
            system,
            prototype,
            car,
        } => (ParticleGroupRow::CAR, system, prototype as u32, car),
        Owner::Storm(id) => (ParticleGroupRow::STORM, id.0, 0, 0),
        Owner::Melee => (ParticleGroupRow::MELEE, 0, 0, 0),
        Owner::Tracer => (ParticleGroupRow::TRACER, 0, 0, 0),
        Owner::Explosion => (ParticleGroupRow::EXPLOSION, 0, 0, 0),
        Owner::Food => (ParticleGroupRow::FOOD, 0, 0, 0),
        Owner::Sky(id) => (ParticleGroupRow::SKY, id, 0, 0),
        Owner::Ember(id) => (ParticleGroupRow::EMBER, id.0, 0, 0),
        Owner::LootTool { item, tool } => (ParticleGroupRow::LOOT_TOOL, item.0, tool as u32, 0),
        Owner::Exhale => (ParticleGroupRow::EXHALE, 0, 0, 0),
        Owner::HeatHaze => (ParticleGroupRow::HEAT_HAZE, 0, 0, 0),
        Owner::Cobweb(effect) => (ParticleGroupRow::COBWEB, effect as u32, 0, 0),
    };
    ParticleGroupKey {
        kind,
        a,
        b,
        c,
        source: source as u32,
    }
}

/// Particle system `source` of loot tool `tool`.
fn loot_tool_particle(
    reference: &crate::reference::ReferenceManifest,
    tool: usize,
    source: usize,
) -> &crate::reference::ParticleReference {
    match &reference.conditions.prototype_tools[tool].kind {
        crate::reference::PrototypeTool::Brush { tool, .. } => &tool.particles[source],
        crate::reference::PrototypeTool::Spray { tool, .. } => &tool.particles[source],
    }
}

/// Tile UV offset. Textures keep Unity row order (v 0 at the bottom); Unity frame 0 is the top-left tile.
fn sheet_offset(frame: u32, tiles: [u32; 2]) -> Vec2 {
    let (column, row) = (frame % tiles[0], frame / tiles[0]);
    Vec2::new(
        column as f32 / tiles[0] as f32,
        1. - (row + 1) as f32 / tiles[1] as f32,
    )
}

fn linear(c: [f32; 4]) -> [f32; 4] {
    [
        crate::lighting::source_linear(c[0]),
        crate::lighting::source_linear(c[1]),
        crate::lighting::source_linear(c[2]),
        c[3],
    ]
}
