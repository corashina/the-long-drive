// WebGPU has no depth resolve attachment. Copy MSAA depth sample 0 into the single-sample depth.
@group(0) @binding(0) var msaa_depth: texture_depth_multisampled_2d;
@vertex
fn vertex(@builtin(vertex_index) vertex:u32)->@builtin(position) vec4<f32>{
    let uv=vec2<f32>(f32((vertex<<1u)&2u),f32(vertex&2u));
    return vec4(uv*2.0-vec2(1.0),0.0,1.0);
}
@fragment
fn fragment(@builtin(position) position:vec4<f32>)->@builtin(frag_depth) f32{
    return textureLoad(msaa_depth,vec2<i32>(position.xy),0);
}
