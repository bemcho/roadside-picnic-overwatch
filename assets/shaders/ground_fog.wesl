// Ground fog: a thin layer that follows the terrain (or open water), raymarched
// from the camera to the depth buffer. Above the layer the march jumps toward
// it; inside, steps follow the fog's mean free path. Light comes from the view's
// directional lights (sun and moon) through the atmosphere and their shadow
// cascades, plus sky light, with forward scattering toward each light.
//
// Clouds: three layers of volumetric fog at different heights, shaped into
// clouds with flat bases and domed, billowy tops, marched along the same ray.
//
// Shroud: ground the stalker has not surveyed (src/world/shroud.rs) and the fog
// over it lie under black, slowly drifting smoke. It is drawn here, after the
// sky's aerial perspective, so nothing of that ground shows from any height.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import bevy_pbr::atmosphere::{
    functions::{calculate_visible_sun_ratio, clamp_to_surface},
    bruneton_functions::transmittance_lut_r_mu_to_uv,
};
import bevy_pbr::render::mesh_view_bindings::{
    view, lights, globals, atmosphere, atmosphere_transmittance_texture,
    atmosphere_transmittance_sampler,
};
import bevy_pbr::render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT;
import bevy_pbr::render::shadow_sampling::sample_shadow_map_hardware;
import bevy_pbr::render::shadows::{get_cascade_index, world_to_directional_light_local};
import bevy_pbr::render::utils::interleaved_gradient_noise;
import bevy_pbr::render::view_transformations::{
    frag_coord_to_ndc, position_ndc_to_world, position_world_to_view,
};

struct GroundFog {
    // Field origin (x, z) and inverse extent.
    field: vec4<f32>,
    // Fog tendency (0-1) and overview fade.
    weather: vec4<f32>,
    // Layer top range (m) and extinction range (1/m), thin to dense.
    shape: vec4<f32>,
    // Wind drift (x, z) and churn phase.
    wind: vec4<f32>,
    // Exposed sky light.
    ambient: vec4<f32>,
    // Fog-free box: center (x, z) and half extents.
    clear_box: vec4<f32>,
    // Cloud cover (0-1) and how dark rain clouds grow (0-1).
    clouds: vec4<f32>,
    // Shroud mask corner (x, z) and one over its extent.
    shroud: vec4<f32>,
    // Harmont's fog: top range (m) and extinction range (1/m), thin to dense.
    harmont: vec4<f32>,
}

@group(1) @binding(0) var<uniform> ground: GroundFog;
@if(MULTISAMPLED)
@group(1) @binding(1) var depth_texture: texture_depth_multisampled_2d;
@else
@group(1) @binding(1) var depth_texture: texture_depth_2d;
// r + g: surface height; b: fog affinity; a: 1 on Harmont's side, 0 in the Zone.
@group(1) @binding(2) var field_texture: texture_2d<f32>;
@group(1) @binding(3) var field_sampler: sampler;
// r: 1 where surveyed, 0 under the shroud.
@group(1) @binding(4) var shroud_mask: texture_2d<f32>;
@group(1) @binding(5) var shroud_sampler: sampler;

const PI: f32 = 3.14159265;
const MAX_STEPS: u32 = 96u;
const FAR: f32 = 6000.0;
// Steepest terrain rise assumed when jumping toward the layer.
const SLOPE: f32 = 0.9;
const ALBEDO: vec3<f32> = vec3(0.93, 0.95, 0.97);
// Distance along the ray over which one direct-light evaluation is reused, in
// the Zone's dense fog and in Harmont's thin fog.
const LIGHT_REUSE: vec2<f32> = vec2(3.0, 10.0);

// A layer of clouds: base and depth above sea level, the spacing of its
// clouds, how much faster than the ground wind it drifts, its share of the
// weather's cover and a noise offset (all in meters but the last three).
struct CloudLayer {
    base: f32,
    depth: f32,
    size: f32,
    drift: f32,
    cover: f32,
    seed: f32,
}

const CLOUD_LAYERS: u32 = 3u;
const CLOUD_STEPS: u32 = 18u;
// Extinction in a cloud's core: about 130 m of visibility.
const CLOUD_EXTINCTION: f32 = 0.03;
// Clouds fade into the sky toward this distance.
const CLOUD_FAR: f32 = 7000.0;
// Clouds clear around the camera as it flies through a layer.
const CLOUD_NEAR: vec2<f32> = vec2(20.0, 120.0);
const CLOUD_ALBEDO: vec3<f32> = vec3(0.96, 0.97, 1.0);

fn cloud_layer(index: u32) -> CloudLayer {
    switch index {
        // Low scud that drifts over the ridge's peaks.
        case 0u: { return CloudLayer(170.0, 110.0, 260.0, 3.0, 0.8, 0.0); }
        // Fair-weather cumulus.
        case 1u: { return CloudLayer(450.0, 260.0, 560.0, 5.0, 1.0, 37.0); }
        // Large clouds high over the map.
        default: { return CloudLayer(1050.0, 320.0, 950.0, 8.0, 0.85, 71.0); }
    }
}

struct FogSample {
    extinction: f32,
    top: f32,
}

fn hash2(p: vec2<f32>) -> f32 {
    var q = fract(vec3(p.x, p.y, p.x) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.zyx + 31.32);
    return fract((q.x + q.y) * q.z);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x),
        mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x),
        u.y,
    );
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let lower = mix(
        mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), u.x),
        mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), u.x),
        u.z,
    );
    let upper = mix(
        mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), u.x),
        mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + vec3(1.0, 1.0, 1.0)), u.x),
        u.z,
    );
    return mix(lower, upper, u.y);
}

fn henyey_greenstein(cos_theta: f32, g: f32) -> f32 {
    let denom = 1.0 + g * g - 2.0 * g * cos_theta;
    return (1.0 - g * g) / (4.0 * PI * denom * sqrt(denom));
}

// 0 inside the hangar hall, 1 outside it.
fn outside_hall(xz: vec2<f32>) -> f32 {
    let d = abs(xz - ground.clear_box.xy) - ground.clear_box.zw;
    return smoothstep(-1.0, 1.0, max(d.x, d.y));
}

// Fog at `p`, `h` meters above the surface, where the field holds `site`
// (affinity, Harmont's side). Fog forms only where the field allows it and
// only in moist air; drifting banks break it up and fray its edges, and
// billows roughen the top. The Zone's thin layer opens in small clear
// patches; Harmont's is thinner and its top rises and falls.
fn sample_fog(p: vec3<f32>, h: f32, site: vec2<f32>) -> FogSample {
    let affinity = site.x;
    let harmont = site.y;
    let w = p.xz + ground.wind.xy;
    let banks = noise2(w / 90.0) * 0.65 + noise2(w / 31.0) * 0.35;
    let clearing = noise2(w / 30.0 + 23.5) * 0.7 + noise2(w / 11.0 + 5.1) * 0.3;
    let gap = smoothstep(0.58, 0.70, clearing) * (1.0 - harmont);
    let place = smoothstep(0.15, 0.55, affinity + 0.25 * (banks - 0.5)) * (1.0 - gap);
    // Water gets the thickest layer.
    let moist = ground.weather.x + 0.2 * (affinity - 0.5);
    let coverage = place * smoothstep(0.12, 0.5, moist + 0.4 * (banks - 0.5));
    // Even saturated air leaves thinner and thicker banks.
    let swell = noise2(w / 150.0 + 41.7);
    let thickness = clamp(0.3 + 0.45 * (moist - 0.6) + 1.1 * (swell - 0.5), 0.0, 1.0);
    let churn = vec3(0.0, ground.wind.z, 0.0);
    let curl = noise3(vec3(w.x, p.y * 1.7, w.y) / 3.2 + churn);
    let billow = (curl - 0.5) * 0.8 + (noise3(vec3(w.x, p.y, w.y) / 9.0 - churn) - 0.5) * 0.9;
    let rise = smoothstep(0.25, 0.75, noise2(w / 210.0 + 7.3));
    var out: FogSample;
    out.top = mix(
        mix(ground.shape.x, ground.shape.y, thickness),
        mix(ground.harmont.x, ground.harmont.y, rise),
        harmont,
    ) + billow;
    // Harmont's fog is densest on the ground and thins out toward its top.
    let thinning = mix(1.0, 1.5 * clamp(1.0 - h / max(out.top, 0.5), 0.0, 1.0), harmont);
    let soft = mix(0.6, 2.0, harmont);
    let profile = smoothstep(-1.6, -0.2, h) * (1.0 - smoothstep(out.top - soft, out.top + 0.15, h))
        * thinning;
    let puffs = 0.55 + 0.9 * curl;
    let dense = clamp(thickness * 0.75 + (banks - 0.5) * 0.8, 0.0, 1.0);
    let density = mix(
        mix(ground.shape.z, ground.shape.w, dense),
        mix(ground.harmont.z, ground.harmont.w, banks),
        harmont,
    );
    out.extinction = coverage * profile * density * puffs * outside_hall(p.xz) * ground.weather.y;
    return out;
}

@if(ATMOSPHERE)
fn sample_transmittance_lut(r: f32, mu: f32) -> vec3<f32> {
    let uv = transmittance_lut_r_mu_to_uv(atmosphere, r, mu);
    return textureSampleLevel(
        atmosphere_transmittance_texture, atmosphere_transmittance_sampler, uv, 0.0).rgb;
}

// Exposed radiance of directional light `i` (sun or moon) reaching `p`
// through the atmosphere.
fn light_radiance(i: u32, p: vec3<f32>) -> vec3<f32> {
    let light = &lights.directional_lights[i];
    var radiance = (*light).color.rgb * view.exposure;
@if(ATMOSPHERE) {
    let p_atmosphere = clamp_to_surface(
        atmosphere, (atmosphere.world_to_atmosphere * vec4(p, 1.0)).xyz);
    let r = length(p_atmosphere);
    let mu = dot((*light).direction_to_light.xyz, normalize(p_atmosphere));
    radiance *= sample_transmittance_lut(r, mu)
        * calculate_visible_sun_ratio(atmosphere, r, mu, (*light).sun_disk_angular_size);
}
    return radiance;
}

struct Direct {
    // Sun and moon light reaching the fog's top, scattered toward the camera.
    light: vec3<f32>,
    // Path length through the fog per meter of depth, toward the strongest light.
    slant: f32,
}

// Direct light at `p`: through the atmosphere, the shadow cascades, and the
// billow between `p` and the light, which shades the side of each billow
// facing away from it. Forward scattering brightens fog toward the light.
fn direct_light(p: vec3<f32>, h: f32, site: vec2<f32>, ray: vec3<f32>) -> Direct {
    var out: Direct;
    out.light = vec3(0.0);
    out.slant = 1.0;
    var strongest = 0.0;
    let view_z = position_world_to_view(p).z;
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        let light = &lights.directional_lights[i];
        let to_light = (*light).direction_to_light.xyz;
        let radiance = light_radiance(i, p);
        if all(radiance < vec3(1.0e-4)) {
            continue;
        }
        var lit = 1.0;
        if ((*light).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            let cascade = get_cascade_index(i, view_z);
            let local = world_to_directional_light_local(
                i, cascade, vec4(p + (*light).shadow_depth_bias * to_light, 1.0));
            if local.w != 0.0 {
                lit = sample_shadow_map_hardware(
                    local.xy, local.z, i32((*light).depth_texture_base_index + cascade));
            }
        }
        let shoulder = sample_fog(p + to_light * 1.2, h + to_light.y * 1.2, site).extinction;
        let phase = 0.7 + 0.3 * 4.0 * PI * henyey_greenstein(dot(to_light, ray), 0.6);
        let reaching = radiance * (lit * phase / PI * exp(-shoulder * 0.36));
        out.light += reaching;
        let strength = dot(reaching, vec3(0.3, 0.6, 0.1));
        if strength > strongest {
            strongest = strength;
            out.slant = 1.0 / max(to_light.y, 0.12);
        }
    }
    return out;
}

// Radiance scattered toward the camera at height `h` in the fog. Light from
// above fades with depth; multiple scattering keeps a bank's inside from
// going dark.
fn light_fog(h: f32, fog: FogSample, direct: Direct) -> vec3<f32> {
    let buried = max(fog.top - h, 0.0);
    let sky = ground.ambient.rgb * (0.45 + 0.55 * exp(-fog.extinction * buried * 0.6));
    let sun = direct.light * exp(-fog.extinction * buried * direct.slant * 0.3);
    return (sky + sun) * ALBEDO;
}

// How much cloud a layer holds at `p`, from 0 in clear air to 1 in a cloud's
// core. Patches of the layer swell into clouds whose tops rise with their
// strength, so each cloud is a dome on a flat base; billows erode its edges.
fn cloud_density(p: vec3<f32>, layer: CloudLayer) -> f32 {
    let rise = (p.y - layer.base) / layer.depth;
    if rise <= 0.0 || rise >= 1.0 {
        return 0.0;
    }
    let w = (p.xz + ground.wind.xy * layer.drift) / layer.size + layer.seed;
    let cells = noise2(w) * 0.55 + noise2(w * 2.2 + 5.3) * 0.3 + noise2(w * 5.3 + 1.7) * 0.15;
    let cover = ground.clouds.x * layer.cover;
    let strength = saturate((cells - 1.0 + cover) / max(cover, 0.05));
    if strength <= 0.0 {
        return 0.0;
    }
    let top = mix(0.3, 1.0, strength);
    let profile = smoothstep(0.0, 0.08, rise) * (1.0 - smoothstep(top * 0.5, top, rise));
    let q = vec3(w.x, rise * layer.depth / layer.size, w.y) * 7.0;
    let billow = noise3(q) * 0.65 + noise3(q * 2.6 + 3.1) * 0.35;
    return saturate((profile * strength - (1.0 - billow) * 0.35) * 2.5);
}

// Light scattered toward the camera at `p` in a cloud: sun and moon dimmed by
// the cloud toward them (with a multiple-scattering tail that keeps cores from
// going black), brightest toward the light, plus sky light that fades toward
// the cloud's base.
fn light_cloud(p: vec3<f32>, rise: f32, layer: CloudLayer, ray: vec3<f32>) -> vec3<f32> {
    var light = vec3(0.0);
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        let to_light = lights.directional_lights[i].direction_to_light.xyz;
        let radiance = light_radiance(i, p);
        if all(radiance < vec3(1.0e-4)) {
            continue;
        }
        let near = 0.12 * layer.depth;
        let far = 0.35 * layer.depth;
        let optical = CLOUD_EXTINCTION * (cloud_density(p + to_light * near, layer) * near
            + cloud_density(p + to_light * far, layer) * (far - near));
        let through = exp(-optical) + 0.3 * exp(-optical * 0.15);
        let mu = dot(to_light, ray);
        let phase = 4.0 * PI * mix(henyey_greenstein(mu, -0.2), henyey_greenstein(mu, 0.7), 0.5);
        light += radiance * through * (0.6 + 0.4 * phase) / PI;
    }
    let sky = ground.ambient.rgb * mix(0.45, 1.1, rise);
    let rain = 1.0 - 0.45 * ground.clouds.y * (1.0 - rise);
    return (light + sky) * CLOUD_ALBEDO * rain;
}

// Cloud light and transmittance along the ray, up to `end`: each layer the
// ray crosses, nearest first, sampled more densely close to the camera.
fn march_clouds(camera: vec3<f32>, ray: vec3<f32>, end: f32, jitter: f32) -> vec4<f32> {
    var scattered = vec3(0.0);
    var transmittance = 1.0;
    if ground.clouds.x <= 0.0 {
        return vec4(scattered, transmittance);
    }
    let reach = min(end, CLOUD_FAR);
    for (var k = 0u; k < CLOUD_LAYERS; k += 1u) {
        // Downward rays meet the highest layer first.
        let layer = cloud_layer(select(k, CLOUD_LAYERS - 1u - k, ray.y < 0.0));
        let top = layer.base + layer.depth;
        var enter = 0.0;
        var leave = reach;
        if abs(ray.y) > 1.0e-4 {
            let a = (layer.base - camera.y) / ray.y;
            let b = (top - camera.y) / ray.y;
            enter = max(min(a, b), 0.0);
            leave = min(max(a, b), reach);
        } else if camera.y < layer.base || camera.y > top {
            continue;
        }
        let span = leave - enter;
        if span <= 0.0 {
            continue;
        }
        for (var i = 0u; i < CLOUD_STEPS; i += 1u) {
            let f = (f32(i) + jitter) / f32(CLOUD_STEPS);
            let t = enter + span * f * f;
            let step = span * (2.0 * f + 1.0 / f32(CLOUD_STEPS)) / f32(CLOUD_STEPS);
            let p = camera + ray * t;
            let fade = smoothstep(CLOUD_NEAR.x, CLOUD_NEAR.y, t)
                * (1.0 - smoothstep(0.5 * CLOUD_FAR, CLOUD_FAR, t));
            let density = cloud_density(p, layer) * fade;
            if density < 0.01 {
                continue;
            }
            let kept = exp(-density * CLOUD_EXTINCTION * step);
            let rise = (p.y - layer.base) / layer.depth;
            scattered += transmittance * (1.0 - kept) * light_cloud(p, rise, layer, ray);
            transmittance *= kept;
            if transmittance < 0.02 {
                return vec4(scattered, 0.0);
            }
        }
    }
    return vec4(scattered, transmittance);
}

// Ground fog light and transmittance along the ray, up to `end`.
fn march_ground_fog(camera: vec3<f32>, ray: vec3<f32>, end: f32, jitter: f32) -> vec4<f32> {
    var transmittance = 1.0;
    var scattered = vec3(0.0);
    if ground.weather.x * ground.weather.y <= 0.0 {
        return vec4(scattered, transmittance);
    }
    // Most clearance the ray can lose per meter over rising ground.
    let descent = SLOPE * length(ray.xz) - ray.y;
    var t = 0.0;
    var entered = false;
    var direct: Direct;
    var lit_at = -1.0e9;
    for (var i = 0u; i < MAX_STEPS; i += 1u) {
        if t >= end || transmittance < 0.01 {
            break;
        }
        let p = camera + ray * t;
        let uv = (p.xz - ground.field.xy) * ground.field.z;
        if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
            break;
        }
        let cell = textureSampleLevel(field_texture, field_sampler, uv, 0.0);
        let h = p.y - (cell.r + cell.g);
        let ceiling = mix(ground.shape.y, ground.harmont.y, cell.a) + 0.6;
        if h > ceiling {
            if descent <= 0.0 {
                break;
            }
            t += max((h - ceiling) / descent, 0.5);
            continue;
        }
        let fog = sample_fog(p, h, cell.ba);
        let above = h - fog.top;
        var step = clamp(0.9 / max(fog.extinction, 0.06), 0.3, 6.0);
        if above > 0.15 {
            // Clear air over the fog: drop to just above its top.
            if ray.y < -0.02 {
                step = clamp((above - 0.1) / -ray.y, 0.3, 6.0);
            }
        } else if above > -0.8 {
            // Cross the fog's soft top in short steps so it does not band;
            // Harmont's thin fog takes longer ones.
            step = min(step, max(mix(0.2, 1.0, cell.a) / max(abs(ray.y), 0.04), 0.3));
        }
        if !entered && above <= 0.15 {
            // Stagger the first sample in the fog per pixel so its top does not band.
            step *= jitter;
            entered = true;
        }
        let span = min(step, end - t);
        if fog.extinction > 1.0e-4 {
            if t - lit_at > mix(LIGHT_REUSE.x, LIGHT_REUSE.y, cell.a) {
                direct = direct_light(p, h, cell.ba, ray);
                lit_at = t;
            }
            let kept = exp(-fog.extinction * span);
            scattered += transmittance * (1.0 - kept) * light_fog(h, fog, direct);
            transmittance *= kept;
        }
        t += span;
    }
    return vec4(scattered, transmittance);
}

// How far the shroud covers ground at `p`: fully over unsurveyed cells, easing
// off over the cell between, its edge wobbled so the survey grid doesn't show.
fn shroud_at(p: vec2<f32>) -> f32 {
    let wobble = vec2<f32>(
        sin(p.y * 0.043 + 2.0 * sin(p.x * 0.019)),
        cos(p.x * 0.051 + 2.0 * sin(p.y * 0.017)),
    ) * 9.0;
    let uv = (p + wobble - ground.shroud.xy) * ground.shroud.zw;
    let seen = textureSampleLevel(shroud_mask, shroud_sampler, uv, 0.0).r;
    return 1.0 - smoothstep(0.2, 0.8, seen);
}

// The shroud's black smoke at `p`, drifting slowly.
fn smoke_at(p: vec2<f32>) -> vec3<f32> {
    let drift = vec2<f32>(1.3, 0.7) * globals.time;
    let smoke = 0.6 * smoke_noise((p + drift) / 47.0) + 0.4 * smoke_noise((p - 1.7 * drift) / 13.0);
    return vec3<f32>(0.012, 0.013, 0.016) * (0.3 + 2.2 * smoke * smoke);
}

fn smoke_hash(cell: vec2<f32>) -> f32 {
    var p = fract(vec3<f32>(cell.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p += dot(p, p.yzx + 33.33);
    return fract((p.x + p.y) * p.z);
}

// Smooth value noise, from 0 to 1.
fn smoke_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = smoke_hash(i);
    let b = smoke_hash(i + vec2<f32>(1.0, 0.0));
    let c = smoke_hash(i + vec2<f32>(0.0, 1.0));
    let d = smoke_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let frag = in.position.xy;
    let depth = textureLoad(depth_texture, vec2<i32>(frag), 0);
    let ndc = frag_coord_to_ndc(vec4(frag, depth, 1.0)).xy;
    let camera = view.world_position;
    let ray = normalize(position_ndc_to_world(vec3(ndc, 1.0)) - camera);
    var end = FAR;
    var shroud = 0.0;
    if depth > 0.0 {
        let hit = position_ndc_to_world(vec3(ndc, depth));
        end = min(distance(hit, camera), FAR);
        shroud = shroud_at(hit.xz);
    }
    let jitter = interleaved_gradient_noise(frag, globals.frame_count);
    let fog = march_ground_fog(camera, ray, end, jitter);
    let clouds = march_clouds(camera, ray, end, jitter);
    // Unsurveyed ground and the fog over it lie under the shroud's smoke.
    var below = fog;
    if shroud > 0.0 {
        let hit = camera + ray * end;
        below = vec4(mix(fog.rgb, smoke_at(hit.xz), shroud), fog.a * (1.0 - shroud));
    }
    // The clouds float above the ground fog and the shroud: a ray heading down
    // meets them first.
    var near = below;
    var far = clouds;
    if ray.y < 0.0 {
        near = clouds;
        far = below;
    }
    let scattered = near.rgb + near.a * far.rgb;
    return vec4(scattered, 1.0 - near.a * far.a);
}
