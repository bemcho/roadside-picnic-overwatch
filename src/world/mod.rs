//! World environment: terrain, lighting, atmosphere, ambience.

mod base;
mod canopy;
mod collision;
mod cordon;
mod ground_fog;
mod hydrology;
mod landscape;
mod places;
mod plants;
mod roads;
mod scenery;
mod sites;
mod terrain;
mod terrain_material;
mod tree_fall;
mod trees;
mod water;
mod weather;
mod wildlife;

pub(crate) use base::ArtifactDisplay;
pub(crate) use collision::STEP_HEIGHT;
pub use collision::{Body, Collider, Pillar, push_out};
pub(crate) use ground_fog::{CAMERA_FAR, camera_distance_fog, camera_fog};
#[cfg(test)]
pub(crate) use hydrology::FORD_X;
pub use places::{Describe, Explored, Place, place_at, structure_on_ray};
pub use terrain::{HALF_SIZE as TERRAIN_HALF_SIZE, SPAWN, Terrain};
pub use weather::{Sky, Weather, Wind, WorldClock};
pub(crate) use wildlife::{CreatureSound, Cue as CreatureCue, Kind as CreatureKind};

use bevy::camera::visibility::VisibilityRange;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{Atmosphere, SunDisk, light_consts::lux};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::world_serialization::WorldInstanceReady;

use crate::states::{AppState, InGamePhase};

use canopy::CanopyModels;
use landscape::{Landscape, LandscapeSource, prepare_landscape, stream_ground_detail};
use scenery::{Scenery, stream_scenery};
use terrain_material::{TerrainMaterial, load_terrain_material, mipmap_filtered_textures};
use water::WaterMaterial;

/// Atmosphere density relative to Earth; thicker air reads as Zone haze.
const HAZE: f32 = 2.5;

/// Spawns the base world (terrain, sun, ambience) when entering the Zone,
/// and tears it down when leaving.
pub struct WorldPlugin;

/// Root marker for everything spawned by this plugin's OnEnter.
#[derive(Component)]
struct WorldRoot;

/// Directional light standing in for the sun.
#[derive(Component)]
struct Sun;

/// Dim, cool directional light for the full moon, opposite the sun.
#[derive(Component)]
struct Moon;

#[derive(Resource)]
struct LandscapeMaterials {
    terrain: Handle<TerrainMaterial>,
    water: Handle<WaterMaterial>,
    foliage: Handle<StandardMaterial>,
}

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<TerrainMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<base::CutawayMaterial>::default(),
            MaterialPlugin::<plants::PlantMaterial>::default(),
            ground_fog::GroundFogRenderPlugin,
        ))
        .init_asset::<Landscape>()
        .init_resource::<Terrain>()
        .add_message::<tree_fall::TreeBreak>()
        .add_systems(
            Startup,
            (
                prepare_landscape,
                load_landscape_materials,
                canopy::preload_canopies,
                wildlife::preload_wildlife,
                cordon::preload_cordon,
                sites::preload_sites,
            ),
        )
        .add_systems(
            OnEnter(AppState::InGame),
            (
                (canopy::prepare_canopy_materials, spawn_world).chain(),
                (weather::start_clock, spawn_sky).chain(),
                places::start_survey,
                base::spawn_base,
                plants::prepare_plant_materials,
                tree_fall::start_break_clock,
                wildlife::spawn_wildlife,
                cordon::prepare_cordon,
                sites::spawn_sites,
            ),
        )
        .add_systems(Update, mipmap_filtered_textures)
        .add_systems(
            Update,
            (
                weather::advance_clock,
                weather::blow_wind,
                (plants::bend_plants, plants::sway_bushes),
                weather::light_the_sky,
                places::survey_around_stalker,
                ground_fog::update_ground_fog,
                ground_fog::follow_terrain_fog,
                ground_fog::update_distance_fog,
                base::update_cutaway,
            )
                .chain()
                .run_if(in_state(InGamePhase::Running)),
        )
        .add_systems(
            PreUpdate,
            (
                (stream_scenery, plants::spawn_grown_plants).chain(),
                stream_ground_detail,
                cordon::stream_cordon,
                sites::stream_full_sites,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            PostUpdate,
            (
                (wildlife::move_wildlife, wildlife::move_mutants),
                wildlife::pose_wildlife,
            )
                .chain()
                .before(TransformSystems::Propagate)
                .run_if(in_state(InGamePhase::Running)),
        )
        .add_systems(
            PostUpdate,
            (
                plants::stream_full_bushes,
                base::stream_full_artifacts,
                base::animate_artifacts,
                tree_fall::animate_falling_trees,
                tree_fall::pick_tree_to_break,
                tree_fall::break_selected_trees,
                canopy::update_canopy_motion,
            )
                .chain()
                .before(TransformSystems::Propagate)
                .run_if(in_state(InGamePhase::Running)),
        )
        .add_systems(OnExit(AppState::InGame), despawn_world);
    }
}

/// Extra reach for every detail range, so the tactical camera's zoomed-out view
/// (up to 90 m from the stalker) still shows the ground detail around its focus.
const STRATEGY_REACH: f32 = 75.0;

/// Fades detail out `distance` meters (plus the strategy reach) from the camera.
/// The world streams nearby detail (collision, plants, ground meshes, walls)
/// around every anchor: the stalker, and the camera focus when zoomed in.
#[derive(Component, Default)]
pub struct StreamAnchor;

/// The point the camera looks at; the fog layers follow its ground height.
#[derive(Component, Default)]
pub struct ViewFocus;

/// Terrain tiles holding a stream anchor, sorted so unchanged sets compare equal.
fn anchor_tiles<'a>(anchors: impl IntoIterator<Item = &'a Transform>) -> Vec<IVec2> {
    let mut tiles: Vec<IVec2> = anchors
        .into_iter()
        .map(|at| {
            (at.translation.xz() / terrain::TILE_SIZE)
                .floor()
                .as_ivec2()
        })
        .collect();
    tiles.sort_unstable_by_key(|tile| (tile.x, tile.y));
    tiles.dedup();
    tiles
}

/// Every tile within `radius` tiles of an anchor tile (overlaps repeat).
fn neighborhoods(anchors: &[IVec2], radius: i32) -> impl Iterator<Item = IVec2> + '_ {
    anchors.iter().flat_map(move |&anchor| {
        (-radius..=radius)
            .flat_map(move |z| (-radius..=radius).map(move |x| anchor + IVec2::new(x, z)))
    })
}

/// Whether `tile` lies within `radius` tiles of any anchor tile.
fn near_anchor(tile: IVec2, anchors: &[IVec2], radius: i32) -> bool {
    anchors
        .iter()
        .any(|anchor| (tile - *anchor).abs().max_element() <= radius)
}

fn detail_range(distance: f32, fade: f32) -> VisibilityRange {
    let distance = distance + STRATEGY_REACH;
    VisibilityRange {
        end_margin: distance..distance + fade,
        use_aabb: true,
        ..default()
    }
}

fn range_scene_meshes(
    ready: On<WorldInstanceReady>,
    roots: Query<&VisibilityRange>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    mut commands: Commands,
) {
    let Ok(range) = roots.get(ready.entity) else {
        return;
    };
    for child in children.iter_descendants(ready.entity) {
        if meshes.contains(child) {
            commands.entity(child).insert(range.clone());
        }
    }
}

fn load_landscape_materials(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Res<Terrain>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(LandscapeMaterials {
        terrain: materials.add(load_terrain_material(
            &asset_server,
            &terrain.roads,
            &mut images,
        )),
        water: water_materials.add(water::material()),
        foliage: standard.add(scenery::foliage_material(&mut images)),
    });
}

fn spawn_world(
    mut commands: Commands,
    source: Res<LandscapeSource>,
    mut landscapes: ResMut<Assets<Landscape>>,
    (mut meshes, mut images): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>),
    materials: Res<LandscapeMaterials>,
    canopies: Res<CanopyModels>,
) {
    let mut landscape = landscapes
        .get_mut(&source.0)
        .expect("landscape loaded before entering the Zone");
    landscape.upload(&mut meshes, &mut images);
    commands.insert_resource(ground_fog::GroundFogField(landscape.fog_field()));
    let root = commands
        .spawn((WorldRoot, Transform::default(), Visibility::default()))
        .id();
    commands.insert_resource(landscape.scenery(root));
    commands.entity(root).with_children(|world| {
        landscape.spawn(
            world,
            &materials.terrain,
            &materials.water,
            &materials.foliage,
            &canopies,
        );
    });
}

fn spawn_sky(mut commands: Commands, mut mediums: ResMut<Assets<ScatteringMedium>>) {
    // Earth-like sky, thickened so the Zone sits in a dusty haze.
    let medium = mediums.add(ScatteringMedium::earth(256, 256).with_density_multiplier(HAZE));
    let atmosphere = Atmosphere::earth(medium);
    let planet_center = Transform::from_xyz(0.0, -atmosphere.inner_radius, 0.0);

    commands
        .spawn((WorldRoot, Transform::IDENTITY, Visibility::default()))
        .with_children(|world| {
            world.spawn((atmosphere, planet_center));

            // Raw (unscattered) sunlight is the input the atmosphere expects;
            // it reddens and fades both lights near the horizon.
            let hour = WorldClock::default().hour();
            world.spawn((
                Sun,
                DirectionalLight {
                    illuminance: lux::RAW_SUNLIGHT,
                    shadow_maps_enabled: true,
                    ..default()
                },
                SunDisk::EARTH,
                Transform::from_translation(weather::sun_direction(hour))
                    .looking_at(Vec3::ZERO, Vec3::Y),
            ));
            world.spawn((
                Moon,
                DirectionalLight {
                    illuminance: weather::MOON_LUX,
                    color: Color::linear_rgb(0.62, 0.74, 1.0),
                    shadow_maps_enabled: false,
                    ..default()
                },
                SunDisk {
                    angular_size: 0.0125,
                    intensity: 1.0,
                },
                Transform::from_translation(weather::moon_direction(hour))
                    .looking_at(Vec3::ZERO, Vec3::Y),
            ));
        });
}

fn despawn_world(mut commands: Commands, roots: Query<Entity, With<WorldRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    commands.remove_resource::<Scenery>();
    commands.remove_resource::<cordon::Cordon>();
    commands.remove_resource::<plants::PlantMaterials>();
    commands.remove_resource::<tree_fall::TreeBreakClock>();
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::camera::CameraPlugin;
    use bevy::mesh::MeshPlugin;

    use super::*;

    #[test]
    fn distant_detail_and_objects_behind_the_camera_are_culled() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            MeshPlugin,
            CameraPlugin,
        ));
        let world = app.world_mut();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(Cuboid::default());
        let camera = world.spawn((Camera3d::default(), Transform::IDENTITY)).id();
        let mut prop = |z| {
            world
                .spawn((
                    Mesh3d(mesh.clone()),
                    Transform::from_xyz(0.0, 0.0, z),
                    detail_range(100.0, 15.0),
                ))
                .id()
        };
        let front = prop(-14.0);
        let behind = prop(14.0);
        let distant = prop(-(175.0 + STRATEGY_REACH));
        let visible = |world: &World, entity| {
            world
                .get::<ViewVisibility>(entity)
                .expect("mesh visibility")
                .get()
        };
        app.update();
        assert!(visible(app.world(), front));
        assert!(!visible(app.world(), behind));
        assert!(!visible(app.world(), distant));

        app.world_mut()
            .get_mut::<Transform>(camera)
            .expect("camera")
            .rotation = Quat::from_rotation_y(std::f32::consts::PI);
        app.update();
        assert!(!visible(app.world(), front));
        assert!(visible(app.world(), behind));
    }
}
