// Distant rain shafts. One instance is one wet cell: a soft billboard that turns about the vertical axis.
// The billboard goes from below the camera to the cloud base. The streaks are noise made here.

struct Shafts {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: farthest billboard distance in meters.
    camera: vec4<f32>,
    // rgb: fog color. a: fog density.
    color: vec4<f32>,
    // x: top above the camera. y: bottom below the camera. z: half width share of the radius. w: streak offset 0..1.
    shape: vec4<f32>,
    // xy: near fade start and end as radius shares. zw: far fade start and end in meters.
    fade: vec4<f32>,
    // xy: center x, z. z: radius. w: opacity.
    cells: array<vec4<f32>, 8>,
};
@group(0) @binding(0) var<uniform> shafts: Shafts;

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

fn hash(p: vec2<f32>) -> f32 {
    let h = vec2<u32>(vec2<i32>(p));
    var n = h.x * 1597334677u ^ h.y * 3812015801u;
    n = n * 747796405u + 2891336453u;
    n = ((n >> ((n >> 28u) + 4u)) ^ n) * 277803737u;
    return f32((n >> 22u) ^ n) / 4294967295.0;
}

// Value noise. The y lattice wraps at PERIOD.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let t = f * f * (3.0 - 2.0 * f);
    let y0 = i.y - floor(i.y / PERIOD) * PERIOD;
    let y1 = select(y0 + 1.0, 0.0, y0 + 1.0 >= PERIOD);
    let a = hash(vec2(i.x, y0));
    let b = hash(vec2(i.x + 1.0, y0));
    let c = hash(vec2(i.x, y1));
    let d = hash(vec2(i.x + 1.0, y1));
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
}

@vertex
fn vertex(@builtin(vertex_index) corner: u32, @builtin(instance_index) id: u32) -> Varyings {
    var out: Varyings;
    let cell = shafts.cells[id];
    let camera = shafts.camera.xyz;
    let offset = cell.xy - camera.xz;
    let dist = length(offset);
    let radius = cell.z;
    let near = smoothstep(shafts.fade.x * radius, shafts.fade.y * radius, dist);
    let far = 1.0 - smoothstep(shafts.fade.z, shafts.fade.w, dist);
    let opacity = cell.w * near * far;
    // Two triangles: corners 0 1 2 and 3 4 5 of the quad.
    let x = select(-1.0, 1.0, corner == 1u || corner == 2u || corner == 4u);
    let y = select(0.0, 1.0, corner == 2u || corner == 4u || corner == 5u);
    out.uv = vec2(x, y);
    if opacity <= 0.0 {
        // A collapsed triangle draws no pixels.
        out.clip = vec4(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let side = vec2(-offset.y, offset.x) / dist;
    let across = cell.xy + side * (x * shafts.shape.z * radius);
    let height = mix(camera.y - shafts.shape.y, camera.y + shafts.shape.x, y);
    var world = vec3(across.x, height, across.y);
    // Past the far plane: move nearer at the same angular size.
    world = camera + (world - camera) * min(1.0, shafts.camera.w / dist);
    // FOG_EXP2 from the front of the shaft, like the source fog.
    let depth = max(dist - shafts.shape.z * radius, 0.0) * shafts.color.a * 1.2011224;
    out.shade = vec3(opacity, clamp(exp2(-depth * depth), 0.0, 1.0), f32(id) * 17.0);
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
    return vec4(color, alpha);
}
