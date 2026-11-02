#ifdef SOURCE_CASTER
#import "shaders/source_world_caster.wgsl"::{caster_view,world_caster_clip}
#endif
#import bevy_pbr::{mesh_functions, mesh_view_bindings::view, view_transformations::position_world_to_clip}

struct TerrainUniform {
    displacement: vec4<f32>,
    renderer: vec4<u32>,
    texcoord: vec4<f32>,
    scale_slope: vec4<f32>,
    layers: array<vec4<f32>,5>,
    detail: vec4<f32>,
    colors: array<vec4<f32>,10>,
    bindings: array<vec4<u32>,12>,
    sampling: array<vec4<f32>,12>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> terrain: TerrainUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var bank0: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var sampler0: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var bank1: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var sampler1: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var bank2: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var sampler2: sampler;
#if TERRAIN_ARRAYS > 3
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var bank3: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var sampler3: sampler;
#endif
#if TERRAIN_ARRAYS > 4
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var bank4: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var sampler4: sampler;
#endif
struct TerrainVertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) tangent: vec4<f32>,
}
struct TerrainOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) tangent: vec3<f32>,
    @location(5) bitangent: vec3<f32>,
}
@vertex
fn vertex(v: TerrainVertex)->TerrainOutput {
    let world=mesh_functions::get_world_from_local(v.instance);
    let position=mesh_functions::mesh_position_local_to_world(world,vec4(v.position,1.));
    // Compiled source offsets64/68 are FarDist/NearDist, in this order.
    let distance=length(position.xyz-view.world_position);
    let quotient=(distance-terrain.displacement.x)/(terrain.displacement.y-terrain.displacement.x);
    let factor=select(0.,clamp(quotient,0.,1.),quotient==quotient);
    var local=v.position;
    local.y+=factor*terrain.displacement.z;
    var out:TerrainOutput;
    out.world_position=mesh_functions::mesh_position_local_to_world(world,vec4(local,1.)).xyz;
    out.position=position_world_to_clip(out.world_position);
#ifdef SOURCE_CASTER
    out.position=world_caster_clip(out.world_position,normalize(mesh_functions::mesh_normal_local_to_world(v.normal,v.instance)));
#endif
    out.normal=normalize(mesh_functions::mesh_normal_local_to_world(v.normal,v.instance));
    let tangent=mesh_functions::mesh_tangent_local_to_world(world,v.tangent,v.instance);
    out.tangent=normalize(tangent.xyz);
    out.bitangent=cross(out.normal,out.tangent)*tangent.w;
    out.uv=v.uv*terrain.texcoord.xy+terrain.texcoord.zw;
    out.color=v.color;
    return out;
}
#ifndef SOURCE_CASTER
fn sample_terrain(slot:u32,uv_in:vec2<f32>)->vec4<f32> {
    let binding=terrain.bindings[slot];
    let sampling=terrain.sampling[slot];
    var uv=uv_in;
    if sampling.y!=0. {uv.x=abs(uv.x);}
    if sampling.z!=0. {uv.y=abs(uv.y);}
    switch binding.x {
        case 0u:{return textureSampleBias(bank0,sampler0,uv,i32(binding.y),sampling.x);}
        case 1u:{return textureSampleBias(bank1,sampler1,uv,i32(binding.y),sampling.x);}
        case 2u:{return textureSampleBias(bank2,sampler2,uv,i32(binding.y),sampling.x);}
#if TERRAIN_ARRAYS > 3
        case 3u:{return textureSampleBias(bank3,sampler3,uv,i32(binding.y),sampling.x);}
#endif
#if TERRAIN_ARRAYS > 4
        default:{return textureSampleBias(bank4,sampler4,uv,i32(binding.y),sampling.x);}
#else
        default:{return textureSampleBias(bank0,sampler0,uv,i32(binding.y),sampling.x);}
#endif
    }
}
fn terrain_normal(slot:u32,uv:vec2<f32>,strength:f32)->vec3<f32> {
    let encoded=sample_terrain(slot,uv);
    let xy=(vec2(encoded.x*encoded.w,encoded.y)*2.-1.)*strength;
    return vec3(xy,sqrt(1.-min(dot(xy,xy),1.)));
}
fn blend_terrain_normals(a:vec3<f32>,b:vec3<f32>)->vec3<f32> {
    return normalize(vec3(a.xy+b.xy,a.z*b.z));
}
struct TerrainSurface {albedo:vec3<f32>,normal:vec3<f32>,smoothness:f32}
fn terrain_surface(v:TerrainOutput)->TerrainSurface {
    let scaling=terrain.scale_slope.x*terrain.scale_slope.y;
    let cliff=clamp((v.normal.y-terrain.scale_slope.z)/(terrain.scale_slope.w-terrain.scale_slope.z),0.,1.);
    let ground=1.-cliff;
    let sand=(1.-v.color.b)*v.color.r*ground;
    let grass=(1.-v.color.b)*(1.-v.color.r)*v.color.g*ground;
    let dirt=(1.-v.color.b)*(1.-v.color.r)*(1.-v.color.g)*ground;
    let snow=v.color.b*ground;
    let normal_weight=1.-terrain.displacement.w;
    let n_sand=terrain_normal(0u,v.uv*(scaling*terrain.layers[0].x),sand*terrain.layers[0].y*normal_weight);
    let n_grass=terrain_normal(1u,v.uv*(scaling*terrain.layers[1].x),grass*terrain.layers[1].y*normal_weight);
    let n_dirt=terrain_normal(2u,v.uv*(scaling*terrain.layers[2].x),dirt*terrain.layers[2].y*normal_weight);
    let n_snow=terrain_normal(3u,v.uv*(scaling*terrain.layers[3].x),snow*terrain.layers[3].y*normal_weight);
    let n_cliff=terrain_normal(4u,v.uv*(scaling*terrain.layers[4].x),cliff*terrain.layers[4].y*normal_weight);
    let detail1=terrain_normal(5u,v.uv*terrain.detail.x,terrain.detail.y);
    let detail2=terrain_normal(6u,v.uv*terrain.detail.z,terrain.detail.w);
    let biomes=blend_terrain_normals(blend_terrain_normals(n_sand,n_grass),blend_terrain_normals(n_dirt,n_snow));
    let details=blend_terrain_normals(blend_terrain_normals(n_cliff,detail1),detail2);
    let normal=blend_terrain_normals(biomes,details);
    let a=v.color.a;
    var albedo=mix(terrain.colors[0].rgb,terrain.colors[1].rgb,a)*sample_terrain(7u,v.uv*(scaling*terrain.layers[2].x)).rgb;
    albedo=mix(albedo,mix(terrain.colors[2].rgb,terrain.colors[3].rgb,a)*sample_terrain(8u,v.uv*(scaling*terrain.layers[1].x)).rgb,clamp(v.color.g,0.,1.));
    albedo=mix(albedo,mix(terrain.colors[4].rgb,terrain.colors[5].rgb,a)*sample_terrain(9u,v.uv*(scaling*terrain.layers[0].x)).rgb,clamp(v.color.r,0.,1.));
    albedo=mix(albedo,mix(terrain.colors[6].rgb,terrain.colors[7].rgb,a)*sample_terrain(10u,v.uv*(scaling*terrain.layers[3].x)).rgb,clamp(v.color.b,0.,1.));
    albedo=mix(albedo,mix(terrain.colors[8].rgb,terrain.colors[9].rgb,a)*sample_terrain(11u,v.uv*(scaling*terrain.layers[4].x)).rgb,cliff);
    albedo=mix(albedo,v.color.rgb,terrain.displacement.w);
    // These source smoothness names differ from their compiled biome weights.
    let smoothness=clamp(sand*terrain.layers[3].z+grass*terrain.layers[2].z+dirt*terrain.layers[1].z+snow*terrain.layers[0].z+cliff*terrain.layers[4].z,0.,1.);
    return TerrainSurface(albedo,normalize(mat3x3(v.tangent,v.bitangent,v.normal)*normal),smoothness);
}

#import "shaders/source_lighting.wgsl"::{lamp_lighting, receiver, cookie_uv_gradient, cookies, cookie_sampler, attenuation, default_cookie, default_sampler, attenuation_sampler}
#import source_view_lighting::{lighting, diffuse_sh, direct_attenuation, lamp_shadow, fog_factor, sun_cookie, sun_cookie_sampler}


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
    let fresnel = specular + (vec3(1.0) - specular) * pow5(1.0 - lh);
    return diffuse * disney * nl + specular_term * fresnel;
}


#import bevy_pbr::{pbr_types::pbr_input_new, forward_io::FragmentOutput}
@fragment
fn fragment(in:TerrainOutput)->FragmentOutput {
    let surface=terrain_surface(in);
    var pbr=pbr_input_new();
    pbr.frag_coord=in.position;
    pbr.world_position=vec4(in.world_position,1.);
    pbr.world_normal=normalize(in.normal);
    pbr.N=surface.normal;
    pbr.V=normalize(view.world_position-in.world_position);
    let diffuse=surface.albedo*0.96;
    let specular=vec3(0.04);
    let smoothness=surface.smoothness;
    var light=diffuse*diffuse_sh(pbr.N);
    let depth=max(-(view.view_from_world*pbr.world_position).z,0.);
    light+=source_brdf(diffuse,specular,smoothness,pbr.N,pbr.V,lighting.direction_to_sun.xyz)*lighting.sun_radiance.rgb*direct_attenuation(pbr.world_position.xyz,depth,terrain.renderer.x != 0u);
    let world_dx = dpdx(pbr.world_position);
    let world_dy = dpdy(pbr.world_position);
    for (var i = 0u; i < lamp_lighting.counts.x; i += 1u) {
        let lamp = lamp_lighting.lights[i];
        if (receiver.x & lamp.mask.x) == 0u { continue; }
        if max(lamp.color.r, max(lamp.color.g, lamp.color.b)) <= 0.0 { continue; }
        var direction = -lamp.direction_kind.xyz;
        var weight = 1.0;
        let local = lamp.cookie_from_world * pbr.world_position;
        if lamp.direction_kind.w != 0.0 {
            let delta = lamp.position_range.xyz - pbr.world_position.xyz;
            let d2 = dot(delta, delta);
            let range2 = lamp.position_range.w * lamp.position_range.w;
            if d2 >= range2 { continue; }
            direction = delta * inverseSqrt(max(d2, 0.000001));
            weight = textureSampleLevel(attenuation, attenuation_sampler, vec2(d2 / range2, 0.5), 0.0).r;
            weight *= lamp_shadow(lamp.mask.z, lamp.position_range.xyz, pbr.world_position.xyz, pbr.N, depth, terrain.renderer.x != 0u);
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
        light += source_brdf(diffuse, specular, smoothness, pbr.N, pbr.V, direction) * lamp.color.rgb * weight;
    }
    var out: FragmentOutput;
    let clip_z=(view.clip_from_world*pbr.world_position).z;
    let fog=fog_factor(clip_z,lighting.near_far.x,lighting.near_far.y);
    light=mix(lighting.fog_color_density.rgb,light,fog);
    pbr.material.flags = 0u;
    out.color = vec4(light, 1.0);
    return out;
}

#else
@fragment fn fragment(in:TerrainOutput) {}
#endif
