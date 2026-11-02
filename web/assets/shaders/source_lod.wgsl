// Native unity_DitherMask alpha bytes. See WorldDitherReference provenance.
fn source_lod_keep(position: vec2<f32>, fade: f32) -> bool {
    let alpha = array<u32,16>(0u,192u,48u,240u,128u,64u,176u,112u,32u,224u,16u,208u,160u,96u,144u,80u);
    let pixel = vec2<u32>(floor(position)) & vec2<u32>(3u);
    let mask = f32(alpha[pixel.y * 4u + pixel.x]) / 255.;
    return fade - mask * select(-1., 1., fade > 0.) >= 0.;
}
