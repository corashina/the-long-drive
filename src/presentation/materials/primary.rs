//! Dynamic primary materials share the accepted source material cache.
use super::*;
use crate::ownership::{PresentationLedger, RendererCounts};
pub(super) use crate::reference::fixed_reservation::primary_staging;
use crate::{
    reference::{AssetId as SourceAssetId, ReferenceError},
    world::RendererWorld,
};
#[derive(Default, Clone)]
pub struct PrimaryOwnership {
    pub(super) assets: HashMap<SourceAssetId, Vec<String>>,
    owners: HashMap<SourceAssetId, Arc<()>>,
    materials: std::collections::HashSet<(SourceAssetId, u32)>,
    pub(super) borrowed_records: std::collections::HashSet<SourceAssetId>,
    counts: RendererCounts,
}
/// Renderer steps an added image may wait for its GPU upload before the chain loads again.
const STALLED_STEPS: u32 = 600;
pub(super) fn pending_texture_cpu(texture: &SourceTexture) -> usize {
    texture.upload.cpu_bytes(
        texture.cpu_reservation,
        texture.chain.is_some() || texture.image.is_some(),
    )
}
impl SourceMaterials {
    pub fn begin_world_session(&mut self) {
        // Resources stay owned. The new stream receives their counts on preparation.
        self.world_primary.counts = RendererCounts::default();
        self.world_deferred = RendererCounts::default();
    }

    fn account_primary(
        &mut self,
        ledger: &mut PresentationLedger,
        jobs: usize,
        pending: usize,
    ) -> Result<(), ReferenceError> {
        let counts = RendererCounts { jobs, pending };
        counts.apply(self.world_primary.counts, ledger)?;
        self.world_primary.counts = counts;
        Ok(())
    }
    pub fn admit_world_primary(
        &mut self,
        identity: (SourceAssetId, u32),
        reference: &ReferenceManifest,
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
        for record in &records {
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
            }
        }
        for record in records {
            for texture in &record.properties.textures {
                if !self.texture_indices.contains_key(texture.file()) {
                    let input_staging = primary_staging(texture, &reference.file_catalog)?;
                    let label = format!(
                        "source material {}:{} {}",
                        record.source.file, record.source.path_id, texture.slot
                    );
                    let entry = SourceTexture::new(texture.clone(), input_staging, false, label)?;
                    self.texture_indices
                        .insert(texture.file().to_owned(), self.textures.len());
                    self.textures.push(entry);
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
        for texture in self.textures.iter_mut() {
            if let Some(image) = &texture.image
                && texture.chain.is_none()
                && !texture.upload.completed()
            {
                texture.stalled = texture.stalled.saturating_add(1);
                if texture.stalled == STALLED_STEPS {
                    warn!(
                        "{}: GPU upload not complete after {STALLED_STEPS} steps \
                         (main image {}, render receipt {}). Loading it again.",
                        texture.label,
                        images.contains(image.id()),
                        if texture.upload.registered() {
                            "registered"
                        } else {
                            "not registered"
                        },
                    );
                }
            }
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
        let jobs = self.primary_loading(chains, server);
        self.account_primary(ledger, jobs, self.pending_primary())?;
        if failure.is_none() && stream.frame().failure.is_none() {
            for index in 0..self.textures.len() {
                let texture = &self.textures[index];
                // A stalled image loads its chain again into the same image.
                let retry = texture.image.is_some() && texture.stalled >= STALLED_STEPS;
                if texture.failed
                    || (texture.persistent && !self.fixed_admitted)
                    || (texture.image.is_some() && !retry)
                    || texture.chain.is_some()
                {
                    continue;
                }
                if ledger.available_external_jobs() == 0 {
                    break;
                }
                // A retry keeps the pending claim of its incomplete upload.
                if !retry && !ledger.can_request_independent(1) {
                    continue;
                }
                self.account_primary(
                    ledger,
                    self.world_primary.counts.jobs + 1,
                    self.pending_primary() + usize::from(!retry),
                )?;
                let texture = &mut self.textures[index];
                let record = Arc::clone(&texture.record);
                let input_bytes = match &record.storage {
                    source::TextureStorage::Rgba8 { input, .. }
                    | source::TextureStorage::NativeBc { input, .. } => input.bytes as usize,
                };
                // Fixed textures load before the game starts. Their top mips load after.
                let defer_top = texture.persistent;
                let half = crate::display_settings::half_textures();
                texture.chain = Some(
                    server
                        .load_builder()
                        .with_settings(move |settings: &mut SourcePrimaryImageSettings| {
                            settings.record = Some(Arc::clone(&record));
                            settings.input_bytes = input_bytes;
                            settings.defer_top = defer_top;
                            settings.half = half;
                        })
                        .load(texture.record.file().to_owned()),
                );
            }
        }
        self.texture_bytes += assemble_textures(&mut self.textures, chains, images);
        let jobs = self.primary_loading(chains, server);
        self.account_primary(ledger, jobs, self.pending_primary())?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
    fn primary_loading(&self, chains: &Assets<SourcePrimaryImage>, server: &AssetServer) -> usize {
        self.textures
            .iter()
            .filter(|t| {
                t.chain.as_ref().is_some_and(|h| {
                    !chains.contains(h.id())
                        && !matches!(server.load_state(h.id()), LoadState::Failed(_))
                })
            })
            .count()
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

impl SourceMaterials {
    fn account_deferred(
        &mut self,
        ledger: &mut PresentationLedger,
        jobs: usize,
    ) -> Result<(), ReferenceError> {
        let counts = RendererCounts {
            jobs,
            pending: self.world_textures.pending(),
        };
        counts.apply(self.world_deferred, ledger)?;
        self.world_deferred = counts;
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
        )?;
        self.account_deferred(ledger, self.world_deferred.jobs)
    }
    pub fn prepare_world_deferred(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        server: &AssetServer,
        images: &mut Assets<Image>,
        batches: &mut Assets<crate::presentation::texture_batches::SourceTextureBatch>,
    ) -> Result<(), ReferenceError> {
        if stream.frame().failure.is_some() {
            self.world_textures.cancel();
        }
        self.account_deferred(ledger, self.world_textures.jobs())?;
        let result = self.world_textures.prepare(server, images, batches, ledger);
        self.account_deferred(ledger, self.world_textures.jobs())?;
        result?;
        let launch = match self.world_textures.queued_jobs() {
            0 => 0,
            queued => ledger.available_external_jobs().min(queued),
        };
        self.account_deferred(ledger, self.world_textures.jobs() + launch)?;
        self.world_textures.start_jobs(server, launch);
        self.account_deferred(ledger, self.world_textures.jobs())
    }
}

impl PrimaryOwnership {}

impl SourceMaterials {
    pub(super) fn select_lighting_textures(
        &mut self,
        reference: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        for t in &reference.render_lighting.textures {
            let file = &t.file;
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
                    || !matches!(&old.storage, source::TextureStorage::Rgba8 { width, height, input }
                        if *width == t.width && *height == t.height && input.file == t.file)
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
                storage: source::TextureStorage::Rgba8 {
                    width: t.width,
                    height: t.height,
                    input: reference.file_catalog.file(file)?.clone(),
                },
                color_space: t.color_space,
                encoding: source::TextureEncoding::Rgba,
                source_encoding: source::TextureEncoding::Rgba,
                uv_channel: 0,
                scale: [1., 1.],
                offset: [0., 0.],
                sampler: t.sampler.clone(),
            };
            let input_staging = primary_staging(&record, &reference.file_catalog)?;
            let entry = SourceTexture::new(record, input_staging, true, file.clone())?;
            self.texture_indices
                .insert(file.clone(), self.textures.len());
            self.textures.push(entry);
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
        serde_json::json!({
            "selected_materials": self.world_primary.materials.iter().collect::<Vec<_>>(),
            "textures": self.textures.iter().map(|t| serde_json::json!({
                "source": t.record.source, "path": t.record.file(), "payload_bytes": t.payload_bytes, "storage": match &t.record.storage { source::TextureStorage::Rgba8 { .. } => "rgba8", source::TextureStorage::NativeBc { format: source::NativeBcFormat::Bc1, .. } => "bc1", source::TextureStorage::NativeBc { .. } => "bc3" },
                "persistent": t.persistent, "users": t.users, "owners": Arc::strong_count(&t.owner),
                "failed": t.failed, "upload_complete": t.upload.completed(),
                "upload_registered": t.upload.registered(), "stalled_steps": t.stalled,
                "cpu_reservation": pending_texture_cpu(t), "input_staging": t.input_staging,
                "image": t.image.as_ref().map(|h| format!("{:?}", h.id())),
                "chain": t.chain.as_ref().map(|h| format!("{:?}", h.id())),
                "load_state": t.chain.as_ref().map(|h| format!("{:?}", server.load_state(h.id()))),
            })).collect::<Vec<_>>(),
        })
    }
}
