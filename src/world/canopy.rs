//! The map's trees: imported, textured models growing in the landmark groves,
//! three dense woods and around the hangar. There are no procedural trees.
//! Each shows its light crown-card model, and the full UHD scan when the
//! camera stands right by it. The scans stream in only near the camera.

use std::f32::consts::TAU;

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::light::NotShadowCaster;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::gpu_scans::GpuScans;
use crate::assets::{self, PreloadedAssets};
use crate::player::PlayerCamera;

use super::collision::{Collider, Pillar, Weight};
use super::cordon::{hangar_belt, inside_yard, near_wall_or_checkpoint, shelterbelt, tree_belt};
use super::hydrology::SWAMP;
use super::terrain::{
    BALD_CENTER, BASE, QUARRY_CENTER, TILE_SIZE, Terrain, lattice_hash, lerp, smoothstep,
};
use super::weather::Wind;
use super::{detail_range, range_scene_meshes};

const SEED: u32 = 0xD4EA_9021;
const SHADOW_ENTER: f32 = 48.0;
const SHADOW_EXIT: f32 = 55.0;
/// The UHD scan replaces the crown-card model this close to the camera; it
/// is far too dense to show more than the nearest few.
const UHD_RANGE: f32 = 8.0;
const UHD_FADE: f32 = 4.0;
/// A scan starts loading this far from the camera (measured to the tree's
/// middle, like its visibility range), and unloads a few meters further out.
const UHD_LOAD: f32 = UHD_RANGE + UHD_FADE + 4.0;
const UHD_DROP: f32 = UHD_LOAD + 5.0;
/// Three dense woods on the Harmont map, clear of the sites and the railway.
pub(super) const WOODS: [(Vec2, f32); 3] = [
    (Vec2::new(-336.0, 559.0), 150.0),
    (Vec2::new(300.0, -720.0), 120.0),
    (Vec2::new(348.0, 631.0), 140.0),
];
const LANDMARK_GROVES: [(Vec2, f32); 3] = [
    (Vec2::new(QUARRY_CENTER.0, QUARRY_CENTER.1), 102.0),
    (Vec2::new(BALD_CENTER.0, BALD_CENTER.1), 68.0),
    (SWAMP, 136.0),
];

pub(super) fn near_landmark(at: Vec2) -> bool {
    shelterbelt(at) > 0.0
        || at.distance_squared(Vec2::from(BASE)) <= 107.0_f32.powi(2)
        || LANDMARK_GROVES
            .into_iter()
            .any(|(center, radius)| at.distance_squared(center) <= radius * radius)
        || super::sites::grove(at) > 0.0
        || wood(at) > 0.0
}

/// How deep into one of the dense woods `at` lies, from 0.0 to 1.0.
pub(super) fn wood(at: Vec2) -> f32 {
    WOODS.into_iter().fold(0.0, |amount, (center, radius)| {
        amount.max(smoothstep(
            1.0,
            0.45,
            at.distance_squared(center) / (radius * radius),
        ))
    })
}

pub(super) fn landmark_grove(at: Vec2) -> f32 {
    LANDMARK_GROVES
        .into_iter()
        .fold(super::sites::grove(at), |amount, (center, radius)| {
            amount.max(smoothstep(
                1.0,
                0.33,
                at.distance_squared(center) / (radius * radius),
            ))
        })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Species {
    Small,
    Broadleaf,
    Tall,
    Birch,
    Dead,
}

impl Species {
    pub(super) const ALL: [Self; 5] = [
        Self::Small,
        Self::Broadleaf,
        Self::Tall,
        Self::Birch,
        Self::Dead,
    ];

    fn model(self) -> &'static TreeModel {
        &MODELS[self as usize]
    }
}

struct TreeModel {
    path: &'static str,
    /// The full scan, in the same frame as `path`.
    close: &'static str,
    scale: (f32, f32),
    footprint: f32,
    wind_bend: f32,
    draw_distance: f32,
    /// Kilograms per cubic meter of height: a tree's weight grows with the
    /// cube of its height.
    density: f32,
    trunks: &'static [Pillar],
}

const MODELS: [TreeModel; Species::ALL.len()] = [
    TreeModel {
        path: assets::TREE_SMALL,
        close: assets::TREE_SMALL_UHD,
        scale: (4.8, 7.5),
        footprint: 0.34,
        wind_bend: 0.034,
        draw_distance: 125.0,
        density: 1.0,
        trunks: &[Pillar {
            x: 0.004,
            z: 0.001,
            radius: 0.017,
            bottom: -0.035,
            top: 0.35,
        }],
    },
    TreeModel {
        path: assets::TREE_BIG,
        close: assets::TREE_BIG_UHD,
        scale: (13.5, 19.5),
        footprint: 0.22,
        wind_bend: 0.017,
        draw_distance: 195.0,
        density: 2.0,
        trunks: &[Pillar {
            x: 0.019,
            z: 0.022,
            radius: 0.083,
            bottom: -0.035,
            top: 0.20,
        }],
    },
    TreeModel {
        path: assets::TREE_VERY_TALL,
        close: assets::TREE_VERY_TALL_UHD,
        scale: (25.5, 33.5),
        footprint: 0.05,
        wind_bend: 0.021,
        draw_distance: 335.0,
        density: 0.35,
        trunks: &[Pillar {
            x: 0.008,
            z: -0.004,
            radius: 0.024,
            bottom: -0.035,
            top: 0.15,
        }],
    },
    TreeModel {
        path: assets::TREE_BIRCH,
        close: assets::TREE_BIRCH_UHD,
        scale: (10.0, 15.0),
        footprint: 0.047,
        wind_bend: 0.028,
        draw_distance: 175.0,
        density: 0.45,
        trunks: &[Pillar {
            x: 0.005,
            z: 0.0,
            radius: 0.032,
            bottom: -0.035,
            top: 0.38,
        }],
    },
    TreeModel {
        path: assets::TREE_DEAD,
        close: assets::TREE_DEAD_UHD,
        scale: (9.5, 14.5),
        footprint: 0.22,
        wind_bend: 0.0,
        draw_distance: 155.0,
        density: 0.25,
        trunks: &[Pillar {
            x: -0.014,
            z: 0.045,
            radius: 0.070,
            bottom: -0.035,
            top: 0.40,
        }],
    },
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct ForestTree {
    pub species: Species,
    pub pose: Transform,
}

impl ForestTree {
    /// An imported tree on a scenery tree spot near a landmark; open country
    /// away from them stays treeless.
    pub(super) fn at_tree_spot(terrain: &Terrain, spot: Transform, seed: u32) -> Option<Self> {
        let roll = |n| lattice_hash(seed as i32, 0, SEED + n);
        let at = spot.translation.xz();
        if !near_landmark(at) || roll(0) >= 0.6 + 0.4 * wood(at) {
            return None;
        }
        let choice = roll(1);
        let species = if choice < 0.32 {
            Species::Small
        } else if choice < 0.52 {
            Species::Broadleaf
        } else if choice < 0.70 {
            Species::Tall
        } else if choice < 0.88 {
            Species::Birch
        } else {
            Species::Dead
        };
        let spec = species.model();
        let scale = lerp(spec.scale.0, spec.scale.1, roll(2));
        Self::grounded(
            terrain,
            species,
            spot.translation.xz(),
            spot.rotation,
            scale,
            false,
        )
    }

    fn grounded(
        terrain: &Terrain,
        species: Species,
        at: Vec2,
        rotation: Quat,
        scale: f32,
        allow_base_pad: bool,
    ) -> Option<Self> {
        let radius = species.model().footprint * scale;
        let edge = (0..8).map(|i| at + Vec2::from_angle(i as f32 * TAU / 8.0) * radius);
        let center = terrain.mesh_height_at(at);
        let mut low = center;
        let mut high = center;
        for root in std::iter::once(at).chain(edge) {
            if terrain.road_mask(root.x, root.y) > 0.02
                || (!allow_base_pad && terrain.base_mask(root.x, root.y) > 0.05)
                || terrain.water_depth_at(root.x, root.y) > 0.0
            {
                return None;
            }
            let y = terrain.mesh_height_at(root);
            low = low.min(y);
            high = high.max(y);
        }
        if high - low > scale * 0.09 {
            return None;
        }
        Some(Self {
            species,
            pose: Transform::from_xyz(at.x, low - 0.05, at.y)
                .with_rotation(rotation)
                .with_scale(Vec3::splat(scale)),
        })
    }

    pub(super) fn collider(self) -> Collider {
        Collider(self.species.model().trunks)
    }

    /// From over 100 kg for a sapling, more than the stalker, to 13–15 t for
    /// the biggest broadleaves and conifers, which stop an armored car.
    pub(super) fn weight(self) -> Weight {
        Weight(self.species.model().density * self.pose.scale.y.powi(3))
    }
}

/// The belts' trees: behind and beside the hangar, and along the patrolled
/// stretch of the Perimeter (`tree_belt`).
pub(super) fn belt_trees(terrain: &Terrain, tile: IVec2) -> Vec<ForestTree> {
    const CELLS: i32 = 12;
    let start = tile * CELLS;
    let cell = TILE_SIZE / CELLS as f32;
    let mut trees = Vec::new();
    for z in start.y..start.y + CELLS {
        for x in start.x..start.x + CELLS {
            let roll = |n| lattice_hash(x, z, SEED + 431 + n);
            let at = (Vec2::new(x as f32, z as f32)
                + Vec2::new(lerp(0.24, 0.76, roll(0)), lerp(0.24, 0.76, roll(1))))
                * cell;
            if roll(2) >= tree_belt(at)
                || near_wall_or_checkpoint(at, 2.6)
                || super::sites::clear_footprint(at, 4.0)
                || terrain.normal_at(at.x, at.y).y < 0.88
            {
                continue;
            }
            let index = (roll(3) * Species::ALL.len() as f32) as usize;
            let species = Species::ALL[index.min(Species::ALL.len() - 1)];
            let spec = species.model();
            let scale = lerp(spec.scale.0, spec.scale.1, roll(4))
                * if inside_yard(at) { 0.84 } else { 1.0 };
            trees.extend(ForestTree::grounded(
                terrain,
                species,
                at,
                Quat::from_rotation_y(roll(5) * TAU),
                scale,
                hangar_belt(at) > 0.0,
            ));
        }
    }
    trees
}

#[derive(Component)]
pub(super) struct CanopyVisual {
    rest: Quat,
    phase: f32,
    bend: f32,
    /// The species' full scan.
    close: &'static str,
}

impl CanopyVisual {
    fn new(tree: ForestTree) -> Self {
        let cell = tree.pose.translation.xz().floor().as_ivec2();
        let roll = |n| lattice_hash(cell.x, cell.y, SEED + n);
        Self {
            rest: tree.pose.rotation,
            phase: roll(11) * TAU,
            bend: if roll(10) < 0.65 {
                tree.species.model().wind_bend * lerp(0.55, 1.0, roll(12))
            } else {
                0.0
            },
            close: tree.species.model().close,
        }
    }

    /// Leans downwind, further in a gust, and stirs even in still air.
    fn displaced(&self, wind: &Wind, distance: f32) -> Quat {
        let bend = self.bend * (1.0 - smoothstep(60.0, 85.0, distance));
        if bend == 0.0 {
            return self.rest;
        }
        let seconds = wind.seconds;
        let stir = bend * (0.35 + wind.strength());
        let sway = (seconds * 1.12 + self.phase).sin();
        let side = (seconds * 0.76 + self.phase).cos();
        let flutter = Quat::from_euler(EulerRot::XYZ, stir * sway * 0.39, 0.0, stir * side * 0.27);
        let downwind = wind.direction();
        // Tips the crown (+Y) toward the downwind side.
        let lean = Quat::from_axis_angle(
            Vec3::new(downwind.y, 0.0, -downwind.x),
            bend * (0.15 + 0.6 * wind.strength()),
        );
        lean * flutter * self.rest
    }
}

/// Each species' crown-card mesh and material.
#[derive(Resource, Default)]
pub(super) struct CanopyModels([(Handle<Mesh>, Handle<StandardMaterial>); MODELS.len()]);

/// The tree's full scan, a child entity loaded while the camera is near.
#[derive(Component)]
pub(super) struct CloseScan(Entity);

/// The trees' full scans, for the RAM fill.
pub(super) fn uhd_files() -> impl Iterator<Item = &'static str> {
    MODELS.iter().map(|model| model.close)
}

/// Pins the five trees' full scans: they load first, the entry curtain waits
/// for them, and no other scan makes them leave the GPU.
pub(super) fn pin_tree_scans(mut scans: ResMut<GpuScans>) {
    for path in uhd_files() {
        scans.pin(path);
    }
}

/// Preloads the crown-card models; the full scans stream in near the camera.
pub(super) fn preload_canopies(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for spec in &MODELS {
        let (mesh, paint) = assets::load_single_primitive(&server, spec.path);
        preloaded.0.extend([mesh.untyped(), paint.untyped()]);
    }
}

pub(super) fn prepare_canopy_materials(
    mut commands: Commands,
    server: Res<AssetServer>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut look = |path| {
        let (mesh, paint) = assets::load_single_primitive(&server, path);
        let paint = paints
            .get(&paint)
            .expect("tree materials finish preloading before the Zone opens");
        (
            mesh,
            materials.add(standard_material_from_gltf_material(paint)),
        )
    };
    commands.insert_resource(CanopyModels(MODELS.map(|spec| look(spec.path))));
}

pub(super) fn spawn_canopies(
    parent: &mut ChildSpawnerCommands,
    models: &CanopyModels,
    sites: &[ForestTree],
) {
    for &tree in sites {
        let (mesh, paint) = &models.0[tree.species as usize];
        let distance = tree.species.model().draw_distance;
        parent.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(paint.clone()),
            tree.pose,
            detail_range(distance, 25.0),
            CanopyVisual::new(tree),
            NotShadowCaster,
        ));
    }
}

/// Distance from `camera` to the middle of the tree standing at `pose`.
fn scan_distance(pose: &Transform, camera: Vec3) -> f32 {
    pose.transform_point(Vec3::Y * 0.5).distance(camera)
}

fn close_range() -> VisibilityRange {
    VisibilityRange {
        end_margin: UHD_RANGE..UHD_RANGE + UHD_FADE,
        use_aabb: true,
        ..default()
    }
}

type StreamedTree<'a> = (
    Entity,
    &'a CanopyVisual,
    &'a Transform,
    &'a mut VisibilityRange,
    Option<&'a CloseScan>,
);

/// Loads a tree's full scan as the camera comes near and drops it once the
/// camera moves away, so only the scans in view take GPU memory. The scan is
/// a child, so it sways and falls with the tree.
pub(super) fn stream_close_trees(
    mut commands: Commands,
    server: Res<AssetServer>,
    camera: Single<&Transform, With<PlayerCamera>>,
    mut trees: Query<StreamedTree, Without<PlayerCamera>>,
) {
    for (tree, visual, pose, mut range, scan) in &mut trees {
        let distance = scan_distance(pose, camera.translation);
        match scan {
            None if distance < UHD_LOAD => {
                let scan = commands
                    .spawn((
                        WorldAssetRoot(
                            server.load(GltfAssetLabel::Scene(0).from_asset(visual.close)),
                        ),
                        close_range(),
                        Visibility::Hidden,
                        ChildOf(tree),
                    ))
                    .observe(range_scene_meshes)
                    .observe(reveal_close_tree)
                    .id();
                commands.entity(tree).insert(CloseScan(scan));
            }
            Some(&CloseScan(scan)) if distance > UHD_DROP => {
                commands.entity(scan).despawn();
                commands.entity(tree).remove::<CloseScan>();
                range.start_margin = 0.0..0.0;
            }
            _ => {}
        }
    }
}

/// Shows a loaded full scan and lets the crown-card model fade out where
/// the scan fades in. Until then the crown-card model stays, so the tree
/// never vanishes while its scan loads.
fn reveal_close_tree(
    ready: On<WorldInstanceReady>,
    parents: Query<&ChildOf>,
    mut ranges: Query<&mut VisibilityRange, With<CanopyVisual>>,
    mut commands: Commands,
) {
    commands.entity(ready.entity).insert(Visibility::Inherited);
    let Ok(tree) = parents.get(ready.entity) else {
        return;
    };
    if let Ok(mut range) = ranges.get_mut(tree.parent()) {
        range.start_margin = close_range().end_margin;
    }
}

pub(super) fn update_canopy_motion(
    mut commands: Commands,
    wind: Res<Wind>,
    camera: Single<&Transform, With<PlayerCamera>>,
    mut trees: Query<
        (Entity, &CanopyVisual, &mut Transform, Has<NotShadowCaster>),
        Without<PlayerCamera>,
    >,
) {
    for (entity, visual, mut pose, no_shadow) in &mut trees {
        let distance_sq = pose.translation.distance_squared(camera.translation);
        if no_shadow && distance_sq < SHADOW_ENTER * SHADOW_ENTER {
            commands.entity(entity).remove::<NotShadowCaster>();
        } else if !no_shadow && distance_sq > SHADOW_EXIT * SHADOW_EXIT {
            commands.entity(entity).insert(NotShadowCaster);
        }
        if visual.bend != 0.0 {
            let rotation = visual.displaced(&wind, distance_sq.sqrt());
            if pose.rotation != rotation {
                pose.rotation = rotation;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::world_serialization::WorldAsset;

    use super::*;
    use crate::world::terrain::base_tile;

    #[test]
    fn the_stalker_fells_no_tree_and_only_big_ones_stop_a_car() {
        let weight = |species: Species, height: f32| {
            ForestTree {
                species,
                pose: Transform::from_scale(Vec3::splat(height)),
            }
            .weight()
            .0
        };
        for species in Species::ALL {
            let (lowest, tallest) = species.model().scale;
            assert!(weight(species, lowest) > 100.0, "{species:?} too light");
            let stops_cars = matches!(species, Species::Broadleaf | Species::Tall);
            assert_eq!(
                weight(species, tallest) > 12_000.0,
                stops_cars,
                "{species:?}"
            );
        }
    }

    #[test]
    fn the_five_tree_scans_are_pinned() {
        let mut app = App::new();
        app.insert_resource(GpuScans::new(1 << 30))
            .add_systems(Startup, pin_tree_scans);
        app.update();

        // Nothing is wanted, so only pinned scans load.
        let mut pinned = app.world_mut().resource_mut::<GpuScans>().plan(0.0).load;
        pinned.sort_unstable();
        let mut trees = [
            assets::TREE_SMALL_UHD,
            assets::TREE_BIG_UHD,
            assets::TREE_VERY_TALL_UHD,
            assets::TREE_BIRCH_UHD,
            assets::TREE_DEAD_UHD,
        ];
        trees.sort_unstable();
        assert_eq!(pinned, trees);
    }

    /// An app streaming the scan of a tree at `pose`, viewed from the origin.
    fn streaming(pose: Transform) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .add_systems(Update, stream_close_trees);
        let camera = app
            .world_mut()
            .spawn((PlayerCamera, Transform::IDENTITY))
            .id();
        let tree = app
            .world_mut()
            .spawn((
                CanopyVisual {
                    rest: Quat::IDENTITY,
                    phase: 0.0,
                    bend: 0.0,
                    close: assets::TREE_SMALL_UHD,
                },
                pose,
                detail_range(125.0, 25.0),
            ))
            .id();
        (app, camera, tree)
    }

    fn start_margin(app: &App, tree: Entity) -> std::ops::Range<f32> {
        let range = app.world().entity(tree).get::<VisibilityRange>();
        range.expect("tree range").start_margin.clone()
    }

    #[test]
    fn full_scans_stream_in_near_the_camera_and_out_past_a_margin() {
        // A 10 m tree whose middle stands 30 m from the camera.
        let pose = Transform::from_xyz(30.0, 0.0, 0.0).with_scale(Vec3::splat(10.0));
        let (mut app, camera, tree) = streaming(pose);
        let mut view_from = |x: f32| {
            app.world_mut()
                .entity_mut(camera)
                .insert(Transform::from_xyz(x, 5.0, 0.0));
            app.update();
            app.world()
                .entity(tree)
                .get::<CloseScan>()
                .map(|scan| scan.0)
        };
        assert_eq!(view_from(0.0), None, "far trees load no scan");
        let scan = view_from(20.0).expect("a near tree loads its scan");
        assert_eq!(view_from(12.0), Some(scan), "kept inside the margin");
        assert_eq!(view_from(0.0), None, "dropped past the margin");
        assert!(app.world().get_entity(scan).is_err());
        assert_eq!(start_margin(&app, tree), 0.0..0.0, "the crown cards show");
    }

    #[test]
    fn a_loading_scan_stays_hidden_under_its_tree() {
        let pose = Transform::from_xyz(3.0, 0.0, 0.0).with_scale(Vec3::splat(12.0));
        let (mut app, _, tree) = streaming(pose);
        app.update();
        let world = app.world();
        let scan = world.entity(tree).get::<CloseScan>().expect("scan").0;
        let scan = world.entity(scan);
        assert_eq!(scan.get::<ChildOf>().map(ChildOf::parent), Some(tree));
        assert_eq!(scan.get::<Visibility>(), Some(&Visibility::Hidden));
        assert_eq!(start_margin(&app, tree), 0.0..0.0, "crown cards wait");
    }

    #[test]
    fn the_hangar_shelterbelt_adds_all_species_without_filling_the_yard() {
        let terrain = Terrain::new();
        let mut trees = Vec::new();
        for z in -2..=1 {
            for x in -2..=2 {
                trees.extend(belt_trees(&terrain, base_tile() + IVec2::new(x, z)));
            }
        }
        trees.retain(|tree| hangar_belt(tree.pose.translation.xz()) > 0.0);
        assert!((60..=110).contains(&trees.len()), "{} trees", trees.len());
        assert!(
            trees
                .iter()
                .filter(|tree| tree.pose.translation.z - BASE.1 < -44.0)
                .count()
                > trees.len() / 2
        );
        for species in Species::ALL {
            assert!(trees.iter().filter(|tree| tree.species == species).count() >= 4);
        }
        for tree in trees {
            let at = tree.pose.translation;
            let offset = at.xz() - Vec2::from(BASE);
            assert!(offset.x.abs() > 49.0 || offset.y < -44.0);
            assert!(at.y <= terrain.mesh_height_at(at.xz()));
            assert!(terrain.road_mask(at.x, at.z) <= 0.02);
            assert!(!near_wall_or_checkpoint(at.xz(), 2.6));
        }
    }

    #[test]
    fn trees_line_the_patrolled_perimeter_clear_of_the_patrol_and_the_gate() {
        use crate::world::cordon::{
            PATROL_TURNS, PERIMETER_GATE, PERIMETER_X, reserve_patrol_track,
        };

        let terrain = Terrain::new();
        let column = (PERIMETER_X / TILE_SIZE).floor() as i32;
        let trees: Vec<_> = (-4..=6)
            .flat_map(|z| belt_trees(&terrain, IVec2::new(column, z)))
            .filter(|tree| hangar_belt(tree.pose.translation.xz()) == 0.0)
            .collect();
        let side = |zone: bool| {
            trees
                .iter()
                .filter(|tree| (tree.pose.translation.x < PERIMETER_X) == zone)
                .count()
        };
        assert!(
            side(true) > 15 && side(false) > 15,
            "{} / {}",
            side(true),
            side(false)
        );
        let [south, north] = PATROL_TURNS;
        for tree in &trees {
            let at = tree.pose.translation.xz();
            assert!(
                (south.y..north.y).contains(&at.y),
                "tree past the patrol at {at}"
            );
            assert!(
                (at - PERIMETER_GATE).abs().max_element() >= 34.0,
                "tree at the gate {at}"
            );
            assert!(
                !reserve_patrol_track(at),
                "tree on the patrol's track at {at}"
            );
            assert!(
                !near_wall_or_checkpoint(at, 2.6),
                "tree in the wall at {at}"
            );
        }
    }

    #[test]
    fn wind_moves_nearby_trees_in_place_and_distant_trees_stop_casting_shadows() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.5));
        app.insert_resource(time)
            .insert_resource(crate::world::Weather::default())
            .init_resource::<Wind>()
            .add_systems(
                Update,
                (super::super::weather::blow_wind, update_canopy_motion).chain(),
            );
        let camera = app
            .world_mut()
            .spawn((PlayerCamera, Transform::IDENTITY))
            .id();
        let root = Vec3::new(2.0, 0.0, 1.0);
        let tree = app
            .world_mut()
            .spawn((
                CanopyVisual {
                    rest: Quat::IDENTITY,
                    phase: 1.0,
                    bend: 0.02,
                    close: assets::TREE_SMALL_UHD,
                },
                Transform::from_translation(root),
                NotShadowCaster,
            ))
            .id();
        app.update();
        let near = app.world().entity(tree);
        let rotation = near.get::<Transform>().expect("visual transform").rotation;
        assert_ne!(rotation, Quat::IDENTITY);
        assert!(!near.contains::<NotShadowCaster>());
        app.update();
        assert_ne!(
            app.world()
                .get::<Transform>(tree)
                .expect("visual transform")
                .rotation,
            rotation
        );
        app.world_mut()
            .get_mut::<Transform>(camera)
            .expect("camera transform")
            .translation
            .x = 100.0;
        app.update();
        let far = app.world().entity(tree);
        assert_eq!(
            *far.get::<Transform>().expect("visual transform"),
            Transform::from_translation(root)
        );
        assert!(far.contains::<NotShadowCaster>());
    }
}
