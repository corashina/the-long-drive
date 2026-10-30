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

// A bound can exclude a lamp only when all arithmetic remains finite.
fn source_finite3(value: vec3<f32>) -> bool {
    return all((bitcast<vec3<u32>>(value) & vec3(0x7f800000u)) != vec3(0x7f800000u));
}

fn object_light_mask(model: mat4x4<f32>, local_center: vec3<f32>, local_extents: vec3<f32>, bounded: bool) -> vec2<u32> {
    var selected = vec2(0u);
    let center = (model * vec4(local_center, 1.0)).xyz;
    let extents = abs(model[0].xyz) * local_extents.x
        + abs(model[1].xyz) * local_extents.y + abs(model[2].xyz) * local_extents.z;
    // Absolute terms also cover cancellation in the center transform.
    let magnitude = abs(model[0].xyz) * abs(local_center.x)
        + abs(model[1].xyz) * abs(local_center.y)
        + abs(model[2].xyz) * abs(local_center.z) + abs(model[3].xyz) + extents;
    let valid = bounded && all(local_extents >= vec3(0.0))
        && source_finite3(center) && source_finite3(extents) && source_finite3(magnitude);
    for (var i = 0u; i < lamp_lighting.counts.x; i += 1u) {
        let lamp = lamp_lighting.lights[i];
        if lamp.direction_kind.w == 0.0 { continue; }
        if (receiver.x & lamp.mask.x) == 0u { continue; }
        if max(lamp.color.r, max(lamp.color.g, lamp.color.b)) <= 0.0 { continue; }
        if valid {
            let position = lamp.position_range.xyz;
            let range = abs(lamp.position_range.w);
            // 128 f32 epsilons cover transform, subtraction, and squared-distance rounding.
            // Expand rather than round toward exclusion. Boundary contact retains the lamp.
            let padding = (magnitude + abs(position) + vec3(range + 1.0)) * 0.0000152587890625;
            let separation = max(abs(position - center) - extents - padding, vec3(0.0));
            let distance2 = dot(separation, separation);
            let range2 = range * range;
            if source_finite3(padding) && source_finite3(vec3(distance2, range2, range))
                && distance2 > range2 { continue; }
        }
        selected[i >> 5u] |= 1u << (i & 31u);
    }
    return selected;
}
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
