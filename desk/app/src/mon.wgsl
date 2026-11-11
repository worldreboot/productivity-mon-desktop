// A baked model, skinned and lit by a sun and a sky. The skinning is world-render's (world.wgsl, `skinned`); the
// light is its sun and hemisphere without shadows, fog or anything a desktop has no use for.

struct Frame {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    // Toward the sun; and in w how far it is turned to white light, as a mon going into a ball is.
    sun: vec4<f32>,
    // x: how much of it is there, from none to all: a plate fades out. The rest is not used.
    more: vec4<f32>,
}

struct Material {
    colour: vec4<f32>,
    // The texture's repeat, then its offset.
    uv: vec4<f32>,
    // x: how see-through a texel must be to be cut. y: 1 for a material that is blended. z: 1 for one with no
    // light on it.
    flags: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> bones: array<mat4x4<f32>>;
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var map: texture_2d<f32>;
@group(1) @binding(2) var map_sampler: sampler;

struct In {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) joints: vec4<u32>,
    @location(4) weights: vec4<f32>,
}

struct Out {
    @builtin(position) at: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

@vertex fn vs(in: In) -> Out {
    let skin = bones[in.joints.x] * in.weights.x + bones[in.joints.y] * in.weights.y + bones[in.joints.z] * in.weights.z + bones[in.joints.w] * in.weights.w;
    let world = frame.model * skin;
    var out: Out;
    out.at = frame.view_proj * world * vec4(in.position, 1.0);
    out.normal = (world * vec4(in.normal, 0.0)).xyz;
    out.uv = in.uv * material.uv.xy + material.uv.zw;
    return out;
}

@fragment fn fs(in: Out, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // The pictures are kept top row first; three.js, which the uvs were made for, keeps them bottom row first.
    let texel = textureSample(map, map_sampler, vec2(in.uv.x, 1.0 - in.uv.y));
    if (texel.a < material.flags.x) {
        discard;
    }
    var normal = normalize(in.normal);
    if (!front) {
        normal = -normal;
    }
    let base = texel.rgb * material.colour.rgb;
    // Kept near one at its brightest: brighter than that and the desktop sees a paler mon than HOME's.
    let sky = mix(vec3(0.46, 0.49, 0.54), vec3(0.76, 0.79, 0.80), 0.5 + 0.5 * normal.y);
    let sun = vec3(1.0, 0.96, 0.9) * max(dot(normal, frame.sun.xyz), 0.0);
    // A picture (z of the flags) is shown as it is.
    let lit = mix(base * (sky + sun * 0.32), base, material.flags.z);
    let shown = mix(clamp(lit, vec3(0.0), vec3(1.0)), vec3(1.0), frame.sun.w);
    let alpha = mix(1.0, texel.a, material.flags.y) * frame.more.x;
    // Written as sRGB by hand, and multiplied by its alpha, which is how the desktop wants it.
    return vec4(pow(shown, vec3(1.0 / 2.2)) * alpha, alpha);
}
