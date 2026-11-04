// Installed GrainBaker pass 1 (colored), fragment blob 5. It draws the 128 x 128 grain lookup.
// The sums keep the source weights in a loop, not the unrolled DXBC order.
struct Params {
 // x: _Phase. yzw: _GrainNoiseParameters.
 noise: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@vertex fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
 let uv=vec2<f32>(f32((index<<1u)&2u),f32(index&2u));return vec4(uv*2.-1.,0.,1.);
}
fn noise(p:vec2<f32>)->f32 { return fract(sin(dot(p,params.noise.yz))*params.noise.w); }
// Step1: 3 x 3 high-pass of the noise. Corners 1, edges 2, center -12, over 24.
fn step1(p:vec2<f32>)->f32 {
 var sum=0.;
 for(var y=-1;y<=1;y++){
  for(var x=-1;x<=1;x++){
   let o=vec2(f32(x),f32(y));
   let w=select(select(1.,2.,x==0||y==0),-12.,x==0&&y==0);
   sum=fma(noise(p+o),w,sum);
  }
 }
 return sum/24.;
}
// Step2: 3 x 3 blur of Step1. Corners 1, edges 2, center 4, over 16.
fn step2(p:vec2<f32>)->f32 {
 var sum=0.;
 for(var y=-1;y<=1;y++){
  for(var x=-1;x<=1;x++){
   let o=vec2(f32(x),f32(y));
   let w=select(select(1.,2.,x==0||y==0),4.,x==0&&y==0);
   sum=fma(step1(p+o),w,sum);
  }
 }
 return sum*.0625;
}
@fragment fn grain(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
 // texcoord * 128 is the texel center. Each channel adds its own phase offset to both axes.
 let p=position.xy;
 let phase=fract(params.noise.x);
 return vec4(step2(p+phase*.07),step2(p+phase*.11),step2(p+phase*.13),1.);
}
