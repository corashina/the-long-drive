// GPU prop culling for one view. Pass 0 is the camera, passes 1-4 are the shadow cascades, and passes 5-8 are the lamp shadow slots.
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
    // Renderer flags: cast, receive << 1, motion vectors << 2.
    renderer_flags: u32,
    // Renderer bounds in the mesh frame, and the probe usage.
    local_center: vec3<f32>,
    probe_usage: f32,
    local_extents: vec3<f32>,
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
    // x: view index for the totals, y: Cam and mainCam layers, z: farCamera layers.
    view: vec4<u32>,
    // Probe slots of the scene. extents.w=1 when the slot is enabled and published.
    probe_center: array<vec4<f32>, 4>,
    probe_extents: array<vec4<f32>, 4>,
    // Far plane at Cam1FarPlane and near plane at Cam2NearPlane. See `LayerRanges` in camera.rs.
    layer_planes: array<vec4<f32>, 2>,
}
// It has the same layout as GpuLight and LightingUniform in lighting.rs.
struct SourceLight {
    cookie_from_world: mat4x4<f32>,
    position_range: vec4<f32>,
    direction_kind: vec4<f32>,
    color: vec4<f32>,
    cookie_rect: vec4<f32>,
    mask: vec4<u32>,
    cookie_size: f32,
}
struct SourceLighting {
    lights: array<SourceLight,64>,
    ambient: vec4<f32>,
    fog: vec4<f32>,
    counts: vec4<u32>,
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
@group(0) @binding(7) var<uniform> lamp_lighting: SourceLighting;
// Lamp mask and probe pair of each camera-visible entry. It has the same layout as in object_lighting.rs.
@group(0) @binding(8) var<storage, read_write> lights: array<vec4<u32>>;

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

// Port of `LayerRanges::visible`. Keep its addition order.
fn layer_visible(layer: u32, center: vec3<f32>, extents: vec3<f32>) -> bool {
    let bit = 1u << layer;
    for (var i = 0u; i < 2u; i++) {
        let plane = view.layer_planes[i];
        let distance = (plane.x * center.x + plane.y * center.y) + plane.z * center.z + plane.w;
        let radius = (abs(plane.x) * extents.x + abs(plane.y) * extents.y) + abs(plane.z) * extents.z;
        if (view.view[1u + i] & bit) != 0u && distance + radius >= 0.0 {
            return true;
        }
    }
    return false;
}

// Returns the entry in the list.
fn append(list: u32, row: u32, batch: u32) -> u32 {
    let slot = atomicAdd(&args[(list * view.counts.z + batch) * 5u + 1u], 1u);
    let entry = batches[batch].first + slot;
    visible[list * view.counts.w + entry] = row;
    atomicAdd(&counted[list], 1u);
    return entry;
}

// A bound can exclude a lamp only when all arithmetic remains finite.
fn source_finite3(value: vec3<f32>) -> bool {
    return all((bitcast<vec3<u32>>(value) & vec3(0x7f800000u)) != vec3(0x7f800000u));
}
// Lamps that can reach the row bounds. object_lighting.rs has the same operations for entities.
fn object_light_mask(model: mat4x4<f32>, local_center: vec3<f32>, local_extents: vec3<f32>, receiver: u32) -> vec2<u32> {
    var selected = vec2(0u);
    let center = (model * vec4(local_center, 1.0)).xyz;
    let extents = abs(model[0].xyz) * local_extents.x
        + abs(model[1].xyz) * local_extents.y + abs(model[2].xyz) * local_extents.z;
    // Absolute terms also cover cancellation in the center transform.
    let magnitude = abs(model[0].xyz) * abs(local_center.x)
        + abs(model[1].xyz) * abs(local_center.y)
        + abs(model[2].xyz) * abs(local_center.z) + abs(model[3].xyz) + extents;
    // GPU prop rows are not skinned or morphed, so their bounds hold.
    let valid = all(local_extents >= vec3(0.0))
        && source_finite3(center) && source_finite3(extents) && source_finite3(magnitude);
    for (var i = 0u; i < lamp_lighting.counts.x; i += 1u) {
        let lamp = lamp_lighting.lights[i];
        if lamp.direction_kind.w == 0.0 { continue; }
        if (receiver & lamp.mask.x) == 0u { continue; }
        if max(lamp.color.r, max(lamp.color.g, lamp.color.b)) <= 0.0 { continue; }
        if valid {
            let position = lamp.position_range.xyz;
            let range = abs(lamp.position_range.w);
            // 128 f32 epsilons cover transform, subtraction, and squared-distance rounding.
            // Expand rather than round toward exclusion. Boundary contact retains the lamp.
            let padding = (magnitude + abs(position) + vec3(range + 1.0)) * 0.0000152587890625;
            let separation = max(abs(position - center) - extents - padding, vec3(0.0));
            let distance2 = dot(separation, separation);
            let range2 = range * range;
            if source_finite3(padding) && source_finite3(vec3(distance2, range2, range))
                && distance2 > range2 { continue; }
        }
        selected[i >> 5u] |= 1u << (i & 31u);
    }
    return selected;
}
// Unity BlendProbes with equal importance: the two slots with the largest box overlap.
// One probe takes full weight. Two share it by overlap volume.
// Returns x=first weight, z=first slot, w=second slot or -1. x=0 without a probe.
// Renderer extents retain the recovered native Vector3f::epsilon clamp.
fn probe_blend(model: mat4x4<f32>, local_center: vec3<f32>, local_extents: vec3<f32>) -> vec4<f32> {
    let center = (model * vec4(local_center, 1.0)).xyz;
    let extents = max(abs(model[0].xyz)*local_extents.x
        + abs(model[1].xyz)*local_extents.y + abs(model[2].xyz)*local_extents.z,
        vec3(9.999999747378752e-06));
    var volumes = vec2(0.0);
    var slots = vec2(-1.0);
    for (var slot = 0; slot < 4; slot++) {
        let probe_center = view.probe_center[slot];
        let probe_extents = view.probe_extents[slot];
        if probe_extents.w == 0.0 { continue; }
        let minimum = max(center-extents, probe_center.xyz-probe_extents.xyz);
        let maximum = min(center+extents, probe_center.xyz+probe_extents.xyz);
        let size = max(maximum-minimum, vec3(0.0));
        let volume = size.x*size.y*size.z;
        if volume > volumes.x {
            volumes = vec2(volume, volumes.x);
            slots = vec2(f32(slot), slots.x);
        } else if volume > volumes.y {
            volumes.y = volume;
            slots.y = f32(slot);
        }
    }
    if slots.x < 0.0 { return vec4(0.0, 0.0, 0.0, -1.0); }
    let weight = select(volumes.x/(volumes.x+volumes.y), 1.0, slots.y < 0.0);
    return vec4(weight, 0.0, slots);
}
// It uses the same packing as `pack` in object_lighting.rs. The material receiver mask of a row is its layer bit.
fn row_object(row: PropRow) -> vec4<u32> {
    let model = transpose(mat4x4<f32>(row.world_x, row.world_y, row.world_z, vec4<f32>(0.0, 0.0, 0.0, 1.0)));
    let mask = object_light_mask(model, row.local_center, row.local_extents, 1u << (row.key & 31u));
    var probes = vec4(0.0, 0.0, 0.0, -1.0);
    if row.probe_usage != 0.0 {
        probes = probe_blend(model, row.local_center, row.local_extents);
    }
    let second = select(u32(probes.w), 0xffu, probes.w < 0.0);
    return vec4(mask, u32(probes.z) | (second << 8u) | (row.renderer_flags << 16u), bitcast<u32>(probes.x));
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
                && intersects_camera(row.center, row.extents)
                && layer_visible(row.key & 31u, row.center, row.extents) {
                lights[append(0u, i, batch)] = row_object(row);
            }
            let mask = masks[i / 4u] >> ((i % 4u) * 8u);
            for (var bit = 0u; bit < 8u; bit++) {
                if (mask & (1u << bit)) != 0u {
                    _ = append(1u + bit, i, batch);
                }
            }
        }
    }
    workgroupBarrier();
    if local < PASSES {
        atomicAdd(&totals[view.view.x * 16u + local], atomicLoad(&counted[local]));
    }
}
