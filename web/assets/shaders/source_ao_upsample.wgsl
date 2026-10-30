// Installed MultiScaleVOUpSample kernels3 and0, expressed as three fragment passes.
// The native shared float caches become R32F halos; raw inputs clamp before blur.
struct UpsampleParams {
    settings: vec4<f32>, // noise filter strength, step size, blur tolerance, upsample tolerance
    control: vec4<u32>, // x = blend with high AO
}
@group(0) @binding(0) var<uniform> params: UpsampleParams;
@group(0) @binding(1) var low_depth: texture_2d<f32>;
@group(0) @binding(2) var low_ao: texture_2d<f32>;
@group(0) @binding(3) var blur_input: texture_2d<f32>;
@group(0) @binding(4) var high_depth: texture_2d<f32>;
@group(0) @binding(5) var high_ao: texture_2d<f32>;
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
}
fn raw_depth(p: vec2<i32>) -> f32 {
    return textureLoad(low_depth, clamp(p, vec2(0), vec2<i32>(textureDimensions(low_depth))-vec2(1)), 0).r;
}
fn raw_ao(p: vec2<i32>) -> f32 {
    return textureLoad(low_ao, clamp(p, vec2(0), vec2<i32>(textureDimensions(low_ao))-vec2(1)), 0).r;
}
fn compare(a: f32, b: f32) -> bool {
    let product = fma(a, b, params.settings.y);
    let left = fma(a, a, params.settings.y);
    let right = fma(b, b, params.settings.y);
    return product*product > (left*right)*params.settings.z;
}
fn smart_blur(values: array<f32,5>, depths: array<f32,5>) -> f32 {
    let delta = vec4(depths[1]-depths[0], depths[2]-depths[1], depths[3]-depths[2], depths[4]-depths[3]);
    let left = compare(delta.x, delta.y);
    let middle = compare(delta.y, delta.z);
    let right = compare(delta.z, delta.w);
    let b = select(values[2], values[1], left || middle);
    let a = select(b, values[0], left);
    let d = select(values[2], values[3], right || middle);
    let e = select(d, values[4], right);
    return ((fma(a+e, 0.5, b)+values[2])+d)*0.25;
}
@fragment
fn horizontal(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    // Output extent=(lowWidth+2,lowHeight+6), source-center origin=(-1,-3).
    let center = vec2<i32>(position.xy)-vec2(1,3);
    var ao: array<f32,5>;
    var inverse: array<f32,5>;
    for(var i=0;i<5;i++) {
        let p=center+vec2(i-2,0);
        ao[i]=raw_ao(p);
        inverse[i]=1.0/raw_depth(p);
    }
    return smart_blur(ao,inverse);
}
@fragment
fn vertical(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    // Output extent=(lowWidth+2,lowHeight+2), source-center origin=(-1,-1).
    let center=vec2<i32>(position.xy)-vec2(1);
    var ao: array<f32,5>;
    var inverse: array<f32,5>;
    for(var i=0;i<5;i++) {
        let p=center+vec2(0,i-2);
        ao[i]=textureLoad(blur_input,p+vec2(1,3),0).r;
        inverse[i]=1.0/raw_depth(p);
    }
    return smart_blur(ao,inverse);
}
// D3D11.3 section3.2.3.6: NaN -> 0, infinities clamp before UNorm storage.
fn unorm_bits(bits: u32) -> u32 {
    let magnitude = bits & 0x7fffffffu;
    if magnitude > 0x7f800000u || (bits & 0x80000000u) != 0u { return 0u; }
    if magnitude > 0x3f800000u { return 0x3f800000u; }
    return bits;
}
fn unorm_value(value: f32) -> f32 { return bitcast<f32>(unorm_bits(bitcast<u32>(value))); }
@fragment
fn bilateral(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let pixel=vec2<i32>(position.xy);
    let tid=(pixel+vec2(1))/2;
    let offsets=array<vec2<i32>,4>(vec2(-1,0),vec2(0,0),vec2(0,-1),vec2(-1,-1));
    var depths: vec4<f32>;
    var blurred: vec4<f32>;
    for(var i=0u;i<4u;i++) {
        let p=tid+offsets[i];
        depths[i]=raw_depth(p);
        blurred[i]=textureLoad(blur_input,p+vec2(1),0).r;
    }
    var rotation=0u;
    if (pixel.x&1)==0 { rotation=select(2u,1u,(pixel.y&1)==0); }
    else { rotation=select(3u,0u,(pixel.y&1)==0); }
    let d=textureLoad(high_depth,pixel,0).r;
    var weights: vec4<f32>;
    var values: vec4<f32>;
    let multipliers=vec4(9.0,3.0,1.0,3.0);
    for(var i=0u;i<4u;i++) {
        let index=(i+rotation)&3u;
        weights[i]=multipliers[i]/(abs(d-depths[index])+params.settings.w);
        values[i]=blurred[index];
    }
    var high=1.0;
    if params.control.x!=0u { high=textureLoad(high_ao,pixel,0).r; }
    return unorm_value((high*(dot(values,weights)+params.settings.x))/(dot(weights,vec4(1.0))+params.settings.x));
}
