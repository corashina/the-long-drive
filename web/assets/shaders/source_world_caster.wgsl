struct WorldCasterView {
    clip_from_world:mat4x4<f32>, direction_to_light:vec4<f32>, bias:vec4<f32>,
    view_from_world:mat4x4<f32>,world_from_view:mat4x4<f32>,clip_from_view:mat4x4<f32>,
}
@group(1) @binding(0) var<uniform> caster_view:WorldCasterView;
#import source_caster_math::{caster_normal_bias,caster_clip_bias}
fn world_caster_clip(world:vec3<f32>,normal:vec3<f32>)->vec4<f32>{
    let displaced=caster_normal_bias(world,normal,normalize(caster_view.direction_to_light.xyz),caster_view.bias.z);
    return caster_clip_bias(caster_view.clip_from_world*vec4(displaced,1.),caster_view.bias.xy);
}
