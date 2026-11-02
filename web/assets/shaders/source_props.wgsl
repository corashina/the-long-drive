#define_import_path source_props
// GPU prop rows for the vertex stage. The batch offset and the instance index select a visible row.
struct PropRow {
    world_x: vec4<f32>,
    world_y: vec4<f32>,
    world_z: vec4<f32>,
    center: vec3<f32>,
    // Signed LOD crossfade, as the material `lod_fade`.
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
@group(2) @binding(50) var<storage> prop_rows: array<PropRow>;
@group(2) @binding(51) var<storage> prop_visible: array<u32>;
// x: first entry of this batch in the visible list. Dynamic offset per batch.
@group(2) @binding(52) var<uniform> prop_batch: vec4<u32>;

fn prop_row(instance_index: u32) -> PropRow {
    return prop_rows[prop_visible[prop_batch.x + instance_index]];
}
fn prop_world_from_local(row: PropRow) -> mat4x4<f32> {
    return transpose(mat4x4<f32>(row.world_x, row.world_y, row.world_z, vec4<f32>(0.0, 0.0, 0.0, 1.0)));
}
// Same result as Bevy's `get_local_from_world`: the glam inverse of the 3x3, then the translation.
fn prop_local_from_world(world_from_local: mat4x4<f32>) -> mat4x4<f32> {
    let x = world_from_local[0].xyz;
    let y = world_from_local[1].xyz;
    let z = world_from_local[2].xyz;
    let c0 = cross(y, z);
    let c1 = cross(z, x);
    let c2 = cross(x, y);
    let inverse_det = 1.0 / dot(z, c2);
    let inverse = transpose(mat3x3<f32>(c0 * inverse_det, c1 * inverse_det, c2 * inverse_det));
    let rotation_scale = mat4x4<f32>(
        vec4<f32>(inverse[0], 0.0),
        vec4<f32>(inverse[1], 0.0),
        vec4<f32>(inverse[2], 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0),
    );
    let translation = mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(-world_from_local[3].xyz, 1.0),
    );
    return rotation_scale * translation;
}
fn prop_determinant_sign(world_from_local: mat4x4<f32>) -> f32 {
    let det = dot(world_from_local[0].xyz, cross(world_from_local[1].xyz, world_from_local[2].xyz));
    return select(1.0, -1.0, det < 0.0);
}
// Same result as Bevy's inverse transpose: the cofactor matrix times the determinant sign.
fn prop_normal_local_to_world(world_from_local: mat4x4<f32>, normal: vec3<f32>) -> vec3<f32> {
    if all(normal == vec3<f32>(0.0)) {
        return normal;
    }
    let x = world_from_local[0].xyz;
    let y = world_from_local[1].xyz;
    let z = world_from_local[2].xyz;
    let cofactor = mat3x3<f32>(cross(y, z), cross(z, x), cross(x, y));
    return normalize(cofactor * normal) * prop_determinant_sign(world_from_local);
}
fn prop_tangent_local_to_world(world_from_local: mat4x4<f32>, tangent: vec4<f32>) -> vec4<f32> {
    if all(tangent == vec4<f32>(0.0)) {
        return tangent;
    }
    let model = mat3x3<f32>(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
    return vec4<f32>(normalize(model * tangent.xyz), tangent.w * prop_determinant_sign(world_from_local));
}
