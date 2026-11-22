//! The old railway across the map (`map::RAILWAY`), laid from pieces of the
//! railway embankment scan (`assets/models/railway/uhd`, squared up by
//! `tools/align_kit_piece.py`): sleeper ladders end to end along the graded
//! bed, two rusted rails on top, and wreckage beside the line.

use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::{self, PreloadedAssets};

use super::collision::{Collider, Pillar, STEP_HEIGHT};
use super::map::RAILWAY;
use super::terrain::{Terrain, lattice_hash};
use super::{WorldRoot, detail_range};

/// Meters per scan unit along the ground, and a flatter scale upward: the
/// scan's sleepers and rails are thick for their size.
const SCALE: f32 = 80.0;
const VERTICAL: f32 = 50.0;
/// Between the two rails' centers (standard gauge).
const GAUGE: f32 = 1.435;
/// The rails' foot above the bed, on the sleepers' tops (scan units).
const RAIL_SEAT: f32 = 0.0040;
/// The rail piece's web, across from its bounding box's middle (scan units).
const RAIL_WEB: f32 = -0.0025;
/// Pieces overlap this much so the line shows no seams.
const OVERLAP: f32 = 0.97;
/// Wreckage lies this far from the line's middle, in meters.
const WRECK_OFFSET: (f32, f32) = (3.6, 5.6);
/// And this far apart along it.
const WRECK_SPACING: (f32, f32) = (60.0, 140.0);
/// No wreckage on the crossings and the sites' drives (`RAILWAY` indices).
const JUNCTIONS: [usize; 4] = [2, 5, 7, 10];
const JUNCTION_CLEARANCE: f32 = 30.0;
const SEED: u32 = 0x7A11_3E5D;

/// A scan piece: its path and its size in scan units (length along +Z).
struct Piece {
    path: &'static str,
    half: Vec2,
    height: f32,
}

macro_rules! piece {
    ($n:literal, $hx:literal, $hz:literal, $h:literal) => {
        Piece {
            path: concat!("models/railway/uhd/rail_", stringify!($n), ".glb"),
            half: Vec2::new($hx, $hz),
            height: $h,
        }
    };
}

/// Stretches of sleepers.
const LADDERS: [Piece; 3] = [
    piece!(48, 0.0172, 0.0494, 0.0055),
    piece!(56, 0.0155, 0.0372, 0.0048),
    piece!(57, 0.0180, 0.0311, 0.0066),
];
const RAIL: Piece = piece!(67, 0.0070, 0.0457, 0.0047);
/// Broken track, rocks, a switch stand and heaps of spoil.
const WRECKAGE: [Piece; 15] = [
    piece!(41, 0.0435, 0.0542, 0.0278),
    piece!(45, 0.0287, 0.0736, 0.0064),
    piece!(50, 0.0212, 0.0390, 0.0138),
    piece!(51, 0.0273, 0.0356, 0.0064),
    piece!(53, 0.0146, 0.0221, 0.0225),
    piece!(54, 0.0219, 0.0240, 0.0286),
    piece!(58, 0.0160, 0.0414, 0.0040),
    piece!(59, 0.0128, 0.0400, 0.0118),
    piece!(60, 0.0247, 0.0404, 0.0066),
    piece!(61, 0.0146, 0.0326, 0.0069),
    piece!(62, 0.0112, 0.0273, 0.0092),
    piece!(64, 0.0110, 0.0353, 0.0138),
    piece!(68, 0.0097, 0.0238, 0.0067),
    piece!(69, 0.0182, 0.0225, 0.0043),
    piece!(70, 0.0199, 0.0476, 0.0160),
];

/// Which of the kit's pieces, by its place in `kit()`.
type PieceIndex = usize;

fn kit() -> impl Iterator<Item = &'static Piece> {
    LADDERS.iter().chain([&RAIL]).chain(&WRECKAGE)
}

const RAIL_INDEX: PieceIndex = LADDERS.len();
const WRECKAGE_INDEX: PieceIndex = LADDERS.len() + 1;

/// The line's length from its south end to each of its points.
fn stations() -> Vec<f32> {
    std::iter::once(0.0)
        .chain(RAILWAY.windows(2).scan(0.0, |run, leg| {
            *run += leg[0].distance(leg[1]);
            Some(*run)
        }))
        .collect()
}

/// The point `distance` along the line, and the line's heading there.
fn along(stations: &[f32], distance: f32) -> (Vec2, Vec2) {
    let leg = stations
        .partition_point(|&station| station <= distance)
        .clamp(1, RAILWAY.len() - 1);
    let (a, b) = (RAILWAY[leg - 1], RAILWAY[leg]);
    let heading = (b - a).normalize();
    (a + heading * (distance - stations[leg - 1]), heading)
}

/// A piece lying from `from` to `to`, `lift` meters above the bed, turned
/// about its middle by `flip` half turns.
fn lay_between(terrain: &Terrain, from: Vec2, to: Vec2, lift: f32, flip: bool) -> Transform {
    let middle = (from + to) / 2.0;
    let rise = terrain.height_at(to.x, to.y) - terrain.height_at(from.x, from.y);
    let heading = to - from;
    let yaw = heading.x.atan2(heading.y) + if flip { std::f32::consts::PI } else { 0.0 };
    let slope = rise / heading.length() * if flip { -1.0 } else { 1.0 };
    Transform::from_xyz(
        middle.x,
        terrain.height_at(middle.x, middle.y) + lift,
        middle.y,
    )
    .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-slope.atan()))
    .with_scale(Vec3::new(SCALE, VERTICAL, SCALE))
}

/// Every piece of the line: which piece and where.
fn lay_line(terrain: &Terrain) -> Vec<(PieceIndex, Transform)> {
    let stations = stations();
    let length = stations[stations.len() - 1];
    let mut laid = Vec::new();

    // Sleepers, a varied stretch at a time.
    let mut at = 0.0;
    for i in 0.. {
        let pick = lattice_hash(i, 0, SEED);
        let index = ((pick * LADDERS.len() as f32) as usize).min(LADDERS.len() - 1);
        let step = LADDERS[index].half.y * 2.0 * SCALE;
        let (from, _) = along(&stations, at);
        let (to, _) = along(&stations, (at + step).min(length));
        if at >= length || from.distance(to) < 0.5 {
            break;
        }
        laid.push((index, lay_between(terrain, from, to, 0.0, pick > 0.5)));
        at += step * OVERLAP;
    }

    // Two rails on the sleepers.
    let step = RAIL.half.y * 2.0 * SCALE;
    let seat = RAIL_SEAT * VERTICAL;
    let mut at = 0.0;
    for i in 0.. {
        let (from, heading) = along(&stations, at);
        let (to, _) = along(&stations, (at + step).min(length));
        if at >= length || from.distance(to) < 0.5 {
            break;
        }
        for side in [-1.0, 1.0] {
            let flip = lattice_hash(i, side as i32, SEED) > 0.5;
            // The rail's web, not its box, sits over the gauge line.
            let web = RAIL_WEB * SCALE * if flip { -1.0 } else { 1.0 };
            let offset = heading.perp() * (side * GAUGE / 2.0 + web);
            laid.push((
                RAIL_INDEX,
                lay_between(terrain, from + offset, to + offset, seat, flip),
            ));
        }
        at += step * OVERLAP;
    }

    // Wreckage beside the line, off the crossings.
    let mut at = WRECK_SPACING.0;
    for i in 0.. {
        if at >= length {
            break;
        }
        let pick = |salt| lattice_hash(i, salt, SEED);
        let (point, heading) = along(&stations, at);
        let side = if pick(1) > 0.5 { 1.0 } else { -1.0 };
        let offset = WRECK_OFFSET.0 + (WRECK_OFFSET.1 - WRECK_OFFSET.0) * pick(2);
        let spot = point + heading.perp() * side * offset;
        let clear = JUNCTIONS
            .iter()
            .all(|&j| RAILWAY[j].distance(spot) > JUNCTION_CLEARANCE)
            && !super::sites::clear_footprint(spot, 4.0)
            && terrain.water_depth_at(spot.x, spot.y) <= 0.0;
        if clear {
            let index = ((pick(3) * WRECKAGE.len() as f32) as usize).min(WRECKAGE.len() - 1);
            let yaw = heading.x.atan2(heading.y) + (pick(4) - 0.5) * 1.2;
            let pose =
                Transform::from_xyz(spot.x, terrain.height_at(spot.x, spot.y) - 0.05, spot.y)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(SCALE));
            laid.push((WRECKAGE_INDEX + index, pose));
        }
        at += WRECK_SPACING.0 + (WRECK_SPACING.1 - WRECK_SPACING.0) * pick(5);
    }
    laid
}

/// One upright pillar for wreckage too tall to step over.
const fn wreck_pillar(piece: &Piece) -> Option<Pillar> {
    if piece.height * SCALE <= STEP_HEIGHT {
        return None;
    }
    Some(Pillar {
        x: 0.0,
        z: 0.0,
        radius: piece.half.x.min(piece.half.y) * 0.8,
        bottom: 0.0,
        top: piece.height,
    })
}

static WRECK_PILLARS: [Option<Pillar>; 15] = {
    let mut pillars = [None; 15];
    let mut i = 0;
    while i < WRECKAGE.len() {
        pillars[i] = wreck_pillar(&WRECKAGE[i]);
        i += 1;
    }
    pillars
};

pub(super) fn preload_railway(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for piece in kit() {
        let (mesh, paint) = assets::load_single_primitive(&server, piece.path);
        preloaded.0.extend([mesh.untyped(), paint.untyped()]);
    }
}

pub(super) fn spawn_railway(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let looks: Vec<_> = kit()
        .map(|piece| {
            let (mesh, paint) = assets::load_single_primitive(&server, piece.path);
            let paint = paints.get(&paint).expect("preloaded railway material");
            (
                Mesh3d(mesh),
                MeshMaterial3d(materials.add(standard_material_from_gltf_material(paint))),
            )
        })
        .collect();
    let root = commands
        .spawn((
            WorldRoot,
            Name::new("Railway"),
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    for (index, pose) in lay_line(&terrain) {
        let mut piece = commands.spawn((
            looks[index].clone(),
            pose,
            detail_range(260.0, 40.0),
            ChildOf(root),
        ));
        let pillar = index
            .checked_sub(WRECKAGE_INDEX)
            .and_then(|wreck| WRECK_PILLARS[wreck].as_ref());
        if let Some(pillar) = pillar {
            piece.insert(Collider(std::slice::from_ref(pillar)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rails_run_unbroken_from_end_to_end_of_the_map() {
        let terrain = Terrain::new();
        let line = lay_line(&terrain);
        let stations = stations();
        let length = stations[stations.len() - 1];
        assert!(length > 3000.0, "line {length} m");
        let count =
            |wanted: fn(PieceIndex) -> bool| line.iter().filter(|(i, _)| wanted(*i)).count();
        let rails = count(|i| i == RAIL_INDEX);
        let rail_length = RAIL.half.y * 2.0 * SCALE * OVERLAP;
        assert!(rails as f32 * rail_length >= 2.0 * (length - rail_length));
        let ladders = count(|i| i < RAIL_INDEX);
        assert!(ladders > (length / 9.0) as usize);
        let wrecks = count(|i| i >= WRECKAGE_INDEX);
        assert!((15..60).contains(&wrecks), "{wrecks} wrecks");
    }

    #[test]
    fn every_piece_lies_on_the_bed() {
        let terrain = Terrain::new();
        for (index, pose) in lay_line(&terrain) {
            let at = pose.translation.xz();
            let ground = terrain.height_at(at.x, at.y);
            let lift = pose.translation.y - ground;
            if index < WRECKAGE_INDEX {
                assert!(
                    (-0.1..0.6).contains(&lift),
                    "piece {index} floats {lift} at {at}"
                );
            } else {
                assert!(lift.abs() < 0.1);
            }
        }
    }
}
