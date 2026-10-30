//! GPU prop rows in the render world: upload, cull per view, and indirect draws.
//! Pass 0 is the camera. Passes 1-4 are the shadow cascades, 5-8 the lamp shadow slots.
use super::{
    caster::CasterPasses,
    lighting_assets::LightingRenderAssets,
    readiness::RendererStateHandle,
    render_bundles::{BundleTarget, DrawBundle},
    render_targets::{DEPTH_FORMAT, HDR_FORMAT, SHADOW_FORMAT},
    shadow_cache::SpecializationKey,
    shadow_scene::ShadowScene,
    surface_pass::SurfacePasses,
    surface_views::{SourceSurfaceView, VIEW_COUNT},
    view_frame::SourceViewFrame,
    world::gpu_props::{KEY_WORD, NO_BATCH, PropBatch, ROW_WORDS, word_batch},
};
use crate::presentation::materials::{SourceMaterial, impostor::ImpostorMaterial};
use crate::presentation::shadow_scene::CasterKey;
use bevy::material::{
    MaterialProperties,
    key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey},
};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    MaterialBindGroupAllocators, MeshPipelineKey, PreparedMaterial, ViewKeyCache,
    alpha_mode_pipeline_key, base_specialize,
};
use bevy::render::{
    MainWorld,
    erased_render_asset::ErasedRenderAssets,
    mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
    render_asset::RenderAssets,
    render_phase::TrackedRenderPass,
    render_resource::{binding_types::*, *},
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
    view::{ExtractedView, ViewUniformOffset},
};
use bevy::shader::ShaderCacheError;
use bevy::{ecs as bevy_ecs, prelude::*};
use std::any::TypeId;
use std::num::NonZeroU64;
use std::ops::Range;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicU32, Ordering},
};

const PASSES: usize = 9;
const ALIGN: u64 = 256;
const ROW_BYTES: u64 = ROW_WORDS as u64 * 4;
const ARGS_BYTES: u64 = 20;
/// Planes, counts and view index of `CullView` in source_props_cull.wgsl.
const CULL_WORDS: usize = 32;
/// Visible rows per pass. Sixteen words per view.
const TOTAL_WORDS: usize = 16;
/// More dirty ranges than this upload all rows. Fading rows give one range per few rows.
const UPLOAD_RANGES: usize = 256;

fn align(bytes: u64) -> u64 {
    bytes.div_ceil(ALIGN) * ALIGN
}
/// The first `size` bytes of a buffer. Dynamic offsets move this window.
fn sized(buffer: &Buffer, size: u64) -> BindingResource<'_> {
    BindingResource::Buffer(BufferBinding {
        buffer,
        offset: 0,
        size: NonZeroU64::new(size),
    })
}

fn compiled(cache: &PipelineCache, id: CachedRenderPipelineId) -> Result<bool, String> {
    match cache.get_render_pipeline_state(id) {
        CachedPipelineState::Ok(_) => Ok(true),
        CachedPipelineState::Err(
            ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable,
        ) => Ok(false),
        CachedPipelineState::Err(error) => Err(format!("Source GPU prop pipeline: {error}")),
        _ => Ok(false),
    }
}
/// A row pipeline is ready with its additive and caster companions.
/// A world shader, such as the impostor, has no additive companion.
fn pipeline_state(
    id: CachedRenderPipelineId,
    world_shader: bool,
    cache: &PipelineCache,
    surface: &SurfacePasses,
    caster: &CasterPasses,
) -> Result<bool, String> {
    let mut ready = compiled(cache, id)?;
    // The companion passes inspect a pipeline after its publication.
    if !world_shader {
        ready &= surface.companion_state(id, cache).unwrap_or(Ok(false))?;
    }
    ready &= caster.classified(id) && caster.companion_state(id, cache).unwrap_or(Ok(true))?;
    Ok(ready)
}

/// Row group of the GPU prop pipelines: rows, visible list of one pass, and batch offset.
/// It replaces the mesh group of a source surface material with `gpu_props`.
pub fn draw_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_prop_draw",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::VERTEX_FRAGMENT,
            (
                (50, storage_buffer_read_only_sized(false, None)),
                (51, storage_buffer_read_only_sized(true, None)),
                (52, uniform_buffer_sized(true, NonZeroU64::new(16))),
            ),
        ),
    )
}
fn cull_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_prop_cull",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only_sized(false, None),
                storage_buffer_read_only_sized(false, None),
                storage_buffer_sized(true, None),
                storage_buffer_sized(true, None),
                storage_buffer_sized(false, None),
                uniform_buffer_sized(true, NonZeroU64::new(CULL_WORDS as u64 * 4)),
                storage_buffer_read_only_sized(true, None),
            ),
        ),
    )
}

/// One view's resolved pipelines for each batch.
#[derive(Clone, Copy, Default)]
struct BatchPipelines {
    base: Option<CachedRenderPipelineId>,
    partner: Option<CachedRenderPipelineId>,
    caster: Option<CachedRenderPipelineId>,
}
struct Specialization {
    key: SpecializationKey,
    // Keep the pointer keys alive.
    _layout: MeshVertexBufferLayoutRef,
    _properties: Arc<MaterialProperties>,
    base: CachedRenderPipelineId,
}
#[derive(Default)]
struct ViewState {
    specializations: Vec<Option<Specialization>>,
    pipelines: Vec<BatchPipelines>,
    /// Rows of each batch in each caster mask bit. Empty bits draw nothing.
    cascades: Vec<[u32; 8]>,
    masks: Option<(u64, u64, u64, u64, u32)>,
    /// Draw attempts and issued draws in this frame.
    draws: [AtomicU32; 2],
    /// Draws of the camera, then of each caster slot. Batches change rarely, so the same bundle runs each frame.
    bundles: [Mutex<DrawBundle>; 1 + super::lamp_shadows::TILES],
}
/// Batch draw data from the mesh allocator.
#[derive(Clone, Copy, PartialEq)]
struct Template {
    gpu: [u32; 4],
    format: IndexFormat,
}
struct Buffers {
    rows: Buffer,
    batches: Buffer,
    batch_offsets: Buffer,
    args: Buffer,
    masks: Buffer,
    cull_views: Buffer,
    totals: Buffer,
    row_capacity: u32,
    batch_capacity: u32,
    /// Entries of one visible list. A multiple of 64, so each list starts on 256 bytes.
    stride: u32,
    args_region: u64,
    visible_region: u64,
    mask_region: u64,
    cull: BindGroup,
    draw: BindGroup,
}
impl Buffers {
    fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        rows: u32,
        batches: u32,
        stride: u32,
    ) -> Self {
        let buffer = |label: &'static str, size: u64, usage: BufferUsages| {
            device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let args_region = align(PASSES as u64 * u64::from(batches) * ARGS_BYTES);
        let visible_region = PASSES as u64 * u64::from(stride) * 4;
        let mask_region = align(u64::from(rows));
        let views = VIEW_COUNT as u64;
        let copy = BufferUsages::COPY_DST;
        let rows_buffer = buffer(
            "source_prop_rows",
            u64::from(rows) * ROW_BYTES,
            BufferUsages::STORAGE | copy,
        );
        let batch_buffer = buffer(
            "source_prop_batches",
            u64::from(batches) * 16,
            BufferUsages::STORAGE | copy,
        );
        let batch_offsets = buffer(
            "source_prop_batch_offsets",
            u64::from(batches) * ALIGN,
            BufferUsages::UNIFORM | copy,
        );
        let args = buffer(
            "source_prop_args",
            views * args_region,
            BufferUsages::STORAGE | BufferUsages::INDIRECT,
        );
        let visible = buffer(
            "source_prop_visible",
            views * visible_region,
            BufferUsages::STORAGE,
        );
        let masks = buffer(
            "source_prop_masks",
            views * mask_region,
            BufferUsages::STORAGE | copy,
        );
        let cull_views = buffer(
            "source_prop_cull_views",
            views * ALIGN,
            BufferUsages::UNIFORM | copy,
        );
        let totals = buffer(
            "source_prop_totals",
            views * TOTAL_WORDS as u64 * 4,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        );
        let cull = device.create_bind_group(
            "source_prop_cull",
            &cache.get_bind_group_layout(&cull_layout()),
            &BindGroupEntries::sequential((
                rows_buffer.as_entire_binding(),
                batch_buffer.as_entire_binding(),
                sized(&args, args_region),
                sized(&visible, visible_region),
                totals.as_entire_binding(),
                sized(&cull_views, CULL_WORDS as u64 * 4),
                sized(&masks, mask_region),
            )),
        );
        let draw = device.create_bind_group(
            "source_prop_draw",
            &cache.get_bind_group_layout(&draw_layout()),
            &BindGroupEntries::with_indices((
                (50, rows_buffer.as_entire_binding()),
                (51, sized(&visible, u64::from(stride) * 4)),
                (52, sized(&batch_offsets, 16)),
            )),
        );
        Self {
            rows: rows_buffer,
            batches: batch_buffer,
            batch_offsets,
            args,
            masks,
            cull_views,
            totals,
            row_capacity: rows,
            batch_capacity: batches,
            stride,
            args_region,
            visible_region,
            mask_region,
            cull,
            draw,
        }
    }
    fn list_offset(&self, view: usize, pass: usize) -> u32 {
        (view as u64 * self.visible_region + pass as u64 * u64::from(self.stride) * 4) as u32
    }
    fn args_offset(&self, view: usize, pass: usize, batch: usize, batches: usize) -> u64 {
        view as u64 * self.args_region + ((pass * batches + batch) as u64) * ARGS_BYTES
    }
}
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;
/// F3 readback of the visible row totals. One copy is in flight at a time.
struct Readback {
    buffer: Buffer,
    state: Arc<AtomicU8>,
    totals: [[u32; TOTAL_WORDS]; VIEW_COUNT],
}

#[derive(Resource)]
pub struct PropDraws {
    query: QueryState<(&'static SourceSurfaceView, &'static ExtractedView)>,
    // Main-world rows, copied in extraction.
    owner: u64,
    words: Vec<u32>,
    dirty: Vec<Range<u32>>,
    full: bool,
    live: u32,
    visible_len: u32,
    batches: Vec<PropBatch>,
    batch_revision: Option<u64>,
    batch_upload: bool,
    // Render state.
    templates: Vec<Option<Template>>,
    buffers: Option<Buffers>,
    views: [ViewState; VIEW_COUNT],
    mask_bytes: Vec<u8>,
    cull: Option<[CachedComputePipelineId; 2]>,
    readback: Option<Readback>,
    pub ready: [bool; VIEW_COUNT],
    pub frame: u32,
}
impl FromWorld for PropDraws {
    fn from_world(world: &mut World) -> Self {
        Self {
            query: world.query(),
            owner: 0,
            words: Vec::new(),
            dirty: Vec::new(),
            full: false,
            live: 0,
            visible_len: 0,
            batches: Vec::new(),
            batch_revision: None,
            batch_upload: false,
            templates: Vec::new(),
            buffers: None,
            views: std::array::from_fn(|_| ViewState::default()),
            mask_bytes: Vec::new(),
            cull: None,
            readback: None,
            ready: [false; VIEW_COUNT],
            frame: 0,
        }
    }
}
impl PropDraws {
    /// Rows, camera-visible rows and issued indirect draws of one view, for F3.
    pub fn audit(&self, view: usize) -> [u32; 3] {
        [
            self.live,
            self.readback.as_ref().map_or(0, |r| r.totals[view][0]),
            self.views[view].draws[1].load(Ordering::Relaxed),
        ]
    }
    fn rows(&self) -> u32 {
        (self.words.len() / ROW_WORDS) as u32
    }
}

/// Copy the dirty rows of the active instances. A new owner uploads every row.
pub(crate) fn extract(mut main: ResMut<MainWorld>, mut draws: ResMut<PropDraws>) {
    let Some(mut instances) = main.get_resource_mut::<super::world::instances::SourceInstances>()
    else {
        return;
    };
    let props = instances.bypass_change_detection().props_mut();
    let draws = &mut *draws;
    let start = draws.dirty.len();
    props.take_dirty(&mut draws.dirty);
    let rows = props.rows();
    draws.words.resize(rows.len() * ROW_WORDS, 0);
    if draws.owner != props.id() {
        draws.owner = props.id();
        draws.full = true;
        draws.batch_revision = None;
        draws.dirty.clear();
        for (slot, row) in rows.iter().enumerate() {
            draws.words[slot * ROW_WORDS..(slot + 1) * ROW_WORDS].copy_from_slice(&row.words());
        }
    } else {
        for range in &draws.dirty[start..] {
            let (start, end) = (range.start as usize, range.end as usize);
            for (words, row) in draws.words[start * ROW_WORDS..end * ROW_WORDS]
                .as_chunks_mut::<ROW_WORDS>()
                .0
                .iter_mut()
                .zip(&rows[start..end])
            {
                *words = row.words();
            }
        }
        // Ranges collect while the buffers wait. Many ranges or rows become one full upload.
        let covered = draws.dirty.iter().map(|r| r.len()).sum::<usize>();
        if draws.full || draws.dirty.len() > UPLOAD_RANGES || 2 * covered > rows.len() {
            draws.dirty.clear();
            draws.full = true;
        }
    }
    draws.live = props.live();
    draws.visible_len = props.visible_len();
    if draws.batch_revision != Some(props.batch_revision()) {
        draws.batch_revision = Some(props.batch_revision());
        draws.batches.clear();
        draws.batches.extend_from_slice(props.batches());
        draws.batch_upload = true;
    }
}

pub(crate) fn prepare(world: &mut World) {
    world.resource_scope(|world, mut draws: Mut<PropDraws>| {
        let result = draws.prepare(world);
        if let Err(error) = result {
            world.resource::<RendererStateHandle>().fail(error);
        }
    });
}
impl PropDraws {
    fn prepare(&mut self, world: &mut World) -> Result<(), String> {
        self.ready = [false; VIEW_COUNT];
        self.frame = 0;
        if world.resource::<RendererStateHandle>().failure().is_some() {
            return Ok(());
        }
        self.poll_readback();
        let frame = world.resource::<SourceViewFrame>().frame;
        if self.live == 0 {
            self.ready = [true; VIEW_COUNT];
            self.frame = frame;
            return Ok(());
        }
        if !self.prepare_cull(world)? || !self.prepare_templates(world) {
            return Ok(());
        }
        if world.resource::<ShadowScene>().frame != frame {
            return Ok(());
        }
        self.prepare_buffers(world);
        let mut views = [None; VIEW_COUNT];
        for (view, extracted) in self.query.iter(world) {
            if world.resource::<SourceViewFrame>().views[view.index].is_none() {
                continue;
            }
            let Some(key) = world
                .resource::<ViewKeyCache>()
                .get(&extracted.retained_view_entity)
                .copied()
            else {
                return Ok(());
            };
            views[view.index] = Some(key);
        }
        for (index, key) in views.into_iter().enumerate() {
            let Some(key) = key else {
                continue;
            };
            self.ready[index] = self.prepare_view(world, index, key)?;
        }
        self.frame = frame;
        Ok(())
    }
    fn prepare_cull(&mut self, world: &World) -> Result<bool, String> {
        let cache = world.resource::<PipelineCache>();
        let Some(ids) = self.cull else {
            let Some(assets) = world.get_resource::<LightingRenderAssets>() else {
                return Ok(false);
            };
            self.cull = Some(["reset", "cull"].map(|entry| {
                cache.queue_compute_pipeline(ComputePipelineDescriptor {
                    label: Some("source_prop_cull".into()),
                    layout: vec![cull_layout()],
                    shader: assets.shaders.props_cull.clone(),
                    entry_point: Some(entry.into()),
                    zero_initialize_workgroup_memory: true,
                    ..default()
                })
            }));
            return Ok(false);
        };
        for id in ids {
            match cache.get_compute_pipeline_state(id) {
                CachedPipelineState::Ok(_) => {}
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => return Ok(false),
                CachedPipelineState::Err(error) => {
                    return Err(format!("Source GPU prop cull pipeline: {error}"));
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
    /// Index range and base vertex of each batch mesh. A missing mesh waits.
    fn prepare_templates(&mut self, world: &World) -> bool {
        let meshes = world.resource::<RenderAssets<RenderMesh>>();
        let allocator = world.resource::<MeshAllocator>();
        self.templates.resize(self.batches.len(), None);
        let mut changed = false;
        for (batch, template) in self.batches.iter().zip(&mut self.templates) {
            // Hidden batches wait for their mesh without blocking the view.
            let next = (batch.shown != 0).then(|| {
                let mesh = meshes.get(batch.key.mesh)?;
                let RenderMeshBufferInfo::Indexed {
                    count,
                    index_format,
                } = mesh.buffer_info
                else {
                    return None;
                };
                let vertices = allocator.mesh_vertex_slice(&batch.key.mesh)?;
                let indices = allocator.mesh_index_slice(&batch.key.mesh)?;
                Some(Template {
                    gpu: [
                        batch.first,
                        count,
                        indices.range.start,
                        vertices.range.start,
                    ],
                    format: index_format,
                })
            });
            let next = match next {
                Some(None) => return false,
                Some(next) => next,
                None => None,
            };
            changed |= *template != next;
            *template = next;
        }
        self.batch_upload |= changed;
        true
    }
    fn prepare_buffers(&mut self, world: &World) {
        let device = world.resource::<RenderDevice>();
        let queue = world.resource::<RenderQueue>();
        let rows = self.rows();
        let batches = self.batches.len() as u32;
        let stride = self.visible_len.max(1).div_ceil(64) * 64;
        let grow = self.buffers.as_ref().is_none_or(|b| {
            b.row_capacity < rows || b.batch_capacity < batches || b.stride < stride
        });
        if grow {
            let old = self.buffers.as_ref();
            let rows = rows
                .max(old.map_or(256, |b| b.row_capacity))
                .next_power_of_two();
            let batches = batches
                .max(old.map_or(16, |b| b.batch_capacity))
                .next_power_of_two();
            let stride = stride.max(old.map_or(64, |b| b.stride)).next_power_of_two();
            self.buffers = Some(Buffers::new(
                device,
                world.resource::<PipelineCache>(),
                rows,
                batches,
                stride,
            ));
            self.full = true;
            self.batch_upload = true;
            for view in &mut self.views {
                view.masks = None;
            }
        }
        let buffers = self.buffers.as_ref().unwrap();
        if self.full {
            queue.write_buffer(&buffers.rows, 0, bytemuck::cast_slice(&self.words));
            self.full = false;
        } else {
            for range in &self.dirty {
                let words = range.start as usize * ROW_WORDS..range.end as usize * ROW_WORDS;
                queue.write_buffer(
                    &buffers.rows,
                    u64::from(range.start) * ROW_BYTES,
                    bytemuck::cast_slice(&self.words[words]),
                );
            }
        }
        self.dirty.clear();
        if self.batch_upload {
            // The table is small. Write it whole.
            for (index, template) in self.templates.iter().enumerate() {
                let gpu = template.map_or([0; 4], |t| t.gpu);
                queue.write_buffer(
                    &buffers.batches,
                    index as u64 * 16,
                    bytemuck::cast_slice(&gpu),
                );
                queue.write_buffer(
                    &buffers.batch_offsets,
                    index as u64 * ALIGN,
                    bytemuck::cast_slice(&[gpu[0], 0, 0, 0]),
                );
            }
            self.batch_upload = false;
        }
    }
    fn prepare_view(
        &mut self,
        world: &mut World,
        index: usize,
        view_key: MeshPipelineKey,
    ) -> Result<bool, String> {
        let batches = self.batches.len();
        let rows = self.rows();
        let view = &mut self.views[index];
        view.specializations.resize_with(batches, || None);
        view.pipelines.resize(batches, BatchPipelines::default());
        let mut ready = true;
        for batch in 0..batches {
            view.pipelines[batch] = BatchPipelines::default();
            let key = self.batches[batch].key;
            // Hidden rows draw nothing, so their pipelines can still compile.
            if self.batches[batch].shown == 0 {
                continue;
            }
            let Some(base) = specialize(
                world,
                view_key,
                key.mesh,
                key.material,
                &mut view.specializations[batch],
            )?
            else {
                ready = false;
                continue;
            };
            let cache = world.resource::<PipelineCache>();
            let surface = world.resource::<SurfacePasses>();
            let casters = world.resource::<CasterPasses>();
            let world_shader = key.material.type_id() == TypeId::of::<ImpostorMaterial>();
            ready &= pipeline_state(base, world_shader, cache, surface, casters)?;
            view.pipelines[batch] = BatchPipelines {
                base: Some(base),
                partner: surface.partner(base),
                caster: casters.pipeline(base),
            };
        }
        let scene = world.resource::<ShadowScene>();
        let buffers = self.buffers.as_ref().unwrap();
        let queue = world.resource::<RenderQueue>();
        let mask_key = (
            scene.mask_revision(index),
            scene.caster_data.revision(),
            self.owner,
            self.batch_revision.unwrap_or(0),
            rows,
        );
        if view.masks != Some(mask_key) {
            view.masks = Some(mask_key);
            self.mask_bytes.clear();
            self.mask_bytes.resize(buffers.mask_region as usize, 0);
            view.cascades.clear();
            view.cascades.resize(batches, [0; 8]);
            for (item, key) in scene.caster_data.entities.iter().enumerate() {
                let CasterKey::Row(slot) = *key else {
                    continue;
                };
                let mask = scene.masks[index].get(item).copied().unwrap_or(0);
                self.mask_bytes[slot as usize] = mask;
                let batch = word_batch(self.words[slot as usize * ROW_WORDS + KEY_WORD]);
                if batch == NO_BATCH {
                    continue;
                }
                for bit in 0..8 {
                    view.cascades[batch as usize][bit] += u32::from(mask >> bit & 1);
                }
            }
            queue.write_buffer(
                &buffers.masks,
                index as u64 * buffers.mask_region,
                &self.mask_bytes,
            );
        }
        let snapshot = world.resource::<SourceViewFrame>().views[index].unwrap();
        let mut cull = [0u32; CULL_WORDS];
        for (plane, words) in snapshot.source_camera_planes.iter().zip(cull.chunks_mut(4)) {
            for (word, value) in words.iter_mut().zip(plane) {
                *word = value.to_bits();
            }
        }
        cull[24..29].copy_from_slice(&[
            snapshot.mask,
            rows,
            batches as u32,
            buffers.stride,
            index as u32,
        ]);
        queue.write_buffer(
            &buffers.cull_views,
            index as u64 * ALIGN,
            bytemuck::cast_slice(&cull),
        );
        Ok(ready)
    }
    fn poll_readback(&mut self) {
        let Some(readback) = &mut self.readback else {
            return;
        };
        if readback.state.load(Ordering::Acquire) != MAPPED {
            return;
        }
        {
            let range = readback.buffer.slice(..).get_mapped_range();
            let mut words = range
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_le_bytes(*bytes));
            for value in readback.totals.as_flattened_mut() {
                *value = words.next().unwrap_or(0);
            }
        }
        readback.buffer.unmap();
        readback.state.store(IDLE, Ordering::Release);
    }
}
/// Base pipeline of one batch in one view. Cache hits do not clone.
fn specialize(
    world: &mut World,
    view_key: MeshPipelineKey,
    mesh_id: AssetId<Mesh>,
    material_id: bevy::asset::UntypedAssetId,
    cached: &mut Option<Specialization>,
) -> Result<Option<CachedRenderPipelineId>, String> {
    let Some(mesh) = world.resource::<RenderAssets<RenderMesh>>().get(mesh_id) else {
        return Ok(None);
    };
    let Some(material) = world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(material_id)
    else {
        return Ok(None);
    };
    let bits: MeshPipelineKey = material.properties.mesh_pipeline_key_bits.downcast();
    let key = view_key
        | MeshPipelineKey::from_bits_retain(mesh.key_bits.bits())
        | bits
        | alpha_mode_pipeline_key(
            material.properties.alpha_mode,
            &Msaa::from_samples(view_key.msaa_samples()),
        );
    let cache_key = SpecializationKey::new(key.bits(), &mesh.layout.0, &material.properties);
    if let Some(cached) = cached.as_ref().filter(|c| c.key == cache_key) {
        return Ok(Some(cached.base));
    }
    let layout = mesh.layout.clone();
    let properties = material.properties.clone();
    let full = ErasedMaterialPipelineKey {
        type_id: material_id.type_id(),
        mesh_key: ErasedMeshPipelineKey::new(key),
        material_key: properties.material_key.clone(),
    };
    let base = base_specialize(world, full, &layout, &properties)
        .map_err(|e| format!("Source GPU prop specialization: {e}"))?;
    *cached = Some(Specialization {
        key: cache_key,
        _layout: layout,
        _properties: properties,
        base,
    });
    Ok(Some(base))
}

/// Cull the rows of one view into its five visible lists. Runs before the shadow atlas.
pub(crate) fn cull(
    view: ViewQuery<&SourceSurfaceView>,
    draws: Res<PropDraws>,
    frame: Res<SourceViewFrame>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let index = view.into_inner().index;
    for count in &draws.views[index].draws {
        count.store(0, Ordering::Relaxed);
    }
    if draws.frame != frame.frame || draws.live == 0 {
        return;
    }
    let (Some(buffers), Some([reset, cull])) = (&draws.buffers, draws.cull) else {
        return;
    };
    let (Some(reset), Some(cull)) = (
        cache.get_compute_pipeline(reset),
        cache.get_compute_pipeline(cull),
    ) else {
        return;
    };
    let offsets = [
        (index as u64 * buffers.args_region) as u32,
        (index as u64 * buffers.visible_region) as u32,
        (index as u64 * ALIGN) as u32,
        (index as u64 * buffers.mask_region) as u32,
    ];
    let encoder = context.command_encoder();
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("source_prop_cull"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &buffers.cull, &offsets);
        pass.set_pipeline(reset);
        pass.dispatch_workgroups(
            (PASSES as u32 * draws.batches.len() as u32).div_ceil(64),
            1,
            1,
        );
        pass.set_pipeline(cull);
        pass.dispatch_workgroups(draws.rows().div_ceil(64), 1, 1);
    }
    if let Some(readback) = &draws.readback
        && matches!(readback.state.load(Ordering::Acquire), IDLE | COPIED)
    {
        let offset = (index * TOTAL_WORDS * 4) as u64;
        encoder.copy_buffer_to_buffer(
            &buffers.totals,
            offset,
            &readback.buffer,
            offset,
            (TOTAL_WORDS * 4) as u64,
        );
        readback.state.store(COPIED, Ordering::Release);
    }
}
/// Map the copied totals after submission.
pub(crate) fn map_readback(mut draws: ResMut<PropDraws>, device: Res<RenderDevice>) {
    let readback = draws.readback.get_or_insert_with(|| Readback {
        buffer: device.create_buffer(&BufferDescriptor {
            label: Some("source_prop_readback"),
            size: (VIEW_COUNT * TOTAL_WORDS * 4) as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        state: Arc::new(AtomicU8::new(IDLE)),
        totals: [[0; TOTAL_WORDS]; VIEW_COUNT],
    });
    if readback.state.load(Ordering::Acquire) != COPIED {
        return;
    }
    readback.state.store(MAPPING, Ordering::Release);
    let state = readback.state.clone();
    readback
        .buffer
        .slice(..)
        .map_async(MapMode::Read, move |result| {
            state.store(
                if result.is_ok() { MAPPED } else { IDLE },
                Ordering::Release,
            );
        });
}

/// Bind the row group and the mesh buffers of one batch draw.
fn bind_batch<'w>(
    world: &'w World,
    pass: &mut TrackedRenderPass<'w>,
    buffers: &'w Buffers,
    list: u32,
    batch: usize,
    mesh: AssetId<Mesh>,
    format: IndexFormat,
) -> bool {
    let allocator = world.resource::<MeshAllocator>();
    let (Some(vertices), Some(indices)) = (
        allocator.mesh_vertex_slice(&mesh),
        allocator.mesh_index_slice(&mesh),
    ) else {
        return false;
    };
    pass.set_vertex_buffer(0, vertices.buffer.slice(..));
    pass.set_index_buffer(indices.buffer.slice(..), format);
    pass.set_bind_group(2, &buffers.draw, &[list, (batch as u64 * ALIGN) as u32]);
    true
}
fn material_group(world: &World, id: bevy::asset::UntypedAssetId) -> Option<&BindGroup> {
    let material = world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(id)?;
    world
        .resource::<MaterialBindGroupAllocators>()
        .get(&id.type_id())?
        .get(material.binding.group)?
        .bind_group()
}
/// Count the draws of one bundle. A failed bundle fails the renderer.
fn bundle_counts(
    world: &World,
    state: &ViewState,
    counts: Result<(u32, u32), String>,
) -> (u32, u32) {
    let (attempted, issued) = counts.unwrap_or_else(|error| {
        world.resource::<RendererStateHandle>().fail(error);
        (0, 0)
    });
    state.draws[0].fetch_add(attempted, Ordering::Relaxed);
    state.draws[1].fetch_add(issued, Ordering::Relaxed);
    (attempted, issued)
}

/// Base and additive draws of each shown batch, after the Bevy phases of the camera pass.
/// Returns the attempted and issued draw counts of this call.
pub(crate) fn draw_camera<'w>(
    world: &'w World,
    pass: &mut TrackedRenderPass<'w>,
    entity: Entity,
    index: usize,
    samples: u32,
) -> (u32, u32) {
    let draws = world.resource::<PropDraws>();
    let Some(buffers) = &draws.buffers else {
        return (0, 0);
    };
    let state = &draws.views[index];
    let cache = world.resource::<PipelineCache>();
    let camera = world
        .resource::<super::source_lighting::SourceViewCamera>()
        .bind_group();
    let offset = world.get::<ViewUniformOffset>(entity);
    let lighting = world.get::<super::source_lighting::SourceViewLighting>(entity);
    let target = BundleTarget {
        color: Some(HDR_FORMAT),
        depth: DEPTH_FORMAT,
        samples,
    };
    let counts = state.bundles[0].lock().unwrap().draw(
        world.resource::<RenderDevice>(),
        target,
        pass,
        |pass| {
            let mut counts = (0, 0);
            for (batch, entry) in draws.batches.iter().enumerate() {
                if entry.shown == 0 {
                    continue;
                }
                let pipelines = state.pipelines.get(batch).copied().unwrap_or_default();
                let template = draws.templates.get(batch).copied().flatten();
                let ids = std::iter::once(pipelines.base).chain(pipelines.partner.map(Some));
                for id in ids {
                    counts.0 += 1;
                    let pipeline = id.and_then(|id| cache.get_render_pipeline(id));
                    let (
                        Some(pipeline),
                        Some(camera),
                        Some(offset),
                        Some(lighting),
                        Some(template),
                        Some(material),
                    ) = (
                        pipeline,
                        camera,
                        offset,
                        lighting,
                        template,
                        material_group(world, entry.key.material),
                    )
                    else {
                        continue;
                    };
                    pass.set_render_pipeline(pipeline);
                    pass.set_bind_group(0, camera, &[offset.offset]);
                    pass.set_bind_group(1, lighting.bind_group(), &[]);
                    pass.set_bind_group(3, material, &[]);
                    if !bind_batch(
                        world,
                        pass,
                        buffers,
                        buffers.list_offset(index, 0),
                        batch,
                        entry.key.mesh,
                        template.format,
                    ) {
                        continue;
                    }
                    pass.draw_indexed_indirect(
                        &buffers.args,
                        buffers.args_offset(index, 0, batch, draws.batches.len()),
                    );
                    counts.1 += 1;
                }
            }
            counts
        },
    );
    bundle_counts(world, state, counts)
}
/// Caster draws of each batch with rows in this caster mask bit, after the caster phase.
/// `tile` is the caster slot: a cascade or a lamp face.
pub(crate) fn draw_cascade<'w>(
    world: &'w World,
    pass: &mut TrackedRenderPass<'w>,
    entity: Entity,
    index: usize,
    cascade: usize,
    tile: usize,
) -> (u32, u32) {
    let draws = world.resource::<PropDraws>();
    let Some(buffers) = &draws.buffers else {
        return (0, 0);
    };
    let state = &draws.views[index];
    let cache = world.resource::<PipelineCache>();
    let camera = world
        .resource::<super::source_lighting::SourceViewCamera>()
        .bind_group();
    let offset = world.get::<ViewUniformOffset>(entity);
    let caster = world.get::<super::caster::CasterViews>(entity);
    let materials = world.resource::<super::caster_material::CasterMaterials>();
    let target = BundleTarget {
        color: None,
        depth: SHADOW_FORMAT,
        samples: 1,
    };
    let counts = state.bundles[1 + tile].lock().unwrap().draw(
        world.resource::<RenderDevice>(),
        target,
        pass,
        |pass| {
            let mut counts = (0, 0);
            for (batch, entry) in draws.batches.iter().enumerate() {
                let pipelines = state.pipelines.get(batch).copied().unwrap_or_default();
                if pipelines.caster.is_none()
                    || state
                        .cascades
                        .get(batch)
                        .is_none_or(|rows| rows[cascade] == 0)
                {
                    continue;
                }
                counts.0 += 1;
                let pipeline = pipelines
                    .caster
                    .and_then(|id| cache.get_render_pipeline(id));
                let template = draws.templates.get(batch).copied().flatten();
                // The primary caster has its own material group. Other casters read the surface group.
                let material = if entry.key.material.type_id() == TypeId::of::<SourceMaterial>() {
                    materials.group(entry.key.material.typed())
                } else {
                    material_group(world, entry.key.material)
                };
                let (
                    Some(pipeline),
                    Some(camera),
                    Some(offset),
                    Some(caster),
                    Some(template),
                    Some(material),
                ) = (pipeline, camera, offset, caster, template, material)
                else {
                    continue;
                };
                pass.set_render_pipeline(pipeline);
                pass.set_bind_group(0, camera, &[offset.offset]);
                pass.set_bind_group(1, caster.binding(tile), &[]);
                pass.set_bind_group(3, material, &[]);
                if !bind_batch(
                    world,
                    pass,
                    buffers,
                    buffers.list_offset(index, 1 + cascade),
                    batch,
                    entry.key.mesh,
                    template.format,
                ) {
                    continue;
                }
                pass.draw_indexed_indirect(
                    &buffers.args,
                    buffers.args_offset(index, 1 + cascade, batch, draws.batches.len()),
                );
                counts.1 += 1;
            }
            counts
        },
    );
    bundle_counts(world, state, counts)
}

pub fn install(app: &mut SubApp) {
    use super::surface_graph::{SourceSurface, SourceSurfaceStage};
    use bevy::render::{ExtractSchedule, Render, RenderSystems};
    app.init_resource::<PropDraws>()
        .add_systems(ExtractSchedule, extract)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
        .add_systems(Render, map_readback.in_set(RenderSystems::Cleanup))
        .add_systems(
            SourceSurface,
            cull.after(bevy::pbr::early_gpu_preprocess)
                .before(SourceSurfaceStage::ShadowAtlas)
                .run_if(super::frame_graph::surface_ready)
                .run_if(super::shadow_queue::graph_ready),
        );
}
