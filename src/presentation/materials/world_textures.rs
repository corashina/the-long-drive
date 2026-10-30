//! Admitted source texture arrays. Each layer retains all authored mip levels.
use crate::ownership::texture_batch::{Admission, BatchIdentity, BatchState, reservation};
use crate::presentation::source_images::wrap;
use crate::presentation::texture_batches::SourceTextureBatch;
use crate::reference::texture_batch::{
    TextureArrayKey as ArrayKey, TextureBatchReference, texture_bank_groups,
};
use crate::reference::{LightingTextureRecord, ReferenceError, SourceId, WorldSourceMaterial};
use bevy::asset::AssetId;
static NEXT_SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::{render_asset::RenderAssets, render_resource::*, texture::GpuImage},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

pub(super) fn bind_arrays(
    result: &mut UnpreparedBindGroup,
    arrays: &[Handle<Image>],
    images: &RenderAssets<GpuImage>,
) -> Result<(), AsBindGroupError> {
    for (index, handle) in arrays.iter().enumerate() {
        let image = images
            .get(handle.id())
            .ok_or(AsBindGroupError::RetryNextUpdate)?;
        result.bindings.0.extend([
            (
                1 + index as u32 * 2,
                OwnedBindingResource::TextureView(
                    TextureViewDimension::D2Array,
                    image.texture_view.clone(),
                ),
            ),
            (
                2 + index as u32 * 2,
                OwnedBindingResource::Sampler(SamplerBindingType::Filtering, image.sampler.clone()),
            ),
        ]);
    }
    Ok(())
}

#[derive(Clone)]
struct Layer {
    source: SourceId,
    path: String,
    hash: String,
    batches: Arc<[TextureBatchReference]>,
}
#[derive(Clone)]
struct TextureArray {
    owner: Arc<()>,
    failed: bool,
    key: ArrayKey,
    layers: Vec<Layer>,
    image: Option<Handle<Image>>,
    users: usize,
    bytes: usize,
    next_layer: usize,
    next_batch: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct TextureBinding {
    pub array: usize,
    pub layer: u32,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}
#[derive(Clone)]
struct MaterialTextures {
    arrays: Vec<usize>,
    slots: HashMap<String, TextureBinding>,
}
#[derive(Clone)]
pub struct WorldTextures {
    uploaded: Arc<Mutex<HashSet<AssetId<Image>>>>,
    uploads: Arc<Mutex<super::world_texture_upload::UploadQueue>>,
    arrays: Vec<Option<TextureArray>>,
    materials: HashMap<SourceId, MaterialTextures>,
    /// Live source layer and bounded queue upload overlap.
    session: u64,
    cancelled: bool,
    gpu_reserved: usize,
}
impl Default for WorldTextures {
    fn default() -> Self {
        Self {
            uploaded: Arc::default(),
            uploads: Arc::default(),
            arrays: Vec::new(),
            materials: HashMap::new(),
            session: NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            cancelled: false,
            gpu_reserved: 0,
        }
    }
}
impl WorldTextures {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn startup_audit(
        &self,
        _images: &Assets<Image>,
        _server: &AssetServer,
    ) -> serde_json::Value {
        let queue = self.uploads.lock().unwrap();
        serde_json::json!({
            "session": self.session,
            "arrays": self.arrays.iter().flatten().map(|a| serde_json::json!({
                "layers": a.layers.len(), "next_layer": a.next_layer, "next_batch": a.next_batch, "failed": a.failed,
                "gpu_bytes": a.bytes, "users": a.users, "owners": Arc::strong_count(&a.owner),
                "image": a.image.as_ref().map(|h| format!("{:?}", h.id())),
                "hashes": a.layers.iter().map(|l| &l.hash).collect::<Vec<_>>(),
                "sources": a.layers.iter().map(|l| &l.source).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "slots": queue.ownership.slots.iter().flatten().map(|o| serde_json::json!({
                "session": o.identity.session, "request": o.identity.request, "state": format!("{:?}", o.state),
                "bytes": o.bytes, "cancelled": o.cancelled,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.uploads.lock().unwrap().ownership.cancel(self.session);
    }
    pub fn drain_ready(
        &mut self,
        server: &AssetServer,
        batches: &mut Assets<SourceTextureBatch>,
    ) -> bool {
        let _ = self.reap(server, batches);
        !self
            .uploads
            .lock()
            .unwrap()
            .jobs
            .iter()
            .flatten()
            .any(|j| j.identity.session == self.session)
    }
    pub fn release_all(&mut self, images: &mut Assets<Image>) {
        for a in self.arrays.drain(..).flatten() {
            if Arc::strong_count(&a.owner) == 1 {
                if let Some(h) = a.image {
                    self.uploaded.lock().unwrap().remove(&h.id());
                    images.remove(h.id());
                }
            }
        }
        self.materials.clear();
        self.gpu_reserved = 0;
    }

    pub fn settled(&self) -> bool {
        self.arrays
            .iter()
            .flatten()
            .all(|a| !a.failed && a.image.is_some() && a.next_layer == a.layers.len())
            && !self
                .uploads
                .lock()
                .unwrap()
                .jobs
                .iter()
                .flatten()
                .any(|j| j.identity.session == self.session)
    }

    pub(crate) fn measurement_images(&self) -> impl Iterator<Item = AssetId<Image>> + '_ {
        self.arrays
            .iter()
            .flatten()
            .filter_map(|a| a.image.as_ref().map(Handle::id))
    }

    pub(super) fn upload_queue(&self) -> &Arc<Mutex<super::world_texture_upload::UploadQueue>> {
        &self.uploads
    }

    pub fn upload_receipts(&self) -> Arc<Mutex<HashSet<AssetId<Image>>>> {
        Arc::clone(&self.uploaded)
    }
    pub fn bytes(&self) -> (usize, usize) {
        (
            self.staging_bytes() + self.metadata_bytes(),
            self.gpu_reserved,
        )
    }

    fn metadata_bytes(&self) -> usize {
        use std::mem::size_of;
        let arrays = self.arrays.capacity() * size_of::<Option<TextureArray>>()
            + self
                .arrays
                .iter()
                .flatten()
                .map(|array| {
                    4096 + array.layers.capacity() * size_of::<Layer>()
                        + array
                            .layers
                            .iter()
                            .map(|layer| {
                                layer.source.file.capacity()
                                    + layer.path.capacity()
                                    + layer.hash.capacity()
                                    + layer.batches.len() * size_of::<TextureBatchReference>()
                                    + layer
                                        .batches
                                        .iter()
                                        .map(|c| {
                                            c.descriptor.file.capacity()
                                                + c.descriptor.sha256.capacity()
                                        })
                                        .sum::<usize>()
                            })
                            .sum::<usize>()
                })
                .sum::<usize>();
        let materials = map_bytes(
            self.materials.capacity(),
            size_of::<(SourceId, MaterialTextures)>(),
        ) + self
            .materials
            .iter()
            .map(|(id, material)| {
                id.file.capacity()
                    + material.arrays.capacity() * size_of::<usize>()
                    + map_bytes(
                        material.slots.capacity(),
                        size_of::<(String, TextureBinding)>(),
                    )
                    + material.slots.keys().map(String::capacity).sum::<usize>()
            })
            .sum::<usize>();
        queue_bytes()
            + arrays
            + materials
            + map_bytes(
                self.uploaded.lock().unwrap().capacity(),
                size_of::<AssetId<Image>>(),
            )
    }

    /// Reserve the complete material before starting any asynchronous load.
    pub fn admit(
        &mut self,
        material: &WorldSourceMaterial,
        textures: &[LightingTextureRecord],
        bank_materials: &[WorldSourceMaterial],
        cpu_available: usize,
        gpu_available: usize,
    ) -> Result<(), ReferenceError> {
        if self.materials.contains_key(&material.source) {
            return Ok(());
        }
        let mut groups = texture_bank_groups(bank_materials, textures)?;
        // Only bind banks used by this material. The bank membership is immutable.
        groups.retain(|(key, _)| {
            material.textures.iter().any(|link| {
                textures
                    .iter()
                    .find(|t| t.id == link.texture)
                    .is_some_and(|t| ArrayKey::from(t) == *key)
            })
        });
        let existing: Vec<_> = groups
            .iter()
            .map(|(key, layers)| {
                self.arrays.iter().position(|array| {
                    array.as_ref().is_some_and(|array| {
                        array.key == *key
                            && array.layers.len() == layers.len()
                            && array.layers.iter().zip(layers).all(|(a, b)| {
                                a.source == b.source
                                    && a.hash == b.mip_data.sha256
                                    && a.path == b.mip_data.path
                            })
                    })
                })
            })
            .collect();
        let mut added = 0usize;

        for ((key, layers), existing) in groups.iter().zip(&existing) {
            if existing.is_none() {
                let bytes = key
                    .layer_bytes()?
                    .checked_mul(layers.len())
                    .ok_or_else(|| ReferenceError("Source texture array size overflow".into()))?;
                added = added
                    .checked_add(bytes)
                    .ok_or_else(|| ReferenceError("Source texture allocation overflow".into()))?;
            }
        }
        // Reserve binding metadata. Layer buffers are admitted when loading starts.
        let metadata = groups
            .iter()
            .zip(&existing)
            .filter(|(_, old)| old.is_none())
            .map(|((_, layers), _)| {
                4096 + std::mem::size_of::<TextureArray>() * 4
                    + layers
                        .iter()
                        .map(|layer| {
                            std::mem::size_of::<Layer>()
                                + layer.source.file.len()
                                + layer.mip_data.path.len()
                                + layer.mip_data.sha256.len()
                                + layer.batches.len() * std::mem::size_of::<TextureBatchReference>()
                                + layer
                                    .batches
                                    .iter()
                                    .map(|c| c.descriptor.file.len() + c.descriptor.sha256.len())
                                    .sum::<usize>()
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
            + map_bytes(
                (self.materials.len() + 1).next_power_of_two() * 2,
                std::mem::size_of::<(SourceId, MaterialTextures)>(),
            )
            + map_bytes(
                material.textures.len().next_power_of_two() * 2,
                std::mem::size_of::<(String, TextureBinding)>(),
            )
            + material
                .textures
                .iter()
                .map(|slot| slot.property.len())
                .sum::<usize>()
            + material.source.file.len()
            + groups.len() * std::mem::size_of::<usize>();
        let cpu = Some(self.staging_bytes());
        let gpu = added.checked_add(self.gpu_reserved);
        if cpu.is_none_or(|v| {
            v.saturating_add(self.metadata_bytes())
                .saturating_add(metadata)
                > cpu_available
        }) || gpu.is_none_or(|v| v > gpu_available)
        {
            return Err(ReferenceError(format!(
                "Source texture budget {}:{}: CPU {} / {}, GPU {} / {} bytes",
                material.source.file,
                material.source.path_id,
                self.staging_bytes()
                    .saturating_add(self.metadata_bytes())
                    .saturating_add(metadata),
                cpu_available,
                self.gpu_reserved.saturating_add(added),
                gpu_available
            )));
        }
        self.gpu_reserved = gpu.unwrap();
        let mut indices = Vec::with_capacity(groups.len());
        for ((key, records), existing) in groups.into_iter().zip(existing) {
            let index = if let Some(index) = existing {
                self.arrays[index].as_mut().unwrap().users += 1;
                index
            } else {
                let bytes = key.layer_bytes()? * records.len();
                let layers = records
                    .into_iter()
                    .map(|record| Layer {
                        source: record.source.clone(),
                        path: record.mip_data.path.clone(),
                        hash: record.mip_data.sha256.clone(),
                        batches: record.batches.clone().into(),
                    })
                    .collect();
                let array = Some(TextureArray {
                    owner: Arc::new(()),
                    failed: false,
                    key,
                    layers,
                    image: None,
                    users: 1,
                    bytes,
                    next_layer: 0,
                    next_batch: 0,
                });
                if let Some(index) = self.arrays.iter().position(Option::is_none) {
                    self.arrays[index] = array;
                    index
                } else {
                    self.arrays.push(array);
                    self.arrays.len() - 1
                }
            };
            indices.push(index);
        }
        let mut slots = HashMap::with_capacity(material.textures.len());
        for link in &material.textures {
            let record = textures.iter().find(|t| t.id == link.texture).unwrap();
            let (array, layer) = indices
                .iter()
                .find_map(|&index| {
                    self.arrays[index]
                        .as_ref()
                        .unwrap()
                        .layers
                        .iter()
                        .position(|l| l.source == record.source)
                        .map(|layer| (index, layer))
                })
                .unwrap();
            slots.insert(
                link.property.clone(),
                TextureBinding {
                    array,
                    layer: layer as u32,
                    scale: link.scale,
                    offset: link.offset,
                },
            );
        }
        self.materials.insert(
            material.source.clone(),
            MaterialTextures {
                arrays: indices,
                slots,
            },
        );
        Ok(())
    }
    fn reap(
        &mut self,
        server: &AssetServer,
        batches: &mut Assets<SourceTextureBatch>,
    ) -> Result<(), ReferenceError> {
        let mut queue = self.uploads.lock().unwrap();
        queue.poll(server, batches);
        let mut failure = None;
        // Current first. The second owner cannot advance array progress before it.
        let identities = queue.ownership.slots.map(|o| o.map(|o| o.identity));
        for identity in identities
            .into_iter()
            .flatten()
            .filter(|id| id.session == self.session)
        {
            let owner = *queue.ownership.owner(identity).unwrap();
            if let Some(job) = queue.remove(identity, batches) {
                let array = self.arrays[job.array].as_mut().unwrap();
                if owner.state == BatchState::Failed {
                    array.failed = true;
                    failure = job.error.map(ReferenceError);
                } else if !owner.cancelled {
                    if array.next_layer != job.layer || array.next_batch != job.batch {
                        return Err(ReferenceError("Texture batch completion order".into()));
                    }
                    array.next_batch += 1;
                    if array.next_batch == array.layers[array.next_layer].batches.len() {
                        array.next_layer += 1;
                        array.next_batch = 0;
                    }
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
    pub fn prepare(
        &mut self,
        server: &AssetServer,
        images: &mut Assets<Image>,
        batches: &mut Assets<SourceTextureBatch>,
        preparation: &crate::presentation::gpu_prepare::GpuPreparation,
        ledger: &mut crate::ownership::PresentationLedger,
    ) -> Result<(), ReferenceError> {
        self.reap(server, batches)?;
        if self.cancelled {
            return Ok(());
        }
        let metadata = self.metadata_bytes();
        let cpu_available = ledger.available_prototype_cpu_bytes() + self.bytes().0;
        let mut queue = self.uploads.lock().unwrap();
        let current = queue.ownership.slots[0].and_then(|owner| {
            queue
                .jobs
                .iter()
                .flatten()
                .find(|j| j.identity == owner.identity)
                .map(|job| (owner, job.array, job.layer, job.batch))
        });
        let empty = current.is_none();
        let refill = current
            .as_ref()
            .is_some_and(|(owner, ..)| owner.identity.session == self.session && !owner.cancelled)
            && queue.ownership.slots[1].is_none();
        if empty || refill {
            let count = if empty { 2 } else { 1 };
            let mut selected = Vec::new();
            'arrays: for (index, array) in self
                .arrays
                .iter()
                .enumerate()
                .filter_map(|(i, a)| a.as_ref().map(|a| (i, a)))
            {
                if array.failed {
                    continue;
                }
                for layer in array.next_layer..array.layers.len() {
                    let first = if layer == array.next_layer {
                        array.next_batch
                    } else {
                        0
                    };
                    for batch in first..array.layers[layer].batches.len() {
                        if current
                            .as_ref()
                            .is_some_and(|(_, a, l, b)| (index, layer, batch) <= (*a, *l, *b))
                        {
                            continue;
                        }
                        let reference = &array.layers[layer].batches[batch];
                        let bytes = reservation(reference).ok_or_else(|| {
                            ReferenceError(format!(
                                "{}: batch reservation overflow",
                                reference.descriptor.file
                            ))
                        })?;
                        queue.next_request += 1;
                        selected.push((
                            index,
                            layer,
                            batch,
                            BatchIdentity {
                                session: self.session,
                                request: queue.next_request,
                            },
                            bytes,
                        ));
                        if selected.len() == count {
                            break 'arrays;
                        }
                    }
                }
            }
            if selected.is_empty() {
                return Ok(());
            }
            // Every request runs, so the report asks for all credit of the batches at once.
            let claims = ledger.can_request_independent(selected.len());
            let shared = if let Some((owner, _, _, _)) = &current {
                let path = &queue
                    .jobs
                    .iter()
                    .flatten()
                    .find(|j| j.identity == owner.identity)
                    .unwrap()
                    .reference
                    .descriptor
                    .file;
                let (a, l, b, identity, _) = selected[0];
                (self.arrays[a].as_ref().unwrap().layers[l].batches[b]
                    .descriptor
                    .file
                    == *path)
                    .then_some((owner.identity, identity, owner.bytes - 4096, 0))
            } else if selected.len() == 2 {
                let (a, l, b, first, bytes) = selected[0];
                let (c, m, d, second, _) = selected[1];
                (self.arrays[a].as_ref().unwrap().layers[l].batches[b]
                    .descriptor
                    .file
                    == self.arrays[c].as_ref().unwrap().layers[m].batches[d]
                        .descriptor
                        .file)
                    .then_some((first, second, bytes - 4096, 1))
            } else {
                None
            };
            if let Some((_, _, _, index)) = shared {
                selected[index].4 = 4096;
            }
            let available = ledger
                .available_staging_bytes()
                .min(cpu_available.saturating_sub(metadata + queue.ownership.bytes()));
            let requested = selected.iter().map(|s| s.4).sum::<usize>();
            // Short credit waits. The report asks the stream for the rest.
            ledger.request_staging(requested);
            ledger.request_prototype(requested, 0);
            if !claims {
                return Ok(());
            }
            let admission = if refill {
                queue
                    .ownership
                    .refill(selected[0].3, selected[0].4, available)
            } else {
                queue
                    .ownership
                    .admit(selected.iter().map(|s| (s.3, s.4)), available, true)
            };
            match admission {
                Ok(()) => (),
                Err(Admission::Wait) => return Ok(()),
                Err(Admission::Permanent) => {
                    let (i, l, b, _, _) = selected[0];
                    let source = &self.arrays[i].as_ref().unwrap().layers[l];
                    return Err(ReferenceError(format!(
                        "{}:{} {}: batch pair admission requested {} bytes, available {} bytes",
                        source.source.file,
                        source.source.path_id,
                        source.batches[b].descriptor.file,
                        selected.iter().map(|s| s.4).sum::<usize>(),
                        available
                    )));
                }
            }
            if let Some((first, second, bytes, _)) = shared {
                queue.ownership.share(first, second, bytes);
            }
            for (index, layer, batch, identity, _) in selected {
                let slot = queue.jobs.iter().position(Option::is_none).unwrap();
                let array = self.arrays[index].as_mut().unwrap();
                if array.image.is_none() {
                    array.image = Some(images.add(array_image(array.key, array.layers.len())?));
                }
                queue.jobs[slot] = Some(super::world_texture_upload::Upload {
                    identity,
                    image: array.image.as_ref().unwrap().clone(),
                    array: index,
                    layer,
                    batch,
                    reference: array.layers[layer].batches[batch].clone(),
                    handle: None,
                    payload: None,
                    error: None,
                    preparation: preparation.clone(),
                    callback_registered: false,
                    completion: Arc::default(),
                });
            }
        }
        Ok(())
    }
    pub fn queued_jobs(&self) -> usize {
        self.uploads
            .lock()
            .unwrap()
            .ownership
            .slots
            .iter()
            .flatten()
            .filter(|o| {
                o.identity.session == self.session && o.state == BatchState::Queued && !o.cancelled
            })
            .count()
    }
    pub fn start_jobs(&mut self, server: &AssetServer, free: usize) {
        let mut queue = self.uploads.lock().unwrap();
        if queue
            .ownership
            .slots
            .iter()
            .flatten()
            .any(|o| o.identity.session == self.session && !o.cancelled)
        {
            queue.start(server, free);
        }
    }
    pub fn jobs(&self) -> usize {
        self.uploads
            .lock()
            .unwrap()
            .ownership
            .slots
            .iter()
            .flatten()
            .filter(|o| o.identity.session == self.session && o.state == BatchState::Loading)
            .count()
    }
    pub fn pending(&self) -> usize {
        self.uploads
            .lock()
            .unwrap()
            .ownership
            .slots
            .iter()
            .flatten()
            .filter(|o| o.identity.session == self.session)
            .count()
    }
    pub fn staging_bytes(&self) -> usize {
        self.uploads
            .lock()
            .unwrap()
            .ownership
            .slots
            .iter()
            .flatten()
            .filter(|o| o.identity.session == self.session)
            .map(|o| o.bytes)
            .sum()
    }
    pub fn admitted(&self, material: &SourceId) -> bool {
        self.materials.contains_key(material)
    }
    pub fn sampling(&self, array: usize) -> Option<Vec4> {
        let key = self.arrays.get(array)?.as_ref()?.key;
        Some(Vec4::new(
            f32::from_bits(key.bias),
            (key.wrap_u == 3) as u8 as f32,
            (key.wrap_v == 3) as u8 as f32,
            (key.mips - 1) as f32,
        ))
    }
    pub fn binding(
        &self,
        material: &SourceId,
        property: &str,
    ) -> Option<(TextureBinding, &Handle<Image>)> {
        let binding = *self.materials.get(material)?.slots.get(property)?;
        Some((
            binding,
            self.arrays[binding.array].as_ref()?.image.as_ref()?,
        ))
    }
    /// Every slot of the material has a complete array image, so `binding` resolves.
    pub fn ready(&self, material: &SourceId) -> bool {
        self.materials.get(material).is_some_and(|m| {
            m.slots.values().all(|b| {
                self.arrays[b.array].as_ref().is_some_and(|a| {
                    !a.failed && a.image.is_some() && a.next_layer == a.layers.len()
                })
            })
        })
    }
    pub fn retirable(
        &self,
        material: &SourceId,
        _images: &Assets<Image>,
        _server: &AssetServer,
    ) -> bool {
        let queue = self.uploads.lock().unwrap();
        self.materials.get(material).is_none_or(|m| {
            m.arrays.iter().all(|&i| {
                self.arrays[i].as_ref().is_none_or(|a| {
                    a.users > 1
                        || !queue
                            .jobs
                            .iter()
                            .flatten()
                            .any(|j| j.identity.session == self.session && j.array == i)
                })
            })
        })
    }
    pub fn release(
        &mut self,
        material: &SourceId,
        images: &mut Assets<Image>,
        server: &AssetServer,
    ) -> Result<(), ReferenceError> {
        if !self.retirable(material, images, server) {
            return Err(ReferenceError(
                "Source texture retirement still has pending work".into(),
            ));
        }
        let Some(material) = self.materials.remove(material) else {
            return Ok(());
        };
        for index in material.arrays {
            let array = self.arrays[index].as_mut().unwrap();
            array.users -= 1;
            if array.users > 0 {
                continue;
            }
            let array = self.arrays[index].take().unwrap();
            if let Some(image) = array.image.filter(|_| Arc::strong_count(&array.owner) == 1) {
                self.uploaded.lock().unwrap().remove(&image.id());
                images.remove(image.id());
            }
            // Dropping layer handles releases this owner without invalidating other users.
            drop(array.layers);
            self.gpu_reserved -= array.bytes;
        }
        Ok(())
    }
}

fn queue_bytes() -> usize {
    std::mem::size_of::<Mutex<super::world_texture_upload::UploadQueue>>()
        + 2 * std::mem::size_of::<usize>()
}

// Hash-table allocation upper bound from capacity, including control bytes.
fn map_bytes(capacity: usize, entry: usize) -> usize {
    if capacity == 0 {
        0
    } else {
        capacity
            .saturating_mul(2)
            .saturating_mul(entry + 1)
            .saturating_add(16)
    }
}

fn array_image(key: ArrayKey, layers: usize) -> Result<Image, ReferenceError> {
    let mut image = Image::new_uninit(
        Extent3d {
            width: key.width,
            height: key.height,
            depth_or_array_layers: layers as u32,
        },
        TextureDimension::D2,
        if key.srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = key.mips;
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    let filter = if key.filter == 0 {
        ImageFilterMode::Nearest
    } else {
        ImageFilterMode::Linear
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: wrap(key.wrap_u),
        address_mode_v: wrap(key.wrap_v),
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: if key.filter == 2 || key.aniso > 1 {
            ImageFilterMode::Linear
        } else {
            ImageFilterMode::Nearest
        },
        anisotropy_clamp: u16::try_from(key.aniso.max(1))
            .map_err(|_| ReferenceError("Source anisotropy exceeds GPU sampler range".into()))?,
        lod_max_clamp: (key.mips - 1) as f32,
        ..default()
    });
    Ok(image)
}
