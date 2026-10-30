// Installed KEyeHistogram:128 weighted bins, source pixel stride2, bilinear LOD0.
// Every scatter tile has at most32 samples, so each RGBA16F component sums to <=2048 exactly.
struct HistogramParams {
    source_size: vec2<u32>,
    sample_extent: vec2<u32>,
    scatter_extent: vec2<u32>,
    tile_columns: u32,
    input_tiles: u32,
}
@group(0) @binding(0) var<uniform> params: HistogramParams;
@group(0) @binding(1) var source_color: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;
@group(0) @binding(3) var float_partials: texture_2d<f32>;
@group(0) @binding(4) var uint_partials: texture_2d<u32>;

struct ScatterOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) weight: vec4<f32>,
}
@vertex
fn scatter_vertex(@builtin(vertex_index) vertex: u32) -> ScatterOutput {
    let sample_index = vertex / 6u;
    let pixel = vec2(sample_index % params.sample_extent.x, sample_index / params.sample_extent.x);
    let uv = vec2<f32>(pixel * 2u) / vec2<f32>(params.source_size);
    let distance = uv - vec2(0.5);
    let factor = max(0.0, 1.0 - dot(abs(distance), abs(distance)));
    let weight = u32((factor * factor) * 64.0);
    let color = textureSampleLevel(source_color, source_sampler, uv, 0.0).rgb;
    let luminance = dot(color, vec3(0.2126729, 0.7151522, 0.0721750));
    // The source clamps log2(0) to bin0. Avoid backend-dependent conversion of infinity.
    var logarithm = 0.0;
    if luminance > 0.0 { logarithm = clamp(log2(luminance) * (1.0 / 18.0) + 0.5, 0.0, 1.0); }
    let bin = u32(logarithm * 127.0);
    let tile = sample_index / 32u;
    let destination = vec2((tile % params.tile_columns) * 32u + bin / 4u, tile / params.tile_columns);
    let corners = array<vec2<f32>, 6>(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let corner = (vec2<f32>(destination) + corners[vertex % 6u]) / vec2<f32>(params.scatter_extent);
    var output: ScatterOutput;
    output.position = vec4(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
    output.weight = vec4(0.0);
    output.weight[bin % 4u] = f32(weight);
    return output;
}
@fragment
fn scatter_fragment(input: ScatterOutput) -> @location(0) vec4<f32> { return input.weight; }

@vertex
fn reduce_vertex(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    return vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
}
@fragment
fn reduce_float(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let first = u32(position.y) * 256u;
    let end = min(first + 256u, params.input_tiles);
    var sum = vec4(0u);
    for (var tile = first; tile < end; tile += 1u) {
        let pixel = vec2<i32>(i32((tile % params.tile_columns) * 32u + u32(position.x)), i32(tile / params.tile_columns));
        sum += vec4<u32>(textureLoad(float_partials, pixel, 0));
    }
    return sum;
}
@fragment
fn reduce_uint(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let first = u32(position.y) * 256u;
    let end = min(first + 256u, params.input_tiles);
    var sum = vec4(0u);
    for (var tile = first; tile < end; tile += 1u) {
        sum += textureLoad(uint_partials, vec2<i32>(i32(position.x), i32(tile)), 0);
    }
    return sum;
}
