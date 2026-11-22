//! Chunk-batched rocks and reeds, and the spots where imported tree models
//! grow; solid pillars are installed only near the player. Every tree on the
//! map is an imported model (see `canopy.rs`): tree spots away from the
//! landmarks and the hangar stay open ground.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::{StreamAnchor, anchor_tiles, near_anchor, neighborhoods};

use super::canopy::{self, ForestTree};
use super::collision::{Collider, Pillar, Unclimbable};
use super::cordon::{reserve_scenery, shelterbelt};
use super::hydrology::Wetlands;
use super::plants::PendingPlants;
use super::sites;
use super::terrain::{TILE_SIZE, Terrain, lattice_hash, lerp, smoothstep, value_noise};
use super::trees::{self, TreeSite};

const SEED: u32 = 0xD33D_305A;
pub(super) const TREE_DENSITY: f32 = 0.25;
/// Tree spots kept in the heart of a dense wood.
const WOOD_DENSITY: f32 = 0.85;
const BOULDER: &[Pillar] = &[Pillar {
    x: 0.0,
    z: 0.0,
    radius: 0.71,
    bottom: -0.28,
    top: 0.89,
}];

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Kind {
    /// A spot an imported tree may take.
    Tree,
    Rock,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Site {
    kind: Kind,
    pose: Transform,
    seed: u32,
}

impl Site {
    /// Only rocks stay solid sites; tree spots become imported trees.
    fn solid(self) -> Collider {
        Collider(BOULDER)
    }
}

/// An imported tree still standing, which may snap and fall.
#[derive(Component, Clone, Copy)]
#[require(Unclimbable)]
pub(super) struct StandingTree(pub ForestTree);

impl StandingTree {
    pub(super) fn pose(self) -> Transform {
        self.0.pose
    }
}

#[derive(Clone)]
pub(super) struct Sites {
    pub solids: Vec<Site>,
    pub canopies: Vec<ForestTree>,
    pub undergrowth: Vec<TreeSite>,
}

#[derive(Resource)]
pub(super) struct Scenery {
    pub root: Entity,
    pub sites: bevy::platform::collections::HashMap<IVec2, Sites>,
    pub active: bevy::platform::collections::HashMap<IVec2, Entity>,
    pub centers: Vec<IVec2>,
}

pub(super) fn forest_density(x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    let regions = [
        (Vec2::new(-232.0, -26.0), Vec2::new(239.0, 223.0)),
        (Vec2::new(237.0, 111.0), Vec2::new(202.0, 224.0)),
        (Vec2::new(-435.0, 534.0), Vec2::new(258.0, 205.0)),
        (Vec2::new(-491.0, 105.0), Vec2::new(250.0, 274.0)),
        (Vec2::new(520.0, -245.0), Vec2::new(248.0, 350.0)),
        (Vec2::new(168.0, 499.0), Vec2::new(201.0, 134.0)),
        (Vec2::new(-1150.0, 70.0), Vec2::new(350.0, 500.0)),
        (Vec2::new(-1610.0, 740.0), Vec2::new(510.0, 305.0)),
        (Vec2::new(-1650.0, -580.0), Vec2::new(610.0, 445.0)),
        (Vec2::new(-630.0, -1250.0), Vec2::new(375.0, 440.0)),
        (Vec2::new(150.0, 1510.0), Vec2::new(473.0, 525.0)),
        (Vec2::new(1260.0, 800.0), Vec2::new(420.0, 430.0)),
        (Vec2::new(1090.0, -895.0), Vec2::new(445.0, 385.0)),
        (Vec2::new(2020.0, -540.0), Vec2::new(430.0, 280.0)),
    ];
    let core = regions
        .into_iter()
        .fold(0.0_f32, |density, (center, radii)| {
            density.max(smoothstep(1.15, 0.42, ((p - center) / radii).length()))
        });
    let groves = value_noise(x / 46.0, z / 46.0, SEED + 3);
    (0.025 + core * 0.88) * smoothstep(0.16, 0.68, groves)
}

fn sites_for_tile(terrain: &Terrain, tile: IVec2, cell: f32, kind: Kind) -> Vec<Site> {
    let cells = (TILE_SIZE / cell) as i32;
    let corner = tile * cells;
    let mut sites = Vec::new();
    for z in corner.y..corner.y + cells {
        for x in corner.x..corner.x + cells {
            let salt = if kind == Kind::Tree { 0 } else { 211 };
            let roll = |n| lattice_hash(x, z, SEED + salt + n);
            let p = (Vec2::new(x as f32, z as f32)
                + Vec2::new(lerp(0.12, 0.88, roll(0)), lerp(0.12, 0.88, roll(1))))
                * cell;
            let chance = roll(2);
            if reserve_scenery(p, 5.0)
                || sites::clear_footprint(p, 4.0)
                || terrain.road_mask(p.x, p.y) > 0.02
                || terrain.base_mask(p.x, p.y) > 0.05
                || (kind == Kind::Tree && shelterbelt(p) > 0.0)
            {
                continue;
            }
            let y = terrain.height_at(p.x, p.y);
            let scale = match kind {
                Kind::Tree => {
                    let grove = forest_density(p.x, p.y).max(0.48 * canopy::landmark_grove(p));
                    let density = (TREE_DENSITY * grove).max(WOOD_DENSITY * canopy::wood(p));
                    if chance > density * terrain.woodland_at(p.x, p.y)
                        || terrain.water_depth_at(p.x, p.y) > 0.0
                    {
                        continue;
                    }
                    Vec3::splat(lerp(7.5, 17.8, roll(3).powf(1.32)))
                }
                Kind::Rock => {
                    let wet = Wetlands::at(p.x, p.y).wetness();
                    let upland = smoothstep(7.0, 75.0, y);
                    let seams =
                        smoothstep(0.27, 0.68, value_noise(p.x / 51.0, p.y / 51.0, SEED + 9));
                    if chance > 0.055 + seams * 0.20 + upland * 0.41 + wet * 0.08
                        || terrain.water_depth_at(p.x, p.y) > 0.08
                    {
                        continue;
                    }
                    let radius = lerp(0.36, 4.5 + upland * 1.35, roll(3).powf(2.4));
                    Vec3::new(radius, radius * lerp(0.66, 1.34, roll(4)), radius)
                }
            };
            let extent = if kind == Kind::Tree {
                scale.x * 0.04
            } else {
                scale.x * 0.74
            };
            let base = [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
                .into_iter()
                .map(|direction| p + extent * direction)
                .fold(y, |low, at| low.min(terrain.height_at(at.x, at.y)));
            if y - base > scale.y * 0.65 {
                continue;
            }
            let pose = Transform::from_xyz(p.x, base - 0.035, p.y)
                .with_rotation(Quat::from_rotation_y(roll(5) * TAU))
                .with_scale(scale);
            sites.push(Site {
                kind,
                pose,
                seed: (roll(6) * 65536.0) as u32,
            });
        }
    }
    sites
}

/// A tile's objects, its batched reeds and its batched rocks.
pub(super) fn build_tile(terrain: &Terrain, tile: IVec2) -> (Sites, Option<Mesh>, Option<Mesh>) {
    let mut stone = Batch::default();
    let mut canopies = canopy::hangar_trees(terrain, tile);
    canopies.extend(
        sites_for_tile(terrain, tile, 8.0, Kind::Tree)
            .into_iter()
            .filter_map(|site| ForestTree::at_tree_spot(terrain, site.pose, site.seed)),
    );
    let solids = sites_for_tile(terrain, tile, 16.0, Kind::Rock);
    for &site in &solids {
        rock(site, &mut stone);
    }
    let mut reeds = Batch::default();
    grow_reeds(terrain, tile, &mut reeds);
    let sites = Sites {
        solids,
        canopies,
        undergrowth: trees::tree_sites(terrain, tile),
    };
    (sites, reeds.finish(true), stone.finish(false))
}

pub(super) fn stream_scenery(
    mut commands: Commands,
    anchors: Query<&Transform, With<StreamAnchor>>,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    mut shared_terrain: Local<Option<Arc<Terrain>>>,
    mut scenery: ResMut<Scenery>,
) {
    if terrain.is_changed() {
        *shared_terrain = None;
    }
    let centers = anchor_tiles(anchors);
    if scenery.centers == centers {
        return;
    }
    scenery.centers = centers.clone();
    let shared_terrain = shared_terrain.get_or_insert_with(|| Arc::new(terrain.clone()));
    scenery.active.retain(|coord, entity| {
        let near = near_anchor(*coord, &centers, 1);
        if !near {
            commands.entity(*entity).despawn();
        }
        near
    });
    for tile in neighborhoods(&centers, 1) {
        if scenery.active.contains_key(&tile) {
            continue;
        }
        let Some(sites) = scenery.sites.get(&tile) else {
            continue;
        };
        let entity = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                ChildOf(scenery.root),
                PendingPlants::grow(shared_terrain.clone(), tile),
            ))
            .with_children(|objects| {
                for site in &sites.solids {
                    objects.spawn((site.pose, site.solid()));
                }
                for tree in &sites.canopies {
                    objects.spawn((
                        tree.pose,
                        tree.collider(),
                        tree.weight(),
                        StandingTree(*tree),
                    ));
                }
                trees::spawn_trees(objects, &server, &sites.undergrowth);
                super::sites::spawn_solids(objects, tile);
                super::town::spawn_solids(objects, tile);
            })
            .id();
        scenery.active.insert(tile, entity);
    }
}

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
}

impl Batch {
    fn triangle(&mut self, pose: Transform, at: [Vec3; 3], tint: [f32; 4]) {
        let [a, b, c] = at.map(|p| pose.transform_point(p));
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        self.positions
            .extend([a.to_array(), b.to_array(), c.to_array()]);
        self.normals.extend([n; 3]);
        self.colors.extend([tint; 3]);
    }

    fn finish(self, textured: bool) -> Option<Mesh> {
        (!self.positions.is_empty()).then(|| {
            let uv = textured.then(|| {
                self.positions
                    .iter()
                    .map(|&[x, y, z]| [x * 0.88 + z * 0.29, y * 0.82 + z * 0.58])
                    .collect::<Vec<_>>()
            });
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
            if let Some(uv) = uv {
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            }
            mesh
        })
    }
}

pub(super) fn foliage_material(images: &mut Assets<Image>) -> StandardMaterial {
    const SIZE: i32 = 96;
    let pixels = (0..SIZE)
        .flat_map(|z| {
            (0..SIZE).flat_map(move |x| {
                let twig = lattice_hash(x / 2, (z + x / 2) % SIZE / 4, SEED + 82);
                let fleck = lattice_hash(x, z, SEED + 83);
                let grain = (145.0 + 80.0 * twig + 26.0 * fleck) as u8;
                [grain, grain, grain, 255]
            })
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 4,
        ..ImageSamplerDescriptor::linear()
    });
    StandardMaterial {
        base_color_texture: Some(images.add(image)),
        perceptual_roughness: 0.96,
        ..default()
    }
}

fn grow_reeds(terrain: &Terrain, tile: IVec2, batch: &mut Batch) {
    const CELL: f32 = 4.0;
    let cells = (TILE_SIZE / CELL) as i32;
    let start = tile * cells;
    for z in start.y..start.y + cells {
        for x in start.x..start.x + cells {
            let roll = |i| lattice_hash(x, z, SEED + 331 + i);
            if roll(0) > 0.56 {
                continue;
            }
            let root = Vec2::new(x as f32 + roll(1), z as f32 + roll(2)) * CELL;
            let wet = Wetlands::at(root.x, root.y);
            if wet.wetness() < 0.35
                || terrain.road_mask(root.x, root.y) > 0.05
                || sites::clear_footprint(root, 1.0)
            {
                continue;
            }
            let y = terrain.height_at(root.x, root.y);
            let depth = wet.depth(y);
            if !(-0.4..0.48).contains(&depth) || (wet.marsh() < 0.32 && roll(3) > 0.46) {
                continue;
            }
            let pose = Transform::from_xyz(root.x, y - 0.05, root.y);
            for blade in 0..9 {
                let angle = roll(4 + blade * 4) * TAU;
                let heading = Vec2::from_angle(angle);
                let at = heading * lerp(0.04, 0.57, roll(5 + blade * 4));
                let tall = lerp(0.45, 1.47, roll(6 + blade * 4));
                let side = Vec3::new(-heading.y, 0.0, heading.x) * (tall * 0.028);
                let base = Vec3::new(at.x, 0.0, at.y);
                let middle = base + Vec3::new(heading.x * 0.04, tall * 0.63, heading.y * 0.04);
                let tip = base + Vec3::new(heading.x * 0.23, tall, heading.y * 0.23);
                let light = lerp(0.36, 0.68, roll(7 + blade * 4));
                let tint = [light * 0.63, light, light * 0.26, 1.0];
                let edge = side * 0.62;
                for [a, b, c] in [
                    [base - side, middle - edge, base + side],
                    [base + side, middle - edge, middle + edge],
                    [middle - edge, tip, middle + edge],
                ] {
                    batch.triangle(pose, [a, b, c], tint);
                    batch.triangle(pose, [a, c, b], tint);
                }
            }
        }
    }
}

fn rock(site: Site, batch: &mut Batch) {
    let shade = |i| lattice_hash(i, 0, SEED + site.seed);
    let sides: i32 = 7 + (site.seed % 3) as i32;
    let color_at = |i| [1.0, 0.12, 0.27, lerp(0.68, 1.20, shade(i + 89))];
    let point = |i: i32, ring: i32| {
        let j = i.rem_euclid(sides);
        let a = Vec2::from_angle(j as f32 * TAU / sides as f32 + ring as f32 * 0.19);
        let radius = [0.83, 1.02, 0.60][ring as usize] * lerp(0.81, 1.14, shade(j * 3 + ring));
        let y = [-0.28, 0.39, 0.83][ring as usize] + (shade(j + ring * 13 + 19) - 0.5) * 0.19;
        Vec3::new(a.x * radius + ring as f32 * 0.046, y, a.y * radius)
    };
    for i in 0..sides {
        for ring in 0..2 {
            let (a, b) = (point(i, ring), point(i + 1, ring));
            let (c, d) = (point(i, ring + 1), point(i + 1, ring + 1));
            batch.triangle(site.pose, [a, c, b], color_at(i + ring * 2));
            batch.triangle(site.pose, [c, d, b], color_at(i + ring * 2 + 1));
        }
        let top = Vec3::new(0.09, 0.94 + shade(31) * 0.1, 0.13);
        batch.triangle(
            site.pose,
            [top, point(i + 1, 2), point(i, 2)],
            color_at(i + 9),
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;

    use super::*;
    use crate::player::Player;
    use crate::world::collision::{Body, push_out};

    #[test]
    fn every_tree_is_an_imported_model_near_a_landmark_with_a_solid_trunk() {
        let terrain = Terrain::new();
        let mut imported = 0;
        let fixtures = (-4..=1).flat_map(|x| (-2..=3).map(move |z| IVec2::new(x, z)));
        for tile in fixtures.chain([IVec2::new(4, -2), IVec2::new(-12, 3)]) {
            let (sites, _, _) = build_tile(&terrain, tile);
            assert!(sites.solids.iter().all(|site| site.kind == Kind::Rock));
            for tree in sites.canopies {
                let pose = tree.pose;
                imported += 1;
                assert!(canopy::near_landmark(pose.translation.xz()));
                let collider = tree.collider();
                let trunk = collider.0[0];
                let at = pose.transform_point(Vec3::new(trunk.x, 0.0, trunk.z));
                let ground = terrain.mesh_height_at(at.xz());
                assert!(at.y <= ground);
                assert_eq!(terrain.water_depth_at(at.x, at.z), 0.0);
                assert!(terrain.road_mask(at.x, at.z) <= 0.02);
                assert_ne!(
                    push_out(
                        at.xz(),
                        ground,
                        Body {
                            radius: 0.35,
                            height: 0.5,
                        },
                        [(&collider, &GlobalTransform::from(pose))],
                    ),
                    at.xz()
                );
            }
        }
        assert!(imported > 20, "{imported} imported trees");
    }

    #[test]
    fn each_dense_wood_is_thick_with_trees() {
        let terrain = Terrain::new();
        for (center, radius) in canopy::WOODS {
            let core = radius * 0.5;
            let low = ((center - core) / TILE_SIZE).floor().as_ivec2();
            let high = ((center + core) / TILE_SIZE).floor().as_ivec2();
            let mut trees = 0;
            for x in low.x..=high.x {
                for z in low.y..=high.y {
                    let (sites, _, _) = build_tile(&terrain, IVec2::new(x, z));
                    trees += sites
                        .canopies
                        .iter()
                        .filter(|tree| tree.pose.translation.xz().distance(center) < core)
                        .count();
                }
            }
            // About a tree every 12 m or closer across the wood's heart.
            let spacing = (std::f32::consts::PI * core * core / trees.max(1) as f32).sqrt();
            assert!(
                spacing < 12.0,
                "{trees} trees, {spacing} m apart at {center}"
            );
        }
    }

    #[test]
    fn open_country_away_from_landmarks_has_no_trees() {
        let terrain = Terrain::new();
        let spots = sites_for_tile(&terrain, IVec2::new(-6, -9), 8.0, Kind::Tree);
        assert!(!spots.is_empty(), "the fixture has tree spots");
        let (sites, _, _) = build_tile(&terrain, IVec2::new(-6, -9));
        assert!(sites.canopies.is_empty());
    }

    #[test]
    fn large_rocks_are_grounded_solid_obstacles() {
        let terrain = Terrain::new();
        let rocks = sites_for_tile(&terrain, IVec2::new(4, 4), 16.0, Kind::Rock);
        let boulder = rocks
            .iter()
            .find(|r| r.pose.scale.y > 1.5)
            .expect("mountain boulder");
        let p = boulder.pose.translation;
        let ground = terrain.height_at(p.x, p.z);
        assert!(p.y <= ground);
        assert_ne!(
            push_out(
                p.xz(),
                ground,
                Body {
                    radius: 0.35,
                    height: 1.8
                },
                [(&boulder.solid(), &GlobalTransform::from(boulder.pose))]
            ),
            p.xz()
        );
    }

    #[test]
    fn distant_colliders_unload_and_return_at_the_same_position() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .insert_resource(Terrain::new())
            .add_systems(Update, stream_scenery);
        let root = app.world_mut().spawn(Transform::IDENTITY).id();
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::IDENTITY))
            .id();
        let boulder = Site {
            kind: Kind::Rock,
            pose: Transform::from_xyz(24.0, 0.0, 12.0),
            seed: 1,
        };
        app.insert_resource(Scenery {
            root,
            sites: [(
                IVec2::ZERO,
                Sites {
                    solids: vec![boulder],
                    canopies: vec![],
                    undergrowth: vec![],
                },
            )]
            .into_iter()
            .collect(),
            active: Default::default(),
            centers: Vec::new(),
        });
        let colliders = |world: &mut World| {
            let mut query = world.query_filtered::<&GlobalTransform, With<Collider>>();
            query
                .iter(world)
                .map(GlobalTransform::translation)
                .collect::<Vec<_>>()
        };
        // The tile also streams the Institute's collider.
        app.update();
        assert!(colliders(app.world_mut()).contains(&boulder.pose.translation));
        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x = TILE_SIZE * 3.0;
        app.update();
        assert!(colliders(app.world_mut()).is_empty());
        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x = 0.0;
        app.update();
        assert!(colliders(app.world_mut()).contains(&boulder.pose.translation));
    }
}
