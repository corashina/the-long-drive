// Installed Bloom fast passes1/3/5, fragment blobs5/9/13.
#import source_float_store::{half4, source_min, source_max, source_sat}
struct Params {
    output_size: vec2<f32>,
    sample_scale: f32,
    clamp_value: f32,
    threshold: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var main_tex: texture_2d<f32>;
@group(0) @binding(2) var higher_tex: texture_2d<f32>;
@group(0) @binding(3) var exposure_tex: texture_2d<f32>;
@group(0) @binding(4) var linear_clamp: sampler;
@group(0) @binding(5) var point_clamp: sampler;
@vertex fn vertex(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
    return vec4(vec2<f32>(f32((i<<1u)&2u),f32(i&2u))*2.-1.,0.,1.);
}
fn tap(uv:vec2<f32>)->vec4<f32>{
    // Selected non-stereo viewport scale is1. Preserve saturation before sampling.
    return textureSampleLevel(main_tex,linear_clamp,vec2(source_sat(uv.x),source_sat(uv.y)),0.);
}
fn sum4(uv:vec2<f32>,scale:f32)->vec4<f32>{
    let texel=1./vec2<f32>(textureDimensions(main_tex));
    let offset=texel*scale;
    let a=tap(uv+vec2(-offset.x,-offset.y));
    let b=tap(uv+vec2(offset.x,-offset.y));
    let c=tap(uv+vec2(-offset.x,offset.y));
    let d=tap(uv+vec2(offset.x,offset.y));
    return d+((b+a)+c);
}
@fragment fn prefilter(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
    let uv=p.xy/params.output_size;
    let average=sum4(uv,1.)*0.25;
    var value=vec4(source_min(average.x,65504.),source_min(average.y,65504.),source_min(average.z,65504.),source_min(average.w,65504.));
    value=value*textureSampleLevel(exposure_tex,point_clamp,uv,0.).r;
    value=vec4(source_min(value.x,params.clamp_value),source_min(value.y,params.clamp_value),source_min(value.z,params.clamp_value),source_min(value.w,params.clamp_value));
    let brightness=source_max(value.z,source_max(value.y,value.x));
    let soft=source_min(source_max(brightness-params.threshold.y,0.),params.threshold.z);
    let square=soft*(soft*params.threshold.w);
    let scale=source_max(brightness-params.threshold.x,square)/source_max(brightness,0.0001);
    return half4(value*scale);
}
@fragment fn downsample(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
    return half4(sum4(p.xy/params.output_size,1.)*0.25);
}
@fragment fn upsample(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
    let uv=p.xy/params.output_size;
    let sum=sum4(uv,params.sample_scale*0.5);
    let higher=textureSampleLevel(higher_tex,linear_clamp,uv,0.);
    return half4(fma(sum,vec4(0.25),higher));
}
