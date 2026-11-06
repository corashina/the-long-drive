//! Top mips of fixed native BC textures. At start the loader fills each top mip from mip 1.
//! After the game starts, the real top mips load in top pack order and overwrite mip 0 in place.
//! The texture and its view do not change, so no bind group changes.
use super::SourceMaterials;
use crate::presentation::primary_image::{SourceTopMip, SourceTopMipSettings};
use crate::presentation::upload_budget::{UploadBudget, UploadSpend};
use crate::reference::{FileCatalog, FileReceipt, TextureStorage};
use bevy::{
    asset::{AssetId, LoadState},
    ecs as bevy_ecs,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_asset::{RenderAssets, prepare_assets},
        render_resource::*,
        renderer::RenderQueue,
        texture::GpuImage,
    },
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

/// Top mip loads at the same time.
const JOBS: usize = 4;

struct Target {
    image: AssetId<Image>,
    width: u32,
    height: u32,
    block: usize,
}
struct Job {
    target: Target,
    receipt: FileReceipt,
    handle: Option<Handle<SourceTopMip>>,
}
struct Upload {
    target: Target,
    data: Vec<u8>,
}
#[derive(Default, Clone)]
pub(super) struct TopMips {
    started: bool,
    finished: bool,
    /// Fixed textures that wait for their top mip, in top pack order.
    queue: Arc<Mutex<VecDeque<Job>>>,
    jobs: Arc<Mutex<Vec<Job>>>,
    /// Loaded top mips. The render world takes each one when the frame budget holds it.
    uploads: Arc<Mutex<VecDeque<Upload>>>,
}
impl TopMips {
    /// Bytes of loaded and loading top mips.
    pub fn bytes(&self) -> usize {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .map(|j| j.receipt.bytes as usize)
            .sum::<usize>()
            + self
                .uploads
                .lock()
                .unwrap()
                .iter()
                .map(|u| u.data.len())
                .sum::<usize>()
    }
}

impl SourceMaterials {
    /// Queues the top mips of the fixed textures. Call once when the game starts.
    pub fn start_top_mips(&mut self, catalog: &FileCatalog) {
        if self.top_mips.started {
            return;
        }
        self.top_mips.started = true;
        let order: HashMap<&str, usize> = catalog
            .top_packs
            .iter()
            .flat_map(|p| &p.files)
            .enumerate()
            .map(|(i, f)| (f.as_str(), i))
            .collect();
        // Half texture quality has no top mip to load.
        let half = crate::display_settings::half_textures();
        let mut queue: Vec<_> = self
            .textures
            .iter()
            .filter(|t| t.persistent && !t.failed)
            .filter(|t| t.record.levels(half).is_ok_and(|l| l.skip == 0))
            .filter_map(|t| {
                let TextureStorage::NativeBc {
                    format,
                    width,
                    height,
                    top: Some(top),
                    ..
                } = &t.record.storage
                else {
                    return None;
                };
                Some(Job {
                    target: Target {
                        image: t.image.as_ref()?.id(),
                        width: *width,
                        height: *height,
                        block: format.block_bytes(),
                    },
                    receipt: top.clone(),
                    handle: None,
                })
            })
            .collect();
        queue.sort_by_key(|j| {
            order
                .get(j.receipt.file.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        *self.top_mips.queue.lock().unwrap() = queue.into();
    }
}

/// Starts top mip loads in order and passes loaded ones to the render world.
pub(super) fn load_top_mips(
    server: Res<AssetServer>,
    materials: Option<ResMut<SourceMaterials>>,
    mut tops: ResMut<Assets<SourceTopMip>>,
) {
    let Some(mut materials) = materials else {
        return;
    };
    let state = &mut materials.top_mips;
    if !state.started || state.finished {
        return;
    }
    let mut jobs = state.jobs.lock().unwrap();
    let mut uploads = state.uploads.lock().unwrap();
    let mut queue = state.queue.lock().unwrap();
    let mut index = 0;
    while index < jobs.len() {
        let job = &jobs[index];
        let handle = job.handle.as_ref().unwrap();
        if let LoadState::Failed(error) = server.load_state(handle.id()) {
            // The texture keeps its mip 1 fill. The game still runs.
            warn!("{}: top mip load: {error}", job.receipt.file);
            jobs.swap_remove(index);
        } else if let Some(SourceTopMip(data)) = tops.remove(handle.id()) {
            let target = jobs.swap_remove(index).target;
            uploads.push_back(Upload { target, data });
        } else {
            index += 1;
        }
    }
    while jobs.len() < JOBS
        && let Some(mut job) = queue.pop_front()
    {
        let bytes = job.receipt.bytes as usize;
        job.handle = Some(
            server
                .load_builder()
                .with_settings(move |s: &mut SourceTopMipSettings| s.bytes = bytes)
                .load(job.receipt.file.clone()),
        );
        jobs.push(job);
    }
    if queue.is_empty() && jobs.is_empty() && uploads.is_empty() {
        drop((queue, jobs, uploads));
        state.finished = true;
        // Unread entries of the last top packs are not needed.
        crate::presentation::source_io::release_packs();
    }
}

#[derive(Resource, Default)]
struct RenderTopMips {
    uploads: Option<Arc<Mutex<VecDeque<Upload>>>>,
    spend: UploadSpend,
}
pub(super) struct TopMipPlugin;
impl Plugin for TopMipPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SourceTopMip>()
            .init_asset_loader::<crate::presentation::primary_image::SourceTopMipLoader>()
            .add_systems(Update, load_top_mips);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<RenderTopMips>()
                .add_systems(ExtractSchedule, extract)
                .add_systems(
                    Render,
                    upload
                        .in_set(RenderSystems::PrepareResources)
                        .after(prepare_assets::<GpuImage>),
                );
        }
    }
}
fn extract(
    source: Extract<Option<Res<SourceMaterials>>>,
    budget: Extract<Res<UploadBudget>>,
    mut state: ResMut<RenderTopMips>,
) {
    state.uploads = source
        .as_ref()
        .filter(|s| s.top_mips.started)
        .map(|s| Arc::clone(&s.top_mips.uploads));
    state.spend = budget.spend.clone();
}
/// Writes loaded top mips into mip 0 while the frame budget holds them.
fn upload(state: Res<RenderTopMips>, images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>) {
    let Some(uploads) = &state.uploads else {
        return;
    };
    let mut uploads = uploads.lock().unwrap();
    while let Some(next) = uploads.front() {
        let Some(image) = images.get(next.target.image) else {
            // The fixed image always exists. A new world drops the old materials.
            uploads.pop_front();
            continue;
        };
        if !state.spend.take(next.data.len()) {
            return;
        }
        let Upload { target, data } = uploads.pop_front().unwrap();
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &image.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(target.width / 4 * target.block as u32),
                rows_per_image: Some(target.height / 4),
            },
            Extent3d {
                width: target.width,
                height: target.height,
                depth_or_array_layers: 1,
            },
        );
    }
}
