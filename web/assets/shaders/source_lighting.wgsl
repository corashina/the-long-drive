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
// Computes the cookie UV derivative by the chain rule. Callers take world derivatives before the lamp loop, in uniform control flow.
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
fn pow5(value: f32) -> f32 { let square = value * value; return square * square * value; }

// Unity 2021.3 BRDF1_Unity_PBS, Linear color space, desktop GGX branch. `specular_on` false drops the specular term.
fn source_brdf(diffuse: vec3<f32>, specular: vec3<f32>, smoothness: f32, normal: vec3<f32>, view: vec3<f32>, light: vec3<f32>, specular_on: bool) -> vec3<f32> {
    let half_sum = light + view;
    let half_dir = half_sum * inverseSqrt(max(dot(half_sum, half_sum), 0.001));
    let nv = abs(dot(normal, view));
    let nl = clamp(dot(normal, light), 0.0, 1.0);
    let nh = clamp(dot(normal, half_dir), 0.0, 1.0);
    let lh = clamp(dot(light, half_dir), 0.0, 1.0);
    let perceptual = 1.0 - smoothness;
    let roughness = max(perceptual * perceptual, 0.002);
    let fd90 = 0.5 + 2.0 * lh * lh * perceptual;
    let disney = (1.0 + (fd90 - 1.0) * pow5(1.0 - nl)) * (1.0 + (fd90 - 1.0) * pow5(1.0 - nv));
    let visibility = 0.5 / (nl * (nv * (1.0 - roughness) + roughness) + nv * (nl * (1.0 - roughness) + roughness) + 0.00001);
    let a2 = roughness * roughness;
    let d = (nh * a2 - nh) * nh + 1.0;
    var specular_term = max(0.0, visibility * a2 / (d * d + 0.0000001) * nl);
    if !specular_on { specular_term = 0.0; }
    let fresnel = specular + (vec3(1.0) - specular) * pow5(1.0 - lh);
    return diffuse * disney * nl + specular_term * fresnel;
}

// GLSL ES 3.00 has no findLSB. The input is a nonzero mask word.
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
