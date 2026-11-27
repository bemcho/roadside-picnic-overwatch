// The ferrofluid pool under the Golden Sphere. The pool is a flat disc (its
// rim bent down to the floor) in its local xz; the vertex stage raises a
// hexagonal lattice of spikes on it, tallest under the Sphere, as strong as
// the Sphere's field and turned with its spin. Like real ferrofluid, spikes
// stand only where the field passes a threshold, so they form a crown that
// grows as the Sphere sinks. Rings of small spikes creep in from the rim and
// sink into the crown, feeding it. Where spikes would be only a few pixels
// apart on screen they settle into smooth liquid instead of sparkling. The
// fragment stage is standard PBR: black metal.

// The same vertex stage raises the spikes in the shadow pass, so they cast
// their shadows; there the full spikes always stand.
import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    view_transformations::position_world_to_clip,
};
@if(PREPASS_PIPELINE) import bevy_pbr::prepass::io::{Vertex, VertexOutput, decompress_vertex};
@else import bevy_pbr::render::forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex};

@if(VISIBILITY_RANGE_DITHER && !PREPASS_PIPELINE)
import bevy_pbr::render::pbr_functions::visibility_range_dither;

// x: the field (0..1), y: the lattice's turn in radians, z: the tallest
// spike's height, w: their spacing, in meters.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> field: vec4<f32>;
// x: the pool's radius, y: its rim's width, z: the field peak's sigma.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> shape: vec4<f32>;
// How far each ring of small spikes has come from the rim (0..1); w: the
// seconds of their pulse, wrapping every PULSE_WRAP.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> rings: vec4<f32>;
// x: where the rings start, y: where they end (from the middle), z, w: the
// small spikes' least and greatest height.
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> flow: vec4<f32>;

const HALF_SQRT3: f32 = 0.8660254;
const TAU: f32 = 6.2831855;
// The rounding of a tall and a small spike's tip, as a share of its foot's
// radius: enough that the grid never clips a moving tip.
const TIP: f32 = 0.12;
const SMALL_TIP: f32 = 0.25;
// Small spikes on each ring, the radius of their feet, and the shares of
// their way in over which they rise from the liquid and sink into the crown.
const SMALL_SPIKES: f32 = 12.0;
const SMALL_FOOT: f32 = 0.09;
const RISING: f32 = 0.15;
const SINKING: f32 = 0.7;
// Each small spike pulses up and down between the least and greatest height,
// at its own pace: a whole number of times (PULSES, plus up to PULSES more)
// in PULSE_WRAP seconds, so the wrapping clock never jumps.
const PULSE_WRAP: f32 = 60.0;
const PULSES: f32 = 20.0;
// The field (times the peak's falloff) under which the liquid stays smooth,
// and over which its spikes stand at their full share.
const CALM: f32 = 0.15;
const RISEN: f32 = 0.3;
// Pixels between two spikes on screen under which they settle, and over which
// they stand in full.
const SETTLED_PIXELS: f32 = 3.0;
const STANDING_PIXELS: f32 = 8.0;

// Meters per pixel at `distance` from the camera.
fn pixel_at(distance: f32) -> f32 {
    return 2.0 * distance / (view.clip_from_view[1][1] * view.viewport.w);
}

// `v` turned by `angle` about the vertical, the way a Y rotation turns xz.
fn turn(v: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(v.x * c + v.y * s, -v.x * s + v.y * c);
}

// The lattice point nearest `p`: one of the corners of the rhombus `p` lies in.
fn nearest_spike(p: vec2<f32>, spacing: f32) -> vec2<f32> {
    let row = p.y / (spacing * HALF_SQRT3);
    let base = floor(vec2<f32>(p.x / spacing - 0.5 * row, row));
    var best = vec2<f32>(0.0);
    var nearest = 1.0e9;
    for (var i = 0; i < 2; i++) {
        for (var j = 0; j < 2; j++) {
            let cell = base + vec2<f32>(f32(i), f32(j));
            let point = vec2<f32>(spacing * (cell.x + 0.5 * cell.y), spacing * HALF_SQRT3 * cell.y);
            let d = distance(p, point);
            if d < nearest {
                nearest = d;
                best = point;
            }
        }
    }
    return best;
}

// A spike of height 1 whose foot has radius `foot`, at `off` from its axis:
// its height and slope (d/dx, d/dz). A concave cone, as ferrofluid spikes
// stand, its tip rounded over `tip`.
fn cone(off: vec2<f32>, foot: f32, tip: f32) -> vec3<f32> {
    let u = length(off) / foot;
    let bent = sqrt(u * u + tip * tip) - tip;
    let base = sqrt(1.0 + tip * tip) - tip;
    let t = clamp(1.0 - bent / base, 0.0, 1.0);
    let falloff = -2.0 * t / base * u / sqrt(u * u + tip * tip) / foot;
    return vec3<f32>(t * t, falloff * off / max(length(off), 1.0e-5));
}

// A value in 0..1 that stays with spike `i` of ring `ring`, `salt` choosing
// one of several.
fn spike_hash(ring: f32, i: f32, salt: f32) -> f32 {
    return fract(sin(ring * 12.9898 + i * 78.233 + salt * 37.719) * 43758.547);
}

// The crown's spikes at `p`: height and slope (d/dx, d/dz).
fn crown(p: vec2<f32>) -> vec3<f32> {
    let r = length(p);
    let sigma = shape.z;
    let peak = exp(-r * r / (2.0 * sigma * sigma));
    let pull = field.x * peak;
    let envelope = peak * field.z * smoothstep(CALM, RISEN, pull)
        * (1.0 - smoothstep(shape.x - shape.y, shape.x, r));
    let local = turn(p, -field.y);
    let spike = cone(local - nearest_spike(local, field.w), 0.5 * field.w, TIP);
    return envelope * vec3<f32>(spike.x, turn(spike.yz, field.y));
}

// The small spikes at `p`, ring `k` of them `progress` of its way in: height
// and slope. Each ring turns with the Sphere, staggered from the others.
fn small_ring(p: vec2<f32>, k: i32, progress: f32) -> vec3<f32> {
    let radius = mix(flow.x, flow.y, progress);
    if abs(length(p) - radius) > SMALL_FOOT {
        return vec3<f32>(0.0);
    }
    let step = TAU / SMALL_SPIKES;
    let start = field.y + f32(k) * step / 3.0;
    let i = round((atan2(p.y, p.x) - start) / step);
    let angle = start + i * step;
    let axis = radius * vec2<f32>(cos(angle), sin(angle));
    let spike = i - SMALL_SPIKES * floor(i / SMALL_SPIKES);
    let pulses = PULSES + floor(PULSES * spike_hash(f32(k), spike, 0.0));
    let phase = TAU * spike_hash(f32(k), spike, 1.0);
    let pulse = 0.5 + 0.5 * sin(TAU * pulses * rings.w / PULSE_WRAP + phase);
    let tall = mix(flow.z, flow.w, pulse);
    let height = tall * smoothstep(0.0, RISING, progress) * (1.0 - smoothstep(SINKING, 1.0, progress));
    return height * cone(p - axis, SMALL_FOOT, SMALL_TIP);
}

// The liquid's height at `p` on the pool and its slope (d/dx, d/dz), `seen`
// the share of spikes standing at the viewer's distance.
fn spikes(p: vec2<f32>, seen: f32) -> vec3<f32> {
    var raised = crown(p);
    for (var k = 0; k < 3; k++) {
        raised += small_ring(p, k, rings[k]);
    }
    return seen * raised;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    // The share of spikes standing at the viewer's distance; a light's view
    // says nothing of it, so shadows keep them all.
    var seen = 1.0;
@if(!PREPASS_PIPELINE) {
    let middle = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let apart = field.w / pixel_at(distance(middle.xyz, view.world_position));
    seen = smoothstep(SETTLED_PIXELS, STANDING_PIXELS, apart);
}
    let raised = spikes(vertex.position.xz, seen);
    let position = vertex.position + vec3<f32>(0.0, raised.x, 0.0);

    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(position, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION && PREPASS_PIPELINE) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(!PREPASS_PIPELINE || (NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)) {
    let normal = normalize(vertex.normal + vec3<f32>(-raised.y, 0.0, -raised.z));
    out.world_normal = mesh_functions::mesh_normal_local_to_world(normal, vertex_in.instance_index);
}
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

@if(!PREPASS_PIPELINE)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
@if(VISIBILITY_RANGE_DITHER)
    visibility_range_dither(in.position, in.visibility_range_dither);

    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
