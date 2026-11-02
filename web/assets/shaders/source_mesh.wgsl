// Uses the Bevy 0.19.1 mesh vertex path and Unity Standard tangent frame.
#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    skinning,
    morph::{morph_position, morph_normal, morph_tangent},
    forward_io::Vertex,
    view_transformations::position_world_to_clip,
}

#import source_view_lighting::{lighting, probe_blend, fog_factor}
#ifdef SOURCE_LAMPS
#import "shaders/source_lighting.wgsl"::object_light_mask
#endif
#ifdef SOURCE_GPU_PROPS
#import source_props::{prop_row, prop_world_from_local, prop_normal_local_to_world, prop_tangent_local_to_world}
#endif

struct SourceVertexOutput {
    // This is `clip position` when the struct is used as a vertex stage output
    // and `frag coord` when used as a fragment stage input
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    // x=first probe weight, y=renderer flags, z=first probe slot, w=second probe slot or -1.
    @location(9) @interpolate(flat) source_renderer: vec4<f32>,
    @location(10) source_fog: f32,
#ifdef SOURCE_LAMPS
    @location(12) @interpolate(flat) source_lights: vec2<u32>,
#endif
#ifdef SOURCE_GPU_PROPS
    // The row LOD fade replaces the material `lod_fade`.
    @location(13) @interpolate(flat) source_fade: f32,
    // The row color replaces the material color.
    @location(14) @interpolate(flat) source_color: vec4<f32>,
#endif
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) world_tangent: vec4<f32>,
    @location(8) world_bitangent: vec3<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
}


#ifdef MORPH_TARGETS
// The instance_index parameter must match vertex_in.instance_index. This is a work around for a wgpu dx12 bug.
// See https://github.com/gfx-rs/naga/issues/2416
fn morph_vertex(vertex_in: Vertex, instance_index: u32) -> Vertex {
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;

    let weight_count = bevy_pbr::morph::layer_count(instance_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {
        let weight = bevy_pbr::morph::weight_at(i, instance_index);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph_position(vertex_index, i, instance_index);
#ifdef VERTEX_NORMALS
        vertex.normal += weight * morph_normal(vertex_index, i, instance_index);
#endif
#ifdef VERTEX_TANGENTS
        vertex.tangent += vec4(weight * morph_tangent(vertex_index, i, instance_index), 0.0);
#endif
    }
    return vertex;
}
#endif

@vertex
fn vertex(vertex_no_morph: Vertex, @location(8) bounds_center_usage: vec4<f32>, @location(9) bounds_extents_flags: vec4<f32>) -> SourceVertexOutput {
    var out: SourceVertexOutput;

#ifdef MORPH_TARGETS
    var vertex = morph_vertex(vertex_no_morph, vertex_no_morph.instance_index);
#else
    var vertex = vertex_no_morph;
#endif

#ifdef SOURCE_GPU_PROPS
    // A GPU prop row replaces the mesh uniform. GPU props are not skinned or morphed.
    let prop = prop_row(vertex_no_morph.instance_index);
    let mesh_world_from_local = prop_world_from_local(prop);
    out.source_fade = prop.fade;
    out.source_color = prop.color;
#else
    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);
#endif

#ifdef SKINNED
    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416 .
    var world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
#else
    var world_from_local = mesh_world_from_local;
#endif

#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else ifdef SOURCE_GPU_PROPS
    out.world_normal = prop_normal_local_to_world(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex_no_morph.instance_index
    );
#endif
#endif

#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef VERTEX_TANGENTS
#ifdef SOURCE_GPU_PROPS
    out.world_tangent = prop_tangent_local_to_world(world_from_local, vertex.tangent);
#else
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex_no_morph.instance_index
    );
#endif
    // Unity computes the signed bitangent at each vertex, before interpolation.
    out.world_bitangent = cross(out.world_normal, out.world_tangent.xyz) * out.world_tangent.w;
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416
    out.instance_index = vertex_no_morph.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);
#endif

#ifdef SOURCE_TRANSPARENT
    out.position = lighting.clip_from_world * out.world_position;
#endif
    let probes = probe_blend(world_from_local,bounds_center_usage,bounds_extents_flags);
    out.source_renderer = vec4(probes.x,bounds_extents_flags.w,probes.zw);
#ifdef SOURCE_LAMPS
    // Authored bounds do not prove the full extent of skinning or morph displacement.
    var bounded = true;
#ifdef SKINNED
    bounded = false;
#endif
#ifdef MORPH_TARGETS
    bounded = false;
#endif
    out.source_lights = object_light_mask(mesh_world_from_local, bounds_center_usage.xyz, bounds_extents_flags.xyz, bounded);
#endif
    out.source_fog = out.position.z;
#ifdef SOURCE_UNLIT
    // Installed Unlit/Color vertex6 evaluates FOG_EXP2 before interpolation.
    out.source_fog = fog_factor(out.position.z,lighting.near_far.x,lighting.near_far.y);
#endif
    return out;
}
