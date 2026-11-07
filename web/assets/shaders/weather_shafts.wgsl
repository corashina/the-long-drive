// Distant rain shafts. One instance is one wet cell: a soft billboard that turns about the vertical axis.
// The billboard goes from below the camera to the cloud base. The streaks are value noise from a baked lattice.
// The CPU sends only visible cells, with the near and far fade in the opacity.

struct Shafts {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: farthest billboard distance in meters.
    camera: vec4<f32>,
    // rgb: fog color. a: fog density.
    color: vec4<f32>,
    // x: top above the camera. y: bottom below the camera. z: half width share of the radius. w: streak offset 0..1.
    shape: vec4<f32>,
    // xy: center x, z. z: radius. w: opacity, more than 0.
    cells: array<vec4<f32>, 8>,
    // Streak seed of each shaft, 4 per vector: its sim cell slot.
    seeds: array<vec4<f32>, 2>,
};
@group(0) @binding(0) var<uniform> shafts: Shafts;
// One lattice hash per texel. It wraps in y at PERIOD rows.
@group(0) @binding(1) var lattice: texture_2d<f32>;
@group(0) @binding(2) var lattice_sampler: sampler;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    // x: -1..1 across the shaft. y: 0 bottom..1 top.
    @location(0) uv: vec2<f32>,
    // x: opacity. y: fog factor. z: noise seed.
    @location(1) shade: vec3<f32>,
};

// Vertical noise cells in one shaft height. The streak offset wraps at 1 without a seam.
const PERIOD: f32 = 6.0;
// Streak columns across the shaft.
const COLUMNS: f32 = 48.0;
// Shaft color as a share of the fog color.
const DARK: f32 = 0.55;

// Value noise with smoothstep weights: one bilinear sample at the smoothed lattice position.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let t = f * f * (3.0 - 2.0 * f);
    let size = vec2<f32>(textureDimensions(lattice));
    return textureSampleLevel(lattice, lattice_sampler, (i + t + 0.5) / size, 0.0).r;
}

@vertex
fn vertex(@builtin(vertex_index) corner: u32, @builtin(instance_index) id: u32) -> Varyings {
    var out: Varyings;
    let cell = shafts.cells[id];
    let camera = shafts.camera.xyz;
    let offset = cell.xy - camera.xz;
    let dist = length(offset);
    let radius = cell.z;
    // Two triangles: corners 0 1 2 and 3 4 5 of the quad.
    let x = select(-1.0, 1.0, corner == 1u || corner == 2u || corner == 4u);
    let y = select(0.0, 1.0, corner == 2u || corner == 4u || corner == 5u);
    out.uv = vec2(x, y);
    let side = vec2(-offset.y, offset.x) / dist;
    let across = cell.xy + side * (x * shafts.shape.z * radius);
    let height = mix(camera.y - shafts.shape.y, camera.y + shafts.shape.x, y);
    var world = vec3(across.x, height, across.y);
    // Past the far plane: move nearer at the same angular size.
    world = camera + (world - camera) * min(1.0, shafts.camera.w / dist);
    // FOG_EXP2 from the front of the shaft, like the source fog.
    let depth = max(dist - shafts.shape.z * radius, 0.0) * shafts.color.a * 1.2011224;
    out.shade = vec3(cell.w, clamp(exp2(-depth * depth), 0.0, 1.0), shafts.seeds[id / 4u][id % 4u] * 17.0);
    out.clip = shafts.clip_from_world * vec4(world, 1.0);
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    // Thickness of a soft cylinder.
    let thick = 1.0 - in.uv.x * in.uv.x;
    let ends = smoothstep(0.0, 0.15, in.uv.y) * (1.0 - smoothstep(0.7, 1.0, in.uv.y));
    let column = (in.uv.x * 0.5 + 0.5) * COLUMNS + in.shade.z;
    let fine = noise(vec2(column, (in.uv.y + shafts.shape.w) * PERIOD));
    let broad = noise(vec2(column * 0.125 + 31.0, in.uv.y * PERIOD));
    let streak = mix(0.35, 1.0, fine) * mix(0.5, 1.0, broad);
    let fog = in.shade.y;
    let color = mix(shafts.color.rgb, shafts.color.rgb * DARK, fog);
    let alpha = in.shade.x * thick * ends * streak * mix(0.5, 1.0, fog);
    if alpha < 1.0 / 255.0 {
        discard;
    }
    return vec4(color, alpha);
}
