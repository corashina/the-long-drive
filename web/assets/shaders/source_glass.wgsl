// Installed FX/Glass/Stained BumpDistort, selected VS4/PS12.
#import bevy_pbr::{forward_io::Vertex, mesh_functions, pbr_bindings}
#import source_float_store::half4

struct GlassViewUniform {
    clip_from_world: mat4x4<f32>,
    near_far: vec4<f32>,
    grab_texel_size: vec4<f32>,
    // Drops on the seated car glass, see glass_drops.rs. xyz: camera. w: cabin radius.
    cabin: vec4<f32>,
    // x: amount, 0 is off. y: scroll up the glass in meters. z: bead share. w: run stretch.
    drops: vec4<f32>,
}
@group(1) @binding(0) var<uniform> glass_view: GlassViewUniform;
@group(1) @binding(1) var grab_texture: texture_2d<f32>;
@group(1) @binding(2) var grab_sampler: sampler;

// Matches the existing SourceSurface material uniform. Glass uses scales.x for BumpAmt.
struct SourceUniform {
    color: vec4<f32>,
    emission: vec4<f32>,
    factors: vec4<f32>,
    scales: vec4<f32>,
    flags: vec4<u32>,
    banks: array<vec4<u32>, 3>,
    uv: array<vec4<f32>, 9>,
    sampling: array<vec4<f32>, 9>,
    defaults: array<vec4<f32>, 9>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(120) var<uniform> source: SourceUniform;

@group(#{MATERIAL_BIND_GROUP}) @binding(121) var bank6_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(122) var bank6_sampler: sampler;
fn sample_slot(slot: u32, input_uv: vec2<f32>) -> vec4<f32> {
    if (source.flags.y & (1u << slot)) == 0u { return source.defaults[slot]; }
    let uv = select(input_uv, abs(input_uv), source.sampling[slot].zw > vec2(0.5));
    let bias = source.sampling[slot].y;
    let bank = source.banks[slot / 4u][slot % 4u];
    switch bank {
        case 0u: { return textureSampleBias(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, bias); }
        case 1u: { return textureSampleBias(pbr_bindings::emissive_texture, pbr_bindings::emissive_sampler, uv, bias); }
        case 2u: { return textureSampleBias(pbr_bindings::metallic_roughness_texture, pbr_bindings::metallic_roughness_sampler, uv, bias); }
        case 3u: { return textureSampleBias(pbr_bindings::occlusion_texture, pbr_bindings::occlusion_sampler, uv, bias); }
        case 4u: { return textureSampleBias(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler, uv, bias); }
        case 5u: { return textureSampleBias(pbr_bindings::depth_map_texture, pbr_bindings::depth_map_sampler, uv, bias); }
        default: { return textureSampleBias(bank6_texture, bank6_sampler, uv, bias); }
    }
}
struct GlassVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) projected_grab: vec4<f32>,
    @location(1) bump_uv: vec2<f32>,
    @location(2) main_uv: vec2<f32>,
    @location(3) world: vec3<f32>,
    // World offset from the mesh origin. It moves with the car.
    @location(4) rel: vec3<f32>,
#ifdef VERTEX_NORMALS
    @location(5) normal: vec3<f32>,
#endif
}
@vertex
fn vertex(in: Vertex) -> GlassVertex {
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(in.position, 1.0));
    let raster_clip = glass_view.clip_from_world * world;
    // Source D3D11 already uses the same reverse-Z device projection.
    let source_clip = raster_clip;
    var out: GlassVertex;
    out.position = raster_clip;
    out.projected_grab = vec4((source_clip.x + source_clip.w) * 0.5,
        (-source_clip.y + source_clip.w) * 0.5, source_clip.zw);
    out.bump_uv = in.uv * source.uv[1].xy + source.uv[1].zw;
    out.main_uv = in.uv * source.uv[0].xy + source.uv[0].zw;
    out.world = world.xyz;
    out.rel = world.xyz - world_from_local[3].xyz;
#ifdef VERTEX_NORMALS
    out.normal = mesh_functions::mesh_normal_local_to_world(in.normal, in.instance_index);
#endif
    return out;
}

// Drop cell side in meters.
const DROP_CELL: f32 = 0.05;
// Grab offset at the drop edge, as a share of the screen.
const DROP_LENS: f32 = 0.012;

fn pcg(n: u32) -> u32 {
    let state = n * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}
fn unit(n: u32) -> f32 {
    return f32(pcg(n) >> 8u) / 16777216.0;
}

// One drop at most in each cell. xy: lens offset. z: cover 0..1.
fn drop_layer(p: vec2<f32>, layer: u32, density: f32, stretch: f32) -> vec3<f32> {
    let g = p / DROP_CELL;
    let cell = floor(g);
    let n = pcg((bitcast<u32>(i32(cell.x)) * 1597334677u) ^ (bitcast<u32>(i32(cell.y)) * 3812015801u) ^ layer) * 4u;
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

// Drops on glass near the camera. Beads stay on the glass. Runs move up by the scroll.
fn glass_drops(world: vec3<f32>, rel: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let drops = glass_view.drops;
    if drops.x <= 0.0 || distance(world, glass_view.cabin.xyz) > glass_view.cabin.w {
        return vec3(0.0);
    }
    let level = vec3(0.0, 1.0, 0.0) - normal * normal.y;
    let size = length(level);
    let up = select(vec3(0.0, 0.0, 1.0), level / size, size > 1e-3);
    let side = cross(normal, up);
    let p = vec2(dot(rel, side), dot(rel, up));
    let beads = drop_layer(p, 1u, 0.45 * drops.x * drops.z, 1.0);
    let runs = drop_layer(p - vec2(0.0, drops.y), 2u, 0.3 * drops.x, drops.w);
    return min(beads + runs, vec3(1.0));
}
@fragment
fn fragment(in: GlassVertex) -> @location(0) vec4<f32> {
    // Canonical normal images already contain decoded source AG channels in RGB.
    let bump = sample_slot(1u, in.bump_uv);
    let normal = bump.xy * 2.0 - vec2(1.0);
    let offset = normal * source.scales.x * glass_view.grab_texel_size.xy;
    let weight = max(0.0, (1.0 - in.projected_grab.z / glass_view.near_far.x) * glass_view.near_far.y);
#ifdef VERTEX_NORMALS
    let face = normalize(in.normal);
#else
    let face = normalize(cross(dpdx(in.world), dpdy(in.world)));
#endif
    let drop = glass_drops(in.world, in.rel, face);
    // Screen y goes down, the glass up goes up.
    let grab_uv = (in.projected_grab.xy + offset * weight) / in.projected_grab.w
        + vec2(drop.x, -drop.y) * DROP_LENS;
    let main = sample_slot(0u, in.main_uv);
    let grabbed = textureSample(grab_texture, grab_sampler, grab_uv);
    // All RGBA channels replace the destination. The pipeline disables blending.
    let color = grabbed * main * source.color;
    return half4(vec4(color.rgb * (1.0 - 0.15 * drop.z), color.a));
}
