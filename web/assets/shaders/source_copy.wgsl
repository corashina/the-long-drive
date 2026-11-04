#import source_float_store::half4
// Installed CopyStd pass1. Preserve infinities. Zero all RGBA when any sampled component is NaN.
@group(0) @binding(0) var source_color: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
struct Fullscreen {
    @builtin(position) position: vec4<f32>,
    // 0 at the top left and 1 at the bottom right of the target.
    @location(0) uv: vec2<f32>,
}
@vertex
fn vertex(@builtin(vertex_index) vertex:u32)->Fullscreen{
    let uv=vec2<f32>(f32((vertex<<1u)&2u),f32(vertex&2u));
    return Fullscreen(vec4(uv*2.0-vec2(1.0),0.0,1.0),vec2(uv.x,1.0-uv.y));
}
@fragment
fn copy(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>{
    return half4(textureSample(source_color,source_sampler,position.xy/vec2<f32>(textureDimensions(source_color))));
}
@fragment
fn copy_kill_nan(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>{
    let color=textureSample(source_color,source_sampler,position.xy/vec2<f32>(textureDimensions(source_color)));
    // Integer classification prevents backend folding of the ordered float predicate.
    let magnitude=bitcast<vec4<u32>>(color)&vec4(0x7fffffffu);
    if any(magnitude>vec4(0x7f800000u)) { return vec4(0.0); }
    return half4(color);
}

// Installed Hidden/BlitCopy. Sample linear RGBA and let the sRGB display attachment encode RGB.
@fragment
fn display_copy(@builtin(position) position:vec4<f32>)->@location(0) vec4<f32>{
    // Native positive-texel stack blit maps screen top to source V=1.
    let uv=position.xy/vec2<f32>(textureDimensions(source_color));
    return textureSample(source_color,source_sampler,vec2(uv.x,1.0-uv.y));
}

// Stretches the held image over the canvas. The scene scale makes the held image smaller than the canvas.
@fragment
fn present(@location(0) uv:vec2<f32>)->@location(0) vec4<f32>{
    return textureSample(source_color,source_sampler,uv);
}

// Copies the static shadow tile into the atlas tile. Viewport and scissor select the tile.
@group(0) @binding(2) var static_shadow: texture_depth_2d;
@fragment
fn restore_shadow(@builtin(position) position:vec4<f32>)->@builtin(frag_depth) f32{
    return textureLoad(static_shadow,vec2<i32>(position.xy),0);
}
