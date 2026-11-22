//! Fog. A thin, terrain-hugging ground fog, from half the armored car's height
//! to its roof, gathers over water, vegetation and low ground at night, at
//! dawn, and on humid or rainy days ([`render`] raymarches it over the fog
//! [`field`]). The same pass marches three layers of cloud at different
//! heights, whose cover follows the weather. Bevy's distance fog adds a cloud
//! shelf over the far lowlands.

mod field;
mod render;

use std::f32::consts::{PI, TAU};

use bevy::camera::Exposure;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::player::PlayerCamera;

use super::terrain::{BASE, Terrain};
use super::terrain_material::{TerrainMaterial, update_terrain_fog_settings};
use super::weather::{self, Weather, Wind, WorldClock};
use super::{LandscapeMaterials, ViewFocus};

pub(super) use field::FogField;
pub(crate) use render::GroundFog;
pub(super) use render::{GroundFogField, GroundFogRenderPlugin};

pub(crate) const CAMERA_FAR: f32 = 16.0 * 128.0;
const MIN_VISIBILITY: f32 = 10.0 * 128.0;
/// Extra fog-free distance per meter the camera rises above the ground.
const OVERVIEW_CLEARANCE: f32 = 10.0;
/// Camera heights above the ground over which the ground fog thins out, so
/// the zoomed-out map is not a white blanket.
const OVERVIEW_FADE: (f32, f32) = (140.0, 520.0);
/// The far lowlands below this height above the view focus take distance fog.
const CLOUD_SHELF_TOP: f32 = 6.0;
const CLOUD_SHELF_DEPTH: f32 = 10.0;

/// Period of the regional cloud-bank pattern, in meters.
const CLOUD_MAP_WIDTH: f32 = 4_096.0;

/// A ground tile, in meters: the terrain's finest grid cell.
const GROUND_TILE: f32 = 2.0;
/// Visibility inside the ground fog, from its densest to its thinnest banks.
const VISIBILITY_TILES: (f32, f32) = (1.0, 5.0);
/// Height of the fog's top above the ground: half the armored car, up to its roof.
const FOG_TOP: (f32, f32) = (1.25, 2.6);
/// The hangar hall stays clear of fog.
const HALL_HALF_EXTENTS: Vec2 = Vec2::new(18.5, 20.5);
/// Diffuse skylight on a clear day, in lux.
const SKY_LUX: f32 = 18_000.0;
/// Rate of the fog's slow internal churn.
const CHURN: f32 = 0.05;
/// Cloud cover aloft from a clear day (a few fair-weather puffs) to a rainy one.
const CLOUD_COVER: (f32, f32) = (0.3, 0.6);

pub(crate) fn camera_fog() -> GroundFog {
    // Koschmieder: visibility = 3.912 / extinction.
    let extinction = |tiles: f32| 3.912 / (tiles * GROUND_TILE);
    GroundFog {
        field: field::origin().extend(1.0 / field::EXTENT).extend(0.0),
        shape: Vec4::new(
            FOG_TOP.0,
            FOG_TOP.1,
            extinction(VISIBILITY_TILES.1),
            extinction(VISIBILITY_TILES.0),
        ),
        clear_box: Vec4::new(BASE.0, BASE.1, HALL_HALF_EXTENTS.x, HALL_HALF_EXTENTS.y),
        shroud: super::shroud::mask_bounds(),
        ..default()
    }
}

pub(crate) fn camera_distance_fog() -> DistanceFog {
    DistanceFog {
        color: Color::linear_rgb(0.72, 0.80, 0.76),
        directional_light_color: Color::linear_rgba(1.0, 0.74, 0.38, 0.18),
        directional_light_exponent: 18.0,
        falloff: FogFalloff::from_visibility_squared(CAMERA_FAR),
    }
}

/// Feeds the weather, the sky light and the wind drift to the fog pass, and
/// thins the fog as the camera climbs to an overview.
pub(super) fn update_ground_fog(
    time: Res<Time>,
    clock: Res<WorldClock>,
    (weather, wind): (Res<Weather>, Res<Wind>),
    terrain: Res<Terrain>,
    focus: Query<&Transform, With<ViewFocus>>,
    mut cameras: Query<(&mut GroundFog, &Transform, &Exposure), Without<ViewFocus>>,
) {
    let Ok(focus) = focus.single() else {
        return;
    };
    let ground = terrain.mesh_height_at(focus.translation.xz());
    let day = weather::daylight(clock.hour());
    let sky = Vec3::new(0.50, 0.60, 1.0).lerp(Vec3::new(0.80, 0.87, 1.0), day)
        * (weather::MOON_LUX * 0.3).lerp(SKY_LUX, day)
        * (1.0 - 0.35 * weather.cloud);
    let dt = time.delta_secs();
    // Gusts drift the banks faster and churn them up.
    let drift = wind.velocity * dt;
    let churn = dt * CHURN * (1.0 + 2.0 * wind.gust);
    for (mut fog, camera, exposure) in &mut cameras {
        let height = (camera.translation.y - ground).max(0.0);
        let fade = 1.0 - smoothstep(OVERVIEW_FADE.0, OVERVIEW_FADE.1, height);
        fog.weather = Vec4::new(weather.fog, fade, 0.0, 0.0);
        fog.clouds = Vec4::new(cloud_cover(weather.cloud), weather.cloud, 0.0, 0.0);
        fog.wind += Vec4::new(drift.x, drift.y, churn, 0.0);
        fog.ambient = (sky * exposure.exposure() / PI).extend(0.0);
    }
}

pub(super) fn follow_terrain_fog(
    focus: Query<&Transform, With<ViewFocus>>,
    terrain: Res<Terrain>,
    landscape_materials: Res<LandscapeMaterials>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Ok(focus) = focus.single() else {
        return;
    };
    let fog_top = terrain.mesh_height_at(focus.translation.xz()) + CLOUD_SHELF_TOP;
    update_terrain_fog_settings(
        &mut materials,
        &landscape_materials.terrain,
        fog_top,
        CLOUD_SHELF_DEPTH,
    );
}

/// Regional visibility around the view focus, shortened by humid air and
/// stretched by the camera's height so a zoomed-out overview stays clear.
pub(super) fn update_distance_fog(
    focus: Query<&Transform, With<ViewFocus>>,
    terrain: Res<Terrain>,
    weather: Res<Weather>,
    mut cameras: Query<(&mut DistanceFog, &Transform), With<PlayerCamera>>,
) {
    let Ok(focus) = focus.single() else {
        return;
    };
    let at = focus.translation.xz();
    for (mut fog, camera) in &mut cameras {
        let height = (camera.translation.y - terrain.mesh_height_at(at)).max(0.0);
        let regional = visibility_at(at, &terrain) * (1.0 - 0.4 * weather.fog);
        fog.falloff = FogFalloff::from_visibility_squared(regional + height * OVERVIEW_CLEARANCE);
    }
}

/// Cover of the cloud layers for the weather's cloudiness (0-1).
fn cloud_cover(cloud: f32) -> f32 {
    CLOUD_COVER.0.lerp(CLOUD_COVER.1, cloud.clamp(0.0, 1.0))
}

fn visibility_at(at: Vec2, terrain: &Terrain) -> f32 {
    let u = at.x / CLOUD_MAP_WIDTH + 0.5;
    let v = at.y / CLOUD_MAP_WIDTH + 0.5;
    let cloud = cloud_noise(u.rem_euclid(1.0), v.rem_euclid(1.0), 0.5);
    let ground = terrain.mesh_height_at(at);
    let lowland = 1.0 - super::terrain::smoothstep(16.0, 95.0, ground);
    let density = (cloud * 0.72 + lowland * 0.18).clamp(0.0, 1.0);
    CAMERA_FAR - (CAMERA_FAR - MIN_VISIBILITY) * density
}

fn cloud_noise(u: f32, v: f32, height: f32) -> f32 {
    let broad = periodic_noise(u, v + height * 0.025, 6.0, 0.7);
    let medium = periodic_noise(u + height * 0.04, v, 15.0, 2.3);
    let fine = periodic_noise(u, v - height * 0.055, 39.0, 4.1);
    let cloud = (broad * 0.57 + medium * 0.31 + fine * 0.12).clamp(0.0, 1.0);
    let billow = smoothstep(0.38, 0.76, cloud);
    let upper_edge = 0.58 + billow * 0.30;
    let vertical = smoothstep(0.0, 0.16, height) * (1.0 - smoothstep(upper_edge, 1.0, height));
    (0.04 + billow * 0.96) * vertical
}

fn periodic_noise(u: f32, v: f32, frequency: f32, phase: f32) -> f32 {
    let x = TAU * u * frequency;
    let z = TAU * v * frequency;
    0.5 + 0.25
        * (f32::sin(x + phase) * f32::cos(z * 0.93 - phase * 0.7)
            + f32::sin((x + z) * 0.73 + phase * 1.9) * f32::cos((z - x) * 0.61 - phase))
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_visibility_stays_between_ten_and_sixteen_tiles() {
        let terrain = Terrain::new();
        let visibilities: Vec<_> = (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| Vec2::new(x as f32 * 384.0, z as f32 * 384.0)))
            .map(|at| visibility_at(at, &terrain))
            .collect();
        assert!(
            visibilities
                .iter()
                .all(|visibility| (MIN_VISIBILITY..=CAMERA_FAR).contains(visibility))
        );
        let minimum = visibilities.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let maximum = visibilities
            .iter()
            .fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        assert!(maximum - minimum >= 128.0, "fog banks must vary by region");
    }

    #[test]
    fn clear_days_keep_a_few_clouds_and_rain_brings_the_most() {
        assert!(cloud_cover(0.0) > 0.0);
        assert!(cloud_cover(0.0) < cloud_cover(0.65));
        assert!(cloud_cover(0.65) < cloud_cover(0.85));
        assert_eq!(cloud_cover(2.0), CLOUD_COVER.1);
    }

    #[test]
    fn distance_fog_builds_denser_cloud_banks_toward_visibility_limit() {
        assert!(matches!(
            camera_distance_fog().falloff,
            FogFalloff::ExponentialSquared { .. }
        ));
    }
}
