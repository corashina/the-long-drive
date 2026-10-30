//! Ordered CPU batch owners and GPU acknowledgement.
use crate::{
    ownership::texture_batch::{BatchIdentity, BatchQueue, BatchState},
    presentation::{
        gpu_prepare::GpuPreparation,
        texture_batches::{SourceTextureBatch, SourceTextureBatchSettings},
    },
    reference::texture_batch::TextureBatchReference,
};
use bevy::{
    ecs as bevy_ecs,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_asset::{RenderAssets, prepare_assets},
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
    },
};
use std::sync::{Arc, Mutex};
pub(super) struct Upload {
    pub identity: BatchIdentity,
    pub image: Handle<Image>,
    pub array: usize,
    pub layer: usize,
    pub batch: usize,
    pub reference: TextureBatchReference,
    pub handle: Option<Handle<SourceTextureBatch>>,
    pub payload: Option<SourceTextureBatch>,
    pub error: Option<String>,
    pub preparation: GpuPreparation,
    pub callback_registered: bool,
    pub completion: Arc<Mutex<Option<BatchIdentity>>>,
}
#[derive(Default)]
pub(super) struct UploadQueue {
    pub ownership: BatchQueue,
    pub jobs: [Option<Upload>; 2],
    pub next_request: u64,
}
impl UploadQueue {
    pub fn poll(&mut self, server: &AssetServer, batches: &Assets<SourceTextureBatch>) {
        for job in self.jobs.iter_mut().flatten() {
            let Some(owner) = self.ownership.owner(job.identity).copied() else {
                continue;
            };
            if owner.state == BatchState::Loading {
                let handle = job.handle.as_ref().unwrap();
                if let bevy::asset::LoadState::Failed(error) = server.load_state(handle.id()) {
                    self.ownership.fail(job.identity);
                    job.error = Some(format!(
                        "{}: texture batch load: {error}",
                        job.reference.descriptor.file
                    ));
                } else if let Some(batch) = batches.get(handle) {
                    job.payload = Some(batch.clone()); // Arc clones only. The decoded Vec has one allocation.
                    self.ownership.ready(job.identity);
                }
            }
            if let Some(receipt) = job.completion.lock().unwrap().take() {
                self.ownership.acknowledge(receipt);
            }
        }
    }
    pub fn start(&mut self, server: &AssetServer, free: usize) {
        for identity in self.ownership.start(free) {
            let job = self
                .jobs
                .iter_mut()
                .flatten()
                .find(|j| j.identity == identity)
                .unwrap();
            let descriptor_bytes = job.reference.descriptor.bytes as usize;
            let decoded_bytes = job.reference.decoded_bytes;
            job.handle = Some(
                server
                    .load_builder()
                    .with_settings(move |s: &mut SourceTextureBatchSettings| {
                        s.descriptor_bytes = descriptor_bytes;
                        s.decoded_bytes = decoded_bytes;
                    })
                    .load(job.reference.descriptor.file.clone()),
            );
        }
    }
    pub fn remove(
        &mut self,
        identity: BatchIdentity,
        batches: &mut Assets<SourceTextureBatch>,
    ) -> Option<Upload> {
        self.ownership.release(identity)?;
        let index = self
            .jobs
            .iter()
            .position(|j| j.as_ref().is_some_and(|j| j.identity == identity))
            .unwrap();
        let job = self.jobs[index].take().unwrap();
        if let Some(handle) = &job.handle {
            if !self
                .jobs
                .iter()
                .flatten()
                .any(|j| j.handle.as_ref().is_some_and(|h| h.id() == handle.id()))
            {
                batches.remove(handle.id());
            }
        }
        Some(job)
    }
}
#[derive(Resource, Default)]
struct RenderUploads(Option<Arc<Mutex<UploadQueue>>>);
pub(super) struct WorldTextureUploadPlugin;
impl Plugin for WorldTextureUploadPlugin {
    fn build(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<RenderUploads>()
                .add_systems(ExtractSchedule, extract)
                .add_systems(
                    Render,
                    upload
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_assets::<GpuImage>),
                )
                .add_systems(Render, complete.in_set(RenderSystems::Cleanup));
        }
    }
}
fn extract(
    source: Extract<Option<Res<super::SourceMaterials>>>,
    mut uploads: ResMut<RenderUploads>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
) {
    if let Some(materials) = source.as_deref() {
        for texture in materials.textures.iter() {
            if texture
                .image
                .as_ref()
                .is_some_and(|h| images.get(h.id()).is_some())
                && texture.upload.register()
            {
                let receipt = texture.upload.clone();
                queue.on_submitted_work_done(move || receipt.complete());
            }
        }
    }
    uploads.0 = source
        .as_ref()
        .map(|s| Arc::clone(s.world_textures.upload_queue()));
}
fn upload(
    uploads: Res<RenderUploads>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
    device: Res<RenderDevice>,
) {
    let Some(shared) = &uploads.0 else { return };
    let mut shared = shared.lock().unwrap();
    let Some(owner) = shared.ownership.slots[0] else {
        return;
    };
    if owner.state != BatchState::Ready || owner.cancelled {
        return;
    }
    let job = shared
        .jobs
        .iter()
        .flatten()
        .find(|j| j.identity == owner.identity)
        .unwrap();
    let Some(image) = images.get(job.image.id()) else {
        return;
    };
    let batch = job.payload.as_ref().unwrap();
    let _scope = job.preparation.scope(&device);
    for region in batch.regions.iter() {
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &image.texture,
                mip_level: region.mip,
                origin: Origin3d {
                    x: 0,
                    y: region.row,
                    z: job.layer as u32,
                },
                aspect: TextureAspect::All,
            },
            &batch.payload[region.decoded_offset..region.decoded_offset + region.decoded_bytes],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(region.width * 4),
                rows_per_image: Some(region.rows),
            },
            Extent3d {
                width: region.width,
                height: region.rows,
                depth_or_array_layers: 1,
            },
        );
    }
    if job.preparation.isolated() {
        queue.submit([]);
        job.preparation.submitted(&queue);
    }
    shared.ownership.submit(owner.identity);
}
fn complete(uploads: Res<RenderUploads>, queue: Res<RenderQueue>) {
    let Some(shared) = &uploads.0 else { return };
    let mut shared = shared.lock().unwrap();
    let Some(owner) = shared.ownership.slots[0] else {
        return;
    };
    if owner.state != BatchState::Submitted {
        return;
    }
    let job = shared
        .jobs
        .iter_mut()
        .flatten()
        .find(|j| j.identity == owner.identity)
        .unwrap();
    if !job.callback_registered {
        job.callback_registered = true;
        let completion = Arc::clone(&job.completion);
        let identity = job.identity;
        queue.on_submitted_work_done(move || *completion.lock().unwrap() = Some(identity));
    }
}
