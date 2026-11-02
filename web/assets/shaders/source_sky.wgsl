// Unity 2021.3 built-in shader algebra. See UNITY-LICENSE.txt.
#import bevy_pbr::{mesh_functions, mesh_view_bindings::view}

struct SkyUniform {
    tint:vec4<f32>, ground:vec4<f32>, sun_direction:vec4<f32>, sun_color:vec4<f32>, parameters:vec4<f32>,
    clouds:array<vec4<f32>,2>, cloud_motion:vec4<f32>, star_color:vec4<f32>, star_motion:vec4<f32>, flares:array<vec4<f32>,3>,
}
struct Settings {kind:vec4<u32>,st:array<vec4<f32>,4>,color:vec4<f32>}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky:SkyUniform;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> settings:Settings;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var tex0:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var sampler0:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var tex1:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var sampler1:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var tex2:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var sampler2:sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var tex3:texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var sampler3:sampler;
struct Vertex {
    @builtin(instance_index) instance:u32,
    @location(0) position:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) uv:vec2<f32>, @location(3) color:vec4<f32>,
}
struct Varyings {
    @builtin(position) position:vec4<f32>, @location(0) ray:vec3<f32>, @location(1) ground:vec3<f32>,
    @location(2) sky_color:vec3<f32>, @location(3) sun:vec3<f32>, @location(4) uv:vec2<f32>, @location(5) color:vec4<f32>,
}
fn atmosphere_scale(cosine:f32)->f32 {
    let x=1.-cosine;
    return 0.25*exp(-0.00287+x*(0.459+x*(3.83+x*(-6.80+x*5.25))));
}
fn scatter_vertex(ray:vec3<f32>)->mat3x3<f32> {
    let tint=pow(max(sky.tint.rgb,vec3(0.)),vec3(1./2.2));
    let wavelength=mix(vec3(0.50,0.42,0.325),vec3(0.80,0.72,0.625),vec3(1.)-tint);
    let inv_wave=1./pow(wavelength,vec3(4.));
    let rayleigh=0.0025*pow(sky.parameters.y,2.5);
    let kr_sun=rayleigh*20.;let kr_pi=rayleigh*12.5663706;
    let extinction=inv_wave*kr_pi+vec3(0.0125663706);
    let camera=vec3(0.,1.0001,0.);
    var inside=vec3(0.);var outside=vec3(0.);
    if ray.y>=0. {
        let far=sqrt(1.025*1.025+ray.y*ray.y-1.)-ray.y;
        let depth=exp(-0.0001*160.);
        let offset=depth*atmosphere_scale(dot(ray,camera)/1.0001);
        let sample_length=far/2.;let scaled=sample_length*40.;let step=ray*sample_length;
        var point=camera+step*0.5;var front=vec3(0.);
        for(var i=0;i<2;i++) {
            let height=length(point);let sample_depth=exp(160.*(1.-height));
            let light_angle=dot(sky.sun_direction.xyz,point)/height;
            let camera_angle=dot(ray,point)/height;
            let scatter=offset+sample_depth*(atmosphere_scale(light_angle)-atmosphere_scale(camera_angle));
            let attenuation=exp(-clamp(scatter,0.,50.)*extinction);
            front+=attenuation*(sample_depth*scaled);point+=step;
        }
        inside=front*(inv_wave*kr_sun);outside=front*0.02;
    } else {
        let far=-0.0001/min(-0.001,ray.y);let pos=camera+far*ray;
        let depth=exp(-0.0001/0.25);
        let camera_scale=atmosphere_scale(dot(-ray,pos));let light_scale=atmosphere_scale(dot(sky.sun_direction.xyz,pos));
        let offset=depth*camera_scale;let temp=light_scale+camera_scale;
        let sample_length=far/2.;let point=camera+ray*sample_length*0.5;
        let height=length(point);let sample_depth=exp(160.*(1.-height));
        let scatter=sample_depth*temp-offset;let attenuation=exp(-clamp(scatter,0.,50.)*extinction);
        let front=attenuation*(sample_depth*sample_length*40.);
        inside=front*(inv_wave*kr_sun+vec3(0.02));outside=clamp(attenuation,vec3(0.),vec3(1.));
    }
    let eye_cos=dot(sky.sun_direction.xyz,-ray);
    let ground=sky.parameters.x*(inside+sky.ground.rgb*outside);
    let sky_color=sky.parameters.x*inside*(0.75+0.75*eye_cos*eye_cos);
    let sun_color=15.*clamp(outside,vec3(0.),vec3(1.))*sky.sun_color.rgb/clamp(length(sky.sun_color.rgb),0.25,1.);
    return mat3x3(ground,sky_color,sun_color);
}
@vertex fn vertex(v:Vertex)->Varyings {
    var out:Varyings;out.uv=v.uv;out.color=v.color;
    let world=mesh_functions::get_world_from_local(v.instance);
    var position=(world*vec4(v.position,1.)).xyz;
    if settings.kind.x==4u {
        let flare=sky.flares[settings.kind.y];let flags=u32(v.position.z);
        var h=-flare.z*v.position.y*0.01;
        if (flags&2u)!=0u {h*=flare.w;}
        var offset=v.normal.xy*h;
        if (flags&1u)!=0u {
            var direction=vec2(1.,0.);
            if length(flare.xy)>0.0001 {direction=normalize(flare.xy);}
            let half_vector=direction*(h*1.4);
            switch u32(v.normal.z) {
                case 0u: {offset=-half_vector;}
                case 1u: {offset=vec2(half_vector.y,-half_vector.x);}
                case 2u: {offset=half_vector;}
                default: {offset=vec2(-half_vector.y,half_vector.x);}
            }
        }
        out.position=view.clip_from_view*vec4(flare.xy*(1.-v.position.x)+offset,flare.z,1.);
        out.position.z=out.position.w;
        out.color*=settings.color*flare.w;
        if (flags&8u)!=0u {out.color=vec4(out.color.rgb*sky.sun_color.rgb,out.color.a);}
        if flare.z>=0. {out.color=vec4(0.);}
        return out;
    }
    if settings.kind.x==3u {
        let angle=sky.star_motion.x;let c=cos(angle);let s=sin(angle);
        let local=vec3(v.position.x,c*v.position.y-s*v.position.z,s*v.position.y+c*v.position.z);
        position=(world*vec4(local,1.)).xyz;
        var view_position=(view.view_from_world*vec4(position,0.)).xyz;
        view_position+=vec3((v.uv-vec2(0.5))*v.normal.x,0.);
        out.position=view.clip_from_view*vec4(view_position,1.);
        out.color*=settings.color*sky.star_color;
    } else {
        out.position=view.clip_from_view*(view.view_from_world*vec4(position,0.));
    }
    out.position.z=0.;
    if settings.kind.x==0u {
        let eye=normalize(position);let colors=scatter_vertex(eye);
        out.ray=-eye;out.ground=colors[0];out.sky_color=colors[1];out.sun=colors[2];
    }
    return out;
}
fn sun_phase(cosine:f32)->f32 {
    let temp=max(pow(1.+0.9801+1.98*cosine,pow(sky.parameters.z,0.65)*10.),0.0001);
    return 1.5*((1.-0.9801)/(2.+0.9801))*(1.+cosine*cosine)/temp;
}
fn texture_uv(uv:vec2<f32>,index:u32)->vec2<f32> {return uv*settings.st[index].xy+settings.st[index].zw;}
@fragment fn fragment(v:Varyings)->@location(0) vec4<f32> {
    if settings.kind.x==0u {
        let ray=normalize(v.ray);let y=ray.y/0.02;
        var color=mix(v.sky_color,v.ground,clamp(y,0.,1.));
        if y<0. {let focused=pow(clamp(dot(sky.sun_direction.xyz,-ray),0.,1.),sky.parameters.w);color+=v.sun*sun_phase(-focused);}
        return vec4(color,1.);
    }
    if settings.kind.x==1u {
        let i=settings.kind.y;let q=sky.cloud_motion[i+2u];
        let a=textureSample(tex0,sampler0,texture_uv(v.uv,0u)*2.+q*vec2(1.5,1.)).r;
        let b=textureSample(tex1,sampler1,texture_uv(v.uv,1u)*2.+q*vec2(1.,1.2)).r;
        let c=textureSample(tex2,sampler2,texture_uv(v.uv,2u)*vec2(10.,10.5)+q*vec2(0.75,0.5)).r;
        let d=textureSample(tex3,sampler3,texture_uv(v.uv,3u)*vec2(10.6,10.)+q*vec2(0.5,0.6)).r;
        let density=clamp(a+b+c+d-sky.cloud_motion[i],0.,1.);
        return vec4(sky.clouds[i].rgb,clamp(2.*density*sky.clouds[i].a,0.,1.));
    }
    let tex=textureSample(tex0,sampler0,v.uv);
    if settings.kind.x==3u {
        // HSV color mode replaces hue and saturation while retaining texture value.
        let value=max(tex.r,max(tex.g,tex.b));let maximum=max(v.color.r,max(v.color.g,v.color.b));
        let color=select(vec3(0.),v.color.rgb/max(maximum,0.000001)*value,maximum>0.);
        return vec4(color,tex.a*v.color.a);
    }
    return tex*v.color;
}
