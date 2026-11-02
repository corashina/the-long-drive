//! Installed ForwardShaderRenderLoop::RenderObjectSorter<0> (RVA 0x3349d0).
//! Runtime identities belong to the target cache. They are not Unity file IDs.
use std::cmp::Ordering;

/// Source preserves only the low twelve material identity bits after this shift.
pub fn material_sort_key(identity: u32, instancing: bool, property_block_id: u32) -> u32 {
    (identity << 20)
        | if instancing {
            0
        } else {
            (property_block_id >> 1) & 0xfffff
        }
}

/// MeshRenderer fudge is zero. Native arithmetic adds Y squared, X squared, then Z squared.
pub fn perspective_distance(center: [f32; 3], camera: [f32; 3]) -> f32 {
    let [x, y, z] = std::array::from_fn(|i| center[i] - camera[i]);
    -((y * y + x * x) + z * z)
}

/// One entry represents one source pass, including a later ordered glass grab.
#[derive(Clone, Copy, Debug)]
pub struct PassSortKey {
    pub layer_and_order: u32,
    pub queue: i16,
    pub distance: f32,
    pub sorting_group: u32,
    pub same_distance_priority: u16,
    pub node_index: i32,
    pub source_material_index: u16,
    pub material_key: u32,
    pub pass_data: u32,
    pub static_batch_first: u16,
    pub static_batch_count: u16,
    pub small_mesh_index: i32,
    pub lightmap_index: u32,
    pub shader_identity: i32,
    pub gather_index: i32,
}
impl PassSortKey {
    pub fn compare(&self, other: &Self) -> Ordering {
        let prefix = self
            .layer_and_order
            .cmp(&other.layer_and_order)
            .then_with(|| self.queue.cmp(&other.queue))
            .then_with(|| {
                self.distance
                    .partial_cmp(&other.distance)
                    .expect("finite source distance")
            });
        if prefix != Ordering::Equal {
            return prefix;
        }
        if self.sorting_group & 0xfffff000 != 0xfffff000
            || other.sorting_group & 0xfffff000 != 0xfffff000
        {
            let order = self.sorting_group.cmp(&other.sorting_group);
            if order != Ordering::Equal {
                return order;
            }
        }
        let priority = self
            .same_distance_priority
            .cmp(&other.same_distance_priority);
        if priority != Ordering::Equal {
            return priority;
        }
        if self.node_index == other.node_index && self.material_key != other.material_key {
            let order = (self.source_material_index >> 1).cmp(&(other.source_material_index >> 1));
            if order != Ordering::Equal {
                return order;
            }
        }
        // Source marks the first eligible pass, then clears this bit for later passes.
        let first = (other.pass_data & 0x01000000).cmp(&(self.pass_data & 0x01000000));
        if first != Ordering::Equal {
            return first;
        }
        match (self.static_batch_count != 0, other.static_batch_count != 0) {
            (true, true) => {
                return self
                    .material_key
                    .cmp(&other.material_key)
                    .then_with(|| self.small_mesh_index.cmp(&other.small_mesh_index))
                    .then_with(|| self.static_batch_first.cmp(&other.static_batch_first));
            }
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
        // The optional high distance-byte comparison cannot distinguish these equal finite keys.
        self.lightmap_index
            .cmp(&other.lightmap_index)
            .then_with(|| self.shader_identity.cmp(&other.shader_identity))
            .then_with(|| self.material_key.cmp(&other.material_key))
            .then_with(|| (self.pass_data as u8).cmp(&(other.pass_data as u8)))
            .then_with(|| self.gather_index.cmp(&other.gather_index))
    }
}
