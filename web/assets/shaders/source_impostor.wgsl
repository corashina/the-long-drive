#ifdef SOURCE_CASTER
#import "shaders/source_world_caster.wgsl"::{caster_view}
#endif
#import "shaders/source_lod.wgsl"::source_lod_keep
// Checked Octahedron and ColoredBarrels programs. Source atlas rows retain authored mip order.
#import bevy_pbr::{mesh_functions, mesh_view_bindings::view}
#ifdef SOURCE_GPU_PROPS
#import source_props::{prop_row, prop_world_from_local, prop_local_from_world}
#endif
struct ImpostorUniform {
    lod: vec4<f32>,
    shape: vec4<f32>, // frames, size, parallax, depth size
    surface: vec4<f32>, // texture bias, clip, material shadow bias, material shadow view
    offset: vec4<f32>,
    size_offset: vec4<f32>,
    renderer: vec4<u32>, // hemi, colored, receive shadows
    source_position: vec4<f32>,
    bindings: array<vec4<u32>,4>,
    sampling: array<vec4<f32>,4>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> impostor: ImpostorUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas0: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var sampler0: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var atlas1: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var sampler1: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var atlas2: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var sampler2: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var atlas3: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var sampler3: sampler;
struct ImpostorVertex { @builtin(instance_index) instance:u32, @location(0) position:vec3<f32> }
struct ImpostorOutput {
    @builtin(position) position:vec4<f32>,
    @location(0) frame0:vec4<f32>,
    @location(1) frame1:vec4<f32>,
    @location(2) frame2:vec4<f32>,
    @location(3) frames:vec2<f32>,
    @location(4) view_position:vec3<f32>,
    @location(5) @interpolate(flat) instance:u32,
}
fn source_vector(v:vec3<f32>)->vec3<f32> {return v*vec3(1.,1.,-1.);}
fn oct_sign(v:vec2<f32>)->vec2<f32> {return select(vec2(-1.),vec2(1.),v>=vec2(0.));}
fn oct_encode(direction:vec3<f32>)->vec2<f32> {
    if impostor.renderer.x!=0u {
        let p=direction.xz/(abs(direction.x)+abs(direction.z)+max(direction.y,.001));
        return vec2(p.x+p.y,p.x-p.y)*.5+.5;
    }
    let p=direction.xzy/(abs(direction.x)+abs(direction.y)+abs(direction.z));
    let q=select(p.xy,(1.-abs(p.yx))*oct_sign(p.xy),p.z<=0.);
    return q*.5+.5;
}
fn oct_decode(frame:vec2<f32>)->vec3<f32> {
    let q=frame/(impostor.shape.x-1.)*2.-1.;
    if impostor.renderer.x!=0u {
        let xz=vec2(q.x+q.y,q.x-q.y)*.5;
        return normalize(vec3(xz.x,1.-abs(xz.x)-abs(xz.y),xz.y));
    }
    let y=1.-abs(q.x)-abs(q.y);
    let xz=select(q,(1.-abs(q.yx))*oct_sign(q),y<0.);
    return normalize(vec3(xz.x,y,xz.y));
}
fn atlas_frame(frame:vec2<f32>, camera:vec3<f32>, ray:vec3<f32>)->vec4<f32> {
    let normal=oct_decode(frame);
    let right=normalize(vec3(normal.z-.001,0.,-normal.x));
    let up=cross(right,normal);
    let depth=dot(ray,normal);
    let t=-dot(camera,normal)/depth;
    let hit=camera+ray*t;
    let plane=select(-vec2(dot(hit,right),dot(hit,up)),vec2(0.),t<=0.);
    let uv=(plane/impostor.shape.y+.5+frame)/impostor.shape.x-impostor.size_offset.zw;
    let tangent_ray=normalize(vec3(dot(ray,right),dot(ray,up),depth));
    return vec4(-tangent_ray.xy*impostor.shape.z/impostor.shape.x,uv);
}
@vertex
fn vertex(input:ImpostorVertex)->ImpostorOutput {
#ifdef SOURCE_GPU_PROPS
    let model=prop_world_from_local(prop_row(input.instance));
    let inverse=prop_local_from_world(model);
#else
    let model=mesh_functions::get_world_from_local(input.instance);
    let inverse=mesh_functions::get_local_from_world(input.instance);
#endif
    var camera=view.world_position;
    if view.clip_from_view[3].w==1. {camera=model[3].xyz+view.world_from_view[2].xyz*5000.;}
#ifdef SOURCE_CASTER
    camera=caster_view.world_from_view[3].xyz;
    if caster_view.clip_from_view[3].w==1. {camera=model[3].xyz+caster_view.world_from_view[2].xyz*5000.;}
#endif
    let local_camera=source_vector((inverse*vec4(camera,1.)).xyz)-impostor.offset.xyz;
    let direction=normalize(local_camera);
    let right=normalize(vec3(-direction.z,0.,direction.x));
    let up=cross(right,direction);
    let plane=right*input.position.x+up*input.position.y;
    let world=model*vec4(source_vector(plane+impostor.offset.xyz),1.);
    let frames=oct_encode(direction)*(impostor.shape.x-1.);
    let base=floor(frames);
    let fraction=fract(frames);
    let side=ceil(fraction.x-fraction.y);
    let middle=base+vec2(side,1.-side);
    let ray=plane-local_camera;
    var out:ImpostorOutput;
    out.position=view.clip_from_world*world;
    out.frame0=atlas_frame(base,local_camera,ray);
    out.frame1=atlas_frame(middle,local_camera,ray);
    out.frame2=atlas_frame(base+1.,local_camera,ray);
    out.frames=frames;
    out.view_position=(view.view_from_world*world).xyz;
#ifdef SOURCE_CASTER
    out.position=caster_view.clip_from_world*world;
    out.view_position=(caster_view.view_from_world*world).xyz;
#endif
    out.instance=input.instance;
    return out;
}
fn atlas_sample(slot:u32, coordinates:vec2<f32>, bias:f32)->vec4<f32> {
    let uv=select(coordinates,abs(coordinates),impostor.sampling[slot].yz>vec2(.5));
    let binding=impostor.bindings[slot];
    let mip_bias=bias+impostor.sampling[slot].x;
    switch binding.x {
        case 0u: {return textureSampleBias(atlas0,sampler0,uv,i32(binding.y),mip_bias);}
        case 1u: {return textureSampleBias(atlas1,sampler1,uv,i32(binding.y),mip_bias);}
        case 2u: {return textureSampleBias(atlas2,sampler2,uv,i32(binding.y),mip_bias);}
        default: {return textureSampleBias(atlas3,sampler3,uv,i32(binding.y),mip_bias);}
    }
}
fn atlas_blend(slot:u32,uv0:vec2<f32>,uv1:vec2<f32>,uv2:vec2<f32>,weights:vec3<f32>)->vec4<f32> {
    return atlas_sample(slot,uv0,impostor.surface.x)*weights.x
        +atlas_sample(slot,uv1,impostor.surface.x)*weights.y
        +atlas_sample(slot,uv2,impostor.surface.x)*weights.z;
}
struct ImpostorFragment {
#ifndef PREPASS_PIPELINE
    @location(0) color:vec4<f32>,
#endif
    @builtin(frag_depth) depth:f32,
}

#ifndef PREPASS_PIPELINE
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


#import bevy_pbr::{pbr_types::pbr_input_new}

#endif
@fragment
fn fragment(in:ImpostorOutput)->ImpostorFragment {
#ifdef SOURCE_GPU_PROPS
    // The row holds the fade and the source position of the shared material.
    let prop=prop_row(in.instance);
    let lod_fade=prop.fade;
    let source_position=prop.source_position;
    let model=prop_world_from_local(prop);
#else
    let lod_fade=impostor.lod.x;
    let source_position=impostor.source_position.xyz;
    let model=mesh_functions::get_world_from_local(in.instance);
#endif
    if !source_lod_keep(in.position.xy,lod_fade) {discard;}
    let fraction=fract(in.frames);
    let weights=vec3(min(1.-fraction.x,1.-fraction.y),abs(fraction.x-fraction.y),min(fraction.x,fraction.y));
    let depths=vec3(atlas_sample(0u,in.frame0.zw,-1.).a,atlas_sample(0u,in.frame1.zw,-1.).a,atlas_sample(0u,in.frame2.zw,-1.).a);
    let uv0=in.frame0.zw+(.5-depths.x)*in.frame0.xy;
    let uv1=in.frame1.zw+(.5-depths.y)*in.frame1.xy;
    let uv2=in.frame2.zw+(.5-depths.z)*in.frame2.xy;
    let albedo_sample=atlas_blend(1u,uv0,uv1,uv2,weights);
    if albedo_sample.a<impostor.surface.y {discard;}
    // Source reads the Z row, not the length of the third column.
    let z_scale=length(vec3(model[0].z,model[1].z,model[2].z));
    let displacement=(dot(depths,weights)-.5)*impostor.shape.w*z_scale;
    var position=in.view_position;
    position.z+=displacement;
#ifdef PREPASS_PIPELINE
    if view.clip_from_view[3].w==1. {position.z=in.view_position.z+displacement*impostor.surface.w-impostor.surface.z;}
#endif
#ifdef SOURCE_CASTER
    if caster_view.clip_from_view[3].w==1. {position.z=in.view_position.z+displacement*impostor.surface.w-impostor.surface.z;}
    let clip=caster_view.clip_from_view*vec4(position,1.);
#else
    let clip=view.clip_from_view*vec4(position,1.);
#endif
    var out:ImpostorFragment;
    out.depth=clip.z/clip.w;
#ifndef PREPASS_PIPELINE
    let encoded=atlas_blend(0u,uv0,uv1,uv2,weights).rgb*2.-1.;
    let normal=normalize((model*vec4(source_vector(encoded),0.)).xyz);
    let world=view.world_from_view*vec4(position,1.);
    var albedo=albedo_sample.rgb;
    var emission=vec3(0.);
    var occlusion=1.;
    var specular=vec3(0.);
    var smoothness=0.;
    let properties=atlas_blend(2u,uv0,uv1,uv2,weights);
    if impostor.renderer.y!=0u {
        specular=vec3(properties.r);smoothness=properties.g;occlusion=properties.b;
        let seed=abs(sin(source_position.x+source_position.z));
        let hue=clamp(abs(fract(seed+vec3(1.,.666666687,.333333343))*6.-3.)-1.,vec3(0.),vec3(1.));
        albedo=albedo*(1.-properties.a)+hue*properties.a*properties.a;
    } else {
        emission=properties.rgb;occlusion=properties.a;
        let spec=atlas_blend(3u,uv0,uv1,uv2,weights);specular=spec.rgb;smoothness=spec.a;
    }
    var pbr=pbr_input_new();
    pbr.frag_coord=in.position;pbr.world_position=world;pbr.world_normal=normal;pbr.N=normal;
    pbr.V=normalize(view.world_position-world.xyz);
    let diffuse=albedo*(1.-max(specular.r,max(specular.g,specular.b)));
    var light=diffuse*diffuse_sh(normal)*occlusion+emission;
    let depth=max(-position.z,0.);
    light+=source_brdf(diffuse,specular,smoothness,pbr.N,pbr.V,lighting.direction_to_sun.xyz)*lighting.sun_radiance.rgb*direct_attenuation(pbr.world_position.xyz,depth,impostor.renderer.z != 0u);
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
            weight *= lamp_shadow(lamp.mask.z, lamp.position_range.xyz, pbr.world_position.xyz, pbr.N, depth, impostor.renderer.z != 0u);
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
    let clip_z=(view.clip_from_world*pbr.world_position).z;
    let fog=fog_factor(clip_z,lighting.near_far.x,lighting.near_far.y);
    light=mix(lighting.fog_color_density.rgb,light,fog);
    pbr.material.flags = 0u;
    out.color = vec4(light, 1.0);

#endif
    return out;
}
