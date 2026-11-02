//! GPU-driven static props. A static renderer is a row in GPU buffers, not an entity.
//! The main world owns the rows. The render world uploads the dirty ranges.
use crate::reference::shadow_candidates::Bounds;
use bevy::asset::{AssetId, UntypedAssetId};
use bevy::color::{Color, ColorToComponents};
use bevy::ecs as bevy_ecs;
use bevy::math::{Affine3A, Mat4, Vec3, Vec4};
use bevy::mesh::Mesh;
use bevy::platform::collections::HashMap;
use bevy::prelude::Resource;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

/// Words of one row in the GPU row buffer. Same layout as `PropRow` in source_props.wgsl.
pub const ROW_WORDS: usize = 28;
/// Row word with the batch and the flags.
pub const KEY_WORD: usize = 19;
/// The row slot is free.
pub const NO_BATCH: u32 = u32::MAX;
/// The last row word holds the batch above 8 flag bits.
pub const FLAG_BITS: u32 = 8;
/// Row flag bits after the 5 layer bits.
pub const ROW_HIDDEN: u32 = 1 << 5;
pub const ROW_SHADOW_ONLY: u32 = 1 << 6;
/// Dirty rows closer than this many rows upload as one range.
const DIRTY_GAP: u32 = 8;
/// Batch of the last row word. A free slot gives `NO_BATCH`.
pub fn word_batch(word: u32) -> u32 {
    match word >> FLAG_BITS {
        batch if batch == NO_BATCH >> FLAG_BITS => NO_BATCH,
        batch => batch,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropRow {
    /// World matrix in the render-origin frame.
    pub world: Mat4,
    /// World AABB in the source frame, as the shadow scene uses it.
    pub bounds: Bounds,
    pub layer: u8,
    /// Source shadow casting mode. 3 is shadows only.
    pub cast_shadows: u8,
    /// Source `makeinvisible`: the camera does not draw the row. Its shadow stays.
    pub camera_hidden: bool,
    /// Root, node and LOD visibility together, as an entity inherits them.
    pub hidden: bool,
    /// Signed LOD crossfade, as the entity material `lod_fade`. 1 is no fade.
    pub fade: f32,
    /// Linear surface color: the condition color, else the material color.
    pub color: Vec4,
    /// Frozen source position for procedural inputs, as the entity impostor uniform.
    pub source_position: Vec3,
    pub batch: u32,
}
impl PropRow {
    pub fn words(&self) -> [u32; ROW_WORDS] {
        let m = self.world;
        let rows = [m.row(0), m.row(1), m.row(2)];
        let mut flags = u32::from(self.layer);
        if self.hidden {
            flags |= ROW_HIDDEN;
        }
        if self.cast_shadows == 3 || self.camera_hidden {
            flags |= ROW_SHADOW_ONLY;
        }
        let f = f32::to_bits;
        let [c, e] = [self.bounds.center, self.bounds.extents];
        [
            f(rows[0].x),
            f(rows[0].y),
            f(rows[0].z),
            f(rows[0].w),
            f(rows[1].x),
            f(rows[1].y),
            f(rows[1].z),
            f(rows[1].w),
            f(rows[2].x),
            f(rows[2].y),
            f(rows[2].z),
            f(rows[2].w),
            f(c[0]),
            f(c[1]),
            f(c[2]),
            f(self.fade),
            f(e[0]),
            f(e[1]),
            f(e[2]),
            self.batch << FLAG_BITS | flags,
            f(self.color.x),
            f(self.color.y),
            f(self.color.z),
            f(self.color.w),
            f(self.source_position.x),
            f(self.source_position.y),
            f(self.source_position.z),
            0,
        ]
    }
}
/// Linear color of a source sRGBA condition color. Entity materials and rows use this one formula.
pub fn linear_color(srgba: [f32; 4]) -> Vec4 {
    Vec4::from_array(
        Color::srgba(srgba[0], srgba[1], srgba[2], srgba[3])
            .to_linear()
            .to_f32_array(),
    )
}
/// Source position of a renderer node. Entity impostors and rows use this one formula.
pub fn source_position(source_world: Mat4) -> Vec3 {
    source_world.w_axis.truncate() * Vec3::new(1., 1., -1.)
}
/// World AABB in the source frame. Entity and row casters use this one formula.
pub fn world_bounds(world: Affine3A, center: [f32; 3], half_extents: [f32; 3]) -> Bounds {
    let center = world.transform_point3(center.into());
    let e = world.matrix3.x_axis.abs() * half_extents[0]
        + world.matrix3.y_axis.abs() * half_extents[1]
        + world.matrix3.z_axis.abs() * half_extents[2];
    Bounds {
        center: [center.x, center.y, -center.z],
        extents: e.to_array(),
    }
}
/// Draws share mesh and material. Layer and shadow mode are row data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BatchKey {
    pub mesh: AssetId<Mesh>,
    pub material: UntypedAssetId,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropBatch {
    pub key: BatchKey,
    /// Live rows. Zero marks a free batch slot.
    pub rows: u32,
    /// Rows that are not hidden. A change bumps the batch revision only at zero.
    pub shown: u32,
    /// First entry of the batch in each visible list.
    pub first: u32,
    pub capacity: u32,
}
static OWNERS: AtomicU64 = AtomicU64::new(1);

#[derive(Resource)]
pub struct GpuProps {
    /// Unique per owner. A save exchange replaces the owner, so the render world uploads all rows.
    id: u64,
    rows: Vec<PropRow>,
    free: Vec<u32>,
    live: u32,
    /// Dirty slots, each once. `marked` holds their flags.
    dirty: Vec<u32>,
    marked: Vec<bool>,
    /// Slots with changed shadow inputs, each once. Fade changes do not count.
    shadow_dirty: Vec<u32>,
    shadow_marked: Vec<bool>,
    batches: Vec<PropBatch>,
    free_batches: Vec<u32>,
    index: HashMap<BatchKey, u32>,
    /// Sum of batch capacities: the length of one visible list.
    visible_len: u32,
    /// Changes when rows are added or removed.
    membership: u64,
    /// Changes on any batch table change.
    batch_revision: u64,
}
impl Default for GpuProps {
    fn default() -> Self {
        Self {
            id: OWNERS.fetch_add(1, Ordering::Relaxed),
            rows: Vec::new(),
            free: Vec::new(),
            live: 0,
            dirty: Vec::new(),
            marked: Vec::new(),
            shadow_dirty: Vec::new(),
            shadow_marked: Vec::new(),
            batches: Vec::new(),
            free_batches: Vec::new(),
            index: HashMap::default(),
            visible_len: 0,
            membership: 0,
            batch_revision: 0,
        }
    }
}
impl GpuProps {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn rows(&self) -> &[PropRow] {
        &self.rows
    }
    pub fn live(&self) -> u32 {
        self.live
    }
    pub fn batches(&self) -> &[PropBatch] {
        &self.batches
    }
    pub fn visible_len(&self) -> u32 {
        self.visible_len
    }
    pub fn membership(&self) -> u64 {
        self.membership
    }
    pub fn batch_revision(&self) -> u64 {
        self.batch_revision
    }
    pub fn retained_bytes(&self) -> usize {
        self.rows.capacity() * std::mem::size_of::<PropRow>()
            + self.free.capacity() * 4
            + self.dirty.capacity() * 4
            + self.marked.capacity()
            + self.shadow_dirty.capacity() * 4
            + self.shadow_marked.capacity()
            + self.batches.capacity() * std::mem::size_of::<PropBatch>()
            + self.free_batches.capacity() * 4
            + self.index.capacity() * (std::mem::size_of::<(BatchKey, u32)>() + 1)
    }
    /// Moves the dirty rows to `out` as sorted ranges. The render world uploads them.
    /// Small gaps join ranges, so many scattered rows give few uploads of little extra data.
    pub fn take_dirty(&mut self, out: &mut Vec<Range<u32>>) {
        self.dirty.sort_unstable();
        let mut current: Option<Range<u32>> = None;
        for &slot in &self.dirty {
            self.marked[slot as usize] = false;
            match &mut current {
                Some(range) if slot <= range.end + DIRTY_GAP => range.end = slot + 1,
                _ => out.extend(current.replace(slot..slot + 1)),
            }
        }
        out.extend(current);
        self.dirty.clear();
    }
    /// Moves the slots with changed shadow inputs to `out`. Membership changes are not in it.
    pub fn take_shadow_dirty(&mut self, out: &mut Vec<u32>) {
        for &slot in &self.shadow_dirty {
            self.shadow_marked[slot as usize] = false;
        }
        out.append(&mut self.shadow_dirty);
    }
    fn mark(&mut self, slot: u32) {
        let marked = &mut self.marked[slot as usize];
        if !*marked {
            *marked = true;
            self.dirty.push(slot);
        }
    }
    /// The row uploads, and the shadow scene reads it again.
    fn mark_shadow(&mut self, slot: u32) {
        self.mark(slot);
        let marked = &mut self.shadow_marked[slot as usize];
        if !*marked {
            *marked = true;
            self.shadow_dirty.push(slot);
        }
    }
    /// Adds a row. The batch of `row` is replaced.
    pub fn alloc(&mut self, key: BatchKey, mut row: PropRow) -> u32 {
        let batch = match self.index.get(&key) {
            Some(&batch) => batch,
            None => {
                let batch = self.free_batches.pop().unwrap_or_else(|| {
                    self.batches.push(PropBatch {
                        key,
                        rows: 0,
                        shown: 0,
                        first: 0,
                        capacity: 0,
                    });
                    self.batches.len() as u32 - 1
                });
                self.batches[batch as usize].key = key;
                self.index.insert(key, batch);
                batch
            }
        };
        assert!(batch < NO_BATCH >> FLAG_BITS, "GPU prop batch count");
        let entry = &mut self.batches[batch as usize];
        entry.rows += 1;
        entry.shown += u32::from(!row.hidden);
        if entry.rows > entry.capacity {
            self.layout();
        }
        self.batch_revision += 1;
        row.batch = batch;
        let slot = self.free.pop().unwrap_or_else(|| {
            self.rows.push(row);
            self.marked.push(false);
            self.shadow_marked.push(false);
            self.rows.len() as u32 - 1
        });
        self.rows[slot as usize] = row;
        self.live += 1;
        self.membership += 1;
        self.mark(slot);
        slot
    }
    pub fn free(&mut self, slot: u32) {
        let row = &mut self.rows[slot as usize];
        let batch = std::mem::replace(&mut row.batch, NO_BATCH);
        assert_ne!(batch, NO_BATCH, "GPU prop row is already free");
        let entry = &mut self.batches[batch as usize];
        entry.rows -= 1;
        entry.shown -= u32::from(!row.hidden);
        if entry.rows == 0 {
            self.index.remove(&entry.key);
            self.free_batches.push(batch);
        }
        self.batch_revision += 1;
        self.free.push(slot);
        self.live -= 1;
        self.membership += 1;
        self.mark(slot);
        // Free capacity stays until it is a large part of the visible list.
        if self.visible_len > 1024 && self.visible_len > 4 * self.live {
            self.layout();
        }
    }
    pub fn set_hidden(&mut self, slot: u32, hidden: bool) {
        let row = &mut self.rows[slot as usize];
        if row.hidden == hidden {
            return;
        }
        row.hidden = hidden;
        let entry = &mut self.batches[row.batch as usize];
        if hidden {
            entry.shown -= 1;
        } else {
            entry.shown += 1;
        }
        // The render world reads only whether a batch shows rows.
        if entry.shown == u32::from(!hidden) {
            self.batch_revision += 1;
        }
        self.mark_shadow(slot);
    }
    /// Linear row color: a live condition color.
    pub fn set_color(&mut self, slot: u32, color: Vec4) {
        let row = &mut self.rows[slot as usize];
        if row.color != color {
            row.color = color;
            self.mark(slot);
        }
    }
    pub fn set_camera_hidden(&mut self, slot: u32, hidden: bool) {
        let row = &mut self.rows[slot as usize];
        if row.camera_hidden != hidden {
            row.camera_hidden = hidden;
            self.mark(slot);
        }
    }
    /// Writes the renderer LOD state `(visible, fade)` of a row, as the entity path applies it.
    /// `shown` is the root and static node visibility.
    pub fn set_lod(&mut self, slot: u32, shown: bool, (visible, fade): (bool, f32)) {
        self.set_hidden(slot, !(shown && visible));
        self.set_fade(slot, fade);
    }
    /// LOD crossfade. Casters dither with it, so the shadow tiles draw again.
    fn set_fade(&mut self, slot: u32, fade: f32) {
        let row = &mut self.rows[slot as usize];
        if row.fade.to_bits() == fade.to_bits() {
            return;
        }
        row.fade = fade;
        self.mark_shadow(slot);
    }
    pub fn set_world(&mut self, slot: u32, world: Mat4, bounds: Bounds) {
        let row = &mut self.rows[slot as usize];
        if row.world == world && row.bounds == bounds {
            return;
        }
        row.world = world;
        row.bounds = bounds;
        self.mark_shadow(slot);
    }
    /// Gives each batch a power-of-two region in the visible lists.
    fn layout(&mut self) {
        let mut first = 0;
        for batch in &mut self.batches {
            batch.capacity = if batch.rows == 0 {
                0
            } else {
                batch.rows.next_power_of_two()
            };
            batch.first = first;
            first += batch.capacity;
        }
        self.visible_len = first;
    }
    /// Removes every row. Retained storage stays.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.free.clear();
        self.batches.clear();
        self.free_batches.clear();
        self.index.clear();
        self.dirty.clear();
        self.marked.clear();
        self.shadow_dirty.clear();
        self.shadow_marked.clear();
        self.live = 0;
        self.visible_len = 0;
        self.membership += 1;
        self.batch_revision += 1;
    }
}
