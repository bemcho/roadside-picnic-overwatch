//! Concrete cordons: around the hangar yard, along the Perimeter that
//! separates Harmont from the Zone, and at the edge of the map.
//!
//! The Perimeter stands three times as high as the others. Booms close the
//! roads through two gates: the hangar yard's, beside its checkpoint, lifts for
//! the stalker and their car, and the Institute gate's only for someone on
//! Institute business (`InstitutePass`). The stalkers' old breach is walled up,
//! and the way into the Zone on foot is a crawl hole dug under the wall south
//! of it, hidden in a thicket (`CRAWL_HOLE`).

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfMaterial;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::assets::{self, PreloadedAssets};
use crate::player::Player;

use super::collision::{Collider, Pillar, Unclimbable};
use super::hydrology::{WATER_LEVEL, Wetlands};
use super::places::{Describe, Place};
use super::terrain::{BASE, HALF_SIZE, SPACING, TILE_SIZE, Terrain, lerp, smoothstep};
use super::{StreamAnchor, WorldRoot, anchor_tiles, detail_range, near_anchor, neighborhoods};

const YARD_MIN: Vec2 = Vec2::new(BASE.0 - 64.0, BASE.1 - 64.0);
const YARD_MAX: Vec2 = Vec2::new(BASE.0 + 64.0, BASE.1 + 60.0);
const GATE_CENTER: f32 = BASE.0 - 1.0;
/// The yard's gate in its north wall: a lane `GATE_HALF` either side of `GATE`,
/// as wide as the Institute's so the same boom closes it.
pub(super) const GATE: Vec2 = Vec2::new(GATE_CENTER, YARD_MAX.y);
pub(super) const GATE_HALF: f32 = PERIMETER_GATE_HALF;
/// The hangar checkpoint stands in the gap east of the lane, turned about so
/// its barrier lines the wall and points at the lane, ending beside the boom.
pub(super) const CHECKPOINT: Vec2 = Vec2::new(
    GATE_CENTER + GATE_HALF + BARRIER_END * CHECKPOINT_SCALE + 0.5,
    YARD_MAX.y + BARRIER_Z * CHECKPOINT_SCALE,
);
const CHECKPOINT_YAW: f32 = PI;
/// The whole gap in the yard's wall, lane and checkpoint: its middle and half
/// its width.
pub(super) const YARD_GAP: (Vec2, f32) = {
    let (west, east) = (WEST_JAMB.x, CHECKPOINT.x + CHECKPOINT_HALF.x);
    (
        Vec2::new((west + east) * 0.5, YARD_MAX.y),
        (east - west) * 0.5,
    )
};
const CHECKPOINT_SCALE: f32 = 11.0;
const CHECKPOINT_HALF: Vec2 = Vec2::new(0.489 * CHECKPOINT_SCALE, 0.348 * CHECKPOINT_SCALE);
/// The checkpoint scan's barrier arm runs along its +X at z = `BARRIER_Z`,
/// out to `BARRIER_END` (model units).
const BARRIER_Z: f32 = 0.06;
const BARRIER_END: f32 = 0.48;
/// The UHD checkpoint's ground level, in model units.
const CHECKPOINT_DATUM: f32 = -0.41;
/// The Perimeter: the UN wall between Harmont, on the hangar's side (+X), and
/// the Zone (-X). Seen from above with +Z up, the Zone lies to the right, as on
/// the Harmont map.
pub(crate) const PERIMETER_X: f32 = 700.0;
/// The official Institute gate, where the highway leaves Harmont (map px 413, 350):
/// an opening as wide as the road, closed by a boom.
pub(super) const PERIMETER_GATE: Vec2 = Vec2::new(PERIMETER_X, 79.0);
pub(super) const PERIMETER_GATE_HALF: f32 = 6.0;
/// The checkpoint guarding the Perimeter gate, on the Harmont side.
pub(super) const PERIMETER_CHECKPOINT: Vec2 = Vec2::new(PERIMETER_X + 14.0, 61.0);
/// The stalkers' old breach in the Perimeter, south of the gate (map px 410,
/// 510), now walled up; the Chapter 4 track still leads to it.
pub(super) const BREACH: Vec2 = Vec2::new(PERIMETER_X, -305.0);
/// A crawl hole dug under the Perimeter south of the old breach: `HOLE_DEPTH`
/// deep on its floor, which reaches `HOLE_FLOOR` across and along the wall,
/// easing up to the ground at `HOLE_RIM`.
pub(crate) const CRAWL_HOLE: Vec2 = Vec2::new(PERIMETER_X, -335.0);
const HOLE_DEPTH: f32 = 1.0;
const HOLE_FLOOR: Vec2 = Vec2::new(2.0, 1.5);
const HOLE_RIM: Vec2 = Vec2::new(4.5, 4.0);
/// Where the hole is this deep, a crawling stalker fits under the wall.
const HOLE_CRAWL_DEPTH: f32 = 0.6;
/// The stalker notices the hole, and the pointer names it, only this close.
const HOLE_NOTICE: f32 = 8.0;
/// The UN patrol's loop beside the Perimeter on Harmont's side: every point
/// `PATROL_RADIUS` from the segment between the two turn centers, so it runs
/// north along the wall from 50 m short of the hangar to 50 m past the
/// Institute, past the gate's checkpoint, and back on the far lane.
pub(crate) const PATROL_TURNS: [Vec2; 2] = [
    Vec2::new(PERIMETER_X + 19.0, BASE.1 - 50.0),
    Vec2::new(PERIMETER_X + 19.0, 560.0),
];
pub(crate) const PATROL_RADIUS: f32 = 12.0;
/// Half the width the patrol's track keeps clear of scenery.
const PATROL_HALF_WIDTH: f32 = 3.5;
const EDGE: f32 = HALF_SIZE - 4.0;
const WALL_HEIGHT_SCALE: f32 = 7.4;
/// The Perimeter stands this many times as high as the other cordons.
const PERIMETER_RISE: f32 = 3.0;
const PANEL_SPAN: f32 = 0.86;
const SINK: f32 = 0.09;
#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
const CHECKPOINT_PILLARS: &[Pillar] = include!("pillars/uhd_checkpoint.rs");
const WALL_PILLAR: &[Pillar] = &[Pillar {
    x: 0.0,
    z: 0.0,
    radius: 0.47,
    bottom: -0.65,
    top: 2.48,
}];
const PERIMETER_PILLAR: &[Pillar] = &[Pillar {
    top: 2.48 * PERIMETER_RISE,
    ..WALL_PILLAR[0]
}];
/// Over the crawl hole the wall's foot stands on the undug ground.
const HOLE_PILLAR: &[Pillar] = &[Pillar {
    bottom: -0.05,
    ..PERIMETER_PILLAR[0]
}];
/// A gate's boom spans its road, `BOOM_LENGTH` long, from a post at one side.
const BOOM_LENGTH: f32 = 2.0 * PERIMETER_GATE_HALF;
const BOOM_HINGE: f32 = 1.0;
/// It lifts while someone it opens for is this near the gate, a whole swing
/// taking 1 / `BOOM_RATE` seconds, and stops nobody once this far up.
const BOOM_REACH: f32 = 40.0;
const BOOM_RATE: f32 = 0.8;
const BOOM_OPEN: f32 = 0.7;
/// The lowered arm, from below the ground so nobody crawls under it.
const BOOM_PILLARS: &[Pillar] = &{
    let mut pillars = [Pillar {
        x: 0.0,
        z: 0.0,
        radius: 0.3,
        bottom: -0.5,
        top: BOOM_HINGE + 0.3,
    }; 24];
    let mut i = 0;
    while i < pillars.len() {
        pillars[i].z = (i as f32 + 0.5) * BOOM_LENGTH / 24.0;
        i += 1;
    }
    pillars
};

const SOUTHWEST: Vec2 = YARD_MIN;
const SOUTHEAST: Vec2 = Vec2::new(YARD_MAX.x, YARD_MIN.y);
const NORTHWEST: Vec2 = Vec2::new(YARD_MIN.x, YARD_MAX.y);
const NORTHEAST: Vec2 = YARD_MAX;
const WEST_JAMB: Vec2 = Vec2::new(GATE_CENTER - GATE_HALF, YARD_MAX.y);
/// The wall's east end, hidden in the checkpoint's sandbags.
const EAST_JAMB: Vec2 = Vec2::new(CHECKPOINT.x + 2.5, YARD_MAX.y);
const YARD_RUNS: [(Vec2, Vec2); 5] = [
    (SOUTHWEST, SOUTHEAST),
    (SOUTHEAST, NORTHEAST),
    (NORTHEAST, EAST_JAMB),
    (WEST_JAMB, NORTHWEST),
    (NORTHWEST, SOUTHWEST),
];

pub(super) fn inside_yard(at: Vec2) -> bool {
    at.cmpge(YARD_MIN).all() && at.cmple(YARD_MAX).all()
}

pub(super) fn yard_grade(at: Vec2) -> f32 {
    let min = Vec2::new(BASE.0 - 46.0, BASE.1 - 40.0);
    let max = Vec2::new(BASE.0 + 46.0, YARD_MAX.y - 2.0);
    let edge = (min - at).max(at - max).max_element();
    smoothstep(16.0, 0.0, edge)
}

/// Inside the Zone, beyond the Perimeter from Harmont and the hangar.
pub(crate) fn in_zone(at: Vec2) -> bool {
    at.x < PERIMETER_X
}

/// How deep the crawl hole is dug at `at`: `HOLE_DEPTH` on its floor, easing
/// to nothing at its rim.
pub(super) fn hole_depth(at: Vec2) -> f32 {
    let offset = (at - CRAWL_HOLE).abs();
    if offset.cmpge(HOLE_RIM).any() {
        return 0.0;
    }
    let edge = ((offset - HOLE_FLOOR) / (HOLE_RIM - HOLE_FLOOR)).max(Vec2::ZERO);
    HOLE_DEPTH * smoothstep(1.0, 0.0, edge.length())
}

/// Someone on official Institute business: the boom at the Institute gate
/// lifts for them.
#[derive(Component)]
pub(crate) struct InstitutePass;

/// The gates closed by a boom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Gate {
    /// The hangar yard's, beside its checkpoint.
    Yard,
    /// The Institute's, in the Perimeter.
    Institute,
}

impl Gate {
    /// The middle of the gate's lane.
    fn opening(self) -> Vec2 {
        match self {
            Gate::Yard => GATE,
            Gate::Institute => PERIMETER_GATE,
        }
    }

    /// Where the boom's post stands and which way its arm reaches across.
    fn post(self) -> (Vec2, Vec2) {
        match self {
            // On the wall's line at the end of the checkpoint's barrier.
            Gate::Yard => (Vec2::new(GATE.x + GATE_HALF, GATE.y), Vec2::NEG_X),
            // At the south jamb, on Harmont's side.
            Gate::Institute => (
                Vec2::new(PERIMETER_X + 1.0, PERIMETER_GATE.y - PERIMETER_GATE_HALF),
                Vec2::Y,
            ),
        }
    }
}

/// The boom across a gate's road, `lift` from lowered (0) to up (1).
#[derive(Component)]
#[require(Unclimbable)]
pub(crate) struct Boom {
    gate: Gate,
    lift: f32,
}

impl Boom {
    fn new(gate: Gate) -> Self {
        Self { gate, lift: 0.0 }
    }

    /// Whether it lifts for the stalker, or, with `pass`, for someone on
    /// Institute business: the yard's for both, the Institute's only for the latter.
    pub(crate) fn opens_for(&self, pass: bool) -> bool {
        pass || self.gate == Gate::Yard
    }
}

/// The boom's arm, hinged at the top of its post.
#[derive(Component)]
pub(super) struct BoomArm;

/// What names the crawl hole under the pointer, shown once the stalker is near.
#[derive(Component)]
pub(super) struct HoleSign;

pub(super) fn spawn_hole_sign(mut commands: Commands, terrain: Res<Terrain>) {
    commands.spawn((
        WorldRoot,
        HoleSign,
        Describe {
            place: Place::CrawlHole,
            radius: 2.5,
            height: 1.5,
        },
        Transform::from_translation(CRAWL_HOLE.extend(terrain.mesh_height_at(CRAWL_HOLE)).xzy()),
        Visibility::Hidden,
    ));
}

/// Lets the pointer name the crawl hole while the stalker is within `HOLE_NOTICE`.
pub(super) fn notice_crawl_hole(
    stalker: Query<&Transform, With<Player>>,
    mut signs: Query<&mut Visibility, With<HoleSign>>,
) {
    let near = stalker
        .iter()
        .any(|at| at.translation.xz().distance(CRAWL_HOLE) < HOLE_NOTICE);
    let shown = if near {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut signs {
        visibility.set_if_neq(shown);
    }
}

pub(super) fn reserve_scenery(at: Vec2, clearance: f32) -> bool {
    let margin = Vec2::splat(clearance);
    (at.cmpge(YARD_MIN - margin).all() && at.cmple(YARD_MAX + margin).all())
        || (at.abs().max_element() - EDGE).abs() < clearance
        || (at.x - PERIMETER_X).abs() < clearance + 3.0
        || near_checkpoint(at, CHECKPOINT, clearance + 3.0)
        || near_checkpoint(at, PERIMETER_CHECKPOINT, clearance + 3.0)
        || on_patrol_track(at, clearance)
}

#[cfg(test)]
pub(super) fn reserve_patrol_track(at: Vec2) -> bool {
    on_patrol_track(at, 0.0)
}

/// Within `clearance` of the UN patrol's track.
fn on_patrol_track(at: Vec2, clearance: f32) -> bool {
    let [south, north] = PATROL_TURNS;
    let line = north - south;
    let fraction = ((at - south).dot(line) / line.length_squared()).clamp(0.0, 1.0);
    let off_center = at.distance(south + line * fraction);
    (off_center - PATROL_RADIUS).abs() < PATROL_HALF_WIDTH + clearance
}

fn near_checkpoint(at: Vec2, checkpoint: Vec2, clearance: f32) -> bool {
    let outside = (at - checkpoint).abs() - CHECKPOINT_HALF;
    outside.max(Vec2::ZERO).length_squared() < clearance * clearance
}

pub(super) fn near_wall_or_checkpoint(at: Vec2, clearance: f32) -> bool {
    if (at.x - PERIMETER_X).abs() < 0.65 + clearance
        || near_checkpoint(at, PERIMETER_CHECKPOINT, clearance)
    {
        return true;
    }
    if (at - Vec2::from(BASE)).abs().max_element() > 84.0 + clearance {
        return false;
    }
    near_checkpoint(at, CHECKPOINT, clearance)
        || YARD_RUNS.into_iter().any(|(start, end)| {
            let line = end - start;
            let fraction = ((at - start).dot(line) / line.length_squared()).clamp(0.0, 1.0);
            at.distance_squared(start + line * fraction) < (0.65 + clearance).powi(2)
        })
}

/// How densely bushes and weeds grow in a belt: behind and beside the
/// hangar, along both sides of the Perimeter (clear of the patrol's track) and
/// in a thicket hiding the crawl hole.
pub(super) fn shelterbelt(at: Vec2) -> f32 {
    hangar_belt(at)
        .max(perimeter_belt(at))
        .max(hole_thicket(at))
}

/// Where the belt grows mostly bushes: the crawl hole's thicket and the
/// patrolled stretch of the Perimeter.
pub(super) fn bushy(at: Vec2) -> bool {
    thicket(at) || (patrolled(at) && perimeter_belt(at) > 0.0)
}

/// The dense thicket along the wall around the crawl hole.
pub(super) fn thicket(at: Vec2) -> bool {
    hole_thicket(at) > 0.0
}

/// How densely trees grow in a belt, from 0.0 to 1.0: behind and beside the
/// hangar, and along both sides of the patrolled stretch of the Perimeter.
pub(super) fn tree_belt(at: Vec2) -> f32 {
    (hangar_belt(at) * 0.60).max(perimeter_trees(at))
}

/// The dense belt behind and beside the hangar, from 0.0 to 1.0.
pub(super) fn hangar_belt(at: Vec2) -> f32 {
    let offset = at - Vec2::from(BASE);
    let side = offset.x.abs();
    if side < 103.0 && (-113.0..-44.0).contains(&offset.y) {
        1.0
    } else if (50.0..98.0).contains(&side) && (-58.0..47.0).contains(&offset.y) {
        0.68
    } else {
        0.0
    }
}

/// Where the Perimeter's belt may grow: within this far of the wall on the
/// Zone's side, and past the patrol's track on Harmont's.
pub(super) const PERIMETER_BELT: (f32, f32) = (-15.0, 46.0);

fn perimeter_belt(at: Vec2) -> f32 {
    let off = at.x - PERIMETER_X;
    if !(PERIMETER_BELT.0..PERIMETER_BELT.1).contains(&off)
        || off.abs() < 3.0
        || on_patrol_track(at, 1.0)
        || near_checkpoint(at, PERIMETER_CHECKPOINT, 3.0)
        || (at - PERIMETER_GATE).abs().max_element() < 22.0
    {
        return 0.0;
    }
    // Thickest against the wall on the Zone's side, and along the patrol.
    let (wall, edge, harmont) = if patrolled(at) {
        (1.0, 0.55, 0.75)
    } else {
        (0.8, 0.4, 0.55)
    };
    if off < 0.0 {
        lerp(wall, edge, off / PERIMETER_BELT.0)
    } else {
        harmont
    }
}

/// Along the stretch of the Perimeter the patrol watches, from 50 m south of
/// the hangar to 50 m past the Institute.
fn patrolled(at: Vec2) -> bool {
    (PATROL_TURNS[0].y..PATROL_TURNS[1].y).contains(&at.y)
}

/// Trees on both sides of the patrolled stretch, clear of the patrol's lanes
/// and of the gate with its guards.
fn perimeter_trees(at: Vec2) -> f32 {
    let off = at.x - PERIMETER_X;
    if !patrolled(at)
        || !(PERIMETER_BELT.0..PERIMETER_BELT.1).contains(&off)
        || on_patrol_track(at, 2.5)
        || near_checkpoint(at, PERIMETER_CHECKPOINT, 4.0)
        || (at - PERIMETER_GATE).abs().max_element() < 34.0
    {
        return 0.0;
    }
    if off < 0.0 { 0.5 } else { 0.35 }
}

/// A thicket of bushes along both sides of the wall, densest over the crawl hole.
fn hole_thicket(at: Vec2) -> f32 {
    let offset = (at - CRAWL_HOLE).abs();
    if offset.x > 12.0 || offset.y > 45.0 || on_patrol_track(at, 0.5) {
        0.0
    } else {
        lerp(1.0, 0.75, smoothstep(10.0, 45.0, offset.y))
    }
}

pub(super) fn preload_cordon(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    let (mesh, paint) = assets::load_single_primitive(&server, assets::SITE_CORDON_WALL);
    preloaded.0.extend([mesh.untyped(), paint.untyped()]);
    preloaded.0.push(
        server
            .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(assets::SITE_CHECKPOINT))
            .untyped(),
    );
}

/// A checkpoint on level ground, turned `yaw` from facing along +X.
fn checkpoint_pose(terrain: &Terrain, at: Vec2, yaw: f32) -> Transform {
    Transform::from_xyz(
        at.x,
        terrain.mesh_height_at(at) - CHECKPOINT_DATUM * CHECKPOINT_SCALE - 0.035,
        at.y,
    )
    .with_rotation(Quat::from_rotation_y(yaw))
    .with_scale(Vec3::splat(CHECKPOINT_SCALE))
}

pub(super) fn prepare_cordon(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    meshes: Res<Assets<Mesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (mesh, paint) = assets::load_single_primitive(&server, assets::SITE_CORDON_WALL);
    let mesh = meshes.get(&mesh).expect("preloaded cordon mesh");
    let paint = gltf_materials
        .get(&paint)
        .expect("preloaded cordon material");
    let root = commands
        .spawn((WorldRoot, Transform::IDENTITY, Visibility::default()))
        .id();
    for (name, at, yaw) in [
        ("Hangar checkpoint", CHECKPOINT, CHECKPOINT_YAW),
        ("Perimeter checkpoint", PERIMETER_CHECKPOINT, 0.0),
    ] {
        commands.spawn((
            ChildOf(root),
            Name::new(name),
            WorldAssetRoot(
                server.load(GltfAssetLabel::Scene(0).from_asset(assets::SITE_CHECKPOINT)),
            ),
            checkpoint_pose(&terrain, at, yaw),
            Collider(CHECKPOINT_PILLARS),
            Unclimbable,
            detail_range(400.0, 40.0),
        ));
    }
    commands.insert_resource(Cordon {
        root,
        chunks: wall_chunks(),
        active: HashMap::default(),
        centers: Vec::new(),
        wall: WallModel::new(mesh),
        paint: materials.add(standard_material_from_gltf_material(paint)),
    });
}

/// Paints the booms across the yard's and the Institute's gates: a post, and an
/// arm of red and white stripes a meter long each.
pub(super) fn spawn_booms(
    mut commands: Commands,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let paint = |color: Color| StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.6,
        ..default()
    };
    let post = materials.add(paint(Color::srgb(0.32, 0.33, 0.31)));
    let stripes = [Color::srgb(0.62, 0.05, 0.04), Color::srgb(0.85, 0.84, 0.8)]
        .map(|color| materials.add(paint(color)));
    let segments = BOOM_LENGTH.round() as usize;
    let length = BOOM_LENGTH / segments as f32;
    let segment = meshes.add(Cuboid::new(0.12, 0.12, length));
    let pole = meshes.add(Cuboid::new(0.3, BOOM_HINGE + 0.4, 0.3));
    for gate in [Gate::Yard, Gate::Institute] {
        let (at, along) = gate.post();
        commands
            .spawn((
                WorldRoot,
                Name::new("Gate boom"),
                Boom::new(gate),
                Collider(BOOM_PILLARS),
                // The arm reaches along the post's +Z.
                Transform::from_xyz(at.x, terrain.mesh_height_at(at), at.y)
                    .with_rotation(Quat::from_rotation_y(along.x.atan2(along.y))),
                Visibility::default(),
                detail_range(400.0, 40.0),
            ))
            .with_children(|boom| {
                boom.spawn((
                    Mesh3d(pole.clone()),
                    MeshMaterial3d(post.clone()),
                    Transform::from_xyz(0.0, (BOOM_HINGE + 0.4) * 0.5 - 0.2, 0.0),
                ));
                boom.spawn((
                    BoomArm,
                    Transform::from_xyz(0.0, BOOM_HINGE, 0.0),
                    Visibility::default(),
                ))
                .with_children(|arm| {
                    for i in 0..segments {
                        arm.spawn((
                            Mesh3d(segment.clone()),
                            MeshMaterial3d(stripes[i % 2].clone()),
                            Transform::from_xyz(0.0, 0.0, (i as f32 + 0.5) * length),
                        ));
                    }
                });
            });
    }
}

/// Lifts each boom while anyone it opens for is near its gate and lowers it
/// again after them; only the lowered arm stops anyone.
pub(super) fn lift_booms(
    time: Res<Time>,
    passes: Query<&GlobalTransform, With<InstitutePass>>,
    stalkers: Query<&GlobalTransform, With<Player>>,
    mut booms: Query<(&mut Boom, &mut Collider, &Children)>,
    mut arms: Query<&mut Transform, With<BoomArm>>,
) {
    for (mut boom, mut collider, children) in &mut booms {
        let opening = boom.gate.opening();
        let near = |pose: &GlobalTransform| pose.translation().xz().distance(opening) < BOOM_REACH;
        let business =
            passes.iter().any(near) || (boom.opens_for(false) && stalkers.iter().any(near));
        let step = BOOM_RATE * time.delta_secs() * if business { 1.0 } else { -1.0 };
        boom.lift = (boom.lift + step).clamp(0.0, 1.0);
        let solid = if boom.lift < BOOM_OPEN {
            BOOM_PILLARS
        } else {
            &[]
        };
        if collider.0.len() != solid.len() {
            collider.0 = solid;
        }
        // The arm swings up about the hinge, from along +Z to nearly upright.
        let angle = -1.45 * smoothstep(0.0, 1.0, boom.lift);
        for &child in children {
            if let Ok(mut arm) = arms.get_mut(child) {
                arm.rotation = Quat::from_rotation_x(angle);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Panel {
    center: Vec2,
    along: Vec2,
    length: f32,
    /// How many times as high as the standard cordon it stands.
    rise: f32,
}

impl Panel {
    fn across(self) -> Vec2 {
        self.along.perp()
    }

    /// The collider of the pillar at `at`, one of this panel's.
    fn collider(self, at: Vec2) -> &'static [Pillar] {
        if self.rise == 1.0 {
            WALL_PILLAR
        } else if hole_depth(at) > HOLE_CRAWL_DEPTH {
            HOLE_PILLAR
        } else {
            PERIMETER_PILLAR
        }
    }

    fn pillars(self) -> impl Iterator<Item = Vec2> {
        let count = (self.length / 0.77).ceil() as u32;
        (0..count).map(move |i| {
            self.center + self.along * self.length * ((i as f32 + 0.5) / count as f32 - 0.5)
        })
    }
}

fn tile_at(point: Vec2) -> IVec2 {
    (point / TILE_SIZE).floor().as_ivec2()
}

fn wall_chunks() -> HashMap<IVec2, Vec<Panel>> {
    let a = Vec2::splat(-EDGE);
    let b = Vec2::new(EDGE, -EDGE);
    let c = Vec2::splat(EDGE);
    let d = Vec2::new(-EDGE, EDGE);
    let perimeter = |z: f32| Vec2::new(PERIMETER_X, z);
    let perimeter_runs = [
        (
            perimeter(-EDGE),
            perimeter(PERIMETER_GATE.y - PERIMETER_GATE_HALF),
        ),
        (
            perimeter(PERIMETER_GATE.y + PERIMETER_GATE_HALF),
            perimeter(EDGE),
        ),
    ];
    let mut chunks: HashMap<_, Vec<_>> = HashMap::default();
    for (from, to, rise) in YARD_RUNS
        .into_iter()
        .chain([(a, b), (b, c), (c, d), (d, a)])
        .map(|(from, to)| (from, to, 1.0))
        .chain(perimeter_runs.map(|(from, to)| (from, to, PERIMETER_RISE)))
    {
        let span = from.distance(to);
        let along = (to - from) / span;
        let count = (span / 7.4).round().max(1.0) as u32;
        let length = span / count as f32;
        for i in 0..count {
            let center = from + (i as f32 + 0.5) * length * along;
            chunks.entry(tile_at(center)).or_default().push(Panel {
                center,
                along,
                length,
                rise,
            });
        }
    }
    chunks
}

#[derive(Resource)]
pub(super) struct Cordon {
    root: Entity,
    chunks: HashMap<IVec2, Vec<Panel>>,
    active: HashMap<IVec2, Entity>,
    centers: Vec<IVec2>,
    wall: WallModel,
    paint: Handle<StandardMaterial>,
}

pub(super) fn stream_cordon(
    mut commands: Commands,
    anchors: Query<&Transform, With<StreamAnchor>>,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cordon: ResMut<Cordon>,
) {
    let centers = anchor_tiles(anchors);
    if cordon.centers == centers {
        return;
    }
    cordon.centers = centers.clone();
    cordon.active.retain(|tile, entity| {
        let keep = near_anchor(*tile, &centers, 2);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for tile in neighborhoods(&centers, 2) {
        if cordon.active.contains_key(&tile) {
            continue;
        }
        if let Some(panels) = cordon.chunks.get(&tile) {
            let entity =
                spawn_wall_chunk(&mut commands, &mut meshes, &terrain, &cordon, tile, panels);
            cordon.active.insert(tile, entity);
        }
    }
}

fn spawn_wall_chunk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    terrain: &Terrain,
    cordon: &Cordon,
    tile: IVec2,
    panels: &[Panel],
) -> Entity {
    let origin = tile.as_vec2() * TILE_SIZE;
    let mesh = meshes.add(cordon.wall.batch(terrain, origin, panels));
    let mut root = commands.spawn((
        ChildOf(cordon.root),
        Name::new("Concrete cordon"),
        Transform::from_xyz(origin.x, 0.0, origin.y),
        Mesh3d(mesh),
        MeshMaterial3d(cordon.paint.clone()),
        detail_range(205.0, 25.0),
    ));
    if panels[0].center.abs().max_element() >= EDGE {
        root.insert(NotShadowCaster);
    }
    root.with_children(|wall| {
        for &panel in panels {
            for at in panel.pillars() {
                wall.spawn((
                    Collider(panel.collider(at)),
                    Unclimbable,
                    wall_pillar_pose(terrain, at, origin),
                ));
            }
        }
    })
    .id()
}

fn wall_pillar_pose(terrain: &Terrain, at: Vec2, origin: Vec2) -> Transform {
    let floor = floor_point(terrain, at);
    Transform::from_xyz(at.x - origin.x, floor.x + floor.y, at.y - origin.y)
}

struct WallModel {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl WallModel {
    fn new(mesh: &Mesh) -> Self {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("the wall GLB has float positions");
        };
        let Some(VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            unreachable!("the wall GLB has float normals");
        };
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            unreachable!("the wall GLB has float UVs");
        };
        Self {
            positions: positions.clone(),
            normals: normals.clone(),
            uv: uv.clone(),
            indices: mesh
                .indices()
                .into_iter()
                .flat_map(Indices::iter)
                .map(|i| i as u32)
                .collect(),
        }
    }

    fn batch(&self, terrain: &Terrain, origin: Vec2, panels: &[Panel]) -> Mesh {
        let mut positions = Vec::with_capacity(self.positions.len() * panels.len());
        let mut normals = Vec::with_capacity(positions.capacity());
        let mut uv = Vec::with_capacity(positions.capacity());
        let mut indices = Vec::with_capacity(self.indices.len() * panels.len());
        for &panel in panels {
            let scale_x = panel.length / PANEL_SPAN;
            let extents = panel.along.abs() * (0.51 * scale_x) + panel.across().abs() * 0.70;
            let floor = GroundPatch::new(terrain, panel.center - extents, panel.center + extents);
            let rise = WALL_HEIGHT_SCALE * panel.rise;
            let offset = positions.len() as u32;
            for (&[x, y, z], &[nx, ny, nz]) in self.positions.iter().zip(&self.normals) {
                let at = panel.center
                    + panel.along * (x * scale_x)
                    + panel.across() * (z * WALL_HEIGHT_SCALE);
                let base = floor.sample(at);
                // Continue over lakes and rivers on a concrete foundation, not a floating row of panels.
                let stretch = (y / 0.15).min(1.0);
                let height = y * rise + base.value.x + stretch * base.value.y - SINK;
                let horizontal =
                    panel.along * (nx / scale_x) + panel.across() * (nz / WALL_HEIGHT_SCALE);
                let vertical = ny
                    / (rise
                        + if stretch < 1.0 {
                            base.value.y / 0.15
                        } else {
                            0.0
                        });
                let slope = Vec2::new(
                    base.dx.x + stretch * base.dx.y,
                    base.dz.x + stretch * base.dz.y,
                );
                let normal = horizontal - slope * vertical;
                positions.push([at.x - origin.x, height, at.y - origin.y]);
                normals.push(
                    Vec3::new(normal.x, vertical, normal.y)
                        .normalize()
                        .to_array(),
                );
            }
            uv.extend_from_slice(&self.uv);
            indices.extend(self.indices.iter().map(|&index| index + offset));
        }
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(indices))
    }
}

fn floor_point(terrain: &Terrain, at: Vec2) -> Vec2 {
    // The wall spans the crawl hole on the undug ground.
    let ground = terrain.mesh_height_at(at) + hole_depth(at);
    let rise = if Wetlands::at(at.x, at.y).depth(ground) > 0.02 {
        (WATER_LEVEL - 0.25 - ground).max(0.0)
    } else {
        0.0
    };
    Vec2::new(ground, rise)
}

struct GroundPatch {
    corner: Vec2,
    width: usize,
    vertices: Vec<Vec2>,
}

struct GroundSample {
    value: Vec2,
    dx: Vec2,
    dz: Vec2,
}

impl GroundPatch {
    fn new(terrain: &Terrain, low: Vec2, high: Vec2) -> Self {
        let low = (low / SPACING).floor().as_ivec2();
        let high = (high / SPACING).floor().as_ivec2() + IVec2::ONE;
        let width = (high.x - low.x + 1) as usize;
        let vertices = (low.y..=high.y)
            .flat_map(|z| {
                (low.x..=high.x)
                    .map(move |x| floor_point(terrain, Vec2::new(x as f32, z as f32) * SPACING))
            })
            .collect();
        Self {
            corner: low.as_vec2(),
            width,
            vertices,
        }
    }

    fn sample(&self, at: Vec2) -> GroundSample {
        let grid = at / SPACING - self.corner;
        let cell = grid.floor().as_uvec2();
        let uv = grid - cell.as_vec2();
        let i = cell.y as usize * self.width + cell.x as usize;
        let (b, c) = (self.vertices[i + 1], self.vertices[i + self.width]);
        let (value, dx, dz) = if uv.x + uv.y <= 1.0 {
            let a = self.vertices[i];
            (a + (b - a) * uv.x + (c - a) * uv.y, b - a, c - a)
        } else {
            let d = self.vertices[i + self.width + 1];
            (
                d + (c - d) * (1.0 - uv.x) + (b - d) * (1.0 - uv.y),
                d - c,
                d - b,
            )
        };
        GroundSample {
            value,
            dx: dx / SPACING,
            dz: dz / SPACING,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::player::Player;
    use crate::player::vehicle::{Driving, Vehicle, blocked, drive_vehicle};
    use crate::world::{Body, fence_out, push_out};

    const STANDING: Body = Body {
        radius: 0.35,
        height: 1.8,
    };
    const PRONE: Body = Body {
        radius: 0.35,
        height: 0.5,
    };

    fn solids_near(
        terrain: &Terrain,
        origin: Vec2,
        radius: f32,
    ) -> Vec<(Collider, GlobalTransform)> {
        wall_chunks()
            .into_values()
            .flatten()
            .filter(|panel| panel.center.distance(origin) < radius + panel.length)
            .flat_map(|panel| panel.pillars().map(move |at| (panel, at)))
            .map(|(panel, at)| {
                (
                    Collider(panel.collider(at)),
                    wall_pillar_pose(terrain, at, Vec2::ZERO).into(),
                )
            })
            .collect()
    }

    #[test]
    fn the_checkpoint_leaves_a_level_lane_both_way_for_the_stalker_and_vehicle() {
        let terrain = Terrain::new();
        let gate = Vec2::new(GATE_CENTER, YARD_MAX.y);
        let mut solids = solids_near(&terrain, gate, 30.0);
        solids.push((
            Collider(CHECKPOINT_PILLARS),
            checkpoint_pose(&terrain, CHECKPOINT, CHECKPOINT_YAW).into(),
        ));
        let obstacles = || solids.iter().map(|(solid, pose)| (solid, pose));
        let body = Body {
            radius: 0.35,
            height: 1.8,
        };
        let mut prior = None;
        for i in -165..=165 {
            let z = gate.y + i as f32 * 0.1;
            let y = terrain.mesh_height_at(Vec2::new(gate.x, z));
            if let Some(prior) = prior {
                let dy: f32 = y - prior;
                assert!(dy.abs() < 0.045, "sharp approach step at {z}");
            }
            prior = Some(y);
            for dx in [-2.2, 0.0, 2.2] {
                let at = Vec2::new(gate.x + dx, z);
                assert_eq!(push_out(at, y, body, obstacles()), at);
                assert!(!blocked(
                    &Transform::from_xyz(at.x, y + 0.03, at.y),
                    obstacles()
                ));
            }
        }
        let bunker = checkpoint_pose(&terrain, CHECKPOINT, CHECKPOINT_YAW)
            .transform_point(Vec3::new(0.0, CHECKPOINT_DATUM, 0.0));
        assert!(blocked(&Transform::from_translation(bunker), obstacles()));
        let yard = Vec2::from(BASE);
        for x in [-42.0, 42.0] {
            for z in [-38.0, 48.0] {
                let at = yard + Vec2::new(x, z);
                assert!(inside_yard(at));
                assert!((terrain.mesh_height_at(at) - terrain.mesh_height_at(yard)).abs() < 0.05);
                assert_eq!(shelterbelt(at), 0.0);
            }
        }
    }

    #[test]
    fn the_armored_vehicle_drives_from_the_yard_through_the_checkpoint() {
        let terrain = Terrain::new();
        let gate = Vec2::new(GATE_CENTER, YARD_MAX.y);
        let checkpoint = checkpoint_pose(&terrain, CHECKPOINT, CHECKPOINT_YAW);
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        app.insert_resource(time)
            .insert_resource(terrain.clone())
            .insert_resource(ButtonInput::<KeyCode>::default())
            .add_systems(Update, drive_vehicle);
        let world = app.world_mut();
        for (collider, pose) in solids_near(&terrain, gate, 52.0) {
            world.spawn((collider, pose));
        }
        world.spawn((
            Collider(CHECKPOINT_PILLARS),
            GlobalTransform::from(checkpoint),
        ));
        let start = gate - Vec2::Y * 21.0;
        let pose = Transform::from_xyz(start.x, terrain.mesh_height_at(start) + 0.035, start.y);
        let car = world.spawn((Vehicle::default(), pose)).id();
        world.spawn((Player::default(), Driving(car), pose));
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        for _ in 0..310 {
            app.update();
        }
        let end = app.world().get::<Transform>(car).unwrap().translation;
        assert!(end.z > gate.y + 10.0, "vehicle stopped at {end}");
        assert!(
            (end.x - gate.x).abs() < 1.0,
            "vehicle drifted out of the lane: {end}"
        );
    }

    #[test]
    fn the_outer_cordon_is_continuous_at_seams_and_corners_before_the_map_limit() {
        let terrain = Terrain::new();
        let chunks = wall_chunks();
        for (&tile, panels) in &chunks {
            assert!(panels.iter().all(|panel| tile_at(panel.center) == tile));
        }
        let panels = chunks.values().flatten().copied().collect::<Vec<_>>();
        for across in [Vec2::Y, Vec2::X, Vec2::NEG_Y, Vec2::NEG_X] {
            let along = across.perp();
            let mut disks = panels
                .iter()
                .flat_map(|panel| panel.pillars())
                .filter(|at| at.dot(across) == EDGE)
                .map(|at| at.dot(along))
                .collect::<Vec<_>>();
            disks.sort_unstable_by(f32::total_cmp);
            assert!(disks[0] + EDGE < 0.40);
            assert!(EDGE - disks.last().copied().unwrap() < 0.40);
            assert!(disks.windows(2).all(|pair| pair[1] - pair[0] < 0.78));
        }
        let corner = Vec2::splat(EDGE) - Vec2::splat(0.12);
        let pillars = solids_near(&terrain, corner, 10.0);
        let y = terrain.mesh_height_at(corner);
        let obstacles = pillars.iter().map(|(solid, pose)| (solid, pose));
        assert!(blocked(
            &Transform::from_xyz(corner.x, y + 0.04, corner.y),
            obstacles
        ));
    }

    #[test]
    fn the_perimeter_stands_three_times_as_high_and_opens_only_at_the_gate() {
        let mut pillars = wall_chunks()
            .into_values()
            .flatten()
            .filter(|panel| panel.rise > 1.0)
            .flat_map(|panel| panel.pillars().map(move |at| (panel, at)))
            .collect::<Vec<_>>();
        assert!(pillars.iter().all(|(_, at)| at.x == PERIMETER_X));
        assert!(
            pillars
                .iter()
                .all(|&(panel, at)| panel.collider(at)[0].top > 3.0 * 2.4)
        );
        pillars.sort_unstable_by(|a, b| a.1.y.total_cmp(&b.1.y));
        let gaps = pillars
            .windows(2)
            .filter(|pair| pair[1].1.y - pair[0].1.y > 0.78)
            .map(|pair| (pair[0].1.y, pair[1].1.y))
            .collect::<Vec<_>>();
        // The old breach is walled up: the gate's road is the one way through.
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        let (south, north) = gaps[0];
        assert!(south < PERIMETER_GATE.y - PERIMETER_GATE_HALF + 0.4);
        assert!(north > PERIMETER_GATE.y + PERIMETER_GATE_HALF - 0.4);
        assert!(BREACH.y < south);
    }

    #[test]
    fn only_a_crawler_fits_under_the_wall_at_the_hidden_hole() {
        let terrain = Terrain::new();
        let solids = solids_near(&terrain, CRAWL_HOLE, 14.0);
        let obstacles = || solids.iter().map(|(solid, pose)| (solid, pose));
        assert_eq!(
            terrain.water_depth_at(CRAWL_HOLE.x, CRAWL_HOLE.y),
            0.0,
            "the hole floods"
        );
        // Straight through, from Harmont's side into the Zone.
        for i in -60..=60 {
            let at = CRAWL_HOLE + Vec2::new(i as f32 * 0.1, 0.0);
            let feet = terrain.mesh_height_at(at);
            assert_eq!(fence_out(at, feet, PRONE, obstacles()), at, "stuck at {at}");
            assert_eq!(push_out(at, feet, PRONE, obstacles()), at, "stuck at {at}");
        }
        let feet = terrain.mesh_height_at(CRAWL_HOLE);
        let dug = floor_point(&terrain, CRAWL_HOLE).x - feet;
        assert!((0.9..1.1).contains(&dug), "dug {dug} m");
        assert_ne!(
            push_out(CRAWL_HOLE, feet, STANDING, obstacles()),
            CRAWL_HOLE
        );
        // Beside the hole the wall's foot reaches into the ground again.
        let beside = CRAWL_HOLE + Vec2::Y * 6.0;
        let feet = terrain.mesh_height_at(beside);
        assert_ne!(fence_out(beside, feet, PRONE, obstacles()), beside);
        // The wall keeps a level foot across the hole: its base is the undug ground.
        let floor = floor_point(&terrain, CRAWL_HOLE).x;
        let either_side = [5.0, -5.0].map(|z| floor_point(&terrain, CRAWL_HOLE + Vec2::Y * z).x);
        let undug = (either_side[0] + either_side[1]) * 0.5;
        assert!((floor - undug).abs() < 0.15, "{floor} {either_side:?}");
    }

    #[test]
    fn the_gate_boom_lifts_for_institute_business_and_lowers_after() {
        let terrain = Terrain::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .add_systems(Update, lift_booms);
        let world = app.world_mut();
        let arm = world.spawn((BoomArm, Transform::IDENTITY)).id();
        let (post, along) = Gate::Institute.post();
        assert_eq!(along, Vec2::Y);
        let post = post.extend(terrain.mesh_height_at(post)).xzy();
        // The stalker on foot is turned away.
        world.spawn((Player::default(), GlobalTransform::from_translation(post)));
        let boom = world
            .spawn((
                Boom::new(Gate::Institute),
                Collider(BOOM_PILLARS),
                GlobalTransform::from_translation(post),
            ))
            .add_child(arm)
            .id();
        // The car waits on the road on Harmont's side of the lowered boom.
        let road = PERIMETER_GATE + Vec2::X * 3.0;
        let car = Transform::from_xyz(road.x, terrain.mesh_height_at(road) + 0.03, road.y)
            .looking_to(Vec3::NEG_X, Vec3::Y);
        let mut solids = solids_near(&terrain, PERIMETER_GATE, 20.0);
        solids.push((
            Collider(BOOM_PILLARS),
            GlobalTransform::from_translation(post),
        ));
        assert!(blocked(&car, solids.iter().map(|(s, p)| (s, p))));
        let stalker = app
            .world_mut()
            .spawn(GlobalTransform::from_translation(road.extend(0.0).xzy()))
            .id();
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(
            app.world().get::<Collider>(boom).unwrap().0.len(),
            BOOM_PILLARS.len()
        );

        app.world_mut().entity_mut(stalker).insert(InstitutePass);
        for _ in 0..60 {
            app.update();
        }
        assert!(app.world().get::<Collider>(boom).unwrap().0.is_empty());
        let up = app.world().get::<Transform>(arm).unwrap().rotation * Vec3::Z;
        assert!(up.y > 0.95, "arm at {up}");
        solids.pop();
        assert!(!blocked(&car, solids.iter().map(|(s, p)| (s, p))));

        app.world_mut().entity_mut(stalker).despawn();
        for _ in 0..60 {
            app.update();
        }
        assert_eq!(
            app.world().get::<Collider>(boom).unwrap().0.len(),
            BOOM_PILLARS.len()
        );
    }

    #[test]
    fn the_yard_boom_closes_the_lane_beside_the_checkpoint_and_lifts_for_the_stalker() {
        let (post, along) = Gate::Yard.post();
        assert_eq!(post + along * BOOM_LENGTH, WEST_JAMB);
        // The checkpoint's own barrier lines the wall and ends at the boom's post.
        let barrier = checkpoint_pose(&Terrain::new(), CHECKPOINT, CHECKPOINT_YAW)
            .transform_point(Vec3::new(BARRIER_END, 0.0, BARRIER_Z))
            .xz();
        assert!((barrier.y - GATE.y).abs() < 0.01, "barrier at {barrier}");
        assert!(
            (0.3..0.7).contains(&(barrier.x - post.x)),
            "barrier at {barrier}"
        );
        assert!(!Boom::new(Gate::Institute).opens_for(false));

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .add_systems(Update, lift_booms);
        let world = app.world_mut();
        let arm = world.spawn((BoomArm, Transform::IDENTITY)).id();
        let boom = world
            .spawn((Boom::new(Gate::Yard), Collider(BOOM_PILLARS)))
            .add_child(arm)
            .id();
        let yard = GATE.extend(0.0).xzy() - Vec3::Z * 20.0;
        world.spawn((Player::default(), GlobalTransform::from_translation(yard)));
        for _ in 0..60 {
            app.update();
        }
        assert!(app.world().get::<Collider>(boom).unwrap().0.is_empty());
    }

    #[test]
    fn walls_and_their_colliders_stream_together_between_the_yard_and_frontier() {
        let mut app = App::new();
        app.insert_resource(Terrain::new())
            .init_resource::<Assets<Mesh>>()
            .add_systems(Update, stream_cordon);
        let world = app.world_mut();
        let root = world.spawn(Transform::IDENTITY).id();
        let player = world
            .spawn((Player::default(), Transform::from_xyz(BASE.0, 0.0, BASE.1)))
            .id();
        world.insert_resource(Cordon {
            root,
            chunks: wall_chunks(),
            active: HashMap::default(),
            centers: Vec::new(),
            wall: WallModel {
                positions: vec![[-0.4, 0.0, 0.0], [0.4, 0.0, 0.0], [0.0, 0.3, 0.0]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                uv: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            },
            paint: Handle::default(),
        });
        app.update();
        let original = app.world().resource::<Cordon>().active.clone();
        assert!(!original.is_empty());
        app.world_mut()
            .get_mut::<Transform>(player)
            .unwrap()
            .translation
            .x = EDGE - 5.0;
        app.update();
        let cordon = app.world().resource::<Cordon>();
        assert!(!cordon.active.is_empty());
        assert!(
            cordon
                .active
                .keys()
                .all(|tile| !original.contains_key(tile))
        );
        assert!(
            original
                .values()
                .all(|&entity| app.world().get_entity(entity).is_err())
        );
        for &entity in cordon.active.values() {
            assert!(app.world().get::<Mesh3d>(entity).is_some());
            let children = app.world().get::<Children>(entity).unwrap();
            assert!(
                children
                    .iter()
                    .all(|child| app.world().get::<Collider>(child).is_some())
            );
        }
    }

    #[test]
    fn wall_profiles_track_the_rendered_floor_and_rise_above_deep_crossings() {
        let terrain = Terrain::new();
        // The east lake reaches across the far edge.
        let river = (600..920)
            .map(|z| Vec2::new(-EDGE, z as f32))
            .max_by(|&a, &b| {
                floor_point(&terrain, a)
                    .y
                    .total_cmp(&floor_point(&terrain, b).y)
            })
            .expect("a sample of the west cordon");
        assert!(floor_point(&terrain, river).y > 3.5);
        let dry = Vec2::new(EDGE, 1047.5);
        for at in [river, dry, Vec2::new(-21.0, -192.5)] {
            let patch = GroundPatch::new(&terrain, at - Vec2::splat(5.1), at + Vec2::splat(5.1));
            for sample in [at, at - Vec2::new(2.6, 0.81), at + Vec2::new(2.75, 0.92)] {
                let base = patch.sample(sample);
                assert!((base.value.x - terrain.mesh_height_at(sample)).abs() < 0.002);
                if at == river {
                    assert!(base.value.x < WATER_LEVEL - 2.0);
                    assert!(base.value.x + base.value.y + 2.48 > WATER_LEVEL + 2.0);
                }
            }
        }
    }
}
