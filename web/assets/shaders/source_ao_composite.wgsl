// Installed shader19/pass2/blob22: Exp2 fog, original eight-sample color destination.
struct CompositeParams { projection:vec4<f32> } // near, far, far/near-1, raw fog density
@group(0) @binding(0) var<uniform> params:CompositeParams;
@group(0) @binding(1) var camera_depth:texture_2d<f32>;
@group(0) @binding(2) var visibility:texture_2d<f32>;
@group(0) @binding(3) var point_clamp:sampler;
@vertex
fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32>{let uv=vec2<f32>(f32((index<<1u)&2u),f32(index&2u));return vec4(uv*2.-vec2(1.),0.,1.);}
@fragment
fn composite(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>{
 let uv=position.xy/vec2<f32>(textureDimensions(visibility));
 let raw=textureSampleLevel(camera_depth,point_clamp,uv,0.).r;
 let linear=1./fma(params.projection.z,raw,1.);
 let distance=fma(linear,params.projection.y,-params.projection.x);
 let exponent=distance*params.projection.w;
 let fog=exp2(-exponent*exponent);
 let ao=textureSampleLevel(visibility,point_clamp,uv,0.).r;
 return vec4(vec3((1.-ao)*fog),0.);
}
