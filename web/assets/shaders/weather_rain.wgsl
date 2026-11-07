// Rain streaks. One instance is one drop. The instance id hashes to a fixed point in a box.
// The box wraps around the camera in world space, so drops stay put when the camera moves.

struct Rain {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: box side in meters.
    camera: vec4<f32>,
    // rgb: drop color. a: drop opacity.
    color: vec4<f32>,
    // x: shelter weight 0..1. y: shelter radius. z: streak length.
    controls: vec4<f32>,
    // xyz: drop offset in meters by fall and wind.
    offset: vec4<f32>,
    // xyz: unit streak axis along the drop velocity.
    axis: vec4<f32>,
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
    let fallen = start + rain.offset.xyz - low;
    let center = low + fallen - floor(fallen / size) * size;
    let offset = center - rain.camera.xyz;
    let dist = length(offset);
    // Fade at the box edge to hide the wrap. Fade near drops when sheltered.
    var fade = 1.0 - smoothstep(0.3 * size, 0.5 * size, dist);
    let radius = rain.controls.y;
    fade *= mix(1.0, smoothstep(radius, radius + 1.0, dist), rain.controls.x);
    // Triangle strip of 4 corners: bit 0 is x, bit 1 is y.
    let x = select(-1.0, 1.0, (corner & 1u) != 0u);
    let y = select(0.0, 1.0, corner >= 2u);
    out.uv = vec2(x, y);
    out.fade = fade;
    if fade <= 0.0 {
        // A collapsed triangle draws no pixels.
        out.clip = vec4(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let down = rain.axis.xyz;
    let across = cross(down, offset);
    let side = across / max(length(across), 1e-4);
    // About one pixel wide at every distance, but not thinner than 6 mm.
    let width = max(0.0015 * dist, 0.006);
    let streak = rain.controls.z * (0.6 + 0.8 * unit(seed + 3u));
    let world = center + side * (x * width) + down * (y * streak);
    out.clip = rain.clip_from_world * vec4(world, 1.0);
    return out;
}

// Brightness added at the lit head of the streak, on top of 1.
const EDGE: f32 = 2.0;

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    let across = 1.0 - abs(in.uv.x);
    // The tail fades in. The head (y = 1, the fall end) is a thin bright edge with a sharp end.
    let y = in.uv.y;
    let along = y * (1.0 - smoothstep(0.9, 1.0, y));
    let edge = smoothstep(0.65, 0.92, y);
    let light = 1.0 + EDGE * edge;
    let alpha = min(rain.color.a * light, 1.0) * in.fade * across * along;
    // A fully faded pixel adds nothing. Skip its blend.
    if alpha < 1.0 / 255.0 {
        discard;
    }
    return vec4(rain.color.rgb * light, alpha);
}
