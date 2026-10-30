// TMP Surface programs 4 and 20, installed build 11140890.
#import bevy_pbr::{mesh_functions, mesh_view_bindings::view}
#import "shaders/source_lighting.wgsl"::{lamp_lighting, receiver, cookie_uv_gradient, attenuation, attenuation_sampler, default_cookie, default_sampler, cookies, cookie_sampler}
#import source_view_lighting::{lighting, diffuse_sh, direct_attenuation, lamp_shadow, fog_factor, sun_cookie, sun_cookie_sampler}
struct PlateUniform { face:vec4<f32>, outline:vec4<f32>, sdf:vec4<f32>, scale:vec4<f32>, bevel:vec4<f32>, bump:vec4<f32>, specular:vec4<f32> }
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> plate:PlateUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var face_tex:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var face_sampler:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var bump_tex:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var bump_sampler:sampler;
struct Vertex { @builtin(instance_index) instance:u32, @location(0) position:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) packed:vec2<f32>, @location(4) color:vec4<f32> }
struct Out { @builtin(position) position:vec4<f32>, @location(0) world:vec4<f32>, @location(1) normal:vec3<f32>, @location(2) tangent:vec3<f32>, @location(3) uv:vec2<f32>, @location(4) face_uv:vec2<f32>, @location(5) sdf:vec2<f32>, @location(6) color:vec4<f32> }
@vertex fn vertex(v:Vertex)->Out {
    var o:Out;
    let model=mesh_functions::get_world_from_local(v.instance);
    o.world=mesh_functions::mesh_position_local_to_world(model,vec4(v.position,1.));
    o.position=lighting.clip_from_world*o.world;
    o.normal=normalize(mesh_functions::mesh_normal_local_to_world(v.normal,v.instance));
    o.tangent=normalize((model*vec4(-1.,0.,0.,0.)).xyz);
    o.uv=v.uv;
    let x=floor(v.packed.x/4096.);
    o.face_uv=vec2(x,v.packed.x-x*4096.)/512.;
    let weight=(select(0.,plate.scale.z,v.packed.y<=0.)/4.+plate.sdf.x)*plate.sdf.w*.5;
    let pixel=vec2(o.position.w)/(view.viewport.zw*vec2(abs(view.clip_from_view[0].x),abs(view.clip_from_view[1].y)));
    var scale=inverseSqrt(dot(pixel,pixel))*abs(v.packed.y)*length(model[1].xyz)*plate.scale.x*(1.+plate.scale.w);
    let eye=normalize(view.world_position-o.world.xyz);
    scale=mix(scale*(1.-plate.scale.y),scale,abs(dot(o.normal,eye)));
    o.sdf=vec2(weight,scale);o.color=v.color;return o;
}
@fragment fn fragment(i:Out,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    let texel=1./vec2<f32>(textureDimensions(atlas));
    let neighbors=vec4(textureSample(atlas,atlas_sampler,i.uv-vec2(texel.x,0.)).a,textureSample(atlas,atlas_sampler,i.uv+vec2(texel.x,0.)).a,textureSample(atlas,atlas_sampler,i.uv-vec2(0.,texel.y)).a,textureSample(atlas,atlas_sampler,i.uv+vec2(0.,texel.y)).a);
    let a=textureSample(atlas,atlas_sampler,i.uv).a;
    let sd=(.5-a-i.sdf.x)*i.sdf.y+.5;
    let outline=plate.sdf.y*plate.sdf.w*i.sdf.y;
    let softness=plate.sdf.z*plate.sdf.w*i.sdf.y;
    let tex=textureSample(face_tex,face_sampler,i.face_uv);
    var face=plate.face*i.color*tex;
    var edge=plate.outline*vec4(1.,1.,1.,i.color.a)*tex;
    face=vec4(face.rgb*face.a,face.a);edge=vec4(edge.rgb*edge.a,edge.a);
    let blend=clamp(sd+outline*.5,0.,1.);
    var color=mix(face,edge,blend*sqrt(min(1.,outline)));
    color*=1.-clamp((sd-outline*.5+softness*.5)/(1.+softness),0.,1.);
    let diffuse=color.rgb/max(color.a,.0001);
    let width=max(.01,plate.sdf.y+plate.bevel.z);
    var height=clamp((neighbors+vec4(i.sdf.x+plate.bevel.y-.5))/width+vec4(.5),vec4(0.),vec4(1.));
    height=mix(height,sin(height*1.570796),plate.bevel.w);
    height*=plate.bevel.x*width*plate.scale.x*-2.;
    let av=normalize(vec2(-1.,height.w-height.z));
    let bv=normalize(vec2(height.y-height.x,1.));
    let bevel_normal=vec3(-av.x*bv.x,-av.y*bv.y,av.x*bv.y);
    let b=textureSample(bump_tex,bump_sampler,i.face_uv);
    let xy=vec2(b.r*b.a,b.g)*2.-1.;
    let bump=vec3(xy,sqrt(1.-min(dot(xy,xy),1.)))*mix(plate.bump.x,plate.bump.y,blend);
    let tangent_normal=-normalize(bevel_normal-mix(vec3(0.,0.,1.),bump,color.a));
    let eye=normalize(view.world_position-i.world.xyz);
    let base_normal=select(-i.normal,i.normal,dot(i.normal,eye)>=0.);
    let bitangent=-normalize(cross(base_normal,i.tangent));
    let normal=normalize(i.tangent*tangent_normal.x+bitangent*tangent_normal.y+base_normal*tangent_normal.z);
    let shininess=mix(plate.bump.z,plate.bump.w,blend)*128.;
    var light=diffuse*diffuse_sh(normal);
    let depth=max(-(view.view_from_world*i.world).z,0.);
    let sun_direction=lighting.direction_to_sun.xyz;
    let sun_specular=pow(max(dot(normal,normalize(sun_direction+eye)),0.),shininess);
    light+=(diffuse*max(dot(normal,sun_direction),0.)+plate.specular.rgb*sun_specular)*lighting.sun_radiance.rgb*direct_attenuation(i.world.xyz,depth,true);
    let world_dx=dpdx(i.world);let world_dy=dpdy(i.world);
    for(var n=0u;n<lamp_lighting.counts.x;n+=1u) {
        let lamp=lamp_lighting.lights[n];
        if lamp.direction_kind.w==0. {continue;}
        if (receiver.x&lamp.mask.x)==0u {continue;}
        var direction=-lamp.direction_kind.xyz;var weight=1.;
        let local=lamp.cookie_from_world*i.world;
        if lamp.direction_kind.w!=0. {
            let delta=lamp.position_range.xyz-i.world.xyz;let d2=dot(delta,delta);let range2=lamp.position_range.w*lamp.position_range.w;
            if d2>=range2 {continue;}
            direction=delta*inverseSqrt(max(d2,.000001));weight=textureSampleLevel(attenuation,attenuation_sampler,vec2(d2/range2,.5),0.).r;
            weight*=lamp_shadow(lamp.mask.z,lamp.position_range.xyz,i.world.xyz,normal,depth,true);
        }
        if lamp.cookie_rect.x>0. {
            var uv=vec2(0.);
            if lamp.direction_kind.w==0. {uv=local.xy/lamp.cookie_size+lamp.cookie_rect.yz+.5;}
            else {if local.z<=0. {continue;} uv=local.xy/local.w+.5;if any(uv<vec2(0.))||any(uv>vec2(1.)){continue;}}
            let uv_dx=cookie_uv_gradient(lamp,local,world_dx);let uv_dy=cookie_uv_gradient(lamp,local,world_dy);
            if lamp.cookie_rect.x==1. {weight*=textureSampleGrad(sun_cookie,sun_cookie_sampler,uv,uv_dx,uv_dy).a;}
            else if lamp.cookie_rect.x==2. {weight*=textureSampleGrad(default_cookie,default_sampler,uv,uv_dx,uv_dy).a;}
            else {
                let half=lamp.cookie_rect.w>0.5;let scale=select(1.,0.5,half);
                let array_uv=select(uv,uv*0.5+0.25,half);
                weight*=textureSampleGrad(cookies,cookie_sampler,array_uv,i32(lamp.cookie_rect.x)-3,uv_dx*scale,uv_dy*scale).a;
            }
        }
        let specular=pow(max(dot(normal,normalize(direction+eye)),0.),shininess);
        light+=(diffuse*max(dot(normal,direction),0.)+plate.specular.rgb*specular)*lamp.color.rgb*weight;
    }
    let clip=lighting.clip_from_world*i.world;
    light=mix(lighting.fog_color_density.rgb,light,fog_factor(clip.z,lighting.near_far.x,lighting.near_far.y));
    return vec4(light,color.a);
}
