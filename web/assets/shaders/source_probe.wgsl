// Copyright (c) 2016 Unity Technologies
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
// of the Software, and to permit persons to whom the Software is furnished to do
// so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
// Port of the native desktop CubeBlur frag_default. Preserve paired sums and fold order.
// Capture storage uses native face directions. No geometry Z reflection occurs in this filter.
#import source_float_store::half4
struct FilterParams {
    right: vec4<f32>,
    up: vec4<f32>,
    back: vec4<f32>,
    size: f32,
    texel: f32,
    level: f32,
    padding: f32,
};
@group(0) @binding(0) var<uniform> params: FilterParams;
@group(0) @binding(1) var source: texture_cube<f32>;
@group(0) @binding(2) var source_sampler: sampler;
@group(0) @binding(3) var capture_color: texture_2d<f32>;
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}
fn face_direction(position: vec4<f32>) -> vec3<f32> {
    let uv = position.xy / params.size;
    return params.right.xyz * (2.0*uv.x-1.0)
        + params.up.xyz * (2.0*uv.y-1.0) - params.back.xyz;
}
@fragment
fn copy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, source_sampler, face_direction(position), 0.0);
}
@fragment
fn capture(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(capture_color, vec2<i32>(position.xy), 0);
}
fn fold(st: vec3<f32>, face: vec3<f32>) -> vec3<f32> {
    let c = min(max(st, vec3<f32>(-1.0)), vec3<f32>(1.0));
    let f = abs(st-c);
    let m = max(max(f.x,f.y),f.z);
    return c-m*face;
}
fn gauss(d: f32) -> vec3<f32> {
    let v = vec3<f32>(d,3.0*d,5.0*d);
    return exp(-v*v);
}
fn sample_nonnegative(direction: vec3<f32>) -> vec4<f32> {
    // A single-level view selects source mip m-1. Its relative LOD is zero.
    return max(vec4<f32>(0.0),textureSampleLevel(source,source_sampler,direction,0.0));
}
@fragment
fn gaussian(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let direction = face_direction(position);

        var st: vec3<f32>;

        var face: vec3<f32> = select(vec3<f32>(0.0), direction, abs(direction)==vec3<f32>(1.0));
        var u: vec3<f32> = face.zxy*params.texel;
        var v: vec3<f32> = face.yzx*params.texel;
        var s: vec4<f32> = vec4<f32>(direction*(vec3<f32>(1.0) - abs(face)), 0.);

        // modulate coefficients based on position (texel density on projected sphere)
        var w: f32 = 1. / sqrt(1. + dot(s.xyz, s.xyz));
        var C: vec3<f32> = gauss(w*w*w);

        var s1: vec4<f32>; var s2: vec4<f32>; var s3: vec4<f32>;
        var c: vec3<f32>;

        var up1: vec3<f32> = fold(direction + 1.5*u, face);
        var um1: vec3<f32> = fold(direction - 1.5*u, face);
        var up2: vec3<f32> = fold(direction + 2.5*u, face);
        var um2: vec3<f32> = fold(direction - 2.5*u, face);

        var vp1: vec3<f32> = fold(direction + 1.5*v, face);
        var vm1: vec3<f32> = fold(direction - 1.5*v, face);
        var vp2: vec3<f32> = fold(direction + 2.5*v, face);
        var vm2: vec3<f32> = fold(direction - 2.5*v, face);

        s = vec4<f32>(0.);
        w = 0.;

        // first row

        c = C.xyz*C.zzz;

        st = direction - 2.5*u - 2.5*v;
        st = fold(st, face);
        s3 = sample_nonnegative(st);

        st = direction - 1.5*u - 2.5*v;
        st = fold(st, face);
        s2 = sample_nonnegative(st);

        st = vm2 - .5*u;
        s1 = sample_nonnegative(st);

        st = vm2 + .5*u;
        s1 += sample_nonnegative(st);

        st = direction + 1.5*u - 2.5*v;
        st = fold(st, face);
        s2 += sample_nonnegative(st);

        st = direction + 2.5*u - 2.5*v;
        st = fold(st, face);
        s3 += sample_nonnegative(st);

        w += dot(c, vec3<f32>(2.0));
        s1 = c.x*s1 + c.y*s2;
        s += c.z*s3;
        s += s1;

        // second row

        c = C.xyz*C.yyy;

        st = direction + 2.5*u - 1.5*v;
        st = fold(st, face);
        s3 = sample_nonnegative(st);

        st = direction + 1.5*u - 1.5*v;
        st = fold(st, face);
        s2 = sample_nonnegative(st);

        st = vm1 + .5*u;
        s1 = sample_nonnegative(st);

        st = vm1 - .5*u;
        s1 += sample_nonnegative(st);

        st = direction - 1.5*u - 1.5*v;
        st = fold(st, face);
        s2 += sample_nonnegative(st);

        st = direction - 2.5*u - 1.5*v;
        st = fold(st, face);
        s3 += sample_nonnegative(st);

        w += dot(c, vec3<f32>(2.0));
        s1 = c.x*s1 + c.y*s2;
        s += c.z*s3;
        s += s1;

        // third row

        c = C.xyz*C.xxx;

        st = um2 - .5*v;
        s3 = sample_nonnegative(st);

        st = um1 - .5*v;
        s2 = sample_nonnegative(st);

        st = direction - .5*u - .5*v;
        s1 = sample_nonnegative(st);

        st = direction + .5*u - .5*v;
        s1 += sample_nonnegative(st);

        st = up1 - .5*v;
        s2 += sample_nonnegative(st);

        st = up2 - .5*v;
        s3 += sample_nonnegative(st);

        w += dot(c, vec3<f32>(2.0));
        s1 = c.x*s1 + c.y*s2;
        s += c.z*s3;
        s += s1;

        // fourth row

        c = C.xyz*C.xxx;

        st = up2 + .5*v;
        s3 = sample_nonnegative(st);

        st = up1 + .5*v;
        s2 = sample_nonnegative(st);

        st = direction + .5*u + .5*v;
        s1 = sample_nonnegative(st);

        st = direction - .5*u + .5*v;
        s1 += sample_nonnegative(st);

        st = um1 + .5*v;
        s2 += sample_nonnegative(st);

        st = um2 + .5*v;
        s3 += sample_nonnegative(st);

        w += dot(c, vec3<f32>(2.0));
        s1 = c.x*s1 + c.y*s2;
        s += c.z*s3;
        s += s1;

        // fifth row

        c = C.xyz*C.yyy;

        st = direction - 2.5*u + 1.5*v;
        st = fold(st, face);
        s3 = sample_nonnegative(st);

        st = direction - 1.5*u + 1.5*v;
        st = fold(st, face);
        s2 = sample_nonnegative(st);

        st = vp1 - .5*u;
        s1 = sample_nonnegative(st);

        st = vp1 + .5*u;
        s1 += sample_nonnegative(st);

        st = direction + 1.5*u + 1.5*v;
        st = fold(st, face);
        s2 += sample_nonnegative(st);

        st = direction + 2.5*u + 1.5*v;
        st = fold(st, face);
        s3 += sample_nonnegative(st);

        w += dot(c, vec3<f32>(2.0));
        s1 = c.x*s1 + c.y*s2;
        s += c.z*s3;
        s += s1;

        // sixth row

        c = C.xyz*C.zzz;

        st = direction + 2.5*u + 2.5*v;
        st = fold(st, face);
        s3 = sample_nonnegative(st);

        st = direction + 1.5*u + 2.5*v;
        st = fold(st, face);
        s2 = sample_nonnegative(st);

        st = vp2 + .5*u;
        s1 = sample_nonnegative(st);

        st = vp2 - .5*u;
        s1 += sample_nonnegative(st);

        st = direction - 1.5*u + 2.5*v;
        st = fold(st, face);
        s2 += sample_nonnegative(st);

        st = direction - 2.5*u + 2.5*v;
        st = fold(st, face);
        s3 += sample_nonnegative(st);

        w += dot(c, vec3<f32>(2.0));
        s1 = c.x*s1 + c.y*s2;
        s += c.z*s3;
        s += s1;

        return half4(s/w);

}
@fragment
fn remap(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return half4(textureSampleLevel(source,source_sampler,face_direction(position),params.level));
}
