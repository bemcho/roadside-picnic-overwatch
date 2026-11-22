//! Grounded, biome-specific plants streamed with the surrounding scenery. The
//! wind bends the batched ground cover in its vertex shader and sways the
//! imported bushes from their roots.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;

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
const PATCH_SIZE: usize = 10;
const DENSE_PATCH_MULTIPLIER: usize = 10;
const BATCHES_PER_SIDE: usize = 4;
const BATCH_SIZE: f32 = TILE_SIZE / BATCHES_PER_SIDE as f32;
const FULL_BUSH_RANGE: f32 = 8.0;

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
}

pub(super) type PlantMaterial = ExtendedMaterial<StandardMaterial, PlantWind>;

/// The wind bending the batched ground cover.
#[derive(Asset, AsBindGroup, Reflect, Clone, Debug, Default)]
pub(super) struct PlantWind {
    /// xy: downwind bend in meters at a 1 m tip, z: seconds of wind, w: gust.
    #[uniform(100)]
    wind: Vec4,
}

impl MaterialExtension for PlantWind {
    fn vertex_shader() -> ShaderRef {
        assets::PLANT_WIND_SHADER.into()
    }

    // Ground cover casts no shadows and needs no depth prepass.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Resource, Default)]
pub(super) struct PlantMaterials {
    leaves: Handle<PlantMaterial>,
    glow: Handle<PlantMaterial>,
}

pub(super) fn prepare_plant_materials(
    mut commands: Commands,
    mut materials: ResMut<Assets<PlantMaterial>>,
) {
    let leaves = StandardMaterial {
        perceptual_roughness: 0.96,
        cull_mode: None,
        double_sided: true,
        ..default()
    };
    let plant = |base| PlantMaterial {
        base,
        extension: PlantWind::default(),
    };
    commands.insert_resource(PlantMaterials {
        leaves: materials.add(plant(leaves.clone())),
        glow: materials.add(plant(StandardMaterial {
            emissive: LinearRgba::rgb(20.0, 265.0, 177.0),
            ..leaves
        })),
    });
}

/// Hands the wind to the ground cover's shader.
pub(super) fn bend_plants(
    wind: Res<Wind>,
    plants: Res<PlantMaterials>,
    mut materials: ResMut<Assets<PlantMaterial>>,
) {
    let bend = wind.direction() * (0.05 + 0.18 * wind.strength());
    for handle in [&plants.leaves, &plants.glow] {
        if let Some(mut material) = materials.get_mut(handle) {
            material.extension.wind = Vec4::new(bend.x, bend.y, wind.seconds, wind.gust);
        }
    }
}

/// Sways each imported bush from its root: a lean downwind and a flutter.
pub(super) fn sway_bushes(wind: Res<Wind>, mut bushes: Query<(&BushDisplay, &mut Transform)>) {
    let downwind = wind.direction();
    let axis = Vec3::new(downwind.y, 0.0, -downwind.x);
    let strength = wind.strength();
    for (bush, mut pose) in &mut bushes {
        let flutter = (wind.seconds * 1.7 + bush.phase).sin();
        let angle = (0.012 + 0.05 * strength) * (0.75 + 0.25 * flutter);
        pose.rotation = Quat::from_axis_angle(axis, angle) * bush.rest;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct PlantSite {
    kind: Kind,
    pose: Transform,
    seed: bool,
}

#[derive(Component)]
pub(in crate::world) struct BushDisplay {
    kind: Kind,
    /// The upright pose's rotation, which the wind sways around.
    rest: Quat,
    phase: f32,
}

#[derive(Component)]
pub(in crate::world) struct BushPreview {
    display: Entity,
}

#[derive(Component)]
pub(in crate::world) struct FullBush {
    display: Entity,
}

impl PlantSite {
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
        Some(Self {
            kind,
            pose: Transform::from_xyz(at.x, base - 0.06, at.y)
                .with_rotation(Quat::from_rotation_y(angle))
                .with_scale(Vec3::splat(scale)),
            seed: true,
        })
    }
}

fn plant_sites(terrain: &Terrain, tile: IVec2) -> Vec<PlantSite> {
    let mut wild = Vec::new();
    for layer in 0..5 {
        grow_plant_layer(terrain, tile, layer, &mut wild);
    }
    let mut belt = Vec::new();
    grow_hangar_plants(terrain, tile, &mut belt);
    expand_patches(terrain, &wild, false, DENSE_PATCH_MULTIPLIER)
        .chain(expand_patches(terrain, &belt, true, DENSE_PATCH_MULTIPLIER))
        .collect()
}

fn expand_patches<'a>(
    terrain: &'a Terrain,
    seeds: &'a [PlantSite],
    in_belt: bool,
    density_multiplier: usize,
) -> impl Iterator<Item = PlantSite> + 'a {
    seeds.iter().flat_map(move |&seed| {
        let root = seed.pose.translation.xz();
        let id = (root * 100.0).as_ivec2();
        let multiplier = if seed.kind == Kind::DryGrass {
            1
        } else {
            density_multiplier.max(1)
        };
        let patch_size = PATCH_SIZE * multiplier;
        let radius = if in_belt { 2.45 } else { 4.2 } * (multiplier as f32).sqrt();
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
                    .then(|| PlantSite {
                        kind: seed.kind,
                        pose: Transform::from_xyz(at.x, ground - 0.055, at.y)
                            .with_rotation(Quat::from_rotation_y(roll(2) * TAU))
                            .with_scale(seed.pose.scale * lerp(0.55, 1.0, roll(3))),
                        seed: false,
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
            let depth = wet.depth(y);
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
                terrain.mesh_height_at(at),
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

fn spawn_plants(
    parent: &mut ChildSpawnerCommands,
    ground_cover: Vec<GroundCover>,
    materials: &PlantMaterials,
    meshes: &mut Assets<Mesh>,
) {
    for cover in ground_cover {
        let (paint, distance) = if cover.glow {
            (&materials.glow, 99.0)
        } else {
            (&materials.leaves, 69.0)
        };
        parent.spawn((
            Mesh3d(meshes.add(cover.mesh)),
            MeshMaterial3d(paint.clone()),
            Transform::from_xyz(cover.center.x, 0.0, cover.center.y),
            VisibilityRange {
                use_aabb: false,
                ..detail_range(distance, 15.0)
            },
            NotShadowCaster,
        ));
    }
}

/// One ground-cover mesh batch, centered on its cell of the tile.
pub(super) struct GroundCover {
    center: Vec2,
    mesh: Mesh,
    glow: bool,
}

/// A tile's plants, grown on a worker thread so streaming never stalls a frame.
pub(super) struct TilePlants {
    ground_cover: Vec<GroundCover>,
    bushes: Vec<PlantSite>,
}

impl TilePlants {
    pub(super) fn grow(terrain: &Terrain, tile: IVec2) -> Self {
        let sites = plant_sites(terrain, tile);
        let mut ground_cover = Vec::new();
        for (i, (leaves, glow)) in plant_batches(&sites, tile).into_iter().enumerate() {
            let center = batch_center(tile, i);
            for (batch, glow) in [(leaves, false), (glow, true)] {
                if let Some(mesh) = batch.mesh() {
                    ground_cover.push(GroundCover { center, mesh, glow });
                }
            }
        }
        let bushes = sites
            .into_iter()
            .filter(|site| site.seed && matches!(site.kind, Kind::Shrub | Kind::Thornbush))
            .collect();
        Self {
            ground_cover,
            bushes,
        }
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

/// Spawns at most one grown tile per frame, spreading the mesh uploads.
pub(super) fn spawn_grown_plants(
    mut commands: Commands,
    mut tiles: Query<(Entity, &mut PendingPlants)>,
    server: Res<AssetServer>,
    materials: Res<PlantMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
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
        .with_children(|parent| {
            spawn_plants(parent, plants.ground_cover, &materials, &mut meshes);
            spawn_bushes(parent, &server, plants.bushes);
        });
}

fn spawn_bushes(parent: &mut ChildSpawnerCommands, server: &AssetServer, bushes: Vec<PlantSite>) {
    for site in bushes {
        let at = site.pose.translation;
        let mut root = parent.spawn((
            BushDisplay {
                kind: site.kind,
                rest: site.pose.rotation,
                phase: (at.x * 0.37 + at.z * 0.23) % TAU,
            },
            site.pose,
            Visibility::default(),
        ));
        let display = root.id();
        root.with_children(|bush| {
            bush.spawn((
                BushPreview { display },
                WorldAssetRoot(
                    server.load(
                        GltfAssetLabel::Scene(0).from_asset(imported_bush_medium_path(&site)),
                    ),
                ),
                Transform::IDENTITY,
                detail_range(66.0, 10.0),
            ))
            .observe(range_scene_meshes);
        });
    }
}

pub(super) fn stream_full_bushes(
    mut commands: Commands,
    player: Query<&Transform, With<crate::player::Player>>,
    bushes: Query<(Entity, &BushDisplay, &GlobalTransform)>,
    fulls: Query<(Entity, &FullBush)>,
    previews: Query<(Entity, &BushPreview)>,
    server: Res<AssetServer>,
) {
    let selected = player.single().ok().and_then(|player| {
        bushes
            .iter()
            .filter_map(|(display, bush, pose)| {
                let distance = player.translation.xz().distance(pose.translation().xz());
                (distance <= FULL_BUSH_RANGE).then_some((distance, display, bush.kind))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    });
    let active = selected.map(|(_, display, _)| display);
    for (entity, full) in &fulls {
        if active != Some(full.display) {
            commands.entity(entity).despawn();
            for (preview, marker) in &previews {
                if marker.display == full.display {
                    commands.entity(preview).insert(Visibility::Inherited);
                }
            }
        }
    }

    let Some((_, display, kind)) = selected else {
        return;
    };
    if fulls.iter().any(|(_, full)| full.display == display) {
        return;
    }
    commands
        .spawn((
            FullBush { display },
            ChildOf(display),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(uhd_bush_path(kind)))),
            Transform::IDENTITY,
            detail_range(12.0, 2.0),
            Visibility::Hidden,
        ))
        .observe(range_scene_meshes)
        .observe(reveal_full_bush);
}

fn reveal_full_bush(
    ready: On<WorldInstanceReady>,
    fulls: Query<&FullBush>,
    previews: Query<(Entity, &BushPreview)>,
    mut commands: Commands,
) {
    let Ok(full) = fulls.get(ready.entity) else {
        return;
    };
    commands.entity(ready.entity).insert(Visibility::Inherited);
    for (preview, marker) in &previews {
        if marker.display == full.display {
            commands.entity(preview).insert(Visibility::Hidden);
        }
    }
}

fn uhd_bush_path(kind: Kind) -> &'static str {
    match kind {
        Kind::Shrub => assets::BUSH_UHD_SHRUB,
        Kind::Thornbush => assets::BUSH_UHD_THORNBUSH,
        _ => unreachable!("only bush kinds have full-quality meshes"),
    }
}

fn imported_bush_medium_path(site: &PlantSite) -> &'static str {
    let seed = (site.pose.translation.xz() * 191.0).as_ivec2();
    match site.kind {
        Kind::Shrub => {
            assets::BUSH_SHRUB_VARIANTS[(lattice_hash(seed.x, seed.y, SEED + 913)
                * assets::BUSH_SHRUB_VARIANTS.len() as f32)
                as usize]
        }
        Kind::Thornbush => assets::BUSH_MEDIUM_THORNBUSH,
        _ => unreachable!("only shrub and thornbush seeds use imported bushes"),
    }
}

fn batch_center(tile: IVec2, index: usize) -> Vec2 {
    let cell = Vec2::new(
        (index % BATCHES_PER_SIDE) as f32,
        (index / BATCHES_PER_SIDE) as f32,
    );
    tile.as_vec2() * TILE_SIZE + (cell + 0.5) * BATCH_SIZE
}

fn plant_batches(
    sites: &[PlantSite],
    tile: IVec2,
) -> [(PlantBatch, PlantBatch); BATCHES_PER_SIDE * BATCHES_PER_SIDE] {
    let origin = tile.as_vec2() * TILE_SIZE;
    let mut batches: [(PlantBatch, PlantBatch); BATCHES_PER_SIDE * BATCHES_PER_SIDE] =
        Default::default();
    for mut site in sites.iter().copied() {
        if site.seed && matches!(site.kind, Kind::Shrub | Kind::Thornbush) {
            continue;
        }
        let at = site.pose.translation.xz();
        let cell = ((at - origin) / BATCH_SIZE)
            .floor()
            .clamp(Vec2::ZERO, Vec2::splat((BATCHES_PER_SIDE - 1) as f32))
            .as_uvec2();
        let index = cell.y as usize * BATCHES_PER_SIDE + cell.x as usize;
        let (leaves, glow) = &mut batches[index];
        let center = batch_center(tile, index);
        // Bevy's fade shader measures distance to the mesh origin.
        site.pose.translation -= Vec3::new(center.x, 0.0, center.y);
        grow_shape(site, at, leaves, glow);
    }
    batches
}

#[derive(Default)]
struct PlantBatch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl PlantBatch {
    fn surface<const V: usize>(
        &mut self,
        pose: Transform,
        points: [Vec3; V],
        triangles: &[u32],
        color: Vec3,
    ) {
        let offset = self.positions.len() as u32;
        let n = (points[triangles[1] as usize] - points[triangles[0] as usize])
            .cross(points[triangles[2] as usize] - points[triangles[0] as usize])
            .normalize_or_zero();
        self.positions
            .extend(points.map(|v| pose.transform_point(v).to_array()));
        self.normals.extend([(pose.rotation * n).to_array(); V]);
        // Alpha carries the height above the root, which the wind shader bends.
        self.colors.extend(points.map(|v| {
            (color * lerp(0.64, 1.13, (v.y * 1.7).clamp(0.0, 1.0)))
                .extend((v.y * pose.scale.y).max(0.0))
                .to_array()
        }));
        self.indices.extend(triangles.iter().map(|&i| i + offset));
    }

    fn mesh(self) -> Option<Mesh> {
        (!self.indices.is_empty()).then(|| {
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
            .with_inserted_indices(Indices::U32(self.indices))
        })
    }
}

fn grow_shape(site: PlantSite, world: Vec2, leaves: &mut PlantBatch, glow: &mut PlantBatch) {
    let seed = (world * 181.0).as_ivec2();
    let roll = |i| lattice_hash(seed.x, seed.y, SEED + 619 + i);
    let pose = site.pose;
    let shade = lerp(0.71, 1.20, roll(0));
    let direction = |i| {
        let a = Vec2::from_angle(i as f32 * 2.399_963 + roll(i + 31) * 0.5);
        Vec3::new(a.x, 0.0, a.y)
    };
    match site.kind {
        Kind::DryGrass | Kind::Glowweed => {
            let luminous = site.kind == Kind::Glowweed;
            let tint = if luminous {
                Vec3::new(0.17, 0.33, 0.24)
            } else {
                Vec3::new(0.47, 0.37, 0.16)
            };
            for i in 0..6 {
                let dir = direction(i);
                let height = lerp(0.26, 0.87, roll(i + 3));
                let root = dir * (0.04 + 0.085 * roll(i + 11));
                let mid = root + dir * 0.07 + Vec3::Y * height * 0.62;
                let tip = root + dir * lerp(0.13, 0.38, roll(i + 20)) + Vec3::Y * height;
                let side = Vec3::Y.cross(dir) * if luminous { 0.050 } else { 0.030 };
                blade(leaves, pose, root, mid, tip, side, tint * shade);
                if luminous && i % 2 == 0 {
                    let bud = tip + Vec3::Y * 0.035;
                    glow.surface(
                        pose,
                        [tip - side * 0.6, tip + dir * 0.027, tip + side * 0.6, bud],
                        &[0, 1, 3, 1, 2, 3],
                        Vec3::new(0.08, 0.38, 0.26),
                    );
                }
            }
        }
        Kind::Shrub | Kind::Thornbush => {
            let thorny = site.kind == Kind::Thornbush;
            let tint = if thorny {
                Vec3::new(0.32, 0.31, 0.095)
            } else {
                Vec3::new(0.14, 0.34, 0.095)
            };
            for i in 0..11 {
                let dir = direction(i);
                let tip =
                    dir * lerp(0.25, 0.55, roll(i + 3)) + Vec3::Y * lerp(0.25, 0.72, roll(i + 50));
                let mid = tip * 0.61 + Vec3::Y * 0.12;
                let side = Vec3::Y.cross(dir) * if thorny { 0.12 } else { 0.19 };
                leaves.surface(
                    pose,
                    [
                        mid * 0.31,
                        mid + side,
                        tip,
                        mid - side,
                        mid + Vec3::Y * 0.065,
                    ],
                    &[0, 4, 1, 1, 4, 2, 2, 4, 3, 3, 4, 0],
                    tint * shade * lerp(0.63, 1.16, roll(i + 8)),
                );
            }
        }
        Kind::Hogweed => {
            let green = Vec3::new(0.25, 0.38, 0.095) * shade;
            for i in 0..3 {
                let dir = direction(i);
                let side = Vec3::Y.cross(dir) * 0.022;
                let tip = dir * 0.17 + Vec3::Y * lerp(0.48, 0.94, roll(i + 7));
                blade(leaves, pose, Vec3::ZERO, tip * 0.57, tip, side, green);
                blade(
                    leaves,
                    pose,
                    tip * 0.23,
                    tip * 0.46 + dir * 0.15,
                    tip * 0.53 + dir * 0.35,
                    side * 3.4,
                    green,
                );
                let r = lerp(0.12, 0.20, roll(i + 11));
                leaves.surface(
                    pose,
                    [
                        tip - dir * r,
                        tip + side * 6.0,
                        tip + dir * r,
                        tip - side * 6.0,
                        tip + Vec3::Y * 0.04,
                    ],
                    &[0, 4, 1, 1, 4, 2, 2, 4, 3, 3, 4, 0],
                    Vec3::new(0.64, 0.69, 0.43) * shade,
                );
            }
        }
    }
}

fn blade(
    batch: &mut PlantBatch,
    pose: Transform,
    root: Vec3,
    middle: Vec3,
    tip: Vec3,
    side: Vec3,
    color: Vec3,
) {
    batch.surface(
        pose,
        [
            root - side,
            root + side,
            middle - side * 0.65,
            middle + side * 0.65,
            tip,
        ],
        &[0, 2, 1, 1, 2, 3, 2, 4, 3],
        color,
    );
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::world_serialization::WorldAsset;

    use super::*;
    use crate::world::Collider;

    #[test]
    fn streamed_tiles_grow_plants_in_the_background_then_spawn_them() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<WorldAsset>()
            .init_asset::<Mesh>()
            .init_resource::<PlantMaterials>()
            .add_systems(Update, spawn_grown_plants);
        let tile = IVec2::new(0, -2);
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
        let ground_cover = children
            .iter()
            .filter(|&child| world.entity(child).contains::<Mesh3d>())
            .count();
        let bushes = children
            .iter()
            .filter(|&child| world.entity(child).contains::<BushDisplay>())
            .count();
        assert_eq!(ground_cover, expected.ground_cover.len());
        assert_eq!(bushes, expected.bushes.len());
        assert!(ground_cover > 0 && bushes > 0);
    }

    #[test]
    fn bushes_and_weeds_are_tenfold_and_respect_habitats_and_clearances() {
        let terrain = Terrain::new();
        let wet_tile = IVec2::new(2, 1);
        assert_eq!(
            plant_sites(&terrain, wet_tile),
            plant_sites(&terrain, wet_tile)
        );
        let mut counts = [0; Kind::ALL.len()];
        let mut baseline_counts = [0; Kind::ALL.len()];
        for tile in [
            wet_tile,
            IVec2::new(2, 2),
            IVec2::new(0, -1),
            IVec2::new(3, -1),
            IVec2::new(-1, 0),
            IVec2::new(-11, -9),
        ] {
            let mut wild = Vec::new();
            for layer in 0..5 {
                grow_plant_layer(&terrain, tile, layer, &mut wild);
            }
            for site in expand_patches(&terrain, &wild, false, 1) {
                baseline_counts[site.kind as usize] += 1;
            }
            for site in expand_patches(&terrain, &wild, false, DENSE_PATCH_MULTIPLIER) {
                let p = site.pose.translation;
                counts[site.kind as usize] += 1;
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
            let before = baseline_counts[kind as usize];
            let after = counts[kind as usize];
            let ratio = after as f32 / before as f32;
            if kind == Kind::DryGrass {
                assert_eq!(after, before, "{kind:?} should retain its patch density");
            } else {
                assert!(
                    (8.5..=10.0).contains(&ratio),
                    "{kind:?}: {before} -> {after} ({ratio:.2}x)"
                );
            }
        }
    }

    #[test]
    fn denser_shelterbelt_keeps_a_clear_yard_and_favors_the_rear() {
        let terrain = Terrain::new();
        let mut count = [0; Kind::ALL.len()];
        let mut rear = 0;
        let mut sites = Vec::new();
        for z in -3..=0 {
            for x in -2..=2 {
                grow_hangar_plants(&terrain, IVec2::new(x, z), &mut sites);
            }
        }
        let baseline: Vec<_> = expand_patches(&terrain, &sites, true, 1).collect();
        let sites: Vec<_> =
            expand_patches(&terrain, &sites, true, DENSE_PATCH_MULTIPLIER).collect();
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
        let baseline_count = baseline.len();
        assert!(
            (baseline_count * 8..=baseline_count * 10).contains(&sites.len()),
            "{baseline_count} -> {}: {count:?}",
            sites.len()
        );
        assert!(rear > sites.len() / 2);
        assert!(count[Kind::Shrub as usize] + count[Kind::Thornbush as usize] > 2_000);
        assert!(count.into_iter().all(|n| n >= 45), "{count:?}");
    }

    #[test]
    fn dense_ground_cover_is_batched_and_visual_only() {
        let terrain = Terrain::new();
        let tile = IVec2::new(0, -2);
        let sites = plant_sites(&terrain, tile);
        assert!(sites.len() > 900);
        let mut world = World::new();
        let root = world.spawn(Transform::IDENTITY).id();
        let mut meshes = Assets::default();
        let plants = TilePlants::grow(&terrain, tile);
        world.commands().entity(root).with_children(|parent| {
            spawn_plants(
                parent,
                plants.ground_cover,
                &PlantMaterials::default(),
                &mut meshes,
            );
        });
        world.flush();
        let mut triangles = 0;
        let mut batches = 0;
        for (mesh, range, collides, no_shadow) in world
            .query::<(
                &Mesh3d,
                &VisibilityRange,
                Has<Collider>,
                Has<NotShadowCaster>,
            )>()
            .iter(&world)
        {
            assert!(!collides && no_shadow);
            let mesh = meshes.get(&mesh.0).unwrap();
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("batch positions");
            };
            assert!(!range.use_aabb);
            assert!(
                positions
                    .iter()
                    .all(|&[x, _, z]| { x.abs().max(z.abs()) < BATCH_SIZE * 0.5 + 14.0 })
            );
            triangles += mesh.indices().unwrap().len() / 3;
            batches += 1;
        }
        assert!((2..=2 * BATCHES_PER_SIDE.pow(2)).contains(&batches));
        assert!((sites.len() * 18..=sites.len() * 44).contains(&triangles));
    }
}
