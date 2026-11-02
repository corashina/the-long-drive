//! Global fixed descriptors, admitted payloads, and physical upload completion.
use super::{
    lighting::{RenderingState, SourceLighting},
    lighting_assets::{CasterDitherBytes, LightingAssets, LightingShaders},
    materials::SourceMaterials,
    reference::ReferenceVisuals,
    world::preflight::{
        self, SourcePlan,
        selected::{Reservation, SelectedMetadata, retained_plan_bytes},
    },
};
use crate::ownership::PresentationLedger;
use crate::{
    ownership::{
        fixed::{FixedAllocation, FixedImageOwnership},
        upload_receipt::UploadReceipt,
    },
    reference::{
        AssetId, AssetUsage, ReferenceError, ReferenceManifest,
        fixed_reservation::FixedReservation, prototype_plan::PrototypePlan,
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
const ROW_SCRATCH: usize = 5 * (1024 * 1024 + 16);
use crate::reference::fixed_reservation::{FIXED_SHADER_PATHS as SHADERS, shader_input};
type RootState = SelectedMetadata<Handle<SourcePlan>, Handle<Gltf>>;
struct Root {
    asset: AssetId,
    state: RootState,
    cpu: usize,
    gpu: usize,
    staging: usize,
    input_done: bool,
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
struct ShaderLoad {
    handle: Option<Handle<Shader>>,
    cpu: usize,
    staging: usize,
    done: bool,
}
#[derive(Resource)]
pub struct SourceFixedAssets {
    roots: Vec<Root>,
    rows: Vec<Row>,
    shaders: Vec<ShaderLoad>,
    uploads: Arc<Mutex<Uploads>>,
    cloud: Option<Handle<Mesh>>,
    cloud_bound: Option<FixedReservation>,
    cloud_fence: Option<Arc<MeshUpload>>,
    cloud_done: bool,
    cloud_retained: usize,
    dither: Option<Handle<CasterDitherBytes>>,
    dither_receipt: UploadReceipt,
    dither_done: bool,
    dither_failed: bool,
    dither_converted: bool,
    cookies: Option<Handle<Image>>,
    cookie_receipt: UploadReceipt,
    cookie_bytes: usize,
    control: usize,
    admitted: bool,
    shaders_bound: bool,
    failure: Option<String>,
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
            .filter(|a| {
                matches!(
                    a.usage,
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
            .map(|a| Root {
                asset: a.id,
                state: RootState::Queued,
                cpu: 0,
                gpu: 0,
                staging: 0,
                input_done: false,
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
                if t.aniso > 1
                    || t.mip_bias != 0.0
                    || t.filter_mode > 2
                    || t.wrap_u > 2
                    || t.wrap_v > 2
                {
                    return Err(ReferenceError("Unsupported fixed lighting sampler".into()));
                }
                Ok(Row {
                    ownership: FixedImageOwnership::new(
                        t.validate_chunk_receipts(&reference.file_catalog)?,
                        true,
                        ROW_SCRATCH,
                    )?,
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
        let product = |n: usize, size: usize| {
            n.checked_mul(size)
                .ok_or_else(|| ReferenceError("Fixed control capacity overflow".into()))
        };
        let source_strings =
            reference
                .render_lighting
                .textures
                .iter()
                .try_fold(0usize, |n, t| {
                    n.checked_add(t.source.file.len())
                        .ok_or_else(|| ReferenceError("Fixed control strings overflow".into()))
                })?;
        let control = [
            super::readiness::control_bytes(),
            std::mem::size_of::<Self>(),
            std::mem::size_of::<AdmittedShaders>(),
            product(roots.capacity(), std::mem::size_of::<Root>())?,
            product(roots.len(), RootState::ready_control_bytes())?,
            product(rows.capacity(), std::mem::size_of::<Row>())?,
            product(SHADERS.len(), std::mem::size_of::<ShaderLoad>())?,
            product(root_slots, std::mem::size_of::<Arc<MeshUpload>>() + 64)?,
            product(
                image_slots,
                std::mem::size_of::<(Handle<Image>, UploadReceipt)>() + 64,
            )?,
            product(image_slots, 4096)?,
            product(
                reference
                    .render_lighting
                    .textures
                    .len()
                    .checked_next_power_of_two()
                    .ok_or_else(|| ReferenceError("Fixed map capacity overflow".into()))?,
                4 * (std::mem::size_of::<(crate::reference::SourceId, Handle<Image>)>() + 1),
            )?,
            product(source_strings, 2)?,
            product(
                reference.render_lighting.textures.len(),
                std::mem::size_of::<Option<Handle<Image>>>(),
            )?,
        ]
        .into_iter()
        .try_fold(0usize, |sum, n| {
            sum.checked_add(n)
                .ok_or_else(|| ReferenceError("Fixed control total overflow".into()))
        })?;
        Ok(Self {
            roots,
            rows,
            shaders: SHADERS
                .iter()
                .map(|_| ShaderLoad {
                    handle: None,
                    cpu: 0,
                    staging: 0,
                    done: false,
                })
                .collect(),
            uploads,
            cloud: None,
            cloud_bound: None,
            cloud_fence: None,
            cloud_done: false,
            cloud_retained: 0,
            dither: None,
            dither_receipt: default(),
            dither_done: false,
            dither_failed: false,
            dither_converted: false,
            cookies: None,
            cookie_receipt: default(),
            cookie_bytes: 0,
            control,
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
            && self.cloud_done
            && self.dither_done
            && self.cookie_receipt.completed()
            && self.shaders_bound
    }
    fn allocation(&self) -> Result<FixedAllocation, ReferenceError> {
        let mut a = FixedAllocation {
            cpu: self.control,
            ..default()
        };
        let mut add = |cpu: usize,
                       gpu: usize,
                       jobs: usize,
                       pending: usize,
                       staging: usize|
         -> Result<(), ReferenceError> {
            let checked = |a: usize, b: usize| {
                a.checked_add(b)
                    .ok_or_else(|| ReferenceError("Fixed ownership total overflow".into()))
            };
            a.cpu = checked(a.cpu, cpu)?;
            a.gpu = checked(a.gpu, gpu)?;
            a.jobs = checked(a.jobs, jobs)?;
            a.pending = checked(a.pending, pending)?;
            a.staging = checked(a.staging, staging)?;
            Ok(())
        };
        for r in &self.roots {
            let (jobs, pending, staging) = if r.failed {
                (0, 0, 0)
            } else {
                match r.state {
                    RootState::PlanLoading(_) => (1, 1, r.staging),
                    RootState::Loading { .. } => (
                        usize::from(!r.input_done),
                        1,
                        if r.input_done { 0 } else { r.staging },
                    ),
                    _ => (0, 0, 0),
                }
            };
            add(r.cpu, r.gpu, jobs, pending, staging)?;
        }
        for row in &self.rows {
            let (cpu, gpu) = row.ownership.bytes();
            let (jobs, pending, staging) = row.ownership.io();
            add(cpu, gpu, jobs, pending, staging)?;
        }
        for shader in &self.shaders {
            let active = shader.handle.is_some() && !shader.done;
            add(
                shader.cpu,
                0,
                usize::from(active),
                usize::from(active),
                if active { shader.staging } else { 0 },
            )?;
        }
        if let Some(bound) = self.cloud_bound {
            add(
                if self.cloud_done {
                    self.cloud_retained
                } else {
                    bound.cpu
                },
                bound.gpu,
                usize::from(!self.cloud_done && self.cloud_fence.is_none()),
                usize::from(!self.cloud_done),
                if !self.cloud_done && self.cloud_fence.is_none() {
                    bound.staging
                } else {
                    0
                },
            )?;
        }
        if self.dither.is_some() && !self.dither_failed {
            add(
                if self.dither_receipt.completed() {
                    0
                } else {
                    1536
                },
                256,
                usize::from(!self.dither_done && !self.dither_converted),
                usize::from(!self.dither_done),
                if !self.dither_done && !self.dither_converted {
                    512
                } else {
                    0
                },
            )?;
        }
        if self.cookies.is_some() {
            add(
                self.cookie_bytes
                    .checked_mul(if self.cookie_receipt.completed() {
                        1
                    } else {
                        2
                    })
                    .ok_or_else(|| ReferenceError("Cookie ownership overflow".into()))?,
                self.cookie_bytes,
                0,
                usize::from(!self.cookie_receipt.completed()),
                0,
            )?;
        }
        Ok(a)
    }
    pub(crate) fn account(
        &self,
        ledger: &mut PresentationLedger,
        snapshot: &mut FixedAllocation,
    ) -> Result<(), ReferenceError> {
        let next = self.allocation()?;
        ledger.set_allocation(next.replace(*snapshot, ledger.allocation())?)?;
        *snapshot = next;
        Ok(())
    }
    pub(crate) fn prepare_metadata(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        snapshot: &mut FixedAllocation,
        reference: &ReferenceManifest,
        server: &AssetServer,
        plans: &Assets<SourcePlan>,
    ) -> Result<(), ReferenceError> {
        self.account(ledger, snapshot)?;
        for index in 0..self.roots.len() {
            if let RootState::PlanLoading(handle) = &self.roots[index].state {
                match preflight::poll_plan(server, plans, handle) {
                    Ok(Some(plan)) => {
                        self.roots[index].cpu = retained_plan_bytes(&plan);
                        self.roots[index].staging = 0;
                        self.roots[index].state.complete_metadata(plan);
                    }
                    Err(e) => {
                        self.roots[index].failed = true;
                        self.roots[index].cpu = 0;
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
            let bound = Reservation::metadata(&reference.file_catalog, &asset.path)?;
            if !crate::ownership::fixed::fixed_room(ledger, bound.cpu, 0, bound.staging, true)? {
                break;
            }
            self.roots[index].cpu = bound.cpu;
            self.roots[index].staging = bound.staging;
            let mut next = self.allocation()?;
            next.jobs = checked_sum(&[next.jobs, 1])?;
            next.pending = checked_sum(&[next.pending, 1])?;
            next.staging = checked_sum(&[next.staging, bound.staging])?;
            ledger.set_allocation(next.replace(*snapshot, ledger.allocation())?)?;
            *snapshot = next;
            self.roots[index].state = RootState::PlanLoading(preflight::load_plan(
                server,
                &reference.file_catalog,
                &asset.path,
                bound.cpu,
                bound.staging,
            )?);
        }
        Ok(())
    }
    pub(crate) fn poll(
        &mut self,
        ledger: &mut PresentationLedger,
        snapshot: &mut FixedAllocation,
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
            if let LoadState::Failed(error) = server.load_state(asset.id()) {
                root.failed = true;
                root.input_done = true;
                root.staging = 0;
                root.cpu = retained_plan_bytes(root.state.plan().unwrap());
                root.gpu = 0;
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
                server.get_load_states(asset.id())
            {
                self.failure
                    .get_or_insert(format!("Fixed glTF dependency: {error}"));
            }
            if let Some(gltf) = gltfs.get(asset) {
                root.input_done = true;
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
                    let mut cpu = checked_sum(&[
                        retained_plan_bytes(root.state.plan().unwrap()),
                        root.state.plan().unwrap().scene_bytes,
                    ])?;
                    let mut gpu = 0;
                    for handle in &root.fence.as_ref().unwrap().meshes {
                        let mesh = meshes.get(handle).ok_or_else(|| {
                            ReferenceError("Fixed MAIN_WORLD mesh missing".into())
                        })?;
                        cpu = checked_sum(&[
                            cpu,
                            crate::presentation::world_metrics::mesh_payload(mesh),
                            4096,
                        ])?;
                        gpu = checked_sum(&[
                            gpu,
                            mesh.get_vertex_buffer_size(),
                            mesh.get_index_buffer_bytes().map_or(0, <[u8]>::len),
                            8,
                        ])?;
                    }
                    if cpu > root.cpu || gpu > root.gpu {
                        return Err(ReferenceError(
                            "Fixed mesh exceeded admitted construction bound".into(),
                        ));
                    }
                    root.cpu = cpu;
                    root.gpu = gpu;
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
                row.ownership.finish_row(true);
                row.load = None;
                row.pixels = Vec::new();
                row.ownership.discard()?;
                self.failure.get_or_insert(format!("Fixed row: {error}"));
                continue;
            }
            let Some(image) = images.get(handle) else {
                continue;
            };
            if self.failure.is_some() {
                row.ownership.finish_row(true);
                row.load = None;
                row.pixels = Vec::new();
                row.ownership.discard()?;
                continue;
            }
            let record = &reference.lighting.textures[index];
            let chunk = &record.chunks[row.chunk];
            if image.width() != chunk.width
                || image.height() != chunk.rows
                || image.texture_descriptor.mip_level_count != 1
                || image.texture_descriptor.format
                    != if record.color_space != 0 {
                        TextureFormat::Rgba8UnormSrgb
                    } else {
                        TextureFormat::Rgba8Unorm
                    }
            {
                self.failure
                    .get_or_insert("Fixed row decoded image contract".into());
                row.ownership.cancel();
                row.ownership.finish_row(true);
                row.load = None;
                row.pixels = Vec::new();
                row.ownership.discard()?;
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
                row.ownership.finish_row(true);
                row.load = None;
                row.pixels = Vec::new();
                row.ownership.discard()?;
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
                    wrap_u: record.wrap_u,
                    wrap_v: record.wrap_v,
                    main_world_only: false,
                    decoded_bytes: row.pixels.len(),
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
                    shader.cpu = 0;
                } else if server.is_loaded_with_dependencies(handle.id()) {
                    if let Some(asset) = shaders.get(handle) {
                        let retained = shader_owned_bytes(asset)?
                            .checked_mul(2)
                            .and_then(|n| n.checked_add(4096))
                            .ok_or_else(|| {
                                ReferenceError("Fixed shader retained overflow".into())
                            })?;
                        if retained > shader.cpu {
                            return Err(ReferenceError(
                                "Fixed shader exceeds construction bound".into(),
                            ));
                        }
                        shader.cpu = retained;
                        shader.done = true;
                    }
                }
            }
        }
        if let Some(handle) = &self.cloud {
            if let LoadState::Failed(error) = server.load_state(handle.id()) {
                self.failure.get_or_insert(format!("Fixed cloud: {error}"));
                self.cloud_bound = None;
            } else if meshes.contains(handle.id()) && self.cloud_fence.is_none() {
                let fence = Arc::new(MeshUpload {
                    meshes: vec![handle.clone()],
                    receipt: default(),
                });
                self.uploads.lock().unwrap().meshes.push(Arc::clone(&fence));
                self.cloud_fence = Some(fence);
            }
            if self
                .cloud_fence
                .as_ref()
                .is_some_and(|f| f.receipt.completed())
                && !self.cloud_done
            {
                let mesh = meshes.get(handle).unwrap();
                self.cloud_retained =
                    checked_sum(&[crate::presentation::world_metrics::mesh_payload(mesh), 8192])?;
                if self.cloud_retained > self.cloud_bound.unwrap().cpu {
                    return Err(ReferenceError(
                        "Cloud retained bytes exceed reservation".into(),
                    ));
                }
                self.cloud_bound.as_mut().unwrap().gpu = checked_sum(&[
                    mesh.get_vertex_buffer_size(),
                    mesh.get_index_buffer_bytes().map_or(0, <[u8]>::len),
                    8,
                ])?;
                self.cloud_done = true;
            }
        }
        if let Some(handle) = &self.dither {
            if let LoadState::Failed(error) = server.load_state(handle.id()) {
                self.dither_failed = true;
                self.failure.get_or_insert(format!("Fixed dither: {error}"));
            }
            if render_lighting.caster_dither.is_none() {
                if let Some(data) = dither.remove(handle.id()) {
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
                    self.uploads
                        .lock()
                        .unwrap()
                        .images
                        .push((handle.clone(), self.dither_receipt.clone()));
                    render_lighting.caster_dither = Some(handle);
                    self.dither_converted = true;
                }
            }
            self.dither_done = self.dither_receipt.completed();
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
        snapshot: &mut FixedAllocation,
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
            let bound = FixedReservation::mesh(plan, retained_plan_bytes(plan))?;
            if !crate::ownership::fixed::fixed_room(
                ledger,
                bound.cpu.saturating_sub(root.cpu),
                bound.gpu,
                bound.staging,
                true,
            )? {
                continue;
            }
            let count = plan.primitives.len();
            let record = reference
                .assets
                .iter()
                .find(|a| a.id == root.asset)
                .unwrap();
            self.roots[index].cpu = bound.cpu;
            self.roots[index].gpu = bound.gpu;
            self.roots[index].staging = bound.staging;
            let mut next = self.allocation()?;
            next.jobs = checked_sum(&[next.jobs, 1])?;
            next.pending = checked_sum(&[next.pending, 1])?;
            next.staging = checked_sum(&[next.staging, bound.staging])?;
            ledger.set_allocation(next.replace(*snapshot, ledger.allocation())?)?;
            *snapshot = next;
            self.roots[index].meshes.reserve_exact(count);
            let handle = server
                .load_builder()
                .with_settings(super::reference::source_gltf_settings)
                .load(record.path.clone());
            visuals.handles.push((record.path.clone(), handle.clone()));
            visuals.gltfs.push((record.id, handle.clone()));
            self.roots[index].state.start_payload(handle);
        }
        // Imported shader libraries must finish before their importing shaders start.
        if let Some(index) = self.shaders.iter().position(|s| !s.done) {
            if self.shaders[index].handle.is_none() {
                let bound = shader_input(SHADERS[index])?;
                let (cpu, staging) = (bound.cpu, bound.staging);
                if crate::ownership::fixed::fixed_room(ledger, cpu, 0, staging, true)? {
                    self.shaders[index].cpu = cpu;
                    self.shaders[index].staging = staging;
                    let mut next = self.allocation()?;
                    next.jobs = checked_sum(&[next.jobs, 1])?;
                    next.pending = checked_sum(&[next.pending, 1])?;
                    next.staging = checked_sum(&[next.staging, staging])?;
                    ledger.set_allocation(next.replace(*snapshot, ledger.allocation())?)?;
                    *snapshot = next;
                    self.shaders[index].handle = Some(server.load(SHADERS[index]));
                }
            }
        } else if !self.shaders_bound {
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
                heat_haze: shader("shaders/source_heat_haze.wgsl"),
                ao_depth: shader("shaders/source_ao_depth.wgsl"),
                ao_render: shader("shaders/source_ao_render.wgsl"),
                ao_upsample: shader("shaders/source_ao_upsample.wgsl"),
                ao_composite: shader("shaders/source_ao_composite.wgsl"),
                depth_resolve: shader("shaders/source_depth_resolve.wgsl"),
                props: shader("shaders/source_props.wgsl"),
                props_cull: shader("shaders/source_props_cull.wgsl"),
                motion: shader("shaders/source_motion.wgsl"),
                taa: shader("shaders/source_taa.wgsl"),
                motion_blur: shader("shaders/source_motion_blur.wgsl"),
            };
            self.shaders_bound = true;
        }
        for index in 0..self.rows.len() {
            if self.rows.iter().any(|r| r.load.is_some()) {
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
            let old = row.ownership.bytes();
            let next = ownership.bytes();
            if !crate::ownership::fixed::fixed_room(
                ledger,
                next.0.saturating_sub(old.0),
                next.1.saturating_sub(old.1),
                ROW_SCRATCH,
                first,
            )? {
                continue;
            }
            self.rows[index].ownership = ownership;
            self.account(ledger, snapshot)?;
            if first {
                self.rows[index].pixels = vec![0; record.validate_chunks()?];
            }
            let (srgb, filter, wrap_u, wrap_v) = (
                record.color_space != 0,
                record.filter_mode,
                record.wrap_u,
                record.wrap_v,
            );
            let decoded_bytes = record.chunk_range(self.rows[index].chunk)?.len();
            self.rows[index].load = Some(
                server
                    .load_builder()
                    .with_settings(move |s: &mut super::textures::SourceImageSettings| {
                        *s = super::textures::SourceImageSettings {
                            srgb,
                            filter,
                            wrap_u,
                            wrap_v,
                            main_world_only: true,
                            decoded_bytes,
                        };
                    })
                    .load(chunk.data.path.clone()),
            );
        }
        if self.cloud.is_none() {
            let bound = FixedReservation::cloud(
                &reference.render_lighting.sky.cloud_mesh,
                &reference.file_catalog,
            )?;
            if crate::ownership::fixed::fixed_room(
                ledger,
                bound.cpu,
                bound.gpu,
                bound.staging,
                true,
            )? {
                self.cloud_bound = Some(bound);
                self.account(ledger, snapshot)?;
                let handle = server
                    .load_builder()
                    .with_settings(super::reference::source_gltf_settings)
                    .load(
                        GltfAssetLabel::Primitive {
                            mesh: 0,
                            primitive: 0,
                        }
                        .from_asset(reference.render_lighting.sky.cloud_mesh.file.clone()),
                    );
                assets.cloud_mesh = handle.clone();
                self.cloud = Some(handle);
            }
        }
        if self.dither.is_none() {
            let path = &reference.render_lighting.caster_dither.file;
            if reference.file_catalog.file(path)?.bytes != 256 {
                return Err(ReferenceError("Fixed dither receipt size".into()));
            }
            if crate::ownership::fixed::fixed_room(ledger, 1536, 256, 512, true)? {
                let mut next = self.allocation()?;
                next.cpu = checked_sum(&[next.cpu, 1536])?;
                next.gpu = checked_sum(&[next.gpu, 256])?;
                next.jobs = checked_sum(&[next.jobs, 1])?;
                next.pending = checked_sum(&[next.pending, 1])?;
                next.staging = checked_sum(&[next.staging, 512])?;
                ledger.set_allocation(next.replace(*snapshot, ledger.allocation())?)?;
                *snapshot = next;
                let handle = server.load(path.clone());
                assets.dither_bytes = handle.clone();
                self.dither = Some(handle);
            }
        }
        if self.cookies.is_none() && self.rows.iter().all(|r| r.image.is_some()) {
            let ids = super::lighting::cookie_ids(reference);
            let layers = ids.len() - 2;
            let bytes = (0..10)
                .map(|m| layers * (512usize >> m).pow(2) * 4)
                .sum::<usize>();
            if crate::ownership::fixed::fixed_room(ledger, 2 * bytes, bytes, 0, true)? {
                let mut next = self.allocation()?;
                next.cpu = checked_sum(&[next.cpu, 2 * bytes])?;
                next.gpu = checked_sum(&[next.gpu, bytes])?;
                next.pending = checked_sum(&[next.pending, 1])?;
                ledger.set_allocation(next.replace(*snapshot, ledger.allocation())?)?;
                *snapshot = next;
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
                let array = super::lighting::pack_custom_cookies(images, &originals)
                    .map_err(ReferenceError)?;
                lighting.cookie_handles = Some([handle(ids[0]), handle(ids[1]), array.clone()]);
                self.uploads
                    .lock()
                    .unwrap()
                    .images
                    .push((array.clone(), self.cookie_receipt.clone()));
                self.cookies = Some(array);
                self.cookie_bytes = bytes;
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
    if shaders.0.is_none() {
        if let Some(source) = source.as_ref().filter(|s| s.shaders_bound) {
            shaders.0 = Some(std::array::from_fn(|i| {
                source.shaders[i].handle.as_ref().unwrap().clone()
            }));
        }
    }
    if extracted.0.is_none() {
        if let Some(source) = source.as_ref() {
            extracted.0 = Some(Arc::clone(&source.uploads));
        }
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

fn shader_owned_bytes(shader: &Shader) -> Result<usize, ReferenceError> {
    use bevy::shader::{ShaderImport, Source};
    let source = match &shader.source {
        Source::Wgsl(std::borrow::Cow::Owned(s)) => s.capacity(),
        Source::Wgsl(std::borrow::Cow::Borrowed(_)) => 0,
        _ => return Err(ReferenceError("Fixed shader source format".into())),
    };
    if !shader.additional_imports.is_empty() || !shader.shader_defs.is_empty() {
        return Err(ReferenceError(
            "Fixed shader definitions require an ownership bound".into(),
        ));
    }
    let import = |i: &ShaderImport| match i {
        ShaderImport::AssetPath(s) | ShaderImport::Custom(s) => s.capacity(),
    };
    let imports = shader.imports.iter().try_fold(0usize, |sum, i| {
        sum.checked_add(import(i))
            .ok_or_else(|| ReferenceError("Fixed shader import ownership overflow".into()))
    })?;
    [
        std::mem::size_of::<Shader>(),
        source,
        shader.path.capacity(),
        import(&shader.import_path),
        imports,
        shader
            .imports
            .capacity()
            .checked_mul(std::mem::size_of::<ShaderImport>())
            .ok_or_else(|| ReferenceError("Fixed shader import capacity overflow".into()))?,
        shader
            .file_dependencies
            .capacity()
            .checked_mul(std::mem::size_of::<Handle<Shader>>())
            .ok_or_else(|| ReferenceError("Fixed shader handle capacity overflow".into()))?,
    ]
    .into_iter()
    .try_fold(0usize, |sum, n| {
        sum.checked_add(n)
            .ok_or_else(|| ReferenceError("Fixed shader retained total overflow".into()))
    })
}

fn checked_sum(values: &[usize]) -> Result<usize, ReferenceError> {
    values.iter().try_fold(0usize, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| ReferenceError("Fixed ownership total overflow".into()))
    })
}
