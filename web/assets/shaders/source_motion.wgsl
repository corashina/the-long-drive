// Source camera and object motion vectors: current minus previous viewport uv.
struct MotionCamera {
    raster: mat4x4<f32>, // jittered clip from world
    world_from_raster: mat4x4<f32>,
    current: mat4x4<f32>, // unjittered clip from world
    previous: mat4x4<f32>,
}
struct MotionObject { current_model: mat4x4<f32>, previous_model: mat4x4<f32> }
@group(0) @binding(0) var<uniform> camera: MotionCamera;
@group(0) @binding(1) var camera_depth: texture_depth_2d;
@group(1) @binding(0) var<uniform> object: MotionObject;
fn viewport(clip: vec4<f32>) -> vec2<f32> {
    let uv = (clip.xy / clip.w + 1.) * .5;
    return vec2(uv.x, 1. - uv.y);
}
@vertex fn camera_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(uv * 2. - 1., 0., 1.);
}
// Static surfaces move only with the camera. The homogeneous point keeps far depth finite.
@fragment fn camera_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec2<f32> {
    let extent = vec2<f32>(textureDimensions(camera_depth));
    let uv = position.xy / extent;
    let depth = textureLoad(camera_depth, vec2<i32>(position.xy), 0);
    let world = camera.world_from_raster * vec4(uv.x * 2. - 1., 1. - uv.y * 2., depth, 1.);
    return viewport(camera.current * world) - viewport(camera.previous * world);
}
struct ObjectOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) current: vec4<f32>,
    @location(1) previous: vec4<f32>,
}
@vertex fn object_vertex(@location(0) position: vec3<f32>) -> ObjectOutput {
    let world = object.current_model * vec4(position, 1.);
    var output: ObjectOutput;
    output.position = camera.raster * world;
    output.current = camera.current * world;
    output.previous = camera.previous * object.previous_model * vec4(position, 1.);
    return output;
}
@fragment fn object_fragment(input: ObjectOutput) -> @location(0) vec2<f32> {
    return viewport(input.current) - viewport(input.previous);
}
