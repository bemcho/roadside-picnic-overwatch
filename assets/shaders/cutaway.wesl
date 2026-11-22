import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

// x: world height above which the building is cut away for the tactical view.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cutaway: vec4<f32>;
// xyz: the middle of the hall's floor in the world; the hall stands unrotated.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> hall_floor: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var plate_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var plate_albedo_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var plate_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var plate_normal_sampler: sampler;
// AO, roughness and metallic.
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var plate_surface: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var plate_surface_sampler: sampler;
// The parked car's bay: x, z and yaw from the floor's middle.
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var<uniform> bay_pose: vec4<f32>;

// The floor paint, in meters from the floor's middle with +Z toward the door.
// The walkway lines follow the exhibit rows (base/artifacts.rs); move them
// together.
const PLATE_TILE: f32 = 3.0;
const LINE_HALF: f32 = 0.06;
// x, then the z range, of the walkway lines beside the exhibit rows.
const WEST_WALKWAY: vec3<f32> = vec3<f32>(-5.4, -16.0, 12.0);
const EAST_WALKWAY: vec3<f32> = vec3<f32>(11.4, -14.0, 4.0);
// The car's bay, half across and half along.
const BAY_HALF: vec2<f32> = vec2<f32>(1.8, 3.4);
const DOOR_HALF_WIDTH: f32 = 11.2;
// z ranges of the trench grate and the hazard band inside the door.
const GRATE: vec2<f32> = vec2<f32>(12.5, 13.2);
const HAZARD: vec2<f32> = vec2<f32>(13.3, 14.2);
// The walls' hazard band at their foot, in meters.
const KICK_BAND: f32 = 0.35;

const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const SAFETY_YELLOW: vec3<f32> = vec3<f32>(0.80, 0.45, 0.01);
const HAZARD_BLACK: vec3<f32> = vec3<f32>(0.025, 0.025, 0.03);
const WALL_PAINT: vec3<f32> = vec3<f32>(0.20, 0.23, 0.25);

// Coverage of |d| < half, antialiased over the pixel's footprint.
fn band(d: f32, half: f32) -> f32 {
    let blur = max(fwidth(d), 1.0e-4);
    return 1.0 - smoothstep(half - blur, half + blur, abs(d));
}

// A line along z at x = line.x, from z = line.y to line.z.
fn walkway(p: vec2<f32>, line: vec3<f32>) -> f32 {
    let along = band(p.y - 0.5 * (line.y + line.z), 0.5 * (line.z - line.y));
    return band(p.x - line.x, LINE_HALF) * along;
}

// The outline of the parked car's bay.
fn bay(p: vec2<f32>) -> f32 {
    let d = p - bay_pose.xy;
    let yaw = bay_pose.z;
    let q = vec2<f32>(d.x * cos(yaw) - d.y * sin(yaw), d.x * sin(yaw) + d.y * cos(yaw));
    let outside = abs(q) - BAY_HALF;
    let edge = length(max(outside, vec2<f32>(0.0))) + min(max(outside.x, outside.y), 0.0);
    return band(edge, LINE_HALF);
}

// Alternating stripes, one per unit of t: 1 on one half, 0 on the other,
// fading to their average where they get finer than a pixel.
fn stripes(t: f32) -> f32 {
    let blur = max(fwidth(t), 1.0e-4);
    let wave = abs(fract(t) - 0.5) * 2.0;
    let sharp = smoothstep(0.5 - 2.0 * blur, 0.5 + 2.0 * blur, wave);
    return mix(sharp, 0.5, saturate(blur * 4.0 - 1.0));
}

// The bars of a square grating, likewise fading to their average.
fn grating(p: vec2<f32>, period: f32, half: f32) -> f32 {
    let d = abs(fract(p / period + 0.5) - 0.5) * period;
    let blur = max(fwidth(p), vec2<f32>(1.0e-4));
    let bars = vec2<f32>(1.0) - smoothstep(vec2<f32>(half) - blur, vec2<f32>(half) + blur, d);
    let open = 1.0 - 2.0 * half / period;
    return mix(max(bars.x, bars.y), 1.0 - open * open, saturate(max(blur.x, blur.y) / period * 2.0 - 1.0));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if in.world_position.y > cutaway.x {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    // Everything below is computed for every fragment, so the derivatives
    // stay in uniform control flow, then blended in by surface.
    let local = in.world_position.xyz - hall_floor.xyz;
    let p = local.xz;
    let normal = normalize(in.world_normal);
    let on_floor = smoothstep(0.8, 0.9, normal.y) * (1.0 - smoothstep(0.15, 0.3, local.y));
    let on_wall = 1.0 - smoothstep(0.35, 0.55, abs(normal.y));

    // Floor: steel plates, cooler and less brown than the swatch, with a worn
    // sheen, then the yellow lines, the hazard band and the trench grate.
    let uv = p / PLATE_TILE;
    let plate = textureSample(plate_albedo, plate_albedo_sampler, uv).rgb;
    let surface = textureSample(plate_surface, plate_surface_sampler, uv).rgb;
    let bump = textureSample(plate_normal, plate_normal_sampler, uv).rgb * 2.0 - 1.0;
    // A second, coarse and offset sample of the same plates breaks up the
    // repeat into larger light and dark patches.
    let patches = dot(textureSample(plate_albedo, plate_albedo_sampler, uv * 0.21 + 0.37).rgb, LUMA);
    let grey = dot(plate, LUMA);
    var floor_color = mix(vec3<f32>(grey), plate, 0.6) * mix(0.8, 1.35, smoothstep(0.1, 0.35, patches));
    // A worn sheen; rough and dull enough that the seams don't glint.
    var floor_roughness = max(surface.g * 0.7, 0.42);
    var floor_metallic = surface.b * 0.25;
    let worn_yellow = SAFETY_YELLOW * (0.6 + 1.2 * grey);
    let wear = mix(0.55, 1.0, smoothstep(0.08, 0.3, grey));
    let lines = max(max(walkway(p, WEST_WALKWAY), walkway(p, EAST_WALKWAY)), bay(p)) * wear;
    floor_color = mix(floor_color, worn_yellow, lines);
    let in_door = band(p.x, DOOR_HALF_WIDTH);
    let hazard = band(p.y - 0.5 * (HAZARD.x + HAZARD.y), 0.5 * (HAZARD.y - HAZARD.x)) * in_door;
    let hazard_color = mix(HAZARD_BLACK, worn_yellow, stripes((p.x + p.y) / 0.6));
    floor_color = mix(floor_color, hazard_color, hazard * wear);
    let paint = max(lines, hazard);
    floor_roughness = mix(floor_roughness, 0.6, paint);
    floor_metallic = mix(floor_metallic, 0.0, paint);
    let grate = band(p.y - 0.5 * (GRATE.x + GRATE.y), 0.5 * (GRATE.y - GRATE.x))
        * band(p.x, DOOR_HALF_WIDTH - 0.8);
    let bars = grating(p, 0.08, 0.015);
    floor_color = mix(floor_color, mix(vec3<f32>(0.01), vec3<f32>(0.12), bars), grate);
    floor_roughness = mix(floor_roughness, mix(1.0, 0.45, bars), grate);
    floor_metallic = mix(floor_metallic, 0.8 * bars, grate);
    // The plate normal map's +x runs along world +X and its +y along -Z,
    // flattened a little.
    let floor_normal = normalize(vec3<f32>(0.6 * bump.x, bump.z, -0.6 * bump.y));

    // Walls: the scan's panels and grime repainted in steel grey, darker
    // toward the floor, with a hazard band along their foot.
    let base = pbr_input.material.base_color.rgb;
    let along = select(local.x, local.z, abs(normal.x) > abs(normal.z));
    let grime = 1.0 - 0.45 * (1.0 - smoothstep(0.2, 1.4, local.y));
    var wall_color = WALL_PAINT * (0.5 + 2.0 * dot(base, LUMA)) * grime;
    let kick = band(local.y - 0.5 * KICK_BAND, 0.5 * KICK_BAND);
    let kick_color = mix(HAZARD_BLACK, SAFETY_YELLOW * 0.8, stripes((along + local.y) / 0.5));
    wall_color = mix(wall_color, kick_color, kick);

    let color = mix(mix(base, wall_color, on_wall), floor_color, on_floor);
    pbr_input.material.base_color = vec4<f32>(color, pbr_input.material.base_color.a);
    pbr_input.material.perceptual_roughness = mix(
        mix(pbr_input.material.perceptual_roughness, 0.55, on_wall),
        floor_roughness,
        on_floor,
    );
    pbr_input.material.metallic = mix(
        mix(pbr_input.material.metallic, 0.0, on_wall),
        floor_metallic,
        on_floor,
    );
    pbr_input.N = normalize(mix(pbr_input.N, floor_normal, on_floor));
    pbr_input.diffuse_occlusion *= mix(1.0, surface.r, on_floor);

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
