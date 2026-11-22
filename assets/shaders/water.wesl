import bevy_pbr::render::{
    mesh_view_bindings::globals,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> wave_scale: f32;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var shroud_mask: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var shroud_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> shroud_bounds: vec4<f32>;

// The shroud over unsurveyed ground (src/world/shroud.rs); kept in step with
// terrain.wesl. How far it covers `p`: fully over unsurveyed cells, easing off over
// the cell between, its edge wobbled so the survey grid doesn't show.
fn shroud_at(p: vec2<f32>) -> f32 {
    let wobble = vec2<f32>(
        sin(p.y * 0.043 + 2.0 * sin(p.x * 0.019)),
        cos(p.x * 0.051 + 2.0 * sin(p.y * 0.017)),
    ) * 9.0;
    let uv = (p + wobble - shroud_bounds.xy) * shroud_bounds.zw;
    let seen = textureSampleLevel(shroud_mask, shroud_sampler, uv, 0.0).r;
    return 1.0 - smoothstep(0.2, 0.8, seen);
}

// Unsurveyed ground turns dark, cold and grey, its relief faintly showing.
fn veil(color: vec3<f32>, shroud: f32) -> vec3<f32> {
    let grey = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    return mix(color, grey * vec3<f32>(0.16, 0.18, 0.22), shroud);
}

fn ripple(p: vec2<f32>, direction: vec2<f32>, rate: f32, time: f32) -> vec2<f32> {
    return direction * cos(dot(p, direction) * rate + time);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let t = globals.time;
    let depth = in.color.r;
    let marsh = in.color.g;
    let current = in.color.ba;
    let p = in.world_position.xz;
    let eddies = vec2<f32>(
        sin(dot(p, vec2<f32>(0.062, 0.11)) + t * 0.32),
        sin(dot(p, vec2<f32>(0.13, -0.052)) - t * 0.27),
    );
    let drift = (p - current * (t * 1.4) + eddies * 0.71) * wave_scale;
    let pixels = max(length(dpdx(p)), length(dpdy(p)));
    let ripples = ripple(drift, vec2<f32>(0.91, 0.41), 2.4, t * 1.6) * 0.058
        + ripple(drift, vec2<f32>(-0.44, 0.90), 4.1, -t * 2.1) * 0.027
        + ripple(drift, vec2<f32>(0.21, 0.98), 7.5, t * 3.3) * 0.015;
    let slope = ripples * (1.0 - smoothstep(0.13, 0.65, pixels)) * mix(1.0, 0.38, marsh);
    pbr.N = normalize(vec3<f32>(slope.x, 1.0, slope.y));

    let deep = 1.0 - exp(-depth * 0.58);
    let shore = smoothstep(0.0, 0.34, depth);
    let foam = shore * (1.0 - smoothstep(0.16, 0.72, depth))
        * smoothstep(0.73, 1.0, sin(dot(drift, vec2<f32>(2.9, 0.7)) + t * 1.8))
        * length(current) * 0.24;
    let facing = max(dot(pbr.N, pbr.V), 0.0);
    let fresnel = pow(1.0 - facing, 4.0);
    let river = mix(vec3<f32>(0.12, 0.29, 0.32), vec3<f32>(0.026, 0.10, 0.15), deep);
    let color = mix(river, vec3<f32>(0.12, 0.17, 0.055), marsh * 0.86);
    let albedo = mix(color, vec3<f32>(0.52, 0.56, 0.52), foam);
    pbr.material.base_color = vec4<f32>(albedo, shore * (0.32 + deep * 0.55 + fresnel * 0.13));
    pbr.material.perceptual_roughness = 0.17 + marsh * 0.27 + foam * 0.14;
    // Keep the subsurface tint tied to the same exposure as reflected light.
    pbr.material.emissive = vec4<f32>(color * (720.0 + fresnel * 620.0), 1.0);

    var out: FragmentOutput;
    let lit = main_pass_post_lighting_processing(pbr, apply_pbr_lighting(pbr));
    out.color = vec4<f32>(veil(lit.rgb, shroud_at(p)), lit.a);
    return out;
}
