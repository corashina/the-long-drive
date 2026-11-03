struct SourceLight {
    cookie_from_world: mat4x4<f32>,
    position_range: vec4<f32>,
    direction_kind: vec4<f32>,
    color: vec4<f32>,
    cookie_rect: vec4<f32>,
    mask: vec4<u32>,
    cookie_size: f32,
}
struct SourceLighting {
    lights: array<SourceLight,64>,
    ambient: vec4<f32>,
    fog: vec4<f32>,
    counts: vec4<u32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> lamp_lighting: SourceLighting;
// Cookie UV derivative by the chain rule. Callers take world derivatives before the lamp loop, in uniform control flow.
fn cookie_uv_gradient(lamp: SourceLight, local: vec4<f32>, world_gradient: vec4<f32>) -> vec2<f32> {
    let local_gradient = lamp.cookie_from_world * world_gradient;
    if lamp.direction_kind.w == 0.0 { return local_gradient.xy / lamp.cookie_size; }
    return (local_gradient.xy * local.w - local.xy * local_gradient.w) / (local.w * local.w);
}
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var cookies: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var cookie_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var attenuation: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> receiver: vec4<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var default_cookie: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var default_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var attenuation_sampler: sampler;
fn power5(v:f32)->f32 { let v2=v*v; return v2*v2*v; }

// GLSL ES 3.00 has no findLSB. Input is a nonzero mask word.
fn object_first_light(mask: u32) -> u32 {
    var bits = mask;
    var index = 0u;
    if (bits & 0xffffu) == 0u { bits >>= 16u; index += 16u; }
    if (bits & 0xffu) == 0u { bits >>= 8u; index += 8u; }
    if (bits & 0xfu) == 0u { bits >>= 4u; index += 4u; }
    if (bits & 0x3u) == 0u { bits >>= 2u; index += 2u; }
    if (bits & 0x1u) == 0u { index += 1u; }
    return index;
}
