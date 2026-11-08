//! Global fixed descriptors, admitted payloads, and physical upload completion.
use super::{
    lighting::{RenderingState, SourceLighting},
    lighting_assets::{CasterDitherBytes, LightingAssets, LightingShaders},
    materials::SourceMaterials,
    reference::ReferenceVisuals,
    world::preflight::{self, SourcePlan, selected::SelectedMetadata},
};
use crate::ownership::{PresentationLedger, RendererCounts};
use crate::{
    ownership::{fixed::FixedImageOwnership, upload_receipt::UploadReceipt},
    reference::{
        AssetId, AssetUsage, ReferenceError, ReferenceManifest, prototype_plan::PrototypePlan,
    },
    world::RendererWorld,
};
use bevy::{
    asset::{LoadState, RenderAssetUsages},
    ecs as bevy_ecs,
    gltf::{Gltf, GltfAssetLabel},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        mesh::{RenderMesh, allocator::MeshAllocator},
        render_asset::{RenderAssets, prepare_assets},
        renderer::{RenderQueue, render_system},
        texture::GpuImage,
    },
};
use std::sync::{Arc, Mutex};
/// Fixed rows that load at once. A chunk load waits several frames, mostly not on the CPU.
const ROW_JOBS: usize = 3;
use crate::reference::fixed_reservation::{FIXED_SHADER_PATHS as SHADERS, shader_input};
type RootState = SelectedMetadata<Handle<SourcePlan>, GltfRead>;
/// The glTF read of a fixed root. The mesh upload follows the read.
enum GltfRead {
    Reading(Handle<Gltf>),
    /// The glTF and its meshes are parsed. The read job and its staging are free.
    Read(Handle<Gltf>),
}
impl GltfRead {
    fn id(&self) -> bevy::asset::AssetId<Gltf> {
        match self {
            Self::Reading(handle) | Self::Read(handle) => handle.id(),
        }
    }
    fn read(&mut self) {
        if let Self::Reading(handle) = self {
            *self = Self::Read(std::mem::take(handle));
        }
    }
}
/// The fixed cloud mesh load. The reservation holds until the mesh uploads.
enum CloudLoad {
    None,
    Loading { mesh: Handle<Mesh> },
    Uploading { fence: Arc<MeshUpload> },
    Done,
    Failed,
}
/// The fixed caster dither load: bytes, then one 3D image upload.
enum DitherLoad {
    None,
    Loading(Handle<CasterDitherBytes>),
    Converted(UploadReceipt),
    Done,
    Failed,
}
struct Root {
    asset: AssetId,
    state: RootState,
    failed: bool,
    meshes: Vec<Handle<Mesh>>,
    fence: Option<Arc<MeshUpload>>,
}
pub(crate) struct MeshUpload {
    meshes: Vec<Handle<Mesh>>,
    pub(crate) receipt: UploadReceipt,
}
#[derive(Default)]
struct Uploads {
    meshes: Vec<Arc<MeshUpload>>,
    images: Vec<(Handle<Image>, UploadReceipt)>,
}
struct Row {
    ownership: FixedImageOwnership,
    chunk: usize,
    load: Option<Handle<Image>>,
    pixels: Vec<u8>,
    image: Option<Handle<Image>>,
    receipt: UploadReceipt,
}
impl Row {
    /// Ends the current chunk load and frees the row pixels.
    fn abandon(&mut self) -> Result<(), ReferenceError> {
        self.ownership.finish_row(true);
        self.load = None;
        self.pixels = Vec::new();
        self.ownership.discard()
    }
}
struct ShaderLoad {
    /// Shaders that this one imports by path. The asset server loads those as dependencies,
    /// so they must finish first.
    imports: Vec<usize>,
    handle: Option<Handle<Shader>>,
    done: bool,
}
#[derive(Resource)]
pub struct SourceFixedAssets {
    roots: Vec<Root>,
    rows: Vec<Row>,
    shaders: Vec<ShaderLoad>,
    uploads: Arc<Mutex<Uploads>>,
    cloud: CloudLoad,
    dither: DitherLoad,
    cookies: Option<Handle<Image>>,
    cookie_receipt: UploadReceipt,
    admitted: bool,
    shaders_bound: bool,
    failure: Option<String>,
}
/// Assets that load as fixed glTF scene roots at startup.
pub(crate) fn scene_root(usage: AssetUsage) -> bool {
    matches!(
        usage,
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
}
impl SourceFixedAssets {
    pub(crate) fn watch_meshes(
        &self,
        meshes: impl Iterator<Item = Handle<Mesh>>,
    ) -> Arc<MeshUpload> {
        let fence = Arc::new(MeshUpload {
            meshes: meshes.collect(),
            receipt: default(),
        });
        let mut uploads = self.uploads.lock().unwrap();
        uploads
            .meshes
            .retain(|f| !f.receipt.completed() || Arc::strong_count(f) > 1);
        uploads.meshes.push(Arc::clone(&fence));
        fence
    }

    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        for path in SHADERS {
            shader_input(path)?;
        }
        let roots = reference
            .assets
            .iter()
            .filter(|a| scene_root(a.usage))
            .map(|a| Root {
                asset: a.id,
                state: RootState::Queued,
                failed: false,
                meshes: Vec::new(),
                fence: None,
            })
            .collect::<Vec<_>>();
        let rows = reference
            .lighting
            .textures
            .iter()
            .map(|t| {
                if t.aniso > 16
                    || t.mip_bias != 0.0
                    || t.filter_mode > 2
                    || t.wrap_u > 2
                    || t.wrap_v > 2
                {
                    return Err(ReferenceError("Unsupported fixed lighting sampler".into()));
                }
                t.validate_chunk_receipts(&reference.file_catalog)?;
                Ok(Row {
                    ownership: FixedImageOwnership::default(),
                    chunk: 0,
                    load: None,
                    pixels: Vec::new(),
                    image: None,
                    receipt: default(),
                })
            })
            .collect::<Result<Vec<_>, ReferenceError>>()?;
        let root_slots = roots
            .len()
            .checked_add(1 + 32)
            .ok_or_else(|| ReferenceError("Fixed root control overflow".into()))?;
        let image_slots = rows
            .len()
            .checked_add(2)
            .ok_or_else(|| ReferenceError("Fixed image control overflow".into()))?;
        let uploads = Arc::new(Mutex::new(Uploads {
            meshes: Vec::with_capacity(root_slots),
            images: Vec::with_capacity(image_slots),
        }));
        Ok(Self {
            roots,
            rows,
            shaders: SHADERS
                .iter()
                .map(|path| ShaderLoad {
                    imports: path_imports(path),
                    handle: None,
                    done: false,
                })
                .collect(),
            uploads,
            cloud: CloudLoad::None,
            dither: DitherLoad::None,
            cookies: None,
            cookie_receipt: default(),
            admitted: false,
            shaders_bound: false,
            failure: None,
        })
    }
    pub fn plan(&self, asset: AssetId) -> Option<&PrototypePlan> {
        self.roots
            .iter()
            .find(|r| r.asset == asset)
            .and_then(|r| r.state.plan())
    }
    pub(crate) fn pipeline_inputs_ready(&self) -> bool {
        self.shaders_bound && self.cookie_receipt.completed()
    }
    pub(crate) fn startup_audit(&self) -> serde_json::Value {
        serde_json::json!({
            "descriptors_ready": self.descriptors_ready(),
            "payloads_ready": self.payloads_ready(),
            "roots_loading": self.roots.iter().filter(|r| matches!(r.state, RootState::Loading { .. })).count(),
            "rows_pending": self.rows.iter().filter(|r| !r.receipt.completed()).count(),
            "shaders_pending": self.shaders.iter().zip(SHADERS).filter_map(|(s, path)| (!s.done).then_some(path)).collect::<Vec<_>>(),
        })
    }
    pub fn descriptors_ready(&self) -> bool {
        self.roots.iter().all(|r| r.state.plan().is_some())
    }
    /// Fixed models, image rows, shaders and the three single uploads: (ready, total).
    pub fn progress(&self) -> (u32, u32) {
        let roots = self
            .roots
            .iter()
            .filter(|r| matches!(r.state, RootState::Ready(_)))
            .count();
        let rows = self
            .rows
            .iter()
            .filter(|r| r.image.is_some() && r.receipt.completed())
            .count();
        let shaders = self.shaders.iter().filter(|s| s.done).count();
        let singles = [
            matches!(self.cloud, CloudLoad::Done),
            matches!(self.dither, DitherLoad::Done),
            self.cookie_receipt.completed(),
        ];
        let ready = roots + rows + shaders + singles.iter().filter(|done| **done).count();
        let total = self.roots.len() + self.rows.len() + self.shaders.len() + singles.len();
        (ready as u32, total as u32)
    }
    pub fn payloads_ready(&self) -> bool {
        self.admitted
            && self.failure.is_none()
            && self
                .roots
                .iter()
                .all(|r| matches!(r.state, RootState::Ready(_)))
            && self
                .rows
                .iter()
                .all(|r| r.image.is_some() && r.receipt.completed())
            && matches!(self.cloud, CloudLoad::Done)
            && matches!(self.dither, DitherLoad::Done)
            && self.cookie_receipt.completed()
            && self.shaders_bound
    }
    fn allocation(&self) -> RendererCounts {
        let mut a = RendererCounts::default();
        let mut add = |jobs: usize, pending: usize| {
            a.jobs += jobs;
            a.pending += pending;
        };
        for r in &self.roots {
            if r.failed {
                continue;
            }
            match r.state {
                RootState::PlanLoading(_) => add(1, 1),
                RootState::Loading {
                    asset: GltfRead::Reading(_),
                    ..
                } => add(1, 1),
                RootState::Loading {
                    asset: GltfRead::Read(_),
                    ..
                } => add(0, 1),
                _ => {}
            }
        }
        for row in &self.rows {
            let (jobs, pending) = row.ownership.io();
            add(jobs, pending);
        }
        for shader in &self.shaders {
            let active = shader.handle.is_some() && !shader.done;
            add(usize::from(active), usize::from(active));
        }
        match &self.cloud {
            CloudLoad::Loading { .. } => add(1, 1),
            CloudLoad::Uploading { .. } => add(0, 1),
            CloudLoad::None | CloudLoad::Done | CloudLoad::Failed => {}
        }
        match &self.dither {
            DitherLoad::Loading(_) => add(1, 1),
            DitherLoad::Converted(_) => add(0, 1),
            DitherLoad::None | DitherLoad::Done | DitherLoad::Failed => {}
        }
        if self.cookies.is_some() {
            add(0, usize::from(!self.cookie_receipt.completed()));
        }
        a
    }
    pub(crate) fn account(
        &self,
        ledger: &mut PresentationLedger,
        snapshot: &mut RendererCounts,
    ) -> Result<(), ReferenceError> {
        let next = self.allocation();
        next.apply(*snapshot, ledger)?;
        *snapshot = next;
        Ok(())
    }
    /// Accounts the current state plus one starting load.
    fn account_job(
        &self,
        ledger: &mut PresentationLedger,
        snapshot: &mut RendererCounts,
    ) -> Result<(), ReferenceError> {
        let mut next = self.allocation();
        next.jobs += 1;
        next.pending += 1;
        next.apply(*snapshot, ledger)?;
        *snapshot = next;
        Ok(())
    }
    pub(crate) fn prepare_metadata(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        snapshot: &mut RendererCounts,
        reference: &ReferenceManifest,
        server: &AssetServer,
        plans: &Assets<SourcePlan>,
    ) -> Result<(), ReferenceError> {
        self.account(ledger, snapshot)?;
        for index in 0..self.roots.len() {
            if let RootState::PlanLoading(handle) = &self.roots[index].state {
                match preflight::poll_plan(server, plans, handle) {
                    Ok(Some(plan)) => {
                        self.roots[index].state.complete_metadata(plan);
                    }
                    Err(e) => {
                        self.roots[index].failed = true;
                        self.failure.get_or_insert(e.0);
                    }
                    _ => {}
                }
            }
        }
        self.account(ledger, snapshot)?;
        if self.failure.is_some() || stream.frame().failure.is_some() {
            return Ok(());
        }
        for index in 0..self.roots.len() {
            if !matches!(self.roots[index].state, RootState::Queued) {
                continue;
            }
            let asset = reference
                .assets
                .iter()
                .find(|a| a.id == self.roots[index].asset)
                .unwrap();
            if !crate::ownership::fixed::fixed_room(ledger, true, true) {
                break;
            }
            self.account_job(ledger, snapshot)?;
            self.roots[index].state = RootState::PlanLoading(preflight::load_plan(
                server,
                &reference.file_catalog,
                &asset.path,
            )?);
        }
        Ok(())
    }
    pub(crate) fn poll(
        &mut self,
        ledger: &mut PresentationLedger,
        snapshot: &mut RendererCounts,
        reference: &ReferenceManifest,
        server: &AssetServer,
        gltfs: &Assets<Gltf>,
        gltf_meshes: &Assets<bevy::gltf::GltfMesh>,
        shaders: &Assets<Shader>,
        meshes: &Assets<Mesh>,
        images: &mut Assets<Image>,
        dither: &mut Assets<CasterDitherBytes>,
        lighting: &mut SourceLighting,
        render_lighting: &mut LightingAssets,
    ) -> Result<(), ReferenceError> {
        for index in 0..self.roots.len() {
            let root = &mut self.roots[index];
            let RootState::Loading { asset, .. } = &root.state else {
                continue;
            };
            if root.failed {
                continue;
            }
            let id = asset.id();
            if let LoadState::Failed(error) = server.load_state(id) {
                root.failed = true;
                let path = &reference
                    .assets
                    .iter()
                    .find(|a| a.id == root.asset)
                    .unwrap()
                    .path;
                lighting.release_failed_capture(path);
                self.failure
                    .get_or_insert(format!("Fixed glTF {path}: {error}"));
                continue;
            }
            if let Some((_, _, bevy::asset::RecursiveDependencyLoadState::Failed(error))) =
                server.get_load_states(id)
            {
                self.failure
                    .get_or_insert(format!("Fixed glTF dependency: {error}"));
            }
            if let Some(gltf) = gltfs.get(id) {
                if let RootState::Loading { asset, .. } = &mut root.state {
                    asset.read();
                }
                if root.fence.is_none() {
                    for handle in &gltf.meshes {
                        let Some(mesh) = gltf_meshes.get(handle) else {
                            continue;
                        };
                        for p in &mesh.primitives {
                            if !root.meshes.iter().any(|h| h.id() == p.mesh.id()) {
                                root.meshes.push(p.mesh.clone());
                            }
                        }
                    }
                    if root.meshes.len() != root.state.plan().unwrap().primitives.len() {
                        continue;
                    }
                    let fence = Arc::new(MeshUpload {
                        meshes: std::mem::take(&mut root.meshes),
                        receipt: default(),
                    });
                    self.uploads.lock().unwrap().meshes.push(Arc::clone(&fence));
                    root.fence = Some(fence);
                }
                if root.fence.as_ref().unwrap().receipt.completed() {
                    root.state.complete_payload();
                }
            }
        }
        for (index, row) in self.rows.iter_mut().enumerate() {
            if self.failure.is_some() && row.image.is_none() {
                row.ownership.cancel();
                if row.load.is_none() {
                    row.pixels = Vec::new();
                    row.ownership.discard()?;
                    continue;
                }
            }
            if row.receipt.completed() && row.image.is_some() {
                row.ownership.uploaded()?;
                continue;
            }
            let Some(handle) = &row.load else { continue };
            if let LoadState::Failed(error) = server.load_state(handle.id()) {
                row.ownership.cancel();
                row.abandon()?;
                self.failure.get_or_insert(format!("Fixed row: {error}"));
                continue;
            }
            let Some(image) = images.get(handle) else {
                continue;
            };
            if self.failure.is_some() {
                row.abandon()?;
                continue;
            }
            let record = &reference.lighting.textures[index];
            let chunk = &record.chunks[row.chunk];
            if image.width() != chunk.width
                || image.height() != chunk.rows
                || image.texture_descriptor.mip_level_count != 1
                || image.texture_descriptor.format
                    != super::textures::texture_format(record.format, record.color_space != 0)
            {
                self.failure
                    .get_or_insert("Fixed row decoded image contract".into());
                row.ownership.cancel();
                row.abandon()?;
                continue;
            }
            if let Err(error) = record.copy_chunk(
                row.chunk,
                image
                    .data
                    .as_deref()
                    .ok_or_else(|| ReferenceError("Fixed row pixels missing".into()))?,
                &mut row.pixels,
            ) {
                self.failure.get_or_insert(error.0);
                row.ownership.cancel();
                row.abandon()?;
                continue;
            }
            row.load = None;
            row.chunk += 1;
            row.ownership.finish_row(true);
            if row.chunk == record.chunks.len() {
                row.ownership.assemble()?;
                let settings = super::textures::SourceImageSettings {
                    srgb: record.color_space != 0,
                    filter: record.filter_mode,
                    aniso: record.aniso as u16,
                    wrap_u: record.wrap_u,
                    wrap_v: record.wrap_v,
                    main_world_only: false,
                    decoded_bytes: row.pixels.len(),
                    format: record.format,
                };
                let image = images.add(super::textures::source_image(
                    record.width,
                    record.height,
                    record.mip_count,
                    std::mem::take(&mut row.pixels),
                    &settings,
                ));
                self.uploads
                    .lock()
                    .unwrap()
                    .images
                    .push((image.clone(), row.receipt.clone()));
                lighting.textures.push((
                    record.id,
                    format!("{}:{}", record.source.file, record.source.path_id),
                    image.clone(),
                ));
                row.image = Some(image);
            }
        }
        for shader in &mut self.shaders {
            if shader.done {
                continue;
            }
            if let Some(handle) = &shader.handle {
                if let LoadState::Failed(error) = server.load_state(handle.id()) {
                    self.failure.get_or_insert(format!("Fixed shader: {error}"));
                    shader.done = true;
                } else if server.is_loaded_with_dependencies(handle.id())
                    && shaders.contains(handle)
                {
                    shader.done = true;
                }
            }
        }
        if let CloudLoad::Loading { mesh } = &self.cloud
            && let LoadState::Failed(error) = server.load_state(mesh.id())
        {
            self.failure.get_or_insert(format!("Fixed cloud: {error}"));
            self.cloud = CloudLoad::Failed;
        }
        if let CloudLoad::Loading { mesh } = &self.cloud
            && meshes.contains(mesh.id())
        {
            let fence = Arc::new(MeshUpload {
                meshes: vec![mesh.clone()],
                receipt: default(),
            });
            self.uploads.lock().unwrap().meshes.push(Arc::clone(&fence));
            self.cloud = CloudLoad::Uploading { fence };
        }
        if let CloudLoad::Uploading { fence } = &self.cloud
            && fence.receipt.completed()
        {
            self.cloud = CloudLoad::Done;
        }
        if let DitherLoad::Loading(handle) = &self.dither {
            if let LoadState::Failed(error) = server.load_state(handle.id()) {
                self.failure.get_or_insert(format!("Fixed dither: {error}"));
                self.dither = DitherLoad::Failed;
            } else if let Some(data) = dither.remove(handle.id()) {
                let mut image = Image::new(
                    Extent3d {
                        width: 4,
                        height: 4,
                        depth_or_array_layers: 16,
                    },
                    TextureDimension::D3,
                    data.0,
                    TextureFormat::R8Unorm,
                    RenderAssetUsages::RENDER_WORLD,
                );
                image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    address_mode_w: ImageAddressMode::Repeat,
                    mag_filter: ImageFilterMode::Nearest,
                    min_filter: ImageFilterMode::Nearest,
                    mipmap_filter: ImageFilterMode::Nearest,
                    lod_max_clamp: 0.,
                    ..default()
                });
                let handle = images.add(image);
                let receipt = UploadReceipt::default();
                self.uploads
                    .lock()
                    .unwrap()
                    .images
                    .push((handle.clone(), receipt.clone()));
                render_lighting.caster_dither = Some(handle);
                self.dither = DitherLoad::Converted(receipt);
            }
        }
        if let DitherLoad::Converted(receipt) = &self.dither
            && receipt.completed()
        {
            self.dither = DitherLoad::Done;
        }
        self.account(ledger, snapshot)?;
        if let Some(error) = &self.failure {
            lighting.state = RenderingState::Failed(error.clone());
            return Err(ReferenceError(error.clone()));
        }
        Ok(())
    }
    /// The caller owns the descriptor-to-payload decision. Task 2B adds its combined check here.
    pub(crate) fn admit_payloads(
        &mut self,
        ledger: &mut PresentationLedger,
        snapshot: &mut RendererCounts,
        reference: &ReferenceManifest,
        server: &AssetServer,
        visuals: &mut ReferenceVisuals,
        materials: &mut SourceMaterials,
        lighting: &mut SourceLighting,
        assets: &mut LightingAssets,
        images: &mut Assets<Image>,
    ) -> Result<bool, ReferenceError> {
        if !self.descriptors_ready() || self.failure.is_some() {
            return Ok(false);
        }
        self.admitted = true;
        materials.fixed_admitted = true;
        for index in 0..self.roots.len() {
            if !matches!(self.roots[index].state, RootState::Planned(_)) {
                continue;
            }
            let root = &self.roots[index];
            let plan = root.state.plan().unwrap();
            if !crate::ownership::fixed::fixed_room(ledger, true, true) {
                continue;
            }
            let count = plan.primitives.len();
            let record = reference
                .assets
                .iter()
                .find(|a| a.id == root.asset)
                .unwrap();
            self.account_job(ledger, snapshot)?;
            self.roots[index].meshes.reserve_exact(count);
            let handle = server
                .load_builder()
                .with_settings(super::reference::source_gltf_settings)
                .load(record.path.clone());
            visuals.handles.push((record.path.clone(), handle.clone()));
            visuals.gltfs.push((record.id, handle.clone()));
            self.roots[index]
                .state
                .start_payload(GltfRead::Reading(handle));
        }
        // A shader starts when the shaders it imports by path are done.
        for index in 0..self.shaders.len() {
            let shader = &self.shaders[index];
            if shader.handle.is_some() || shader.imports.iter().any(|&i| !self.shaders[i].done) {
                continue;
            }
            if !crate::ownership::fixed::fixed_room(ledger, true, true) {
                break;
            }
            self.account_job(ledger, snapshot)?;
            self.shaders[index].handle = Some(server.load(SHADERS[index]));
        }
        if self.shaders.iter().all(|s| s.done) && !self.shaders_bound {
            let shader = |path: &str| {
                self.shaders[SHADERS.iter().position(|p| *p == path).unwrap()]
                    .handle
                    .as_ref()
                    .unwrap()
                    .clone()
            };
            materials.shaders = [
                shader("shaders/source_surface.wgsl"),
                shader("shaders/source_mesh.wgsl"),
                shader("shaders/source_glass.wgsl"),
            ];
            lighting.shader = shader("shaders/source_surface.wgsl");
            lighting.sky.shader = shader("shaders/source_sky.wgsl");
            assets.shaders = LightingShaders {
                lighting: shader("shaders/source_view_lighting.wgsl"),
                cloud: shader("shaders/source_cloud.wgsl"),
                sky: shader("shaders/source_render_sky.wgsl"),
                float_store: shader("shaders/source_float_store.wgsl"),
                glass: shader("shaders/source_glass.wgsl"),
                copy: shader("shaders/source_copy.wgsl"),
                dof: shader("shaders/source_dof.wgsl"),
                histogram: shader("shaders/source_histogram.wgsl"),
                exposure: shader("shaders/source_exposure.wgsl"),
                probe: shader("shaders/source_probe.wgsl"),
                caster: shader("shaders/source_caster.wgsl"),
                caster_math: shader("shaders/source_caster_math.wgsl"),
                bloom: shader("shaders/source_bloom.wgsl"),
                lut: shader("shaders/source_lut.wgsl"),
                lut_ldr: shader("shaders/source_lut_ldr.wgsl"),
                uber: shader("shaders/source_uber.wgsl"),
                grain: shader("shaders/source_grain.wgsl"),
                ao_depth: shader("shaders/source_ao_depth.wgsl"),
                ao_render: shader("shaders/source_ao_render.wgsl"),
                ao_upsample: shader("shaders/source_ao_upsample.wgsl"),
                ao_composite: shader("shaders/source_ao_composite.wgsl"),
                depth_resolve: shader("shaders/source_depth_resolve.wgsl"),
                props: shader("shaders/source_props.wgsl"),
                props_cull: shader("shaders/source_props_cull.wgsl"),
                motion: shader("shaders/source_motion.wgsl"),
                taa: shader("shaders/source_taa.wgsl"),
                rain: shader("shaders/weather_rain.wgsl"),
                bolt: shader("shaders/weather_bolt.wgsl"),
                shafts: shader("shaders/weather_shafts.wgsl"),
                spray: shader("shaders/weather_spray.wgsl"),
            };
            self.shaders_bound = true;
        }
        for index in 0..self.rows.len() {
            if self.rows.iter().filter(|r| r.load.is_some()).count() >= ROW_JOBS {
                break;
            }
            let row = &self.rows[index];
            if row.load.is_some() || row.image.is_some() {
                continue;
            }
            let record = &reference.lighting.textures[index];
            let chunk = &record.chunks[row.chunk];
            let first = row.chunk == 0;
            let mut ownership = row.ownership;
            if !ownership.begin_row()? {
                continue;
            }
            if !crate::ownership::fixed::fixed_room(ledger, true, first) {
                continue;
            }
            self.rows[index].ownership = ownership;
            self.account(ledger, snapshot)?;
            if first {
                self.rows[index].pixels = vec![0; record.validate_chunks()?];
            }
            let (srgb, filter, aniso, wrap_u, wrap_v, format) = (
                record.color_space != 0,
                record.filter_mode,
                record.aniso as u16,
                record.wrap_u,
                record.wrap_v,
                record.format,
            );
            let decoded_bytes = record.chunk_range(self.rows[index].chunk)?.len();
            self.rows[index].load = Some(
                server
                    .load_builder()
                    .with_settings(move |s: &mut super::textures::SourceImageSettings| {
                        *s = super::textures::SourceImageSettings {
                            srgb,
                            filter,
                            aniso,
                            wrap_u,
                            wrap_v,
                            main_world_only: true,
                            decoded_bytes,
                            format,
                        };
                    })
                    .load(chunk.data.path.clone()),
            );
        }
        if matches!(self.cloud, CloudLoad::None) {
            if crate::ownership::fixed::fixed_room(ledger, true, true) {
                let mesh = server
                    .load_builder()
                    .with_settings(super::reference::source_gltf_settings)
                    .load(
                        GltfAssetLabel::Primitive {
                            mesh: 0,
                            primitive: 0,
                        }
                        .from_asset(reference.render_lighting.sky.cloud_mesh.file.clone()),
                    );
                assets.cloud_mesh = mesh.clone();
                self.cloud = CloudLoad::Loading { mesh };
                self.account(ledger, snapshot)?;
            }
        }
        if matches!(self.dither, DitherLoad::None) {
            let path = &reference.render_lighting.caster_dither.file;
            if reference.file_catalog.file(path)?.bytes != 256 {
                return Err(ReferenceError("Fixed dither receipt size".into()));
            }
            if crate::ownership::fixed::fixed_room(ledger, true, true) {
                let handle = server.load(path.clone());
                assets.dither_bytes = handle.clone();
                self.dither = DitherLoad::Loading(handle);
                self.account(ledger, snapshot)?;
            }
        }
        if self.cookies.is_none() && self.rows.iter().all(|r| r.image.is_some()) {
            let ids = super::lighting::cookie_ids(reference);
            if crate::ownership::fixed::fixed_room(ledger, false, true) {
                let handle = |id| {
                    lighting
                        .textures
                        .iter()
                        .find(|(i, _, _)| *i == id)
                        .unwrap()
                        .2
                        .clone()
                };
                let originals = ids[2..].iter().map(|id| handle(*id)).collect::<Vec<_>>();
                let originals = originals.iter().collect::<Vec<_>>();
                let array = super::lighting::pack_custom_cookies(
                    images,
                    &originals,
                    // Half texture quality drops the top cookie mip, as Unity `masterTextureLimit` 1.
                    if crate::display_settings::half_textures() {
                        256
                    } else {
                        512
                    },
                )
                .map_err(ReferenceError)?;
                lighting.cookie_handles = Some([handle(ids[0]), handle(ids[1]), array.clone()]);
                self.uploads
                    .lock()
                    .unwrap()
                    .images
                    .push((array.clone(), self.cookie_receipt.clone()));
                self.cookies = Some(array);
                self.account(ledger, snapshot)?;
            }
        }
        self.account(ledger, snapshot)?;
        Ok(true)
    }
}
/// Render consumers borrow only the completed handles from the one admitted input owner.
#[derive(Resource, Default)]
pub(crate) struct AdmittedShaders(Option<[Handle<Shader>; SHADERS.len()]>);
impl AdmittedShaders {
    pub(crate) fn get(&self, path: &str) -> Option<&Handle<Shader>> {
        self.0
            .as_ref()
            .and_then(|handles| SHADERS.iter().position(|p| *p == path).map(|i| &handles[i]))
    }
}
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub(crate) struct FixedShaderExtraction;
#[derive(Resource, Default)]
struct ExtractedFixedUploads(Option<Arc<Mutex<Uploads>>>);
pub struct FixedAssetsPlugin;
impl Plugin for FixedAssetsPlugin {
    fn build(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<ExtractedFixedUploads>()
                .init_resource::<AdmittedShaders>()
                .add_systems(
                    ExtractSchedule,
                    extract_uploads.in_set(FixedShaderExtraction),
                )
                .add_systems(
                    Render,
                    complete_uploads
                        .in_set(RenderSystems::Cleanup)
                        .after(prepare_assets::<RenderMesh>)
                        .after(render_system),
                );
        }
    }
}
fn extract_uploads(
    source: Extract<Option<Res<SourceFixedAssets>>>,
    mut extracted: ResMut<ExtractedFixedUploads>,
    mut shaders: ResMut<AdmittedShaders>,
) {
    if shaders.0.is_none()
        && let Some(source) = source.as_ref().filter(|s| s.shaders_bound)
    {
        shaders.0 = Some(std::array::from_fn(|i| {
            source.shaders[i].handle.as_ref().unwrap().clone()
        }));
    }
    if extracted.0.is_none()
        && let Some(source) = source.as_ref()
    {
        extracted.0 = Some(Arc::clone(&source.uploads));
    }
}
fn complete_uploads(
    uploads: Res<ExtractedFixedUploads>,
    meshes: Res<RenderAssets<RenderMesh>>,
    allocator: Res<MeshAllocator>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
) {
    let Some(uploads) = &uploads.0 else { return };
    let mut uploads = uploads.lock().unwrap();
    uploads
        .meshes
        .retain(|f| !f.receipt.completed() || Arc::strong_count(f) > 1);
    for fence in &uploads.meshes {
        if fence.meshes.iter().all(|h| {
            meshes.get(h.id()).is_some_and(|mesh| {
                allocator.mesh_vertex_slice(&h.id()).is_some()
                    && (!mesh.indexed() || allocator.mesh_index_slice(&h.id()).is_some())
            })
        }) && fence.receipt.register()
        {
            let receipt = fence.receipt.clone();
            queue.on_submitted_work_done(move || receipt.complete());
        }
    }
    for (handle, receipt) in &uploads.images {
        if images.get(handle.id()).is_some() && receipt.register() {
            let receipt = receipt.clone();
            queue.on_submitted_work_done(move || receipt.complete());
        }
    }
}

/// Fixed shaders that `path` imports with `#import "shaders/..."`.
fn path_imports(path: &str) -> Vec<usize> {
    let source = crate::reference::fixed_reservation::shader_source(path).unwrap();
    source
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("#import \""))
        .filter_map(|rest| rest.split('"').next())
        .map(|import| {
            SHADERS
                .iter()
                .position(|p| *p == import)
                .expect("a path import names a fixed shader")
        })
        .collect()
}
