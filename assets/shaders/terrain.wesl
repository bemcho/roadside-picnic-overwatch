import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};
@if(DISTANCE_FOG) import bevy_pbr::render::pbr_functions::apply_fog;
@if(DISTANCE_FOG) import bevy_pbr::render::mesh_view_bindings::{fog, view};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var grass_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var ground_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var grass_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var grass_surface: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var earth_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var earth_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var earth_surface: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var road_surface: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var<uniform> road_bounds: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var<uniform> fog_settings: vec4<f32>;

fn patch_hash(cell: vec2<f32>) -> f32 {
    let p = fract(vec3<f32>(cell.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    let q = p + dot(p, p.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn patch_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let t = fract(p);
    let w = t * t * (3.0 - 2.0 * t);
    return mix(
        mix(patch_hash(cell), patch_hash(cell + vec2<f32>(1.0, 0.0)), w.x),
        mix(patch_hash(cell + vec2<f32>(0.0, 1.0)), patch_hash(cell + 1.0), w.x),
        w.y,
    );
}

fn rotation(angle: f32) -> mat2x2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));
}

// Project onto all three axes so quarry walls do not stretch the ground textures.
fn sample_ground(
    tex: texture_2d<f32>, p: vec3<f32>, weights: vec3<f32>, axes: mat2x2<f32>,
) -> vec3<f32> {
    return textureSample(tex, ground_sampler, axes * p.zy).rgb * weights.x
        + textureSample(tex, ground_sampler, axes * p.xz).rgb * weights.y
        + textureSample(tex, ground_sampler, axes * p.xy).rgb * weights.z;
}

fn ground_gradient(tex: texture_2d<f32>, uv: vec2<f32>, axes: mat2x2<f32>) -> vec2<f32> {
    let n = textureSample(tex, ground_sampler, axes * uv).xyz * 2.0 - 1.0;
    return transpose(axes) * (n.xy / max(n.z, 0.3));
}

fn ground_normal(
    tex: texture_2d<f32>,
    p: vec3<f32>,
    weights: vec3<f32>,
    normal: vec3<f32>,
    axes: mat2x2<f32>,
) -> vec3<f32> {
    let nx = ground_gradient(tex, p.zy, axes);
    let ny = ground_gradient(tex, p.xz, axes);
    let nz = ground_gradient(tex, p.xy, axes);
    let gradient = vec3<f32>(0.0, nx.y, nx.x) * weights.x
        + vec3<f32>(ny.x, 0.0, ny.y) * weights.y
        + vec3<f32>(nz.x, nz.y, 0.0) * weights.z;
    return normalize(normal + (gradient - normal * dot(gradient, normal)) * 0.8);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let normal = normalize(in.world_normal);
    let axis_weights = pow(abs(normal), vec3<f32>(4.0));
    let weights = axis_weights / dot(axis_weights, vec3<f32>(1.0));
    let p = in.world_position.xyz;
    // Independent offsets break repeat rows; a second noise scale frays patch edges.
    let region = p.xz / 4.0;
    let warp = vec3<f32>(
        patch_noise(region + 28.0), patch_noise(region - 19.0), patch_noise(region + 6.0),
    ) - 0.5;
    let grass_axes = rotation(0.53);
    let earth_axes = rotation(-0.31);
    var grass_uv = (p + warp * 1.9) / 2.3;
    var earth_uv = (p - warp * 1.7) / 1.8;
    let grass_height = sample_ground(grass_surface, grass_uv, weights, grass_axes).b;
    let earth_height = sample_ground(earth_surface, earth_uv, weights, earth_axes).b;
    let biome = clamp(in.color.r, 0.0, 1.0);
    let patches = patch_noise(region) * 0.68 + patch_noise(p.xz / 1.25 + 32.0) * 0.32;
    let spread = patches + (earth_height - grass_height) * 0.16 + (biome - 0.5) * 0.34;
    let exposed = mix(biome, smoothstep(0.42, 0.58, spread), mix(0.74, 0.27, smoothstep(0.55, 1.0, biome)));

    // Offset along the surface so stones appear raised, without moving the collision mesh.
    let height = mix(grass_height, earth_height, exposed);
    let facing = dot(normal, pbr_input.V);
    let tangent_view = pbr_input.V - normal * facing;
    let pixel_size = max(length(dpdx(p)), length(dpdy(p)));
    let relief = (1.0 - smoothstep(0.025, 0.12, pixel_size)) * 0.07;
    let slide = tangent_view * (height - 0.5) * relief / max(facing, 0.32);
    grass_uv += slide / 2.3;
    earth_uv += slide / 1.8;

    let grass = sample_ground(grass_albedo, grass_uv, weights, grass_axes);
    let earth = sample_ground(earth_albedo, earth_uv, weights, earth_axes);
    let moss = grass * vec3<f32>(0.91, 1.65, 0.89);
    let stone = earth * vec3<f32>(1.32, 1.32, 1.48);
    let wetness = clamp(in.color.g, 0.0, 1.0);
    let ash = clamp(in.color.b, 0.0, 1.0);
    var albedo = mix(moss, stone, exposed);
    let luminance = dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
    albedo = mix(albedo, vec3<f32>(luminance) * vec3<f32>(0.72, 0.74, 0.70), ash * 0.9);
    let road_uv = clamp((p.xz - road_bounds.xy) * road_bounds.zw, vec2<f32>(0.0), vec2<f32>(1.0));
    let road = textureSampleLevel(road_surface, ground_sampler, road_uv, 0.0).rg;
    let gravel = (vec3<f32>(0.23, 0.184, 0.124) + earth * 0.48) * (1.0 - road.g * 0.34);
    albedo = mix(albedo, gravel, road.r);
    albedo *= mix(vec3<f32>(1.0), vec3<f32>(0.48, 0.56, 0.42), wetness);
    let alpine = smoothstep(32.0, 100.0, p.y);
    albedo = mix(albedo, vec3<f32>(0.62, 0.81, 1.06) * (luminance + 0.038), alpine * 0.74);
    let snowline = 112.0 + patch_noise(p.xz / 24.0) * 29.0 + (1.0 - normal.y) * 72.0;
    let snow = smoothstep(snowline - 12.0, snowline + 13.0, p.y);
    albedo = mix(albedo, vec3<f32>(0.65, 0.71, 0.75) * (0.76 + 0.24 * patches), snow);
    pbr_input.material.base_color = vec4<f32>(albedo * in.color.a, 1.0);

    // Packed maps: red AO, green roughness, blue height.
    let surface = mix(
        sample_ground(grass_surface, grass_uv, weights, grass_axes),
        sample_ground(earth_surface, earth_uv, weights, earth_axes),
        exposed,
    );
    pbr_input.material.perceptual_roughness = mix(0.34, 0.98, surface.g) * mix(1.0, 0.65, wetness);
    pbr_input.material.metallic = 0.0;
    pbr_input.diffuse_occlusion *= vec3<f32>(mix(1.0, surface.r, 0.6));
    pbr_input.N = normalize(mix(
        ground_normal(grass_normal, grass_uv, weights, normal, grass_axes),
        ground_normal(earth_normal, earth_uv, weights, normal, earth_axes),
        exposed,
    ));

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    @if(DISTANCE_FOG) {
        let fog_top = fog_settings.x;
        let fog_fade_bottom = fog_top - fog_settings.y * 0.7;
        let low_ground = 1.0 - smoothstep(fog_fade_bottom, fog_top, p.y);
        let distant_ground = apply_fog(
            fog,
            out.color,
            p,
            view.world_position.xyz,
            pbr_input.frag_coord.xy,
        );
        out.color = mix(out.color, distant_ground, low_ground);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
