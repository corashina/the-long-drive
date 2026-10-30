#define_import_path source_float_store
// D3D11.3 3.2.2: finite narrowing truncates and saturates overflow; existing Inf/NaN remain exceptional.
// This integer helper is the exact AO-depth revision2 helper tested through ANGLE.
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
fn half_store(value: f32) -> f32 { return bitcast<f32>(half_store_bits(bitcast<u32>(value))); }
fn half2(value: vec2<f32>) -> vec2<f32> { return vec2(half_store(value.x), half_store(value.y)); }
fn half4(value: vec4<f32>) -> vec4<f32> { return vec4(half_store(value.x), half_store(value.y), half_store(value.z), half_store(value.w)); }
fn nan(value: f32) -> bool { return (bitcast<u32>(value) & 0x7fffffffu) > 0x7f800000u; }
fn compare_value(value: f32) -> f32 {
    let bits = bitcast<u32>(value);
    return bitcast<f32>(select(bits, bits & 0x80000000u, (bits & 0x7fffffffu) < 0x00800000u));
}
fn source_max(a: f32, b: f32) -> f32 {
    if nan(a) { return b; }
    if nan(b) { return a; }
    return select(a, b, compare_value(a) < compare_value(b));
}
fn source_min(a: f32, b: f32) -> f32 {
    if nan(a) { return b; }
    if nan(b) { return a; }
    return select(a, b, compare_value(b) < compare_value(a));
}
fn source_sat(value: f32) -> f32 {
    let bits = bitcast<u32>(value);
    let magnitude = bits & 0x7fffffffu;
    var result = bits;
    if magnitude > 0x7f800000u || (bits & 0x80000000u) != 0u { result = 0u; }
    else if magnitude > 0x3f800000u { result = 0x3f800000u; }
    return bitcast<f32>(result);
}
