#ifdef SOURCE_CASTER
#import "shaders/source_world_caster.wgsl"::{caster_view}
#endif
#import "shaders/source_lod.wgsl"::source_lod_keep
#import bevy_pbr::{mesh_view_bindings::view, view_transformations::position_world_to_clip}
struct GrassUniform {
    lod: vec4<f32>,
    camera: vec4<f32>,
    shape: vec4<f32>,
    fade_lod: vec4<f32>,
    wind: vec4<f32>,
    options: vec4<f32>,
    color: vec4<f32>,
    secondary: vec4<f32>,
    bottom: vec4<f32>,
    burn: vec4<f32>,
    smoothing: vec4<u32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> grass: GrassUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var displacement: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var displacement_sampler: sampler;
struct GrassFrameUniform {origin_time:vec4<f32>,area:vec4<f32>}
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> grass_frame:GrassFrameUniform;
struct GrassVertex {
    @location(0) anchor: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) blade: vec4<f32>,
}
struct GrassOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) alive: f32,
}
fn grass_hash(p:vec2<f32>)->f32 { return fract(sin(dot(p,vec2(12.9898,78.233002)))*43758.546875); }
fn grass_smooth(t:f32)->f32 {let v=clamp(t,0.,1.);return v*v*(3.-2.*v);}
fn source_wind(p:vec2<f32>)->vec2<f32> {
    let s=sin(grass.options.x); let c=cos(grass.options.x);
    let r=vec2(p.x*c-p.y*s,p.y*c+p.x*s);
    let t=grass_frame.origin_time.w;
    let speed=t*grass.wind.y; let turbulence=t*grass.wind.w;
    let phase=r.x+3.*speed+5.*cos(r.y*.01+speed*.2)+4.*sin(r.y*.05-speed*.15)
        +4.*sin(r.y*.2+speed*.2)+2.*cos(r.y*.6-speed*.4);
    let base=sin(phase*.7)*grass.wind.x*cos(phase*.15);
    let a=sin((r.x+452.)*100.+3.*turbulence+r.y+987.)*cos((r.x+452.)*10.+2.*turbulence+(r.y+987.)*.5);
    let b=sin(r.x*100.+3.*turbulence+r.y)*cos(r.x*10.+2.*turbulence+r.y*.5);
    let wind=vec2(a,b)*grass.wind.z+base;
    return vec2(wind.y*c-wind.x*s,wind.x*c+wind.y*s);
}
@vertex
fn vertex(input:GrassVertex)->GrassOutput {
    let anchor=input.anchor;
    let distance=length(anchor-grass.camera.xyz);
    let encoded=textureSampleLevel(displacement,displacement_sampler,(anchor.xz-grass_frame.area.xy)/grass_frame.area.zw,0.);
    let alpha=clamp(encoded.a,0.,1.);
    var facing=grass.camera.xyz;
#ifdef PREPASS_PIPELINE
    facing=vec3(view.world_position.xy+grass_frame.origin_time.xy,-view.world_position.z-grass_frame.origin_time.z);
#endif

#ifdef SOURCE_CASTER
    facing=vec3(caster_view.world_from_view[3].xy+grass_frame.origin_time.xy,-caster_view.world_from_view[3].z-grass_frame.origin_time.z);
#endif
    let toward=normalize(facing-anchor);
    let top=1.+.8*pow(max(toward.y,0.),20.);
    let disorder=vec2(grass_hash(anchor.xz+1000.),grass_hash(anchor.xz-1000.))*2.-1.;
    let bend=source_wind(anchor.xz)+disorder*grass.options.y*top;
    // Selected _ColorMap slots are null with the checked shader default white.
    let fade=grass_smooth((distance-grass.fade_lod.y)/(grass.fade_lod.x-grass.fade_lod.y));
    var height=fade*mix(grass.shape.y,grass.shape.z,grass_hash(anchor.xz))*alpha;
    var width=grass.shape.x*top;
    if grass.smoothing.x!=0u {height*=input.blade.z;}
    if grass.smoothing.y!=0u {width*=input.blade.z;}
    let direction=normalize(encoded.rgb);
    let horizontal=direction.xy+bend*direction.z*direction.z*direction.z;
    let bend_length=length(direction.xy+bend);
    let t=input.blade.x; let square=t*t;
    let softness=height*grass.shape.w;
    let center=anchor+vec3(horizontal.x*softness*square,
        max(direction.z,.01)*(t*height-bend_length*square*.5*softness),horizontal.y*softness*square);
    let side=normalize(vec3(toward.z,0.,-toward.x))*width*.5;
    let source_position=center+side*input.blade.y*(1.-square);
    let world=vec3(source_position.xy,-source_position.z)-grass_frame.origin_time.xyz;
    let color=mix(grass.color,grass.secondary,grass_hash(anchor.xz+vec2(1000.,-1000.)))
        *mix(grass.burn,vec4(1.),alpha*alpha)*mix(grass.bottom,vec4(1.),t);
    var out:GrassOutput;
    out.world=world;out.position=position_world_to_clip(world);out.color=color;

#ifdef SOURCE_CASTER
    out.position=caster_view.clip_from_world*vec4(world,1.);
#endif
    out.alive=select(1.,0.,distance>grass.fade_lod.y || alpha<grass.options.z);
    return out;
}
#ifdef PREPASS_PIPELINE
@fragment fn fragment(input:GrassOutput) {if input.alive<.5 || !source_lod_keep(input.position.xy,grass.lod.x) {discard;}}
#else
#import "shaders/source_lighting.wgsl"::{lamp_lighting, receiver, cookie_uv_gradient, cookies, cookie_sampler, attenuation, default_cookie, default_sampler, attenuation_sampler}
#import source_view_lighting::{lighting, diffuse_sh, direct_attenuation, fog_factor, sun_cookie, sun_cookie_sampler}
#import bevy_pbr::{forward_io::FragmentOutput,pbr_types::pbr_input_new}
@fragment
fn fragment(input:GrassOutput)->FragmentOutput {
    if input.alive<.5 || !source_lod_keep(input.position.xy,grass.lod.x) {discard;}
    let sun_uv=(lighting.cookie_from_world*vec4(input.world,1.)).xy;
    var color=lighting.sun_radiance.rgb*input.color.rgb*textureSample(sun_cookie,sun_cookie_sampler,sun_uv).a;
    let world_dx=dpdx(vec4(input.world,1.));let world_dy=dpdy(vec4(input.world,1.));
    for(var i=0u;i<lamp_lighting.counts.x;i+=1u){
        let lamp=lamp_lighting.lights[i];if (receiver.x & lamp.mask.x)==0u {continue;}
        var weight=1.;let local=lamp.cookie_from_world*vec4(input.world,1.);
        if lamp.direction_kind.w!=0. {
            let delta=lamp.position_range.xyz-input.world;
            let ratio=dot(delta,delta)/(lamp.position_range.w*lamp.position_range.w);
            if ratio>=1. {continue;}
            weight=textureSampleLevel(attenuation,attenuation_sampler,vec2(ratio,.5),0.).r;
        }
        if lamp.cookie_rect.x>0. {
            var uv=local.xy/lamp.cookie_size+lamp.cookie_rect.yz+.5;
            if lamp.direction_kind.w!=0. {if local.z<=0. {continue;} uv=local.xy/local.w+.5;if any(uv<vec2(0.))||any(uv>vec2(1.)){continue;}}
            let uv_dx=cookie_uv_gradient(lamp,local,world_dx);let uv_dy=cookie_uv_gradient(lamp,local,world_dy);
            if lamp.cookie_rect.x==1. {weight*=textureSampleGrad(sun_cookie,sun_cookie_sampler,uv,uv_dx,uv_dy).a;}
            else if lamp.cookie_rect.x==2. {weight*=textureSampleGrad(default_cookie,default_sampler,uv,uv_dx,uv_dy).a;}
            else {let half=lamp.cookie_rect.w>.5;let scale=select(1.,.5,half);weight*=textureSampleGrad(cookies,cookie_sampler,select(uv,uv*.5+.25,half),i32(lamp.cookie_rect.x)-3,uv_dx*scale,uv_dy*scale).a;}
        }
        color+=lamp.color.rgb*input.color.rgb*weight;
    }
    var pbr=pbr_input_new();pbr.world_position=vec4(input.world,1.);
    var out:FragmentOutput;out.color=vec4(color,1.);return out;
}
#endif