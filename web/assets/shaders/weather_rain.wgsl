// Rain streaks and snow flakes. One instance is one drop. The instance id hashes to a fixed point
// in a box and to rain or snow. The box wraps around the camera in world space, so drops stay put
// when the camera moves.

struct Rain {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: box side in meters.
    camera: vec4<f32>,
    // rgb: drop color. a: drop opacity.
    color: vec4<f32>,
    // x: shelter weight 0..1. y: shelter radius. z: streak length. w: flake opacity.
    controls: vec4<f32>,
    // xyz: drop offset in meters by fall and wind.
    offset: vec4<f32>,
    // xyz: unit streak axis along the drop velocity. w: flake share 0..1.
    axis: vec4<f32>,
    // xyz: flake offset in meters by fall and wind. w: flutter clock in seconds.
    flakes: vec4<f32>,
};
@group(0) @binding(0) var<uniform> rain: Rain;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    // x: -1..1 across the streak. y: 0..1 along the streak.
    @location(0) uv: vec2<f32>,
    @location(1) fade: f32,
    // 1 for a flake, 0 for a streak.
    @location(2) flake: f32,
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
    let seed = id * 8u;
    let flake = unit(seed + 4u) < rain.axis.w;
    let start = vec3(unit(seed), unit(seed + 1u), unit(seed + 2u)) * size;
    let low = rain.camera.xyz - vec3(0.5 * size);
    let fallen = start + select(rain.offset.xyz, rain.flakes.xyz, flake) - low;
    var center = low + fallen - floor(fallen / size) * size;
    if flake {
        // Flutter: a slow sideways sway with a phase and a rate per flake.
        let phase = 6.2831853 * unit(seed + 5u);
        let t = rain.flakes.w * (0.8 + 0.8 * unit(seed + 6u));
        center += vec3(sin(t + phase), 0.0, cos(1.3 * t + phase)) * 0.3;
    }
    let offset = center - rain.camera.xyz;
    let dist = length(offset);
    // Fade at the box edge to hide the wrap. Fade near drops when sheltered.
    var fade = 1.0 - smoothstep(0.3 * size, 0.5 * size, dist);
    let radius = rain.controls.y;
    fade *= mix(1.0, smoothstep(radius, radius + 1.0, dist), rain.controls.x);
    // Two triangles: corners 0 1 2 and 0 2 3 of the quad.
    let x = select(-1.0, 1.0, corner == 1u || corner == 2u || corner == 4u);
    let y = select(0.0, 1.0, corner == 2u || corner == 4u || corner == 5u);
    out.uv = vec2(x, y);
    out.fade = fade;
    out.flake = select(0.0, 1.0, flake);
    if fade <= 0.0 {
        // A collapsed triangle draws no pixels.
        out.clip = vec4(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    if flake {
        // A round sprite that faces the camera. Not smaller than about two pixels or 1 cm.
        let view = offset / max(dist, 1e-4);
        let level = cross(view, vec3(0.0, 1.0, 0.0));
        let right = select(normalize(level), vec3(1.0, 0.0, 0.0), abs(view.y) > 0.999);
        let up = cross(right, view);
        let radius = max(0.002 * dist, 0.01) * (0.7 + 0.6 * unit(seed + 3u));
        let world = center + (right * x + up * (2.0 * y - 1.0)) * radius;
        out.clip = rain.clip_from_world * vec4(world, 1.0);
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

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    if in.flake > 0.5 {
        let r = length(vec2(in.uv.x, 2.0 * in.uv.y - 1.0));
        let soft = 1.0 - smoothstep(0.3, 1.0, r);
        return vec4(min(rain.color.rgb * 1.6, vec3(1.0)), rain.controls.w * in.fade * soft);
    }
    let across = 1.0 - abs(in.uv.x);
    let along = sin(3.14159265 * in.uv.y);
    return vec4(rain.color.rgb, rain.color.a * in.fade * across * along);
}
