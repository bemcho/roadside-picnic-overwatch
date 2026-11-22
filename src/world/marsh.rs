//! The marsh's dead wood, fungus, rocks and debris: props from the swamp kit
//! (`assets/models/swamp/uhd`, each centered on its footprint and grounded at
//! y = 0) scattered over the marsh around `hydrology::SWAMP`. Trees, stumps,
//! logs and mud mounds may stand in the shallow pools; the rest keep to the
//! dry hummocks. The full scans show near the view, simplified copies
//! (`medium/`) farther out.

use std::f32::consts::TAU;

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::light::NotShadowCaster;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::{self, PreloadedAssets};

use super::collision::{Collider, Pillar, STEP_HEIGHT};
use super::cordon::reserve_scenery;
use super::hydrology::SWAMP;
use super::terrain::{MARSH_RADII, Terrain, lattice_hash, lerp, marsh_distance};
use super::{WorldRoot, detail_range};

/// Meters per kit unit; the dead trees stand taller than the scan's.
const SCALE: f32 = 30.0;
const TREE_SCALE: f32 = 1.3;
/// One prop at most per cell of this size, kept at this rate.
const CELL: f32 = 13.0;
const DENSITY: f32 = 0.4;
/// How deep a prop that may wade stands in a pool.
const WADE: f32 = 0.5;
/// Props shorter than this cast no shadows.
const SHADOW_HEIGHT: f32 = 2.0;
/// The full scan within this distance, the simplified one out to `FAR`.
const NEAR: f32 = 60.0;
const FAR: f32 = 320.0;
const FADE: f32 = 20.0;
const SEED: u32 = 0x5A3B_0C11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Tree,
    Trunk,
    Stump,
    Log,
    Branch,
    Fungus,
    Moss,
    Rock,
    Mound,
    Debris,
}

impl Group {
    const ALL: [Self; 10] = [
        Self::Tree,
        Self::Trunk,
        Self::Stump,
        Self::Log,
        Self::Branch,
        Self::Fungus,
        Self::Moss,
        Self::Rock,
        Self::Mound,
        Self::Debris,
    ];

    /// How often the group turns up.
    fn share(self) -> f32 {
        match self {
            Self::Tree => 0.17,
            Self::Trunk => 0.03,
            Self::Stump => 0.10,
            Self::Log => 0.12,
            Self::Branch => 0.14,
            Self::Fungus => 0.10,
            Self::Moss => 0.06,
            Self::Rock => 0.10,
            Self::Mound => 0.07,
            Self::Debris => 0.05,
        }
    }

    /// Whether it may stand in a shallow pool.
    fn wades(self) -> bool {
        matches!(
            self,
            Self::Tree | Self::Trunk | Self::Stump | Self::Log | Self::Mound
        )
    }

    fn scale(self) -> f32 {
        if self == Self::Tree {
            SCALE * TREE_SCALE
        } else {
            SCALE
        }
    }
}

/// A kit piece: its two tiers, its height and footprint (meters at
/// `SCALE`), and the upright pillar around its trunk or bulk (kit units).
struct Prop {
    group: Group,
    near: &'static str,
    far: &'static str,
    height: f32,
    /// Half the footprint's longer side.
    reach: f32,
    pillar: Option<Pillar>,
}

/// One pillar, measured at `SCALE`: radius and offset from the origin.
const fn pillar(height: f32, radius: f32, x: f32, z: f32) -> Option<Pillar> {
    if height <= STEP_HEIGHT {
        return None;
    }
    Some(Pillar {
        x: x / SCALE,
        z: z / SCALE,
        radius: radius / SCALE,
        bottom: 0.0,
        top: height / SCALE,
    })
}

macro_rules! prop {
    ($group:ident, $n:literal, $height:literal, ($fx:literal, $fz:literal),
     $radius:literal @ ($x:literal, $z:literal)) => {
        Prop {
            group: Group::$group,
            near: concat!("models/swamp/uhd/swamp_", $n, ".glb"),
            far: concat!("models/swamp/medium/swamp_", $n, ".glb"),
            height: $height,
            reach: if $fx > $fz { $fx / 2.0 } else { $fz / 2.0 },
            pillar: pillar($height, $radius, $x, $z),
        }
    };
}

/// The kit's usable props, measured at `SCALE`: height, footprint (x, z),
/// and the trunk's radius and offset. Its oily water and ground patches,
/// texture cards and leaf fragments are left out.
static KIT: [Prop; 60] = [
    prop!(Tree, "02", 7.2, (3.0, 2.9), 0.30 @ (0.15, 0.18)),
    prop!(Tree, "09", 6.5, (2.3, 2.7), 0.38 @ (-0.04, -0.13)),
    prop!(Tree, "12", 6.1, (1.6, 2.5), 0.57 @ (-0.01, 0.10)),
    prop!(Tree, "13", 4.2, (3.2, 2.4), 0.39 @ (0.88, -0.20)),
    prop!(Tree, "14", 6.7, (1.7, 2.4), 0.44 @ (0.25, 0.09)),
    prop!(Tree, "22", 6.4, (1.8, 1.8), 0.28 @ (0.30, -0.05)),
    prop!(Tree, "29", 6.7, (1.7, 3.1), 0.26 @ (0.0, 0.27)),
    prop!(Tree, "35", 4.9, (2.4, 2.2), 0.24 @ (-0.29, 0.23)),
    prop!(Tree, "26", 4.2, (2.5, 1.8), 0.29 @ (-0.73, -0.44)),
    prop!(Tree, "27", 3.0, (2.3, 2.7), 0.30 @ (0.0, 0.0)),
    prop!(Trunk, "00", 6.2, (6.8, 6.9), 1.48 @ (1.20, -0.14)),
    prop!(Trunk, "17", 3.0, (3.8, 4.2), 1.85 @ (0.13, -0.19)),
    prop!(Stump, "30", 2.8, (2.8, 2.4), 0.74 @ (-0.12, -0.17)),
    prop!(Stump, "59", 2.0, (1.6, 2.1), 0.52 @ (0.15, 0.0)),
    prop!(Stump, "76", 1.1, (1.6, 1.3), 0.55 @ (-0.01, 0.07)),
    prop!(Stump, "94", 1.2, (1.1, 1.1), 0.12 @ (0.31, -0.01)),
    prop!(Log, "11", 1.4, (6.2, 5.1), 0.96 @ (2.06, -1.35)),
    prop!(Log, "15", 1.0, (4.1, 8.8), 1.55 @ (1.49, -1.76)),
    prop!(Log, "33", 1.9, (5.9, 4.2), 0.83 @ (1.92, -0.78)),
    prop!(Log, "34", 0.9, (3.3, 7.1), 2.42 @ (-0.18, 0.43)),
    prop!(Log, "38", 2.2, (4.1, 2.6), 0.71 @ (1.37, -0.38)),
    prop!(Log, "40", 1.0, (5.8, 4.0), 0.75 @ (2.02, -1.32)),
    prop!(Branch, "42", 1.3, (2.3, 3.4), 0.13 @ (0.24, -1.24)),
    prop!(Branch, "48", 0.4, (0.4, 5.3), 0.0 @ (0.0, 0.0)),
    prop!(Branch, "55", 0.4, (0.6, 3.4), 0.0 @ (0.0, 0.0)),
    prop!(Branch, "69", 1.1, (1.7, 3.5), 0.59 @ (0.35, 1.22)),
    prop!(Branch, "73", 0.5, (1.7, 3.5), 1.09 @ (0.19, -0.28)),
    prop!(Branch, "84", 2.1, (1.5, 0.8), 0.11 @ (-0.02, 0.16)),
    prop!(Branch, "86", 1.2, (1.9, 2.2), 0.27 @ (0.47, -0.57)),
    prop!(Branch, "90", 2.2, (0.9, 0.7), 0.11 @ (0.17, 0.03)),
    prop!(Branch, "96", 1.6, (1.0, 0.9), 0.10 @ (-0.28, 0.11)),
    prop!(Branch, "98", 1.1, (1.0, 1.6), 0.22 @ (0.36, -0.46)),
    prop!(Fungus, "19", 3.3, (1.2, 1.8), 0.17 @ (0.08, -0.53)),
    prop!(Fungus, "28", 3.9, (2.8, 2.2), 0.38 @ (-0.44, -0.24)),
    prop!(Fungus, "41", 3.0, (1.1, 2.2), 0.12 @ (-0.10, 0.50)),
    prop!(Fungus, "39", 2.1, (3.4, 2.2), 1.01 @ (-0.22, 0.09)),
    prop!(Fungus, "68", 1.4, (1.2, 1.2), 0.18 @ (-0.02, 0.40)),
    prop!(Fungus, "81", 1.5, (0.7, 0.7), 0.17 @ (0.14, -0.01)),
    prop!(Fungus, "99", 0.7, (1.1, 1.4), 0.39 @ (0.35, 0.24)),
    prop!(Moss, "43", 1.2, (1.1, 1.3), 0.36 @ (-0.05, 0.37)),
    prop!(Moss, "66", 0.7, (1.1, 1.0), 0.42 @ (-0.22, -0.27)),
    prop!(Rock, "21", 1.4, (4.6, 4.1), 2.02 @ (-0.01, -0.21)),
    prop!(Rock, "24", 1.8, (4.4, 3.4), 1.65 @ (1.36, -0.25)),
    prop!(Rock, "51", 1.1, (1.8, 2.6), 0.41 @ (0.69, 0.70)),
    prop!(Rock, "74", 0.9, (2.6, 1.8), 0.73 @ (0.60, 0.07)),
    prop!(Rock, "80", 1.0, (1.8, 2.0), 0.42 @ (0.59, -0.26)),
    prop!(Rock, "87", 0.6, (1.6, 2.1), 0.97 @ (-0.26, -0.46)),
    prop!(Rock, "89", 0.9, (2.0, 1.7), 0.84 @ (0.39, -0.05)),
    prop!(Rock, "91", 0.6, (1.9, 1.7), 0.75 @ (0.47, 0.17)),
    prop!(Rock, "100", 0.6, (1.4, 1.3), 0.39 @ (0.05, 0.07)),
    prop!(Rock, "103", 0.4, (1.4, 1.4), 0.49 @ (0.20, 0.10)),
    prop!(Mound, "04", 2.1, (6.7, 11.0), 3.54 @ (0.04, -0.38)),
    prop!(Mound, "23", 1.8, (4.2, 5.6), 2.13 @ (0.39, 0.22)),
    prop!(Mound, "46", 1.4, (2.3, 2.7), 0.74 @ (0.46, 0.39)),
    prop!(Mound, "44", 0.5, (1.6, 3.8), 0.95 @ (0.20, -0.48)),
    prop!(Debris, "07", 1.4, (5.1, 4.7), 1.19 @ (1.09, 0.43)),
    prop!(Debris, "20", 2.3, (2.0, 2.6), 0.57 @ (0.19, -0.16)),
    prop!(Debris, "32", 1.8, (2.6, 8.6), 0.97 @ (0.40, 2.74)),
    prop!(Debris, "52", 1.2, (1.8, 2.2), 0.52 @ (0.52, -0.05)),
    prop!(Debris, "102", 0.6, (2.0, 1.6), 0.24 @ (0.56, 0.56)),
];

/// A prop of a group picked by `group_roll`, then one of its pieces.
fn pick(group_roll: f32, piece_roll: f32) -> usize {
    let total: f32 = Group::ALL.iter().map(|group| group.share()).sum();
    let mut left = group_roll * total;
    let group = Group::ALL
        .into_iter()
        .find(|group| {
            left -= group.share();
            left < 0.0
        })
        .unwrap_or(Group::Debris);
    let members: Vec<_> = (0..KIT.len()).filter(|&i| KIT[i].group == group).collect();
    members[((piece_roll * members.len() as f32) as usize).min(members.len() - 1)]
}

/// Off the roads, the rail bed, the sites and the cordon walls.
fn clear(terrain: &Terrain, at: Vec2, radius: f32) -> bool {
    !super::sites::clear_footprint(at, radius + 2.0)
        && !reserve_scenery(at, radius)
        && [Vec2::ZERO, Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
            .into_iter()
            .all(|side| terrain.road_mask(at.x + side.x * radius, at.y + side.y * radius) == 0.0)
}

/// Every prop in the marsh: which piece and where.
fn scatter(terrain: &Terrain) -> Vec<(usize, Transform)> {
    let reach = Vec2::new(MARSH_RADII.0, MARSH_RADII.1);
    let first = ((SWAMP - reach) / CELL).floor().as_ivec2();
    let last = ((SWAMP + reach) / CELL).ceil().as_ivec2();
    let mut props = Vec::new();
    for z in first.y..last.y {
        for x in first.x..last.x {
            let roll = |n| lattice_hash(x, z, SEED + n);
            let at = (Vec2::new(x as f32, z as f32) + Vec2::new(roll(1), roll(2))) * CELL;
            if roll(0) >= DENSITY || marsh_distance(at.x, at.y) > 1.0 {
                continue;
            }
            let index = pick(roll(3), roll(4));
            let prop = &KIT[index];
            let scale = prop.group.scale() * lerp(0.85, 1.15, roll(5));
            let radius = prop.reach * scale / SCALE;
            let depth = terrain.water_depth_at(at.x, at.y);
            let wade = if prop.group.wades() { WADE } else { 0.0 };
            if depth > wade || !clear(terrain, at, radius) {
                continue;
            }
            // On the lowest ground under the footprint, so nothing floats.
            let ground = [Vec2::ZERO, Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
                .into_iter()
                .map(|side| terrain.mesh_height_at(at + side * radius * 0.5))
                .fold(f32::INFINITY, f32::min);
            let pose = Transform::from_xyz(at.x, ground - 0.05, at.y)
                .with_rotation(Quat::from_rotation_y(roll(6) * TAU))
                .with_scale(Vec3::splat(scale));
            props.push((index, pose));
        }
    }
    props
}

pub(super) fn preload_marsh(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for prop in &KIT {
        let (mesh, paint) = assets::load_single_primitive(&server, prop.near);
        let (distant, _) = assets::load_single_primitive(&server, prop.far);
        preloaded
            .0
            .extend([mesh.untyped(), paint.untyped(), distant.untyped()]);
    }
}

pub(super) fn spawn_marsh(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Both tiers of a prop share the full scan's material.
    let tiers: Vec<_> = KIT
        .iter()
        .map(|prop| {
            let (mesh, paint) = assets::load_single_primitive(&server, prop.near);
            let (distant, _) = assets::load_single_primitive(&server, prop.far);
            let paint = paints.get(&paint).expect("preloaded swamp material");
            let paint = MeshMaterial3d(materials.add(standard_material_from_gltf_material(paint)));
            ((Mesh3d(mesh), paint.clone()), (Mesh3d(distant), paint))
        })
        .collect();
    let near = detail_range(NEAR, FADE);
    let far = VisibilityRange {
        start_margin: near.end_margin.clone(),
        ..detail_range(FAR, FADE)
    };
    let root = commands
        .spawn((
            WorldRoot,
            Name::new("Marsh"),
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    for (index, pose) in scatter(&terrain) {
        let prop = &KIT[index];
        let (full, simple) = &tiers[index];
        let close = commands
            .spawn((full.clone(), pose, near.clone(), ChildOf(root)))
            .id();
        let distant = commands
            .spawn((simple.clone(), pose, far.clone(), ChildOf(root)))
            .id();
        if let Some(pillar) = &prop.pillar {
            commands
                .entity(close)
                .insert(Collider(std::slice::from_ref(pillar)));
        }
        if prop.height * pose.scale.y / SCALE < SHADOW_HEIGHT {
            for entity in [close, distant] {
                commands.entity(entity).insert(NotShadowCaster);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marsh_holds_every_group_off_the_roads_and_out_of_deep_pools() {
        let terrain = Terrain::new();
        let props = scatter(&terrain);
        assert!((250..900).contains(&props.len()), "{} props", props.len());
        for group in Group::ALL {
            assert!(
                props.iter().any(|&(i, _)| KIT[i].group == group),
                "no {group:?}"
            );
        }
        let wading = props
            .iter()
            .filter(|(_, pose)| {
                let at = pose.translation;
                terrain.water_depth_at(at.x, at.z) > 0.05
            })
            .count();
        assert!(wading > 10, "{wading} props stand in the pools");
        for (index, pose) in &props {
            let at = pose.translation.xz();
            let prop = &KIT[*index];
            assert!(marsh_distance(at.x, at.y) <= 1.0);
            assert_eq!(terrain.road_mask(at.x, at.y), 0.0);
            let depth = terrain.water_depth_at(at.x, at.y);
            assert!(depth <= if prop.group.wades() { WADE } else { 0.0 });
            assert!(pose.translation.y <= terrain.mesh_height_at(at));
        }
    }

    #[test]
    fn props_too_tall_to_step_over_are_solid() {
        for prop in &KIT {
            assert_eq!(prop.pillar.is_some(), prop.height > STEP_HEIGHT);
            if let Some(pillar) = &prop.pillar {
                assert!(pillar.radius > 0.0 && (pillar.top * SCALE - prop.height).abs() < 1e-4);
            }
        }
    }
}
