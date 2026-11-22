// Bends imported plants downwind. Each vertex moves by its height above the
// plant's root (the instance origin), so roots stay put and tips move most.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};

// xy: downwind bend in meters at a 1 m tip, z: seconds of wind, w: gust (0-1).
// A storage buffer shared by every plant material, rewritten each frame.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage, read> wind: vec4<f32>;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );

    let flex = max(world_position.y - world_from_local[3].y, 0.0);
    let phase = dot(world_from_local[3].xz, vec2<f32>(0.37, 0.23));
    let flutter = 0.65 + (0.25 + 0.15 * wind.w) * sin(wind.z * 2.1 + phase)
        + 0.1 * sin(wind.z * 5.3 + phase * 1.9);
    // Tall stalks bend from the root, but not further than a 1 m blade per meter.
    let push = wind.xy * flutter * flex * min(flex, 1.0);
    world_position.x += push.x;
    world_position.z += push.y;
    // Keep stalks about their length as they bend over.
    world_position.y -= 0.5 * dot(push, push) / max(flex, 0.05);

    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_in.instance_index,
    );
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_in.instance_index,
    );
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index,
        world_from_local[3],
    );
    return out;
}
