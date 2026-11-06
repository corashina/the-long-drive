// Rain water on the glass of the seated car. glass_water.rs fills the uniform.
// source_glass.wgsl bends the grab with it. source_surface.wgsl bends the normal of Standard transparent glass.
#define_import_path weather_glass

struct GlassWater {
    // xyz: camera. w: cabin radius. Only glass nearer than it gets water.
    cabin: vec4<f32>,
    // xyz: seated body, the origin of the pattern. w: clock in seconds, 0..64.
    anchor: vec4<f32>,
    // xyz: water flow in world axes, in meters. w: 0.
    flow: vec4<f32>,
    // x: amount, 0 is off. y: bead and ring share. z: run stretch. w: 0.
    drops: vec4<f32>,
}

// Water on one glass point.
struct Water {
    // Tilt of the water surface normal, in world axes.
    tilt: vec3<f32>,
    // Tilt in the glass plane: x side, y up.
    slope: vec2<f32>,
    // Bead and run cover, 0..1.
    cover: f32,
}

// Drop cell side in meters.
const DROP_CELL: f32 = 0.05;
// Ring cell side in meters.
const RING_CELL: f32 = 0.07;
// Ring starts per second in each cell. 64 s holds a whole number of rings.
const RING_RATE: f32 = 1.25;
// Ring line half width, in ring cells.
const RING_WIDTH: f32 = 0.06;
const TAU: f32 = 6.2831853;

fn pcg(n: u32) -> u32 {
    let state = n * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn unit(n: u32) -> f32 {
    return f32(pcg(n) >> 8u) / 16777216.0;
}
fn cell_hash(cell: vec2<f32>, layer: u32) -> u32 {
    return pcg((bitcast<u32>(i32(cell.x)) * 1597334677u) ^ (bitcast<u32>(i32(cell.y)) * 3812015801u) ^ layer) * 4u;
}

// One drop at most in each cell. xy: slope. z: cover 0..1.
fn drop_layer(p: vec2<f32>, layer: u32, density: f32, stretch: f32) -> vec3<f32> {
    let g = p / DROP_CELL;
    let cell = floor(g);
    let n = cell_hash(cell, layer);
    if unit(n) >= density {
        return vec3(0.0);
    }
    let center = vec2(0.3) + 0.4 * vec2(unit(n + 1u), unit(n + 2u));
    let radius = mix(0.1, 0.25, unit(n + 3u));
    let d = (g - cell - center) / vec2(radius, radius * stretch);
    let cover = 1.0 - dot(d, d);
    if cover <= 0.0 {
        return vec3(0.0);
    }
    return vec3(d * cover, smoothstep(0.0, 0.3, cover));
}

// One ring at most in each cell. It grows from an impact and fades. Returns the slope.
fn ring_layer(p: vec2<f32>, clock: f32, density: f32) -> vec2<f32> {
    let g = p / RING_CELL;
    let cell = floor(g);
    let n = cell_hash(cell, 3u);
    if unit(n) >= density {
        return vec2(0.0);
    }
    let age = fract(clock * RING_RATE + unit(n + 3u));
    let center = vec2(0.25) + 0.5 * vec2(unit(n + 1u), unit(n + 2u));
    let offset = g - cell - center;
    let distance = length(offset);
    let x = (distance - 0.4 * age) / RING_WIDTH;
    if abs(x) >= 1.0 || distance < 1e-4 {
        return vec2(0.0);
    }
    // Height (1 - x^2)^2 across the ring line. Its slope goes along the radius.
    let fade = (1.0 - age) * (1.0 - age);
    return offset / distance * (-4.0 * x * (1.0 - x * x)) * fade;
}

// A thin water sheet. Three waves move with the flow and wobble with the clock.
fn sheet(p: vec2<f32>, clock: f32) -> vec2<f32> {
    // Wave vectors in radians per meter. Phase speeds are whole turns in 64 s.
    let a = vec2(97.0, 41.0);
    let b = vec2(-53.0, 113.0);
    let c = vec2(149.0, -71.0);
    let w = TAU / 64.0;
    return a * (0.5 * cos(dot(p, a) + clock * 19.0 * w)) / length(a)
        + b * (0.35 * cos(dot(p, b) + clock * 27.0 * w)) / length(b)
        + c * (0.25 * cos(dot(p, c) + clock * 41.0 * w)) / length(c);
}

// Water at a world point of glass with the face normal `normal`.
fn glass_water(water: GlassWater, world: vec3<f32>, normal: vec3<f32>) -> Water {
    var out = Water(vec3(0.0), vec2(0.0), 0.0);
    let amount = water.drops.x;
    if amount <= 0.0 || distance(world, water.cabin.xyz) > water.cabin.w {
        return out;
    }
    // The glass plane axes. Up is the world up on the glass.
    let level = vec3(0.0, 1.0, 0.0) - normal * normal.y;
    let size = length(level);
    let up = select(vec3(0.0, 0.0, 1.0), level / size, size > 1e-3);
    let side = cross(normal, up);
    let rel = world - water.anchor.xyz;
    let p = vec2(dot(rel, side), dot(rel, up));
    let moved = p - vec2(dot(water.flow.xyz, side), dot(water.flow.xyz, up));
    let clock = water.anchor.w;
    let beads = drop_layer(p, 1u, 0.45 * amount * water.drops.y, 1.0);
    let runs = drop_layer(moved, 2u, 0.3 * amount, water.drops.z);
    let drops = min(beads + runs, vec3(1.0));
    out.cover = drops.z;
    out.slope = drops.xy
        + 0.35 * amount * sheet(moved, clock)
        + 0.6 * ring_layer(p, clock, 0.5 * amount * water.drops.y);
    out.tilt = side * out.slope.x + up * out.slope.y;
    return out;
}
