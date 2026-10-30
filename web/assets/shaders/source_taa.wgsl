#import source_float_store::half4
// Installed TemporalAntialiasing perspective pass0/blob3.
struct TaaParams {
    extent: vec4<f32>, // width, height, reciprocal width, reciprocal height
    jitter: vec4<f32>,
    blend: vec4<f32>, // stationary, motion, amplification, sharpness
};
@group(0) @binding(0) var<uniform> params: TaaParams;
@group(0) @binding(1) var source_color: texture_2d<f32>;
@group(0) @binding(2) var history_color: texture_2d<f32>;
@group(0) @binding(3) var source_depth: texture_depth_2d;
@group(0) @binding(4) var source_motion: texture_2d<f32>;
@group(0) @binding(5) var linear_clamp: sampler;
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv=vec2<f32>(f32((index<<1u)&2u),f32(index&2u));
    return vec4(uv*2.0-vec2(1.0),0.0,1.0);
}
fn clamped(uv: vec2<f32>) -> vec2<f32> { return clamp(uv,vec2(0.0),vec2(1.0)); }
fn closest_fragment(uv: vec2<f32>) -> vec2<f32> {
    let offsets=array<vec2<f32>,4>(vec2(-1.0,-1.0),vec2(1.0,-1.0),vec2(-1.0,1.0),vec2(1.0,1.0));
    let last=vec2<i32>(params.extent.xy)-vec2(1);
    let pixel=vec2<i32>(uv*params.extent.xy);
    var selected=vec2(0.0);
    var depth=textureLoad(source_depth,clamp(pixel,vec2(0),last),0);
    for(var index=0u;index<4u;index++) {
        let offset=offsets[index];
        let sample_depth=textureLoad(source_depth,clamp(pixel+vec2<i32>(offset),vec2(0),last),0);
        // Installed D3D reversed depth uses >=. Equal depths select the later corner.
        if sample_depth>=depth { selected=offset;depth=sample_depth; }
    }
    return uv+selected*params.extent.zw;
}
fn blend_weight(motion_length:f32)->f32 {
    let weight=fma(motion_length*params.blend.z,params.blend.y-params.blend.x,params.blend.x);
    // Preserve DXBC max then min, including the startup .85/.65 inverted bounds.
    return min(max(weight,params.blend.y),params.blend.x);
}
struct Resolved {
    @location(0) destination: vec4<f32>,
    @location(1) history: vec4<f32>,
};
@fragment
fn resolve(@builtin(position) position:vec4<f32>)->Resolved {
    let uv=position.xy*params.extent.zw;
    let motion=textureSampleLevel(source_motion,linear_clamp,closest_fragment(uv),0.0).xy;
    let motion_length=length(motion);
    var history=textureSampleLevel(history_color,linear_clamp,clamped(uv-motion),0.0);
    let center_uv=clamped(uv-params.jitter.xy);
    let top=textureSampleLevel(source_color,linear_clamp,clamped(center_uv-params.extent.zw*0.5),0.0);
    let bottom=textureSampleLevel(source_color,linear_clamp,clamped(center_uv+params.extent.zw*0.5),0.0);
    var color=textureSampleLevel(source_color,linear_clamp,center_uv,0.0);
    let corners=fma(top+bottom,vec4(4.0),-color*2.0);
    let sharpen=fma(-corners,vec4(0.166667),color)*params.blend.w;
    color=clamp(fma(sharpen,vec4(2.718282),color),vec4(0.0),vec4(65472.0));
    let average=(corners.rgb+color.rgb)*0.142857;
    let luma=vec3(0.2126729,0.7151522,0.0721750);
    let difference=abs(dot(average,luma)-dot(color.rgb,luma));
    let nudge=fma(min(motion_length*100.0,1.0),-3.75,4.0);
    let minimum=fma(vec3(-nudge),vec3(difference),min(top.rgb,bottom.rgb));
    let maximum=fma(vec3(nudge),vec3(difference),max(top.rgb,bottom.rgb));
    let center=(maximum+minimum)*0.5;
    let extents=(maximum-minimum)*0.5;
    let offset=history.rgb-center;
    let ratios=abs(extents/(offset+vec3(0.0001)));
    let distance=min(min(min(ratios.x,ratios.y),ratios.z),1.0);
    history=vec4(fma(offset,vec3(distance),center),history.a);
    color=clamp(fma(vec4(blend_weight(motion_length)),history-color,color),vec4(0.0),vec4(65472.0));
    var output:Resolved;output.destination=half4(color);output.history=half4(color);return output;
}
// Source CheckHistory copies current color when the history is invalid.
@fragment
fn seed(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32> {
    return half4(textureSampleLevel(source_color,linear_clamp,position.xy*params.extent.zw,0.0));
}
