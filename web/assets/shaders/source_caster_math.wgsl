#define_import_path source_caster_math

// Installed Standard SHADOWS_DEPTH: vertices9666/9684; fragments9759/9763.
fn caster_cutout_margin(main_alpha: f32, color_alpha: f32, cutoff: f32) -> f32 {
    return fma(main_alpha, color_alpha, -cutoff);
}
fn caster_premultiplied_alpha(main_alpha: f32, color_alpha: f32, metallic: f32) -> f32 {
    let alpha = main_alpha * color_alpha;
    let one_minus_reflectivity = fma(-metallic, 0.96, 0.96);
    return fma(alpha, one_minus_reflectivity, 1.0 - one_minus_reflectivity);
}
fn caster_dither_coordinate(source_pixel: vec2<f32>, alpha: f32) -> vec3<f32> {
    return vec3(source_pixel * 0.25, alpha * 0.9375);
}
fn caster_normal_bias(position: vec3<f32>, normal: vec3<f32>, direction_to_light: vec3<f32>, bias: f32) -> vec3<f32> {
    // The source movc preserves the original position for zero bias, including invalid normal intermediates.
    if bias == 0.0 { return position; }
    let cosine = dot(normal, direction_to_light);
    let inset = sqrt(fma(-cosine, cosine, 1.0)) * bias;
    return fma(-normal, vec3(inset), position);
}
fn caster_is_nan(value: f32) -> bool { return (bitcast<u32>(value) & 0x7fffffffu) > 0x7f800000u; }
fn caster_min(left: f32, right: f32) -> f32 {
    if caster_is_nan(left) { return right; }
    if caster_is_nan(right) { return left; }
    return min(left, right);
}
fn caster_clip_bias(clip: vec4<f32>, bias: vec2<f32>) -> vec4<f32> {
    let offset = max(caster_min(bias.x / clip.w, 0.0), -1.0);
    let depth = clip.z + offset;
    let bounded = caster_min(clip.w, depth);
    return vec4(clip.xy, fma(bias.y, bounded - depth, depth), clip.w);
}
fn caster_transform(matrix: mat4x4<f32>, point: vec4<f32>) -> vec4<f32> {
    // DXBC multiply/add order: Y, X, Z, W.
    var result = matrix[1] * point.y;
    result = fma(matrix[0], vec4(point.x), result);
    result = fma(matrix[2], vec4(point.z), result);
    return fma(matrix[3], vec4(point.w), result);
}
