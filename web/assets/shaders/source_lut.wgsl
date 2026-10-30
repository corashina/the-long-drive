#define_import_path source_lut
#import source_float_store::{half4, source_max, source_min, source_sat}
struct Params {
    size: vec4<f32>, balance: vec4<f32>, color_filter: vec4<f32>, hue_sat_con: vec4<f32>,
    mixer_r: vec4<f32>, mixer_g: vec4<f32>, mixer_b: vec4<f32>,
    lift: vec4<f32>, inverse_gamma: vec4<f32>, gain: vec4<f32>,
    slice: vec4<u32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var curves: texture_2d<f32>;
@group(0) @binding(2) var curve_sampler: sampler;
fn f(bits: u32) -> f32 { return bitcast<f32>(bits); }
fn positive(v: vec3<f32>) -> vec3<f32> {
    return vec3(source_max(v.x, 0.), source_max(v.y, 0.), source_max(v.z, 0.));
}
fn sat3(v: vec3<f32>) -> vec3<f32> { return vec3(source_sat(v.x),source_sat(v.y),source_sat(v.z)); }
fn luminance(v: vec3<f32>) -> f32 {
    return dot(v, vec3(f(0x3e59c6edu), f(0x3f371437u), f(0x3d93d07du)));
}
// Installed KGenLut3D_NeutralTonemap, exact DXBC immediate words, up to the tonemap.
// PPv2 Lut3DBaker.compute ColorGrade: LogGrade, LUT_SPACE_DECODE, LinearGrade, then max(0).
fn grade(id: vec3<f32>) -> vec3<f32> {
    var color = fma(id, vec3(params.size.y), vec3(f(0xbed3c1dcu)));
    color = fma(color, vec3(params.hue_sat_con.z), vec3(f(0x3ce1b590u)));
    color = (exp2(color * f(0x4159b00eu)) + f(0xbd449774u)) * f(0x3e3851ebu);
    var lms = vec3(
        dot(vec3(f(0x3ec7e32au),f(0x3f0cc8efu),f(0x3c123fb3u)),color),
        dot(vec3(f(0x3d911567u),f(0x3f769271u),f(0x3ab1f688u)),color),
        dot(vec3(f(0x3cbd4d68u),f(0x3e0317f0u),f(0x3f6fadc1u)),color));
    lms *= params.balance.xyz;
    color = vec3(
        dot(vec3(f(0x4036f12cu),f(0xbfd07c31u),f(0xbccbe836u)),lms),
        dot(vec3(f(0xbe5739f3u),f(0x3f943fe6u),f(0x39aa0442u)),lms),
        dot(vec3(f(0xbd2b430fu),f(0xbdf20297u),f(0x3f88ca2eu)),lms)) * params.color_filter.xyz;
    color = vec3(dot(color,params.mixer_r.xyz),dot(color,params.mixer_g.xyz),dot(color,params.mixer_b.xyz));
    color = fma(color,params.gain.xyz,params.lift.xyz);
    let sign_value = fma(sat3(fma(color,vec3(f(0x7f7fffffu)),vec3(.5))),vec3(2.),vec3(-1.));
    color = positive(exp2(log2(abs(color))*params.inverse_gamma.xyz)*sign_value);
    // Preserve the installed arithmetic selections before HSV curve sampling.
    let choose_y = select(0.,1.,color.y >= color.z);
    var p = fma(vec4(choose_y), vec4(color.yz-color.zy,1.,-1.),vec4(color.zy,-1.,f(0x3f2aaaabu)));
    let choose_x = select(0.,1.,color.x >= p.x);
    let q0 = vec4(p.x,p.y,p.w,color.x);
    let q1 = vec4(color.x,p.y,p.z,p.x);
    let q = fma(vec4(choose_x),q1-q0,q0);
    let delta = q.x-source_min(q.y,q.w);
    let hue = abs((q.w-q.y)/fma(delta,6.,f(0x38d1b717u))+q.z);
    let saturation = delta/(q.x+f(0x38d1b717u));
    var sat_mult = source_sat(textureSampleLevel(curves,curve_sampler,vec2(hue,.25),0.).y)*2.;
    let sat_curve = source_sat(textureSampleLevel(curves,curve_sampler,vec2(saturation,.25),0.).z);
    sat_mult = dot(vec2(sat_curve),vec2(sat_mult));
    sat_mult *= source_sat(textureSampleLevel(curves,curve_sampler,vec2(luminance(color),.25),0.).w);
    let shifted = hue+params.hue_sat_con.x;
    let hue_curve = source_sat(textureSampleLevel(curves,curve_sampler,vec2(shifted,.25),0.).x);
    let hue_sum = shifted+hue_curve;
    var rotated = hue_sum-.5;
    if rotated > 1. { rotated = hue_sum-1.5; }
    if hue_sum-.5 < 0. { rotated = hue_sum+.5; }
    let hue_rgb = sat3(abs(fma(fract(vec3(rotated)+vec3(1.,f(0x3f2aaaabu),f(0x3eaaaaabu))),vec3(6.),vec3(-3.)))-1.);
    let rgb_scale = fma(vec3(saturation),hue_rgb-1.,vec3(1.));
    let rgb = rgb_scale*q.x;
    let lum = luminance(rgb);
    let sat_factor = dot(vec2(params.hue_sat_con.y),vec2(sat_mult));
    return positive(fma(vec3(sat_factor),fma(vec3(q.x),rgb_scale,vec3(-lum)),vec3(lum)));
}
// Installed NeutralTonemap and the Eval max(0), exact DXBC immediate words.
fn neutral(color: vec3<f32>) -> vec3<f32> {
    let scaled = color*f(0x3fa81d09u);
    let numerator = fma(scaled,fma(color,vec3(f(0x3e867da1u)),vec3(f(0x3d8e8a71u))),vec3(f(0x3bb24207u)));
    let denominator = fma(scaled,fma(color,vec3(f(0x3e867da1u)),vec3(f(0x3e947ae1u))),vec3(f(0x3da71de8u)));
    return positive((numerator/denominator+f(0xbd888888u))*f(0x3fa81d09u));
}
@vertex fn vertex(@builtin(vertex_index) index:u32) -> @builtin(position) vec4<f32> {
    let uv=vec2<f32>(f32((index<<1u)&2u),f32(index&2u));
    return vec4(uv*2.-1.,0.,1.);
}
@fragment fn bake(@builtin(position) position:vec4<f32>) -> @location(0) vec4<f32> {
    return half4(vec4(neutral(grade(vec3(floor(position.xy),f32(params.slice.x)))),1.));
}
// PPv2 KGenLut3D_NoTonemap: ColorGrade without a tonemap. The Eval max(0) repeats the ColorGrade max(0).
@fragment fn bake_none(@builtin(position) position:vec4<f32>) -> @location(0) vec4<f32> {
    return half4(vec4(grade(vec3(floor(position.xy),f32(params.slice.x))),1.));
}
