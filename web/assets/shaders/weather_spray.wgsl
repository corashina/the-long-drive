// Wheel spray. The CPU writes the wheel emitters into one ring slot at a fixed rate.
// One instance is one particle of one emitter in one slot. Its motion comes from the slot age,
// so particles keep no state.

struct Spray {
    clip_from_world: mat4x4<f32>,
    // xyz: camera position. w: clock in seconds.
    camera: vec4<f32>,
    // rgb: spray color. a: opacity at full amount.
    color: vec4<f32>,
    // x: particles per emitter. y: life in seconds. z: slot seconds. w: clock period in seconds.
    controls: vec4<f32>,
    // The clock at the write of each slot. Empty slots are far in the past.
    births: array<vec4<f32>, 4>,
};
struct Emitter {
    // xyz: wheel ground contact. w: amount 0..1.
    position: vec4<f32>,
    // xyz: car velocity. w: wheel radius.
    velocity: vec4<f32>,
};

const SLOTS: u32 = 16u;
const EMITTERS: u32 = 32u;
// Air drag rate, 1/s.
const DRAG: f32 = 2.5;
// Fall acceleration of the mist, m/s^2.
const FALL: f32 = 3.0;
// Largest sprite half size per meter of distance. It limits the screen size near the camera.
const NEAR_SIZE: f32 = 0.25;
// Alpha below this value adds less than one 8-bit step.
const MIN_ALPHA: f32 = 1.0 / 255.0;

@group(0) @binding(0) var<uniform> spray: Spray;
@group(0) @binding(1) var<uniform> emitters: array<Emitter, 512>;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    // -1..1 on the sprite.
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
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
    // A collapsed triangle draws no pixels.
    out.clip = vec4(0.0, 0.0, 0.0, 1.0);
    out.uv = vec2(0.0);
    out.alpha = 0.0;
    let per = u32(spray.controls.x);
    let slot = id / (per * EMITTERS);
    let index = id / per;
    let emitter = emitters[index];
    var slot_age = spray.camera.w - spray.births[slot / 4u][slot % 4u];
    if slot_age < 0.0 {
        slot_age += spray.controls.w;
    }
    // The seed changes with each write of the slot.
    let seed = pcg(id ^ bitcast<u32>(spray.births[slot / 4u][slot % 4u])) * 8u;
    // Emission spreads over the slot time. The amount sets the share of live particles.
    let delay = unit(seed) * spray.controls.z;
    let age = slot_age - delay;
    let life = spray.controls.y * (0.6 + 0.4 * unit(seed + 1u));
    if unit(seed + 2u) >= emitter.position.w || age <= 0.0 || age >= life {
        return out;
    }
    let car = emitter.velocity.xyz;
    let speed = length(car);
    let back = -car / max(speed, 1e-3);
    let radius = emitter.velocity.w;
    let start = emitter.position.xyz + car * delay + back * (0.7 * radius) + vec3(0.0, 0.3 * radius, 0.0);
    let side = vec3(unit(seed + 3u) - 0.5, 0.0, unit(seed + 4u) - 0.5) * 2.4;
    let rise = vec3(0.0, 0.8 + 2.2 * unit(seed + 5u), 0.0);
    let thrown = car * 0.3 + back * (0.08 * speed) + side + rise;
    let drift = (1.0 - exp(-DRAG * age)) / DRAG;
    let center = start + thrown * drift - vec3(0.0, 0.5 * FALL * age * age, 0.0);
    let t = age / life;
    let offset = center - spray.camera.xyz;
    let dist = length(offset);
    let fade = (1.0 - t) * (1.0 - t) * smoothstep(0.0, 0.05, age) * smoothstep(0.5, 2.0, dist);
    let alpha = spray.color.a * emitter.position.w * fade;
    if alpha < MIN_ALPHA {
        return out;
    }
    // Two triangles: corners 0 1 2 and 0 2 3 of the quad.
    let x = select(-1.0, 1.0, corner == 1u || corner == 2u || corner == 4u);
    let y = select(-1.0, 1.0, corner == 2u || corner == 4u || corner == 5u);
    let view = offset / max(dist, 1e-4);
    let level = cross(view, vec3(0.0, 1.0, 0.0));
    let right = select(normalize(level), vec3(1.0, 0.0, 0.0), abs(view.y) > 0.999);
    let up = cross(right, view);
    let size = min(mix(0.15, 0.8, t) * (0.7 + 0.6 * unit(seed + 6u)), NEAR_SIZE * dist);
    let world = center + (right * x + up * y) * size;
    out.clip = spray.clip_from_world * vec4(world, 1.0);
    out.uv = vec2(x, y);
    out.alpha = alpha;
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    let alpha = in.alpha * (1.0 - smoothstep(0.2, 1.0, length(in.uv)));
    // The quad corners and the faint edge write nothing. Skip their blend.
    if alpha < MIN_ALPHA {
        discard;
    }
    return vec4(spray.color.rgb, alpha);
}
