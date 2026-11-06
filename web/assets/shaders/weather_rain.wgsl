// Rain streaks. One instance is one drop. The instance id hashes to a fixed point in a box.
// The box wraps around the camera in world space, so drops stay put when the camera moves.

struct Rain {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: box side in meters.
    camera: vec4<f32>,
    // rgb: drop color. a: drop opacity.
    color: vec4<f32>,
    // x: fall offset in meters. y: shelter weight 0..1. z: shelter radius. w: streak length.
    controls: vec4<f32>,
};
@group(0) @binding(0) var<uniform> rain: Rain;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    // x: -1..1 across the streak. y: 0..1 along the streak.
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
};

fn pcg(n: u32) -> u32 {
    let state = n * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn unit(n: u32) -> f32 {
    return f32(pcg(n) >> 8u) / 16777216.0;
}

@vertex
fn vertex(@builtin(vertex_index) corner: u32, @builtin(instance_index) id: u32) -> Varyings {
    var out: Varyings;
    let size = rain.camera.w;
    let seed = id * 4u;
    let start = vec3(unit(seed), unit(seed + 1u), unit(seed + 2u)) * size;
    let low = rain.camera.xyz - vec3(0.5 * size);
    let fallen = start - vec3(0.0, rain.controls.x, 0.0) - low;
    let center = low + fallen - floor(fallen / size) * size;
    let offset = center - rain.camera.xyz;
    let dist = length(offset);
    // Fade at the box edge to hide the wrap. Fade near drops when sheltered.
    var fade = 1.0 - smoothstep(0.3 * size, 0.5 * size, dist);
    let radius = rain.controls.z;
    fade *= mix(1.0, smoothstep(radius, radius + 1.0, dist), rain.controls.y);
    // Two triangles: corners 0 1 2 and 0 2 3 of the quad.
    let x = select(-1.0, 1.0, corner == 1u || corner == 2u || corner == 4u);
    let y = select(0.0, 1.0, corner == 2u || corner == 4u || corner == 5u);
    out.uv = vec2(x, y);
    out.fade = fade;
    if fade <= 0.0 {
        // A collapsed triangle draws no pixels.
        out.clip = vec4(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let down = vec3(0.0, -1.0, 0.0);
    let across = cross(down, offset);
    let side = across / max(length(across), 1e-4);
    // About one pixel wide at every distance, but not thinner than 6 mm.
    let width = max(0.0015 * dist, 0.006);
    let streak = rain.controls.w * (0.6 + 0.8 * unit(seed + 3u));
    let world = center + side * (x * width) + down * (y * streak);
    out.clip = rain.clip_from_world * vec4(world, 1.0);
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    let across = 1.0 - abs(in.uv.x);
    let along = sin(3.14159265 * in.uv.y);
    return vec4(rain.color.rgb, rain.color.a * in.fade * across * along);
}
