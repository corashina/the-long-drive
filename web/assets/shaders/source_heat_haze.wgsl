// Installed Custom/heathazeshader fragment (blob 2), exact DXBC order.
// Unity image-effect UV starts at the bottom. `unity_uv` is that UV; the targets here start at the top.
struct Params {
    extent: vec4<f32>,
    up_offset_scale: vec4<f32>,
    down_offset_scale: vec4<f32>,
    // _Weight, _MinDist, _DistanceM, _Time.y
    values: vec4<f32>,
    // _UpSpeed, _UpSpeed2, _UpStrength, 0
    up: vec4<f32>,
    // _DownSpeed, _DownSpeed2, _DownStrength, 0
    down: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source_color: texture_2d<f32>;
@group(0) @binding(2) var linear_clamp: sampler;
@group(0) @binding(3) var camera_depth: texture_2d<f32>;
@group(0) @binding(4) var noise: texture_2d<f32>;
@group(0) @binding(5) var noise_sampler: sampler;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2. - 1., 0., 1.);
}
@fragment fn haze(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy * params.extent.zw;
    let unity_uv = vec2(uv.x, 1. - uv.y);
    let time = params.values.w;
    let down_uv = fma(params.down_offset_scale.zw, unity_uv, params.down_offset_scale.xy)
        + vec2(sin(params.down.y * time), -params.down.x * time);
    var amount = textureSample(noise, noise_sampler, down_uv).x * params.down.z;
    let up_uv = fma(params.up_offset_scale.zw, unity_uv, params.up_offset_scale.xy)
        + vec2(sin(params.up.y * time), -params.up.x * time);
    amount = fma(textureSample(noise, noise_sampler, up_uv).x, params.up.z, amount);
    // Reversed depth: 0 is far. Near pixels below 1 - _MinDist do not move.
    let depth = textureLoad(camera_depth, vec2<i32>(position.xy), 0).x;
    amount = saturate((1. - depth) - params.values.y) * amount * params.values.z;
    let moved = fma(vec2(amount), vec2(params.values.x), unity_uv);
    return textureSampleLevel(source_color, linear_clamp, vec2(moved.x, 1. - moved.y), 0.);
}
