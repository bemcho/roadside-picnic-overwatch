// Bends imported plants downwind. Each vertex moves by its height above the
// plant's root (the instance origin), so roots stay put and tips move most.
// The prepass bends them the same way, and as last frame's wind bent them for
// the motion vectors.

import bevy_pbr::render::{
    mesh_functions,
    view_transformations::position_world_to_clip,
};
@if(PREPASS_PIPELINE) import bevy_pbr::prepass::io::{Vertex, VertexOutput, decompress_vertex};
@else import bevy_pbr::render::forward_io::{Vertex, VertexOutput, decompress_vertex};

// Two vec4s, the wind now and a frame ago. xy: downwind bend in meters at a
// 1 m tip, z: seconds of wind, w: gust (0-1). A storage buffer shared by every
// plant material, rewritten each frame.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<storage, read> wind: array<vec4<f32>, 2>;

// `local` in the world of `world_from_local`, bent by `w`.
fn bent(world_from_local: mat4x4<f32>, local: vec3<f32>, w: vec4<f32>) -> vec4<f32> {
    var world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(local, 1.0),
    );
    let flex = max(world_position.y - world_from_local[3].y, 0.0);
    let phase = dot(world_from_local[3].xz, vec2<f32>(0.37, 0.23));
    let flutter = 0.65 + (0.25 + 0.15 * w.w) * sin(w.z * 2.1 + phase)
        + 0.1 * sin(w.z * 5.3 + phase * 1.9);
    // Tall stalks bend from the root, but not further than a 1 m blade per meter.
    let push = w.xy * flutter * flex * min(flex, 1.0);
    world_position.x += push.x;
    world_position.z += push.y;
    // Keep stalks about their length as they bend over.
    world_position.y -= 0.5 * dot(push, push) / max(flex, 0.05);
    return world_position;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    out.world_position = bent(world_from_local, vertex.position, wind[0]);
    out.position = position_world_to_clip(out.world_position.xyz);
@if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS)
    out.previous_world_position = bent(
        mesh_functions::get_previous_world_from_local(vertex_in.instance_index),
        vertex.position,
        wind[1],
    );
@if(!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_in.instance_index,
    );
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_TANGENTS && (!PREPASS_PIPELINE || NORMAL_PREPASS_OR_DEFERRED_PREPASS))
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
