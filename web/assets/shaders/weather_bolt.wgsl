// Lightning bolt. One strip of quads from the cloud to the strike point.
// The seed gives a random walk across the line, so the strip is jagged.

struct Bolt {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: farthest draw distance.
    camera: vec4<f32>,
    // xyz: top point. w: walk step in meters.
    start: vec4<f32>,
    // xyz: strike point. w: seed.
    end: vec4<f32>,
    // rgb: emissive color.
    color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> bolt: Bolt;

const SEGMENTS: u32 = 32u;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    // -1..1 across the strip.
    @location(0) across: f32,
};

fn pcg(n: u32) -> u32 {
    let state = n * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn unit(n: u32) -> f32 {
    return f32(pcg(n) >> 8u) / 16777216.0;
}

// Random walk across the line after `node` steps.
fn walk(seed: u32, node: u32) -> vec2<f32> {
    var sum = vec2(0.0);
    for (var i = 1u; i <= node; i++) {
        sum += vec2(unit(seed + i * 2u), unit(seed + i * 2u + 1u)) - vec2(0.5);
    }
    return sum;
}

// Node position. The walk drift is removed, so both ends stay on the line.
fn node_position(seed: u32, node: u32, last: vec2<f32>, a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let t = f32(node) / f32(SEGMENTS);
    let o = (walk(seed, node) - last * t) * bolt.start.w;
    return mix(bolt.start.xyz, bolt.end.xyz, t) + a * o.x + b * o.y;
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Varyings {
    var out: Varyings;
    let seed = pcg(u32(bolt.end.w));
    let segment = index / 6u;
    let corner = index % 6u;
    // Two triangles: corners 0 1 2 and 0 2 3 of the quad.
    let x = select(-1.0, 1.0, corner == 1u || corner == 2u || corner == 4u);
    let far_end = corner == 2u || corner == 4u || corner == 5u;
    let node = segment + select(0u, 1u, far_end);
    let axis = normalize(bolt.end.xyz - bolt.start.xyz);
    let a = normalize(cross(axis, vec3(0.0, 0.0, 1.0)));
    let b = cross(axis, a);
    let last = walk(seed, SEGMENTS);
    let center = node_position(seed, node, last, a, b);
    let before = node_position(seed, max(node, 1u) - 1u, last, a, b);
    let after = node_position(seed, min(node + 1u, SEGMENTS), last, a, b);
    let offset = center - bolt.camera.xyz;
    let dist = length(offset);
    let across = cross(after - before, offset);
    let side = across / max(length(across), 1e-4);
    // About two pixels wide at every distance, but not thinner than 1.5 m.
    let width = max(0.003 * dist, 1.5);
    // Points past the far plane move toward the camera on the same ray. The screen position stays.
    let scale = min(1.0, bolt.camera.w / max(dist, 1e-4));
    let world = bolt.camera.xyz + (offset + side * (x * width)) * scale;
    out.clip = bolt.clip_from_world * vec4(world, 1.0);
    out.across = x;
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    let core = 1.0 - abs(in.across);
    return vec4(bolt.color.rgb * core * core, 1.0);
}
