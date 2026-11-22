use std::convert::Infallible;

use bevy::light::NotShadowCaster;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;

use crate::assets::PreloadedAssets;

use super::canopy::{self, CanopyModels};
use super::ground_fog::FogField;
use super::scenery::{self, Scenery, Sites};
use super::terrain::{HORIZON_TILE_RADIUS, SCENIC_TILE_RADIUS, TILE_RADIUS, Terrain, TerrainGrid};
use super::terrain_material::TerrainMaterial;
use super::water::{self, WaterMaterial};
use super::{StreamAnchor, anchor_tiles, detail_range, near_anchor};

struct TerrainTile {
    coord: IVec2,
    ground: Mesh,
    playable: bool,
    water: Option<Mesh>,
    forest: Option<Mesh>,
    stone: Option<Mesh>,
}

struct TileMeshes {
    coord: IVec2,
    ground: Handle<Mesh>,
    playable: bool,
    water: Option<Handle<Mesh>>,
    forest: Option<Handle<Mesh>>,
    stone: Option<Handle<Mesh>>,
}

#[derive(Component)]
pub(super) struct GroundLod {
    coord: IVec2,
    distant: Handle<Mesh>,
    distant_water: Option<Handle<Mesh>>,
    near: bool,
    water: Option<Entity>,
}

pub(super) fn stream_ground_detail(
    mut commands: Commands,
    anchors: Query<&Transform, With<StreamAnchor>>,
    terrain: Res<Terrain>,
    materials: Res<super::LandscapeMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<Vec<IVec2>>,
    mut tiles: Query<(Entity, &mut GroundLod, &mut Mesh3d)>,
) {
    let centers = anchor_tiles(anchors);
    if *last == centers {
        return;
    }
    *last = centers.clone();
    for (entity, mut lod, mut mesh) in &mut tiles {
        let near = near_anchor(lod.coord, &centers, 2);
        if lod.near == near {
            continue;
        }
        lod.near = near;
        let water = if near {
            let grid = TerrainGrid::detail(lod.coord);
            let (detail, wet) = terrain.build_grid(grid);
            mesh.0 = meshes.add(detail);
            commands.entity(entity).remove::<NotShadowCaster>();
            water::build_tile(grid, &wet).map(|surface| meshes.add(surface))
        } else {
            mesh.0 = lod.distant.clone();
            commands.entity(entity).insert(NotShadowCaster);
            lod.distant_water.clone()
        };
        if let Some(old) = lod.water.take() {
            commands.entity(old).despawn();
        }
        lod.water = water.map(|mesh| {
            commands
                .spawn((water::surface(mesh, &materials.water), ChildOf(entity)))
                .id()
        });
    }
}

#[derive(Asset, TypePath)]
pub(super) struct Landscape {
    sites: HashMap<IVec2, Sites>,
    pending: Vec<TerrainTile>,
    meshes: Vec<TileMeshes>,
    pending_fog: Option<FogField>,
    fog: Handle<Image>,
}

#[derive(Resource)]
pub(super) struct LandscapeSource(pub Handle<Landscape>);

pub(super) fn prepare_landscape(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    mut preloaded: ResMut<PreloadedAssets>,
) {
    let terrain = terrain.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let mut sites = HashMap::default();
        let mut pending = Vec::new();
        let mut fog = FogField::default();
        for z in -SCENIC_TILE_RADIUS..SCENIC_TILE_RADIUS {
            for x in -SCENIC_TILE_RADIUS..SCENIC_TILE_RADIUS {
                let tile = IVec2::new(x, z);
                let grid = TerrainGrid::distant(tile);
                let (ground, wet) = terrain.build_grid(grid);
                fog.add_tile(grid, &ground, &wet);
                let in_playable_tiles = (-TILE_RADIUS..TILE_RADIUS).contains(&x)
                    && (-TILE_RADIUS..TILE_RADIUS).contains(&z);
                let (forest, stone) = if in_playable_tiles {
                    let (objects, reeds, stone) = scenery::build_tile(&terrain, tile);
                    sites.insert(tile, objects);
                    (reeds, stone)
                } else {
                    (None, None)
                };
                pending.push(TerrainTile {
                    coord: tile,
                    ground,
                    playable: in_playable_tiles,
                    water: water::build_tile(grid, &wet),
                    forest,
                    stone,
                });
            }
        }
        // The large, low-resolution outer tiles reach past the camera's far plane.
        let inner = SCENIC_TILE_RADIUS / 4;
        for z in -HORIZON_TILE_RADIUS..HORIZON_TILE_RADIUS {
            for x in -HORIZON_TILE_RADIUS..HORIZON_TILE_RADIUS {
                if (-inner..inner).contains(&x) && (-inner..inner).contains(&z) {
                    continue;
                }
                let tile = IVec2::new(x, z);
                let grid = TerrainGrid::horizon(tile);
                let (ground, wet) = terrain.build_grid(grid);
                pending.push(TerrainTile {
                    coord: tile,
                    ground,
                    playable: false,
                    water: water::build_tile(grid, &wet),
                    forest: None,
                    stone: None,
                });
            }
        }
        Landscape {
            sites,
            pending,
            meshes: Vec::new(),
            pending_fog: Some(fog),
            fog: Handle::default(),
        }
    });
    let source = server.add_async(async move { Ok::<_, Infallible>(task.await) });
    preloaded.0.push(source.clone().untyped());
    commands.insert_resource(LandscapeSource(source));
}

impl Landscape {
    pub(super) fn upload(&mut self, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) {
        if let Some(fog) = self.pending_fog.take() {
            self.fog = images.add(fog.into_image());
        }
        for tile in self.pending.drain(..) {
            self.meshes.push(TileMeshes {
                coord: tile.coord,
                ground: meshes.add(tile.ground),
                playable: tile.playable,
                water: tile.water.map(|mesh| meshes.add(mesh)),
                forest: tile.forest.map(|mesh| meshes.add(mesh)),
                stone: tile.stone.map(|mesh| meshes.add(mesh)),
            });
        }
    }

    pub(super) fn spawn(
        &self,
        world: &mut ChildSpawnerCommands,
        terrain: &Handle<TerrainMaterial>,
        water_surface: &Handle<WaterMaterial>,
        foliage: &Handle<StandardMaterial>,
        canopies: &CanopyModels,
    ) {
        for tile in &self.meshes {
            if tile.playable {
                let mut entity = world.spawn((
                    Mesh3d(tile.ground.clone()),
                    MeshMaterial3d(terrain.clone()),
                    NotShadowCaster,
                ));
                let parent = entity.id();
                let water = tile.water.clone().map(|mesh| {
                    entity
                        .commands()
                        .spawn((water::surface(mesh, water_surface), ChildOf(parent)))
                        .id()
                });
                entity.insert(GroundLod {
                    coord: tile.coord,
                    distant: tile.ground.clone(),
                    distant_water: tile.water.clone(),
                    near: false,
                    water,
                });
            } else {
                world.spawn((
                    Mesh3d(tile.ground.clone()),
                    MeshMaterial3d(terrain.clone()),
                    NotShadowCaster,
                ));
                if let Some(mesh) = &tile.water {
                    world.spawn(water::surface(mesh.clone(), water_surface));
                }
            }
            if let Some(mesh) = &tile.forest {
                // Batched reeds; frustum culling bounds the cost.
                world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(foliage.clone())));
            }
            if let Some(mesh) = &tile.stone {
                world.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(terrain.clone()),
                    detail_range(290.0, 0.0),
                ));
            }
        }
        for sites in self.sites.values() {
            canopy::spawn_canopies(world, canopies, &sites.canopies);
        }
    }

    /// The ground fog's field texture, once uploaded.
    pub(super) fn fog_field(&self) -> Handle<Image> {
        self.fog.clone()
    }

    pub(super) fn scenery(&self, root: Entity) -> Scenery {
        Scenery {
            root,
            sites: self.sites.clone(),
            active: HashMap::default(),
            centers: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Player;
    use crate::world::canopy::{CanopyVisual, ForestTree, Species};
    use crate::world::terrain::TILE_SIZE;

    #[test]
    fn ground_detail_follows_the_player_into_the_new_land() {
        let mut app = App::new();
        app.insert_resource(Terrain::new())
            .insert_resource(super::super::LandscapeMaterials {
                terrain: Handle::default(),
                water: Handle::default(),
                foliage: Handle::default(),
            })
            .init_resource::<Assets<Mesh>>()
            .add_systems(Update, stream_ground_detail);
        let world = app.world_mut();
        let distant = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let tile = world
            .spawn((
                Mesh3d(distant.clone()),
                GroundLod {
                    coord: IVec2::new(12, -9),
                    distant: distant.clone(),
                    distant_water: None,
                    near: false,
                    water: None,
                },
            ))
            .id();
        let player = world.spawn((Player::default(), Transform::IDENTITY)).id();
        let mesh = |app: &App| {
            app.world()
                .get::<Mesh3d>(tile)
                .expect("a ground tile")
                .0
                .clone()
        };
        app.update();
        assert_eq!(mesh(&app), distant);
        let mut at = app
            .world_mut()
            .get_mut::<Transform>(player)
            .expect("player spawn");
        at.translation = Vec3::new(12.5 * TILE_SIZE, 0.0, -8.5 * TILE_SIZE);
        app.update();
        let close = mesh(&app);
        assert_ne!(close, distant);
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&close)
                .expect("nearby mesh")
                .count_vertices(),
            (super::super::terrain::CELLS + 1).pow(2)
        );
        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player spawn")
            .translation = Vec3::ZERO;
        app.update();
        assert_eq!(mesh(&app), distant);
    }

    #[test]
    fn a_canopy_has_only_one_visual_in_a_multi_chunk_landscape() {
        let tile = IVec2::ZERO;
        let landscape = Landscape {
            sites: [(
                tile,
                Sites {
                    solids: vec![],
                    canopies: vec![ForestTree {
                        species: Species::Birch,
                        pose: Transform::IDENTITY,
                    }],
                    undergrowth: vec![],
                },
            )]
            .into_iter()
            .collect(),
            meshes: [tile, IVec2::X]
                .map(|coord| TileMeshes {
                    coord,
                    ground: Handle::default(),
                    playable: false,
                    water: None,
                    forest: None,
                    stone: None,
                })
                .into(),
            pending: vec![],
            pending_fog: None,
            fog: Handle::default(),
        };
        let mut world = World::new();
        let root = world.spawn(Transform::IDENTITY).id();
        world.commands().entity(root).with_children(|objects| {
            landscape.spawn(
                objects,
                &Handle::default(),
                &Handle::default(),
                &Handle::default(),
                &CanopyModels::default(),
            );
        });
        world.flush();
        assert_eq!(world.query::<&CanopyVisual>().iter(&world).count(), 1);
    }
}
