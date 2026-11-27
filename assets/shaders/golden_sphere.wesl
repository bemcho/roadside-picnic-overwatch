// The Golden Sphere's mirror: the standard PBR gold, and in its place what the
// live probe saw (5 GB GPUs and up) or, on modest GPUs, where its reflection
// ray meets one of the bodies standing in for the stalker, the car or the
// carrier, that body lit by the Sun and the sky.

import bevy_pbr::render::{
    mesh_view_bindings::view,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct StandIn {
    // xyz: its centre, w: its turn about the vertical.
    centre_yaw: vec4<f32>,
    // xyz: its half size, w: 0 off, 1 a capsule, 2 a box.
    half_size: vec4<f32>,
    paint: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> stand_ins: array<StandIn, 3>;
// xyz: the way to the Sun, w: its illuminance in lux.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> sun: vec4<f32>;
// The probe's six faces (`reflection.rs`), sampled the way it renders them.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var face_px: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var face_nx: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var face_py: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var face_ny: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var face_pz: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var face_nz: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var face_sampler: sampler;
// x: 1 while the probe's faces are live.
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var<uniform> live: vec4<f32>;

const PI: f32 = 3.14159265;
// The sky's light on a body, as a share of the Sun's.
const SKY: f32 = 0.2;
const MISS: f32 = 1.0e9;

// What the probe saw along `d`: rgb, and alpha 0 where it saw only sky. The
// faces and their cameras' up follow `FACES` in `reflection.rs`; the twin of
// this function lives in the other of `golden_sphere.wesl`/`ferrofluid.wesl`.
fn probe_sample(d: vec3<f32>) -> vec4<f32> {
    let a = abs(d);
    var face = 0;
    var forward = vec3<f32>(1.0, 0.0, 0.0);
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if a.x >= a.y && a.x >= a.z {
        if d.x < 0.0 {
            face = 1;
            forward = vec3<f32>(-1.0, 0.0, 0.0);
        }
    } else if a.y >= a.z {
        if d.y > 0.0 {
            face = 2;
            forward = vec3<f32>(0.0, 1.0, 0.0);
            up = vec3<f32>(0.0, 0.0, -1.0);
        } else {
            face = 3;
            forward = vec3<f32>(0.0, -1.0, 0.0);
            up = vec3<f32>(0.0, 0.0, 1.0);
        }
    } else if d.z > 0.0 {
        face = 4;
        forward = vec3<f32>(0.0, 0.0, 1.0);
    } else {
        face = 5;
        forward = vec3<f32>(0.0, 0.0, -1.0);
    }
    let along = dot(d, forward);
    let uv = vec2<f32>(0.5 + 0.5 * dot(d, cross(forward, up)) / along, 0.5 - 0.5 * dot(d, up) / along);
    var seen = vec4<f32>(0.0);
    switch face {
        case 0: { seen = textureSampleLevel(face_px, face_sampler, uv, 0.0); }
        case 1: { seen = textureSampleLevel(face_nx, face_sampler, uv, 0.0); }
        case 2: { seen = textureSampleLevel(face_py, face_sampler, uv, 0.0); }
        case 3: { seen = textureSampleLevel(face_ny, face_sampler, uv, 0.0); }
        case 4: { seen = textureSampleLevel(face_pz, face_sampler, uv, 0.0); }
        default: { seen = textureSampleLevel(face_nz, face_sampler, uv, 0.0); }
    }
    return seen;
}

// `v` turned by `angle` about the vertical, as a Y rotation turns it.
fn turn(v: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(v.x * c + v.z * s, v.y, -v.x * s + v.z * c);
}

// The distance along the ray `ro + t rd` to a vertical capsule, and the
// normal there (t is MISS on a miss): its side, else its end caps.
fn hit_capsule(ro: vec3<f32>, rd: vec3<f32>, body: StandIn) -> vec4<f32> {
    let radius = body.half_size.x;
    let reach = max(body.half_size.y - radius, 0.0);
    let pa = body.centre_yaw.xyz - vec3<f32>(0.0, reach, 0.0);
    let ba = vec3<f32>(0.0, 2.0 * reach, 0.0);
    let oa = ro - pa;
    let baba = dot(ba, ba);
    let bard = dot(ba, rd);
    let baoa = dot(ba, oa);
    let a = baba - bard * bard;
    let b = baba * dot(rd, oa) - baoa * bard;
    let c = baba * dot(oa, oa) - baoa * baoa - radius * radius * baba;
    let h = b * b - a * c;
    var t = MISS;
    if h >= 0.0 && a > 1.0e-6 {
        let side = (-b - sqrt(h)) / a;
        let y = baoa + side * bard;
        if side > 0.0 && y > 0.0 && y < baba {
            t = side;
        }
    }
    if t == MISS {
        for (var end = 0; end < 2; end++) {
            let oc = oa - ba * f32(end);
            let half_b = dot(oc, rd);
            let cap = half_b * half_b - (dot(oc, oc) - radius * radius);
            if cap >= 0.0 {
                let at = -half_b - sqrt(cap);
                if at > 0.0 && at < t {
                    t = at;
                }
            }
        }
    }
    if t == MISS {
        return vec4<f32>(0.0, 0.0, 0.0, MISS);
    }
    let p = ro + rd * t;
    let axis = pa + ba * clamp(dot(p - pa, ba) / max(baba, 1.0e-6), 0.0, 1.0);
    return vec4<f32>(normalize(p - axis), t);
}

// The distance along the ray to a box turned about the vertical, and the
// normal there.
fn hit_box(ro: vec3<f32>, rd: vec3<f32>, body: StandIn) -> vec4<f32> {
    let yaw = body.centre_yaw.w;
    let o = turn(ro - body.centre_yaw.xyz, -yaw);
    let d = turn(rd, -yaw);
    let inv = 1.0 / select(d, vec3<f32>(1.0e-6), abs(d) < vec3<f32>(1.0e-6));
    let n = inv * o;
    let k = abs(inv) * body.half_size.xyz;
    let near = -n - k;
    let far = -n + k;
    let enter = max(max(near.x, near.y), near.z);
    let leave = min(min(far.x, far.y), far.z);
    if enter > leave || enter <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, MISS);
    }
    let face = -sign(d) * step(near.yzx, near) * step(near.zxy, near);
    return vec4<f32>(turn(face, yaw), enter);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var color = apply_pbr_lighting(pbr_input);

    let ro = in.world_position.xyz;
    let rd = reflect(-pbr_input.V, pbr_input.N);
    let gold = pbr_input.material.base_color.rgb;
    if live.x > 0.5 {
        let seen = probe_sample(rd);
        color = vec4<f32>(mix(color.rgb, seen.rgb * gold, seen.a), color.a);
    }
    var nearest = vec4<f32>(0.0, 0.0, 0.0, MISS);
    var paint = vec3<f32>(0.0);
    for (var i = 0; i < 3; i++) {
        let body = stand_ins[i];
        var hit = vec4<f32>(0.0, 0.0, 0.0, MISS);
        if body.half_size.w == 1.0 {
            hit = hit_capsule(ro, rd, body);
        } else if body.half_size.w == 2.0 {
            hit = hit_box(ro, rd, body);
        }
        if hit.w < nearest.w {
            nearest = hit;
            paint = body.paint.rgb;
        }
    }
    if nearest.w < MISS {
        let light = sun.w * (max(dot(nearest.xyz, sun.xyz), 0.0) + SKY);
        color = vec4<f32>(paint * light / PI * view.exposure * gold, color.a);
    }

    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
