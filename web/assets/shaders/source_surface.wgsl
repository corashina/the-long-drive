#define_import_path source_surface
#import "shaders/source_lod.wgsl"::source_lod_keep
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_vertex_output,
    pbr_bindings,
    mesh_view_bindings,
}
#ifdef SOURCE_GPU_PROPS
#import bevy_pbr::{pbr_types, pbr_functions}
#endif

#import source_float_store::half4

#import source_view_lighting::{lighting, diffuse_sh, indirect_specular, direct_attenuation, lamp_shadow, fog_factor, sun_cookie, sun_cookie_sampler}

#import "shaders/source_lighting.wgsl"::{lamp_lighting, object_first_light, receiver, cookie_uv_gradient, cookies, cookie_sampler, attenuation, default_cookie, default_sampler, attenuation_sampler}

struct SourceUniform {
    color: vec4<f32>,
    emission: vec4<f32>,
    specular: vec4<f32>,
    factors: vec4<f32>,
    scales: vec4<f32>,
    flags: vec4<u32>,
    banks: array<vec4<u32>, 3>,
    layers: array<vec4<u32>, 3>,
    uv: array<vec4<f32>, 9>,
    sampling: array<vec4<f32>, 9>,
    defaults: array<vec4<f32>, 9>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(120) var<uniform> source: SourceUniform;
#ifdef WORLD_TEXTURE_ARRAYS
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var array0: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var sampler0: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var array1: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var sampler1: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var array2: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var sampler2: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var array3: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var sampler3: sampler;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(121) var bank6_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(122) var bank6_sampler: sampler;

#endif

fn source_flag(bit: u32) -> bool { return (source.flags.x & (1u << bit)) != 0u; }
fn sample_source(slot: u32, main_uv: vec2<f32>, detail_uv: vec2<f32>, offset: vec2<f32>) -> vec4<f32> {
    if (source.flags.y & (1u << slot)) == 0u { return source.defaults[slot]; }
    var uv = main_uv;
    if source.sampling[slot].x > 0.5 { uv = detail_uv; }
    uv = uv * source.uv[slot].xy + source.uv[slot].zw + offset;
    // Unity MirrorOnce: reflect negative coordinates, then clamp at the edge.
    uv = select(uv, abs(uv), source.sampling[slot].zw > vec2(0.5));
    let bias = source.sampling[slot].y;
    let bank = source.banks[slot / 4u][slot % 4u];
#ifdef WORLD_TEXTURE_ARRAYS
    let layer = i32(source.layers[slot / 4u][slot % 4u]);
    switch bank {
        case 0u: { return textureSampleBias(array0, sampler0, uv, layer, bias); }
        case 1u: { return textureSampleBias(array1, sampler1, uv, layer, bias); }
        case 2u: { return textureSampleBias(array2, sampler2, uv, layer, bias); }
        default: { return textureSampleBias(array3, sampler3, uv, layer, bias); }
    }
#else
    switch bank {
        case 0u: { return textureSampleBias(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, bias); }
        case 1u: { return textureSampleBias(pbr_bindings::emissive_texture, pbr_bindings::emissive_sampler, uv, bias); }
        case 2u: { return textureSampleBias(pbr_bindings::metallic_roughness_texture, pbr_bindings::metallic_roughness_sampler, uv, bias); }
        case 3u: { return textureSampleBias(pbr_bindings::occlusion_texture, pbr_bindings::occlusion_sampler, uv, bias); }
        case 4u: { return textureSampleBias(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler, uv, bias); }
        case 5u: { return textureSampleBias(pbr_bindings::depth_map_texture, pbr_bindings::depth_map_sampler, uv, bias); }
        default: { return textureSampleBias(bank6_texture, bank6_sampler, uv, bias); }
    }
#endif
}
fn source_normal(sampled: vec4<f32>, scale: f32) -> vec3<f32> {
    let rg = select(sampled.xy, vec2(sampled.r * sampled.a, sampled.g), source.specular.w > 0.5);
    let xy = (rg * 2.0 - 1.0) * scale;
    return vec3(xy, sqrt(1.0 - clamp(dot(xy, xy), 0.0, 1.0)));
}
fn pow5(value: f32) -> f32 { let square = value * value; return square * square * value; }

// Unity 2021.3 BRDF1_Unity_PBS, Linear color space, desktop GGX branch.
fn source_brdf(diffuse: vec3<f32>, specular: vec3<f32>, smoothness: f32, normal: vec3<f32>, view: vec3<f32>, light: vec3<f32>) -> vec3<f32> {
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
    if source_flag(6u) { specular_term = 0.0; }
    let fresnel = specular + (vec3(1.0) - specular) * pow5(1.0 - lh);
    return diffuse * disney * nl + specular_term * fresnel;
}

@fragment
fn fragment(
    in: VertexOutput,
#ifdef VERTEX_TANGENTS
    @location(8) world_bitangent: vec3<f32>,
#endif
    @location(9) @interpolate(flat) source_renderer: vec4<f32>,
    @location(10) source_fog: f32,
#ifdef SOURCE_LAMPS
    @location(12) @interpolate(flat) source_lights: vec2<u32>,
#endif
#ifdef SOURCE_GPU_PROPS
    @location(13) @interpolate(flat) source_fade: f32,
    @location(14) @interpolate(flat) source_color: vec4<f32>,
#endif
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
#ifdef SOURCE_GPU_PROPS
    let lod_fade = source_fade;
    let color = source_color;
#else
    let lod_fade = bitcast<f32>(receiver.y);
    let color = source.color;
#endif
    if !source_lod_keep(in.position.xy, lod_fade) { discard; }
    let player = source.flags.z == 3u;
    // Source player_model: discard near the camera of a view with `_Dist`. Flag 12: the ragdoll has `_Dist` 0.
    if player && !source_flag(12u) && length(in.world_position.xyz - mesh_view_bindings::view.world_position) < lighting.near_far.z { discard; }
#ifdef SOURCE_GPU_PROPS
    // Same fields as pbr_input_from_vertex_output, without the mesh uniform flags.
    var pbr = pbr_types::pbr_input_new();
    pbr.is_orthographic = mesh_view_bindings::view.clip_from_view[3].w == 1.0;
    pbr.V = pbr_functions::calculate_view(in.world_position, pbr.is_orthographic);
    pbr.frag_coord = in.position;
    pbr.world_position = in.world_position;
    pbr.world_normal = pbr_functions::prepare_world_normal(in.world_normal, false, is_front);
    pbr.N = normalize(pbr.world_normal);
#else
    var pbr = pbr_input_from_vertex_output(in, is_front, false);
#endif
    var uv = vec2(0.0);
#ifdef VERTEX_UVS_A
    uv = in.uv;
#endif
    var detail_uv = uv;
#ifdef VERTEX_UVS_B
    detail_uv = in.uv_b;
#endif
    var offset = vec2(0.0);
#ifdef VERTEX_TANGENTS
    let tbn = mat3x3(in.world_tangent.xyz, world_bitangent, in.world_normal);
    if source_flag(4u) {
        let height = sample_source(5u, uv, detail_uv, vec2(0.0)).g;
        var tangent_view = normalize(transpose(tbn) * pbr.V);
        tangent_view.z += 0.42;
        offset = (height * source.scales.w - source.scales.w * 0.5) * tangent_view.xy / tangent_view.z;
    }
#endif
    let main = sample_source(0u, uv, detail_uv, offset);
    var albedo = main.rgb * color.rgb;
    var alpha = main.a * color.a;
#ifdef VERTEX_COLORS
    // Target-generated terrain keeps its linear vertex colors. Authored source materials do not use this bit.
    if source_flag(11u) { albedo *= in.color.rgb; alpha *= in.color.a; }
#endif
    if source_flag(5u) { alpha = color.a; }
    if source_flag(9u) && alpha < source.factors.w { discard; }
    if player && source.factors.w >= alpha { discard; }
    let mask = sample_source(7u, uv, detail_uv, offset).a;
    if source_flag(1u) {
        albedo *= mix(vec3(1.0), sample_source(6u, uv, detail_uv, offset).rgb * 4.59479380, mask);
    }
#ifdef VERTEX_TANGENTS
    if source_flag(0u) {
        var tangent_normal = source_normal(sample_source(1u, uv, detail_uv, offset), source.scales.x);
        if source_flag(1u) {
            let detail_normal = source_normal(sample_source(8u, uv, detail_uv, offset), source.scales.y);
            let blended = normalize(vec3(tangent_normal.xy + detail_normal.xy, tangent_normal.z * detail_normal.z));
            tangent_normal = mix(tangent_normal, blended, mask);
        }
        pbr.N = normalize(tbn * tangent_normal);
    }
    if player {
        // The player normal texture holds the decoded R*A and G channels.
        let xy = sample_source(1u, uv, detail_uv, offset).rg * 2.0 - 1.0;
        let q = vec3(xy, sqrt(1.0 - min(dot(xy, xy), 1.0)));
        pbr.N = normalize(tbn * mix(vec3(0.5, 0.5, 1.0), q, source.scales.x));
    }
#endif
    var metallic = source.factors.x;
    var smoothness = source.factors.y;
    if source_flag(2u) {
        let metal = sample_source(2u, uv, detail_uv, offset);
        metallic = metal.r;
        smoothness = metal.a * source.factors.z;
    }
    if source_flag(5u) { smoothness = main.a * source.factors.z; }
    let occlusion = mix(1.0, sample_source(3u, uv, detail_uv, offset).g, source.scales.z);
    var specular = mix(vec3(0.04), albedo, metallic);
    var one_minus_reflectivity = 0.96 * (1.0 - metallic);
    if source.specular.w > 0.5 {
        specular = source.specular.rgb;
        one_minus_reflectivity = 1.0 - max(specular.r, max(specular.g, specular.b));
        alpha = 1.0;
    }
    var diffuse = albedo * one_minus_reflectivity;
    if source_flag(8u) {
        diffuse *= alpha;
        alpha = 1.0 - one_minus_reflectivity + alpha * one_minus_reflectivity;
    }
    // Preserve source Standard OutputForward alpha, including premultiplied specular.
    var output_alpha = alpha;
    if (source.flags.z == 0u || player) && !source_flag(8u) && !source_flag(10u) { output_alpha = 1.0; }
#ifdef SOURCE_UNLIT
    let fog = clamp(source_fog,0.0,1.0);
#else
    let fog = fog_factor(source_fog,lighting.near_far.x,lighting.near_far.y);
#endif
    var out: FragmentOutput;
#ifndef SOURCE_FORWARD_ADD
    // ForwardBase stays before the lamp loop, so its implicit derivatives remain in uniform control flow.
    var light = diffuse * diffuse_sh(pbr.N) * occlusion;
    if !source_flag(7u) {
        light += indirect_specular(specular,one_minus_reflectivity,smoothness,pbr.N,pbr.V,occlusion,source_renderer);
    }
    // Glossy-off unity_IndirectSpecColor is RGB0 in the fixed source profile.
    if source_flag(3u) { light += sample_source(4u, uv, detail_uv, offset).rgb * source.emission.rgb; }
    var base_alpha = output_alpha;
    if source.flags.z == 1u { light = color.rgb; base_alpha = 1.0; }
    out.color = vec4(mix(lighting.fog_color_density.xyz,light,fog),base_alpha);
#ifdef SOURCE_REPLACEMENT
    out.color = half4(out.color);
#endif
#endif
#ifdef SOURCE_LAMPS
    let view_depth = -(mesh_view_bindings::view.view_from_world * in.world_position).z;
    let receive_shadows = (u32(source_renderer.y) & 2u) != 0u;
    let sun_attenuation = direct_attenuation(in.world_position.xyz,view_depth,receive_shadows);
    var direct = source_brdf(diffuse,specular,smoothness,pbr.N,pbr.V,lighting.direction_to_sun.xyz)*lighting.sun_radiance.xyz*sun_attenuation;
    let world_dx = dpdx(in.world_position);
    let world_dy = dpdy(in.world_position);
    // Visit low bits first. Floating-point accumulation keeps source slot order.
    var remaining = source_lights;
    loop {
        var word = 0u;
        if remaining.x == 0u {
            if remaining.y == 0u { break; }
            word = 1u;
        }
        let bit = object_first_light(remaining[word]);
        remaining[word] &= remaining[word] - 1u;
        let i = word * 32u + bit;
        let lamp = lamp_lighting.lights[i];
        var direction = -lamp.direction_kind.xyz;
        var weight = 1.0;
        let local = lamp.cookie_from_world * in.world_position;
        if lamp.direction_kind.w != 0.0 {
            let delta = lamp.position_range.xyz - in.world_position.xyz;
            let d2 = dot(delta, delta);
            let range2 = lamp.position_range.w * lamp.position_range.w;
            if d2 >= range2 { continue; }
            direction = delta * inverseSqrt(max(d2, 0.000001));
            weight = textureSampleLevel(attenuation, attenuation_sampler, vec2(d2 / range2, 0.5), 0.0).r;
            weight *= lamp_shadow(lamp.mask.z, lamp.position_range.xyz, in.world_position.xyz, pbr.N, view_depth, receive_shadows);
        }
        if lamp.cookie_rect.x > 0.0 {
            var cookie_uv = vec2(0.0);
            if lamp.direction_kind.w == 0.0 { cookie_uv = local.xy / lamp.cookie_size + lamp.cookie_rect.yz + 0.5; }
            else {
                if local.z <= 0.0 { continue; }
                cookie_uv = local.xy / local.w + 0.5;
                if any(cookie_uv < vec2(0.0)) || any(cookie_uv > vec2(1.0)) { continue; }
            }
            let uv_dx = cookie_uv_gradient(lamp, local, world_dx);
            let uv_dy = cookie_uv_gradient(lamp, local, world_dy);
            if lamp.cookie_rect.x == 1.0 { weight *= textureSampleGrad(sun_cookie, sun_cookie_sampler, cookie_uv, uv_dx, uv_dy).a; }
            else if lamp.cookie_rect.x == 2.0 { weight *= textureSampleGrad(default_cookie, default_sampler, cookie_uv, uv_dx, uv_dy).a; }
            else {
                let half = lamp.cookie_rect.w > 0.5;
                let array_uv = select(cookie_uv, cookie_uv * 0.5 + 0.25, half);
                let scale = select(1.0, 0.5, half);
                weight *= textureSampleGrad(cookies, cookie_sampler, array_uv, i32(lamp.cookie_rect.x) - 3, uv_dx * scale, uv_dy * scale).a;
            }
        }
        direct += source_brdf(diffuse, specular, smoothness, pbr.N, pbr.V, direction) * lamp.color.rgb * weight;
    }
    let added = vec4(direct * fog, output_alpha);
#ifdef SOURCE_FORWARD_ADD
    out.color = added;
#else
    // Same result as the ForwardAdd draw with One One blending over this base output.
    out.color += added;
#endif
#endif
    return out;
}
