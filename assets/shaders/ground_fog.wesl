// Ground fog: a thin layer that follows the terrain (or open water), raymarched
// from the camera to the depth buffer. Above the layer the march jumps toward
// it; inside, steps follow the fog's mean free path. Light comes from the view's
// directional lights (sun and moon) through the atmosphere and their shadow
// cascades, plus sky light, with forward scattering toward each light.

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
}

@group(1) @binding(0) var<uniform> ground: GroundFog;
@if(MULTISAMPLED)
@group(1) @binding(1) var depth_texture: texture_depth_multisampled_2d;
@else
@group(1) @binding(1) var depth_texture: texture_depth_2d;
// r + g: surface height; b: fog affinity.
@group(1) @binding(2) var field_texture: texture_2d<f32>;
@group(1) @binding(3) var field_sampler: sampler;

const PI: f32 = 3.14159265;
const MAX_STEPS: u32 = 96u;
const FAR: f32 = 6000.0;
// Steepest terrain rise assumed when jumping toward the layer.
const SLOPE: f32 = 0.9;
const ALBEDO: vec3<f32> = vec3(0.93, 0.95, 0.97);
// Distance along the ray over which one direct-light evaluation is reused.
const LIGHT_REUSE: f32 = 3.0;

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

// Fog at `p`, `h` meters above the surface, where the field's affinity is
// `affinity`. Fog forms only where the field allows it (water and its banks,
// the quarry, the woods) and only in moist air; drifting banks break it up
// and fray its edges, and billows roughen the top.
fn sample_fog(p: vec3<f32>, h: f32, affinity: f32) -> FogSample {
    let w = p.xz + ground.wind.xy;
    let banks = noise2(w / 90.0) * 0.65 + noise2(w / 31.0) * 0.35;
    let place = smoothstep(0.15, 0.55, affinity + 0.25 * (banks - 0.5));
    // Water gets the thickest layer.
    let moist = ground.weather.x + 0.2 * (affinity - 0.5);
    let coverage = place * smoothstep(0.12, 0.5, moist + 0.4 * (banks - 0.5));
    // Even saturated air leaves thinner and thicker banks.
    let swell = noise2(w / 150.0 + 41.7);
    let thickness = clamp(0.3 + 0.45 * (moist - 0.6) + 1.1 * (swell - 0.5), 0.0, 1.0);
    let churn = vec3(0.0, ground.wind.z, 0.0);
    let curl = noise3(vec3(w.x, p.y * 1.7, w.y) / 3.2 + churn);
    let billow = (curl - 0.5) * 0.8 + (noise3(vec3(w.x, p.y, w.y) / 9.0 - churn) - 0.5) * 0.9;
    var out: FogSample;
    out.top = mix(ground.shape.x, ground.shape.y, thickness) + billow;
    let profile = smoothstep(-1.6, -0.2, h) * (1.0 - smoothstep(out.top - 0.6, out.top + 0.15, h));
    let puffs = 0.55 + 0.9 * curl;
    let density = mix(
        ground.shape.z,
        ground.shape.w,
        clamp(thickness * 0.75 + (banks - 0.5) * 0.8, 0.0, 1.0),
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

struct Direct {
    // Sun and moon light reaching the fog's top, scattered toward the camera.
    light: vec3<f32>,
    // Path length through the fog per meter of depth, toward the strongest light.
    slant: f32,
}

// Direct light at `p`: through the atmosphere, the shadow cascades, and the
// billow between `p` and the light, which shades the side of each billow
// facing away from it. Forward scattering brightens fog toward the light.
fn direct_light(p: vec3<f32>, h: f32, affinity: f32, ray: vec3<f32>) -> Direct {
    var out: Direct;
    out.light = vec3(0.0);
    out.slant = 1.0;
    var strongest = 0.0;
    let view_z = position_world_to_view(p).z;
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        let light = &lights.directional_lights[i];
        let to_light = (*light).direction_to_light.xyz;
        var radiance = (*light).color.rgb * view.exposure;
@if(ATMOSPHERE) {
        let p_atmosphere = clamp_to_surface(
            atmosphere, (atmosphere.world_to_atmosphere * vec4(p, 1.0)).xyz);
        let r = length(p_atmosphere);
        let mu = dot(to_light, normalize(p_atmosphere));
        radiance *= sample_transmittance_lut(r, mu)
            * calculate_visible_sun_ratio(atmosphere, r, mu, (*light).sun_disk_angular_size);
}
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
        let shoulder = sample_fog(p + to_light * 1.2, h + to_light.y * 1.2, affinity).extinction;
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

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    if ground.weather.x * ground.weather.y <= 0.0 {
        return vec4(0.0);
    }
    let frag = in.position.xy;
    let depth = textureLoad(depth_texture, vec2<i32>(frag), 0);
    let ndc = frag_coord_to_ndc(vec4(frag, depth, 1.0)).xy;
    let camera = view.world_position;
    let ray = normalize(position_ndc_to_world(vec3(ndc, 1.0)) - camera);
    var end = FAR;
    if depth > 0.0 {
        end = min(distance(position_ndc_to_world(vec3(ndc, depth)), camera), FAR);
    }

    let ceiling = ground.shape.y + 0.6;
    // Most clearance the ray can lose per meter over rising ground.
    let descent = SLOPE * length(ray.xz) - ray.y;
    let jitter = interleaved_gradient_noise(frag, globals.frame_count);
    var t = 0.0;
    var transmittance = 1.0;
    var scattered = vec3(0.0);
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
        if h > ceiling {
            if descent <= 0.0 {
                break;
            }
            t += max((h - ceiling) / descent, 0.5);
            continue;
        }
        let fog = sample_fog(p, h, cell.b);
        let above = h - fog.top;
        var step = clamp(0.9 / max(fog.extinction, 0.06), 0.3, 6.0);
        if above > 0.15 {
            // Clear air over the fog: drop to just above its top.
            if ray.y < -0.02 {
                step = clamp((above - 0.1) / -ray.y, 0.3, 6.0);
            }
        } else if above > -0.8 {
            // Cross the fog's soft top in short steps so it does not band.
            step = min(step, max(0.2 / max(abs(ray.y), 0.04), 0.3));
        }
        if !entered && above <= 0.15 {
            // Stagger the first sample in the fog per pixel so its top does not band.
            step *= jitter;
            entered = true;
        }
        let span = min(step, end - t);
        if fog.extinction > 1.0e-4 {
            if t - lit_at > LIGHT_REUSE {
                direct = direct_light(p, h, cell.b, ray);
                lit_at = t;
            }
            let kept = exp(-fog.extinction * span);
            scattered += transmittance * (1.0 - kept) * light_fog(h, fog, direct);
            transmittance *= kept;
        }
        t += span;
    }
    return vec4(scattered, 1.0 - transmittance);
}
