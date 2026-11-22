//! Big textured rocks (`assets/models/rocks`): boulders and standing stones on
//! the shoulders of the country roads, and groups of crags and monoliths with
//! a few boulders around them out in open country, mostly on high ground.
//! Each rock shows its textured `medium/` model near the view and a light
//! `low/` copy with the same material out to a range that grows with its size,
//! so the big ones read from far off.

use std::f32::consts::TAU;

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::{self, PreloadedAssets};

use super::collision::{Collider, Pillar};
use super::cordon::reserve_scenery;
use super::roads::{self, SHOULDER};
use super::terrain::{HALF_SIZE, Terrain, lattice_hash, lerp, smoothstep};
use super::{WorldRoot, canopy, detail_range, sites};

const SEED: u32 = 0x40C5_5EED;
/// One roll for a rock every this many meters of road.
const ROADSIDE_STEP: f32 = 24.0;
const ROADSIDE_CHANCE: f32 = 0.65;
/// How far past the shoulder a roadside rock may stand, in meters.
const ROADSIDE_SETBACK: (f32, f32) = (1.0, 7.0);
/// One rock group at most per cell of open country.
const COUNTRY_CELL: f32 = 190.0;
/// Chance of a group on low ground and on the uplands.
const COUNTRY_CHANCE: (f32, f32) = (0.25, 0.7);
/// Boulders around a group's big rock, and their distance from its edge.
const SATELLITES: u32 = 4;
const SATELLITE_GAP: (f32, f32) = (1.5, 9.0);
/// Size jitter around a rock's own size.
const JITTER: (f32, f32) = (0.85, 1.15);
/// Free ground between two rocks, in meters.
const SPACING: f32 = 1.5;
/// Ground no steeper than this rise across a rock, per meter of its size.
const MAX_RISE: f32 = 0.45;
/// Model tier ranges: meters plus meters per meter of the rock's size.
const NEAR: (f32, f32) = (45.0, 8.0);
const FAR: (f32, f32) = (350.0, 100.0);
const FADE: f32 = 25.0;

/// A rock model: its size in meters (the tallest side), half its footprint's
/// longer side and how much of it lies buried, both in model units.
struct Rock {
    near: &'static str,
    far: &'static str,
    size: f32,
    radius: f32,
    sink: f32,
    pillars: &'static [Pillar],
}

macro_rules! rock {
    ($size:literal, $radius:literal, $sink:literal) => {
        Rock {
            near: concat!("models/rocks/medium/rock-", $size, "m.glb"),
            far: concat!("models/rocks/low/rock-", $size, "m.glb"),
            size: $size as f32,
            radius: $radius,
            sink: $sink,
            pillars: include!(concat!("pillars/rock_", $size, "m.rs")),
        }
    };
}

/// The rounded boulders sink deeper, so they rest on more than one point.
static KIT: [Rock; 7] = [
    rock!(2, 0.474, 0.08),
    rock!(4, 0.474, 0.15),
    rock!(7, 0.349, 0.04),
    rock!(10, 0.501, 0.22),
    rock!(13, 0.501, 0.03),
    rock!(15, 0.363, 0.04),
    rock!(17, 0.411, 0.03),
];

/// How often each rock turns up beside a road, as a group's big rock, and
/// around a group.
const ROADSIDE: [f32; 7] = [0.45, 0.3, 0.15, 0.1, 0.0, 0.0, 0.0];
const LANDMARK: [f32; 7] = [0.0, 0.0, 0.15, 0.2, 0.2, 0.2, 0.25];
const SATELLITE: [f32; 7] = [0.65, 0.35, 0.0, 0.0, 0.0, 0.0, 0.0];

/// The rock `roll` picks from `weights`.
fn pick(weights: &[f32; 7], roll: f32) -> usize {
    let mut left = roll * weights.iter().sum::<f32>();
    (0..KIT.len())
        .find(|&i| {
            left -= weights[i];
            weights[i] > 0.0 && left < 0.0
        })
        .unwrap_or(0)
}

/// A rock standing on the map, and its footprint radius in meters.
#[derive(Clone, Copy, Debug)]
struct Placed {
    rock: usize,
    pose: Transform,
    radius: f32,
}

/// Places `rock` at `scale` meters per model unit on `at`, if the ground
/// there is free: off the roads, sites, walls, woods and water, not too
/// steep and clear of the rocks already placed.
fn place(
    terrain: &Terrain,
    placed: &[Placed],
    rock: usize,
    scale: f32,
    at: Vec2,
    turn: f32,
) -> Option<Placed> {
    let radius = KIT[rock].radius * scale;
    let inside = at.abs().max_element() < HALF_SIZE - radius - 20.0;
    if !inside
        || reserve_scenery(at, radius + 2.0)
        || sites::clear_footprint(at, radius + 4.0)
        || terrain.base_mask(at.x, at.y) > 0.0
        || canopy::wood(at).max(canopy::landmark_grove(at)) > 0.05
        || placed
            .iter()
            .any(|other| at.distance(other.pose.translation.xz()) < radius + other.radius + SPACING)
    {
        return None;
    }
    let ring: Vec<_> = (0..8)
        .map(|i| at + Vec2::from_angle(i as f32 * TAU / 8.0) * radius)
        .chain([at])
        .collect();
    if ring
        .iter()
        .any(|p| terrain.road_mask(p.x, p.y) > 0.0 || terrain.water_depth_at(p.x, p.y) > 0.0)
    {
        return None;
    }
    let heights = ring.iter().map(|&p| terrain.mesh_height_at(p));
    let (low, high) = heights.fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), h| {
        (low.min(h), high.max(h))
    });
    // `scale` is the rock's size in meters: the models are one unit across.
    if high - low > scale * MAX_RISE {
        return None;
    }
    let pose = Transform::from_xyz(at.x, low - KIT[rock].sink * scale, at.y)
        .with_rotation(Quat::from_rotation_y(turn * TAU))
        .with_scale(Vec3::splat(scale));
    Some(Placed { rock, pose, radius })
}

/// Boulders and standing stones along both shoulders of the country roads.
fn roadside(terrain: &Terrain, placed: &mut Vec<Placed>) {
    for (road, points) in roads::country_roads().enumerate() {
        for (leg, &[a, b]) in points.array_windows().enumerate() {
            let along = (b - a).normalize_or_zero();
            let steps = (a.distance(b) / ROADSIDE_STEP) as i32;
            for step in 0..steps {
                let roll = |n| lattice_hash(road as i32 * 64 + leg as i32, step, SEED + n);
                if roll(0) >= ROADSIDE_CHANCE {
                    continue;
                }
                let rock = pick(&ROADSIDE, roll(1));
                let scale = KIT[rock].size * lerp(JITTER.0, JITTER.1, roll(2));
                let side = if roll(3) < 0.5 {
                    along.perp()
                } else {
                    -along.perp()
                };
                let setback = lerp(ROADSIDE_SETBACK.0, ROADSIDE_SETBACK.1, roll(4));
                let center = a + along * ((step as f32 + lerp(0.2, 0.8, roll(5))) * ROADSIDE_STEP);
                let at = center + side * (SHOULDER + KIT[rock].radius * scale + setback);
                if let Some(rock) = place(terrain, placed, rock, scale, at, roll(6)) {
                    placed.push(rock);
                }
            }
        }
    }
}

/// Crags and monoliths in open country, each with a few boulders around it.
fn country(terrain: &Terrain, placed: &mut Vec<Placed>) {
    let cells = (HALF_SIZE / COUNTRY_CELL).ceil() as i32;
    for z in -cells..cells {
        for x in -cells..cells {
            let roll = |n| lattice_hash(x, z, SEED + 100 + n);
            let at = (Vec2::new(x as f32, z as f32)
                + Vec2::new(lerp(0.15, 0.85, roll(1)), lerp(0.15, 0.85, roll(2))))
                * COUNTRY_CELL;
            let upland = smoothstep(10.0, 80.0, terrain.height_at(at.x, at.y));
            if roll(0) >= lerp(COUNTRY_CHANCE.0, COUNTRY_CHANCE.1, upland) {
                continue;
            }
            let rock = pick(&LANDMARK, roll(3));
            let scale = KIT[rock].size * lerp(JITTER.0, JITTER.1, roll(4));
            let Some(main) = place(terrain, placed, rock, scale, at, roll(5)) else {
                continue;
            };
            placed.push(main);
            for n in 0..SATELLITES {
                let roll = |k| roll(10 + n * 8 + k);
                let rock = pick(&SATELLITE, roll(0));
                let scale = KIT[rock].size * lerp(0.4, 1.0, roll(1));
                let gap = lerp(SATELLITE_GAP.0, SATELLITE_GAP.1, roll(2));
                let reach = main.radius + KIT[rock].radius * scale + gap;
                let near = at + Vec2::from_angle(roll(3) * TAU) * reach;
                if let Some(rock) = place(terrain, placed, rock, scale, near, roll(4)) {
                    placed.push(rock);
                }
            }
        }
    }
}

/// Every rock on the map, roadside ones first.
fn scatter(terrain: &Terrain) -> Vec<Placed> {
    let mut placed = Vec::new();
    roadside(terrain, &mut placed);
    country(terrain, &mut placed);
    placed
}

pub(super) fn preload_rocks(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for rock in &KIT {
        let (mesh, paint) = assets::load_single_primitive(&server, rock.near);
        let (distant, _) = assets::load_single_primitive(&server, rock.far);
        preloaded
            .0
            .extend([mesh.untyped(), paint.untyped(), distant.untyped()]);
    }
}

pub(super) fn spawn_rocks(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Both tiers of a rock share its textured material, so each tier of a
    // rock draws as one instanced batch.
    let tiers: Vec<_> = KIT
        .iter()
        .map(|rock| {
            let (mesh, paint) = assets::load_single_primitive(&server, rock.near);
            let (distant, _) = assets::load_single_primitive(&server, rock.far);
            let paint = paints.get(&paint).expect("preloaded rock material");
            let paint = MeshMaterial3d(materials.add(standard_material_from_gltf_material(paint)));
            ((Mesh3d(mesh), paint.clone()), (Mesh3d(distant), paint))
        })
        .collect();
    let root = commands
        .spawn((
            WorldRoot,
            Name::new("Rocks"),
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    for placed in scatter(&terrain) {
        let (full, simple) = &tiers[placed.rock];
        let size = placed.pose.scale.y;
        let near = detail_range(NEAR.0 + NEAR.1 * size, FADE);
        let far = VisibilityRange {
            start_margin: near.end_margin.clone(),
            ..detail_range(FAR.0 + FAR.1 * size, FADE)
        };
        commands.spawn((
            full.clone(),
            placed.pose,
            near,
            Collider(KIT[placed.rock].pillars),
            ChildOf(root),
        ));
        commands.spawn((simple.clone(), placed.pose, far, ChildOf(root)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off_roads_and_water(terrain: &Terrain, placed: &Placed) -> bool {
        let at = placed.pose.translation.xz();
        terrain.road_mask(at.x, at.y) == 0.0 && terrain.water_depth_at(at.x, at.y) == 0.0
    }

    #[test]
    fn rocks_line_the_roads_off_the_lanes() {
        let terrain = Terrain::new();
        let mut placed = Vec::new();
        roadside(&terrain, &mut placed);
        assert!(
            (80..600).contains(&placed.len()),
            "{} roadside rocks",
            placed.len()
        );
        for rock in &placed {
            assert!(off_roads_and_water(&terrain, rock));
            // Within sight of the lane: the shoulder is the nearest road edge.
            let at = rock.pose.translation.xz();
            let reach = SHOULDER + rock.radius + ROADSIDE_SETBACK.1 + 1.0;
            let beside = (0..16).any(|i| {
                let p = at + Vec2::from_angle(i as f32 * TAU / 16.0) * reach;
                terrain.road_mask(p.x, p.y) > 0.0
            });
            assert!(beside, "rock at {at} is far from its road");
        }
    }

    #[test]
    fn open_country_holds_groups_of_big_rocks() {
        let terrain = Terrain::new();
        let placed = scatter(&terrain);
        let big = placed
            .iter()
            .filter(|rock| KIT[rock.rock].size >= 10.0)
            .count();
        assert!(big >= 25, "{big} big rocks");
        for (i, rock) in placed.iter().enumerate() {
            assert!(off_roads_and_water(&terrain, rock));
            let at = rock.pose.translation.xz();
            assert!(!sites::clear_footprint(at, rock.radius));
            assert!(rock.pose.translation.y < terrain.mesh_height_at(at));
            for other in &placed[..i] {
                let gap = at.distance(other.pose.translation.xz()) - rock.radius - other.radius;
                assert!(gap >= SPACING - 1e-3, "rocks overlap at {at}");
            }
        }
    }

    #[test]
    fn every_rock_is_solid_and_picks_only_weighted_models() {
        for rock in &KIT {
            assert!(!rock.pillars.is_empty());
        }
        for roll in [0.0, 0.3, 0.6, 0.99] {
            assert!(ROADSIDE[pick(&ROADSIDE, roll)] > 0.0);
            assert!(LANDMARK[pick(&LANDMARK, roll)] > 0.0);
            assert!(SATELLITE[pick(&SATELLITE, roll)] > 0.0);
        }
    }
}
