//! New Harmont's townsfolk: factory and construction workers walking the
//! sidewalks round the town's four blocks, and scientists walking between the
//! plaza and the International Institute, going in at its door and coming back
//! out a while later. They stop now and then, wait for the stalker or a car in
//! their way, and cross the ring street only when no traffic is near. The
//! models are rigged like the guards (`tools/rig_stalker.py`).

use std::time::Duration;

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::{PreloadedAssets, TOWNSFOLK, TOWNSFOLK_FAR};
use crate::player::Player;

use super::collision::{Collider, Pillar, Unclimbable};
use super::places::{Describe, Place};
use super::sites::LANDMARKS;
use super::terrain::{Terrain, lattice_hash, lerp};
use super::town::{AVENUE, PLAZA, STREETS};
use super::{WorldRoot, detail_range, range_scene_meshes, roads};

/// Who a pedestrian is: indexes `TOWNSFOLK` and the tooltip text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Scientist,
    FactoryWorker,
    ConstructionWorker,
}

/// The rigs are 0.977 units tall, like the guards': about 1.8 m.
const MODEL_SCALE: f32 = 1.84;
/// Clip order in the rigs (the exporter sorts them by name), as for the guards.
const IDLE_CLIP: usize = 1;
const WALK_CLIP: usize = 4;
/// Each pedestrian's walking pace lies in this range; the walk clip's own
/// pace is the stalker's 1.8 m/s.
const PACE: (f32, f32) = (1.2, 1.4);
const CLIP_PACE: f32 = 1.8;
const BLEND: Duration = Duration::from_millis(300);
/// The full scan shows within this distance of the camera, the far tier beyond,
/// crossfading over `NEAR_FADE`.
const NEAR: f32 = 35.0;
const NEAR_FADE: f32 = 5.0;
/// How quickly a pedestrian turns to its heading, per second.
const TURN_RATE: f32 = 5.0;
/// From a street's middle line out to its sidewalk: clear of the lanes 2.5 m
/// either side, inside the 8.5 m paving.
const SIDEWALK: f32 = 7.0;
/// The share of corners where a pedestrian stops a while, and for how long.
const PAUSE_ODDS: f32 = 0.35;
const PAUSE: (f32, f32) = (2.0, 8.0);
/// Seconds a scientist spends inside the Institute.
const INSIDE: (f32, f32) = (30.0, 90.0);
/// The door lies this far past the Institute's entrance toward the building.
const DOOR_IN: f32 = 6.0;
/// Something within this gap ahead (beyond its own radius) stops a pedestrian.
const STOP_GAP: f32 = 1.2;
/// A pedestrian crosses a street only with no vehicle this near the crossing:
/// a sedan cruising at 8 m/s covers it while they walk across.
const CROSSING_CLEAR: f32 = 90.0;
/// Described things wider than this, on the ground, count as vehicles.
const VEHICLE_RADIUS: f32 = 1.5;
/// Higher above or below a pedestrian than this (a helicopter) is not in the way.
const IN_THE_WAY_HEIGHT: f32 = 3.0;
const BODY: &[Pillar] = &[Pillar {
    x: 0.0,
    z: 0.0,
    radius: 0.17,
    bottom: 0.0,
    top: 0.98,
}];

/// A walk through town: its waypoints, whether it loops or goes there and
/// back, and which legs cross a street.
#[derive(Clone, Debug)]
struct Route {
    points: Vec<Vec2>,
    looped: bool,
    /// The legs that cross a carriageway, by the index of their far end on the
    /// way up (only there-and-back routes cross).
    crossings: Vec<usize>,
    /// The last point is the Institute's door.
    door: bool,
}

impl Route {
    /// The waypoint after `from`, turning a there-and-back route at its ends.
    fn next(&self, from: usize, up: &mut bool) -> usize {
        let last = self.points.len() - 1;
        if self.looped {
            return (from + 1) % self.points.len();
        }
        if from == last {
            *up = false;
        } else if from == 0 {
            *up = true;
        }
        if *up { from + 1 } else { from - 1 }
    }

    fn crosses(&self, a: usize, b: usize) -> bool {
        !self.looped && a.abs_diff(b) == 1 && self.crossings.contains(&a.max(b))
    }

    /// Every leg as its two ends.
    fn legs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let n = self.points.len();
        let count = if self.looped { n } else { n - 1 };
        (0..count).map(move |i| (i, (i + 1) % n))
    }

    /// The point `fraction` of the way along the route, and the waypoint
    /// ahead of it when walking up (or down) the route.
    fn start(&self, fraction: f32, up: bool) -> (Vec2, usize) {
        let lengths: Vec<f32> = self
            .legs()
            .map(|(a, b)| self.points[a].distance(self.points[b]))
            .collect();
        let mut left = fraction.clamp(0.0, 1.0) * lengths.iter().sum::<f32>();
        for ((a, b), length) in self.legs().zip(lengths) {
            if left <= length {
                let at = self.points[a].lerp(self.points[b], left / length.max(1e-3));
                return (at, if up || self.looped { b } else { a });
            }
            left -= length;
        }
        let last = self.points.len() - 1;
        (self.points[last], last)
    }
}

/// The sidewalk round a town block between the streets at `x` and `z`,
/// `SIDEWALK` in from each; `reverse` walks it the other way round.
fn block(x: [f32; 2], z: [f32; 2], reverse: bool) -> Route {
    let (x0, x1) = (x[0] + SIDEWALK, x[1] - SIDEWALK);
    let (z0, z1) = (z[0] + SIDEWALK, z[1] - SIDEWALK);
    let mut points = vec![
        Vec2::new(x0, z0),
        Vec2::new(x1, z0),
        Vec2::new(x1, z1),
        Vec2::new(x0, z1),
    ];
    if reverse {
        points.reverse();
    }
    Route {
        points,
        looped: true,
        crossings: Vec::new(),
        door: false,
    }
}

/// The town's four blocks, between the ring street, the avenue and the
/// boulevard.
fn blocks(reverse: bool) -> [Route; 4] {
    let x = [AVENUE[0].x, PLAZA.x, AVENUE[1].x];
    let z = [STREETS[1][1].y, PLAZA.y, STREETS[0][1].y];
    [(0, 0), (1, 0), (0, 1), (1, 1)]
        .map(|(i, j)| block([x[i], x[i + 1]], [z[j], z[j + 1]], reverse))
}

/// The Institute's door: a little past its entrance, toward the building.
fn institute_door() -> Vec2 {
    let institute = LANDMARKS[0];
    let entrance = institute.entrance();
    entrance + (institute.center - entrance).normalize() * DOOR_IN
}

/// From the plaza west along the avenue's north sidewalk, across the ring
/// street, beside the Institute road (on the town's side, never crossing it)
/// and up the Institute's drive to its door.
fn institute_route() -> Route {
    let kerb = Vec2::new(AVENUE[0].x + SIDEWALK, PLAZA.y + SIDEWALK);
    let road = roads::INSTITUTE.x + SIDEWALK;
    Route {
        points: vec![
            PLAZA + Vec2::new(-SIDEWALK, SIDEWALK),
            kerb,
            kerb - Vec2::X * 2.0 * SIDEWALK,
            Vec2::new(road, PLAZA.y + SIDEWALK),
            Vec2::new(road, roads::INSTITUTE.y - SIDEWALK),
            LANDMARKS[0].entrance(),
            institute_door(),
        ],
        looped: false,
        crossings: vec![2],
        door: true,
    }
}

/// Where a pedestrian walks: one of the blocks (either way round), or to the
/// Institute and back.
#[derive(Clone, Copy)]
enum Walk {
    Block(usize, bool),
    Institute,
}

impl Walk {
    fn route(self) -> Route {
        match self {
            Walk::Block(i, reverse) => blocks(reverse)[i % 4].clone(),
            Walk::Institute => institute_route(),
        }
    }
}

/// Every pedestrian: who, where, how far along and which way they start.
/// One scientist starts at the Institute's door and comes out after a while.
const WALKERS: [(Role, Walk, f32, bool); 15] = [
    (Role::Scientist, Walk::Institute, 0.1, true),
    (Role::Scientist, Walk::Institute, 0.55, true),
    (Role::Scientist, Walk::Institute, 0.35, false),
    (Role::Scientist, Walk::Institute, 1.0, true),
    (Role::Scientist, Walk::Block(2, true), 0.3, true),
    (Role::FactoryWorker, Walk::Block(0, false), 0.1, true),
    (Role::FactoryWorker, Walk::Block(1, true), 0.6, true),
    (Role::FactoryWorker, Walk::Block(2, false), 0.8, true),
    (Role::FactoryWorker, Walk::Block(3, true), 0.2, true),
    (Role::FactoryWorker, Walk::Block(3, false), 0.5, true),
    (Role::ConstructionWorker, Walk::Block(0, true), 0.6, true),
    (Role::ConstructionWorker, Walk::Block(1, false), 0.15, true),
    (Role::ConstructionWorker, Walk::Block(2, true), 0.75, true),
    (Role::ConstructionWorker, Walk::Block(3, false), 0.9, true),
    (Role::ConstructionWorker, Walk::Block(0, false), 0.4, true),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Activity {
    Walking,
    /// Standing a while at a corner, seconds left.
    Pausing(f32),
    /// Inside the Institute, seconds left.
    Inside(f32),
}

/// A townsperson walking their route.
#[derive(Component, Debug)]
#[require(Unclimbable)]
pub(crate) struct Pedestrian {
    role: Role,
    route: Route,
    /// The waypoint it walks toward.
    toward: usize,
    /// Walking up a there-and-back route, rather than back down it.
    up: bool,
    pace: f32,
    activity: Activity,
    /// Waiting for the way ahead, or the crossing, to clear.
    waiting: bool,
    /// Its own seed, and the corners turned, for the pauses.
    seed: i32,
    corners: i32,
}

impl Pedestrian {
    fn new(index: usize, role: Role, walk: Walk, fraction: f32, up: bool) -> (Self, Vec2) {
        let route = walk.route();
        let (at, toward) = route.start(fraction, up);
        let seed = index as i32;
        let pedestrian = Self {
            role,
            route,
            toward,
            up,
            pace: lerp(PACE.0, PACE.1, lattice_hash(seed, 0, 0x70C5)),
            activity: Activity::Walking,
            waiting: false,
            seed,
            corners: 0,
        };
        (pedestrian, at)
    }

    /// Moving its legs: walking and not held up.
    fn walking(&self) -> bool {
        self.activity == Activity::Walking && !self.waiting
    }

    /// The walk clip runs at their pace; the idle one as it is.
    fn clip_speed(&self) -> f32 {
        if self.walking() {
            self.pace / CLIP_PACE
        } else {
            1.0
        }
    }

    fn inside(&self) -> bool {
        matches!(self.activity, Activity::Inside(_))
    }

    fn target(&self) -> Vec2 {
        self.route.points[self.toward]
    }

    /// The waypoint it last left.
    fn leaving(&self) -> usize {
        let n = self.route.points.len();
        match (self.route.looped, self.up) {
            (true, _) => (self.toward + n - 1) % n,
            (false, true) => self.toward.saturating_sub(1),
            (false, false) => (self.toward + 1).min(n - 1),
        }
    }

    /// At the kerb of a street it is about to cross: the middle of the crossing.
    fn crossing_ahead(&self, at: Vec2) -> Option<Vec2> {
        let from = self.leaving();
        let kerb = self.route.points[from];
        (self.route.crosses(from, self.toward) && at.distance(kerb) < 0.5)
            .then(|| kerb.lerp(self.target(), 0.5))
    }

    /// Steps the walk by `dt` seconds from `at`, returning where it stands.
    fn advance(&mut self, at: Vec2, dt: f32) -> Vec2 {
        match &mut self.activity {
            Activity::Pausing(left) | Activity::Inside(left) => {
                *left -= dt;
                if *left <= 0.0 {
                    self.activity = Activity::Walking;
                }
                return at;
            }
            Activity::Walking if self.waiting => return at,
            Activity::Walking => {}
        }
        let target = self.target();
        let step = self.pace * dt;
        if at.distance(target) > step {
            return at + (target - at).normalize() * step;
        }
        self.arrive();
        target
    }

    /// Reached its waypoint: goes in at the Institute's door, or perhaps
    /// stops a while, and heads for the next one.
    fn arrive(&mut self) {
        self.corners += 1;
        let roll = |salt| lattice_hash(self.seed, self.corners, salt);
        let at_door = self.route.door && self.toward == self.route.points.len() - 1;
        if at_door {
            self.activity = Activity::Inside(lerp(INSIDE.0, INSIDE.1, roll(0x1D51)));
        } else if roll(0x9A05) < PAUSE_ODDS {
            self.activity = Activity::Pausing(lerp(PAUSE.0, PAUSE.1, roll(0x51E7)));
        }
        self.toward = self.route.next(self.toward, &mut self.up);
    }
}

/// Something stands right in front of a pedestrian at `at` heading `heading`:
/// one of `others`, each a position and radius.
fn way_blocked(at: Vec3, heading: Vec2, others: impl IntoIterator<Item = (Vec3, f32)>) -> bool {
    others.into_iter().any(|(other, radius)| {
        let offset = (other - at).xz();
        (other.y - at.y).abs() < IN_THE_WAY_HEIGHT
            && offset.dot(heading) > 0.0
            && offset.length() < radius + STOP_GAP
    })
}

/// A vehicle (anything described wider than a person, on the ground) is near
/// the crossing at `crossing`.
fn traffic_near(crossing: Vec3, others: impl IntoIterator<Item = (Vec3, f32)>) -> bool {
    others.into_iter().any(|(other, radius)| {
        radius > VEHICLE_RADIUS
            && (other.y - crossing.y).abs() < IN_THE_WAY_HEIGHT
            && other.xz().distance(crossing.xz()) < CROSSING_CLEAR
    })
}

fn heading(direction: Vec2) -> Quat {
    // The rigs face +Z.
    Quat::from_rotation_y(direction.x.atan2(direction.y))
}

/// What a role looks and moves like: its animation graph with the idle and
/// walk nodes, and the near model's material, which the far tier borrows
/// (`source` as loaded, `paint` made from it once on entering the game; both
/// tiers then show the same texture images).
struct Look {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    source: Handle<GltfMaterial>,
    paint: Handle<StandardMaterial>,
}

/// Each role's look, indexed by role.
#[derive(Resource)]
pub(super) struct TownsfolkLooks(Vec<Look>);

/// Marks a pedestrian's far-tier model.
#[derive(Component)]
pub(super) struct FarTier;

/// The animation player inside a pedestrian's near model, and whether it shows
/// walking. It leads: the far model's player copies it.
#[derive(Component)]
pub(super) struct TownsfolkAnimator {
    pedestrian: Entity,
    walking: bool,
}

/// The animation player inside a pedestrian's far model.
#[derive(Component)]
pub(super) struct FarAnimator {
    pedestrian: Entity,
}

pub(super) fn preload_townsfolk(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut preloaded: ResMut<PreloadedAssets>,
) {
    let looks = TOWNSFOLK
        .iter()
        .zip(TOWNSFOLK_FAR)
        .map(|(&near, far)| {
            // Both tiers share one skeleton, so the near model's clips drive both.
            let clips = [IDLE_CLIP, WALK_CLIP].map(|i| {
                server.load::<AnimationClip>(GltfAssetLabel::Animation(i).from_asset(near))
            });
            preloaded
                .0
                .extend(clips.iter().cloned().map(Handle::untyped));
            for path in [near, far] {
                let scene = GltfAssetLabel::Scene(0).from_asset(path);
                preloaded.0.push(server.load::<WorldAsset>(scene).untyped());
            }
            let (graph, nodes) = AnimationGraph::from_clips(clips);
            let paint = GltfAssetLabel::Material {
                index: 0,
                is_scale_inverted: false,
            };
            let source = server.load(paint.from_asset(near));
            preloaded.0.push(source.clone().untyped());
            Look {
                graph: graphs.add(graph),
                idle: nodes[0],
                walk: nodes[1],
                source,
                paint: Handle::default(),
            }
        })
        .collect();
    commands.insert_resource(TownsfolkLooks(looks));
}

/// Spawns the walkers, each showing the full scan within `NEAR` of the camera
/// and the light far tier beyond it.
pub(super) fn spawn_townsfolk(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    mut looks: ResMut<TownsfolkLooks>,
    sources: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for look in &mut looks.0 {
        let source = sources
            .get(&look.source)
            .expect("the townsfolk materials finish preloading before the Zone opens");
        look.paint = materials.add(standard_material_from_gltf_material(source));
    }
    let near = VisibilityRange {
        end_margin: NEAR..NEAR + NEAR_FADE,
        use_aabb: true,
        ..default()
    };
    let far = VisibilityRange {
        start_margin: near.end_margin.clone(),
        ..detail_range(150.0, 20.0)
    };
    for (i, (role, walk, fraction, up)) in WALKERS.into_iter().enumerate() {
        let (pedestrian, at) = Pedestrian::new(i, role, walk, fraction, up);
        let facing = heading(pedestrian.target() - at);
        let root = commands
            .spawn((
                Name::new("Townsperson"),
                Transform::from_xyz(at.x, terrain.mesh_height_at(at), at.y)
                    .with_rotation(facing)
                    .with_scale(Vec3::splat(MODEL_SCALE)),
                Visibility::default(),
                Collider(BODY),
                Describe {
                    place: Place::Townsfolk(role),
                    radius: 0.5,
                    height: 1.9,
                },
                pedestrian,
                WorldRoot,
            ))
            .id();
        let model = |path: &'static str, range: &VisibilityRange| {
            (
                ChildOf(root),
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))),
                Transform::default(),
                Visibility::default(),
                range.clone(),
            )
        };
        commands
            .spawn(model(TOWNSFOLK[role as usize], &near))
            .observe(range_scene_meshes)
            .observe(attach_townsfolk_animator);
        commands
            .spawn((model(TOWNSFOLK_FAR[role as usize], &far), FarTier))
            .observe(range_scene_meshes)
            .observe(attach_townsfolk_animator)
            .observe(share_near_paint);
    }
}

/// Hooks a pedestrian's rig up to its animation graph once either of its
/// models has spawned.
fn attach_townsfolk_animator(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    looks: Res<TownsfolkLooks>,
    models: Query<(&ChildOf, Has<FarTier>)>,
    pedestrians: Query<&Pedestrian>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok((parent, far)) = models.get(ready.entity) else {
        return;
    };
    let owner = parent.parent();
    let Ok(pedestrian) = pedestrians.get(owner) else {
        return;
    };
    let Some(look) = looks.0.get(pedestrian.role as usize) else {
        return;
    };
    for entity in children.iter_descendants(ready.entity) {
        let Ok(mut animation_player) = players.get_mut(entity) else {
            continue;
        };
        let mut rig = commands.entity(entity);
        rig.insert(AnimationGraphHandle(look.graph.clone()));
        if far {
            rig.insert(FarAnimator { pedestrian: owner });
            continue;
        }
        let mut transitions = AnimationTransitions::new();
        let walking = pedestrian.walking();
        transitions
            .play(
                &mut animation_player,
                if walking { look.walk } else { look.idle },
                Duration::ZERO,
            )
            .set_speed(pedestrian.clip_speed())
            .repeat();
        rig.insert((
            transitions,
            TownsfolkAnimator {
                pedestrian: owner,
                walking,
            },
        ));
    }
}

/// Draws a far model with its near model's material: the far GLB carries no
/// textures, so the 4K maps are loaded once per role.
fn share_near_paint(
    ready: On<WorldInstanceReady>,
    looks: Res<TownsfolkLooks>,
    parents: Query<&ChildOf>,
    pedestrians: Query<&Pedestrian>,
    children: Query<&Children>,
    mut paints: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let Some(look) = parents
        .get(ready.entity)
        .and_then(|parent| pedestrians.get(parent.parent()))
        .ok()
        .and_then(|pedestrian| looks.0.get(pedestrian.role as usize))
    else {
        return;
    };
    for entity in children.iter_descendants(ready.entity) {
        if let Ok(mut paint) = paints.get_mut(entity) {
            paint.0 = look.paint.clone();
        }
    }
}

/// Holds a pedestrian up while the stalker or a vehicle stands right in front
/// of them, or while traffic is near the street they are about to cross.
pub(super) fn watch_the_way(
    mut pedestrians: Query<(&mut Pedestrian, &Transform)>,
    stalkers: Query<&Transform, With<Player>>,
    others: Query<(&GlobalTransform, &Describe), Without<Pedestrian>>,
) {
    let others: Vec<(Vec3, f32)> = stalkers
        .iter()
        .map(|stalker| (stalker.translation, 0.4))
        .chain(
            others
                .iter()
                .map(|(pose, describe)| (pose.translation(), describe.radius)),
        )
        .collect();
    for (mut pedestrian, transform) in &mut pedestrians {
        if pedestrian.activity != Activity::Walking {
            continue;
        }
        let at = transform.translation;
        let ahead = (pedestrian.target() - at.xz()).normalize_or_zero();
        let crossing = pedestrian.crossing_ahead(at.xz());
        let waiting = way_blocked(at, ahead, others.iter().copied())
            || crossing.is_some_and(|middle| {
                traffic_near(middle.extend(at.y).xzy(), others.iter().copied())
            });
        if pedestrian.waiting != waiting {
            pedestrian.waiting = waiting;
        }
    }
}

/// Walks each pedestrian along their route over the rendered ground, turning
/// smoothly toward where they head.
pub(super) fn walk_townsfolk(
    time: Res<Time>,
    terrain: Res<Terrain>,
    mut pedestrians: Query<(&mut Pedestrian, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let turn = 1.0 - (-TURN_RATE * dt).exp();
    for (mut pedestrian, mut transform) in &mut pedestrians {
        let at = pedestrian.advance(transform.translation.xz(), dt);
        transform.translation = at.extend(terrain.mesh_height_at(at)).xzy();
        let Some(direction) = (pedestrian.target() - at).try_normalize() else {
            continue;
        };
        if pedestrian.inside() {
            // Comes back out facing the way home.
            transform.rotation = heading(direction);
        } else if pedestrian.walking() {
            transform.rotation = transform.rotation.slerp(heading(direction), turn);
        }
    }
}

/// Hides a scientist inside the Institute, and their collider with them.
pub(super) fn show_townsfolk(
    mut commands: Commands,
    mut pedestrians: Query<(Entity, &Pedestrian, &mut Visibility)>,
) {
    for (entity, pedestrian, mut visibility) in &mut pedestrians {
        let hidden = *visibility == Visibility::Hidden;
        if pedestrian.inside() && !hidden {
            *visibility = Visibility::Hidden;
            commands.entity(entity).remove::<Collider>();
        } else if !pedestrian.inside() && hidden {
            *visibility = Visibility::Inherited;
            commands.entity(entity).insert(Collider(BODY));
        }
    }
}

/// Cross-fades each pedestrian between standing and walking, at a stride
/// matching their pace.
pub(super) fn animate_townsfolk(
    looks: Res<TownsfolkLooks>,
    pedestrians: Query<&Pedestrian>,
    mut animators: Query<(
        &mut TownsfolkAnimator,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
    )>,
) {
    for (mut animator, mut animation_player, mut transitions) in &mut animators {
        let Ok(pedestrian) = pedestrians.get(animator.pedestrian) else {
            continue;
        };
        let walking = pedestrian.walking();
        if walking == animator.walking {
            continue;
        }
        let Some(look) = looks.0.get(pedestrian.role as usize) else {
            continue;
        };
        transitions
            .play(
                &mut animation_player,
                if walking { look.walk } else { look.idle },
                BLEND,
            )
            .set_speed(pedestrian.clip_speed())
            .repeat();
        animator.walking = walking;
    }
}

/// Keeps each far model's rig in step with its near model's (same clips, times
/// and blend weights), so the crossfade between the tiers never shows two poses.
pub(super) fn follow_near_animators(
    leaders: Query<(&TownsfolkAnimator, &AnimationPlayer)>,
    mut followers: Query<(&FarAnimator, &mut AnimationPlayer), Without<TownsfolkAnimator>>,
) {
    for (follower, mut animation_player) in &mut followers {
        let lead = leaders
            .iter()
            .find(|(leader, _)| leader.pedestrian == follower.pedestrian);
        if let Some((_, lead)) = lead {
            animation_player.clone_from(lead);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::cordon::in_zone;
    use crate::world::{sites, town, traffic_routes};

    /// Every route's legs, sampled each half meter, with whether the leg
    /// crosses a street and whether it is the drive up to the Institute.
    fn samples(route: &Route) -> Vec<(Vec2, bool, bool)> {
        let last = route.points.len() - 1;
        route
            .legs()
            .flat_map(|(a, b)| {
                let (from, to) = (route.points[a], route.points[b]);
                let steps = (from.distance(to) * 2.0).ceil() as usize;
                let crossing = route.crosses(a, b);
                let drive = route.door && b + 1 >= last;
                (0..=steps).map(move |i| (from.lerp(to, i as f32 / steps as f32), crossing, drive))
            })
            .collect()
    }

    fn distance_to_lanes(at: Vec2) -> f32 {
        traffic_routes()
            .into_iter()
            .flat_map(|(points, closed)| {
                let n = points.len();
                let legs = if closed { n } else { n - 1 };
                (0..legs)
                    .map(|i| [points[i], points[(i + 1) % n]])
                    .collect::<Vec<_>>()
            })
            .map(|[a, b]| {
                let along = (b - a).normalize();
                let run = (at - a).dot(along).clamp(0.0, a.distance(b));
                at.distance(a + along * run)
            })
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn routes_keep_to_dry_sidewalks_off_the_carriageway_and_buildings() {
        let terrain = Terrain::new();
        let routes = blocks(false).into_iter().chain([institute_route()]);
        for route in routes {
            for (at, crossing, drive) in samples(&route) {
                assert!(!in_zone(at), "in the Zone at {at}");
                assert_eq!(terrain.water_depth_at(at.x, at.y), 0.0, "wet at {at}");
                assert!(!town::near_building(at, 1.0), "in a building at {at}");
                if !crossing {
                    assert!(distance_to_lanes(at) > 4.0, "in traffic at {at}");
                }
                if route.looped {
                    assert!(town::contains(at), "out of town at {at}");
                } else if !drive {
                    // Only the drive's last meters enter the Institute's grounds.
                    assert!(
                        !sites::clear_footprint(at, 0.0) || town::paved(at),
                        "on a site at {at}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_scientists_walk_ends_at_the_institute_door() {
        let institute = LANDMARKS[0];
        assert_eq!(
            institute.junction,
            roads::INSTITUTE,
            "the Institute leads LANDMARKS"
        );
        let route = institute_route();
        let door = *route.points.last().expect("a route has points");
        assert_eq!(door, institute_door());
        assert!(door.distance(institute.entrance()) <= DOOR_IN + 1e-3);
        assert!(door.distance(institute.center) < institute.entrance().distance(institute.center));
        assert!(town::paved(route.points[0]), "it starts on the plaza");
        // It crosses the ring street once, and no other street.
        assert_eq!(
            route.legs().filter(|&(a, b)| route.crosses(a, b)).count(),
            1
        );
    }

    /// Steps `pedestrian` for `seconds`, returning the activities seen.
    fn run(pedestrian: &mut Pedestrian, at: &mut Vec2, seconds: f32) -> Vec<Activity> {
        let dt = 0.1;
        let mut seen = Vec::new();
        for _ in 0..(seconds / dt) as usize {
            *at = pedestrian.advance(*at, dt);
            seen.push(pedestrian.activity);
        }
        seen
    }

    #[test]
    fn a_scientist_goes_in_at_the_door_comes_out_and_walks_back_to_town() {
        let (mut scientist, mut at) =
            Pedestrian::new(0, Role::Scientist, Walk::Institute, 0.95, true);
        for _ in 0..600 {
            if scientist.inside() {
                break;
            }
            run(&mut scientist, &mut at, 0.1);
        }
        assert!(scientist.inside(), "went in");
        assert_eq!(at, institute_door());
        run(&mut scientist, &mut at, INSIDE.1 + 1.0);
        assert!(!scientist.inside(), "came back out");
        assert!(!scientist.up, "heading back to town");
        let start = institute_route().points[0];
        let before = at.distance(start);
        run(&mut scientist, &mut at, 60.0);
        assert!(at.distance(start) < before, "walking toward the plaza");
    }

    #[test]
    fn a_worker_rounds_their_block_pausing_at_some_corners() {
        let (mut worker, mut at) =
            Pedestrian::new(5, Role::FactoryWorker, Walk::Block(0, false), 0.0, true);
        let start = at;
        let lap: f32 = worker
            .route
            .legs()
            .map(|(a, b)| worker.route.points[a].distance(worker.route.points[b]))
            .sum();
        let mut paused = false;
        let mut far = 0.0_f32;
        let mut back = false;
        for _ in 0..4 {
            let seen = run(&mut worker, &mut at, lap / PACE.0 / 2.0);
            paused |= seen.iter().any(|a| matches!(a, Activity::Pausing(_)));
            far = far.max(at.distance(start));
            back |= far > 50.0 && at.distance(start) < 30.0;
        }
        assert!(paused, "never stopped at a corner");
        assert!(back, "did not come round the block");
        assert!(!worker.inside());
    }

    #[test]
    fn a_pedestrian_waits_for_the_way_and_the_crossing_to_clear() {
        let at = Vec3::ZERO;
        let stalker = (Vec3::new(0.0, 0.0, 1.0), 0.4);
        assert!(way_blocked(at, Vec2::Y, [stalker]));
        assert!(!way_blocked(at, Vec2::NEG_Y, [stalker]), "behind");
        let helicopter = (Vec3::new(0.0, 20.0, 1.0), 5.0);
        assert!(!way_blocked(at, Vec2::Y, [helicopter]));
        let sedan = (Vec3::new(60.0, 0.0, 0.0), 2.2);
        assert!(traffic_near(at, [sedan]));
        assert!(!traffic_near(at, [stalker]), "a person is not traffic");

        // At the ring street's kerb, the scientist sees the crossing ahead.
        let (mut scientist, _) = Pedestrian::new(0, Role::Scientist, Walk::Institute, 0.0, true);
        scientist.toward = 2;
        let kerb = scientist.route.points[1];
        assert!(scientist.crossing_ahead(kerb).is_some());
        assert!(scientist.crossing_ahead(kerb + Vec2::X * 3.0).is_none());
        scientist.waiting = true;
        assert_eq!(scientist.advance(kerb, 1.0), kerb, "waits at the kerb");
        assert!(!scientist.walking());
    }

    #[test]
    fn walkers_are_a_mix_of_trades_with_scientists_at_the_institute() {
        let count = |role| WALKERS.iter().filter(|w| w.0 == role).count();
        assert!((10..=16).contains(&WALKERS.len()));
        assert!(count(Role::Scientist) >= 4);
        assert!(count(Role::FactoryWorker) >= 4 && count(Role::ConstructionWorker) >= 4);
        let to_institute = WALKERS
            .iter()
            .filter(|w| matches!(w.1, Walk::Institute))
            .count();
        assert!(to_institute >= 3);
    }
}
