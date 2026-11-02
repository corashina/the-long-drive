//! Persistent source lighting resources.
use super::sim_frame::SimFrameResource;
use super::{
    materials::{MaterialCache, SourceMaterial, bind_materials},
    reference::ReferenceVisuals,
};
use crate::{
    app::RuntimeReference,
    lighting::source_linear,
    reference::{LightId, LightKind, LightingRecord},
};
use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::ecs as bevy_ecs;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
    render_asset::RenderAssets,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
};
use bevy::shader::Shader;
use simulation::boundary::{PoseKey, PoseRow};
use std::sync::{Arc, Mutex};

pub const MAX_LIGHTS: usize = 64;
#[derive(Clone, Debug, PartialEq)]
pub enum RenderingState {
    Loading,
    Ready,
    Failed(String),
}
#[derive(Clone, Copy, PartialEq, ShaderType, Default)]
pub struct GpuLight {
    pub cookie_from_world: Mat4,
    pub position_range: Vec4,
    pub direction_kind: Vec4,
    pub color: Vec4,
    pub cookie_rect: Vec4,
    /// x: culling mask, y: spot cotangent bits, z: lamp shadow slot + 1 (0: no shadow).
    pub mask: UVec4,
    pub cookie_size: f32,
}
#[derive(Clone, Copy, PartialEq, ShaderType)]
pub struct LightingUniform {
    pub lights: [GpuLight; MAX_LIGHTS],
    pub ambient: Vec4,
    pub fog: Vec4,
    pub counts: UVec4,
}
impl Default for LightingUniform {
    fn default() -> Self {
        Self {
            lights: [GpuLight::default(); MAX_LIGHTS],
            ambient: Vec4::ZERO,
            fog: Vec4::ZERO,
            counts: UVec4::ZERO,
        }
    }
}
/// Pose holder for one source light. Shadowed lamps get a slot in `lamp_shadows`.
#[derive(Component, Clone, Copy)]
pub struct SourceLight;
#[derive(Clone)]
pub struct WorldBoundLight {
    pub record: crate::reference::LightReference,
    pub output: crate::lighting::LightOutput,
}
#[derive(Clone)]
pub struct BoundLight {
    pub world: Option<WorldBoundLight>,
    pub index: usize,
    pub road: bool,
    pub id: LightId,
    pub source_index: usize,
    pub entity: Entity,
    pub node: Option<Entity>,
    pub item: Option<crate::reference::ItemId>,
    pub body: Option<crate::reference::BodyId>,
    pub pose: Transform,
}
#[derive(Clone)]
pub struct BoundLampMaterial {
    pub output_index: usize,
    pub receivers: Vec<Entity>,
    pub current: Option<(crate::reference::AssetId, u32)>,
    pub choices: Vec<super::materials::LampMaterialChoice>,
}
#[derive(Clone)]
pub struct AuthoredMaterial {
    pub asset: crate::reference::AssetId,
    pub index: u32,
    pub handle: Handle<StandardMaterial>,
}
#[derive(Resource, Clone)]
pub struct SourceLighting {
    pub sky: super::sky::SkyState,
    pub(crate) road_lamps: super::world::RoadLamps,
    pub materials: Vec<AuthoredMaterial>,
    material_loads: Option<SourceMaterialCapture>,
    pub materials_ready: bool,
    pub materials_prepared: bool,
    pub warming: bool,
    pub warmups: Vec<Entity>,
    pub lamp_materials: Vec<BoundLampMaterial>,
    control_poses: Vec<(usize, Entity)>,
    visibility: Vec<(usize, Entity)>,
    pub state: RenderingState,
    pub bound: bool,
    pub lights: Vec<BoundLight>,
    pub uniform: LightingUniform,
    pub textures: Vec<(u32, String, Handle<Image>)>,
    pub shader: Handle<Shader>,
    pub cookie_handles: Option<[Handle<Image>; 3]>,
    pub status: Arc<Mutex<RenderingState>>,
}
impl SourceLighting {
    pub(crate) fn clone_bytes(&self) -> usize {
        use std::mem::size_of;
        self.sky.materials.capacity() * size_of::<Handle<super::sky::SkyMaterial>>()
            + self.materials.capacity() * size_of::<AuthoredMaterial>()
            + self.warmups.capacity() * size_of::<Entity>()
            + self.lamp_materials.capacity() * size_of::<BoundLampMaterial>()
            + self
                .lamp_materials
                .iter()
                .map(|m| {
                    m.receivers.capacity() * size_of::<Entity>()
                        + m.choices.capacity() * size_of::<super::materials::LampMaterialChoice>()
                })
                .sum::<usize>()
            + self.control_poses.capacity() * size_of::<(usize, Entity)>()
            + self.visibility.capacity() * size_of::<(usize, Entity)>()
            + self.lights.capacity() * size_of::<BoundLight>()
            + self
                .lights
                .iter()
                .filter_map(|l| l.world.as_ref())
                .map(|l| l.record.source.file.capacity())
                .sum::<usize>()
            + self.textures.capacity() * size_of::<(u32, String, Handle<Image>)>()
            + self
                .textures
                .iter()
                .map(|(_, path, _)| path.capacity())
                .sum::<usize>()
            + self.road_lamps.clone_bytes()
    }

    /// Replace owned light instances. Retain loaded scenes and prepared assets.
    pub fn reset_instances(&mut self, commands: &mut Commands) {
        self.road_lamps.reset_instances(commands);
        for light in self.lights.drain(..) {
            commands.entity(light.entity).despawn();
        }
        self.uniform.lights.fill(GpuLight::default());
        self.uniform.counts.x = 0;
        for material in &mut self.lamp_materials {
            material.current = None;
        }
        self.bound = false;
    }
}
#[derive(Resource)]
pub struct LightingGpu {
    pub uniform: UniformBuffer<LightingUniform>,
    pub cookie_sampler: Sampler,
    pub cookies: Option<[(TextureView, Sampler); 3]>,
    pub attenuation_view: TextureView,
    pub source_shader: Option<AssetId<Shader>>,
    pub sky_shader: Option<AssetId<Shader>>,
    pub status: Arc<Mutex<RenderingState>>,
}
#[derive(Resource, Default)]
pub(crate) struct ExtractedLighting {
    shader: Option<AssetId<Shader>>,
    sky_shader: Option<AssetId<Shader>>,
    prepared: bool,
    warming: bool,
    warmups: Vec<bevy::render::sync_world::MainEntity>,
    uniform: LightingUniform,
    cookies: Option<[AssetId<Image>; 3]>,
    status: Option<Arc<Mutex<RenderingState>>>,
}

#[derive(Resource, Clone, Copy, Default, serde::Serialize)]
pub(crate) struct LightingPipelineAudit {
    cookies: bool,
    prepared: bool,
    warming: bool,
    source_pipelines: u32,
    sky_pipelines: u32,
    pending_pipelines: u32,
    error_pipelines: u32,
    receiver_warmups: u32,
    missing_receiver_warmups: u32,
    first_missing_entity: Option<u64>,
    predicates_ready: bool,
    required_counts_present: bool,
}

impl ExtractedLighting {
    pub(crate) fn is_warmup(&self, entity: bevy::render::sync_world::MainEntity) -> bool {
        self.warming && self.warmups.contains(&entity)
    }
}

pub struct SourceLightingPlugin;
impl Plugin for SourceLightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<super::primary_image::SourcePrimaryImage>()
            .init_asset_loader::<super::primary_image::SourcePrimaryImageLoader>()
            .init_asset::<super::texture_batches::SourceTextureBatch>()
            .init_asset_loader::<super::texture_batches::SourceTextureBatchLoader>()
            .init_asset_loader::<super::textures::SourceImageLoader>()
            .add_plugins((
                MaterialPlugin::<SourceMaterial>::default(),
                super::sky::SourceSkyPlugin,
            ))
            .init_resource::<MaterialCache>()
            .add_systems(
                Update,
                (
                    bind_materials,
                    super::world::prepare_road_lamp,
                    super::materials::prepare_lamp_materials,
                    super::materials::sync_lamp_materials.after(super::conditions::sync_conditions),
                )
                    .chain(),
            );
        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .init_resource::<ExtractedLighting>()
            .init_resource::<LightingPipelineAudit>()
            .add_systems(RenderStartup, init_gpu)
            .add_systems(ExtractSchedule, extract_lighting)
            .add_systems(
                Render,
                upload_lighting.in_set(RenderSystems::PrepareResources),
            )
            .add_systems(Render, check_pipelines.in_set(RenderSystems::Cleanup));
    }
}
pub fn load_lighting(commands: &mut Commands, _server: &AssetServer, lighting: &LightingRecord) {
    let textures = Vec::with_capacity(lighting.textures.len());
    commands.insert_resource(SourceLighting {
        sky: super::sky::SkyState::new(lighting),
        road_lamps: super::world::RoadLamps::new(),
        materials: Vec::new(),
        material_loads: None,
        materials_ready: false,
        materials_prepared: false,
        warming: true,
        warmups: Vec::new(),
        lamp_materials: Vec::new(),
        control_poses: Vec::new(),
        visibility: Vec::new(),
        state: RenderingState::Loading,
        bound: false,
        lights: Vec::with_capacity(MAX_LIGHTS),
        uniform: LightingUniform::default(),
        textures,
        shader: default(),
        cookie_handles: None,
        status: Arc::new(Mutex::new(RenderingState::Loading)),
    });
}
struct PendingMaterial {
    asset: crate::reference::AssetId,
    index: u32,
    path: String,
    handle: Option<Handle<StandardMaterial>>,
}
#[derive(Clone)]
struct SourceMaterialCapture(Arc<Mutex<Vec<PendingMaterial>>>);
impl bevy::gltf::extensions::GltfExtensionHandler for SourceMaterialCapture {
    fn dyn_clone(&self) -> Box<dyn bevy::gltf::extensions::ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }
    fn on_material(
        &mut self,
        context: &mut bevy::asset::LoadContext<'_>,
        material: &bevy::gltf::gltf::Material,
        _: Handle<bevy::gltf::GltfMaterial>,
        _: &bevy::gltf::GltfMaterial,
        label: &str,
    ) {
        let mut pending = self.0.lock().unwrap();
        for entry in pending.iter_mut() {
            if Some(entry.index as usize) == material.index()
                && context.path().path() == std::path::Path::new(&entry.path)
            {
                // Retain the PBR extension's existing subasset. Do not request another file load.
                entry.handle = Some(context.get_label_handle(format!("{label}/std")));
            }
        }
    }
}
impl SourceLighting {
    pub(crate) fn release_failed_capture(&mut self, path: &str) {
        if let Some(capture) = &self.material_loads {
            for entry in capture
                .0
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|e| e.path == path)
            {
                entry.handle = None;
            }
        }
    }
}
pub fn load_lamp_materials(
    commands: &mut Commands,
    extensions: &bevy::gltf::extensions::GltfExtensionHandlers,
    reference: &crate::reference::ReferenceManifest,
) {
    let capture = SourceMaterialCapture(Arc::new(Mutex::new(
        reference
            .lighting
            .materials
            .iter()
            .map(|material| PendingMaterial {
                asset: material.asset,
                index: material.gltf_material,
                path: reference
                    .assets
                    .iter()
                    .find(|asset| asset.id == material.asset)
                    .unwrap()
                    .path
                    .clone(),
                handle: None,
            })
            .collect(),
    )));
    bevy::tasks::block_on(async {
        extensions.0.write().await.push(Box::new(capture.clone()));
    });
    commands.queue(move |world: &mut World| {
        world.resource_mut::<SourceLighting>().material_loads = Some(capture);
    });
}

/// Keep all custom source cookies in the existing single sampled texture slot.
/// A 256 cookie sits padded in the center of its 512 layer.
pub(crate) fn pack_custom_cookies(
    images: &mut Assets<Image>,
    handles: &[&Handle<Image>],
) -> Result<Handle<Image>, String> {
    let originals = handles
        .iter()
        .map(|handle| images.get(*handle).expect("loaded source cookie"))
        .collect::<Vec<_>>();
    let layers = originals.len();
    for (index, image) in originals.iter().enumerate() {
        if !matches!(
            (image.width(), image.texture_descriptor.mip_level_count),
            (512, 10) | (256, 9)
        ) || image.height() != image.width()
            || !matches!(
                image.texture_descriptor.format,
                TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
            )
            || image.data.is_none()
        {
            return Err(format!(
                "Source custom cookie {index} has an unsupported image layout"
            ));
        }
    }
    let mut array = Image::new_uninit(
        Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: layers as u32,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    array.texture_descriptor.mip_level_count = 10;
    array.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    array.data_order = TextureDataOrder::LayerMajor;
    array.sampler = match &originals[0].sampler {
        ImageSampler::Descriptor(descriptor) => ImageSampler::Descriptor(descriptor.clone()),
        ImageSampler::Default => return Err("Source custom cookie has no sampler".into()),
    };
    let layer_bytes = (0..10)
        .map(|level| {
            let size = 512usize >> level;
            size * size * 4
        })
        .sum::<usize>();
    let mut data = vec![0; layer_bytes * layers];
    for (layer, image) in originals.iter().enumerate() {
        let source = image.data.as_ref().unwrap();
        let mips = image.texture_descriptor.mip_level_count;
        let mut source_offset = 0;
        let mut target_offset = layer * layer_bytes;
        for level in 0..10 {
            let target_size = 512usize >> level;
            let source_size = (image.width() as usize >> level).max(1);
            let pad = (target_size - source_size) / 2;
            for y in 0..target_size {
                let source_y = y.saturating_sub(pad).min(source_size - 1);
                for x in 0..target_size {
                    let source_x = x.saturating_sub(pad).min(source_size - 1);
                    let from = source_offset + (source_y * source_size + source_x) * 4;
                    let to = target_offset + (y * target_size + x) * 4;
                    data[to..to + 4].copy_from_slice(&source[from..from + 4]);
                }
            }
            target_offset += target_size * target_size * 4;
            // The last source mip also fills the smaller target mips.
            if level + 1 < mips {
                source_offset += source_size * source_size * 4;
            }
        }
    }
    array.data = Some(data);
    Ok(images.add(array))
}

/// Cookie texture ids: the cloud shadow cookie, the default spot cookie, then the custom cookie
/// array layers: the first lighting spot cookie, both flashlight cookies and the world Light cookies.
pub(crate) fn cookie_ids(reference: &crate::reference::ReferenceManifest) -> Vec<u32> {
    let r = &reference.lighting;
    let portable = [
        &reference.flashlights.instances[0],
        &reference.flashlights.instances[1],
    ]
    .map(|instance| {
        let c = r
            .controllers
            .iter()
            .find(|c| c.id == instance.controller)
            .unwrap();
        r.lights
            .iter()
            .find(|l| l.id == c.states[0].lights[0])
            .unwrap()
            .cookie
            .unwrap()
    });
    let mut ids = vec![
        r.cycle.cloud_shadow_cookie,
        r.default_spot_cookie,
        r.lights
            .iter()
            .find(|l| l.kind == LightKind::Spot && l.cookie.is_some())
            .unwrap()
            .cookie
            .unwrap(),
        portable[0],
        portable[1],
    ];
    for cookie in &reference.world.light_cookies {
        if !ids.contains(&cookie.texture) {
            ids.push(cookie.texture);
        }
    }
    ids
}
/// Shader cookie rect of a light. x: 0 none, then 1 + its index in `ids` (`cookie_ids`).
/// w: 1 for a 256 cookie array layer. A spot light without a cookie uses the default spot cookie.
/// None: the cookie is not bound.
pub(crate) fn cookie_rect(
    reference: &crate::reference::ReferenceManifest,
    ids: &[u32],
    light: &crate::reference::LightReference,
) -> Option<Vec4> {
    let Some(id) = light
        .cookie
        .or((light.kind == LightKind::Spot).then_some(ids[1]))
    else {
        return Some(Vec4::ZERO);
    };
    let index = ids.iter().position(|c| *c == id)?;
    let half = index > 1
        && reference
            .lighting
            .textures
            .iter()
            .any(|t| t.id == id && t.width == 256);
    Some(Vec4::new(
        (index + 1) as f32,
        0.,
        0.,
        if half { 1. } else { 0. },
    ))
}
pub(crate) fn bind_lighting(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    visuals: Res<ReferenceVisuals>,
    mut source: ResMut<SourceLighting>,
    images: Res<Assets<Image>>,
    sim: Res<SimFrameResource>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
) {
    if source.bound
        || !visuals.bound
        || source.textures.len() != reference.0.lighting.textures.len()
        || source.cookie_handles.is_none()
        || matches!(source.state, RenderingState::Failed(_))
    {
        return;
    }
    if source
        .textures
        .iter()
        .any(|(_, _, h)| !images.contains(h.id()))
    {
        return;
    }
    let r = &reference.0.lighting;
    let cookie_ids = cookie_ids(&reference.0);
    let directional_cookie = r
        .textures
        .iter()
        .find(|texture| texture.id == r.cycle.cloud_shadow_cookie)
        .expect("validated directional cookie");
    if directional_cookie.wrap_u != 0 || directional_cookie.wrap_v != 0 {
        source.state = RenderingState::Failed(
            "Source directional cookie requires repeat wrapping on both axes".into(),
        );
        return;
    }
    if !source.materials_prepared {
        for (output_index, output) in sim.frame.lamp_materials.iter().enumerate() {
            let Some(node) = visuals.nodes.get(&output.renderer) else {
                // Road prototype outputs bind when a streamed instance registers.
                continue;
            };
            let receivers = primitives
                .iter()
                .filter(|(_, parent)| parent.parent() == *node)
                .map(|(e, _)| e)
                .collect();
            source.lamp_materials.push(BoundLampMaterial {
                output_index,
                receivers,
                current: None,
                choices: Vec::new(),
            });
        }
        for (index, pose) in sim.frame.control_poses.iter().enumerate() {
            source.control_poses.push((
                index,
                *visuals
                    .nodes
                    .get(&pose.node)
                    .expect("source control pose node"),
            ));
        }
        for (index, output) in sim.frame.node_visibility.iter().enumerate() {
            source.visibility.push((
                index,
                *visuals
                    .nodes
                    .get(&output.node)
                    .expect("source lamp visibility node"),
            ));
        }
    }
    for (source_index, light) in r.lights.iter().enumerate() {
        if light.node.is_some_and(|id| {
            reference
                .0
                .nodes
                .iter()
                .any(|n| n.id == id && n.asset == r.road_lamp.asset)
        }) {
            continue;
        }
        let index = source.lights.len();
        if index == MAX_LIGHTS {
            source.state =
                RenderingState::Failed(format!("Light {} exceeds light capacity", light.id.0));
            return;
        }
        if light.kind == LightKind::Area {
            source.state =
                RenderingState::Failed(format!("Light {} has unsupported area type", light.id.0));
            return;
        }
        let node = light.node.and_then(|id| visuals.nodes.get(&id).copied());
        if light.node.is_some() && node.is_none() {
            source.state =
                RenderingState::Failed(format!("Light {} has no imported node", light.id.0));
            return;
        }
        let pose = Transform::from_translation(Vec3::from_array(light.pose.translation))
            .with_rotation(Quat::from_array(light.pose.rotation));
        let entity = commands.spawn((SourceLight, pose)).id();
        let mut ancestor = light.node;
        let mut item = None;
        let mut body = None;
        while let Some(id) = ancestor {
            if item.is_none() {
                item = reference
                    .0
                    .items
                    .iter()
                    .find(|item| item.node == id)
                    .map(|item| item.id);
            }
            if body.is_none() {
                body = reference
                    .0
                    .bodies
                    .iter()
                    .find(|body| body.node == id)
                    .map(|body| body.id);
            }
            ancestor = reference
                .0
                .nodes
                .iter()
                .find(|node| node.id == id)
                .and_then(|node| node.parent);
        }
        source.lights.push(BoundLight {
            world: None,
            index,
            road: false,
            item,
            body,
            id: light.id,
            source_index,
            entity,
            node,
            pose,
        });
        source.uniform.lights[index].cookie_rect =
            cookie_rect(&reference.0, &cookie_ids, light).unwrap_or(Vec4::ZERO);
        source.uniform.lights[index].cookie_size = light.cookie_size;
        source.uniform.lights[index].mask = UVec4::new(
            light.culling_mask,
            if light.kind == LightKind::Spot {
                let a = light.full_cone_degrees * 0.5 / 360.;
                let half = (a + a) * std::f32::consts::PI;
                (half.cos() / half.sin()).to_bits()
            } else {
                0
            },
            0,
            0,
        );
    }
    // One reserved slot for the lit cigarette ember light (emberScript.feny).
    if let Some(record) = ember_light(&reference.0) {
        let index = source.lights.len();
        if index == MAX_LIGHTS {
            source.state = RenderingState::Failed("Cigarette ember exceeds light capacity".into());
            return;
        }
        let entity = commands.spawn((SourceLight, Transform::default())).id();
        source.lights.push(BoundLight {
            world: Some(WorldBoundLight {
                record: record.clone(),
                output: crate::lighting::LightOutput {
                    id: record.id,
                    enabled: false,
                    intensity: record.startup_intensity,
                },
            }),
            index,
            road: false,
            item: None,
            body: None,
            id: record.id,
            source_index: 0,
            entity,
            node: None,
            pose: Transform::default(),
        });
        source.uniform.lights[index].cookie_rect =
            cookie_rect(&reference.0, &cookie_ids, record).unwrap_or(Vec4::ZERO);
        source.uniform.lights[index].cookie_size = record.cookie_size;
        source.uniform.lights[index].mask = UVec4::new(record.culling_mask, 0, 0, 0);
    }
    // One reserved slot for the pumpkin light (lightdither).
    if let Some(record) = dither_light(&reference.0) {
        let record = &record.light;
        let index = source.lights.len();
        if index == MAX_LIGHTS {
            source.state = RenderingState::Failed("Pumpkin light exceeds light capacity".into());
            return;
        }
        let entity = commands.spawn((SourceLight, Transform::default())).id();
        source.lights.push(BoundLight {
            world: Some(WorldBoundLight {
                record: record.clone(),
                output: crate::lighting::LightOutput {
                    id: record.id,
                    enabled: false,
                    intensity: record.startup_intensity,
                },
            }),
            index,
            road: false,
            item: None,
            body: None,
            id: record.id,
            source_index: 0,
            entity,
            node: None,
            pose: Transform::default(),
        });
        source.uniform.lights[index].cookie_rect = Vec4::ZERO;
        source.uniform.lights[index].cookie_size = record.cookie_size;
        source.uniform.lights[index].mask = UVec4::new(record.culling_mask, 0, 0, 0);
    }
    source.uniform.counts.x = source.lights.len() as u32;
    source.road_lamps.fixed_lights = source.lights.len();
    source.road_lamps.capacity = MAX_LIGHTS - source.lights.len();
    if r.road_lamp.lights.len() != 1 {
        source.state =
            RenderingState::Failed("Source road lamp window exceeds renderer capacity".into());
        return;
    }
    source.bound = true;
}
/// The point light of the cigarette ember record.
pub(crate) fn ember_light(
    reference: &crate::reference::ReferenceManifest,
) -> Option<&crate::reference::LightReference> {
    reference
        .item_prototypes
        .iter()
        .find(|p| p.item_id == Some(reference.acquisition.cigarette_item_id))
        .and_then(|p| p.consumable.as_ref())
        .map(|c| &c.ember.light)
}
/// The `lightdither` record of the pumpkin prototype.
pub(crate) fn dither_light(
    reference: &crate::reference::ReferenceManifest,
) -> Option<&crate::reference::LightDitherReference> {
    reference
        .item_prototypes
        .iter()
        .find_map(|p| p.light_dither.as_ref())
}
/// The particle of the cigarette ember record.
pub(crate) fn ember_particle(
    reference: &crate::reference::ReferenceManifest,
) -> Option<&crate::reference::ParticleReference> {
    reference
        .item_prototypes
        .iter()
        .find(|p| p.item_id == Some(reference.acquisition.cigarette_item_id))
        .and_then(|p| p.consumable.as_ref())
        .map(|c| &c.ember.particle)
}
pub(crate) fn sync_control_visuals(
    sim: Res<SimFrameResource>,
    source: Res<SourceLighting>,
    mut poses: Query<&mut Transform>,
    mut nodes: Query<&mut Visibility>,
) {
    for &(index, entity) in &source.control_poses {
        if let Ok(mut pose) = poses.get_mut(entity) {
            let output = &sim.frame.control_poses[index].local_pose;
            let next = Transform {
                translation: Vec3::from_array(output.translation),
                rotation: Quat::from_array(output.rotation),
                ..*pose
            };
            pose.set_if_neq(next);
        }
    }
    for &(index, entity) in &source.visibility {
        if let Ok(mut visibility) = nodes.get_mut(entity) {
            visibility.set_if_neq(if sim.frame.node_visibility[index].visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}
/// The source record of a bound light.
pub(crate) fn record_of<'a>(
    reference: &'a RuntimeReference,
    bound: &'a BoundLight,
) -> &'a crate::reference::LightReference {
    bound
        .world
        .as_ref()
        .map_or(&reference.0.lighting.lights[bound.source_index], |w| {
            &w.record
        })
}
pub(crate) fn sync_lighting(
    reference: Res<RuntimeReference>,
    sim: Res<SimFrameResource>,
    mut source: ResMut<SourceLighting>,
    globals: Query<&GlobalTransform, Without<SourceLight>>,
    mut lights: Query<(&mut Transform, &mut GlobalTransform), With<SourceLight>>,
) {
    let cycle = &sim.frame.lighting.cycle;
    source.uniform.ambient = Vec4::new(
        source_linear(cycle.ambient),
        source_linear(cycle.ambient),
        source_linear(cycle.ambient),
        cycle.fog_density,
    );
    source.uniform.fog = Vec4::from_array(cycle.fog_color.map(source_linear));
    let road_output = sim.frame.lighting.road_lamp;
    let active = |key| {
        sim.pose(key)
            .is_some_and(|(_, current)| current.flags & PoseRow::ACTIVE != 0)
    };
    let source = &mut *source;
    for bound_index in 0..source.lights.len() {
        let bound = &source.lights[bound_index];
        let index = bound.index;
        let active = bound
            .item
            .map(|id| active(PoseKey::Item(id)))
            .unwrap_or_else(|| bound.body.is_none_or(|id| active(PoseKey::Body(id))));
        let record = record_of(&reference, bound);
        let output = if let Some(world) = &bound.world {
            &world.output
        } else if bound.road {
            &road_output
        } else {
            &sim.frame.lights[bound.source_index]
        };
        let Ok((mut pose, mut global)) = lights.get_mut(bound.entity) else {
            continue;
        };
        let mut next_pose = *pose;
        if record.kind == LightKind::Directional {
            let [x, y, z] = cycle.directional_euler_degrees.map(f32::to_radians);
            next_pose.rotation = Quat::from_euler(EulerRot::YXZ, -y, -x, z);
        } else if let Some(node) = bound.node {
            if let Ok(global) = globals.get(node) {
                next_pose = global.compute_transform();
            }
        }
        pose.set_if_neq(next_pose);
        global.set_if_neq(GlobalTransform::from(next_pose));
        let color = if record.kind == LightKind::Directional {
            cycle.directional_color
        } else {
            record.color
        };
        let intensity = if !active {
            0.
        } else if record.kind == LightKind::Directional {
            cycle.directional_intensity
        } else if output.enabled {
            output.intensity
        } else {
            0.
        };
        let cookie_from_world = if record.kind == LightKind::Directional {
            Mat4::from_quat(pose.rotation).inverse()
        } else if record.kind == LightKind::Spot {
            Mat4::from_cols_array_2d(&crate::lighting::spot_cookie_matrix(
                &crate::reference::Transform {
                    translation: pose.translation.to_array(),
                    rotation: pose.rotation.to_array(),
                    scale: pose.scale.to_array(),
                },
                record.range_meters,
                f32::from_bits(source.uniform.lights[index].mask.y),
            ))
        } else {
            Mat4::from_rotation_translation(pose.rotation, pose.translation).inverse()
        };
        let phase = if record.kind == LightKind::Directional {
            crate::lighting::directional_cookie_phase(
                [
                    cookie_from_world.row(0).truncate().to_array(),
                    cookie_from_world.row(1).truncate().to_array(),
                ],
                sim.frame.state.origin,
                bound.pose.translation.to_array(),
                record.cookie_size,
            )
        } else {
            [0.; 2]
        };
        let gpu = &mut source.uniform.lights[index];
        gpu.cookie_rect.y = phase[0];
        gpu.cookie_rect.z = phase[1];
        gpu.color = Vec4::new(
            source_linear(color[0] * intensity),
            source_linear(color[1] * intensity),
            source_linear(color[2] * intensity),
            1.,
        );
        gpu.position_range = pose.translation.extend(record.range_meters);
        gpu.direction_kind = (pose.rotation * Vec3::NEG_Z).extend(match record.kind {
            LightKind::Directional => 0.,
            LightKind::Point => 1.,
            _ => 2.,
        });
        gpu.cookie_from_world = cookie_from_world;
    }
}
pub(crate) fn lighting_readiness(
    conditions: Res<super::conditions::ConditionVisuals>,
    mut commands: Commands,
    server: Res<AssetServer>,
    mut source: ResMut<SourceLighting>,
    visuals: Res<ReferenceVisuals>,
    images: Res<Assets<Image>>,
    reference: Res<RuntimeReference>,
    fixed: Res<super::fixed_assets::SourceFixedAssets>,
) {
    if matches!(source.state, RenderingState::Failed(_)) {
        return;
    }
    if !fixed.payloads_ready() || source.textures.len() != reference.0.lighting.textures.len() {
        return;
    }
    for (_, name, handle) in &source.textures {
        if let Some(LoadState::Failed(error)) = server.get_load_state(handle.id()) {
            source.state = RenderingState::Failed(format!("Cannot load {name}: {error}"));
            return;
        }
        if !images.contains(handle.id()) {
            return;
        }
    }
    if let Some(LoadState::Failed(error)) = server.get_load_state(source.shader.id()) {
        source.state =
            RenderingState::Failed(format!("Cannot load shaders/source_surface.wgsl: {error}"));
        return;
    }
    if !source.materials_ready {
        let Some(capture) = &source.material_loads else {
            return;
        };
        let pending = capture.0.lock().unwrap();
        if let Some(missing) = pending.iter().find(|material| material.handle.is_none()) {
            if visuals
                .handles
                .iter()
                .all(|(_, handle)| server.is_loaded_with_dependencies(handle.id()))
            {
                let error = format!(
                    "Source lamp material {}:{} has no PBR subasset",
                    missing.asset.0, missing.index
                );
                drop(pending);
                source.state = RenderingState::Failed(error);
            }
            return;
        }
        for material in pending.iter() {
            let handle = material.handle.as_ref().unwrap();
            if let Some(LoadState::Failed(error)) = server.get_load_state(handle.id()) {
                drop(pending);
                source.state = RenderingState::Failed(format!("Source lamp material: {error}"));
                return;
            }
            if !server.is_loaded_with_dependencies(handle.id()) {
                return;
            }
        }
        let materials = pending
            .iter()
            .map(|material| AuthoredMaterial {
                asset: material.asset,
                index: material.index,
                handle: material.handle.as_ref().unwrap().clone(),
            })
            .collect();
        drop(pending);
        source.materials = materials;
        source.materials_ready = true;
    }
    if source
        .lamp_materials
        .iter()
        .any(|target| target.current.is_none())
    {
        return;
    }
    if !source.sky.bound() {
        return;
    }
    if let Some(LoadState::Failed(error)) = server.get_load_state(source.sky.shader.id()) {
        source.state =
            RenderingState::Failed(format!("Cannot load shaders/source_sky.wgsl: {error}"));
        return;
    }
    if source.bound && source.materials_prepared && conditions.prepared {
        let state = source.status.lock().unwrap().clone();
        if source.warming && state == RenderingState::Ready {
            for entity in source.warmups.drain(..) {
                commands.entity(entity).despawn();
            }
            super::world::finish_road_lamp_warmup(&mut commands, &mut source);
            source.warming = false;
            return;
        }
        source.state = state;
    }
}
fn init_gpu(mut commands: Commands, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let limits = device.limits();
    let bytes = LightingUniform::min_size().get();
    let supported = u64::from(limits.max_uniform_buffer_binding_size) >= bytes;
    let state = if supported {
        RenderingState::Loading
    } else {
        RenderingState::Failed(format!(
            "Source lighting GPU limits: needs {bytes}-byte uniform; device has {} bytes",
            limits.max_uniform_buffer_binding_size
        ))
    };
    let status = Arc::new(Mutex::new(state));
    capture_gpu_errors(&device, status.clone());
    info!(
        "Source GPU: {}-byte uniform limit; uniform {bytes} bytes",
        limits.max_uniform_buffer_binding_size
    );
    let attenuation = device.create_texture_with_data(
        &queue,
        &TextureDescriptor {
            label: Some("source attenuation"),
            size: Extent3d {
                width: 1024,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &crate::lighting::attenuation_table(),
    );
    let mut uniform = UniformBuffer::default();
    if supported {
        uniform.write_buffer(&device, &queue);
    }
    commands.insert_resource(LightingGpu {
        uniform,
        cookie_sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        cookies: None,
        attenuation_view: attenuation.create_view(&default()),
        source_shader: None,
        sky_shader: None,
        status,
    });
}
fn capture_gpu_errors(device: &RenderDevice, status: Arc<Mutex<RenderingState>>) {
    device
        .wgpu_device()
        .on_uncaptured_error(Arc::new(move |error| {
            let mut state = status.lock().unwrap();
            if !matches!(*state, RenderingState::Failed(_)) {
                *state = RenderingState::Failed(format!("Source GPU validation: {error}"));
            }
        }));
}
fn extract_lighting(
    source: Extract<Option<Res<SourceLighting>>>,
    conditions: Extract<Res<super::conditions::ConditionVisuals>>,
    food: Extract<Res<super::consumables::ConsumableVisuals>>,
    mut output: ResMut<ExtractedLighting>,
) {
    if let Some(source) = source.as_ref() {
        output.uniform = source.uniform;
        output.prepared = source.materials_prepared && conditions.prepared && food.warmed;
        output.warming = source.warming;
        if source.warming && output.prepared && output.warmups.is_empty() {
            output.warmups.extend(
                source
                    .warmups
                    .iter()
                    .copied()
                    .map(bevy::render::sync_world::MainEntity::from),
            );
        } else if !source.warming {
            output.warmups.clear();
        }
        output.shader = Some(source.shader.id());
        output.sky_shader = Some(source.sky.shader.id());
        output.cookies = source
            .cookie_handles
            .as_ref()
            .map(|handles| std::array::from_fn(|i| handles[i].id()));
        if output
            .status
            .as_ref()
            .is_none_or(|status| !Arc::ptr_eq(status, &source.status))
        {
            output.status = Some(source.status.clone());
        }
    }
}
fn upload_lighting(
    extracted: Res<ExtractedLighting>,
    mut gpu: ResMut<LightingGpu>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if let Some(status) = &extracted.status {
        if !Arc::ptr_eq(status, &gpu.status) {
            if let RenderingState::Failed(error) = &*gpu.status.lock().unwrap() {
                *status.lock().unwrap() = RenderingState::Failed(error.clone());
            }
            gpu.status = status.clone();
            capture_gpu_errors(&device, status.clone());
        }
    }
    if matches!(*gpu.status.lock().unwrap(), RenderingState::Failed(_)) {
        return;
    }
    gpu.source_shader = extracted.shader;
    gpu.sky_shader = extracted.sky_shader;
    let unchanged = gpu.uniform.get() == &extracted.uniform;
    if !unchanged {
        gpu.uniform.set(extracted.uniform);
        gpu.uniform.write_buffer(&device, &queue);
    }
    if gpu.cookies.is_none() {
        if let Some(ids) = extracted.cookies {
            if ids.iter().all(|id| images.get(*id).is_some()) {
                gpu.cookies = Some(ids.map(|id| {
                    let image = images.get(id).unwrap();
                    (image.texture_view.clone(), image.sampler.clone())
                }));
            }
        }
    }
}
fn check_pipelines(
    cache: Res<PipelineCache>,
    isolation: Res<super::pipeline_ownership::PipelineOwners>,
    gpu: Res<LightingGpu>,
    extracted: Res<ExtractedLighting>,
    material_pipelines: Res<bevy::pbr::SpecializedMaterialPipelineCache>,
    mut last: Local<usize>,
    mut audit: ResMut<LightingPipelineAudit>,
) {
    *audit = LightingPipelineAudit {
        cookies: gpu.cookies.is_some(),
        prepared: extracted.prepared,
        warming: extracted.warming,
        ..default()
    };
    let mut pending_pipelines = 0;
    let mut error_pipelines = 0;
    let mut ready = gpu.cookies.is_some() && extracted.prepared;
    let mut count = 0;
    let mut sky_count = 0;
    let mut inspect = |state: &CachedPipelineState| match state {
        CachedPipelineState::Ok(_) => {}
        CachedPipelineState::Err(
            bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
            | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
        ) => {
            ready = false;
            pending_pipelines += 1;
        }
        CachedPipelineState::Err(error) => {
            ready = false;
            error_pipelines += 1;
            *gpu.status.lock().unwrap() =
                RenderingState::Failed(format!("Source shader pipeline: {error}"));
        }
        _ => {
            ready = false;
            pending_pipelines += 1;
        }
    };
    for (index, pipeline) in cache.pipelines().enumerate() {
        if isolation.isolated(CachedRenderPipelineId::new(index)) {
            continue;
        }
        if let PipelineDescriptor::RenderPipelineDescriptor(descriptor) = &pipeline.descriptor {
            if descriptor
                .fragment
                .as_ref()
                .is_some_and(|f| Some(f.shader.id()) == gpu.source_shader)
            {
                count += 1;
                inspect(&pipeline.state);
            }
            if descriptor
                .fragment
                .as_ref()
                .is_some_and(|f| Some(f.shader.id()) == gpu.sky_shader)
            {
                sky_count += 1;
                inspect(&pipeline.state);
            }
        }
    }
    if extracted.warming {
        for entity in &extracted.warmups {
            audit.receiver_warmups += 1;
            if !material_pipelines
                .values()
                .any(|view| view.contains_key(entity))
            {
                ready = false;
                audit.missing_receiver_warmups += 1;
                audit
                    .first_missing_entity
                    .get_or_insert(entity.id().to_bits());
            }
        }
    }
    audit.source_pipelines = count as u32;
    audit.sky_pipelines = sky_count as u32;
    audit.pending_pipelines = pending_pipelines;
    audit.error_pipelines = error_pipelines;
    audit.predicates_ready = ready;
    audit.required_counts_present = count > 0 && sky_count > 0;
    if *last != count {
        info!("Source pipelines: {count} receiver");
        *last = count;
    }
    if ready
        && count > 0
        && sky_count > 0
        && !matches!(*gpu.status.lock().unwrap(), RenderingState::Failed(_))
    {
        *gpu.status.lock().unwrap() = RenderingState::Ready;
    }
}

/// Register one source spot in a reserved slot. Shader allocations remain fixed.
pub(crate) fn register_road_light(
    source: &mut SourceLighting,
    commands: &mut Commands,
    reference: &LightingRecord,
    slot: usize,
    node: Entity,
) -> Entity {
    let source_index = reference
        .lights
        .iter()
        .position(|l| l.id == reference.road_lamp.lights[0])
        .unwrap();
    let light = &reference.lights[source_index];
    let index = source.road_lamps.fixed_lights + slot;
    let entity = commands.spawn((SourceLight, Transform::default())).id();
    source.lights.push(BoundLight {
        world: None,
        index,
        road: true,
        item: None,
        body: None,
        id: light.id,
        source_index,
        entity,
        node: Some(node),
        pose: Transform::default(),
    });
    let a = light.full_cone_degrees * 0.5 / 360.;
    let half = (a + a) * std::f32::consts::PI;
    source.uniform.lights[index] = GpuLight {
        cookie_rect: Vec4::new(2., 0., 0., 0.),
        mask: UVec4::new(
            light.culling_mask,
            (half.cos() / half.sin()).to_bits(),
            0,
            0,
        ),
        cookie_size: light.cookie_size,
        ..default()
    };
    source.uniform.counts.x = source.uniform.counts.x.max(index as u32 + 1);
    entity
}

pub(crate) fn register_world_light(
    source: &mut SourceLighting,
    commands: &mut Commands,
    light: &crate::world::WorldLight,
    slot: usize,
    node: Entity,
    enabled: bool,
    cookie: Vec4,
) -> Entity {
    let record = &light.record;
    let index = source.road_lamps.fixed_lights + slot;
    let entity = commands.spawn((SourceLight, Transform::default())).id();
    source.lights.push(BoundLight {
        body: None,
        item: None,
        world: Some(WorldBoundLight {
            record: record.clone(),
            output: crate::lighting::LightOutput {
                id: record.id,
                enabled,
                intensity: light.intensity,
            },
        }),
        index,
        road: light.biome.is_some(),
        id: record.id,
        source_index: 0,
        entity,
        node: Some(node),
        pose: Transform::default(),
    });
    let half = record.full_cone_degrees.to_radians() * 0.5;
    source.uniform.lights[index] = GpuLight {
        cookie_rect: cookie,
        mask: UVec4::new(
            record.culling_mask,
            (half.cos() / half.sin()).to_bits(),
            0,
            0,
        ),
        cookie_size: record.cookie_size,
        ..default()
    };
    source.uniform.counts.x = source.uniform.counts.x.max(index as u32 + 1);
    entity
}

pub(crate) fn remove_road_light(
    source: &mut SourceLighting,
    commands: &mut Commands,
    entity: Entity,
) {
    if let Some(bound) = source.lights.iter().find(|light| light.entity == entity) {
        source.uniform.lights[bound.index] = GpuLight::default();
    }
    source.lights.retain(|light| light.entity != entity);
    commands.entity(entity).despawn();
    source.uniform.counts.x = source
        .lights
        .iter()
        .map(|light| light.index + 1)
        .max()
        .unwrap_or(0) as u32;
}
