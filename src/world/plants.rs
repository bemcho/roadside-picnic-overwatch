//! Grounded, biome-specific plants streamed with the surrounding scenery.
//! Every weed and bush is an imported model: a light far model, a textured
//! near model and, for the plant nearest the stalker, its full UHD scan. The
//! wind bends them in their vertex shader from the root up.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::{self, PlantModel, PreloadedAssets};

use super::cordon::{near_wall_or_checkpoint, reserve_scenery, shelterbelt};
use super::hydrology::Wetlands;
use super::scenery::forest_density;
use super::terrain::{
    BASE, HALF_SIZE, TILE_SIZE, Terrain, lattice_hash, lerp, smoothstep, value_noise,
};
use super::weather::Wind;
use super::{detail_range, range_scene_meshes};

const CELL: f32 = 8.0;
const SEED: u32 = 0x1035_F108;
/// Plants in a patch of dry grass, and of the other weeds and bushes.
const GRASS_PATCH: usize = 5;
const PATCH: usize = 15;
/// The near model shows within this distance of the camera, the far one out
/// to `FAR` (plus the raised camera's reach).
const NEAR: f32 = 30.0;
const NEAR_FADE: f32 = 6.0;
const FAR: f32 = 66.0;
const FAR_FADE: f32 = 10.0;
/// The plant nearest the stalker within this distance shows its full scan.
const FULL_RANGE: f32 = 8.0;
/// Brightens glowweed's own emission enough to show at dusk.
const GLOW_GAIN: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    DryGrass,
    Glowweed,
    Hogweed,
    Shrub,
    Thornbush,
}

impl Kind {
    const ALL: [Self; 5] = [
        Self::DryGrass,
        Self::Glowweed,
        Self::Hogweed,
        Self::Shrub,
        Self::Thornbush,
    ];

    fn size_and_roots(self) -> (f32, f32, f32) {
        match self {
            Self::DryGrass => (0.50, 1.08, 0.18),
            Self::Glowweed => (0.64, 1.40, 0.37),
            Self::Hogweed => (1.1, 1.92, 0.17),
            Self::Shrub => (1.0, 1.95, 0.40),
            Self::Thornbush => (0.90, 1.58, 0.27),
        }
    }

    /// The imported plants that grow in this kind's habitat.
    fn models(self) -> &'static [PlantModel] {
        match self {
            Self::DryGrass => &assets::DRY_WEEDS,
            Self::Glowweed => &assets::GLOWWEEDS,
            Self::Hogweed => &assets::DAMP_WEEDS,
            Self::Shrub => &assets::SHRUBS,
            Self::Thornbush => &assets::THORNBUSHES,
        }
    }

    fn patch(self) -> usize {
        if self == Self::DryGrass {
            GRASS_PATCH
        } else {
            PATCH
        }
    }
}

pub(super) type PlantMaterial = ExtendedMaterial<StandardMaterial, PlantWind>;

/// The wind bending the plants from their roots. Every plant material shares
/// one small buffer: rewriting it each frame leaves the materials unchanged,
/// where changing a material would re-specialize every plant drawn with it.
#[derive(Asset, AsBindGroup, Reflect, Clone, Debug, Default)]
pub(super) struct PlantWind {
    /// One vec4: xy downwind bend in meters at a 1 m tip, z seconds of wind,
    /// w gust.
    #[storage(100, read_only)]
    wind: Handle<ShaderBuffer>,
}

impl MaterialExtension for PlantWind {
    fn vertex_shader() -> ShaderRef {
        assets::PLANT_WIND_SHADER.into()
    }

    // Plants cast no shadows and need no depth prepass.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// One imported plant's far and near meshes and their shared, wind-bent material.
#[derive(Default)]
struct PlantLook {
    far: Handle<Mesh>,
    near: Handle<Mesh>,
    paint: Handle<PlantMaterial>,
}

/// Every plant model's look, by kind and then variant, and their wind buffer.
#[derive(Resource)]
pub(super) struct PlantLooks {
    looks: [Vec<PlantLook>; Kind::ALL.len()],
    wind: Handle<ShaderBuffer>,
}

impl Default for PlantLooks {
    fn default() -> Self {
        Self {
            looks: Kind::ALL.map(|kind| kind.models().iter().map(|_| default()).collect()),
            wind: default(),
        }
    }
}

impl PlantLooks {
    fn of(&self, site: &PlantSite) -> &PlantLook {
        &self.looks[site.kind as usize][site.variant]
    }
}

pub(super) fn preload_plants(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for kind in Kind::ALL {
        for model in kind.models() {
            let (near, paint) = assets::load_single_primitive(&server, model.near);
            let (far, _) = assets::load_single_primitive(&server, model.far);
            preloaded
                .0
                .extend([near.untyped(), paint.untyped(), far.untyped()]);
        }
    }
}

pub(super) fn prepare_plant_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<PlantMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let wind = buffers.add(ShaderBuffer::from(vec![Vec4::ZERO]));
    let looks = Kind::ALL.map(|kind| {
        kind.models()
            .iter()
            .map(|model| {
                // The far model draws with the near model's material.
                let (near, paint) = assets::load_single_primitive(&server, model.near);
                let (far, _) = assets::load_single_primitive(&server, model.far);
                let paint = paints
                    .get(&paint)
                    .expect("plant materials finish preloading before the Zone opens");
                let mut base = standard_material_from_gltf_material(paint);
                if kind == Kind::Glowweed {
                    base.emissive *= GLOW_GAIN;
                }
                PlantLook {
                    far,
                    near,
                    paint: materials.add(PlantMaterial {
                        base,
                        extension: PlantWind { wind: wind.clone() },
                    }),
                }
            })
            .collect()
    });
    commands.insert_resource(PlantLooks { looks, wind });
}

/// Hands the wind to the plants' shader.
pub(super) fn bend_plants(
    wind: Res<Wind>,
    looks: Res<PlantLooks>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let bend = wind.direction() * (0.05 + 0.18 * wind.strength());
    if let Some(mut buffer) = buffers.get_mut(&looks.wind) {
        buffer.clear();
        buffer.extend([Vec4::new(bend.x, bend.y, wind.seconds, wind.gust)]);
    }
}

/// Sways the full scan from its root: a lean downwind and a flutter. Its
/// scene keeps the scan's own materials, so the shader can't bend it.
pub(super) fn sway_full_plants(wind: Res<Wind>, mut fulls: Query<(&FullPlant, &mut Transform)>) {
    let downwind = wind.direction();
    let axis = Vec3::new(downwind.y, 0.0, -downwind.x);
    let strength = wind.strength();
    for (full, mut pose) in &mut fulls {
        let flutter = (wind.seconds * 1.7 + full.phase).sin();
        let angle = (0.012 + 0.05 * strength) * (0.75 + 0.25 * flutter);
        pose.rotation = Quat::from_axis_angle(axis, angle) * full.rest;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct PlantSite {
    kind: Kind,
    /// Which of the kind's models grows here.
    variant: usize,
    pose: Transform,
}

/// An imported plant: its near model, with the far model as a child.
#[derive(Component)]
pub(in crate::world) struct Plant {
    model: &'static PlantModel,
}

/// The full scan standing in for the plant nearest the stalker.
#[derive(Component)]
pub(in crate::world) struct FullPlant {
    plant: Entity,
    /// The upright pose's rotation, which the wind sways around.
    rest: Quat,
    phase: f32,
}

impl PlantSite {
    /// A plant of `kind` at `pose`, its model picked by its position.
    fn new(kind: Kind, pose: Transform) -> Self {
        let cell = (pose.translation.xz() * 191.0).as_ivec2();
        let count = kind.models().len();
        let variant = (lattice_hash(cell.x, cell.y, SEED + 913) * count as f32) as usize;
        Self {
            kind,
            variant: variant.min(count - 1),
            pose,
        }
    }

    fn grounded(
        terrain: &Terrain,
        kind: Kind,
        at: Vec2,
        y: f32,
        size: f32,
        angle: f32,
    ) -> Option<Self> {
        let (small, big, roots) = kind.size_and_roots();
        let scale = lerp(small, big, size);
        let base = root_height(terrain, at, y, angle, scale * roots);
        if y - base > 0.22 * scale {
            return None;
        }
        Some(Self::new(
            kind,
            Transform::from_xyz(at.x, base - 0.06, at.y)
                .with_rotation(Quat::from_rotation_y(angle))
                .with_scale(Vec3::splat(scale)),
        ))
    }
}

fn plant_sites(terrain: &Terrain, tile: IVec2) -> Vec<PlantSite> {
    let mut wild = Vec::new();
    for layer in 0..5 {
        grow_plant_layer(terrain, tile, layer, &mut wild);
    }
    let mut belt = Vec::new();
    grow_hangar_plants(terrain, tile, &mut belt);
    expand_patches(terrain, &wild, false)
        .chain(expand_patches(terrain, &belt, true))
        .collect()
}

/// Spreads each seed into a patch of its kind: a few dry grasses, or a
/// thicket of weeds or bushes.
fn expand_patches<'a>(
    terrain: &'a Terrain,
    seeds: &'a [PlantSite],
    in_belt: bool,
) -> impl Iterator<Item = PlantSite> + 'a {
    seeds.iter().flat_map(move |&seed| {
        let root = seed.pose.translation.xz();
        let id = (root * 100.0).as_ivec2();
        // The belt's seeds already stand close together.
        let patch_size = if in_belt {
            seed.kind.patch().div_ceil(3)
        } else {
            seed.kind.patch()
        };
        let radius = if in_belt { 1.05 } else { 1.35 } * (patch_size as f32).sqrt();
        std::iter::once(seed).chain((1..patch_size).filter_map(move |i| {
            let roll = |n| lattice_hash(id.x, id.y, SEED + i as u32 * 17 + n);
            let angle = i as f32 * 2.399_963 + roll(0) * 0.50;
            let reach = radius * ((i as f32 - 0.42) / patch_size as f32).sqrt();
            let offset = Vec2::from_angle(angle) * reach;
            [1.0, 0.60, 0.32].into_iter().find_map(|factor| {
                let at = root + factor * offset;
                let wet = Wetlands::at(at.x, at.y);
                let ground = terrain.mesh_height_at(at);
                let max_depth = match seed.kind {
                    Kind::Hogweed | Kind::Glowweed if !in_belt => 0.20,
                    _ => 0.025,
                };
                let clear = if in_belt {
                    shelterbelt(at) > 0.0 && !near_wall_or_checkpoint(at, 0.7)
                } else {
                    !reserve_scenery(at, 1.6)
                        && !super::sites::clear_footprint(at, 1.0)
                        && terrain.base_mask(at.x, at.y) <= 0.05
                };
                (clear
                    && at.abs().max_element() < HALF_SIZE - 3.0
                    && terrain.road_mask(at.x, at.y) <= 0.025
                    && wet.depth(ground) <= max_depth
                    && (in_belt || seed.kind != Kind::Glowweed || wet.wetness() > 0.28)
                    && (ground - seed.pose.translation.y).abs() < reach * factor * 0.54 + 0.22)
                    .then(|| {
                        PlantSite::new(
                            seed.kind,
                            Transform::from_xyz(at.x, ground - 0.055, at.y)
                                .with_rotation(Quat::from_rotation_y(roll(2) * TAU))
                                .with_scale(seed.pose.scale * lerp(0.55, 1.0, roll(3))),
                        )
                    })
            })
        }))
    })
}

fn grow_plant_layer(terrain: &Terrain, tile: IVec2, layer: u32, sites: &mut Vec<PlantSite>) {
    let cells = (TILE_SIZE / CELL) as i32;
    let start = tile * cells;
    for z in start.y..start.y + cells {
        for x in start.x..start.x + cells {
            let roll = |n| lattice_hash(x, z, SEED + 17 * layer + n);
            let at = (Vec2::new(x as f32, z as f32) + Vec2::new(roll(0), roll(1))) * CELL;
            if at.abs().max_element() > HALF_SIZE - 3.0
                || reserve_scenery(at, 1.6)
                || super::sites::clear_footprint(at, 1.0)
                || terrain.base_mask(at.x, at.y) > 0.05
                || terrain.road_mask(at.x, at.y) > 0.025
            {
                continue;
            }
            let y = terrain.height_at(at.x, at.y);
            let patch = smoothstep(0.17, 0.73, value_noise(at.x / 34.0, at.y / 34.0, SEED + 9));
            let density = 0.42 * patch * smoothstep(97.0, 44.0, y);
            if roll(2) >= density {
                continue;
            }
            let wet = Wetlands::at(at.x, at.y);
            // Plants stand on the rendered mesh, so judge the water there.
            let ground = terrain.mesh_height_at(at);
            let depth = wet.depth(ground);
            if depth > 0.20 || terrain.normal_at(at.x, at.y).y < 0.87 {
                continue;
            }
            let damp = wet.wetness();
            let dry = if depth > 0.025 { 0.0 } else { 1.0 - damp };
            let forest = forest_density(at.x, at.y);
            let glow = smoothstep(0.28, 0.78, damp);
            let weights = [
                (0.38 - forest * 0.22) * dry,
                0.34 * glow,
                0.07 * (1.0 - forest) * dry + 0.19 * damp,
                (0.08 + forest * 0.32) * dry,
                (0.08 + 0.14 * smoothstep(4.0, 28.0, y)) * dry,
            ];
            let Some(kind) = pick_plant(roll(3), weights) else {
                continue;
            };
            sites.extend(PlantSite::grounded(
                terrain,
                kind,
                at,
                ground,
                roll(4),
                roll(5) * TAU,
            ));
        }
    }
}

fn grow_hangar_plants(terrain: &Terrain, tile: IVec2, sites: &mut Vec<PlantSite>) {
    const CELLS: i32 = 26;
    let mid = (tile.as_vec2() + Vec2::splat(0.5)) * TILE_SIZE;
    if (mid - Vec2::from(BASE)).abs().max_element() > 113.0 + TILE_SIZE * 0.5 {
        return;
    }
    let cell = TILE_SIZE / CELLS as f32;
    let start = tile * CELLS;
    for z in start.y..start.y + CELLS {
        for x in start.x..start.x + CELLS {
            let roll = |n| lattice_hash(x, z, SEED + 419 + n);
            let at = (Vec2::new(x as f32, z as f32)
                + Vec2::new(lerp(0.07, 0.93, roll(0)), lerp(0.07, 0.93, roll(1))))
                * cell;
            let density = shelterbelt(at) * 0.85;
            if roll(2) >= density
                || near_wall_or_checkpoint(at, 0.7)
                || terrain.road_mask(at.x, at.y) > 0.025
                || terrain.water_depth_at(at.x, at.y) > 0.025
                || terrain.normal_at(at.x, at.y).y < 0.89
            {
                continue;
            }
            let kind = pick_plant(roll(3), [0.18, 0.12, 0.22, 0.25, 0.24])
                .expect("shelterbelt plant weights cover the random roll");
            sites.extend(PlantSite::grounded(
                terrain,
                kind,
                at,
                terrain.mesh_height_at(at),
                roll(4),
                roll(5) * TAU,
            ));
        }
    }
}

fn pick_plant(mut roll: f32, weights: [f32; Kind::ALL.len()]) -> Option<Kind> {
    for (kind, weight) in Kind::ALL.into_iter().zip(weights) {
        if roll < weight {
            return Some(kind);
        }
        roll -= weight;
    }
    None
}

fn root_height(terrain: &Terrain, at: Vec2, height: f32, angle: f32, radius: f32) -> f32 {
    (0..6).fold(height, |low, i| {
        let p = at + Vec2::from_angle(angle + TAU / 6.0 * i as f32) * radius;
        low.min(terrain.height_at(p.x, p.y))
    })
}

/// A tile's plants, grown on a worker thread so streaming never stalls a frame.
pub(super) struct TilePlants(Vec<PlantSite>);

impl TilePlants {
    pub(super) fn grow(terrain: &Terrain, tile: IVec2) -> Self {
        Self(plant_sites(terrain, tile))
    }
}

/// Plants still growing for a streamed scenery tile.
#[derive(Component)]
pub(super) struct PendingPlants(Task<TilePlants>);

impl PendingPlants {
    pub(super) fn grow(terrain: Arc<Terrain>, tile: IVec2) -> Self {
        Self(AsyncComputeTaskPool::get().spawn(async move { TilePlants::grow(&terrain, tile) }))
    }
}

/// Spawns at most one grown tile per frame, spreading the spawns.
pub(super) fn spawn_grown_plants(
    mut commands: Commands,
    mut tiles: Query<(Entity, &mut PendingPlants)>,
    looks: Res<PlantLooks>,
) {
    let Some((tile, plants)) = tiles
        .iter_mut()
        .find_map(|(tile, mut pending)| Some((tile, check_ready(&mut pending.0)?)))
    else {
        return;
    };
    commands
        .entity(tile)
        .remove::<PendingPlants>()
        .with_children(|parent| spawn_plants(parent, &plants.0, &looks));
}

/// Each plant shows its near model up close and crossfades to its far model.
/// Plants sharing a model are drawn as one GPU instance batch.
fn spawn_plants(parent: &mut ChildSpawnerCommands, sites: &[PlantSite], looks: &PlantLooks) {
    let near = VisibilityRange {
        end_margin: NEAR..NEAR + NEAR_FADE,
        use_aabb: true,
        ..default()
    };
    let far = VisibilityRange {
        start_margin: near.end_margin.clone(),
        ..detail_range(FAR, FAR_FADE)
    };
    for site in sites {
        let look = looks.of(site);
        parent
            .spawn((
                Plant {
                    model: &site.kind.models()[site.variant],
                },
                site.pose,
                Mesh3d(look.near.clone()),
                MeshMaterial3d(look.paint.clone()),
                near.clone(),
            ))
            .with_child((
                Mesh3d(look.far.clone()),
                MeshMaterial3d(look.paint.clone()),
                far.clone(),
            ));
    }
}

/// Swaps the plant nearest the stalker for its full scan, and back. The
/// multipart scans differ in shape from their near models, so those swaps show.
pub(super) fn stream_full_plants(
    mut commands: Commands,
    player: Query<&Transform, With<crate::player::Player>>,
    plants: Query<(Entity, &Plant, &Transform, &ChildOf)>,
    fulls: Query<(Entity, &FullPlant)>,
    server: Res<AssetServer>,
) {
    let nearest = player.single().ok().and_then(|player| {
        plants
            .iter()
            .filter_map(|(entity, plant, pose, tile)| {
                let full = plant.model.full?;
                let distance = player.translation.xz().distance(pose.translation.xz());
                (distance <= FULL_RANGE).then_some((distance, entity, full, pose, tile))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    });
    let active = nearest.map(|(_, entity, ..)| entity);
    for (entity, full) in &fulls {
        if active != Some(full.plant) {
            commands.entity(entity).despawn();
            if plants.contains(full.plant) {
                commands.entity(full.plant).insert(Visibility::Inherited);
            }
        }
    }
    let Some((_, entity, full, pose, tile)) = nearest else {
        return;
    };
    if fulls.iter().any(|(_, full)| full.plant == entity) {
        return;
    }
    let at = pose.translation;
    commands
        .spawn((
            FullPlant {
                plant: entity,
                rest: pose.rotation,
                phase: (at.x * 0.37 + at.z * 0.23) % TAU,
            },
            *pose,
            ChildOf(tile.parent()),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(full))),
            detail_range(12.0, 2.0),
            Visibility::Hidden,
        ))
        .observe(range_scene_meshes)
        .observe(reveal_full_plant);
}

/// Shows a loaded full scan and hides the plant it stands in for.
fn reveal_full_plant(
    ready: On<WorldInstanceReady>,
    fulls: Query<&FullPlant>,
    mut commands: Commands,
) {
    let Ok(full) = fulls.get(ready.entity) else {
        return;
    };
    commands.entity(ready.entity).insert(Visibility::Inherited);
    commands.entity(full.plant).insert(Visibility::Hidden);
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use bevy::asset::AssetPlugin;

    use super::*;
    use crate::world::terrain::base_tile;
    use crate::world::{Collider, SWAMP};

    #[test]
    fn every_plant_model_ships_all_three_tiers() {
        for kind in Kind::ALL {
            assert!(!kind.models().is_empty());
            for model in kind.models() {
                for path in [model.far, model.near].into_iter().chain(model.full) {
                    assert!(Path::new("assets").join(path).is_file(), "missing {path}");
                }
            }
        }
    }

    #[test]
    fn streamed_tiles_grow_plants_in_the_background_then_spawn_them() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<PlantLooks>()
            .add_systems(Update, spawn_grown_plants);
        let tile = base_tile() + IVec2::NEG_Y;
        let expected = TilePlants::grow(&Terrain::new(), tile);
        let root = app
            .world_mut()
            .spawn(PendingPlants::grow(Arc::new(Terrain::new()), tile))
            .id();
        for _ in 0..2_000 {
            app.update();
            if !app.world().entity(root).contains::<PendingPlants>() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let world = app.world_mut();
        assert!(!world.entity(root).contains::<PendingPlants>());
        let children = world.entity(root).get::<Children>().expect("grown plants");
        let plants = children
            .iter()
            .filter(|&child| world.entity(child).contains::<Plant>())
            .count();
        assert_eq!(plants, expected.0.len());
        assert!(plants > 0);
    }

    #[test]
    fn patches_respect_habitats_and_clearances() {
        let terrain = Terrain::new();
        let wet_tile = (SWAMP / TILE_SIZE).floor().as_ivec2();
        assert_eq!(
            plant_sites(&terrain, wet_tile),
            plant_sites(&terrain, wet_tile)
        );
        let mut counts = [0; Kind::ALL.len()];
        let mut seeds = [0; Kind::ALL.len()];
        for tile in [
            wet_tile,
            wet_tile + IVec2::X,
            base_tile(),
            IVec2::new(3, -1),
            IVec2::new(-1, 0),
            IVec2::new(-11, -9),
        ] {
            let mut wild = Vec::new();
            for layer in 0..5 {
                grow_plant_layer(&terrain, tile, layer, &mut wild);
            }
            for seed in &wild {
                seeds[seed.kind as usize] += 1;
            }
            for site in expand_patches(&terrain, &wild, false) {
                let p = site.pose.translation;
                counts[site.kind as usize] += 1;
                assert!(site.variant < site.kind.models().len());
                let wet = Wetlands::at(p.x, p.z);
                let y = terrain.mesh_height_at(p.xz());
                assert!(!reserve_scenery(p.xz(), 1.6));
                assert!(terrain.road_mask(p.x, p.z) <= 0.025);
                assert!(wet.depth(y) <= 0.23);
                assert!(p.y < y && y - p.y < 0.5);
                if site.kind == Kind::Glowweed {
                    assert!(wet.wetness() > 0.28);
                }
            }
        }
        assert!(
            counts.into_iter().all(|count| count >= 12),
            "plant counts: {counts:?}"
        );
        for kind in Kind::ALL {
            let (seeds, plants) = (seeds[kind as usize], counts[kind as usize]);
            assert!(
                (seeds..=seeds * kind.patch()).contains(&plants),
                "{kind:?}: {seeds} seeds -> {plants}"
            );
        }
    }

    #[test]
    fn denser_shelterbelt_keeps_a_clear_yard_and_favors_the_rear() {
        let terrain = Terrain::new();
        let mut count = [0; Kind::ALL.len()];
        let mut rear = 0;
        let mut seeds = Vec::new();
        for z in -2..=1 {
            for x in -2..=2 {
                grow_hangar_plants(&terrain, base_tile() + IVec2::new(x, z), &mut seeds);
            }
        }
        let sites: Vec<_> = expand_patches(&terrain, &seeds, true).collect();
        for site in &sites {
            let at = site.pose.translation;
            let offset = at.xz() - Vec2::from(BASE);
            assert!(offset.x.abs() > 49.0 || offset.y < -44.0);
            assert!(terrain.road_mask(at.x, at.z) <= 0.025);
            assert!(at.y <= terrain.mesh_height_at(at.xz()) + 0.02);
            assert!(!near_wall_or_checkpoint(at.xz(), 0.7));
            count[site.kind as usize] += 1;
            rear += usize::from(offset.y < -44.0);
        }
        assert!(rear > sites.len() / 2);
        assert!(
            count[Kind::Shrub as usize] + count[Kind::Thornbush as usize] > 300,
            "{count:?}"
        );
        assert!(count.into_iter().all(|n| n >= 45), "{count:?}");
    }

    #[test]
    fn plants_are_imported_models_with_near_and_far_tiers_and_no_collision() {
        let terrain = Terrain::new();
        let tile = base_tile() + IVec2::NEG_Y;
        let sites = plant_sites(&terrain, tile);
        assert!(
            (300..=3_000).contains(&sites.len()),
            "{} plants",
            sites.len()
        );
        let mut world = World::new();
        let root = world.spawn(Transform::IDENTITY).id();
        world
            .commands()
            .entity(root)
            .with_children(|parent| spawn_plants(parent, &sites, &PlantLooks::default()));
        world.flush();
        let mut plants = 0;
        for (range, children, collides) in world
            .query_filtered::<(&VisibilityRange, &Children, Has<Collider>), With<Plant>>()
            .iter(&world)
        {
            assert!(!collides);
            let far = world
                .get::<VisibilityRange>(children[0])
                .expect("far model");
            assert!(world.get::<Mesh3d>(children[0]).is_some());
            assert_eq!(range.end_margin, far.start_margin);
            assert!(far.end_margin.start > range.end_margin.end);
            plants += 1;
        }
        assert_eq!(plants, sites.len());
    }
}
