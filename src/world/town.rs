//! New Harmont: buildings cut from the Harmont town scan, lined along a street
//! grid around a paved downtown plaza on one level town pad (the map's "New
//! Harmont" box). Each building shows its full scan near the view and a light
//! version farther out; the scans are small enough to stay loaded.

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::{self, PreloadedAssets};

use super::WorldRoot;
use super::collision::{Collider, Pillar};
use super::places::Place;
use super::terrain::{TILE_SIZE, landform_height, smoothstep};

/// The plaza at the middle of the town (map px 174, 214), and its paved half-size.
pub(super) const PLAZA: Vec2 = Vec2::new(1274.0, 406.0);
const PLAZA_HALF: Vec2 = Vec2::new(46.0, 34.0);
/// The avenue's ends inside the town; it runs from the Institute's street
/// through the plaza.
pub(super) const AVENUE: [Vec2; 2] = [Vec2::new(1110.0, 406.0), Vec2::new(1440.0, 406.0)];
const NORTH: Vec2 = Vec2::new(PLAZA.x, 520.0);
const SOUTH: Vec2 = Vec2::new(PLAZA.x, 290.0);
/// The boulevard north and south from the plaza, and the ring street around
/// the town's four blocks.
pub(super) const STREETS: [&[Vec2]; 4] = [
    &[PLAZA, NORTH],
    &[PLAZA, SOUTH],
    &[
        NORTH,
        Vec2::new(AVENUE[0].x, NORTH.y),
        Vec2::new(AVENUE[0].x, SOUTH.y),
        SOUTH,
    ],
    &[
        NORTH,
        Vec2::new(AVENUE[1].x, NORTH.y),
        Vec2::new(AVENUE[1].x, SOUTH.y),
        SOUTH,
    ],
];

/// The level town pad, a little wider than the street grid, and its slope back
/// to the land.
const PAD_HALF: Vec2 = Vec2::new(190.0, 140.0);
const PAD_BLEND: f32 = 60.0;
/// The scan's scale: its pieces were cut at the town diorama's size.
const SCALE: f32 = 170.0;
/// How far the buildings sink into the pad, so no edge floats.
const SINK: f32 = 0.12;
/// The full scans give way to the light ones beyond this distance.
const NEAR: f32 = 230.0;
const FADE: f32 = 30.0;

/// A piece of the town scan, centered on its footprint with its lowest point
/// at the origin.
struct Piece {
    number: u8,
    near: &'static str,
    far: &'static str,
    /// Half the footprint along model x and z, in meters.
    half: Vec2,
    height: f32,
    pillars: &'static [Pillar],
}

macro_rules! piece {
    ($n:literal, $hx:literal, $hz:literal, $h:literal) => {
        Piece {
            number: $n,
            near: concat!("models/harmont/uhd/harmont_", stringify!($n), ".glb"),
            far: concat!("models/harmont/medium/harmont_", stringify!($n), ".glb"),
            half: Vec2::new($hx, $hz),
            height: $h,
            pillars: include!(concat!("pillars/harmont_", stringify!($n), ".rs")),
        }
    };
}

#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
#[allow(
    clippy::zero_prefixed_literal,
    reason = "Piece numbers spell their zero-padded file names"
)]
const PIECES: [Piece; 17] = [
    piece!(05, 10.9, 25.3, 18.5),
    piece!(06, 14.8, 14.0, 18.3),
    piece!(08, 6.9, 25.3, 21.1),
    piece!(09, 10.7, 16.6, 17.3),
    piece!(10, 8.0, 23.5, 16.1),
    piece!(11, 5.2, 4.1, 11.7),
    piece!(12, 6.4, 15.1, 10.4),
    piece!(14, 4.8, 9.5, 12.6),
    piece!(15, 13.4, 24.9, 24.4),
    piece!(16, 13.8, 17.8, 16.0),
    piece!(17, 3.9, 21.5, 15.6),
    piece!(18, 6.0, 8.8, 18.0),
    piece!(19, 3.6, 9.4, 5.3),
    piece!(20, 3.7, 8.0, 6.3),
    piece!(22, 4.9, 3.9, 6.1),
    piece!(24, 4.9, 5.6, 6.4),
    piece!(27, 3.5, 3.2, 8.3),
];

/// A building: which piece, where, and its turn in degrees.
#[derive(Clone, Copy)]
struct Building {
    piece: u8,
    at: Vec2,
    yaw: f32,
}

impl Building {
    const fn new(piece: u8, x: f32, z: f32, yaw: f32) -> Self {
        Self {
            piece,
            at: Vec2::new(x, z),
            yaw,
        }
    }

    fn piece(self) -> &'static Piece {
        PIECES
            .iter()
            .find(|piece| piece.number == self.piece)
            .expect("every building uses a listed piece")
    }

    /// Half the footprint along world x and z.
    fn half(self) -> Vec2 {
        let half = self.piece().half;
        if self.yaw % 180.0 == 0.0 {
            half
        } else {
            half.yx()
        }
    }

    fn pose(self) -> Transform {
        Transform::from_xyz(self.at.x, pad_level() - SINK, self.at.y)
            .with_rotation(Quat::from_rotation_y(self.yaw.to_radians()))
            .with_scale(Vec3::splat(SCALE))
    }
}

/// Tall buildings face the plaza, mid-rise ones the avenue and the boulevard,
/// and low houses the ring street (laid out by footprint, 3 m apart at least).
const BUILDINGS: [Building; 53] = [
    // Around the plaza.
    Building::new(15, 1312.4, 455.4, 90.0),
    Building::new(6, 1302.3, 356.0, 180.0),
    Building::new(9, 1332.8, 353.4, 180.0),
    Building::new(5, 1235.2, 452.9, 90.0),
    Building::new(18, 1251.7, 364.0, 270.0),
    Building::new(15, 1224.4, 345.1, 180.0),
    Building::new(6, 1336.0, 490.3, 90.0),
    Building::new(5, 1347.3, 321.6, 270.0),
    Building::new(8, 1200.7, 426.4, 90.0),
    Building::new(15, 1201.1, 483.8, 90.0),
    // Along the avenue and the boulevard.
    Building::new(12, 1302.6, 490.4, 90.0),
    Building::new(14, 1333.5, 424.3, 90.0),
    Building::new(16, 1365.8, 433.3, 90.0),
    Building::new(10, 1396.5, 443.0, 0.0),
    Building::new(17, 1413.3, 441.0, 0.0),
    Building::new(11, 1291.6, 326.8, 270.0),
    Building::new(14, 1333.5, 387.7, 270.0),
    Building::new(16, 1365.8, 378.7, 270.0),
    Building::new(10, 1396.5, 369.0, 180.0),
    Building::new(17, 1413.3, 371.0, 180.0),
    Building::new(11, 1256.4, 485.2, 90.0),
    Building::new(12, 1156.9, 425.9, 90.0),
    Building::new(14, 1132.0, 429.0, 0.0),
    Building::new(11, 1256.4, 314.8, 270.0),
    Building::new(12, 1204.9, 386.1, 270.0),
    Building::new(14, 1175.3, 387.7, 270.0),
    Building::new(16, 1143.0, 378.7, 270.0),
    // Houses on the ring street.
    Building::new(27, 1307.0, 503.3, 0.0),
    Building::new(27, 1359.1, 503.3, 0.0),
    Building::new(19, 1381.1, 502.9, 90.0),
    Building::new(24, 1405.1, 501.6, 90.0),
    Building::new(19, 1422.9, 476.9, 0.0),
    Building::new(24, 1420.9, 500.3, 90.0),
    Building::new(22, 1292.4, 307.4, 180.0),
    Building::new(20, 1310.0, 311.5, 180.0),
    Building::new(20, 1386.6, 307.2, 270.0),
    Building::new(27, 1407.1, 306.7, 180.0),
    Building::new(20, 1422.8, 336.5, 180.0),
    Building::new(27, 1423.3, 316.1, 270.0),
    Building::new(19, 1251.1, 502.9, 90.0),
    Building::new(24, 1167.0, 501.6, 90.0),
    Building::new(22, 1147.5, 502.6, 0.0),
    Building::new(20, 1129.9, 498.5, 0.0),
    Building::new(19, 1127.1, 452.9, 0.0),
    Building::new(24, 1128.4, 477.0, 0.0),
    Building::new(20, 1240.5, 307.2, 270.0),
    Building::new(27, 1220.1, 306.7, 180.0),
    Building::new(19, 1198.1, 307.1, 270.0),
    Building::new(24, 1174.0, 308.4, 270.0),
    Building::new(22, 1154.5, 307.4, 180.0),
    Building::new(20, 1132.6, 307.2, 270.0),
    Building::new(22, 1127.4, 355.6, 270.0),
    Building::new(20, 1127.2, 333.7, 180.0),
];

/// The town pad's height: the land at the plaza.
fn pad_level() -> f32 {
    landform_height(PLAZA.x, PLAZA.y)
}

/// The town pad's height at `at` and how strongly it holds there.
pub(super) fn foundation(at: Vec2) -> Option<(f32, f32)> {
    let outside = ((at - PLAZA).abs() - PAD_HALF).max(Vec2::ZERO).length();
    (outside < PAD_BLEND).then(|| (pad_level(), smoothstep(PAD_BLEND, 0.0, outside)))
}

/// Inside the town pad.
pub(super) fn contains(at: Vec2) -> bool {
    (at - PLAZA).abs().cmple(PAD_HALF).all()
}

/// On the plaza's paving.
pub(super) fn paved(at: Vec2) -> bool {
    (at - PLAZA).abs().cmple(PLAZA_HALF).all()
}

/// On the plaza or within `clearance` of a building.
pub(super) fn clear_footprint(at: Vec2, clearance: f32) -> bool {
    paved(at) || near_building(at, clearance)
}

/// Within `clearance` of a building.
pub(super) fn near_building(at: Vec2, clearance: f32) -> bool {
    BUILDINGS.into_iter().any(|building| {
        (at - building.at)
            .abs()
            .cmple(building.half() + clearance)
            .all()
    })
}

/// Each building's center, radius and height, for hover outlines.
pub(super) fn outlines() -> impl Iterator<Item = (Vec2, f32, f32, Place)> {
    BUILDINGS.into_iter().map(|building| {
        (
            building.at,
            building.half().max_element(),
            building.piece().height,
            Place::Town,
        )
    })
}

pub(super) fn preload_town(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for piece in &PIECES {
        for path in [piece.near, piece.far] {
            let (mesh, paint) = assets::load_single_primitive(&server, path);
            preloaded.0.extend([mesh.untyped(), paint.untyped()]);
        }
    }
}

pub(super) fn spawn_town(
    mut commands: Commands,
    server: Res<AssetServer>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut load = |path| {
        let (mesh, paint) = assets::load_single_primitive(&server, path);
        let paint = paints.get(&paint).expect("preloaded town material");
        (
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(standard_material_from_gltf_material(paint))),
        )
    };
    let pieces = PIECES
        .each_ref()
        .map(|piece| (load(piece.near), load(piece.far)));
    let near = VisibilityRange {
        end_margin: NEAR..NEAR + FADE,
        use_aabb: true,
        ..default()
    };
    let far = VisibilityRange {
        start_margin: NEAR..NEAR + FADE,
        ..near.clone()
    };
    let far = VisibilityRange {
        end_margin: 1.0e5..1.0e5,
        ..far
    };
    for building in BUILDINGS {
        let index = PIECES
            .iter()
            .position(|piece| piece.number == building.piece)
            .expect("every building uses a listed piece");
        let (full, light) = pieces[index].clone();
        commands.spawn((
            WorldRoot,
            Name::new("Harmont building"),
            building.pose(),
            Visibility::default(),
            children![(full, near.clone()), (light, far.clone())],
        ));
    }
}

/// The colliders of the buildings standing in `tile`.
pub(super) fn spawn_solids(parent: &mut ChildSpawnerCommands, tile: IVec2) {
    for building in BUILDINGS {
        if (building.at / TILE_SIZE).floor().as_ivec2() == tile {
            parent.spawn((building.pose(), Collider(building.piece().pillars)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streets() -> impl Iterator<Item = [Vec2; 2]> {
        let avenue = [Vec2::new(PLAZA.x - 500.0, PLAZA.y), AVENUE[1]];
        STREETS
            .into_iter()
            .flat_map(|street| street.array_windows().copied())
            .chain([avenue])
    }

    #[test]
    fn buildings_keep_off_the_streets_the_plaza_and_each_other() {
        for (i, building) in BUILDINGS.into_iter().enumerate() {
            let half = building.half();
            assert!(contains(building.at + half) && contains(building.at - half));
            for [a, b] in streets() {
                let along = (b - a).normalize();
                let corners = [-1.0, 1.0].map(|x| [-1.0, 1.0].map(|z| half * Vec2::new(x, z)));
                let nearest = corners
                    .into_iter()
                    .flatten()
                    .map(|corner| {
                        let at = building.at + corner;
                        let run = (at - a).dot(along).clamp(0.0, a.distance(b));
                        at.distance(a + along * run)
                    })
                    .fold(f32::INFINITY, f32::min);
                assert!(nearest > 11.0, "building {i} stands on a street");
            }
            assert!(!paved(building.at));
            for other in &BUILDINGS[..i] {
                let gap = (building.at - other.at).abs() - half - other.half();
                assert!(gap.max_element() > 2.0, "buildings {i} overlap");
            }
        }
    }

    #[test]
    fn every_piece_is_used_and_solid() {
        for piece in &PIECES {
            assert!(BUILDINGS.iter().any(|b| b.piece == piece.number));
            assert!(piece.pillars.len() > 10, "piece {}", piece.number);
        }
    }
}
