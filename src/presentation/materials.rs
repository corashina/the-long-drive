mod generated;

pub mod grass;
pub mod impostor;
pub mod primary;
pub mod specular;
pub mod terrain;
use crate::ownership::upload_receipt;
mod world_texture_upload;
pub mod world_textures;
use super::primary_image::{SourcePrimaryImage, SourcePrimaryImageSettings};
use super::renderer_visibility::SourceMaterialWarmup;
use crate::reference::{
    self as source, AssetUsage, MaterialRecord, ReferenceManifest, ShaderFamily, TextureRecord,
};
use bevy::asset::{AssetId, LoadState, UntypedAssetId};
use bevy::gltf::{GltfExtras, GltfMaterialExtras};
use bevy::material::key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline, MaterialPipeline,
    MaterialPipelineKey, MeshPipeline, MeshPipelineKey, PreparedMaterial, ViewKeyCache,
    alpha_mode_pipeline_key, base_specialize,
};
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    erased_render_asset::ErasedRenderAssets,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    mesh::RenderMesh,
    render_asset::RenderAssets,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
};
use bevy::shader::{ShaderCacheError, ShaderDefVal, ShaderRef};
use bevy::{
    asset as bevy_asset, ecs as bevy_ecs, image as bevy_image, reflect as bevy_reflect,
    render as bevy_render,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

const SLOTS: [&str; 9] = source::MATERIAL_TEXTURE_SLOTS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderingState {
    Loading,
    Ready,
    Failed(String),
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct SourceUniform {
    color: Vec4,
    emission: Vec4,
    specular: Vec4,
    factors: Vec4,
    scales: Vec4,
    flags: UVec4,
    banks: [UVec4; 3],
    layers: [UVec4; 3],
    uv: [Vec4; 9],
    sampling: [Vec4; 9],
    defaults: [Vec4; 9],
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SurfaceKey {
    cull: u8,
    z_write: bool,
    z_test: u8,
    src: u8,
    dst: u8,
    reflected: bool,
    standard: bool,
    unlit: bool,
    transparent: bool,
    glass: bool,
    glass_shader: Option<Handle<Shader>>,
    /// Static rows draw this material from the GPU prop buffers.
    gpu_props: bool,
}
impl SurfaceKey {
    /// ForwardAdd uses back-face culling, LEqual depth, and One One blending.
    /// With the same base state and replace blending, one draw adds both results.
    pub fn single_pass(&self) -> bool {
        self.cull >= 2
            && self.standard
            && !self.unlit
            && !self.transparent
            && !self.glass
            && self.src == 1
            && self.dst == 0
            && self.z_write
            && self.z_test == 4
    }
}
impl From<&SourceSurface> for SurfaceKey {
    fn from(value: &SourceSurface) -> Self {
        value.state.clone()
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(SurfaceKey)]
pub struct SourceSurface {
    #[uniform(120)]
    data: SourceUniform,
    #[texture(121)]
    #[sampler(122)]
    bank6: Option<Handle<Image>>,
    state: SurfaceKey,
}
impl SourceSurface {
    /// Linear color.
    pub fn color(&self) -> Vec4 {
        self.data.color
    }
    pub fn set_color(&mut self, linear: Vec4) {
        self.data.color = linear;
    }
}
fn blend_factor(value: u8) -> BlendFactor {
    match value {
        0 => BlendFactor::Zero,
        1 => BlendFactor::One,
        2 => BlendFactor::Dst,
        3 => BlendFactor::Src,
        4 => BlendFactor::OneMinusDst,
        5 => BlendFactor::SrcAlpha,
        6 => BlendFactor::OneMinusSrc,
        7 => BlendFactor::DstAlpha,
        8 => BlendFactor::OneMinusDstAlpha,
        9 => BlendFactor::SrcAlphaSaturated,
        _ => BlendFactor::OneMinusSrcAlpha,
    }
}

#[derive(Clone)]
struct SourceTexture {
    owner: Arc<()>,
    upload: upload_receipt::UploadReceipt,
    failed: bool,
    persistent: bool,
    users: usize,
    record: Arc<TextureRecord>,
    chain: Option<Handle<SourcePrimaryImage>>,
    input_staging: usize,
    payload_bytes: usize,
    cpu_reservation: usize,
    image: Option<Handle<Image>>,
    label: String,
}
#[derive(Resource, Clone)]
pub struct SourceMaterials {
    pub preparation: super::gpu_prepare::GpuPreparation,
    pub(crate) generated: super::world::shared_materials::SharedMaterials<
        super::world::instance_materials::SharedBinding,
    >,
    pub world_textures: world_textures::WorldTextures,
    pub world_primary: primary::PrimaryOwnership,
    pub world_deferred: primary::DeferredAllocation,
    pub standard_records: HashMap<AssetId<StandardMaterial>, (source::AssetId, u32)>,
    records: Vec<Arc<MaterialRecord>>,
    sort_data: HashMap<(source::AssetId, u32), super::renderer_visibility::SourceSortData>,
    textures: Vec<SourceTexture>,
    texture_indices: HashMap<String, usize>,
    pub handles: HashMap<(source::AssetId, u32, bool, u32), Handle<SourceMaterial>>,
    pub(crate) shaders: [Handle<Shader>; 3],
    pub(crate) fixed_admitted: bool,
    /// Ragdoll asset. Its `player_model` materials have no camera distance clip.
    ragdoll: source::AssetId,
    pub state: RenderingState,
    pub bound: bool,
    pub texture_bytes: usize,
}
#[derive(Clone)]
struct PipelineRequest {
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    label: String,
}
#[derive(Resource, Clone, ExtractResource)]
pub struct MaterialLoading(Arc<Mutex<LoadingData>>);
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(crate) struct MaterialPrewarmAudit {
    pub(crate) state: u8,
    pub(crate) sealed: bool,
    pub(crate) request_count: u32,
    pub(crate) missing_mesh_count: u32,
    pub(crate) missing_material_count: u32,
    pub(crate) first_missing_mesh_request: Option<u32>,
    pub(crate) first_missing_material_request: Option<u32>,
    pub(crate) warmed_queued: bool,
    pub(crate) warmed_id_count: u32,
    pub(crate) ready_pipeline_count: u32,
    pub(crate) view_key_count: u32,
    pub(crate) cookies_present: bool,
}
impl MaterialLoading {
    /// Compile the pipelines of a retained mesh and material before loading completes.
    pub(crate) fn warm(&self, mesh: AssetId<Mesh>, material: UntypedAssetId, label: String) {
        self.0.lock().unwrap().requests.push(PipelineRequest {
            mesh,
            material,
            label,
        });
    }
    pub fn is_ready(&self) -> bool {
        self.0.lock().unwrap().state == RenderingState::Ready
    }
    pub fn source_shadow_capacity(&self) -> Option<usize> {
        let data = self.0.lock().unwrap();
        data.sealed.then_some(data.shadow_capacity)
    }
    pub fn transparent_pass_capacity(&self) -> Option<usize> {
        let data = self.0.lock().unwrap();
        data.sealed.then_some(data.transparent_pass_capacity)
    }

    /// State: 0=Loading, 1=Ready, 2=Failed. Request indices start at zero.
    pub(crate) fn prewarm_audit(&self, world: &World) -> MaterialPrewarmAudit {
        let mut pending = self.0.lock().unwrap();
        let state = match pending.state {
            RenderingState::Loading => 0,
            RenderingState::Ready => 1,
            RenderingState::Failed(_) => 2,
        };
        // Requests are assigned as a complete Vec before sealing. Its retained
        // storage identifies that assignment without scanning request contents.
        let requests = (pending.requests.as_ptr() as usize, pending.requests.len());
        if state == 0 {
            pending.terminal_prewarm_audit = None;
        } else if let Some((previous_requests, audit)) = pending.terminal_prewarm_audit {
            if previous_requests == requests
                && audit.state == state
                && audit.sealed == pending.sealed
            {
                return audit;
            }
        }
        let meshes = world.get_resource::<RenderAssets<RenderMesh>>();
        let materials = world.get_resource::<ErasedRenderAssets<PreparedMaterial>>();
        let warmed = world.get_resource::<WarmedPipelines>();
        let cache = world.get_resource::<PipelineCache>();
        let mut audit = MaterialPrewarmAudit {
            state,
            sealed: pending.sealed,
            request_count: pending.requests.len() as u32,
            warmed_queued: warmed.is_some_and(|warmed| warmed.queued),
            warmed_id_count: warmed.map_or(0, |warmed| warmed.ids.len() as u32),
            ready_pipeline_count: warmed.map_or(0, |warmed| {
                warmed
                    .ids
                    .iter()
                    .filter(|(id, _)| {
                        cache.is_some_and(|cache| cache.get_render_pipeline(*id).is_some())
                    })
                    .count() as u32
            }),
            view_key_count: world
                .get_resource::<ViewKeyCache>()
                .map_or(0, |keys| keys.len() as u32),
            cookies_present: world
                .get_resource::<LightingGpu>()
                .is_some_and(|gpu| gpu.cookies.is_some()),
            ..Default::default()
        };
        for (index, request) in pending.requests.iter().enumerate() {
            if meshes.is_none_or(|meshes| meshes.get(request.mesh).is_none()) {
                audit.missing_mesh_count += 1;
                audit.first_missing_mesh_request.get_or_insert(index as u32);
            }
            if materials.is_none_or(|materials| materials.get(request.material).is_none()) {
                audit.missing_material_count += 1;
                audit
                    .first_missing_material_request
                    .get_or_insert(index as u32);
            }
        }
        if state != 0 {
            pending.terminal_prewarm_audit = Some((requests, audit));
        }
        audit
    }
}
struct LoadingData {
    requests: Vec<PipelineRequest>,
    sealed: bool,
    state: RenderingState,
    shadow_capacity: usize,
    transparent_pass_capacity: usize,
    // A terminal prewarm boundary remains fixed until requests, seal, or state change.
    terminal_prewarm_audit: Option<((usize, usize), MaterialPrewarmAudit)>,
}
#[derive(Resource, Default)]
pub(crate) struct WarmedPipelines {
    pub(crate) ids: Vec<(CachedRenderPipelineId, String)>,
    queued: bool,
}

impl SourceMaterials {
    pub(crate) fn sort_data(
        &self,
        identity: (source::AssetId, u32),
        renderer: &source::render_lighting::RendererRecord,
        slot: u32,
    ) -> Result<Option<super::renderer_visibility::SourceSortData>, source::ReferenceError> {
        let Some(mut sort) = self.sort_data.get(&identity).copied() else {
            return Ok(None);
        };
        if slot > 0x7fff {
            return Err(source::ReferenceError(
                "Source transparent material slot is invalid".into(),
            ));
        }
        sort.source_material_index = (slot as u16) << 1;
        let layer = u32::from((i32::from(renderer.sorting_layer) + 0x8000) as u16);
        let order = u32::from((i32::from(renderer.sorting_order) + 0x8000) as u16);
        sort.layer_and_order = (layer << 16) | order;
        Ok(Some(sort))
    }
}
pub struct SourceMaterialPlugin;
impl Plugin for SourceMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractResourcePlugin::<MaterialLoading>::default(),
            ExtractResourcePlugin::<RetainedColors>::default(),
            MaterialPlugin::<terrain::TerrainMaterial>::default(),
            MaterialPlugin::<specular::SpecularMaterial>::default(),
            MaterialPlugin::<impostor::ImpostorMaterial>::default(),
            MaterialPlugin::<grass::material::GrassMaterial>::default(),
            grass::writers::GrassDisplacementPlugin,
            world_texture_upload::WorldTextureUploadPlugin,
            super::fixed_assets::FixedAssetsPlugin,
        ))
        .insert_resource(MaterialLoading(Arc::new(Mutex::new(LoadingData {
            requests: Vec::new(),
            sealed: false,
            state: RenderingState::Loading,
            shadow_capacity: 0,
            transparent_pass_capacity: 0,
            terminal_prewarm_audit: None,
        }))))
        .init_resource::<RetainedColors>()
        .init_resource::<super::renderers::RendererMeshes>()
        .init_resource::<super::renderer_visibility::CameraVisibility>()
        .add_systems(
            PostUpdate,
            super::renderer_visibility::filter_camera_visibility
                .after(bevy::camera::visibility::VisibilitySystems::CheckVisibility)
                .before(super::shadow_scene::prepare_scene)
                .before(
                    bevy::camera::visibility::VisibilitySystems::MarkNewlyHiddenEntitiesInvisible,
                ),
        )
        .add_systems(
            Update,
            super::renderers::bind_renderer_meshes
                .before(bind_materials)
                .before(bind_authored_materials),
        )
        .add_systems(
            Update,
            (
                material_readiness,
                bind_authored_materials
                    .after(bind_materials)
                    // Apply material commands before lighting removes its prototype.
                    .before(super::lighting::lighting_readiness)
                    .before(super::world::prepare_road_lamp),
            )
                .chain(),
        );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                Render,
                upload_retained_colors
                    .after(RenderSystems::PrepareBindGroups)
                    .before(prewarm_materials),
            );
            render.init_resource::<WarmedPipelines>().add_systems(
                Render,
                prewarm_materials.after(RenderSystems::PrepareBindGroups),
            );
        }
    }
}

pub fn load_materials(
    commands: &mut Commands,
    _server: &AssetServer,
    reference: &ReferenceManifest,
) -> Result<(), source::ReferenceError> {
    let mut sort_data = HashMap::new();
    let mut sort_identities = HashMap::<String, u32>::new();
    let mut shader_identities = HashMap::<String, i32>::new();
    for record in &reference.rendering.materials {
        if !matches!(
            record.state.alpha_mode,
            source::AlphaMode::Blend | source::AlphaMode::Premultiply | source::AlphaMode::Refract
        ) {
            continue;
        }
        let mut identity = serde_json::to_value(record).expect("validated material");
        identity.as_object_mut().unwrap().remove("asset");
        identity.as_object_mut().unwrap().remove("gltf_material");
        let next = sort_identities.len() as u32 + 1;
        let runtime_material = *sort_identities.entry(identity.to_string()).or_insert(next);
        let next = shader_identities.len() as i32 + 1;
        let runtime_shader = *shader_identities
            .entry(format!("{:?}", record.shader_family))
            .or_insert(next);
        sort_data.insert(
            (record.asset, record.gltf_material),
            super::renderer_visibility::SourceSortData {
                material_identity: runtime_material,
                glass_source: if record.shader_family == ShaderFamily::StainedGlass {
                    record.source.path_id
                } else {
                    0
                },
                material_key: crate::reference::sorting::material_sort_key(
                    runtime_material,
                    record.enable_instancing,
                    0,
                ),
                shader_identity: runtime_shader,
                source_material_index: 0,
                layer_and_order: 0,
                queue: if record.state.render_queue < 0 {
                    3000
                } else {
                    record.state.render_queue as i16
                },
                additive: record.shader_family == ShaderFamily::Standard,
                glass: record.shader_family == ShaderFamily::StainedGlass,
                grab: false,
            },
        );
    }
    let fixed_assets: HashSet<_> = reference
        .assets
        .iter()
        .filter(|asset| {
            matches!(
                asset.usage,
                AssetUsage::UniqueScene
                    | AssetUsage::RoadLampPrototype
                    | AssetUsage::SkyPrototype
                    | AssetUsage::FoodPrototype
                    | AssetUsage::ItemPrototype
                    | AssetUsage::PlayerPrototype
                    | AssetUsage::RagdollPrototype
                    | AssetUsage::SkyTraffic
                    | AssetUsage::CreaturePrototype
            )
        })
        .map(|asset| asset.id)
        .collect();
    // The player and ragdoll assets draw their characters with the renderer, segment and invisible materials.
    // Their other materials stay unloaded.
    let clothing = &reference.clothing;
    let outfits = [
        (&clothing.characters, &clothing.invisible_material),
        (
            &clothing.ragdoll.characters,
            &clothing.ragdoll.invisible_material,
        ),
    ];
    let records: Vec<_> = reference
        .rendering
        .materials
        .iter()
        .filter(|record| {
            fixed_assets.contains(&record.asset)
                && outfits.iter().all(|(characters, invisible)| {
                    record.asset != characters[0].asset
                        || record.source == **invisible
                        || characters.iter().any(|c| {
                            c.renderers
                                .iter()
                                .any(|renderer| renderer.materials.contains(&record.source))
                                || c.segments
                                    .iter()
                                    .any(|s| s.default_material == record.source)
                        })
                })
        })
        .map(|record| Arc::new(record.clone()))
        .collect();
    let mut textures = Vec::new();
    let mut texture_indices = HashMap::new();
    for record in &records {
        for texture in &record.properties.textures {
            if texture_indices.contains_key(texture.file()) {
                continue;
            }
            let input_staging = primary::primary_staging(texture, &reference.file_catalog)?;
            let payload_bytes = texture.payload_bytes()?;
            let cpu_reservation = source::fixed_reservation::png_cpu(payload_bytes, input_staging)?;
            texture_indices.insert(texture.file().to_owned(), textures.len());
            textures.push(SourceTexture {
                owner: Arc::new(()),
                upload: default(),
                failed: false,
                persistent: true,
                users: 0,
                record: Arc::new(texture.clone()),
                chain: None,
                input_staging,
                payload_bytes,
                cpu_reservation,
                image: None,
                label: format!(
                    "material {}:{} {}",
                    record.source.file, record.source.path_id, texture.slot
                ),
            });
        }
    }
    for texture in &reference.plates.material.textures {
        if texture_indices.contains_key(texture.file()) {
            continue;
        }
        let input_staging = primary::primary_staging(texture, &reference.file_catalog)?;
        let payload_bytes = texture.payload_bytes()?;
        let cpu_reservation = source::fixed_reservation::png_cpu(payload_bytes, input_staging)?;

        texture_indices.insert(texture.file().to_owned(), textures.len());
        textures.push(SourceTexture {
            owner: Arc::new(()),
            upload: default(),
            failed: false,
            persistent: true,
            users: 0,
            record: Arc::new(texture.clone()),
            chain: None,
            input_staging,
            payload_bytes,
            cpu_reservation,
            image: None,
            label: format!("source texture {}", texture.file()),
        });
    }
    let mut source = SourceMaterials {
        preparation: default(),
        generated: default(),
        world_textures: world_textures::WorldTextures::default(),
        world_primary: primary::PrimaryOwnership::default(),
        world_deferred: primary::DeferredAllocation::default(),
        standard_records: HashMap::new(),
        sort_data,
        records,
        textures,
        texture_indices,
        handles: HashMap::new(),
        shaders: default(),
        fixed_admitted: false,
        ragdoll: reference.clothing.ragdoll.asset,
        state: RenderingState::Loading,
        bound: false,
        texture_bytes: 0,
    };
    source.select_lighting_textures(reference)?;
    commands.insert_resource(source);
    Ok(())
}

pub fn material_readiness(
    server: Res<AssetServer>,
    mut source: ResMut<SourceMaterials>,
    loading: Res<MaterialLoading>,
) {
    if source.state != RenderingState::Loading {
        return;
    }
    for shader in &source.shaders {
        if let LoadState::Failed(error) = server.load_state(shader.id()) {
            source.state = RenderingState::Failed(format!("Source material shader: {error}"));
            return;
        }
    }
    if source.bound {
        source.state = loading.0.lock().unwrap().state.clone();
    }
}
fn assemble_textures(
    textures: &mut [SourceTexture],
    chains: &mut Assets<SourcePrimaryImage>,
    images: &mut Assets<Image>,
) -> usize {
    let mut bytes = 0;
    for texture in textures
        .iter_mut()
        .filter(|t| t.image.is_none() && !t.failed)
    {
        let Some(handle) = &texture.chain else {
            continue;
        };
        let Some(SourcePrimaryImage(image)) = chains.remove(handle.id()) else {
            continue;
        };
        texture.image = Some(images.add(image));
        texture.chain = None;
        bytes += texture.payload_bytes;
    }
    bytes
}
#[derive(serde::Deserialize)]
pub(crate) struct MaterialIdentity {
    pub asset: source::AssetId,
    pub gltf_material: u32,
}
/// Source submesh index of one glTF primitive.
fn submesh(primitive: &GltfExtras) -> Option<u16> {
    #[derive(serde::Deserialize)]
    struct PrimitiveIdentity {
        submesh: u16,
    }
    serde_json::from_str::<PrimitiveIdentity>(&primitive.value)
        .ok()
        .map(|value| value.submesh)
        .filter(|submesh| *submesh <= 0x7fff)
}
pub(crate) fn bind_authored_materials(
    mut commands: Commands,
    mut source: ResMut<SourceMaterials>,
    mut materials: ResMut<Assets<SourceMaterial>>,
    visuals: Res<super::reference::ReferenceVisuals>,
    renderer_meshes: Res<super::renderers::RendererMeshes>,
    master: Res<super::lighting::SourceLighting>,
    primitives: Query<
        (
            Entity,
            &GltfMaterialExtras,
            &Mesh3d,
            &GlobalTransform,
            Option<&bevy::camera::visibility::RenderLayers>,
            &GltfExtras,
            &super::renderer_visibility::SourceRenderer,
            Option<&OutfitSlot>,
        ),
        (Without<super::sky::SkyMesh>, Without<SourceMaterialWarmup>),
    >,
    loading: Res<MaterialLoading>,
    retained: Res<RetainedColors>,
) {
    if source.bound
        || source.state != RenderingState::Loading
        || !visuals.bound
        || !renderer_meshes.bound
        || master.road_lamps.recipe.is_none()
        || source
            .textures
            .iter()
            .any(|texture| texture.image.is_none())
    {
        return;
    }
    let mut shared = HashMap::<String, Handle<SourceMaterial>>::new();
    let mut segments =
        HashMap::<(source::AssetId, u8, u8, u8, bool, u32), Handle<SourceMaterial>>::new();
    let mut requests = Vec::new();
    let mut variants = HashSet::new();
    let mut transparent_pass_capacity = 0;
    for (entity, extras, mesh, transform, layers, primitive, renderer, slot) in &primitives {
        let mask = layers.map_or(1, |layers| {
            layers.iter().fold(0u32, |bits, layer| {
                bits | 1u32.checked_shl(layer as u32).unwrap_or(0)
            })
        });
        let identity: MaterialIdentity = match serde_json::from_str(&extras.value) {
            Ok(value) => value,
            Err(error) => {
                source.state = RenderingState::Failed(format!("Source material identity: {error}"));
                return;
            }
        };
        let find = |index: u32| {
            source
                .records
                .iter()
                .find(|record| record.asset == identity.asset && record.gltf_material == index)
                .cloned()
        };
        let Some(record) = find(identity.gltf_material) else {
            source.state = RenderingState::Failed(format!(
                "Missing material {}:{}",
                identity.asset.0, identity.gltf_material
            ));
            return;
        };
        let record = &*record;
        let reflected = transform.affine().matrix3.determinant() < 0.;
        let label = format!(
            "material {}:{} (asset {}, index {})",
            record.source.file, record.source.path_id, identity.asset.0, identity.gltf_material
        );
        if matches!(
            record.state.alpha_mode,
            source::AlphaMode::Blend | source::AlphaMode::Premultiply | source::AlphaMode::Refract
        ) {
            transparent_pass_capacity += 1 + usize::from(matches!(
                record.shader_family,
                ShaderFamily::Standard | ShaderFamily::StainedGlass
            ));
            let Some(submesh) = submesh(primitive) else {
                source.state =
                    RenderingState::Failed("Source transparent material slot is invalid".into());
                return;
            };
            let sort = source
                .sort_data(
                    (identity.asset, identity.gltf_material),
                    &renderer.0,
                    u32::from(submesh),
                )
                .expect("validated transparent slot")
                .expect("transparent material metadata");
            commands
                .entity(entity)
                .insert((bevy::render::batching::NoAutomaticBatching, sort));
        }
        let handle = if let Some(slot) = slot {
            // `playermodeloutfitscript.Refresh`: one copy of the segment material with its own color.
            let (Some(segment), invisible) = (find(slot.material), slot.invisible.map(find)) else {
                source.state = RenderingState::Failed(format!(
                    "Missing outfit material {}:{}",
                    identity.asset.0, slot.material
                ));
                return;
            };
            let handle = segments
                .entry((
                    identity.asset,
                    slot.figure,
                    slot.character,
                    slot.segment,
                    reflected,
                    mask,
                ))
                .or_insert_with(|| {
                    let mut material = make_material(&segment, reflected, mask, &source);
                    retain(&mut material, &retained);
                    materials.add(material)
                })
                .clone();
            let invisible = match invisible {
                Some(Some(record)) => Some(shared_handle(
                    &mut source,
                    &mut shared,
                    &mut materials,
                    &record,
                    reflected,
                    mask,
                )),
                Some(None) => {
                    source.state =
                        RenderingState::Failed("Missing outfit invisible material".into());
                    return;
                }
                None => None,
            };
            if let Some(invisible) = &invisible
                && variants.insert((mesh.0.id(), invisible.id()))
            {
                requests.push(PipelineRequest {
                    mesh: mesh.0.id(),
                    material: invisible.id().untyped(),
                    label: format!("{label} invisible"),
                });
            }
            commands.entity(entity).insert(OutfitMaterials {
                segment: handle.clone(),
                invisible,
            });
            handle
        } else {
            shared_handle(
                &mut source,
                &mut shared,
                &mut materials,
                record,
                reflected,
                mask,
            )
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(handle.clone()));
        if variants.insert((mesh.0.id(), handle.id())) {
            requests.push(PipelineRequest {
                mesh: mesh.0.id(),
                material: handle.id().untyped(),
                label,
            });
        }
    }
    if requests.is_empty() {
        source.state = RenderingState::Failed("No source material primitives".into());
        return;
    }
    info!(
        "Source materials: {} records, {} handles, {} textures, {} mip bytes, {} mesh/material pairs",
        source.records.len(),
        shared.len(),
        source.textures.len(),
        source.texture_bytes,
        requests.len()
    );
    source.bound = true;
    let mut pending = loading.0.lock().unwrap();
    pending.requests.extend(requests);
    pending.shadow_capacity = primitives.iter().count();
    // The checked plate contract owns exactly two retained glyph meshes.
    pending.transparent_pass_capacity = transparent_pass_capacity + 2;
    pending.sealed = false;
}

/// Material handle of a record. Records with the same identity share one handle.
fn shared_handle(
    source: &mut SourceMaterials,
    shared: &mut HashMap<String, Handle<SourceMaterial>>,
    materials: &mut Assets<SourceMaterial>,
    record: &MaterialRecord,
    reflected: bool,
    mask: u32,
) -> Handle<SourceMaterial> {
    let key = (record.asset, record.gltf_material, reflected, mask);
    if let Some(handle) = source.handles.get(&key) {
        return handle.clone();
    }
    let mut identity = serde_json::to_value(record).expect("validated record");
    identity.as_object_mut().unwrap().remove("asset");
    identity.as_object_mut().unwrap().remove("gltf_material");
    identity["reflected"] = serde_json::json!(reflected);
    identity["receiver_mask"] = serde_json::json!(mask);
    let handle = shared
        .entry(identity.to_string())
        .or_insert_with(|| materials.add(make_material(record, reflected, mask, source)))
        .clone();
    source.handles.insert(key, handle.clone());
    handle
}

/// Outfit segment slot of a player or ragdoll primitive.
#[derive(Component)]
pub struct OutfitSlot {
    /// 0 for the local player and its ragdoll, else the remote figure slot plus 1. Each figure has its own colors.
    pub figure: u8,
    pub character: u8,
    pub segment: u8,
    /// glTF material index of the segment `default_material`.
    pub material: u32,
    /// glTF material index of the invisible material. Only for a segment with `disable_able`.
    pub invisible: Option<u32>,
}
/// Materials of an outfit slot. The segment copy has a retained color.
#[derive(Component)]
pub struct OutfitMaterials {
    pub segment: Handle<SourceMaterial>,
    pub invisible: Option<Handle<SourceMaterial>>,
}

fn make_material(
    record: &MaterialRecord,
    reflected: bool,
    mask: u32,
    source: &SourceMaterials,
) -> SourceMaterial {
    if record.shader_family == ShaderFamily::AmplifyRope {
        // Retain the master rope surface. Source vertex sway remains deferred.
        return generated::from_base(
            StandardMaterial {
                base_color: Color::WHITE,
                metallic: 0.,
                perceptual_roughness: 0.5,
                fog_enabled: false,
                ..default()
            },
            mask,
            reflected,
        );
    }
    let floats = &record.properties.floats;
    let factor = |name: &str, default: f32| floats.get(name).copied().unwrap_or(default);
    let keyword = |name: &str| record.keywords.iter().any(|value| value == name);
    let mut flags = 0u32;
    for (bit, name) in [
        "_NORMALMAP",
        "_DETAIL_MULX2",
        "_METALLICGLOSSMAP",
        "_EMISSION",
        "_PARALLAXMAP",
        "_SMOOTHNESS_TEXTURE_ALBEDO_CHANNEL_A",
        "_SPECULARHIGHLIGHTS_OFF",
        "_GLOSSYREFLECTIONS_OFF",
        "_ALPHAPREMULTIPLY_ON",
        "_ALPHATEST_ON",
        "_ALPHABLEND_ON",
    ]
    .iter()
    .enumerate()
    {
        if keyword(name) {
            flags |= 1 << bit;
        }
    }
    let player = record.shader_family == ShaderFamily::PlayerModel;
    // Source player_model has no keywords. It always clips alpha, also in the caster.
    if player {
        flags |= 1 << 9;
    }
    // The ragdoll draws with `_Dist` 0 in every view: no head clip.
    if player && record.asset == source.ragdoll {
        flags |= 1 << 12;
    }
    let mut data = SourceUniform {
        color: Vec4::from_array(
            Color::srgba(
                record.color[0],
                record.color[1],
                record.color[2],
                record.color[3],
            )
            .to_linear()
            .to_f32_array(),
        ),
        emission: Vec4::ZERO,
        specular: Vec4::ZERO,
        factors: Vec4::new(
            factor("_Metallic", 0.),
            factor("_Glossiness", 0.5),
            factor("_GlossMapScale", 1.),
            factor("_Cutoff", 0.5),
        ),
        scales: Vec4::new(
            match record.shader_family {
                ShaderFamily::StainedGlass => factor("_BumpAmt", 0.),
                ShaderFamily::PlayerModel => factor("_BumpStrength", 1.),
                _ => factor("_BumpScale", 1.),
            },
            factor("_DetailNormalMapScale", 1.),
            factor("_OcclusionStrength", 1.),
            factor("_Parallax", 0.),
        ),
        flags: UVec4::new(
            flags,
            0,
            match record.shader_family {
                ShaderFamily::Standard => 0,
                ShaderFamily::UnlitColor => 1,
                ShaderFamily::StainedGlass => 2,
                ShaderFamily::AmplifyRope => unreachable!("rope uses its master surface"),
                ShaderFamily::PlayerModel => 3,
            },
            factor("_UVSec", 0.) as u32,
        ),
        banks: [UVec4::ZERO; 3],
        layers: [UVec4::ZERO; 3],
        uv: [Vec4::new(1., 1., 0., 0.); 9],
        sampling: [Vec4::ZERO; 9],
        defaults: [Vec4::ONE; 9],
    };
    if let Some(color) = record.properties.colors.get("_EmissionColor") {
        data.emission = Vec4::from_array(
            Color::srgba(color[0], color[1], color[2], color[3])
                .to_linear()
                .to_f32_array(),
        );
    }
    let mapping = record
        .texture_banks()
        .expect("validated texture bank count");
    let mut textures: [Option<Handle<Image>>; 7] = default();
    for (index, slot) in SLOTS.iter().enumerate() {
        if let Some(texture) = record
            .properties
            .textures
            .iter()
            .find(|entry| entry.slot == *slot)
        {
            let bank = usize::from(mapping[index].expect("mapped source texture"));
            data.banks[index / 4][index % 4] = bank as u32;
            textures[bank] = source.textures[source.texture_indices[texture.file()]]
                .image
                .clone();
            data.flags.y |= 1 << index;
            data.uv[index] = Vec4::new(
                texture.scale[0],
                texture.scale[1],
                texture.offset[0],
                texture.offset[1],
            );
            data.sampling[index] = Vec4::new(
                texture.uv_channel as f32,
                texture.sampler.mip_bias,
                (texture.sampler.wrap_u == 3) as u8 as f32,
                (texture.sampler.wrap_v == 3) as u8 as f32,
            );
        } else if let Some(default) = record.properties.defaults.get(*slot) {
            data.defaults[index] = match default {
                source::TextureDefault::White => Vec4::ONE,
                source::TextureDefault::Grey => Vec4::splat(0.5),
                source::TextureDefault::Black => Vec4::new(0., 0., 0., 1.),
                source::TextureDefault::Bump => Vec4::new(0.5, 0.5, 1., 1.),
            };
            if *default == source::TextureDefault::Grey && matches!(index, 0 | 4 | 6) {
                data.defaults[index] = Vec4::new(0.21404114, 0.21404114, 0.21404114, 0.5);
            }
        }
    }
    let alpha_mode = match record.state.alpha_mode {
        source::AlphaMode::Opaque => AlphaMode::Opaque,
        source::AlphaMode::Mask => AlphaMode::Mask(data.factors.w),
        source::AlphaMode::Blend | source::AlphaMode::Refract => AlphaMode::Blend,
        source::AlphaMode::Premultiply => AlphaMode::Premultiplied,
    };
    let main = record
        .properties
        .textures
        .iter()
        .find(|texture| texture.slot == "_MainTex");
    SourceMaterial {
        retained_color: None,
        preparation: default(),
        base: StandardMaterial {
            base_color_channel: main.map_or(bevy::mesh::UvChannel::Uv0, |texture| {
                if texture.uv_channel == 0 {
                    bevy::mesh::UvChannel::Uv0
                } else {
                    bevy::mesh::UvChannel::Uv1
                }
            }),
            uv_transform: main.map_or(bevy::math::Affine2::IDENTITY, |texture| {
                bevy::math::Affine2::from_scale_angle_translation(
                    Vec2::from_array(texture.scale),
                    0.,
                    Vec2::from_array(texture.offset),
                )
            }),
            base_color: Color::srgba(
                record.color[0],
                record.color[1],
                record.color[2],
                record.color[3],
            ),
            cull_mode: match record.state.cull {
                0 => None,
                1 => Some(Face::Front),
                _ => Some(Face::Back),
            },
            base_color_texture: textures[0].clone(),
            emissive_texture: textures[1].clone(),
            metallic_roughness_texture: textures[2].clone(),
            occlusion_texture: textures[3].clone(),
            normal_map_texture: textures[4].clone(),
            depth_map: textures[5].clone(),
            alpha_mode,
            unlit: record.shader_family == ShaderFamily::UnlitColor,
            ..default()
        },
        extension: SourceExtension {
            lod_fade: 1.,
            receiver_mask: mask,
            surface: SourceSurface {
                data,
                bank6: textures[6].clone(),
                state: SurfaceKey {
                    cull: record.state.cull,
                    z_write: record.state.z_write,
                    z_test: record.state.z_test,
                    src: record.state.src_blend,
                    dst: record.state.dst_blend,
                    reflected,
                    // Player model has the Standard pass set: ForwardBase, ForwardAdd and ShadowCaster.
                    standard: record.shader_family == ShaderFamily::Standard || player,
                    unlit: record.shader_family == ShaderFamily::UnlitColor,
                    transparent: matches!(
                        record.state.alpha_mode,
                        source::AlphaMode::Blend
                            | source::AlphaMode::Premultiply
                            | source::AlphaMode::Refract
                    ),
                    glass: record.shader_family == ShaderFamily::StainedGlass,
                    glass_shader: (record.shader_family == ShaderFamily::StainedGlass)
                        .then(|| source.shaders[2].clone()),
                    gpu_props: false,
                },
            },
        },
    }
}

fn prewarm_materials(world: &mut World) {
    let Some(loading) = world.get_resource::<MaterialLoading>().cloned() else {
        return;
    };
    let mut pending = loading.0.lock().unwrap();
    if !pending.sealed || pending.state != RenderingState::Loading {
        return;
    }
    if !world.resource::<WarmedPipelines>().queued {
        if pending.requests.iter().any(|request| {
            world
                .resource::<RenderAssets<RenderMesh>>()
                .get(request.mesh)
                .is_none()
                || world
                    .resource::<ErasedRenderAssets<PreparedMaterial>>()
                    .get(request.material)
                    .is_none()
        }) {
            return;
        }
        let view_keys: Vec<_> = world.resource::<ViewKeyCache>().values().copied().collect();
        if view_keys.is_empty() {
            return;
        }
        let mut work = Vec::new();
        for request in &pending.requests {
            let Some(mesh) = world
                .resource::<RenderAssets<RenderMesh>>()
                .get(request.mesh)
            else {
                return;
            };
            let Some(material) = world
                .resource::<ErasedRenderAssets<PreparedMaterial>>()
                .get(request.material)
            else {
                return;
            };
            for view_key in &view_keys {
                let bits: MeshPipelineKey = material.properties.mesh_pipeline_key_bits.downcast();
                let mesh_key = *view_key
                    | MeshPipelineKey::from_bits_retain(mesh.key_bits.bits())
                    | bits
                    | alpha_mode_pipeline_key(
                        material.properties.alpha_mode,
                        &Msaa::from_samples(view_key.msaa_samples()),
                    );
                work.push((
                    ErasedMaterialPipelineKey {
                        type_id: request.material.type_id(),
                        mesh_key: ErasedMeshPipelineKey::new(mesh_key),
                        material_key: material.properties.material_key.clone(),
                    },
                    mesh.layout.clone(),
                    material.properties.clone(),
                    request.label.clone(),
                ));
            }
        }
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        for (key, layout, properties, label) in work {
            let mesh_key: MeshPipelineKey = key.mesh_key.downcast();
            let mut descriptor = match world
                .resource::<MeshPipeline>()
                .specialize(mesh_key, &layout)
            {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    pending.state = RenderingState::Failed(format!("{label}: {error}"));
                    return;
                }
            };
            descriptor
                .layout
                .push(properties.material_layout.as_ref().unwrap().clone());
            descriptor.layout[0] = super::source_lighting::source_camera_layout();
            descriptor.layout[1] = super::source_lighting::surface_lighting_layout();
            let limits = world.resource::<RenderDevice>().limits();
            // Source outputs occupy locations 0..11, with conditional gaps.
            let varying_locations = 12;
            if varying_locations > limits.max_inter_stage_shader_variables {
                pending.state = RenderingState::Failed(format!(
                    "{label}: source shader needs {varying_locations} varying locations; device limit is {}",
                    limits.max_inter_stage_shader_variables
                ));
                return;
            }
            let mut textures = 0;
            let mut samplers = 0;
            let mut uniforms = 0;
            for layout in &descriptor.layout {
                for entry in &layout.entries {
                    if entry.visibility.contains(ShaderStages::FRAGMENT) {
                        match entry.ty {
                            BindingType::Texture { .. } => {
                                textures += entry.count.map_or(1, |count| count.get())
                            }
                            BindingType::Sampler(_) => samplers += 1,
                            BindingType::Buffer {
                                ty: BufferBindingType::Uniform,
                                ..
                            } => uniforms += 1,
                            _ => {}
                        }
                    }
                }
            }
            if textures > limits.max_sampled_textures_per_shader_stage
                || samplers > limits.max_samplers_per_shader_stage
                || uniforms > limits.max_uniform_buffers_per_shader_stage
            {
                pending.state = RenderingState::Failed(format!(
                    "{label}: layout requires {textures} textures, {samplers} samplers, {uniforms} uniforms; limits are {}, {}, {}",
                    limits.max_sampled_textures_per_shader_stage,
                    limits.max_samplers_per_shader_stage,
                    limits.max_uniform_buffers_per_shader_stage
                ));
                return;
            }
            let id = match base_specialize(world, key, &layout, &properties) {
                Ok(id) => id,
                Err(error) => {
                    pending.state = RenderingState::Failed(format!("{label}: {error}"));
                    return;
                }
            };
            if seen.insert(id) {
                info!(
                    "Source pipeline: {textures} fragment textures, {samplers} samplers, {uniforms} uniforms, {varying_locations} varying locations (upper bound; device limit {})",
                    limits.max_inter_stage_shader_variables
                );
                ids.push((id, label));
            }
        }
        let mut warmed = world.resource_mut::<WarmedPipelines>();
        warmed.ids = ids;
        warmed.queued = true;
    }
    let cache = world.resource::<PipelineCache>();
    for (id, label) in &world.resource::<WarmedPipelines>().ids {
        match cache.get_render_pipeline_state(*id) {
            CachedPipelineState::Ok(_) => {}
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => return,
            CachedPipelineState::Err(error) => {
                pending.state = RenderingState::Failed(format!("{label}: shader pipeline {error}"));
                return;
            }
            _ => return,
        }
    }
    info!(
        "Source material pipelines ready: {}",
        world.resource::<WarmedPipelines>().ids.len()
    );
    pending.state = RenderingState::Ready;
}

impl Default for SourceSurface {
    fn default() -> Self {
        Self {
            data: SourceUniform {
                color: Vec4::ONE,
                emission: Vec4::ZERO,
                specular: Vec4::ZERO,
                factors: Vec4::ZERO,
                scales: Vec4::ZERO,
                flags: UVec4::ZERO,
                banks: [UVec4::ZERO; 3],
                layers: [UVec4::ZERO; 3],
                uv: [Vec4::ZERO; 9],
                sampling: [Vec4::ZERO; 9],
                defaults: [Vec4::ONE; 9],
            },
            bank6: None,
            state: SurfaceKey {
                cull: 2,
                z_write: true,
                z_test: 4,
                src: 1,
                dst: 0,
                reflected: false,
                standard: true,
                unlit: false,
                transparent: false,
                glass: false,
                glass_shader: None,
                gpu_props: false,
            },
        }
    }
}

// Shared receiver bindings. Meshes retain StandardMaterial surface and alpha data.
use super::lighting::LightingGpu;
use bevy::ecs::system::SystemParamItem;

/// Forward source surfaces need the six base maps, not Bevy transmission maps.
#[derive(Asset, TypePath, Clone, Debug, Default)]
pub struct SourceMaterial {
    pub(crate) retained_color: Option<RetainedColor>,
    pub preparation: super::gpu_prepare::GpuPreparation,
    pub base: StandardMaterial,
    pub extension: SourceExtension,
}
impl SourceMaterial {
    pub(crate) fn shadow_batch_state(&self) -> super::shadow_batch::CasterBatchState {
        let surface = &self.extension.surface;
        super::shadow_batch::CasterBatchState {
            standard: surface.state.standard && !surface.state.transparent && !surface.state.glass,
            opaque: self.base.alpha_mode == AlphaMode::Opaque
                && surface.state.z_write
                && surface.state.z_test == 4
                && surface.state.src == 1
                && surface.state.dst == 0,
            flags: surface.data.flags.x,
            lod_fade: self.extension.lod_fade,
        }
    }
    pub(crate) fn caster_data(&self) -> SourceUniform {
        let mut data = self.extension.surface.data;
        if let Some(color) = &self.retained_color {
            data.color = color.0.lock().unwrap().linear;
        }
        data
    }
    /// Linear `_EmissionColor`.
    pub fn emission(&self) -> Vec4 {
        self.extension.surface.data.emission
    }
    /// The bound `_EmissionMap` image.
    pub fn emission_map(&self) -> Option<&Handle<Image>> {
        self.slot_map(EMISSION_SLOT)
    }
    pub fn set_emission(&mut self, linear: Vec4) {
        self.extension.surface.data.emission = linear;
    }
    /// Source `SetTexture("_EmissionMap", map)`. Null samples the white default.
    pub fn set_emission_map(&mut self, map: Option<Handle<Image>>) -> Result<(), String> {
        self.set_slot_map(EMISSION_SLOT, map)
    }
    /// Source `mainTexture = map`. Null samples the white default.
    pub fn set_main_map(&mut self, map: Option<Handle<Image>>) -> Result<(), String> {
        self.set_slot_map(MAIN_SLOT, map)
    }
    /// The bound image of texture slot `slot`.
    pub fn slot_map(&self, slot: usize) -> Option<&Handle<Image>> {
        let data = &self.extension.surface.data;
        (data.flags.y & (1 << slot) != 0)
            .then(|| self.bank(data.banks[slot / 4][slot % 4]).as_ref())
            .flatten()
    }
    fn set_slot_map(&mut self, slot: usize, map: Option<Handle<Image>>) -> Result<(), String> {
        let data = &mut self.extension.surface.data;
        let bit = 1 << slot;
        let Some(map) = map else {
            data.flags.y &= !bit;
            data.defaults[slot] = Vec4::ONE;
            return Ok(());
        };
        let bank = if data.flags.y & bit != 0 {
            data.banks[slot / 4][slot % 4]
        } else {
            // Banks 4 and 5 are the normal and depth maps: their presence changes the pipeline.
            let used = |bank: u32| {
                (0..9).any(|other| {
                    data.flags.y & (1 << other) != 0 && data.banks[other / 4][other % 4] == bank
                })
            };
            let bank = [1, 2, 3, 6]
                .into_iter()
                .find(|bank| !used(*bank))
                .ok_or_else(|| format!("No free texture bank for {}", SLOTS[slot]))?;
            data.flags.y |= bit;
            data.banks[slot / 4][slot % 4] = bank;
            data.uv[slot] = Vec4::new(1., 1., 0., 0.);
            bank
        };
        *self.bank_mut(bank) = Some(map);
        Ok(())
    }
    fn bank(&self, bank: u32) -> &Option<Handle<Image>> {
        match bank {
            0 => &self.base.base_color_texture,
            1 => &self.base.emissive_texture,
            2 => &self.base.metallic_roughness_texture,
            3 => &self.base.occlusion_texture,
            4 => &self.base.normal_map_texture,
            5 => &self.base.depth_map,
            _ => &self.extension.surface.bank6,
        }
    }
    fn bank_mut(&mut self, bank: u32) -> &mut Option<Handle<Image>> {
        match bank {
            0 => &mut self.base.base_color_texture,
            1 => &mut self.base.emissive_texture,
            2 => &mut self.base.metallic_roughness_texture,
            3 => &mut self.base.occlusion_texture,
            4 => &mut self.base.normal_map_texture,
            5 => &mut self.base.depth_map,
            _ => &mut self.extension.surface.bank6,
        }
    }
}
/// Index of `_MainTex` in `SLOTS`.
const MAIN_SLOT: usize = 0;
/// Index of `_EmissionMap` in `SLOTS`.
const EMISSION_SLOT: usize = 4;
/// Source `SetColor("_EmissionColor")` and, with `map`, `SetTexture("_EmissionMap")`.
/// `None` keeps the current map. Only a changed value touches the asset.
pub fn write_emission(
    assets: &mut Assets<SourceMaterial>,
    handle: &Handle<SourceMaterial>,
    linear: Vec4,
    map: Option<Handle<Image>>,
) -> Result<(), String> {
    let Some(material) = assets.get(handle) else {
        return Ok(());
    };
    if material.emission() == linear
        && map
            .as_ref()
            .is_none_or(|map| material.emission_map() == Some(map))
    {
        return Ok(());
    }
    let mut material = assets.get_mut(handle).expect("checked source material");
    material.set_emission(linear);
    if map.is_some() {
        material.set_emission_map(map)?;
    }
    Ok(())
}
impl AsBindGroup for SourceMaterial {
    type Data = (
        <StandardMaterial as AsBindGroup>::Data,
        <SourceExtension as AsBindGroup>::Data,
    );
    type Param = (
        <StandardMaterial as AsBindGroup>::Param,
        <SourceExtension as AsBindGroup>::Param,
    );
    fn label() -> &'static str {
        "source material"
    }
    fn bind_group_data(&self) -> Self::Data {
        (
            self.base.bind_group_data(),
            self.extension.bind_group_data(),
        )
    }
    fn as_bind_group(
        &self,
        descriptor: &BindGroupLayoutDescriptor,
        device: &RenderDevice,
        cache: &PipelineCache,
        param: &mut SystemParamItem<Self::Param>,
    ) -> Result<PreparedBindGroup, AsBindGroupError> {
        crate::presentation::gpu_prepare::bind_group(
            self,
            &self.preparation,
            descriptor,
            device,
            cache,
            param,
        )
    }
    fn unprepared_bind_group(
        &self,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        params: &mut SystemParamItem<Self::Param>,
        direct: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        if self.preparation.isolated() && !direct {
            return Err(AsBindGroupError::CreateBindGroupDirectly);
        }
        let mut prepared = self
            .base
            .unprepared_bind_group(layout, device, &mut params.0, true)?;
        prepared.bindings.0.retain(|(binding, _)| *binding <= 12);
        let extension =
            self.extension
                .unprepared_bind_group(layout, device, &mut params.1, true)?;
        prepared.bindings.0.extend(extension.bindings.0);
        if let Some(color) = &self.retained_color {
            let mut retained = color.0.lock().unwrap();
            let buffer = retained
                .buffer
                .get_or_insert_with(|| {
                    let mut bytes = encase::UniformBuffer::new(Vec::new());
                    bytes.write(&self.extension.surface.data).unwrap();
                    device.create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("retained condition receiver"),
                        contents: bytes.as_ref(),
                        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    })
                })
                .clone();
            *prepared
                .bindings
                .0
                .iter_mut()
                .find(|(binding, _)| *binding == 120)
                .expect("source color binding") = (120, OwnedBindingResource::Buffer(buffer));
            retained.dirty = true;
        }
        Ok(prepared)
    }
    fn bind_group_layout_entries(device: &RenderDevice, _: bool) -> Vec<BindGroupLayoutEntry> {
        let mut entries = StandardMaterial::bind_group_layout_entries(device, true);
        entries.retain(|entry| entry.binding <= 12);
        entries.extend(SourceExtension::bind_group_layout_entries(device, true));
        entries
    }
}
impl Material for SourceMaterial {
    fn vertex_shader() -> ShaderRef {
        SourceExtension::vertex_shader()
    }
    fn fragment_shader() -> ShaderRef {
        SourceExtension::fragment_shader()
    }
    fn alpha_mode(&self) -> AlphaMode {
        self.base.alpha_mode
    }
    fn opaque_render_method(&self) -> bevy::material::OpaqueRendererMethod {
        self.base.opaque_render_method()
    }
    fn depth_bias(&self) -> f32 {
        self.base.depth_bias
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        StandardMaterial::specialize(
            pipeline,
            descriptor,
            layout,
            MaterialPipelineKey::<StandardMaterial> {
                mesh_key: key.mesh_key,
                bind_group_data: key.bind_group_data.0,
            },
        )?;
        // The generated material path also uses zero transmission. Remove the
        // matching shader feature so it cannot read the omitted bindings.
        let unused = |definition: &ShaderDefVal| {
            let name = match definition {
                ShaderDefVal::Bool(name, _)
                | ShaderDefVal::Int(name, _)
                | ShaderDefVal::UInt(name, _) => name,
            };
            name != "PBR_TRANSMISSION_TEXTURES_SUPPORTED"
                && name != "PBR_MULTI_LAYER_MATERIAL_TEXTURES_SUPPORTED"
                && name != "PBR_ANISOTROPY_TEXTURE_SUPPORTED"
                && name != "PBR_SPECULAR_TEXTURES_SUPPORTED"
                && name != "VISIBILITY_RANGE_DITHER"
                && name != "LOAD_PREPASS_NORMALS"
        };
        descriptor.vertex.shader_defs.retain(unused);
        if let Some(fragment) = &mut descriptor.fragment {
            fragment.shader_defs.retain(unused);
        }
        SourceExtension::specialize(
            &MaterialExtensionPipeline {
                mesh_pipeline: pipeline.mesh_pipeline.clone(),
            },
            descriptor,
            layout,
            MaterialExtensionKey {
                mesh_key: key.mesh_key,
                bind_group_data: key.bind_group_data.1,
            },
        )
    }
}

#[derive(Asset, TypePath, Clone, Debug)]
pub struct SourceExtension {
    pub lod_fade: f32,
    pub receiver_mask: u32,
    pub surface: SourceSurface,
}
impl SourceExtension {
    pub fn has_caster(&self) -> bool {
        self.surface.state.standard
    }
    /// Only a shared opaque or cutout material can draw GPU prop rows.
    pub fn use_gpu_props(&mut self) {
        assert!(!self.surface.state.transparent && !self.surface.state.glass);
        self.surface.state.gpu_props = true;
    }
}

impl MaterialExtension for SourceExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_mesh.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_surface.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let state = key.bind_group_data;
        descriptor.layout[0] = super::source_lighting::source_camera_layout();
        descriptor.layout[1] = if state.glass {
            super::stained_glass::glass_view_layout()
        } else {
            super::source_lighting::surface_lighting_layout()
        };
        if state.glass {
            let shader = state
                .glass_shader
                .as_ref()
                .expect("source glass shader handle");
            descriptor.vertex.shader = shader.clone();
            if let Some(fragment) = &mut descriptor.fragment {
                fragment.shader = shader.clone();
            }
        }
        let mut attributes = Vec::new();
        for (location, attribute) in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_UV_0,
            Mesh::ATTRIBUTE_UV_1,
            Mesh::ATTRIBUTE_TANGENT,
            Mesh::ATTRIBUTE_COLOR,
            // Bevy reads joints at 6 and weights at 7 for a skinned mesh, such as the siphon hose.
            Mesh::ATTRIBUTE_JOINT_INDEX,
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
        ]
        .into_iter()
        .enumerate()
        {
            if layout.0.contains(attribute) {
                attributes.push(attribute.at_shader_location(location as u32));
            }
        }
        if state.gpu_props {
            descriptor.layout[2] = super::prop_draws::draw_layout();
        }
        attributes.push(super::renderers::SOURCE_BOUNDS_CENTER.at_shader_location(8));
        attributes.push(super::renderers::SOURCE_BOUNDS_EXTENTS.at_shader_location(9));
        descriptor.vertex.buffers = vec![layout.0.get_layout(&attributes)?];
        for (name, value) in [
            ("SOURCE_STANDARD", state.standard),
            ("SOURCE_UNLIT", state.unlit),
            ("SOURCE_TRANSPARENT", state.transparent),
            ("SOURCE_OBJECT_REFLECTED", state.reflected),
            ("SOURCE_FORWARD_ONE", state.single_pass()),
            ("SOURCE_LAMPS", state.single_pass()),
            ("SOURCE_GPU_PROPS", state.gpu_props),
        ] {
            if value || name == "SOURCE_OBJECT_REFLECTED" {
                descriptor
                    .vertex
                    .shader_defs
                    .push(ShaderDefVal::Bool(name.into(), value));
                if let Some(fragment) = &mut descriptor.fragment {
                    fragment
                        .shader_defs
                        .push(ShaderDefVal::Bool(name.into(), value));
                }
            }
        }

        descriptor.primitive.cull_mode = match state.cull {
            0 => None,
            1 => Some(Face::Front),
            _ => Some(Face::Back),
        };
        let view_inverted = key.mesh_key.contains(MeshPipelineKey::INVERT_CULLING);
        descriptor.primitive.front_face = if state.reflected ^ view_inverted {
            FrontFace::Cw
        } else {
            FrontFace::Ccw
        };
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.format = super::render_targets::DEPTH_FORMAT;
            depth.depth_write_enabled = Some(state.z_write);
            // Source uses conventional depth. Bevy uses reverse depth.
            depth.depth_compare = Some(match state.z_test {
                0 | 8 => CompareFunction::Always,
                1 => CompareFunction::Never,
                2 => CompareFunction::Greater,
                3 => CompareFunction::Equal,
                4 => CompareFunction::GreaterEqual,
                5 => CompareFunction::Less,
                6 => CompareFunction::NotEqual,
                _ => CompareFunction::LessEqual,
            });
        }
        if let Some(fragment) = &mut descriptor.fragment {
            if state.src == 1 && state.dst == 0 {
                fragment
                    .shader_defs
                    .push(ShaderDefVal::Bool("SOURCE_REPLACEMENT".into(), true));
            }
            for target in fragment.targets.iter_mut().flatten() {
                let component = BlendComponent {
                    src_factor: blend_factor(state.src),
                    dst_factor: blend_factor(state.dst),
                    operation: BlendOperation::Add,
                };
                target.blend = if state.src == 1 && state.dst == 0 {
                    None
                } else {
                    Some(BlendState {
                        color: component,
                        alpha: component,
                    })
                };
            }
            fragment
                .shader_defs
                .push(ShaderDefVal::Bool("SOURCE_SURFACE".into(), true));
        }
        Ok(())
    }
}
impl Default for SourceExtension {
    fn default() -> Self {
        Self {
            receiver_mask: u32::MAX,
            lod_fade: 1.,
            surface: SourceSurface::default(),
        }
    }
}
impl AsBindGroup for SourceExtension {
    type Data = SurfaceKey;
    type Param = (
        <SourceReceiver as AsBindGroup>::Param,
        <SourceSurface as AsBindGroup>::Param,
    );
    fn label() -> &'static str {
        "source surface lighting"
    }
    fn bind_group_data(&self) -> Self::Data {
        self.surface.state.clone()
    }
    fn unprepared_bind_group(
        &self,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        params: &mut SystemParamItem<Self::Param>,
        _: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let mut result = SourceReceiver {
            receiver_mask: self.receiver_mask,
            lod_fade: self.lod_fade,
        }
        .unprepared_bind_group(layout, device, &mut params.0, false)?;
        result.bindings.0.extend(
            self.surface
                .unprepared_bind_group(layout, device, &mut params.1, false)?
                .bindings
                .0,
        );
        Ok(result)
    }
    fn bind_group_layout_entries(device: &RenderDevice, force: bool) -> Vec<BindGroupLayoutEntry> {
        let mut entries = SourceReceiver::bind_group_layout_entries(device, force);
        entries.extend(SourceSurface::bind_group_layout_entries(device, force));
        entries
    }
}
pub struct SourceReceiver {
    pub receiver_mask: u32,
    pub lod_fade: f32,
}
impl AsBindGroup for SourceReceiver {
    type Data = ();
    type Param = SRes<LightingGpu>;
    fn label() -> &'static str {
        "source lighting"
    }
    fn bind_group_data(&self) -> Self::Data {
        ()
    }
    fn unprepared_bind_group(
        &self,
        _: &BindGroupLayout,
        device: &RenderDevice,
        params: &mut SystemParamItem<Self::Param>,
        _: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let gpu = params;
        let cookies = gpu
            .cookies
            .as_ref()
            .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let result = UnpreparedBindGroup {
            bindings: BindingResources(vec![
                (
                    100,
                    OwnedBindingResource::Buffer(
                        gpu.uniform
                            .buffer()
                            .ok_or(AsBindGroupError::RetryNextUpdate)?
                            .clone(),
                    ),
                ),
                (
                    103,
                    OwnedBindingResource::TextureView(
                        TextureViewDimension::D2Array,
                        cookies[2].0.clone(),
                    ),
                ),
                (
                    104,
                    OwnedBindingResource::Sampler(
                        SamplerBindingType::Filtering,
                        cookies[2].1.clone(),
                    ),
                ),
                (
                    107,
                    OwnedBindingResource::TextureView(
                        TextureViewDimension::D2,
                        cookies[1].0.clone(),
                    ),
                ),
                (
                    108,
                    OwnedBindingResource::Sampler(
                        SamplerBindingType::Filtering,
                        cookies[1].1.clone(),
                    ),
                ),
                (
                    111,
                    OwnedBindingResource::Sampler(
                        SamplerBindingType::Filtering,
                        gpu.cookie_sampler.clone(),
                    ),
                ),
                (
                    105,
                    OwnedBindingResource::TextureView(
                        TextureViewDimension::D2,
                        gpu.attenuation_view.clone(),
                    ),
                ),
                (
                    106,
                    OwnedBindingResource::Buffer(
                        device.create_buffer_with_data(&BufferInitDescriptor {
                            label: Some("source receiver mask"),
                            contents: &[
                                self.receiver_mask.to_le_bytes(),
                                self.lod_fade.to_le_bytes(),
                                [0; 4],
                                [0; 4],
                            ]
                            .concat(),
                            usage: BufferUsages::UNIFORM,
                        }),
                    ),
                ),
            ]),
        };
        Ok(result)
    }
    fn bind_group_layout_entries(_: &RenderDevice, _: bool) -> Vec<BindGroupLayoutEntry> {
        use binding_types::*;
        let mut entries = BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (
                    100,
                    uniform_buffer::<super::lighting::LightingUniform>(false),
                ),
                (
                    103,
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                ),
                (104, sampler(SamplerBindingType::Filtering)),
                (
                    105,
                    texture_2d(TextureSampleType::Float { filterable: true }),
                ),
                (106, uniform_buffer::<UVec4>(false)),
                (
                    107,
                    texture_2d(TextureSampleType::Float { filterable: true }),
                ),
                (108, sampler(SamplerBindingType::Filtering)),
                (111, sampler(SamplerBindingType::Filtering)),
            ),
        )
        .to_vec();
        for entry in &mut entries {
            if matches!(entry.binding, 100 | 106) {
                entry.visibility = ShaderStages::VERTEX_FRAGMENT;
            }
        }
        entries
    }
}
use bevy::ecs::system::lifetimeless::SRes;

#[derive(Component, Clone, Copy)]
pub struct SourceShadowMode(pub u8);

#[derive(Resource, Default)]
pub struct MaterialCache {
    /// Mirrored copies of authored materials. They key mirrored receiver conversions.
    pub variants: std::collections::HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    pub converted:
        std::collections::HashMap<(AssetId<StandardMaterial>, u32), Handle<SourceMaterial>>,
}
pub fn bind_materials(
    mut commands: Commands,
    visuals: Res<super::reference::ReferenceVisuals>,
    originals: Res<Assets<StandardMaterial>>,
    mut materials: ResMut<Assets<SourceMaterial>>,
    mut cache: ResMut<MaterialCache>,
    imported_nodes: Query<&bevy::gltf::GltfExtras>,
    authored: Res<SourceMaterials>,
    renderer_meshes: Res<super::renderers::RendererMeshes>,
    query: Query<
        (
            Entity,
            &MeshMaterial3d<StandardMaterial>,
            Option<&bevy::camera::visibility::RenderLayers>,
            Option<&SourceShadowMode>,
            Option<&ChildOf>,
            Option<&GltfMaterialExtras>,
            &GlobalTransform,
            Option<&GltfExtras>,
            Option<&super::renderer_visibility::SourceRenderer>,
        ),
        (With<Mesh3d>, Without<super::sky::SkyMesh>),
    >,
) {
    if !visuals.bound || !renderer_meshes.bound || !authored.textures_ready() {
        return;
    }
    for (entity, handle, layers, shadow_mode, parent, extras, global, primitive, renderer) in &query
    {
        if shadow_mode.is_none()
            && parent.is_some_and(|parent| imported_nodes.contains(parent.parent()))
        {
            continue;
        }
        // Authored primitives wait for their renderer record: it holds the pass sort order.
        if extras.is_some() && renderer.is_none() {
            continue;
        }
        let mask = layers.map_or(1, |layers| {
            layers.iter().fold(0, |mask, layer| {
                mask | 1u32.checked_shl(layer as u32).unwrap_or(0)
            })
        });
        let Some(base) = originals.get(&handle.0).cloned() else {
            continue;
        };
        let material = cache
            .converted
            .entry((handle.0.id(), mask))
            .or_insert_with(|| {
                if let Some(extras) = extras {
                    let identity: MaterialIdentity = serde_json::from_str(&extras.value)
                        .expect("validated source material identity");
                    materials.add(authored_material(
                        (identity.asset, identity.gltf_material),
                        global.affine().matrix3.determinant() < 0.,
                        mask,
                        &authored,
                    ))
                } else {
                    let mut base = base.clone();
                    base.fog_enabled = false;
                    materials.add(generated::from_base(
                        base,
                        mask,
                        global.affine().matrix3.determinant() < 0.,
                    ))
                }
            })
            .clone();
        // Blended authored primitives, such as spawned item glass, need their pass sort data.
        if let (Some(extras), Some(primitive), Some(renderer)) = (extras, primitive, renderer) {
            let identity: MaterialIdentity =
                serde_json::from_str(&extras.value).expect("validated source material identity");
            let sort = submesh(primitive).and_then(|submesh| {
                authored
                    .sort_data(
                        (identity.asset, identity.gltf_material),
                        &renderer.0,
                        u32::from(submesh),
                    )
                    .expect("validated transparent slot")
            });
            if let Some(sort) = sort {
                commands
                    .entity(entity)
                    .insert((sort, bevy::render::batching::NoAutomaticBatching));
            }
        }
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((MeshMaterial3d(material), bevy::light::NotShadowCaster));
    }
}

#[derive(Clone)]
pub struct LampReceiverChoice {
    entity: Entity,
    sort: Option<super::renderer_visibility::SourceSortData>,
    material: Handle<SourceMaterial>,
}
#[derive(Clone)]
pub struct LampMaterialChoice {
    key: (crate::reference::AssetId, u32),
    receivers: Vec<LampReceiverChoice>,
}

impl LampMaterialChoice {}

/// Prepare source selections and actual mesh pipeline variants during loading.
pub(crate) fn prepare_lamp_materials(
    mut commands: Commands,
    reference: Res<crate::app::RuntimeReference>,
    sim: Res<super::sim_frame::SimFrameResource>,
    mut source: ResMut<super::lighting::SourceLighting>,
    mut originals: ResMut<Assets<StandardMaterial>>,
    mut materials: ResMut<Assets<SourceMaterial>>,
    mut cache: ResMut<MaterialCache>,
    authored_materials: Res<SourceMaterials>,
    receivers: Query<(
        &Mesh3d,
        &MeshMaterial3d<SourceMaterial>,
        &GlobalTransform,
        &super::renderer_visibility::SourceRenderer,
        &GltfExtras,
    )>,
) {
    if !authored_materials.textures_ready()
        || !source.bound
        || source.materials_prepared
        || !source.materials_ready
        || source.road_lamps.recipe.is_none()
    {
        return;
    }
    if source.lamp_materials.iter().any(|target| {
        target
            .receivers
            .iter()
            .any(|entity| !receivers.contains(*entity))
    }) {
        return;
    }
    let source = &mut *source;
    let far = Transform::from_translation(Vec3::splat(1_000_000.));
    for target in &mut source.lamp_materials {
        let output = &sim.frame.lamp_materials[target.output_index];
        let mut keys = Vec::new();
        for state in reference
            .0
            .lighting
            .controllers
            .iter()
            .flat_map(|c| &c.states)
        {
            if !state.renderers.contains(&output.renderer) {
                continue;
            }
            for material in [&state.on_material, &state.off_material]
                .into_iter()
                .flatten()
            {
                let key = (material.asset, material.gltf_material);
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        for key in keys {
            let authored = source
                .materials
                .iter()
                .find(|m| (m.asset, m.index) == key)
                .expect("validated lamp material");
            let authored_base = authored_caster_material(key, &authored_materials);
            let mut choice = LampMaterialChoice {
                key,
                receivers: Vec::with_capacity(target.receivers.len()),
            };
            for &entity in &target.receivers {
                let (mesh, receiver, global, renderer, primitive) = receivers.get(entity).unwrap();
                let mask = materials.get(&receiver.0).unwrap().extension.receiver_mask;
                let inverted = global.affine().matrix3.determinant() < 0.;
                let mut base = authored_base.clone();
                if inverted {
                    base.cull_mode = base.cull_mode.map(|face| match face {
                        Face::Front => Face::Back,
                        Face::Back => Face::Front,
                    });
                }
                let surface = if inverted {
                    cache
                        .variants
                        .entry(authored.handle.id())
                        .or_insert_with(|| originals.add(base.clone()))
                        .clone()
                } else {
                    authored.handle.clone()
                };
                let material = cache
                    .converted
                    .entry((surface.id(), mask))
                    .or_insert_with(|| {
                        materials.add(authored_material(key, inverted, mask, &authored_materials))
                    })
                    .clone();
                let sort = match authored_materials.sort_data(
                    key,
                    &renderer.0,
                    serde_json::from_str::<serde_json::Value>(&primitive.value)
                        .expect("validated primitive")["submesh"]
                        .as_u64()
                        .expect("validated submesh") as u32,
                ) {
                    Ok(sort) => sort,
                    Err(error) => {
                        source.state = super::lighting::RenderingState::Failed(error.0);
                        return;
                    }
                };
                let mut warm = commands.spawn((
                    mesh.clone(),
                    MeshMaterial3d(material.clone()),
                    SourceMaterialWarmup,
                    renderer.clone(),
                    far,
                    Visibility::Visible,
                    bevy::light::NotShadowCaster,
                    bevy::camera::visibility::NoFrustumCulling,
                ));
                if let Some(sort) = sort {
                    warm.insert((sort, bevy::render::batching::NoAutomaticBatching));
                }
                source.warmups.push(warm.id());
                choice.receivers.push(LampReceiverChoice {
                    entity,
                    sort,
                    material,
                });
            }
            target.choices.push(choice);
        }
    }
    source.materials_prepared = true;
    info!(
        "Source lamp cache: {} targets, {} choices, {} materials, {} warmup meshes",
        source.lamp_materials.len(),
        source
            .lamp_materials
            .iter()
            .map(|t| t.choices.len())
            .sum::<usize>(),
        cache.converted.len(),
        source.warmups.len()
    );
}

/// Select cached handles. Gameplay does not allocate materials or mesh variants.
pub(crate) fn sync_lamp_materials(
    mut commands: Commands,
    sim: Res<super::sim_frame::SimFrameResource>,
    mut source: ResMut<super::lighting::SourceLighting>,
    receivers: Query<&MeshMaterial3d<SourceMaterial>>,
) {
    if !source.materials_prepared {
        return;
    }
    for target in &mut source.lamp_materials {
        let output = &sim.frame.lamp_materials[target.output_index];
        let key = (output.asset, output.material_index);
        let choice = target
            .choices
            .iter()
            .find(|choice| choice.key == key)
            .expect("prepared source material choice");
        if target.current == Some(key)
            && choice
                .receivers
                .iter()
                .all(|r| receivers.get(r.entity).is_ok_and(|m| m.0 == r.material))
        {
            continue;
        }
        for receiver in &choice.receivers {
            commands
                .entity(receiver.entity)
                .insert(MeshMaterial3d(receiver.material.clone()));
            if let Some(sort) = receiver.sort {
                commands
                    .entity(receiver.entity)
                    .insert((sort, bevy::render::batching::NoAutomaticBatching));
            } else {
                commands
                    .entity(receiver.entity)
                    .remove::<super::renderer_visibility::SourceSortData>();
            }
        }
        target.current = Some(key);
    }
}

#[track_caller]
pub fn authored_material(
    key: (source::AssetId, u32),
    reflected: bool,
    mask: u32,
    source: &SourceMaterials,
) -> SourceMaterial {
    let Some(record) = source
        .records
        .iter()
        .find(|r| (r.asset, r.gltf_material) == key)
    else {
        panic!(
            "Missing authored material {}:{} (asset records {}, world admitted {})",
            key.0.0,
            key.1,
            source.records.iter().filter(|r| r.asset == key.0).count(),
            source.world_primary_admitted(key),
        );
    };
    let mut material = make_material(record, reflected, mask, source);
    material.preparation = source.preparation.clone();
    material
}
impl SourceMaterials {
    pub(crate) fn shareable_generated_primary(&self, identity: (source::AssetId, u32)) -> bool {
        self.records
            .iter()
            .find(|record| (record.asset, record.gltf_material) == identity)
            .is_some_and(|record| super::world::shared_materials::shareable_primary(record))
    }
    pub fn textures_ready(&self) -> bool {
        self.textures.iter().all(|t| t.image.is_some())
    }
}

pub fn authored_caster_material(
    key: (source::AssetId, u32),
    source: &SourceMaterials,
) -> StandardMaterial {
    let mut base = authored_material(key, false, 0, source).base;
    base.emissive_texture = None;
    base.metallic_roughness_texture = None;
    base.occlusion_texture = None;
    base.normal_map_texture = None;
    base.depth_map = None;
    base
}

impl SourceMaterials {
    pub(crate) fn texture(&self, file: &str) -> Option<Handle<Image>> {
        self.texture_indices
            .get(file)
            .and_then(|i| self.textures[*i].image.clone())
    }
}
pub(crate) fn warm_retained_material(
    world: &World,
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    label: String,
) {
    world
        .resource::<MaterialLoading>()
        .warm(mesh, material, label);
}

pub(crate) fn seal_plate_materials(world: &World) {
    if world.resource::<SourceMaterials>().bound
        && world
            .resource::<super::conditions::ConditionVisuals>()
            .prepared
        && world
            .resource::<super::consumables::ConsumableVisuals>()
            .warmed
    {
        let warmup = world
            .iter_entities()
            .filter(|e| e.contains::<MeshMaterial3d<SourceMaterial>>())
            .count();
        let weight = world.resource::<SourceMaterials>().maximum_pass_weight();
        let mut pending = world.resource::<MaterialLoading>().0.lock().unwrap();
        pending.transparent_pass_capacity =
            pending.transparent_pass_capacity.max(warmup * weight + 2);
        pending.sealed = true;
    }
}

pub(crate) fn set_source_color(material: &mut SourceMaterial, color: [f32; 4]) {
    let value = Color::srgba(color[0], color[1], color[2], color[3]);
    material.base.base_color = value;
    material.extension.surface.data.color = Vec4::from_array(value.to_linear().to_f32_array());
}
impl SourceMaterials {
    fn maximum_pass_weight(&self) -> usize {
        self.records
            .iter()
            .filter(|r| {
                matches!(
                    r.state.alpha_mode,
                    source::AlphaMode::Blend
                        | source::AlphaMode::Premultiply
                        | source::AlphaMode::Refract
                )
            })
            .map(|r| {
                1 + usize::from(matches!(
                    r.shader_family,
                    ShaderFamily::Standard | ShaderFamily::StainedGlass
                ))
            })
            .max()
            .unwrap_or(1)
    }
}

// The source shader reads color first in binding 120. Keep the original buffer and bind group.
#[derive(Clone, Debug)]
pub(crate) struct RetainedColor(Arc<Mutex<RetainedColorData>>);
#[derive(Debug)]
struct RetainedColorData {
    linear: Vec4,
    revision: u64,
    dirty: bool,
    buffer: Option<Buffer>,
}
#[derive(Resource, Clone, Default, ExtractResource)]
pub(crate) struct RetainedColors(Arc<Mutex<Vec<RetainedColor>>>);
impl RetainedColor {
    pub(crate) fn revision_color(&self) -> (u64, Vec4) {
        let value = self.0.lock().unwrap();
        (value.revision, value.linear)
    }
}
impl SourceUniform {
    pub(crate) fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }
}
pub(crate) fn retain_color(material: &mut SourceMaterial, world: &World) {
    retain(material, world.resource::<RetainedColors>());
}
fn retain(material: &mut SourceMaterial, colors: &RetainedColors) {
    let color = RetainedColor(Arc::new(Mutex::new(RetainedColorData {
        linear: material.extension.surface.data.color,
        revision: 0,
        dirty: true,
        buffer: None,
    })));
    colors.0.lock().unwrap().push(color.clone());
    material.retained_color = Some(color);
}
pub(crate) fn update_retained_color(material: &SourceMaterial, color: [f32; 4]) {
    let linear = Vec4::from_array(
        Color::srgba(color[0], color[1], color[2], color[3])
            .to_linear()
            .to_f32_array(),
    );
    let mut retained = material
        .retained_color
        .as_ref()
        .expect("retained source color")
        .0
        .lock()
        .unwrap();
    if retained.linear != linear {
        retained.linear = linear;
        retained.revision += 1;
        retained.dirty = true;
    }
}
fn upload_retained_colors(colors: Res<RetainedColors>, queue: Res<RenderQueue>) {
    for color in colors.0.lock().unwrap().iter() {
        let mut color = color.0.lock().unwrap();
        if !color.dirty {
            continue;
        }
        let Some(buffer) = &color.buffer else {
            continue;
        };
        let mut bytes = [0u8; 16];
        for (slot, value) in bytes.chunks_exact_mut(4).zip(color.linear.to_array()) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        queue.write_buffer(buffer, 0, &bytes);
        color.dirty = false;
    }
}

impl SourceMaterials {
    pub(crate) fn measurement_images(&self) -> impl Iterator<Item = (AssetId<Image>, bool)> + '_ {
        self.textures
            .iter()
            .flat_map(|t| t.image.iter().map(move |h| (h.id(), true)))
            .chain(
                self.world_textures
                    .measurement_images()
                    .map(|id| (id, true)),
            )
    }
}

impl SourceMaterials {
    pub fn drain_ready(
        &mut self,
        server: &AssetServer,
        chains: &Assets<SourcePrimaryImage>,
        batches: &mut Assets<crate::presentation::texture_batches::SourceTextureBatch>,
    ) -> bool {
        self.world_textures.drain_ready(server, batches)
            && self.textures.iter().all(|t| {
                (t.image.is_some() && primary::pending_texture_cpu(t) == 0)
                    || (t.image.is_none()
                        && t.chain.iter().all(|h| {
                            chains.contains(h.id())
                                || matches!(server.load_state(h.id()), LoadState::Failed(_))
                        }))
            })
    }
    pub fn release_all(
        &mut self,
        server: &AssetServer,
        images: &mut Assets<Image>,
        chains: &mut Assets<SourcePrimaryImage>,
        materials: &mut Assets<SourceMaterial>,
    ) -> Result<(), source::ReferenceError> {
        let assets: Vec<_> = self.world_primary.asset_ids().collect();
        for asset in assets {
            self.release_world_asset(asset, images, chains, materials, server)?;
        }
        self.world_textures.release_all(images);
        Ok(())
    }
    pub fn settled(&self) -> bool {
        self.textures
            .iter()
            .all(|t| !t.failed && t.image.is_some() && primary::pending_texture_cpu(t) == 0)
            && self.world_textures.settled()
    }
}
