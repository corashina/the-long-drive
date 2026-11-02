struct GrassFrame {origin_time:vec4<f32>,area:vec4<f32>,clip_from_world:mat4x4<f32>}
@group(0) @binding(0) var<uniform> frame:GrassFrame;
@group(1) @binding(0) var<uniform> world_from_local:mat4x4<f32>;
@vertex fn vertex(@location(0) position:vec3<f32>)->@builtin(position) vec4<f32>{return frame.clip_from_world*world_from_local*vec4(position,1.);}
@fragment fn fragment()->@location(0) vec4<f32>{return vec4(1.,0.,0.,1.);}