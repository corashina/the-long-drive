// Selected Unity particle blending, soft particles, distortion, BumpDistort glass and camera-facing billboards.
#import bevy_pbr::mesh_view_bindings::view
struct Particles {
    positions:array<vec4<f32>,128>,colors:array<vec4<f32>,128>,angles:array<vec4<f32>,128>,
    tint:vec4<f32>,st:vec4<f32>,fog:vec4<f32>,settings:vec4<f32>,effect:vec4<f32>,
    // FX/Glass/Stained BumpDistort: x _BumpAmt, y 1. bump_st: _BumpMap_ST.
    glass:vec4<f32>,bump_st:vec4<f32>,
}
struct ParticleView {clip_from_world:mat4x4<f32>,near_far:vec4<f32>,grab_texel_size:vec4<f32>}
@group(1) @binding(0) var<uniform> particle_view:ParticleView;
@group(1) @binding(1) var camera_depth:texture_depth_2d;
@group(1) @binding(2) var grab_texture:texture_2d<f32>;
@group(1) @binding(3) var grab_sampler:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> particles:Particles;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var image:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var image_sampler:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var bump:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var bump_sampler:sampler;
struct Vertex {@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>}
struct Varyings {
    @builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>,@location(2) depth:f32,
    @location(3) eye:f32,@location(4) grab:vec4<f32>,@location(5) bump_uv:vec2<f32>,
}
@vertex fn vertex(v:Vertex)->Varyings {
    let i=u32(v.position.z);let p=particles.positions[i];let angle=particles.angles[i];// xy: cos, sin. zw: sheet tile offset.
    var center=view.view_from_world*vec4(p.xyz,1.);
    let diameter=clamp(p.w,particles.settings.y*abs(center.z)*2./abs(view.clip_from_view[1][1]),particles.settings.z*abs(center.z)*2./abs(view.clip_from_view[1][1]));
    let corner=vec2(v.position.x*angle.x-v.position.y*angle.y,v.position.x*angle.y+v.position.y*angle.x)*diameter;
    center.x+=corner.x;center.y+=corner.y;
    var o:Varyings;o.position=view.clip_from_view*center;o.uv=v.uv*particles.st.xy+particles.st.zw+angle.zw;
    o.color=particles.colors[i]*particles.tint*particles.settings.x;let a=view.clip_from_view[2][2];let b=view.clip_from_view[3][2];let near=b/(a+1.);let far=b/a;
    o.depth=max((abs(center.z)-near)*far/(far-near),0.);
    // Source COMPUTE_EYEDEPTH and D3D ComputeGrabScreenPos.
    o.eye=-center.z;
    // z: the source finite reverse-Z clip depth for the BumpDistort grab weight.
    let source_z=(particle_view.clip_from_world*(view.world_from_view*center)).z;
    o.grab=vec4((o.position.x+o.position.w)*.5,(-o.position.y+o.position.w)*.5,source_z,o.position.w);
    o.bump_uv=v.uv*particles.bump_st.xy+particles.bump_st.zw;
    if i>=u32(particles.settings.w) {o.position=vec4(2.,2.,2.,1.);}
    return o;
}
@fragment fn fragment(v:Varyings)->@location(0) vec4<f32> {
    if particles.glass.y>0. {
        // Installed FX/Glass/Stained BumpDistort as in source_glass.wgsl. UnpackNormal over DXT5nm AG.
        // Blend Off: the grab replaces the destination. No vertex color and no fog.
        let packed=textureSample(bump,bump_sampler,v.bump_uv);
        let normal=vec2(packed.r*packed.a,packed.g)*2.-1.;
        let offset=normal*particles.glass.x*particle_view.grab_texel_size.xy;
        let weight=max(0.,(1.-v.grab.z/particle_view.near_far.x)*particle_view.near_far.y);
        let grabbed=textureSampleLevel(grab_texture,grab_sampler,(v.grab.xy+offset*weight)/v.grab.w,0.);
        return vec4((grabbed*textureSample(image,image_sampler,v.uv)*particles.tint).rgb,1.);
    }
    var c=textureSample(image,image_sampler,v.uv)*v.color;
    if particles.effect.x>0. {
        // Source LinearEyeDepth over the single-sample camera depth.
        let a=view.clip_from_view[2][2];let b=view.clip_from_view[3][2];
        let scene=b/(textureLoad(camera_depth,vec2<i32>(v.position.xy),0)+a);
        c.a*=clamp(particles.effect.x*(scene-v.eye),0.,1.);
    }
    if particles.effect.y>0. {
        // Source UnpackScaleNormal over RG or DXT5nm AG, then EFFECT_BUMP fragDistortion.
        let packed=textureSample(bump,bump_sampler,v.uv);
        let xy=(vec2(packed.r*packed.a,packed.g)*2.-1.)*particles.effect.w;
        let normal=normalize(vec3(xy,sqrt(1.-clamp(dot(xy,xy),0.,1.))));
        let uv=(v.grab.xy+normal.xy*particles.effect.y*c.a)/v.grab.w;
        let grabbed=textureSampleLevel(grab_texture,grab_sampler,uv,0.).rgb;
        c=vec4(mix(grabbed,c.rgb,clamp(c.a-particles.effect.z,0.,1.)),c.a);
    }
    c.a=clamp(c.a,0.,1.);
    let fog=exp2(-1.442695*pow(particles.fog.w*v.depth,2.));
    c=vec4(mix(particles.fog.rgb,c.rgb,fog),c.a);
    return c;
}
