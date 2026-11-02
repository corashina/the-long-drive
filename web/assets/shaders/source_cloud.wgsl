// Installed cloud shader 721, vertex blob2 and fragment blob5.
struct CloudParams {
    world_from_local: mat4x4<f32>,
    clip_from_world: mat4x4<f32>,
    uv: array<vec4<f32>, 4>,
    color: vec4<f32>,
    // speed, source _Time.x, density, unused
    controls: vec4<f32>,
};
@group(0) @binding(0) var<uniform> cloud: CloudParams;
@group(0) @binding(1) var scatter0: texture_2d<f32>;
@group(0) @binding(2) var sampler0: sampler;
@group(0) @binding(3) var scatter1: texture_2d<f32>;
@group(0) @binding(4) var sampler1: sampler;
@group(0) @binding(5) var density: texture_2d<f32>;
@group(0) @binding(6) var sampler2: sampler;
@group(0) @binding(7) var detail: texture_2d<f32>;
@group(0) @binding(8) var sampler3: sampler;
struct CloudVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) scatter_uv: vec4<f32>,
    @location(1) detail_uv: vec4<f32>,
};
fn source_transform(matrix: mat4x4<f32>, position: vec4<f32>) -> vec4<f32> {
    var value = matrix[1] * position.y;
    value = fma(matrix[0], vec4(position.x), value);
    value = fma(matrix[2], vec4(position.z), value);
    return fma(matrix[3], vec4(position.w), value);
}
@vertex
fn vertex(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> CloudVertex {
    var out: CloudVertex;
    let world = source_transform(cloud.world_from_local, vec4(position, 1.0));
    out.position = source_transform(cloud.clip_from_world, world);
    let phase = cloud.controls.x * cloud.controls.y;
    let uv0 = fma(uv, cloud.uv[0].xy, cloud.uv[0].zw);
    let uv1 = fma(uv, cloud.uv[1].xy, cloud.uv[1].zw);
    let uv2 = fma(uv, cloud.uv[2].xy, cloud.uv[2].zw);
    let uv3 = fma(uv, cloud.uv[3].xy, cloud.uv[3].zw);
    out.scatter_uv = vec4(fma(uv0, vec2(2.0), phase * vec2(1.5,1.0)),
        fma(uv1, vec2(2.0), phase * vec2(1.0,1.2)));
    out.detail_uv = vec4(fma(uv2, vec2(10.0,10.5), phase * vec2(0.75,0.5)),
        fma(uv3, vec2(10.6,10.0), phase * vec2(0.5,0.6)));
    return out;
}
@fragment
fn fragment(in: CloudVertex) -> @location(0) vec4<f32> {
    var sum = textureSample(scatter0, sampler0, in.scatter_uv.xy).r;
    sum += textureSample(scatter1, sampler1, in.scatter_uv.zw).r;
    sum += textureSample(density, sampler2, in.detail_uv.xy).r;
    sum += textureSample(detail, sampler3, in.detail_uv.zw).r;
    let coverage = clamp(sum - cloud.controls.z, 0.0, 1.0);
    let alpha = clamp(dot(vec2(coverage), vec2(cloud.color.a)), 0.0, 1.0);
    return vec4(cloud.color.rgb, alpha);
}
