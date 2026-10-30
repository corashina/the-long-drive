//! Dynamic primary materials share the accepted source material cache.
use super::*;
use crate::ownership::PresentationLedger;
pub(super) use crate::reference::fixed_reservation::{png_chain_staging, primary_staging};
use crate::{
    reference::{AssetId as SourceAssetId, ReferenceError},
    world::{
        RendererWorld,
        stream::{PROTOTYPE_CPU, PROTOTYPE_GPU},
    },
};
#[derive(Default, Clone)]
pub struct PrimaryOwnership {
    pub(super) assets: HashMap<SourceAssetId, Vec<String>>,
    owners: HashMap<SourceAssetId, Arc<()>>,
    materials: std::collections::HashSet<(SourceAssetId, u32)>,
    pub(super) borrowed_records: std::collections::HashSet<SourceAssetId>,
    cpu: usize,
    gpu: usize,
    jobs: usize,
    pending: usize,
    staging: usize,
}
fn payload(texture: &TextureRecord) -> Result<usize, ReferenceError> {
    texture.payload_bytes()
}
fn texture_cpu_reservation(texture: &SourceTexture) -> usize {
    texture.cpu_reservation
}
pub(super) fn pending_texture_cpu(texture: &SourceTexture) -> usize {
    texture.upload.cpu_bytes(
        texture_cpu_reservation(texture),
        texture.chain.is_some() || texture.image.is_some(),
    )
}
pub(super) fn texture_metadata(texture: &TextureRecord) -> usize {
    4096 + texture.retained_cpu_bytes()
}

pub(super) fn record_bytes(record: &MaterialRecord) -> usize {
    record.retained_cpu_bytes()
}

impl SourceMaterials {
    pub fn begin_world_session(&mut self) {
        // Resources stay owned. The new stream receives their full charges on preparation.
        self.world_primary.cpu = 0;
        self.world_primary.gpu = 0;
        self.world_primary.jobs = 0;
        self.world_primary.pending = 0;
        self.world_primary.staging = 0;
        self.world_deferred = DeferredAllocation::default();
    }

    fn container_bytes(&self) -> usize {
        use std::mem::size_of;
        self.generated.bytes()
            + self.sort_data.capacity()
                * (size_of::<(
                    (SourceAssetId, u32),
                    super::super::renderer_visibility::SourceSortData,
                )>() + 1)
            + self.records.capacity() * size_of::<Arc<MaterialRecord>>()
            + self.textures.capacity() * size_of::<SourceTexture>()
            + self
                .textures
                .iter()
                .map(|t| t.label.capacity())
                .sum::<usize>()
            + self.texture_indices.capacity() * (size_of::<(String, usize)>() + 1)
            + self
                .texture_indices
                .keys()
                .map(String::capacity)
                .sum::<usize>()
            + self.handles.capacity()
                * (size_of::<((SourceAssetId, u32, bool, u32), Handle<SourceMaterial>)>() + 1)
            + self.standard_records.capacity()
                * (size_of::<(AssetId<StandardMaterial>, (SourceAssetId, u32))>() + 1)
            + self.world_primary.assets.capacity() * (size_of::<(SourceAssetId, Vec<String>)>() + 1)
            + self
                .world_primary
                .assets
                .values()
                .map(|paths| {
                    paths.capacity() * size_of::<String>()
                        + paths.iter().map(String::capacity).sum::<usize>()
                })
                .sum::<usize>()
            + self.world_primary.owners.capacity() * (size_of::<(SourceAssetId, Arc<()>)>() + 1)
            + self.world_primary.borrowed_records.capacity() * (size_of::<SourceAssetId>() + 1)
            + self.world_primary.materials.capacity() * (size_of::<(SourceAssetId, u32)>() + 1)
    }
    fn primary_bytes(&self) -> (usize, usize) {
        let cpu = self
            .textures
            .iter()
            .map(|t| pending_texture_cpu(t) + texture_metadata(&t.record))
            .sum::<usize>()
            + self
                .records
                .iter()
                .map(|record| record_bytes(record))
                .sum::<usize>();
        let gpu = self
            .textures
            .iter()
            .filter(|t| !t.failed || t.image.is_some())
            .map(|t| t.payload_bytes)
            .sum();
        (cpu + self.container_bytes(), gpu)
    }
    fn account_primary(
        &mut self,
        ledger: &mut PresentationLedger,
        cpu: usize,
        gpu: usize,
        jobs: usize,
        pending: usize,
        staging: usize,
    ) -> Result<(), ReferenceError> {
        let mut allocation = ledger.allocation();
        allocation.prototype_cpu = allocation
            .prototype_cpu
            .saturating_sub(self.world_primary.cpu)
            + cpu;
        allocation.prototype_gpu = allocation
            .prototype_gpu
            .saturating_sub(self.world_primary.gpu)
            + gpu;
        allocation.jobs = allocation.jobs.saturating_sub(self.world_primary.jobs) + jobs;
        allocation.pending = allocation
            .pending
            .saturating_sub(self.world_primary.pending)
            + pending;
        allocation.staging_bytes = allocation
            .staging_bytes
            .saturating_sub(self.world_primary.staging)
            + staging;
        allocation.dependency_staging_bytes = allocation
            .dependency_staging_bytes
            .saturating_sub(self.world_primary.staging)
            + staging;
        ledger.set_allocation(allocation)?;
        self.world_primary.cpu = cpu;
        self.world_primary.gpu = gpu;
        self.world_primary.jobs = jobs;
        self.world_primary.pending = pending;
        self.world_primary.staging = staging;
        Ok(())
    }
    pub fn admit_world_primary(
        &mut self,
        identity: (SourceAssetId, u32),
        reference: &ReferenceManifest,
        ledger: &mut PresentationLedger,
    ) -> Result<(), ReferenceError> {
        let asset = identity.0;
        if self.world_primary.materials.contains(&identity) {
            return Ok(());
        }
        let borrowed = self.world_primary.borrowed_records.contains(&asset)
            || (!self.world_primary.assets.contains_key(&asset)
                && self.records.iter().any(|r| r.asset == asset));
        let records: Vec<_> = reference
            .rendering
            .materials
            .iter()
            .filter(|r| (r.asset, r.gltf_material) == identity)
            .collect();
        if records.is_empty() {
            return Err(ReferenceError("Missing selected primary material".into()));
        }
        let mut paths = Vec::new();
        let mut added_cpu = 0usize;
        let mut added_gpu = 0usize;
        for record in &records {
            added_cpu += record_bytes(record);
            for texture in &record.properties.textures {
                if paths.iter().any(|p| p == texture.file()) {
                    continue;
                }
                paths.push(texture.file().to_owned());
                if let Some(&index) = self.texture_indices.get(texture.file()) {
                    let retained = &self.textures[index].record;
                    let a = &retained.sampler;
                    let b = &texture.sampler;
                    if retained.source != texture.source
                        || retained.color_space != texture.color_space
                        || retained.encoding != texture.encoding
                        || a.filter != b.filter
                        || a.wrap_u != b.wrap_u
                        || a.wrap_v != b.wrap_v
                        || a.aniso != b.aniso
                        || a.mip_bias.to_bits() != b.mip_bias.to_bits()
                        || a.mip_count != b.mip_count
                        || retained.source_encoding != texture.source_encoding
                        || retained.storage != texture.storage
                    {
                        return Err(ReferenceError(
                            "Conflicting source texture identity or sampler".into(),
                        ));
                    }
                }
                if !self.texture_indices.contains_key(texture.file()) {
                    let bytes = payload(texture)?;
                    added_gpu += bytes;
                    added_cpu += 4096
                        + std::mem::size_of::<SourceTexture>()
                        + texture.retained_cpu_bytes()
                        + 128;
                }
            }
        }
        self.account_primary(
            ledger,
            self.world_primary.cpu + added_cpu,
            self.world_primary.gpu + added_gpu,
            self.world_primary.jobs,
            self.world_primary.pending,
            self.world_primary.staging,
        )?;
        for record in records {
            for texture in &record.properties.textures {
                if !self.texture_indices.contains_key(texture.file()) {
                    let input_staging = primary_staging(texture, &reference.file_catalog)?;
                    let payload_bytes = payload(texture)?;
                    let cpu_reservation =
                        crate::reference::fixed_reservation::png_cpu(payload_bytes, input_staging)?;
                    self.texture_indices
                        .insert(texture.file().to_owned(), self.textures.len());
                    self.textures.push(SourceTexture {
                        owner: Arc::new(()),
                        upload: default(),
                        failed: false,
                        persistent: false,
                        users: 0,
                        record: Arc::new(texture.clone()),
                        chain: None,
                        input_staging,
                        payload_bytes,
                        cpu_reservation,
                        image: None,
                        label: format!(
                            "source material {}:{} {}",
                            record.source.file, record.source.path_id, texture.slot
                        ),
                    });
                }
            }
            if !borrowed {
                self.records.push(Arc::new(record.clone()));
            }
        }
        let owned = self.world_primary.assets.entry(asset).or_default();
        for path in paths {
            if !owned.contains(&path) {
                self.textures[self.texture_indices[&path]].users += 1;
                owned.push(path);
            }
        }
        if borrowed {
            self.world_primary.borrowed_records.insert(asset);
        }
        self.world_primary.materials.insert(identity);
        self.world_primary
            .owners
            .entry(asset)
            .or_insert_with(|| Arc::new(()));
        Ok(())
    }
    pub fn world_primary_admitted(&self, identity: (SourceAssetId, u32)) -> bool {
        self.world_primary.materials.contains(&identity)
    }
    pub fn world_primary_ready(&self, identity: (SourceAssetId, u32)) -> bool {
        self.world_primary.materials.contains(&identity)
            && self
                .records
                .iter()
                .find(|record| (record.asset, record.gltf_material) == identity)
                .is_some_and(|record| {
                    record.properties.textures.iter().all(|record| {
                        let texture = &self.textures[self.texture_indices[record.file()]];
                        texture.image.is_some() && pending_texture_cpu(texture) == 0
                    })
                })
    }
    pub fn prepare_world_primary(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        server: &AssetServer,
        images: &mut Assets<Image>,
        chains: &mut Assets<SourcePrimaryImage>,
        materials: &mut Assets<SourceMaterial>,
    ) -> Result<(), ReferenceError> {
        let mut failure = None;
        for texture in self.textures.iter_mut().filter(|t| t.image.is_none()) {
            if let Some(handle) = &texture.chain {
                if let LoadState::Failed(error) = server.load_state(handle.id()) {
                    if !texture.failed {
                        failure = Some(ReferenceError(format!("{}: {error}", texture.label)));
                    }
                    texture.failed = true;
                }
                if texture.failed
                    && (chains.contains(handle.id())
                        || matches!(server.load_state(handle.id()), LoadState::Failed(_)))
                {
                    if Arc::strong_count(&texture.owner) == 1 {
                        chains.remove(handle.id());
                    }
                    texture.chain = None;
                }
            }
        }
        let retired: Vec<_> = self
            .world_primary
            .assets
            .iter()
            .filter(|(_, paths)| {
                paths
                    .iter()
                    .any(|p| self.textures[self.texture_indices[p]].failed)
            })
            .map(|(id, _)| *id)
            .collect();
        for asset in retired {
            if self.world_asset_retirable(asset, chains, server) {
                self.release_world_asset(asset, images, chains, materials, server)?;
            }
        }
        let (jobs, staging) = self.primary_loading(chains, server);
        let (cpu, gpu) = self.primary_bytes();
        self.account_primary(ledger, cpu, gpu, jobs, self.pending_primary(), staging)?;
        if !self.preparation.cancelled() && failure.is_none() && stream.frame().failure.is_none() {
            for index in 0..self.textures.len() {
                let texture = &self.textures[index];
                if texture.failed
                    || (texture.persistent && !self.fixed_admitted)
                    || texture.image.is_some()
                    || texture.chain.is_some()
                {
                    continue;
                }
                let staging = texture.input_staging;
                if staging > 32 * 1024 * 1024 {
                    return Err(ReferenceError(
                        "Primary input overlap exceeds staging cap".into(),
                    ));
                }
                // Every request runs, so the report asks for all credit of the load at once.
                let job = ledger.available_external_jobs() > 0;
                let claim = ledger.can_request_independent(1);
                let input = ledger.request_staging(staging);
                let decoded = texture_cpu_reservation(texture);
                // Without credit the load waits. Retired prototypes give their credit back.
                let prototype = ledger.request_prototype(decoded, 0);
                if !job {
                    break;
                }
                if !(claim && input && prototype) {
                    continue;
                }
                // All reads belong to this one job. Reserve the largest sequential input.
                self.account_primary(
                    ledger,
                    self.world_primary.cpu + decoded,
                    gpu,
                    self.world_primary.jobs + 1,
                    self.pending_primary() + 1,
                    self.world_primary.staging + staging,
                )?;
                let texture = &mut self.textures[index];
                let record = Arc::clone(&texture.record);
                let encoded_bytes = match &record.storage {
                    source::TextureStorage::PngChain { mips } => mips
                        .iter()
                        .map(|m| {
                            crate::presentation::world::preflight::checked_input_bytes(&m.file)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    source::TextureStorage::NativeBc { input, .. } => vec![input.bytes as usize],
                };
                texture.chain = Some(
                    server
                        .load_builder()
                        .with_settings(move |settings: &mut SourcePrimaryImageSettings| {
                            settings.record = Some(Arc::clone(&record));
                            settings.encoded_bytes = encoded_bytes.clone();
                        })
                        .load(texture.record.file().to_owned()),
                );
            }
        }
        if !self.preparation.cancelled() {
            self.texture_bytes += assemble_textures(&mut self.textures, chains, images);
        }
        let (jobs, staging) = self.primary_loading(chains, server);
        let (cpu, gpu) = self.primary_bytes();
        self.account_primary(ledger, cpu, gpu, jobs, self.pending_primary(), staging)?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
    fn primary_loading(
        &self,
        chains: &Assets<SourcePrimaryImage>,
        server: &AssetServer,
    ) -> (usize, usize) {
        self.textures
            .iter()
            .filter(|t| {
                t.chain.as_ref().is_some_and(|h| {
                    !chains.contains(h.id())
                        && !matches!(server.load_state(h.id()), LoadState::Failed(_))
                })
            })
            .fold((0, 0), |(jobs, bytes), t| {
                (jobs + 1, bytes + t.input_staging)
            })
    }
    /// World primary textures of `asset` are admitted.
    pub fn holds_world_asset(&self, asset: SourceAssetId) -> bool {
        self.world_primary.assets.contains_key(&asset)
    }
    pub fn world_asset_retirable(
        &self,
        asset: SourceAssetId,
        chains: &Assets<SourcePrimaryImage>,
        server: &AssetServer,
    ) -> bool {
        self.world_primary.assets.get(&asset).is_none_or(|paths| {
            paths.iter().all(|path| {
                let t = &self.textures[self.texture_indices[path]];
                if t.image.is_some() {
                    pending_texture_cpu(t) == 0
                } else {
                    t.chain.iter().all(|h| {
                        chains.contains(h.id())
                            || matches!(server.load_state(h.id()), LoadState::Failed(_))
                    })
                }
            })
        })
    }
    pub fn release_world_asset(
        &mut self,
        asset: SourceAssetId,
        images: &mut Assets<Image>,
        chains: &mut Assets<SourcePrimaryImage>,
        materials: &mut Assets<SourceMaterial>,
        server: &AssetServer,
    ) -> Result<(), ReferenceError> {
        let Some(_paths) = self.world_primary.assets.get(&asset) else {
            return Ok(());
        };
        if !self.world_asset_retirable(asset, chains, server) {
            return Err(ReferenceError(
                "Source primary texture retirement still has a loader job".into(),
            ));
        }
        let unique = self
            .world_primary
            .owners
            .remove(&asset)
            .is_none_or(|owner| Arc::strong_count(&owner) == 1);
        self.world_primary
            .materials
            .retain(|identity| identity.0 != asset);
        let paths = self.world_primary.assets.remove(&asset).unwrap();
        for path in paths {
            self.textures[self.texture_indices[&path]].users -= 1;
        }
        // Startup overrides can borrow one of the five imported material assets.
        // Their cache handles and records remain owned by the imported scene.
        if !self.world_primary.borrowed_records.remove(&asset) {
            self.handles.retain(|key, handle| {
                if key.0 == asset {
                    if unique {
                        materials.remove(handle.id());
                    }
                    false
                } else {
                    true
                }
            });
            self.records.retain(|r| r.asset != asset);
        }
        self.textures.retain(|texture| {
            if !texture.persistent && texture.users == 0 {
                if Arc::strong_count(&texture.owner) == 1 {
                    if let Some(chain) = &texture.chain {
                        chains.remove(chain.id());
                    }
                    if let Some(image) = &texture.image {
                        images.remove(image.id());
                    }
                }
                false
            } else {
                true
            }
        });
        self.texture_indices.clear();
        for (index, texture) in self.textures.iter().enumerate() {
            self.texture_indices
                .insert(texture.record.file().to_owned(), index);
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct DeferredAllocation {
    pending: usize,
    staging: usize,
    cpu: usize,
    gpu: usize,
    jobs: usize,
}
impl SourceMaterials {
    fn account_deferred(
        &mut self,
        ledger: &mut PresentationLedger,
        jobs: usize,
        staging: usize,
    ) -> Result<(), ReferenceError> {
        let (cpu, gpu) = self.world_textures.bytes();
        let mut allocation = ledger.allocation();
        allocation.prototype_cpu = allocation
            .prototype_cpu
            .saturating_sub(self.world_deferred.cpu)
            + cpu;
        allocation.prototype_gpu = allocation
            .prototype_gpu
            .saturating_sub(self.world_deferred.gpu)
            + gpu;
        allocation.jobs = allocation.jobs.saturating_sub(self.world_deferred.jobs) + jobs;
        let pending = self.world_textures.pending();
        allocation.pending = allocation
            .pending
            .saturating_sub(self.world_deferred.pending)
            + pending;
        allocation.staging_bytes = allocation
            .staging_bytes
            .saturating_sub(self.world_deferred.staging)
            + staging;
        allocation.dependency_staging_bytes = allocation
            .dependency_staging_bytes
            .saturating_sub(self.world_deferred.staging)
            + staging;
        ledger.set_allocation(allocation)?;
        self.world_deferred = DeferredAllocation {
            pending,
            cpu,
            gpu,
            jobs,
            staging,
        };
        Ok(())
    }
    pub fn admit_world_material(
        &mut self,
        material: &source::WorldSourceMaterial,
        reference: &ReferenceManifest,
        ledger: &mut PresentationLedger,
    ) -> Result<(), ReferenceError> {
        if self.world_textures.admitted(&material.source) {
            return Ok(());
        }
        let (cpu, gpu) = self.world_textures.bytes();
        self.world_textures.admit(
            material,
            if reference
                .world
                .terrain
                .materials
                .iter()
                .any(|m| m.source == material.source)
            {
                &reference.world.terrain.textures
            } else {
                &reference.world.material_textures
            },
            if reference
                .world
                .terrain
                .materials
                .iter()
                .any(|m| m.source == material.source)
            {
                &reference.world.terrain.materials
            } else {
                std::slice::from_ref(material)
            },
            // A charge, not a request: the stream checks the shared total.
            PROTOTYPE_CPU.saturating_sub(ledger.allocation().prototype_cpu) + cpu,
            PROTOTYPE_GPU.saturating_sub(ledger.allocation().prototype_gpu) + gpu,
        )?;
        self.account_deferred(
            ledger,
            self.world_deferred.jobs,
            self.world_deferred.staging,
        )
    }
    pub fn prepare_world_deferred(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        server: &AssetServer,
        images: &mut Assets<Image>,
        batches: &mut Assets<crate::presentation::texture_batches::SourceTextureBatch>,
    ) -> Result<(), ReferenceError> {
        if self.preparation.cancelled() || stream.frame().failure.is_some() {
            self.world_textures.cancel();
        }
        self.account_deferred(
            ledger,
            self.world_textures.jobs(),
            self.world_textures.staging_bytes(),
        )?;
        let result =
            self.world_textures
                .prepare(server, images, batches, &self.preparation, ledger);
        self.account_deferred(
            ledger,
            self.world_textures.jobs(),
            self.world_textures.staging_bytes(),
        )?;
        result?;
        // CPU and pair staging are committed before either AssetServer request.
        let launch = match self.world_textures.queued_jobs() {
            0 => 0,
            queued => ledger.available_external_jobs().min(queued),
        };
        self.account_deferred(
            ledger,
            self.world_textures.jobs() + launch,
            self.world_textures.staging_bytes(),
        )?;
        self.world_textures.start_jobs(server, launch);
        self.account_deferred(
            ledger,
            self.world_textures.jobs(),
            self.world_textures.staging_bytes(),
        )
    }
}

impl PrimaryOwnership {
    pub fn asset_ids(&self) -> impl Iterator<Item = SourceAssetId> + '_ {
        self.assets.keys().copied()
    }
}

impl SourceMaterials {
    pub(super) fn select_lighting_textures(
        &mut self,
        reference: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        for t in &reference.render_lighting.textures {
            let file = &t.mips[0].file;
            if let Some(&index) = self.texture_indices.get(file) {
                let old = &self.textures[index].record;
                if old.source != t.source
                    || old.color_space != t.color_space
                    || old.sampler.filter != t.sampler.filter
                    || old.sampler.wrap_u != t.sampler.wrap_u
                    || old.sampler.wrap_v != t.sampler.wrap_v
                    || old.sampler.aniso != t.sampler.aniso
                    || old.sampler.mip_bias.to_bits() != t.sampler.mip_bias.to_bits()
                    || old.sampler.mip_count != t.sampler.mip_count
                    || !matches!(&old.storage, source::TextureStorage::PngChain { mips } if *mips == t.mips)
                {
                    return Err(ReferenceError(
                        "Conflicting persistent lighting sampler".into(),
                    ));
                }
                self.textures[index].persistent = true;
                continue;
            }
            let record = TextureRecord {
                slot: String::new(),
                source: t.source.clone(),
                storage: source::TextureStorage::PngChain {
                    mips: t.mips.clone(),
                },
                color_space: t.color_space,
                encoding: source::TextureEncoding::Rgba,
                source_encoding: source::TextureEncoding::Rgba,
                uv_channel: 0,
                scale: [1., 1.],
                offset: [0., 0.],
                sampler: t.sampler.clone(),
            };
            let input_staging =
                png_chain_staging(&t.mips, t.sampler.mip_count, &reference.file_catalog)?;
            let payload_bytes = payload(&record)?;
            let cpu_reservation =
                crate::reference::fixed_reservation::png_cpu(payload_bytes, input_staging)?;
            self.texture_indices
                .insert(file.clone(), self.textures.len());
            self.textures.push(SourceTexture {
                owner: Arc::new(()),
                upload: default(),
                failed: false,
                persistent: true,
                users: 0,
                record: Arc::new(record),
                chain: None,
                input_staging,
                payload_bytes,
                cpu_reservation,
                image: None,
                label: file.clone(),
            });
        }
        Ok(())
    }
    pub(crate) fn cancel_fixed(&mut self) {
        self.fixed_admitted = false;
        for texture in self
            .textures
            .iter_mut()
            .filter(|t| t.persistent && t.image.is_none())
        {
            texture.failed = true;
        }
    }
    fn pending_primary(&self) -> usize {
        self.textures
            .iter()
            .filter(|t| pending_texture_cpu(t) > 0)
            .count()
    }
    pub(crate) fn fixed_textures_ready(&self) -> bool {
        self.fixed_admitted
            && self
                .textures
                .iter()
                .filter(|t| t.persistent)
                .all(|t| t.image.is_some() && t.upload.completed())
    }
}

impl SourceMaterials {
    pub(crate) fn primary_startup_audit(&self, server: &AssetServer) -> serde_json::Value {
        let (cpu, gpu) = self.primary_bytes();
        let (_, deferred_gpu) = self.world_textures.bytes();
        serde_json::json!({
            "cpu_bytes": cpu, "gpu_bytes": gpu, "deferred_gpu_bytes": deferred_gpu,
            "accounted_primary_gpu_bytes": self.world_primary.gpu,
            "accounted_deferred_gpu_bytes": self.world_deferred.gpu,
            "selected_materials": self.world_primary.materials.iter().collect::<Vec<_>>(),
            "textures": self.textures.iter().map(|t| serde_json::json!({
                "source": t.record.source, "path": t.record.file(), "payload_bytes": t.payload_bytes, "storage": match &t.record.storage { source::TextureStorage::PngChain { .. } => "png_chain", source::TextureStorage::NativeBc { format: source::NativeBcFormat::Bc1, .. } => "bc1", source::TextureStorage::NativeBc { .. } => "bc3" },
                "persistent": t.persistent, "users": t.users, "owners": Arc::strong_count(&t.owner),
                "failed": t.failed, "upload_complete": t.upload.completed(),
                "cpu_reservation": pending_texture_cpu(t), "input_staging": t.input_staging,
                "image": t.image.as_ref().map(|h| format!("{:?}", h.id())),
                "chain": t.chain.as_ref().map(|h| format!("{:?}", h.id())),
                "load_state": t.chain.as_ref().map(|h| format!("{:?}", server.load_state(h.id()))),
            })).collect::<Vec<_>>(),
        })
    }
}
