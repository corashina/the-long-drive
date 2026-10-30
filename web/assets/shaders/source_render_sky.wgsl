// Installed Unity 2021.3.18f1 Skybox/Procedural, globalgamemanagers.assets:4.
// Vertex variants 8 (HQ) / 4 (NONE), fragment variants 17 / 13.
// Atmospheric colors are evaluated per vertex on the recovered octahedron.
struct SkyParams {
    world_from_source: mat4x4<f32>,
    clip_from_world: mat4x4<f32>,
    // Direction remains in source world axes for atmospheric equations.
    light_direction: vec4<f32>,
    light_radiance: vec4<f32>,
    tint_thickness: vec4<f32>,
    ground_exposure: vec4<f32>,
    sun: vec4<f32>,
}
@group(0) @binding(0) var<uniform> sky: SkyParams;
struct SkyOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ray: vec3<f32>,
    @location(1) ground: vec3<f32>,
    @location(2) atmosphere: vec3<f32>,
    @location(3) sun: vec3<f32>,
    @location(4) ground_factor: f32,
}
fn scale_exp(cosine: f32) -> f32 {
    let x = 1.0 - cosine;
    let a = fma(x, 5.25, -6.800000190734863);
    let b = fma(x, a, 3.8299999237060547);
    let c = fma(x, b, 0.45899999141693115);
    let d = fma(x, c, -0.0028699999675154686);
    return exp2(d * 1.4426950216293335);
}
fn attenuation(scatter: f32, extinction: vec3<f32>) -> vec3<f32> {
    let bounded = min(max(scatter, 0.0), 50.0);
    return exp2((-bounded * extinction) * 1.4426950216293335);
}
fn sky_vertex(position: vec3<f32>, sun_enabled: bool) -> SkyOutput {
    var result: SkyOutput;
    let local = sky.world_from_source[1] * position.y;
    let local_x = fma(sky.world_from_source[0], vec4(position.x), local);
    let local_z = fma(sky.world_from_source[2], vec4(position.z), local_x);
    let world = local_z + sky.world_from_source[3];
    let clip_y = sky.clip_from_world[1] * world.y;
    let clip_x = fma(sky.clip_from_world[0], vec4(world.x), clip_y);
    let clip_z = fma(sky.clip_from_world[2], vec4(world.z), clip_x);
    result.position = fma(sky.clip_from_world[3], vec4(world.w), clip_z);
    // Uniform positive object scale cancels in the source normalized direction.
    // Preserve its multiply/normalize sequence instead of using normalized input.
    let source_world = vec3(local_z.x, local_z.y, -local_z.z);
    let eye = source_world * inverseSqrt(dot(source_world, source_world));
    let tint = exp2(log2(sky.tint_thickness.xyz) * 0.4545454680919647);
    let wavelength = fma(vec3(1.0) - tint,
        vec3(0.30000001192092896, 0.30000004172325134, 0.30000001192092896),
        vec3(0.5, 0.41999998688697815, 0.32499998807907104));
    let wavelength2 = wavelength * wavelength;
    let inverse_wavelength = vec3(1.0) / (wavelength2 * wavelength2);
    let thickness = exp2(log2(sky.tint_thickness.w) * 2.5);
    let rayleigh_sun = thickness * 0.04999999701976776;
    let rayleigh_pi = thickness * 0.03141592815518379;
    let extinction = fma(inverse_wavelength, vec3(rayleigh_pi), vec3(0.012566370889544487));
    var incoming: vec3<f32>;
    var outgoing: vec3<f32>;
    if eye.y >= 0.0 {
        let far = fma(-source_world.y, inverseSqrt(dot(source_world,source_world)),
            sqrt(fma(eye.y, eye.y, 0.050624966621398926)));
        let start = scale_exp(eye.y) * 0.24603183567523956;
        let sample_length = far * 0.5;
        let scaled_length = far * 20.0;
        let sample_ray = sample_length * eye;
        var point = fma(sample_ray, vec3(0.5), vec3(0.0, 1.000100016593933, 0.0));
        let height0 = sqrt(dot(point,point));
        let density0 = exp2((1.0-height0) * 230.83120727539062);
        let light0 = dot(sky.light_direction.xyz, point) / height0;
        let view0 = dot(eye, point) / height0;
        let delta0 = fma(scale_exp(light0), 0.25, -(scale_exp(view0) * 0.25));
        let scatter0 = fma(density0, delta0, start);
        let color0 = attenuation(scatter0, extinction);
        let weight0 = scaled_length * density0;
        point = fma(eye, vec3(sample_length), point);
        let height1 = sqrt(dot(point,point));
        let density1 = exp2((1.0-height1) * 230.83120727539062);
        let light1 = dot(sky.light_direction.xyz, point) / height1;
        let view1 = dot(eye, point) / height1;
        let delta1 = fma(scale_exp(light1), 0.25, -(scale_exp(view1) * 0.25));
        let scatter1 = fma(density1, delta1, start);
        let color1 = attenuation(scatter1, extinction);
        let weight1 = scaled_length * density1;
        let front = fma(color0, vec3(weight0), weight1 * color1);
        incoming = front * (inverse_wavelength * rayleigh_sun);
        outgoing = front * 0.019999999552965164;
    } else {
        let far = -9.999999747378752e-05 / min(eye.y, -0.0010000000474974513);
        let end = fma(vec3(far), eye, vec3(0.0, 1.000100016593933, 0.0));
        let camera_scale = scale_exp(dot(-eye, end));
        let light_scale = scale_exp(dot(sky.light_direction.xyz, end));
        let offset = camera_scale * 0.24990001320838928;
        let total_scale = fma(light_scale, 0.25, camera_scale * 0.25);
        let sample_ray = (far * 0.5) * eye;
        let point = fma(sample_ray, vec3(0.5), vec3(0.0, 1.000100016593933, 0.0));
        let density = exp2((1.0-sqrt(dot(point,point))) * 230.83120727539062);
        outgoing = attenuation(fma(density,total_scale,-offset), extinction);
        let front = ((far * 20.0) * density) * outgoing;
        incoming = fma(inverse_wavelength, vec3(rayleigh_sun), vec3(0.019999999552965164)) * front;
    }
    result.ground = fma(sky.ground_exposure.xyz,outgoing,incoming) * sky.ground_exposure.w;
    let cosine = dot(sky.light_direction.xyz,-eye);
    let phase = fma(cosine*cosine,0.75,0.75);
    result.atmosphere = (phase * incoming) * sky.ground_exposure.w;
    result.ray = -eye;
    result.ground_factor = eye.y * -50.0;
    result.sun = vec3(0.0);
    if sun_enabled {
        let intensity = min(max(sqrt(dot(sky.light_radiance.xyz,sky.light_radiance.xyz)),0.25),1.0);
        result.sun = ((clamp(outgoing,vec3(0.0),vec3(1.0)) * sky.light_radiance.xyz) * 15.0) / intensity;
    }
    return result;
}
@vertex fn vertex_main(@location(0) position: vec3<f32>) -> SkyOutput {
    return sky_vertex(position,true);
}
@vertex fn vertex_capture(@location(0) position: vec3<f32>) -> SkyOutput {
    return sky_vertex(position,false);
}
@fragment fn fragment_main(input: SkyOutput) -> @location(0) vec4<f32> {
    let size = exp2(log2(sky.sun.x) * 0.6499999761581421);
    let ray = input.ray * inverseSqrt(dot(input.ray,input.ray));
    let focused = exp2(log2(clamp(dot(sky.light_direction.xyz,-ray),0.0,1.0)) * sky.sun.y);
    let base = fma(-focused,1.9800000190734863,1.9801000356674194);
    let numerator = fma(focused,focused,1.0) * 0.010016442276537418;
    let denominator = max(exp2(log2(base) * (size * 10.0)),9.999999747378752e-05);
    let ground = clamp(ray.y * 50.0,0.0,1.0);
    let color = fma(vec3(ground),input.ground-input.atmosphere,input.atmosphere);
    if ray.y < 0.0 {return vec4(fma(input.sun,vec3(numerator/denominator),color),1.0);}
    return vec4(color,1.0);
}
@fragment fn fragment_capture(input: SkyOutput) -> @location(0) vec4<f32> {
    let ground = clamp(input.ground_factor,0.0,1.0);
    return vec4(fma(vec3(ground),input.ground-input.atmosphere,input.atmosphere),1.0);
}
