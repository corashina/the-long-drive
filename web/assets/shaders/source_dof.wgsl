// Post stack v2 DepthOfField with the Small kernel. The CoC temporal filter runs while TAA is active.
// Unity UV starts at the bottom. Every step is symmetric, so screen UV works unchanged.
struct DofParams {
    // x = far / near - 1, y = far, z = focus distance, w = lens coefficient.
    depth: vec4<f32>,
    // x = max CoC, y = 1 / max CoC, z = 1 / aspect.
    coc: vec4<f32>,
    // _TaaParams: TAA jitter in screen UV, then the history blend.
    taa: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: DofParams;
@group(0) @binding(1) var camera_depth: texture_depth_2d;
@group(0) @binding(2) var main_texture: texture_2d<f32>;
@group(0) @binding(3) var coc_texture: texture_2d<f32>;
@group(0) @binding(4) var dof_texture: texture_2d<f32>;
@group(0) @binding(5) var linear_clamp: sampler;
@group(0) @binding(6) var motion_texture: texture_2d<f32>;

const PI: f32 = 3.14159265359;
// DiskKernels.hlsl KERNEL_SMALL: rings = 2, points per ring = 5.
const KERNEL: array<vec2<f32>, 16> = array(
    vec2(0., 0.),
    vec2(0.54545456, 0.),
    vec2(0.16855472, 0.5187581),
    vec2(-0.44128203, 0.3206101),
    vec2(-0.44128197, -0.3206102),
    vec2(0.1685548, -0.5187581),
    vec2(1., 0.),
    vec2(0.809017, 0.58778524),
    vec2(0.30901697, 0.95105654),
    vec2(-0.30901703, 0.9510565),
    vec2(-0.80901706, 0.5877852),
    vec2(-1., 0.),
    vec2(-0.80901694, -0.58778536),
    vec2(-0.30901664, -0.9510566),
    vec2(0.30901712, -0.9510565),
    vec2(0.80901694, -0.5877853),
);

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2. - 1., 0., 1.);
}
fn texel(t: texture_2d<f32>) -> vec2<f32> {
    return 1. / vec2<f32>(textureDimensions(t));
}
fn max3(v: vec3<f32>) -> f32 {
    return max(v.x, max(v.y, v.z));
}

// FragCoC. The raw depth is reversed: 0 is the far plane.
@fragment
fn coc(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let raw = textureLoad(camera_depth, vec2<i32>(position.xy), 0);
    let depth = params.depth.y / fma(params.depth.x, raw, 1.);
    let coc = (depth - params.depth.z) * params.depth.w / max(depth, 1e-4);
    return vec4(saturate(coc * 0.5 * params.coc.y + 0.5));
}

// FragTempFilter. `main_texture` is the CoC history and `coc_texture` the new CoC.
@fragment
fn temporal(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let t = texel(main_texture);
    let uv = position.xy * t;
    let coc = vec4(
        textureSampleLevel(coc_texture, linear_clamp, uv - vec2(t.x, 0.), 0.).r,
        textureSampleLevel(coc_texture, linear_clamp, uv - vec2(0., t.y), 0.).r,
        textureSampleLevel(coc_texture, linear_clamp, uv + vec2(0., t.y), 0.).r,
        textureSampleLevel(coc_texture, linear_clamp, uv + vec2(t.x, 0.), 0.).r,
    );
    let offsets = array(vec2(-t.x, 0.), vec2(0., -t.y), vec2(0., t.y), vec2(t.x, 0.));
    // The dejittered center sample.
    let coc0 = textureSampleLevel(coc_texture, linear_clamp, uv - params.taa.xy, 0.).r;
    // CoC dilation: the closest point of the four neighbors.
    var closest = vec3(0., 0., coc0);
    for (var i = 0; i < 4; i++) {
        if coc[i] < closest.z {
            closest = vec3(offsets[i], coc[i]);
        }
    }
    let motion = textureSampleLevel(motion_texture, linear_clamp, uv + closest.xy, 0.).xy;
    let history = textureSampleLevel(main_texture, linear_clamp, uv - motion, 0.).r;
    let coc_max = max(max(max(coc0, coc.x), max(coc.y, coc.z)), coc.w);
    return vec4(mix(coc0, clamp(history, closest.z, coc_max), params.taa.z));
}

// FragPrefilter: the gathered 2x2 source block of this half-resolution texel.
@fragment
fn prefilter(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(main_texture)) - 1;
    let base = vec2<i32>(position.xy) * 2;
    var avg = vec3(0.);
    var weight = 0.;
    var coc_min = 1e30;
    var coc_max = -1e30;
    for (var i = 0; i < 4; i++) {
        let p = min(base + vec2(i & 1, i >> 1u), size);
        let c = textureLoad(main_texture, p, 0).rgb;
        let coc = textureLoad(coc_texture, p, 0).r * 2. - 1.;
        // CoC and luma weights reduce bleeding and flickering.
        let w = abs(coc) / (max3(c) + 1.);
        avg += c * w;
        weight += w;
        coc_min = min(coc_min, coc);
        coc_max = max(coc_max, coc);
    }
    avg /= max(weight, 1e-4);
    let coc = select(coc_max, coc_min, -coc_min > coc_max) * params.coc.x;
    avg *= smoothstep(0., texel(main_texture).y * 2., abs(coc));
    return vec4(avg, coc);
}

// FragBlur with the disk kernel.
@fragment
fn blur(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let t = texel(main_texture);
    let uv = position.xy * t;
    let samp0 = textureSampleLevel(main_texture, linear_clamp, uv, 0.);
    var bg = vec4(0.);
    var fg = vec4(0.);
    let margin = t.y * 2.;
    for (var i = 0; i < 16; i++) {
        let disp = KERNEL[i] * params.coc.x;
        let dist = length(disp);
        let duv = vec2(disp.x * params.coc.z, disp.y);
        let samp = textureSampleLevel(main_texture, linear_clamp, uv + duv, 0.);
        let bg_coc = max(min(samp0.a, samp.a), 0.);
        let bg_weight = saturate((bg_coc - dist + margin) / margin);
        // Focused areas are dark after the CoC premultiply. Only the near field needs this cut.
        let fg_weight = saturate((-samp.a - dist + margin) / margin) * step(t.y, -samp.a);
        bg += vec4(samp.rgb, 1.) * bg_weight;
        fg += vec4(samp.rgb, 1.) * fg_weight;
    }
    bg = vec4(bg.rgb / (bg.a + f32(bg.a == 0.)), smoothstep(t.y, t.y * 2., samp0.a));
    fg = vec4(fg.rgb / (fg.a + f32(fg.a == 0.)), fg.a * PI / 16.);
    let alpha = saturate(fg.a);
    return vec4(mix(bg.rgb, fg.rgb, alpha), alpha);
}

// FragPostBlur: 9 tap tent filter with 4 bilinear samples.
@fragment
fn postfilter(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let t = texel(main_texture);
    let uv = position.xy * t;
    let d = t * 0.5;
    var acc = textureSampleLevel(main_texture, linear_clamp, uv - d, 0.);
    acc += textureSampleLevel(main_texture, linear_clamp, uv - vec2(-d.x, d.y), 0.);
    acc += textureSampleLevel(main_texture, linear_clamp, uv + vec2(-d.x, d.y), 0.);
    acc += textureSampleLevel(main_texture, linear_clamp, uv + d, 0.);
    return acc / 4.;
}

// FragCombine.
@fragment
fn combine(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let t = texel(main_texture);
    let uv = position.xy * t;
    let dof = textureSampleLevel(dof_texture, linear_clamp, uv, 0.);
    let coc = (textureSampleLevel(coc_texture, linear_clamp, uv, 0.).r - 0.5) * 2. * params.coc.x;
    // The far field alpha.
    let ffa = smoothstep(t.y * 2., t.y * 4., coc);
    let color = textureLoad(main_texture, vec2<i32>(position.xy), 0);
    let alpha = max3(dof.rgb);
    return mix(color, vec4(dof.rgb, alpha), ffa + dof.a - ffa * dof.a);
}
