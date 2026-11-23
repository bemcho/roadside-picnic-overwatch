//! Grounded, biome-specific plants streamed with the surrounding scenery.
//! Every weed and bush is an imported model: a light far model, a textured
//! near model and, for the plant nearest the stalker, its full UHD scan. The
//! wind bends them in their vertex shader from the root up.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::{NoAutoAabb, VisibilityRange};
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

use crate::assets::gpu_scans::{GpuScans, UPLOAD_LEAD};
use crate::assets::{self, PlantModel, PreloadedAssets};

use super::cordon::{
    CRAWL_HOLE, PERIMETER_BELT, PERIMETER_X, bushy, near_wall_or_checkpoint, reserve_scenery,
    shelterbelt, thicket,
};
use super::crush::Trampled;
use super::hydrology::Wetlands;
use super::scenery::forest_density;
use super::terrain::{
    BASE, HALF_SIZE, TILE_SIZE, Terrain, lattice_hash, lerp, smoothstep, value_noise,
};
use super::weather::Wind;
use super::{StreamAnchor, anchor_distance, anchor_tiles, detail_range, range_scene_meshes};

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
/// Plants spawned per frame at most: a tile's thousands at once stalled the
/// frame in Bevy's per-entity material and visibility bookkeeping.
const PLANTS_PER_FRAME: usize = 150;
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
/// The meshes keep their CPU copy, so Bevy would rescan every vertex for each
/// new plant: their bounds are measured once instead.
#[derive(Default)]
struct PlantLook {
    far: Handle<Mesh>,
    near: Handle<Mesh>,
    paint: Handle<PlantMaterial>,
    far_bounds: Option<Aabb>,
    near_bounds: Option<Aabb>,
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

/// Every plant's full scan, for the RAM fill.
pub(super) fn uhd_files() -> impl Iterator<Item = &'static str> {
    Kind::ALL
        .into_iter()
        .flat_map(Kind::models)
        .filter_map(|model| model.full)
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
    meshes: Res<Assets<Mesh>>,
) {
    let wind = buffers.add(ShaderBuffer::from(vec![Vec4::ZERO]));
    let bounds = |mesh: &Handle<Mesh>| meshes.get(mesh).and_then(MeshAabb::get_aabb);
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
                    far_bounds: bounds(&far),
                    near_bounds: bounds(&near),
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

#[cfg(test)]
impl Plant {
    pub(in crate::world) fn test() -> Self {
        Self {
            model: &Kind::DryGrass.models()[0],
        }
    }
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
    grow_belt_plants(terrain, tile, &mut belt);
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
        // The belt's seeds already stand close together; the crawl hole's
        // thicket still grows full patches.
        let patch_size = if in_belt && !thicket(root) {
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
                    shelterbelt(at) > 0.0
                        && !near_wall_or_checkpoint(at, 0.7)
                        && !super::sites::clear_footprint(at, 1.0)
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

/// Whether `tile` reaches into a belt: around the hangar, along the
/// Perimeter, or over the crawl hole.
fn reaches_belt(tile: IVec2) -> bool {
    let low = tile.as_vec2() * TILE_SIZE;
    let high = low + TILE_SIZE;
    let mid = (low + high) * 0.5;
    (mid - Vec2::from(BASE)).abs().max_element() <= 113.0 + TILE_SIZE * 0.5
        || (low.x < PERIMETER_X + PERIMETER_BELT.1 && high.x > PERIMETER_X + PERIMETER_BELT.0)
        || (mid - CRAWL_HOLE).abs().max_element() <= TILE_SIZE
}

fn grow_belt_plants(terrain: &Terrain, tile: IVec2, sites: &mut Vec<PlantSite>) {
    const CELLS: i32 = 26;
    if !reaches_belt(tile) {
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
                || super::sites::clear_footprint(at, 1.0)
                || terrain.road_mask(at.x, at.y) > 0.025
                || terrain.water_depth_at(at.x, at.y) > 0.025
                || terrain.normal_at(at.x, at.y).y < 0.89
            {
                continue;
            }
            // Dry grass, glowweed, hogweed, shrubs and thornbushes.
            let weights = if bushy(at) {
                [0.06, 0.04, 0.10, 0.42, 0.40]
            } else {
                [0.18, 0.12, 0.22, 0.25, 0.24]
            };
            let kind = pick_plant(roll(3), weights)
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
pub(super) struct PendingPlants {
    tile: IVec2,
    task: Task<TilePlants>,
}

impl PendingPlants {
    pub(super) fn grow(terrain: Arc<Terrain>, tile: IVec2) -> Self {
        let task =
            AsyncComputeTaskPool::get().spawn(async move { TilePlants::grow(&terrain, tile) });
        Self { tile, task }
    }
}

/// A grown tile's plants still waiting to spawn.
#[derive(Component)]
pub(super) struct GrownPlants {
    tile: IVec2,
    sites: Vec<PlantSite>,
}

/// Whether streamed tiles still have plants to grow or spawn. The entry
/// curtain stays up until they have all spawned.
#[derive(Resource, Default, PartialEq)]
pub struct PlantsGrowing(pub bool);

/// Tiles with plants still to grow or spawn.
type Growing = Or<(With<PendingPlants>, With<GrownPlants>)>;

/// Notes whether any tile still has plants to grow or spawn.
pub(super) fn note_growing_plants(tiles: Query<(), Growing>, mut growing: ResMut<PlantsGrowing>) {
    growing.set_if_neq(PlantsGrowing(!tiles.is_empty()));
}

/// Hands each tile whose plants finished growing over to `spawn_grown_plants`.
pub(super) fn finish_growing_plants(
    mut commands: Commands,
    mut tiles: Query<(Entity, &mut PendingPlants)>,
) {
    for (tile, mut pending) in &mut tiles {
        if let Some(plants) = check_ready(&mut pending.task) {
            commands
                .entity(tile)
                .remove::<PendingPlants>()
                .insert(GrownPlants {
                    tile: pending.tile,
                    sites: plants.0,
                });
        }
    }
}

/// Spawns at most `PLANTS_PER_FRAME` grown plants per frame, nearest an anchor
/// first, so a new tile fills in over a few frames instead of stalling one.
/// Trampled plants stay gone.
pub(super) fn spawn_grown_plants(
    mut commands: Commands,
    anchors: Query<&Transform, With<StreamAnchor>>,
    mut tiles: Query<(Entity, &mut GrownPlants)>,
    looks: Res<PlantLooks>,
    trampled: Res<Trampled>,
) {
    let centers = anchor_tiles(anchors);
    let mut tiles: Vec<_> = tiles.iter_mut().collect();
    tiles.sort_by_key(|(_, grown)| anchor_distance(grown.tile, &centers));
    let mut budget = PLANTS_PER_FRAME;
    for (tile, mut grown) in tiles {
        let take = budget.min(grown.sites.len());
        let at = grown.sites.len() - take;
        let mut batch = grown.sites.split_off(at);
        batch.retain(|site| !trampled.contains(site.pose.translation));
        let mut tile = commands.entity(tile);
        tile.with_children(|parent| spawn_plants(parent, &batch, &looks));
        if grown.sites.is_empty() {
            tile.remove::<GrownPlants>();
        }
        budget -= take;
        if budget == 0 {
            break;
        }
    }
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
        let mut plant = parent.spawn((
            Plant {
                model: &site.kind.models()[site.variant],
            },
            site.pose,
            Mesh3d(look.near.clone()),
            MeshMaterial3d(look.paint.clone()),
            near.clone(),
        ));
        bound(&mut plant, look.near_bounds);
        plant.with_children(|plant| {
            let mut far_model = plant.spawn((
                Mesh3d(look.far.clone()),
                MeshMaterial3d(look.paint.clone()),
                far.clone(),
            ));
            bound(&mut far_model, look.far_bounds);
        });
    }
}

/// Gives a plant model its measured bounds, so Bevy skips recomputing them.
fn bound(model: &mut EntityCommands, bounds: Option<Aabb>) {
    if let Some(bounds) = bounds {
        model.insert((bounds, NoAutoAabb));
    }
}

type PlantsQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Plant, &'static Transform, &'static ChildOf)>;

/// A plant that has a full scan and stands within `FULL_RANGE` of the stalker.
struct NearPlant<'a> {
    entity: Entity,
    full: &'static str,
    pose: &'a Transform,
    /// The tile the plant stands on.
    tile: Entity,
}

/// Swaps the plant nearest the stalker for its full scan, and back. The
/// multipart scans differ in shape from their near models, so those swaps show.
///
/// A scan is wanted from `UPLOAD_LEAD` out, so it is on the GPU by the time
/// the stalker is near, and it shows only once it is. Until then the plant's
/// own model stays. A scan that leaves the GPU takes its instance with it,
/// which is when the GPU frees it.
pub(super) fn stream_full_plants(
    mut commands: Commands,
    player: Query<&Transform, With<crate::player::Player>>,
    plants: PlantsQuery,
    fulls: Query<(Entity, &FullPlant)>,
    mut scans: ResMut<GpuScans>,
) {
    let nearest = player
        .single()
        .ok()
        .and_then(|player| want_plant_scans(&mut scans, player.translation.xz(), &plants));
    let active = nearest.and_then(|near| Some((scans.scene(near.full)?, near)));
    let active_plant = active.as_ref().map(|(_, near)| near.entity);
    for (entity, full) in &fulls {
        if active_plant != Some(full.plant) {
            commands.entity(entity).despawn();
            if plants.contains(full.plant) {
                commands.entity(full.plant).insert(Visibility::Inherited);
            }
        }
    }
    let Some((scene, near)) = active else {
        return;
    };
    if fulls.iter().any(|(_, full)| full.plant == near.entity) {
        return;
    }
    let at = near.pose.translation;
    commands
        .spawn((
            FullPlant {
                plant: near.entity,
                rest: near.pose.rotation,
                phase: (at.x * 0.37 + at.z * 0.23) % TAU,
            },
            *near.pose,
            ChildOf(near.tile),
            WorldAssetRoot(scene),
            detail_range(12.0, 2.0),
            Visibility::Hidden,
        ))
        .observe(range_scene_meshes)
        .observe(reveal_full_plant);
}

/// Wants the full scan of every plant within `UPLOAD_LEAD` of `player`, and
/// returns the nearest plant within `FULL_RANGE` that has one.
fn want_plant_scans<'a>(
    scans: &mut GpuScans,
    player: Vec2,
    plants: &'a PlantsQuery,
) -> Option<NearPlant<'a>> {
    let mut nearest: Option<(f32, NearPlant)> = None;
    for (entity, plant, pose, tile) in plants {
        let Some(full) = plant.model.full else {
            continue;
        };
        let distance = player.distance(pose.translation.xz());
        if distance > UPLOAD_LEAD {
            continue;
        }
        scans.want(full, distance - FULL_RANGE);
        let closer = nearest.as_ref().is_none_or(|(best, _)| distance < *best);
        if distance <= FULL_RANGE && closer {
            let near = NearPlant {
                entity,
                full,
                pose,
                tile: tile.parent(),
            };
            nearest = Some((distance, near));
        }
    }
    nearest.map(|(_, near)| near)
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
            .init_resource::<Trampled>()
            .add_systems(Update, (finish_growing_plants, spawn_grown_plants).chain());
        let tile = base_tile() + IVec2::NEG_Y;
        let expected = TilePlants::grow(&Terrain::new(), tile);
        let root = app
            .world_mut()
            .spawn(PendingPlants::grow(Arc::new(Terrain::new()), tile))
            .id();
        for _ in 0..2_000 {
            app.update();
            let root = app.world().entity(root);
            if !root.contains::<PendingPlants>() && !root.contains::<GrownPlants>() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let world = app.world_mut();
        assert!(!world.entity(root).contains::<PendingPlants>());
        assert!(!world.entity(root).contains::<GrownPlants>());
        let children = world.entity(root).get::<Children>().expect("grown plants");
        let plants = children
            .iter()
            .filter(|&child| world.entity(child).contains::<Plant>())
            .count();
        assert_eq!(plants, expected.0.len());
        assert!(plants > 0);
    }

    /// `count` grown dry grasses in a row on `tile`.
    fn grown(tile: IVec2, count: usize) -> GrownPlants {
        let sites = (0..count)
            .map(|i| PlantSite {
                kind: Kind::DryGrass,
                variant: 0,
                pose: Transform::from_xyz(i as f32, 0.0, 0.0),
            })
            .collect();
        GrownPlants { tile, sites }
    }

    /// An app spawning plants from a grown tile of `count` dry grasses in a row.
    fn grown_tile(count: usize) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_resource::<PlantLooks>()
            .init_resource::<Trampled>()
            .add_systems(Update, spawn_grown_plants);
        let tile = app.world_mut().spawn(grown(IVec2::ZERO, count)).id();
        (app, tile)
    }

    fn plants_on(app: &mut App, tile: Entity) -> usize {
        let world = app.world_mut();
        let mut plants = world.query_filtered::<&ChildOf, With<Plant>>();
        plants
            .iter(world)
            .filter(|parent| parent.parent() == tile)
            .count()
    }

    #[test]
    fn a_grown_tile_spawns_at_most_plants_per_frame_until_done() {
        let (mut app, tile) = grown_tile(2 * PLANTS_PER_FRAME + 1);
        app.update();
        assert_eq!(plants_on(&mut app, tile), PLANTS_PER_FRAME);
        app.update();
        app.update();
        assert_eq!(plants_on(&mut app, tile), 2 * PLANTS_PER_FRAME + 1);
        assert!(!app.world().entity(tile).contains::<GrownPlants>());
    }

    #[test]
    fn nearest_tiles_spawn_their_plants_first() {
        let (mut app, far) = grown_tile(PLANTS_PER_FRAME + 50);
        app.world_mut()
            .entity_mut(far)
            .insert(grown(IVec2::new(3, 0), PLANTS_PER_FRAME + 50));
        let near = app
            .world_mut()
            .spawn(grown(IVec2::ZERO, PLANTS_PER_FRAME + 50))
            .id();
        app.world_mut()
            .spawn((StreamAnchor, Transform::from_xyz(64.0, 0.0, 64.0)));
        app.update();
        assert_eq!(plants_on(&mut app, near), PLANTS_PER_FRAME);
        assert_eq!(plants_on(&mut app, far), 0);
        app.update();
        assert_eq!(plants_on(&mut app, near), PLANTS_PER_FRAME + 50);
        assert_eq!(
            plants_on(&mut app, far),
            PLANTS_PER_FRAME - 50,
            "the budget carries over"
        );
    }

    #[test]
    fn growing_plants_are_noted_until_all_have_spawned() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<PlantsGrowing>()
            .add_systems(Update, note_growing_plants);
        let growing = |app: &App| app.world().resource::<PlantsGrowing>().0;
        let tile = app.world_mut().spawn(grown(IVec2::ZERO, 1)).id();
        app.update();
        assert!(growing(&app), "a grown tile still has plants to spawn");

        app.world_mut().entity_mut(tile).remove::<GrownPlants>();
        app.update();
        assert!(!growing(&app));

        let task = AsyncComputeTaskPool::get().spawn(async { TilePlants(Vec::new()) });
        app.world_mut().spawn(PendingPlants {
            tile: IVec2::ZERO,
            task,
        });
        app.update();
        assert!(growing(&app), "a pending tile is still growing");
    }

    #[test]
    fn plants_trampled_while_waiting_never_spawn() {
        let (mut app, tile) = grown_tile(2);
        assert!(
            app.world_mut()
                .resource_mut::<Trampled>()
                .trample(Vec3::ZERO)
        );
        app.update();
        app.update();
        assert_eq!(plants_on(&mut app, tile), 1);
    }

    #[test]
    fn a_tile_despawned_mid_spawn_drops_its_plants() {
        let (mut app, tile) = grown_tile(2 * PLANTS_PER_FRAME);
        app.update();
        app.world_mut().despawn(tile);
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<(), With<Plant>>()
                .iter(world)
                .count(),
            0
        );
    }

    #[test]
    fn plants_carry_their_models_bounds() {
        let (mut app, tile) = grown_tile(0);
        let bounds = Aabb::from_min_max(Vec3::ZERO, Vec3::ONE);
        let world = app.world_mut();
        let mut looks = world.resource_mut::<PlantLooks>();
        let grass = &mut looks.looks[Kind::DryGrass as usize][0];
        grass.near_bounds = Some(bounds);
        grass.far_bounds = Some(bounds);
        let site = |kind, x| PlantSite {
            kind,
            variant: 0,
            pose: Transform::from_xyz(x, 0.0, 0.0),
        };
        let sites = vec![site(Kind::DryGrass, 0.0), site(Kind::Shrub, 1.0)];
        world.entity_mut(tile).insert(GrownPlants {
            tile: IVec2::ZERO,
            sites,
        });
        app.update();

        let world = app.world_mut();
        let mut plants = world.query_filtered::<(Entity, &Transform, &Children), With<Plant>>();
        let plants: Vec<_> = plants
            .iter(world)
            .map(|(near, pose, children)| (near, children[0], pose.translation.x == 0.0))
            .collect();
        assert_eq!(plants.len(), 2);
        for (near, far, grass) in plants {
            for entity in [near, far] {
                assert_eq!(world.get::<Aabb>(entity).copied(), grass.then_some(bounds));
                assert_eq!(world.entity(entity).contains::<NoAutoAabb>(), grass);
            }
        }
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
                grow_belt_plants(&terrain, base_tile() + IVec2::new(x, z), &mut seeds);
            }
        }
        // The hangar's belt only, east of the Perimeter's.
        let sites: Vec<_> = expand_patches(&terrain, &seeds, true)
            .filter(|site| site.pose.translation.x > PERIMETER_X + PERIMETER_BELT.1)
            .collect();
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
    fn a_belt_lines_the_perimeter_and_a_thicket_hides_the_crawl_hole() {
        let terrain = Terrain::new();
        let tile = (CRAWL_HOLE / TILE_SIZE).floor().as_ivec2();
        let mut seeds = Vec::new();
        for z in -1..=1 {
            for x in -1..=0 {
                grow_belt_plants(&terrain, tile + IVec2::new(x, z), &mut seeds);
            }
        }
        let sites: Vec<_> = expand_patches(&terrain, &seeds, true).collect();
        let zone_side = sites
            .iter()
            .filter(|site| (-15.0..-3.0).contains(&(site.pose.translation.x - PERIMETER_X)))
            .count();
        assert!(zone_side > 400, "{zone_side} plants on the Zone's side");
        let around_hole = sites
            .iter()
            .filter(|site| site.pose.translation.xz().distance(CRAWL_HOLE) < 7.0)
            .count();
        assert!(around_hole >= 12, "{around_hole} plants hide the hole");
        for site in &sites {
            let at = site.pose.translation.xz();
            assert!(
                !near_wall_or_checkpoint(at, 0.7),
                "plant in the wall at {at}"
            );
            assert!(
                !super::super::cordon::reserve_patrol_track(at),
                "plant on the patrol's track at {at}"
            );
        }
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

    /// The scan of the first shrub model.
    fn shrub_scan() -> &'static str {
        Kind::Shrub.models()[0]
            .full
            .expect("a shrub has a full scan")
    }

    /// An app streaming full scans for a shrub `distance` metres from the
    /// stalker, on a GPU holding `scans`.
    fn streaming_plant(distance: f32, scans: GpuScans) -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(scans)
            .add_systems(Update, stream_full_plants);
        app.world_mut()
            .spawn((crate::player::Player::default(), Transform::IDENTITY));
        let tile = app.world_mut().spawn(Transform::IDENTITY).id();
        let plant = app
            .world_mut()
            .spawn((
                Plant {
                    model: &Kind::Shrub.models()[0],
                },
                Transform::from_xyz(distance, 0.0, 0.0),
                ChildOf(tile),
            ))
            .id();
        (app, plant)
    }

    /// The books of a GPU with the shrub's scan resident.
    fn shrub_resident() -> GpuScans {
        let mut scans = GpuScans::new(1 << 30);
        scans.insert_resident(shrub_scan(), Handle::default());
        scans
    }

    fn full_plants(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        let mut fulls = world.query_filtered::<Entity, With<FullPlant>>();
        fulls.iter(world).collect()
    }

    #[test]
    fn plants_within_40_m_ask_for_their_scans() {
        let (mut app, _) = streaming_plant(30.0, GpuScans::new(1 << 30));
        app.update();
        assert!(full_plants(&mut app).is_empty(), "too far to show");
        let plan = app.world_mut().resource_mut::<GpuScans>().plan(0.0);
        assert_eq!(plan.load, [shrub_scan()]);
    }

    #[test]
    fn plants_past_40_m_do_not_ask_for_their_scans() {
        let (mut app, _) = streaming_plant(45.0, GpuScans::new(1 << 30));
        app.update();
        let plan = app.world_mut().resource_mut::<GpuScans>().plan(0.0);
        assert!(plan.load.is_empty(), "{:?}", plan.load);
    }

    #[test]
    fn the_nearest_plant_shows_its_scan_only_once_on_the_gpu() {
        let (mut app, plant) = streaming_plant(5.0, GpuScans::new(1 << 30));
        app.update();
        assert!(
            full_plants(&mut app).is_empty(),
            "the scan is not on the GPU"
        );

        let mut scans = app.world_mut().resource_mut::<GpuScans>();
        scans.insert_resident(shrub_scan(), Handle::default());
        app.update();
        let fulls = full_plants(&mut app);
        assert_eq!(fulls.len(), 1);
        let full = app.world().entity(fulls[0]).get::<FullPlant>();
        assert_eq!(full.map(|full| full.plant), Some(plant));
    }

    #[test]
    fn a_shown_scan_is_wanted_every_frame() {
        let (mut app, _) = streaming_plant(5.0, shrub_resident());
        app.update();
        assert_eq!(full_plants(&mut app).len(), 1);
        for now in [10.0, 20.0, 30.0] {
            app.update();
            let mut scans = app.world_mut().resource_mut::<GpuScans>();
            assert!(scans.plan(now).load.is_empty(), "it is loaded already");
            assert_eq!(scans.last_shown(shrub_scan()), Some(now));
        }
    }

    #[test]
    fn a_scan_that_leaves_the_gpu_takes_its_instance_and_gives_the_plant_back() {
        let (mut app, plant) = streaming_plant(5.0, shrub_resident());
        app.update();
        assert_eq!(full_plants(&mut app).len(), 1);
        // The scan has loaded and stands in for the plant.
        app.world_mut().entity_mut(plant).insert(Visibility::Hidden);

        // Eviction: nothing is on the GPU any more.
        app.insert_resource(GpuScans::new(1 << 30));
        app.update();

        assert!(full_plants(&mut app).is_empty(), "frees its VRAM");
        let shown = app.world().entity(plant).get::<Visibility>();
        assert_eq!(shown, Some(&Visibility::Inherited));
    }
}
