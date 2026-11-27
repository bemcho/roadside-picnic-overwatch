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
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var asphalt_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var asphalt_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var asphalt_surface: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var asphalt_crack: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(114) var rock_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(115) var rock_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(116) var rock_surface: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(117) var old_road_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(118) var old_road_normal: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(119) var old_road_height: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(120) var road_legs: texture_2d<u32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(121) var leg_table: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(122) var<uniform> old_road_band: vec4<f32>;

const TAU: f32 = 6.2831855;

// The old asphalt (tools/prepare_asphalt_textures.py): a tiling grain set with crack decals
// scattered over it, one decal at most per cell. The maps' mip chains are the distance
// versions, so only the cracks and the relief are faded out by hand.
// Metres one repeat of the grain maps covers.
const ASPHALT_TILE: f32 = 1.2;
// Metres per side of the cells that each may hold one crack decal, the share of cells that do,
// the decal's largest side and how far its centre may stray from its cell's. A decal turned any
// way must stay inside its cell (a test checks it), or the neighbouring cell would cut it off.
const CRACK_CELL: f32 = 8.0;
const CRACK_SHARE: f32 = 0.5;
const CRACK_SIZE: f32 = 4.0;
const CRACK_JITTER: f32 = 2.0;
// The smallest decal, as a share of CRACK_SIZE, and the factor a crack darkens the grain by.
const CRACK_SMALLEST: f32 = 0.6;
const CRACK_DARKEST: f32 = 0.2;
// Metres the grain's lows lie under its tops, and a crack's bottom under the grain.
const GRAIN_DEPTH: f32 = 0.01;
const CRACK_DEPTH: f32 = 0.03;
// The relief march's steps, and the ground's pixel size (m per pixel) under which it runs.
const RELIEF_STEPS: i32 = 12;
const RELIEF_NEAR: f32 = 0.02;

// The Zone's old road (tools/bake_road_kit.py): the retired road kit baked into a strip that is
// laid along every asphalt leg (roads.rs `leg_frame`), growing in from `old_road_band.x` inside
// the Perimeter until it is whole at `.y`; `.z` is the strip's period along the road, `.w` its
// width. Metres its relief spans in the parallax: the bake's own 1.16 m (heaved slabs, pothole
// floors) would sink the road out of the ground it is painted on, so the parallax keeps a tenth.
const OLD_ROAD_RELIEF: f32 = 0.12;
// The ground's pixel size (m per pixel) under which the parallax runs, and under which the photo
// grain starts to show in the kit, once the kit's own 16 mm texels are magnified.
const OLD_ROAD_NEAR: f32 = 0.05;
const OLD_ROAD_TEXEL: f32 = 0.016;
// How much nearer one leg must be than its bend's other leg to be laid alone; inside that the
// two frames blend, so the kit does not break along the bend's bisector.
const OLD_ROAD_BEND: f32 = 1.5;
// Metres per cell of the noise that decides which patches break up first.
const OLD_ROAD_PATCH: f32 = 3.0;

// Steep ground (cut banks, fill faces, pit walls) is dressed in rock
// (tools/prepare_rock_textures.py). Metres one repeat of the rock maps covers.
const ROCK_TILE: f32 = 3.5;
// The slope, as 1 - y of the smooth vertex normal, at which the rock starts to
// show (46 degrees) and where it covers the ground (55 degrees). The cut banks
// lean past 55 degrees to their rims. A facet's own slope would be sharper but
// flips triangle by triangle on rough hillsides, a checkerboard of rock.
const ROCK_FROM: f32 = 0.3;
const ROCK_FULL: f32 = 0.42;

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

struct Crack {
    centre: vec2<f32>,
    turn: mat2x2<f32>,
    // The decal's side in metres.
    size: f32,
    // How much of the decal shows: zero in a cell without one.
    weight: f32,
}

// The crack decal of the cell around `xz`: shifted off the cell's centre, turned and scaled at
// random, never larger than `CRACK_SIZE`, which stays inside the cell so that no neighbour cuts
// it off.
fn crack_of(xz: vec2<f32>, fade: f32) -> Crack {
    let cell = floor(xz / CRACK_CELL);
    let offset = vec2<f32>(
        patch_hash(cell + vec2<f32>(17.0, 5.0)), patch_hash(cell + vec2<f32>(3.0, 29.0)),
    ) - 0.5;
    var crack: Crack;
    crack.centre = (cell + 0.5) * CRACK_CELL + offset * CRACK_JITTER;
    crack.turn = rotation(patch_hash(cell + vec2<f32>(53.0, 11.0)) * TAU);
    crack.size = CRACK_SIZE * mix(CRACK_SMALLEST, 1.0, patch_hash(cell + vec2<f32>(23.0, 41.0)));
    crack.weight = select(0.0, fade, patch_hash(cell) < CRACK_SHARE);
    return crack;
}

// The decal at a world point: tangent normal xy, depth and mask, the last two weighted by how
// much of the crack shows. The gradients follow the decal's turn, so they never jump at cell borders.
fn crack_at(crack: Crack, xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if crack.weight <= 0.0 {
        return vec4<f32>(0.0);
    }
    let uv = crack.turn * (xz - crack.centre) / crack.size + 0.5;
    let texel = textureSampleGrad(
        asphalt_crack, ground_sampler, uv,
        crack.turn * dx / crack.size, crack.turn * dy / crack.size,
    );
    let inside = all(uv > vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0));
    return texel * vec4<f32>(1.0, 1.0, vec2<f32>(select(0.0, crack.weight, inside)));
}

// World xz tilt of a tangent normal. The maps' y runs up the texture, against the rows, and
// the rows run along +z, so the tilt's z is the map's y flipped.
fn asphalt_tilt(xy: vec2<f32>) -> vec2<f32> {
    let z = sqrt(max(1.0 - dot(xy, xy), 0.0));
    return vec2<f32>(xy.x, -xy.y) / max(z, 0.3);
}

// A tiling map without a visible lattice: every lattice point of the tiling has its own random
// offset into the map, and the four around the pixel blend bilinearly. Independent copies lose
// contrast where they blend, so the blend is rescaled from the map's average (`mean`) by the
// weights' energy. The offsets are hashed per pixel because they follow the world position:
// a baked offset map would be larger than the grain maps themselves.
fn stochastic(
    tex: texture_2d<f32>, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, mean: vec4<f32>,
) -> vec4<f32> {
    let cell = floor(uv);
    let t = fract(uv);
    let w = t * t * (3.0 - 2.0 * t);
    var blend = vec4<f32>(0.0);
    var energy = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let lattice = cell + vec2<f32>(f32(i), f32(j));
            let weight = select(1.0 - w.x, w.x, i == 1) * select(1.0 - w.y, w.y, j == 1);
            let offset = vec2<f32>(
                patch_hash(lattice + vec2<f32>(7.0, 19.0)), patch_hash(lattice + vec2<f32>(31.0, 2.0)),
            );
            blend += textureSampleGrad(tex, ground_sampler, uv + offset, dx, dy) * weight;
            energy += weight * weight;
        }
    }
    return mean + (blend - mean) / sqrt(energy);
}

// The averages of the grain maps, from the last mip level. They are not computed here: the shipped KTX2
// maps already carry each average as their 1 x 1 mip (baked once by `ctt`), so this is one texel fetch,
// and no per-map constant has to be kept in step with the bake (Precalculate once).
fn map_mean(tex: texture_2d<f32>) -> vec4<f32> {
    return textureSampleLevel(tex, ground_sampler, vec2<f32>(0.5), 16.0);
}

// How far the grain's surface lies under its top at a world point.
fn grain_depth(xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, height_mean: f32) -> f32 {
    let height = stochastic(
        asphalt_surface, xz / ASPHALT_TILE, dx / ASPHALT_TILE, dy / ASPHALT_TILE, vec4<f32>(height_mean),
    ).b;
    return (1.0 - clamp(height, 0.0, 1.0)) * GRAIN_DEPTH;
}

// Where the view ray meets the asphalt, as how deep it has gone: a march through the grain and
// crack depths near the camera, a single step on the grain's own height farther out.
fn asphalt_hit(
    crack: Crack, xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, along: vec2<f32>, pixel_size: f32,
    height_mean: f32,
) -> f32 {
    if pixel_size >= RELIEF_NEAR {
        return grain_depth(xz, dx, dy, height_mean);
    }
    // The crack's depth fades out so that the march and the single step meet.
    let relief = 1.0 - smoothstep(0.5 * RELIEF_NEAR, RELIEF_NEAR, pixel_size);
    let deepest = GRAIN_DEPTH + CRACK_DEPTH * relief;
    var above = -grain_depth(xz, dx, dy, height_mean) - crack_at(crack, xz, dx, dy).b * CRACK_DEPTH * relief;
    var ray_above = 0.0;
    var hit = deepest;
    for (var i = 1; i <= RELIEF_STEPS; i++) {
        let ray = deepest * f32(i) / f32(RELIEF_STEPS);
        let at = xz + along * ray;
        let gap = ray - grain_depth(at, dx, dy, height_mean)
            - crack_at(crack, at, dx, dy).b * CRACK_DEPTH * relief;
        if gap >= 0.0 {
            hit = mix(ray_above, ray, above / (above - gap));
            break;
        }
        above = gap;
        ray_above = ray;
    }
    return hit;
}

struct OldAsphalt {
    albedo: vec3<f32>,
    tilt: vec2<f32>,
    roughness: f32,
    occlusion: f32,
}

// The old asphalt where the mask holds `old` of it. `along` is how far the view ray shifts over
// the ground per metre of depth; `dx` and `dy` are the ground's screen derivatives.
fn old_asphalt(
    xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, along: vec2<f32>, old: f32, pixel_size: f32,
) -> OldAsphalt {
    // Cracks keep to the broken middle of the road and fade out with distance.
    let fade = smoothstep(0.6, 1.0, old) * (1.0 - smoothstep(0.08, 0.25, pixel_size));
    let crack = crack_of(xz, fade);
    let surface_mean = map_mean(asphalt_surface);
    let at = xz + along * asphalt_hit(crack, xz, dx, dy, along, pixel_size, surface_mean.b);
    let uv = at / ASPHALT_TILE;
    let grad_x = dx / ASPHALT_TILE;
    let grad_y = dy / ASPHALT_TILE;
    let albedo = max(
        stochastic(asphalt_albedo, uv, grad_x, grad_y, map_mean(asphalt_albedo)).rgb, vec3<f32>(0.0),
    );
    let normal = stochastic(asphalt_normal, uv, grad_x, grad_y, map_mean(asphalt_normal)).xy;
    let surface = stochastic(asphalt_surface, uv, grad_x, grad_y, surface_mean).rgb;
    let notch = crack_at(crack, at, dx, dy);
    // The decal's tilt is in its own turned frame; turn it back into the world's.
    let notch_tilt = transpose(crack.turn) * asphalt_tilt(notch.xy * 2.0 - 1.0);
    var asphalt: OldAsphalt;
    asphalt.albedo = albedo * mix(1.0, CRACK_DARKEST, notch.a);
    asphalt.tilt = mix(asphalt_tilt(normal * 2.0 - 1.0), notch_tilt, notch.a);
    asphalt.roughness = mix(0.34, 0.98, surface.g);
    asphalt.occlusion = clamp(surface.r, 0.0, 1.0) * mix(1.0, 0.5, notch.a);
    return asphalt;
}

// How far the Zone's old road has taken over at a world point: 0 outside the band, 1 past it.
fn old_road_wear(xz: vec2<f32>) -> f32 {
    return 1.0 - smoothstep(old_road_band.y, old_road_band.x, xz.x);
}

struct RoadLeg {
    start: vec2<f32>,
    along: vec2<f32>,
    length: f32,
    // How far along its route the leg starts.
    route: f32,
}

// Row `id` - 1 of the leg table.
fn road_leg(id: u32) -> RoadLeg {
    let a = textureLoad(leg_table, vec2<i32>(0, i32(id) - 1), 0);
    let b = textureLoad(leg_table, vec2<i32>(1, i32(id) - 1), 0);
    return RoadLeg(a.xy, a.zw, b.x, b.y);
}

// Whether `next` carries on from `leg` along the same route (a new route starts at 0).
fn continues(leg: RoadLeg, next: RoadLeg) -> bool {
    return next.route > 0.0 && abs(leg.route + leg.length - next.route) < 0.01;
}

// The legs the kit is laid along at a world point: the mask texel's leg or the bend neighbour
// on whose side of the bend's bisector the point lies (`near`), the other one, and how much the
// other weighs: 0.5 on the bisector, 0 from OLD_ROAD_BEND either side of it. Distances to the
// legs would not do: past a bend's vertex both legs measure the same distance to it, so the whole
// outer wedge of the bend would blend.
struct LegPair {
    near: RoadLeg,
    other: RoadLeg,
    blend: f32,
    found: bool,
}

// How far `xz` lies on `next`'s side of the bisector between `leg` and `next`, the leg after it.
fn past_bend(leg: RoadLeg, next: RoadLeg, xz: vec2<f32>) -> f32 {
    let corner = leg.start + leg.along * leg.length;
    return dot(xz - corner, normalize(leg.along + next.along));
}

fn old_road_legs(xz: vec2<f32>) -> LegPair {
    var pair: LegPair;
    let size = textureDimensions(road_legs);
    let texel = vec2<i32>(floor((xz - road_bounds.xy) * road_bounds.zw * vec2<f32>(size)));
    if any(texel < vec2<i32>(0)) || any(texel >= vec2<i32>(size)) {
        return pair;
    }
    let id = textureLoad(road_legs, texel, 0).r;
    if id == 0u {
        return pair;
    }
    let leg = road_leg(id);
    pair.near = leg;
    pair.other = leg;
    pair.found = true;
    // Signed distance from the nearer bisector, negative on the neighbour's side.
    var side = 1.0e9;
    if id > 1u {
        let before = road_leg(id - 1u);
        if continues(before, leg) {
            side = past_bend(before, leg, xz);
            pair.other = before;
        }
    }
    if id < textureDimensions(leg_table).y {
        let after = road_leg(id + 1u);
        if continues(leg, after) {
            let after_side = -past_bend(leg, after, xz);
            if after_side < side {
                side = after_side;
                pair.other = after;
            }
        }
    }
    if side < 0.0 {
        pair.near = pair.other;
        pair.other = leg;
    }
    pair.blend = 0.5 * (1.0 - smoothstep(0.0, OLD_ROAD_BEND, abs(side)));
    return pair;
}

// The strip's texture coordinates at a world point of a leg, and their screen gradients. Across
// runs toward the kit's +x, the leg's direction turned clockwise.
struct StripUv {
    uv: vec2<f32>,
    dx: vec2<f32>,
    dy: vec2<f32>,
    across: vec2<f32>,
}

fn strip_uv(leg: RoadLeg, xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> StripUv {
    var strip: StripUv;
    strip.across = vec2<f32>(leg.along.y, -leg.along.x);
    let scale = old_road_band.wz;
    let offset = xz - leg.start;
    strip.uv = vec2<f32>(dot(offset, strip.across), leg.route + dot(offset, leg.along)) / scale
        + vec2<f32>(0.5, 0.0);
    strip.dx = vec2<f32>(dot(dx, strip.across), dot(dx, leg.along)) / scale;
    strip.dy = vec2<f32>(dot(dy, strip.across), dot(dy, leg.along)) / scale;
    return strip;
}

struct Kit {
    albedo: vec3<f32>,
    // 1 on the asphalt, 0 in potholes and the gaps between slabs.
    cover: f32,
    height: f32,
    tilt: vec2<f32>,
}

fn kit_at(leg: RoadLeg, xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> Kit {
    let strip = strip_uv(leg, xz, dx, dy);
    let colour = textureSampleGrad(old_road_albedo, ground_sampler, strip.uv, strip.dx, strip.dy);
    // The normal's xy lie in the road's frame (across, along); no y flip, unlike the photo maps.
    let xy = textureSampleGrad(old_road_normal, ground_sampler, strip.uv, strip.dx, strip.dy).xy * 2.0 - 1.0;
    let z = sqrt(max(1.0 - dot(xy, xy), 0.0));
    var kit: Kit;
    kit.albedo = colour.rgb;
    kit.cover = colour.a;
    kit.height = textureSampleGrad(old_road_height, ground_sampler, strip.uv, strip.dx, strip.dy).r;
    kit.tilt = (strip.across * xy.x + leg.along * xy.y) / max(z, 0.3);
    return kit;
}

// Where the view ray meets the kit's relief near the camera, in one parallax step: the ray goes
// as deep as the strip's height there says. A march like `asphalt_hit`'s would fetch the strip
// twelve times per road pixel out to a wider range than the grain's; the normal map carries the
// slab edges, so one fetch keeps the potholes' depth for little.
fn kit_hit(leg: RoadLeg, xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, along: vec2<f32>, pixel_size: f32) -> vec2<f32> {
    let relief = OLD_ROAD_RELIEF * (1.0 - smoothstep(0.5 * OLD_ROAD_NEAR, OLD_ROAD_NEAR, pixel_size));
    if relief <= 0.0 {
        return xz;
    }
    let strip = strip_uv(leg, xz, dx, dy);
    let height = textureSampleGrad(old_road_height, ground_sampler, strip.uv, strip.dx, strip.dy).r;
    return xz + along * (1.0 - height) * relief;
}

// Today's asphalt (`current`) giving way to the old road kit as `wear` grows: patches chosen by
// a noise below `wear` take the kit, its potholes and cracks first, whole slabs from mid-band on.
// The kit's gaps show `ground`, the bare ground under the road, shaded toward their floor. Close
// up, the photo grain of `current` modulates the kit, whose own texels are OLD_ROAD_TEXEL.
fn old_road(
    xz: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, along: vec2<f32>, pixel_size: f32, wear: f32,
    ground: vec3<f32>, current: OldAsphalt,
) -> OldAsphalt {
    let legs = old_road_legs(xz);
    if !legs.found {
        return current;
    }
    let at = kit_hit(legs.near, xz, dx, dy, along, pixel_size);
    var kit = kit_at(legs.near, at, dx, dy);
    if legs.blend > 0.0 {
        let other = kit_at(legs.other, at, dx, dy);
        kit.albedo = mix(kit.albedo, other.albedo, legs.blend);
        kit.cover = mix(kit.cover, other.cover, legs.blend);
        kit.height = mix(kit.height, other.height, legs.blend);
        kit.tilt = mix(kit.tilt, other.tilt, legs.blend);
    }
    let noise = patch_noise(xz / OLD_ROAD_PATCH) * 0.7 + patch_noise(xz / 0.9 + 11.0) * 0.3;
    let lows = 1.0 - smoothstep(0.3, 0.5, kit.height);
    let take = smoothstep(noise - 0.06, noise + 0.06, wear * 1.2 - 0.1)
        * mix(lows, 1.0, smoothstep(0.35, 0.75, wear));
    let detail = 1.0 - smoothstep(0.5 * OLD_ROAD_TEXEL, OLD_ROAD_TEXEL, pixel_size);
    let grain = mix(
        vec3<f32>(1.0), current.albedo / max(map_mean(asphalt_albedo).rgb, vec3<f32>(1.0e-3)), detail,
    );
    let pit = ground * mix(0.45, 0.8, kit.height);
    var road: OldAsphalt;
    road.albedo = mix(pit, kit.albedo * grain, kit.cover);
    road.tilt = kit.tilt + current.tilt * 0.5 * kit.cover * detail;
    road.roughness = mix(0.95, current.roughness, kit.cover);
    road.occlusion = current.occlusion * mix(0.5, 1.0, kit.height);
    var out: OldAsphalt;
    out.albedo = mix(current.albedo, road.albedo, take);
    out.tilt = mix(current.tilt, road.tilt, take);
    out.roughness = mix(current.roughness, road.roughness, take);
    out.occlusion = mix(current.occlusion, road.occlusion, take);
    return out;
}

// The rock on one projection plane, weighted: colour, packed surface and the
// tangent normal's slope along the plane's (u, v). The maps' y runs up the
// texture, against v.
struct Rock {
    albedo: vec3<f32>,
    surface: vec3<f32>,
    tilt: vec3<f32>,
}

fn rock_plane(uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, weight: f32) -> Rock {
    var rock: Rock;
    if weight <= 0.0 {
        return rock;
    }
    let albedo = stochastic(rock_albedo, uv, dx, dy, map_mean(rock_albedo)).rgb;
    let surface = stochastic(rock_surface, uv, dx, dy, map_mean(rock_surface)).rgb;
    let n = stochastic(rock_normal, uv, dx, dy, map_mean(rock_normal)).xyz * 2.0 - 1.0;
    rock.albedo = max(albedo, vec3<f32>(0.0)) * weight;
    rock.surface = surface * weight;
    rock.tilt = vec3<f32>(n.x, -n.y, 0.0) / max(n.z, 0.3) * weight;
    return rock;
}

// The rock projected on the planes the surface faces (`weights`), with its
// tilt turned back onto the world axes each plane spans. `dx` and `dy` are the
// world position's screen derivatives, so it may run in a branch.
fn rock_layer(p: vec3<f32>, dx: vec3<f32>, dy: vec3<f32>, weights: vec3<f32>) -> Rock {
    // Planes with a sliver of weight are skipped; the rest share all of it.
    let kept = select(vec3<f32>(0.0), weights, weights >= vec3<f32>(0.02));
    let share = kept / dot(kept, vec3<f32>(1.0));
    let x = rock_plane(p.zy / ROCK_TILE, dx.zy / ROCK_TILE, dy.zy / ROCK_TILE, share.x);
    let y = rock_plane(p.xz / ROCK_TILE, dx.xz / ROCK_TILE, dy.xz / ROCK_TILE, share.y);
    let z = rock_plane(p.xy / ROCK_TILE, dx.xy / ROCK_TILE, dy.xy / ROCK_TILE, share.z);
    var rock: Rock;
    rock.albedo = x.albedo + y.albedo + z.albedo;
    rock.surface = x.surface + y.surface + z.surface;
    rock.tilt = vec3<f32>(0.0, x.tilt.y, x.tilt.x) + vec3<f32>(y.tilt.x, 0.0, y.tilt.y)
        + vec3<f32>(z.tilt.xy, 0.0);
    return rock;
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
    let p = in.world_position.xyz;
    // The slope of the triangle itself: the vertex normals are averaged over
    // 4 m and lean far less than a cut bank's 2 m faces, so projected by them
    // the ground's top-down maps streak down the bank. Where the ground is
    // steep, the projection takes the facet's tilt, turned to the vertex
    // normal's smooth heading so that the side planes blend without seams.
    // The lighting keeps the smooth vertex normal: lit by the facets, the
    // ground would show its triangles.
    let world_dx = dpdx(p);
    let world_dy = dpdy(p);
    let facet_normal = normalize(cross(world_dy, world_dx));
    let facet = select(-facet_normal, facet_normal, dot(facet_normal, normal) >= 0.0);
    let steep = smoothstep(ROCK_FROM, ROCK_FULL, 1.0 - normal.y);
    let heading = normalize(
        select(facet.xz, normal.xz, dot(normal.xz, normal.xz) > 1.0e-6) + vec2<f32>(1.0e-6, 0.0),
    );
    let tilted = vec3<f32>(heading * length(facet.xz), facet.y).xzy;
    let surface_normal = normalize(mix(normal, tilted, steep));
    let axis_weights = pow(abs(surface_normal), vec3<f32>(4.0));
    let weights = axis_weights / dot(axis_weights, vec3<f32>(1.0));
    var rock: Rock;
    if steep > 0.0 {
        rock = rock_layer(p, world_dx, world_dy, weights);
    }
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
    let pixel_size = max(length(world_dx), length(world_dy));
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
    var albedo = mix(mix(moss, stone, exposed), rock.albedo, steep);
    let luminance = dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
    albedo = mix(albedo, vec3<f32>(luminance) * vec3<f32>(0.72, 0.74, 0.70), ash * 0.9);
    let road_uv = clamp((p.xz - road_bounds.xy) * road_bounds.zw, vec2<f32>(0.0), vec2<f32>(1.0));
    // Road mask: red gravel, green tire ruts, blue asphalt, alpha the old, broken share of it.
    let road = textureSampleLevel(road_surface, ground_sampler, road_uv, 0.0);
    // The ground the old road's potholes open onto: half gravel, half what grows beside it.
    let bare = albedo;
    let gravel = (vec3<f32>(0.23, 0.184, 0.124) + earth * 0.48) * (1.0 - road.g * 0.34);
    albedo = mix(albedo, gravel, road.r);
    // Weathered asphalt, grained by the earth map.
    let tar = vec3<f32>(0.052, 0.053, 0.057) + dot(earth, vec3<f32>(0.2126, 0.7152, 0.0722)) * 0.2;
    albedo = mix(albedo, tar, road.b);
    // The ground's derivatives were taken outside any branch; the asphalt samples with them.
    let ground_dx = world_dx.xz;
    let ground_dy = world_dy.xz;
    var asphalt: OldAsphalt;
    if road.a > 0.0 {
        let along = -tangent_view.xz / max(facing, 0.32);
        // In the Zone the kit takes over, and today's crack decals give way to its own cracks.
        let wear = old_road_wear(p.xz);
        asphalt = old_asphalt(p.xz, ground_dx, ground_dy, along, road.a * (1.0 - wear), pixel_size);
        if wear > 0.0 {
            let ground = mix(gravel, bare, 0.5);
            asphalt = old_road(p.xz, ground_dx, ground_dy, along, pixel_size, wear, ground, asphalt);
        }
    }
    albedo = mix(albedo, asphalt.albedo, road.a);
    albedo *= mix(vec3<f32>(1.0), vec3<f32>(0.48, 0.56, 0.42), wetness);
    let alpine = smoothstep(32.0, 100.0, p.y);
    albedo = mix(albedo, vec3<f32>(0.62, 0.81, 1.06) * (luminance + 0.038), alpine * 0.74);
    let snowline = 112.0 + patch_noise(p.xz / 24.0) * 29.0 + (1.0 - normal.y) * 72.0;
    let snow = smoothstep(snowline - 12.0, snowline + 13.0, p.y);
    albedo = mix(albedo, vec3<f32>(0.65, 0.71, 0.75) * (0.76 + 0.24 * patches), snow);
    pbr_input.material.base_color = vec4<f32>(albedo * in.color.a, 1.0);

    // Packed maps: red AO, green roughness, blue height.
    let surface = mix(
        mix(
            sample_ground(grass_surface, grass_uv, weights, grass_axes),
            sample_ground(earth_surface, earth_uv, weights, earth_axes),
            exposed,
        ),
        rock.surface,
        steep,
    );
    let roughness = mix(mix(0.34, 0.98, surface.g), asphalt.roughness, road.a);
    pbr_input.material.perceptual_roughness = roughness * mix(1.0, 0.65, wetness);
    pbr_input.material.metallic = 0.0;
    let occlusion = mix(mix(1.0, surface.r, 0.6), mix(1.0, asphalt.occlusion, 0.6), road.a);
    pbr_input.diffuse_occlusion *= vec3<f32>(occlusion);
    pbr_input.N = normalize(mix(
        ground_normal(grass_normal, grass_uv, weights, normal, grass_axes),
        ground_normal(earth_normal, earth_uv, weights, normal, earth_axes),
        exposed,
    ));
    let rock_tilt = rock.tilt - normal * dot(rock.tilt, normal);
    pbr_input.N = normalize(mix(pbr_input.N, normalize(normal + rock_tilt * 0.8), steep));
    // Asphalt lies flatter than the ground's texture.
    pbr_input.N = normalize(mix(pbr_input.N, normal, road.b * 0.7));
    // The old asphalt's own grain and cracks tilt the surface normal.
    let tilt = vec3<f32>(asphalt.tilt.x, 0.0, asphalt.tilt.y);
    let asphalt_normal_world = normalize(normal + (tilt - normal * dot(tilt, normal)) * 0.8);
    pbr_input.N = normalize(mix(pbr_input.N, asphalt_normal_world, road.a));

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
