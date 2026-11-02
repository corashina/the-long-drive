// Installed MotionBlur shader15 passes0..5, fragment blobs3/5/7/9/11/13.
#import source_float_store::{half2, half4, source_max, source_min, source_sat}
struct Params {
    output_size: vec2<f32>,
    screen_size: vec2<f32>,
    settings: vec4<f32>, // radius, reciprocal radius, velocity scale, loop count
    depth: vec4<f32>, // reversed-Z Linear01 x/y, tile offset, unused
    control: vec4<u32>, // tile loop
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var main_tex: texture_2d<f32>;
@group(0) @binding(2) var velocity_tex: texture_2d<f32>;
@group(0) @binding(3) var neighbor_tex: texture_2d<f32>;
@group(0) @binding(4) var camera_depth: texture_depth_2d;
@group(0) @binding(5) var camera_motion: texture_2d<f32>;
@group(0) @binding(6) var point_clamp: sampler;
@group(0) @binding(7) var linear_clamp: sampler;
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv=vec2<f32>(f32((index<<1u)&2u), f32(index&2u));
    return vec4(uv*2.0-vec2(1.0),0.0,1.0);
}
fn uv_at(position: vec4<f32>) -> vec2<f32> { return position.xy/params.output_size; }
fn maxv(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> { return select(a,b,dot(a,a)<dot(b,b)); }
fn decode(v: vec2<f32>) -> vec2<f32> { return fma(v,vec2(2.0),vec2(-1.0))*params.settings.x; }
fn main_point(uv: vec2<f32>) -> vec2<f32> { return textureSampleLevel(main_tex,point_clamp,uv,0.0).rg; }
@fragment
fn velocity(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv=uv_at(position);
    let size=vec2<f32>(textureDimensions(camera_motion));
    var v=((params.settings.z*0.5)*size)*textureSampleLevel(camera_motion,linear_clamp,uv,0.0).rg;
    v=v/source_max(length(v)*params.settings.y,1.0);
    let raw=textureLoad(camera_depth,vec2<i32>(position.xy),0);
    let z=raw*params.depth.x;
    let d=1.0/(z+params.depth.y);
    let packed=fma(v,vec2(params.settings.y),vec2(1.0))*0.5;
    // UNorm conversion maps NaN to zero and clamps infinity. Native packed depth remains quantized.
    return vec4(source_sat(packed.x),source_sat(packed.y),source_sat(d),0.0);
}
fn four(uv: vec2<f32>, normalized: bool) -> vec2<f32> {
    let texel=1.0/vec2<f32>(textureDimensions(main_tex));
    var a=main_point(fma(texel,vec2(-0.5,-0.5),uv));
    var b=main_point(fma(texel,vec2(0.5,-0.5),uv));
    var c=main_point(fma(texel,vec2(-0.5,0.5),uv));
    var d=main_point(fma(texel,vec2(0.5,0.5),uv));
    if normalized { a=decode(a); b=decode(b); c=decode(c); d=decode(d); }
    return maxv(maxv(maxv(a,b),c),d);
}
@fragment
fn tile_decode(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    return half2(four(uv_at(position),true));
}
@fragment
fn tile_reduce(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    return half2(four(uv_at(position),false));
}
@fragment
fn tile_variable(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let texel=1.0/vec2<f32>(textureDimensions(main_tex));
    let start=fma(texel,vec2(params.depth.z),uv_at(position));
    var best=vec2(0.0);
    for(var x=0u;x<params.control.x;x++) {
        let column=fma(vec2(texel.x,0.0),vec2(f32(x)),start);
        for(var y=0u;y<params.control.x;y++) {
            let uv=fma(vec2(0.0,texel.y),vec2(f32(y)),column);
            best=maxv(best,main_point(uv));
        }
    }
    return half2(best);
}
@fragment
fn neighbor(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let uv=uv_at(position);
    let texel=1.0/vec2<f32>(textureDimensions(main_tex));
    var values: array<vec2<f32>,9>;
    for(var y=0;y<3;y++) { for(var x=0;x<3;x++) {
        values[u32(y*3+x)]=main_point(fma(texel,vec2<f32>(f32(x-1),f32(y-1)),uv));
    } }
    values[4]=values[4]*bitcast<f32>(0x3f8147aeu);
    let a=maxv(values[0],maxv(values[1],values[2]));
    let b=maxv(values[3],maxv(values[4],values[5]));
    let c=maxv(values[6],maxv(values[7],values[8]));
    return half2(maxv(a,maxv(b,c))*bitcast<f32>(0x3f7d7721u));
}
fn noise(uv: vec2<f32>) -> f32 {
    let p=floor(uv*params.screen_size);
    return fract(fract(dot(vec2(bitcast<f32>(0x3d897143u),bitcast<f32>(0x3bbf4590u)),p))*bitcast<f32>(0x4253ee82u));
}
fn velocity_at(uv: vec2<f32>) -> vec3<f32> {
    let value=textureSampleLevel(velocity_tex,point_clamp,uv,0.0).rgb;
    return vec3(decode(value.xy),value.z);
}
@fragment
fn reconstruct(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv=uv_at(position);
    let cp=textureSampleLevel(main_tex,linear_clamp,uv,0.0);
    let angle=noise(uv+vec2(2.0,0.0))*bitcast<f32>(0x40c90fdbu);
    let jitter=vec2(cos(angle),sin(angle))/vec2<f32>(textureDimensions(neighbor_tex));
    let vmax=textureSampleLevel(neighbor_tex,point_clamp,fma(jitter,vec2(0.25),uv),0.0).rg;
    let lm=length(vmax);
    if lm<2.0 { return half4(cp); }
    let vp=velocity_at(uv);
    let speed=length(vp.xy);
    let lp=source_max(speed,0.5);
    var bg=source_max(speed,1.0);
    let reciprocal_depth=1.0/vp.z;
    let alt=select(vmax,vp.xy*(lm/lp),lm<lp+lp);
    let sc=floor(source_min(lm*0.5,params.settings.w));
    let dt=1.0/sc;
    let offset=noise(uv)-0.5;
    var t=fma(-dt,0.5,1.0);
    var count=0.0;
    var acc=vec4(0.0);
    loop {
        if dt*0.25>=t { break; }
        let alternate=fract(count*0.25)>bitcast<f32>(0x3eff7ceeu);
        let negative=fract(count*0.5)>bitcast<f32>(0x3eff7ceeu);
        let direction=select(vmax,alt,alternate);
        let ts=fma(offset,dt,select(t,-t,negative));
        let displacement=direction*ts;
        let color_uv=fma(displacement,1.0/vec2<f32>(textureDimensions(main_tex)),uv);
        let velocity_uv=fma(displacement,1.0/vec2<f32>(textureDimensions(velocity_tex)),uv);
        let color=textureSampleLevel(main_tex,linear_clamp,color_uv,0.0).rgb;
        let vd=velocity_at(velocity_uv);
        let fg=source_sat(((vp.z-vd.z)*reciprocal_depth)*20.0);
        let sample_speed=fma(fg,length(vd.xy)-bg,bg);
        let weight=(source_sat(fma(-lm,abs(ts),sample_speed))/sample_speed)*(bitcast<f32>(0x3f99999au)-t);
        acc=fma(vec4(color,1.0),vec4(weight),acc);
        bg=source_max(bg,sample_speed);
        if negative { t=t-dt; }
        count=count+1.0;
    }
    let center_weight=bitcast<f32>(0x3f99999au)/dot(vec2(bg),vec2(sc));
    acc=fma(vec4(cp.rgb,1.0),vec4(center_weight),acc);
    return half4(vec4(acc.rgb/acc.a,cp.a));
}
@fragment
fn reset_copy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return half4(textureSampleLevel(main_tex,linear_clamp,uv_at(position),0.0));
}
