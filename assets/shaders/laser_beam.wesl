// A Perimeter gun's laser beam: a thin cylinder that would thin to broken
// dashes under a pixel wide. The vertex stage keeps it `beam.y` pixels wide
// at least, and the fragment stage fades it by how much of that width the
// real beam covers, with a soft edge across it, so it reads as a smooth line
// at any range.

import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::view,
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
};

@if(VISIBILITY_RANGE_DITHER)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

// x: the beam's radius in meters, y: its least width on screen in pixels.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> beam: vec4<f32>;

// Meters per pixel at `distance` from the camera.
fn pixel_at(distance: f32) -> f32 {
    return 2.0 * distance / (view.clip_from_view[1][1] * view.viewport.w);
}

// How much the beam is widened at `distance`: 1 while it is wide enough.
fn widening(distance: f32) -> f32 {
    return max(1.0, 0.5 * beam.y * pixel_at(distance) / beam.x);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    // The cylinder runs along local Y, unscaled across: widen it about its axis.
    let axis = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(0.0, vertex.position.y, 0.0, 1.0),
    );
    let across = (world_from_local * vec4<f32>(vertex.position.x, 0.0, vertex.position.z, 0.0)).xyz;
    let widen = widening(distance(axis.xyz, view.world_position));
    let world_position = vec4<f32>(axis.xyz + across * widen, 1.0);

    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_in.instance_index,
    );
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index,
        world_from_local[3],
    );
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    let to_view = view.world_position - in.world_position.xyz;
    // The share of its widened width the real beam covers.
    let coverage = 1.0 / widening(length(to_view));
    // Full along the middle, fading out to the silhouette.
    let facing = abs(dot(normalize(in.world_normal), normalize(to_view)));
    let edge = smoothstep(0.0, 0.7, facing);

    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    out.color.a *= coverage * edge;
    return out;
}
