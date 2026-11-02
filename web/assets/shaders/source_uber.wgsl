#import source_float_store::{half4, source_max, source_sat}
// Installed Uber4653 (main HDR), Uber4641 (main LDR) and Uber4251 (far/foreground), Linear project.
struct Params { extent: vec4<f32>, noise: vec4<f32>, bloom: vec4<f32>, tint: vec4<f32>, dirt: vec4<f32>, grading: vec4<f32>, vignette: vec4<f32>,
 // Binocular lens: _Distortion_Amount, _Distortion_CenterScale, then CA amount, binoculars, distortion.
 distortion: vec4<f32>, distortion_center: vec4<f32>, lens: vec4<f32>, strip: vec4<f32> }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var source_color: texture_2d<f32>;
@group(0) @binding(2) var noise_texture: texture_2d<f32>;
@group(0) @binding(3) var linear_clamp: sampler;
@group(0) @binding(4) var point_repeat: sampler;
#ifdef SOURCE_UBER_MAIN
@group(0) @binding(5) var bloom_texture: texture_2d<f32>;
@group(0) @binding(6) var exposure_texture: texture_2d<f32>;
@group(0) @binding(7) var dirt_texture: texture_2d<f32>;
@group(0) @binding(8) var lut_texture: texture_3d<f32>;
@group(0) @binding(9) var linear_repeat: sampler;
@group(0) @binding(10) var vignette_mask: texture_2d<f32>;
@group(0) @binding(11) var strip_lut: texture_2d<f32>;
#endif
fn f(bits:u32)->f32 { return bitcast<f32>(bits); }
fn sat3(x:vec3<f32>)->vec3<f32> { return vec3(source_sat(x.x),source_sat(x.y),source_sat(x.z)); }
fn safe_abs(x:vec3<f32>)->vec3<f32> { return vec3(source_max(abs(x.x),f(0x34000000u)),source_max(abs(x.y),f(0x34000000u)),source_max(abs(x.z),f(0x34000000u))); }
fn linear_to_srgb(c:vec3<f32>)->vec3<f32> {
 let high=fma(exp2(log2(safe_abs(c))*f(0x3ed55555u)),vec3(f(0x3f870a3du)),vec3(f(0xbd6147aeu)));
 return select(high,c*f(0x414eb852u),c<=vec3(f(0x3b4d2e1cu)));
}
fn srgb_to_linear(c:vec3<f32>)->vec3<f32> {
 let high=exp2(log2(safe_abs((c+f(0x3d6147aeu))*f(0x3f72a76fu)))*f(0x4019999au));
 return select(high,c*f(0x3d9e8391u),c<=vec3(f(0x3d25aee6u)));
}
#ifdef SOURCE_UBER_MAIN
// Post stack v2 Distort. `uv` is Unity UV.
fn distort(uv:vec2<f32>)->vec2<f32> {
 if params.lens.z==0. { return uv; }
 let u=(uv-.5)*params.distortion.z+.5;
 let ruv=params.distortion_center.zw*(u-.5-params.distortion_center.xy);
 var ru=length(ruv);
 if ru==0. { return u; }
 if params.distortion.w>0. {
  ru=tan(ru*params.distortion.x)*(1./(ru*params.distortion.y));
 } else {
  ru=(1./ru)*params.distortion.x*atan(ru*params.distortion.y);
 }
 return u+ruv*(ru-1.);
}
fn flip(uv:vec2<f32>)->vec2<f32> { return vec2(uv.x,1.-uv.y); }
fn main_color(uv:vec2<f32>)->vec4<f32> { return textureSampleLevel(source_color,linear_clamp,flip(distort(uv)),0.); }
#endif
@vertex fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
 let uv=vec2<f32>(f32((index<<1u)&2u),f32(index&2u));return vec4(uv*2.-1.,0.,1.);
}
@fragment fn uber(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
 let uv=position.xy*params.extent.zw;
#ifdef SOURCE_UBER_MAIN
 var color:vec4<f32>;
 // Distorted screen UV for bloom, dirt and the vignette.
 var effect_uv=uv;
 if params.lens.y>0. {
  let u=flip(uv);
  effect_uv=flip(distort(u));
  if params.lens.x>0. {
   // CHROMATIC_ABERRATION_LOW with the internal red, green, blue spectrum.
   let coords=2.*u-1.;
   let delta=-coords*dot(coords,coords)*params.lens.x/3.;
   let a=main_color(u);let b=main_color(delta+u);let c=main_color(delta*2.+u);
   color=vec4(a.r,b.g,c.b,(a.a+b.a+c.a)/3.);
  } else {
   color=textureSampleLevel(source_color,linear_clamp,effect_uv,0.);
  }
 } else {
  color=textureSampleLevel(source_color,linear_clamp,uv,0.);
 }
 let texel=vec2(1.)/vec2<f32>(textureDimensions(bloom_texture));
 let radius=params.bloom.x*.5;
 let b1=textureSampleLevel(bloom_texture,linear_clamp,clamp(fma(-texel,vec2(radius),effect_uv),vec2(0.),vec2(1.)),0.);
 let b2=textureSampleLevel(bloom_texture,linear_clamp,clamp(fma(texel*vec2(1.,-1.),vec2(radius),effect_uv),vec2(0.),vec2(1.)),0.);
 let b3=textureSampleLevel(bloom_texture,linear_clamp,clamp(fma(texel*vec2(-1.,1.),vec2(radius),effect_uv),vec2(0.),vec2(1.)),0.);
 let b4=textureSampleLevel(bloom_texture,linear_clamp,clamp(fma(texel,vec2(radius),effect_uv),vec2(0.),vec2(1.)),0.);
 let bloom=(b4+((b2+b1)+b3))*params.bloom.y*.25;
 let exposure=textureLoad(exposure_texture,vec2<i32>(0),0).r;
 color=vec4(color.rgb*exposure,color.a)+vec4(bloom.rgb*params.tint.rgb,bloom.a);
 let dirt=textureSampleLevel(dirt_texture,linear_repeat,fma(effect_uv,params.dirt.xy,params.dirt.zw),0.).rgb*params.bloom.z;
 color=fma(vec4(dirt,0.),bloom,color);
 // Post stack v2 masked vignette (binocular volume). Unity UV starts at the bottom.
 if params.vignette.w>0. {
  let mask=srgb_to_linear(vec3(textureSampleLevel(vignette_mask,linear_clamp,flip(effect_uv),0.).a)).x;
  let masked=color.rgb*mix(params.vignette.rgb,vec3(1.),mask);
  color=vec4(mix(color.rgb,masked,params.vignette.w),mix(1.,color.a,mask));
 }
#ifdef SOURCE_UBER_LDR
 // Installed Uber4641 COLOR_GRADING_LDR_2D: ApplyLut2D on the sRGB color, then back to linear.
 color=vec4(sat3(color.rgb),source_sat(color.a));
 let srgb=linear_to_srgb(color.rgb);
 let scaled=srgb*params.strip.z;
 let shift=floor(scaled.b);
 let strip_uv=fma(scaled.rg,params.strip.xy,params.strip.xy*.5);
 let first=vec2(fma(shift,params.strip.y,strip_uv.x),strip_uv.y);
 let a=textureSampleLevel(strip_lut,linear_clamp,first,0.).rgb;
 let b=textureSampleLevel(strip_lut,linear_clamp,first+vec2(params.strip.y,0.),0.).rgb;
 color=vec4(srgb_to_linear(fma(vec3(fma(srgb.b,params.strip.z,-shift)),b-a,a)),color.a);
#else
 color=color*params.grading.z;
 let encoded=sat3(fma(log2(fma(color.rgb,vec3(f(0x40b1c71du)),vec3(f(0x3d449774u)))),vec3(f(0x3d96870eu)),vec3(f(0x3ec5a683u))));
 let lut_uv=fma(encoded*params.grading.y,vec3(params.grading.x),vec3(params.grading.x*.5));
 color=vec4(textureSampleLevel(lut_texture,linear_clamp,lut_uv,0.).rgb,color.a);
#endif
#else
 var color=textureSampleLevel(source_color,linear_clamp,uv,0.);
#endif
 // Lighting image loading preserves the source Alpha8 channel in RGBA8 alpha.
 let noise=textureSampleLevel(noise_texture,point_repeat,fma(uv,params.noise.xy,params.noise.zw),0.).a;
 let n=fma(noise,2.,-1.);
 let direction=fma(source_sat(fma(n,f(0x7f7fffffu),.5)),2.,-1.);
 let triangular=direction*(1.-sqrt(1.-abs(n)));
 return half4(vec4(srgb_to_linear(fma(vec3(triangular),vec3(f(0x3b808081u)),linear_to_srgb(color.rgb))),color.a));
}
