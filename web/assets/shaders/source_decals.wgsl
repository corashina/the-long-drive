// Bullet-hole decals: oriented quads of one ShootDecals prefab, alpha blended, sun and ambient light.
#import bevy_pbr::mesh_view_bindings::view
struct Decals {
    poses:array<vec4<f32>,256>,rotations:array<vec4<f32>,256>,
    node:mat4x4<f32>,tint:vec4<f32>,st:vec4<f32>,fog:vec4<f32>,ambient:vec4<f32>,sun:vec4<f32>,sun_color:vec4<f32>,settings:vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> decals:Decals;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var image:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var image_sampler:sampler;
struct Vertex {@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>}
struct Varyings {@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) light:vec3<f32>,@location(2) depth:f32}
fn rotate(q:vec4<f32>,v:vec3<f32>)->vec3<f32> {let t=2.*cross(q.xyz,v);return v+q.w*t+cross(q.xyz,t);}
@vertex fn vertex(v:Vertex)->Varyings {
    let i=u32(v.position.z);let p=decals.poses[i];let q=decals.rotations[i];
    let local=(decals.node*vec4(v.position.xy,0.,1.)).xyz*p.w;
    var center=view.view_from_world*vec4(p.xyz+rotate(q,local),1.);
    var o:Varyings;o.position=view.clip_from_view*center;o.uv=v.uv*decals.st.xy+decals.st.zw;
    // LookRotation(normal) in converted space: the surface normal is local -Z.
    let normal=rotate(q,vec3(0.,0.,-1.));
    o.light=select(decals.ambient.rgb+decals.sun_color.rgb*max(dot(normal,decals.sun.xyz),0.),vec3(1.),decals.settings.x==0.);
    let a=view.clip_from_view[2][2];let b=view.clip_from_view[3][2];let near=b/(a+1.);let far=b/a;
    o.depth=max((abs(center.z)-near)*far/(far-near),0.);
    if i>=u32(decals.settings.w) {o.position=vec4(2.,2.,2.,1.);}
    return o;
}
@fragment fn fragment(v:Varyings)->@location(0) vec4<f32> {
    var c=textureSample(image,image_sampler,v.uv)*decals.tint;
    c=vec4(c.rgb*v.light,clamp(c.a,0.,1.));
    let fog=exp2(-1.442695*pow(decals.fog.w*v.depth,2.));
    return vec4(mix(decals.fog.rgb,c.rgb,fog),c.a);
}
