// The quarry scan's stone, cut away above the haul ramp (world/ramp_cut.rs):
// fragments higher than the terrain in the ramp's corridor are discarded, then
// the standard PBR path runs.
//
// Three pipelines draw the material: the main pass, the deferred prepass and
// the depth-only prepass of the shadow maps, which has no output and only cuts.
// The normal and motion-vector prepasses are not supported, the camera runs
// neither.

@if(PREPASS_PIPELINE && !DEFERRED_PREPASS)
import bevy_pbr::prepass::io::VertexOutput;

@if(!PREPASS_PIPELINE || DEFERRED_PREPASS)
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
};

@if(PREPASS_PIPELINE && DEFERRED_PREPASS)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};

@if(!PREPASS_PIPELINE)
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

// The image's corner (x, z) and one over its extent (x, z), in meters.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cut_bounds: vec4<f32>;
// The terrain's height in the ramp's corridor, 1.0e6 (nothing is cut) elsewhere.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var cut_heights: texture_2d<f32>;

// Stone this far above the terrain is left, so the ground does not z-fight it.
const TOLERANCE: f32 = 0.05;

fn above_ramp(world: vec3<f32>) -> bool {
    let uv = (world.xz - cut_bounds.xy) * cut_bounds.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0)) {
        return false;
    }
    let size = vec2<i32>(textureDimensions(cut_heights));
    let texel = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - 1);
    return world.y > textureLoad(cut_heights, texel, 0).r + TOLERANCE;
}

@if(PREPASS_PIPELINE && !DEFERRED_PREPASS)
@fragment
fn fragment(in: VertexOutput) {
    if above_ramp(in.world_position.xyz) {
        discard;
    }
}

@if(!PREPASS_PIPELINE || DEFERRED_PREPASS)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if above_ramp(in.world_position.xyz) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
