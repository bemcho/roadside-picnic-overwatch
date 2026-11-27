// A cut scan's stone, cut away above the haul ramps (world/ramp_cut.rs):
// fragments higher than the cut, 0.1 m under the drawn terrain, in the ramps'
// corridor are discarded, then the standard PBR path runs.
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

// The image's corner (x, z), in meters, and its lattice points per meter (x, z).
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cut_bounds: vec4<f32>;
// At each lattice point: the height above which stone is cut (red) and how far
// the point lies past the corridor's edge, negative inside (green).
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var cut_lattice: texture_2d<f32>;

fn above_ramp(world: vec3<f32>) -> bool {
    let grid = (world.xz - cut_bounds.xy) * cut_bounds.zw;
    let last = vec2<f32>(textureDimensions(cut_lattice) - 1u);
    if any(grid < vec2<f32>(0.0)) || any(grid >= last) {
        return false;
    }
    let cell = vec2<i32>(floor(grid));
    let t = fract(grid);
    // Linear over each half of the lattice cell, split along the anti-diagonal
    // as the ground mesh's triangles are: the cut follows the drawn ground and
    // the corridor's edge runs smoothly over the stone instead of in steps.
    let b = textureLoad(cut_lattice, cell + vec2<i32>(1, 0), 0).rg;
    let c = textureLoad(cut_lattice, cell + vec2<i32>(0, 1), 0).rg;
    var cut: vec2<f32>;
    if t.x + t.y <= 1.0 {
        let a = textureLoad(cut_lattice, cell, 0).rg;
        cut = a + (b - a) * t.x + (c - a) * t.y;
    } else {
        let d = textureLoad(cut_lattice, cell + vec2<i32>(1, 1), 0).rg;
        cut = d + (c - d) * (1.0 - t.x) + (b - d) * (1.0 - t.y);
    }
    return cut.y < 0.0 && world.y > cut.x;
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
