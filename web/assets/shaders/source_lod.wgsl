// Native unity_DitherMask alpha bytes. See WorldDitherReference provenance.
// The cells that a signed LOD fade keeps: the count of kept levels (0 to 16), and bit 5 when the fade is positive.
// A positive fade keeps the levels with alpha <= fade; a negative fade keeps the levels with alpha >= -fade.
// `lod_code` in object_lighting.rs uses the same f32 test.
fn source_lod_code(fade: f32) -> u32 {
    let sign = select(-1., 1., fade > 0.);
    var kept = 0u;
    for (var level = 0u; level < 16u; level++) {
        if fade - f32(level * 16u) / 255. * sign >= 0. { kept++; }
    }
    return kept | select(0u, 32u, fade > 0.);
}
fn source_lod_keep_code(position: vec2<f32>, code: u32) -> bool {
    // The bytes are the multiples of 16 below 256, so the cell level is alpha / 16.
    let alpha = array<u32,16>(0u,192u,48u,240u,128u,64u,176u,112u,32u,224u,16u,208u,160u,96u,144u,80u);
    let pixel = vec2<u32>(floor(position)) & vec2<u32>(3u);
    let level = alpha[pixel.y * 4u + pixel.x] / 16u;
    let kept = code & 31u;
    return select(level >= 16u - kept, level < kept, (code & 32u) != 0u);
}
// The same cells as `source_lod_keep_code(position, source_lod_code(fade))`, for a fade in a material.
fn source_lod_keep(position: vec2<f32>, fade: f32) -> bool {
    let alpha = array<u32,16>(0u,192u,48u,240u,128u,64u,176u,112u,32u,224u,16u,208u,160u,96u,144u,80u);
    let pixel = vec2<u32>(floor(position)) & vec2<u32>(3u);
    let mask = f32(alpha[pixel.y * 4u + pixel.x]) / 255.;
    return fade - mask * select(-1., 1., fade > 0.) >= 0.;
}
