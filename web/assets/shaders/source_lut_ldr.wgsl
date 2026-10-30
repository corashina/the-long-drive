#import source_float_store::{source_max, source_min, source_sat}
// Installed Lut2DBaker pass LutGenLDR (blob 5), exact DXBC immediate words.
// It bakes the 1024x32 ARGB32 strip from the user LDR LUT, then runs ColorGradeLDR and YrgbCurve.
struct Params {
    lut: vec4<f32>, user: vec4<f32>, balance: vec4<f32>, color_filter: vec4<f32>,
    hue_sat_con_brightness: vec4<f32>, mixer_r: vec4<f32>, mixer_g: vec4<f32>, mixer_b: vec4<f32>,
    lift: vec4<f32>, inverse_gamma: vec4<f32>, gain: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var user_lut: texture_2d<f32>;
@group(0) @binding(2) var user_sampler: sampler;
@group(0) @binding(3) var curves: texture_2d<f32>;
@group(0) @binding(4) var curve_sampler: sampler;
fn f(bits: u32) -> f32 { return bitcast<f32>(bits); }
fn sat3(v: vec3<f32>) -> vec3<f32> { return vec3(source_sat(v.x), source_sat(v.y), source_sat(v.z)); }
fn positive(v: vec3<f32>) -> vec3<f32> { return vec3(source_max(v.x, 0.), source_max(v.y, 0.), source_max(v.z, 0.)); }
fn curve(u: f32, v: f32) -> vec4<f32> { return textureSampleLevel(curves, curve_sampler, vec2(u, v), 0.); }
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2. - 1., 0., 1.);
}
@fragment fn bake(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = params;
    // GetLutStripValue: the neutral color of this strip texel.
    let uv = position.xy / vec2(1024., 32.) - p.lut.yz;
    let red = fract(uv.x * p.lut.x);
    let neutral = vec3(red, uv.y, uv.x - red / p.lut.x) * p.lut.w;
    // ApplyLut2D on the user strip, then lerp by the contribution.
    let scaled = neutral.zxy * p.user.z;
    let shift = floor(scaled.x);
    let half_texel = p.user.xy * .5;
    let user_uv = fma(scaled.yz, p.user.xy, half_texel);
    let first = vec2(fma(shift, p.user.y, user_uv.x), user_uv.y);
    let a = textureSampleLevel(user_lut, user_sampler, first, 0.).rgb;
    let b = textureSampleLevel(user_lut, user_sampler, vec2(p.user.y, 0.) + first, 0.).rgb;
    let lookup = fma(vec3(fma(neutral.z, p.user.z, -shift)), b - a, a);
    var color = fma(vec3(p.user.w), fma(-vec3(red, uv.y, uv.x - red / p.lut.x), vec3(p.lut.w), lookup), neutral);
    // ColorGradeLDR: brightness and contrast around the linear mid grey.
    color = fma(color, vec3(p.hue_sat_con_brightness.w), vec3(f(0xbe5edc67u)));
    color = fma(color, vec3(p.hue_sat_con_brightness.z), vec3(f(0x3e5edc67u)));
    var lms = vec3(
        dot(vec3(f(0x3ec7e32au), f(0x3f0cc8efu), f(0x3c123fb3u)), color),
        dot(vec3(f(0x3d911567u), f(0x3f769271u), f(0x3ab1f688u)), color),
        dot(vec3(f(0x3cbd4d68u), f(0x3e0317f0u), f(0x3f6fadc1u)), color)) * p.balance.xyz;
    color = vec3(
        dot(vec3(f(0x4036f12cu), f(0xbfd07c31u), f(0xbccbe836u)), lms),
        dot(vec3(f(0xbe5739f3u), f(0x3f943fe6u), f(0x39aa0442u)), lms),
        dot(vec3(f(0xbd2b430fu), f(0xbdf20297u), f(0x3f88ca2eu)), lms)) * p.color_filter.xyz;
    color = vec3(dot(color, p.mixer_r.xyz), dot(color, p.mixer_g.xyz), dot(color, p.mixer_b.xyz));
    color = fma(color, p.gain.xyz, p.lift.xyz);
    let sign_value = fma(sat3(fma(color, vec3(f(0x7f7fffffu)), vec3(.5))), vec3(2.), vec3(-1.));
    color = positive(sign_value * exp2(log2(abs(color)) * p.inverse_gamma.xyz));
    // RgbToHsv with the installed arithmetic selections.
    let choose_g = select(0., 1., color.y >= color.z);
    let k = fma(vec4(choose_g), vec4(color.y - color.z, color.z - color.y, -1., 1.), vec4(color.z, color.y, f(0x3f2aaaabu), -1.));
    let choose_r = select(0., 1., color.x >= k.x);
    let q0 = vec4(k.x, k.y, k.z, color.x);
    let q1 = vec4(color.x, k.y, k.w, k.x);
    let q = fma(vec4(choose_r), q1 - q0, q0);
    let luminance = dot(color, vec3(f(0x3e59c6edu), f(0x3f371437u), f(0x3d93d07du)));
    let delta = q.x - source_min(q.y, q.w);
    let hue = abs((q.w - q.y) / fma(delta, 6., f(0x38d1b717u)) + q.z);
    let saturation = delta / (q.x + f(0x38d1b717u));
    // Hue vs sat, sat vs sat and lum vs sat, then the saturation.
    let hue_sat = source_sat(curve(hue, .25).y);
    let sat_sat = source_sat(curve(saturation, .25).z);
    let lum_sat = source_sat(curve(luminance, .25).w);
    let sat_mult = dot(vec2(sat_sat), vec2(hue_sat + hue_sat)) * lum_sat;
    let sat_factor = dot(vec2(p.hue_sat_con_brightness.y), vec2(sat_mult));
    // Hue vs hue and RotateHue.
    let shifted = hue + p.hue_sat_con_brightness.x;
    let hue_sum = shifted + source_sat(curve(shifted, .25).x);
    var rotated = hue_sum - .5;
    if rotated > 1. { rotated = hue_sum - 1.5; }
    if hue_sum - .5 < 0. { rotated = hue_sum + .5; }
    let hue_rgb = sat3(abs(fma(fract(vec3(rotated) + vec3(1., f(0x3f2aaaabu), f(0x3eaaaaabu))), vec3(6.), vec3(-3.))) - 1.) - 1.;
    let rgb_scale = fma(vec3(saturation), hue_rgb, vec3(1.));
    let lum = dot(rgb_scale * q.x, vec3(f(0x3e59c6edu), f(0x3f371437u), f(0x3d93d07du)));
    color = sat3(fma(vec3(sat_factor), fma(vec3(q.x), rgb_scale, vec3(-lum)), vec3(lum)));
    // YrgbCurve: master, then red, green and blue.
    color += 1. / 256.;
    color = vec3(source_sat(curve(color.x, .75).w), source_sat(curve(color.y, .75).w), source_sat(curve(color.z, .75).w)) + 1. / 256.;
    return vec4(source_sat(curve(color.x, .75).x), source_sat(curve(color.y, .75).y), source_sat(curve(color.z, .75).z), 1.);
}
