// GPU prop culling for one view. Pass 0 is the camera, passes 1-4 are the shadow cascades, 5-8 the lamp shadow slots.
// A row that passes appends its index to the visible list of its batch in that pass.
struct PropRow {
    world_x: vec4<f32>,
    world_y: vec4<f32>,
    world_z: vec4<f32>,
    center: vec3<f32>,
    fade: f32,
    extents: vec3<f32>,
    // Batch above 8 flag bits.
    key: u32,
    // Linear surface color: the condition color, else the material color.
    color: vec4<f32>,
    // Frozen source position for procedural inputs.
    source_position: vec3<f32>,
    unused: u32,
}
struct PropBatch {
    first: u32,
    index_count: u32,
    first_index: u32,
    base_vertex: u32,
}
struct CullView {
    // Source camera planes. The row bounds use the same source frame.
    planes: array<vec4<f32>, 6>,
    // x: camera layer mask, y: row count, z: batch count, w: visible list stride.
    counts: vec4<u32>,
    // x: view index for the totals.
    view: vec4<u32>,
}
@group(0) @binding(0) var<storage> rows: array<PropRow>;
@group(0) @binding(1) var<storage> batches: array<PropBatch>;
// DrawIndexedIndirect arguments, 5 words per batch and pass.
@group(0) @binding(2) var<storage, read_write> args: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> visible: array<u32>;
// Visible rows per view and pass, 16 words per view.
@group(0) @binding(4) var<storage, read_write> totals: array<atomic<u32>>;
@group(0) @binding(5) var<uniform> view: CullView;
// Shadow split masks, one byte per row.
@group(0) @binding(6) var<storage> masks: array<u32>;

const PASSES: u32 = 9u;
const NO_BATCH: u32 = 0xffffffu;
const HIDDEN: u32 = 32u;
const SHADOW_ONLY: u32 = 64u;

var<workgroup> counted: array<atomic<u32>, 9>;

@compute @workgroup_size(64)
fn reset(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i < PASSES {
        atomicStore(&totals[view.view.x * 16u + i], 0u);
    }
    let count = view.counts.z;
    if i >= PASSES * count {
        return;
    }
    let batch = batches[i % count];
    let base = i * 5u;
    atomicStore(&args[base], batch.index_count);
    atomicStore(&args[base + 1u], 0u);
    atomicStore(&args[base + 2u], batch.first_index);
    atomicStore(&args[base + 3u], batch.base_vertex);
    atomicStore(&args[base + 4u], 0u);
}

// Port of `intersects_camera`. Keep its addition order.
fn intersects_camera(center: vec3<f32>, extents: vec3<f32>) -> bool {
    for (var p = 0u; p < 6u; p++) {
        let plane = view.planes[p];
        let distance = (plane.x * center.x + plane.y * center.y) + plane.z * center.z + plane.w;
        let radius = (abs(plane.x) * extents.x + abs(plane.y) * extents.y) + abs(plane.z) * extents.z;
        if distance + radius < 0.0 {
            return false;
        }
    }
    return true;
}

fn append(list: u32, row: u32, batch: u32) {
    let slot = atomicAdd(&args[(list * view.counts.z + batch) * 5u + 1u], 1u);
    visible[list * view.counts.w + batches[batch].first + slot] = row;
    atomicAdd(&counted[list], 1u);
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    if local < PASSES {
        atomicStore(&counted[local], 0u);
    }
    workgroupBarrier();
    let i = id.x;
    if i < view.counts.y {
        let row = rows[i];
        let batch = row.key >> 8u;
        if batch != NO_BATCH {
            // HIDDEN also holds the LOD selection.
            if (row.key & (HIDDEN | SHADOW_ONLY)) == 0u
                && (view.counts.x & (1u << (row.key & 31u))) != 0u
                && intersects_camera(row.center, row.extents) {
                append(0u, i, batch);
            }
            let mask = masks[i / 4u] >> ((i % 4u) * 8u);
            for (var bit = 0u; bit < 8u; bit++) {
                if (mask & (1u << bit)) != 0u {
                    append(1u + bit, i, batch);
                }
            }
        }
    }
    workgroupBarrier();
    if local < PASSES {
        atomicAdd(&totals[view.view.x * 16u + local], atomicLoad(&counted[local]));
    }
}
