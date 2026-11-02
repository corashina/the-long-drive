// Source KAutoExposureAvgLuminance fixed/progressive, expressed as one fragment.
struct ExposureParams {
    filtering: vec4<f32>, // low/high fractions, min/max luminance
    adaptation: vec4<f32>, // speedDown, speedUp, key, frame delta
    control: vec4<u32>, // x: reset history
}
@group(0) @binding(0) var<uniform> params: ExposureParams;
@group(0) @binding(1) var histogram: texture_2d<u32>;
@group(0) @binding(2) var previous: texture_2d<f32>;
@vertex
fn vertex(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
}
@fragment
fn exposure() -> @location(0) f32 {
    var counts: array<u32,128>;
    var maximum = 0u;
    for(var texel=0u;texel<32u;texel+=1u){
        let value=textureLoad(histogram,vec2<i32>(i32(texel),0),0);
        for(var channel=0u;channel<4u;channel+=1u){
            counts[texel*4u+channel]=value[channel];
            maximum=max(maximum,value[channel]);
        }
    }
    // A nonempty source viewport always contributes positive histogram weight.
    let inverse_maximum=1.0/f32(maximum);
    var total=fma(f32(counts[0]),inverse_maximum,f32(counts[1])*inverse_maximum);
    for(var i=2u;i<128u;i+=1u){total=fma(f32(counts[i]),inverse_maximum,total);}
    var remaining=total*params.filtering.xy;
    var filtered_sum=0.0;
    var filtered_weight=0.0;
    for(var i=0u;i<128u;i+=1u){
        let value=f32(counts[i])*inverse_maximum;
        let dark=min(remaining.x,value);
        let after_dark=fma(f32(counts[i]),inverse_maximum,-dark);
        remaining-=vec2(dark);
        let kept=min(remaining.y,after_dark);
        remaining.y-=kept;
        let luminance=exp2((f32(i)/128.0-0.5)/(1.0/18.0));
        filtered_sum+=luminance*kept;
        filtered_weight+=kept;
    }
    let average=clamp(filtered_sum/max(filtered_weight,0.0001),params.filtering.z,params.filtering.w);
    let current=params.adaptation.z/max(average,0.0001);
    if params.control.x!=0u{return current;}
    let old=textureLoad(previous,vec2<i32>(0),0).r;
    let delta=current-old;
    let speed=select(params.adaptation.y,params.adaptation.x,delta>0.0);
    return fma(delta,1.0-exp2(-params.adaptation.w*speed),old);
}
