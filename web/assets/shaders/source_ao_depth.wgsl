// Installed MultiScaleVO depth-copy and selected downsample kernels.
// Four RGBA16F attachments retain sixteen scalar half-float phases.
struct DepthParams {
    projection: vec4<f32>, // x = far/near - 1; y = 1
    control: vec4<u32>, // x = input stride; y = linearize raw depth; z = half store
}
@group(0) @binding(0) var<uniform> params: DepthParams;
@group(0) @binding(1) var input_depth: texture_2d<f32>;
@group(0) @binding(2) var camera_depth: texture_2d<f32>;
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
}
// D3D11.3 sections 3.2.2 and 22.4.9: typed half stores truncate.
// Return an exactly representable half value before the attachment conversion.
fn half_store_bits(bits: u32) -> u32 {
    let magnitude = bits & 0x7fffffffu;
    let sign = bits & 0x80000000u;
    if magnitude >= 0x7f800000u { return bits; }
    if magnitude >= 0x477fe000u { return sign | 0x477fe000u; }
    if magnitude >= 0x38800000u { return bits & 0xffffe000u; }
    if magnitude < 0x33800000u { return sign; }
    let discarded = 126u - (magnitude >> 23u);
    return bits & ~((1u << discarded) - 1u);
}
fn half_store(value: f32) -> f32 {
    // Keep branch merges integer-valued. ANGLE folds signed float zero returns.
    return bitcast<f32>(half_store_bits(bitcast<u32>(value)));
}
fn load_zero(p: vec2<i32>) -> f32 {
    if any(p < vec2(0)) || any(p >= vec2<i32>(textureDimensions(input_depth))) {
        return 0.0;
    }
    return textureLoad(input_depth, p, 0).r;
}
fn selected_depth(p: vec2<i32>) -> f32 {
    let raw = load_zero(p * i32(params.control.x));
    if params.control.y == 0u { return raw; }
    if raw == 0.0 { return 100000.0; }
    return 1.0 / fma(params.projection.x, raw, params.projection.y);
}
@fragment
fn copy_depth(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let size = vec2<i32>(textureDimensions(camera_depth));
    let pixel = clamp(vec2<i32>(floor(position.xy)), vec2(0), size - vec2(1));
    return textureLoad(camera_depth, pixel, 0).r;
}
@fragment
fn downsample(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let value = selected_depth(vec2<i32>(position.xy));
    if params.control.z != 0u { return half_store(value); }
    return value;
}
struct PhaseRows {
    @location(0) row0: vec4<f32>,
    @location(1) row1: vec4<f32>,
    @location(2) row2: vec4<f32>,
    @location(3) row3: vec4<f32>,
}
fn phase_row(p: vec2<i32>, y: i32) -> vec4<f32> {
    return vec4(
        half_store(selected_depth(p + vec2(0, y))), half_store(selected_depth(p + vec2(1, y))),
        half_store(selected_depth(p + vec2(2, y))), half_store(selected_depth(p + vec2(3, y))));
}
@fragment
fn tile_depth(@builtin(position) position: vec4<f32>) -> PhaseRows {
    let p = vec2<i32>(position.xy) * 4;
    return PhaseRows(phase_row(p, 0), phase_row(p, 1), phase_row(p, 2), phase_row(p, 3));
}
