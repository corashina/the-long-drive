// Installed MultiScaleVORender_interleaved, kernel1.
// Scalar source slices map to RGBA phase X and array layer phase Y.
struct AoParams {
    inverse_thickness: array<vec4<f32>, 3>,
    weights: array<vec4<f32>, 3>,
    settings: vec4<f32>, // reject fadeoff, intensity
}
@group(0) @binding(0) var<uniform> params: AoParams;
@group(0) @binding(1) var tiled_depth: texture_2d_array<f32>;
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
}
fn nan(value: f32) -> bool {
    return (bitcast<u32>(value) & 0x7fffffffu) > 0x7f800000u;
}
fn flush(value: f32) -> f32 {
    let bits = bitcast<u32>(value);
    if (bits & 0x7f800000u) == 0u { return bitcast<f32>(bits & 0x80000000u); }
    return value;
}
// D3D min/max select the other operand when exactly one input is NaN.
// Integer classification prevents ANGLE from folding an ordered float NaN test.
fn source_min(a: f32, b: f32) -> f32 {
    if nan(a) { return b; }
    if nan(b) { return a; }
    return min(flush(a), flush(b));
}
fn source_max(a: f32, b: f32) -> f32 {
    if nan(a) { return b; }
    if nan(b) { return a; }
    return max(flush(a), flush(b));
}
fn source_sat(value: f32) -> f32 {
    return source_min(1.0, source_max(0.0, value));
}
fn depth(p: vec2<i32>, phase: vec2<i32>) -> f32 {
    let extent = vec2<i32>(textureDimensions(tiled_depth));
    let value = textureLoad(tiled_depth, clamp(p, vec2(0), extent - vec2(1)), phase.y, 0);
    return value[phase.x];
}
fn pair(center: vec2<i32>, phase: vec2<i32>, offset: vec2<i32>, inv_range: f32, front: f32) -> f32 {
    let a = fma(depth(center + offset, phase), inv_range, -front);
    let b = fma(depth(center - offset, phase), inv_range, -front);
    let pa = source_sat(params.settings.x * a);
    let pb = source_sat(params.settings.x * b);
    let first = source_min(source_max(a, pb), 1.0);
    let second = source_min(source_max(pa, b), 1.0);
    return source_sat(fma(-pb, pa, first + second));
}
fn pattern(center: vec2<i32>, phase: vec2<i32>, offset: vec2<i32>, inv_depth: f32, index: u32) -> f32 {
    let thickness = params.inverse_thickness[index >> 2u][index & 3u];
    let inv_range = inv_depth * thickness;
    let front = thickness - 0.5;
    if offset.y == 0 {
        return pair(center, phase, offset, inv_range, front)
            + pair(center, phase, offset.yx, inv_range, front);
    }
    var result = pair(center, phase, offset, inv_range, front)
        + pair(center, phase, vec2(-offset.x, offset.y), inv_range, front);
    if offset.x != offset.y {
        result += pair(center, phase, offset.yx, inv_range, front);
        result += pair(center, phase, vec2(-offset.y, offset.x), inv_range, front);
    }
    return result;
}
// D3D11.3 section3.2.3.6: NaN -> 0, infinities clamp before UNorm storage.
fn unorm_bits(bits: u32) -> u32 {
    let magnitude = bits & 0x7fffffffu;
    if magnitude > 0x7f800000u || (bits & 0x80000000u) != 0u { return 0u; }
    if magnitude > 0x3f800000u { return 0x3f800000u; }
    return bits;
}
fn unorm_value(value: f32) -> f32 { return bitcast<f32>(unorm_bits(bitcast<u32>(value))); }
@fragment
fn render(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let pixel = vec2<i32>(position.xy);
    let center = pixel >> vec2(2);
    let phase = pixel & vec2(3);
    let inv_depth = 1.0 / depth(center, phase);
    // Installed DXBC accumulates weights in order 3,1,4,8,11,6,10.
    var result = (pattern(center, phase, vec2(4,0), inv_depth, 3u) * params.weights[0].w) * 0.5;
    result = fma(pattern(center, phase, vec2(2,0), inv_depth, 1u) * params.weights[0].y, 0.5, result);
    result = fma(pattern(center, phase, vec2(1,1), inv_depth, 4u) * params.weights[1].x, 0.5, result);
    result = fma(pattern(center, phase, vec2(2,2), inv_depth, 8u) * params.weights[2].x, 0.5, result);
    result = fma(pattern(center, phase, vec2(3,3), inv_depth, 11u) * params.weights[2].w, 0.5, result);
    result = fma(pattern(center, phase, vec2(1,3), inv_depth, 6u) * params.weights[1].z, 0.25, result);
    result = fma(pattern(center, phase, vec2(2,4), inv_depth, 10u) * params.weights[2].z, 0.25, result);
    return unorm_value(fma(params.settings.y, result - 1.0, 1.0));
}
