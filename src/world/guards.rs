//! UN soldiers guarding the ways through the cordons: two at the hangar yard's
//! gate and two at the Institute gate in the Perimeter, each pacing a short
//! beat along the wall beside the opening, swinging its arms in the walk clip,
//! and standing a while at either end watching the passage. Five sentries keep
//! shorter beats by the checkpoints: three at the Institute gate's and two at
//! the hangar yard's. The soldiers are rigged like the stalker
//! (`tools/rig_stalker.py`).

use std::time::Duration;

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::{GUARDS, PreloadedAssets};

use super::collision::{Collider, Pillar, Unclimbable};
use super::cordon::{
    CHECKPOINT, PERIMETER_CHECKPOINT, PERIMETER_GATE, PERIMETER_GATE_HALF, YARD_GAP,
};
use super::places::{Describe, Place};
use super::terrain::{Terrain, lattice_hash, lerp};
use super::{WorldRoot, detail_range, range_scene_meshes};

/// The rigs are 0.977 units tall, like the stalker's: about 1.8 m.
const MODEL_SCALE: f32 = 1.84;
/// Clip order in the rigs (the exporter sorts them by name).
const IDLE_CLIP: usize = 1;
const WALK_CLIP: usize = 4;
/// A slow patrol pace; the walk clip matches the stalker's 1.8 m/s.
const WALK_SPEED: f32 = 1.2;
const WALK_CLIP_SPEED: f32 = WALK_SPEED / 1.8;
const BLEND: Duration = Duration::from_millis(250);
/// How quickly a guard turns to its heading, per second.
const TURN_RATE: f32 = 6.0;
/// Seconds a guard stands at each end of its beat.
const REST: (f32, f32) = (3.0, 9.0);
/// A beat starts this far along the wall from the opening's edge, lies this far
/// out from the wall on the guarded side, and runs this long.
const BEAT_START: f32 = 2.0;
const BEAT_OUT: f32 = 2.5;
const BEAT_LENGTH: f32 = 6.0;
/// A sentry's shorter beat beside its checkpoint.
const SENTRY_BEAT: f32 = 3.0;
const BODY: &[Pillar] = &[Pillar {
    x: 0.0,
    z: 0.0,
    radius: 0.17,
    bottom: 0.0,
    top: 0.98,
}];

/// A soldier pacing between the two ends of its beat.
#[derive(Component, Debug)]
#[require(Unclimbable)]
pub(super) struct Guard {
    beat: [Vec2; 2],
    /// The end it walks toward, or stands at while resting.
    toward: usize,
    /// Seconds left to stand still.
    rest: f32,
    /// How long it stands at each end.
    rests: [f32; 2],
    /// Where it looks while standing: across the opening.
    watch: Vec2,
}

impl Guard {
    fn walking(&self) -> bool {
        self.rest <= 0.0
    }
}

/// Each rig's animation graph, with its idle and walk nodes.
#[derive(Resource)]
pub(super) struct GuardAnimations(Vec<(Handle<AnimationGraph>, [AnimationNodeIndex; 2])>);

/// The animation player inside a guard's model, and whether it shows walking.
#[derive(Component)]
pub(super) struct GuardAnimator {
    guard: Entity,
    walking: bool,
}

/// Two guards for an opening `half` meters wide either side of `center` in a
/// wall running `along`: their beats lie on the `out` side, one either side.
fn post(center: Vec2, half: f32, along: Vec2, out: Vec2) -> [Guard; 2] {
    [-1.0, 1.0].map(|side| {
        let start = center + along * side * (half + BEAT_START) + out * BEAT_OUT;
        Guard {
            beat: [start, start + along * side * BEAT_LENGTH],
            toward: 1,
            rest: 0.0,
            rests: [REST.0; 2],
            watch: -along * side,
        }
    })
}

/// A sentry pacing `SENTRY_BEAT` from `at` along `along`, watching `watch`
/// from either end.
fn sentry(at: Vec2, along: Vec2, watch: Vec2) -> Guard {
    Guard {
        beat: [at, at + along * SENTRY_BEAT],
        toward: 1,
        rest: 0.0,
        rests: [REST.0; 2],
        watch,
    }
}

/// Every guard: outside the yard's gate, on Harmont's side of the Perimeter
/// gate, and the sentries by the two checkpoints, clear of the patrol's lanes.
fn guards() -> impl Iterator<Item = Guard> {
    let posts = [
        post(YARD_GAP.0, YARD_GAP.1, Vec2::X, Vec2::Y),
        post(PERIMETER_GATE, PERIMETER_GATE_HALF, Vec2::Y, Vec2::X),
    ];
    let sentries = [
        // Between the Institute gate's checkpoint and the road, and behind it.
        sentry(
            PERIMETER_CHECKPOINT + Vec2::new(-1.0, 8.5),
            Vec2::X,
            Vec2::Y,
        ),
        sentry(PERIMETER_CHECKPOINT + Vec2::new(8.0, 8.0), Vec2::X, Vec2::Y),
        sentry(
            PERIMETER_CHECKPOINT + Vec2::new(9.0, -5.0),
            Vec2::Y,
            Vec2::X,
        ),
        // Inside the hangar yard, behind its checkpoint and east of it.
        sentry(CHECKPOINT + Vec2::new(-2.0, -6.5), Vec2::X, Vec2::Y),
        sentry(CHECKPOINT + Vec2::new(7.5, -5.0), Vec2::NEG_Y, Vec2::NEG_X),
    ];
    posts.into_iter().flatten().chain(sentries)
}

fn heading(direction: Vec2) -> Quat {
    // The rigs face +Z.
    Quat::from_rotation_y(direction.x.atan2(direction.y))
}

pub(super) fn preload_guards(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut preloaded: ResMut<PreloadedAssets>,
) {
    let animations = GUARDS
        .iter()
        .map(|&path| {
            let clips = [IDLE_CLIP, WALK_CLIP].map(|i| {
                server.load::<AnimationClip>(GltfAssetLabel::Animation(i).from_asset(path))
            });
            preloaded
                .0
                .extend(clips.iter().cloned().map(Handle::untyped));
            preloaded.0.push(
                server
                    .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(path))
                    .untyped(),
            );
            let (graph, nodes) = AnimationGraph::from_clips(clips);
            let nodes = [nodes[0], nodes[1]];
            (graphs.add(graph), nodes)
        })
        .collect();
    commands.insert_resource(GuardAnimations(animations));
}

pub(super) fn spawn_guards(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
) {
    for (i, mut guard) in guards().enumerate() {
        let at = guard.beat[0];
        // Each guard keeps its own time, so the two at a post fall out of step.
        guard.rests = [1, 2].map(|n| lerp(REST.0, REST.1, lattice_hash(i as i32, n, 0x6A4D)));
        guard.rest = guard.rests[0];
        let model = i % GUARDS.len();
        commands
            .spawn((
                Name::new("Guard"),
                Transform::from_xyz(at.x, terrain.mesh_height_at(at), at.y)
                    .with_rotation(heading(guard.watch))
                    .with_scale(Vec3::splat(MODEL_SCALE)),
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(GUARDS[model]))),
                detail_range(150.0, 20.0),
                Collider(BODY),
                Describe {
                    place: Place::Guard,
                    radius: 0.5,
                    height: 1.9,
                },
                GuardModel(model),
                guard,
                WorldRoot,
            ))
            .observe(range_scene_meshes)
            .observe(attach_guard_animator);
    }
}

/// Which rig a guard wears.
#[derive(Component)]
pub(super) struct GuardModel(usize);

/// Hooks a guard's rig up to its animation graph once the scene has spawned.
fn attach_guard_animator(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    animations: Res<GuardAnimations>,
    guards: Query<(&Guard, &GuardModel)>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok((guard, model)) = guards.get(ready.entity) else {
        return;
    };
    let Some((graph, [idle, walk])) = animations.0.get(model.0) else {
        return;
    };
    for entity in children.iter_descendants(ready.entity) {
        let Ok(mut animation_player) = players.get_mut(entity) else {
            continue;
        };
        let mut transitions = AnimationTransitions::new();
        let walking = guard.walking();
        let node = if walking { *walk } else { *idle };
        transitions
            .play(&mut animation_player, node, Duration::ZERO)
            .set_speed(if walking { WALK_CLIP_SPEED } else { 1.0 })
            .repeat();
        commands.entity(entity).insert((
            AnimationGraphHandle(graph.clone()),
            transitions,
            GuardAnimator {
                guard: ready.entity,
                walking,
            },
        ));
    }
}

/// Walks each guard along its beat over the rendered ground, turning at the
/// ends to watch the opening for a while.
pub(super) fn patrol_guards(
    time: Res<Time>,
    terrain: Res<Terrain>,
    mut guards: Query<(&mut Guard, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let turn = 1.0 - (-TURN_RATE * dt).exp();
    for (mut guard, mut transform) in &mut guards {
        let at = transform.translation.xz();
        let facing = if guard.walking() {
            let target = guard.beat[guard.toward];
            let to = target - at;
            let step = WALK_SPEED * dt;
            let next = if to.length() <= step {
                guard.rest = guard.rests[guard.toward];
                guard.toward = 1 - guard.toward;
                target
            } else {
                at + to.normalize() * step
            };
            transform.translation = next.extend(terrain.mesh_height_at(next)).xzy();
            to
        } else {
            guard.rest -= dt;
            guard.watch
        };
        if let Some(direction) = facing.try_normalize() {
            transform.rotation = transform.rotation.slerp(heading(direction), turn);
        }
    }
}

/// Cross-fades each guard between standing and walking.
pub(super) fn animate_guards(
    animations: Res<GuardAnimations>,
    guards: Query<(&Guard, &GuardModel)>,
    mut animators: Query<(
        &mut GuardAnimator,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
    )>,
) {
    for (mut animator, mut animation_player, mut transitions) in &mut animators {
        let Ok((guard, model)) = guards.get(animator.guard) else {
            continue;
        };
        let walking = guard.walking();
        if walking == animator.walking {
            continue;
        }
        let Some((_, [idle, walk])) = animations.0.get(model.0) else {
            continue;
        };
        let node = if walking { *walk } else { *idle };
        transitions
            .play(&mut animation_player, node, BLEND)
            .set_speed(if walking { WALK_CLIP_SPEED } else { 1.0 })
            .repeat();
        animator.walking = walking;
    }
}

#[cfg(test)]
mod tests {
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::world::cordon::{in_zone, near_wall_or_checkpoint};
    use crate::world::sites;

    #[test]
    fn two_guards_watch_each_way_through_a_cordon_and_sentries_the_checkpoints() {
        let all: Vec<_> = guards().collect();
        assert_eq!(all.len(), 9);
        // The two posts' four guards come first, then the sentries.
        for (checkpoint, count) in [(PERIMETER_CHECKPOINT, 3), (CHECKPOINT, 2)] {
            let near = all[4..]
                .iter()
                .filter(|guard| guard.beat[0].distance(checkpoint) < 12.0)
                .count();
            assert_eq!(near, count, "sentries at {checkpoint}");
        }
        for (opening, half) in [YARD_GAP, (PERIMETER_GATE, PERIMETER_GATE_HALF)] {
            let near: Vec<_> = all[..4]
                .iter()
                .filter(|guard| {
                    guard.beat[0].distance(opening) < half + BEAT_START + BEAT_OUT + 1.0
                })
                .collect();
            assert_eq!(near.len(), 2, "guards at {opening}");
            // One either side, out of the opening itself.
            assert!(near[0].watch.dot(near[1].watch) < 0.0);
        }
    }

    #[test]
    fn beats_lie_on_dry_open_ground_outside_the_zone() {
        let terrain = Terrain::new();
        for guard in guards() {
            for i in 0..=10 {
                let at = guard.beat[0].lerp(guard.beat[1], i as f32 / 10.0);
                assert!(!in_zone(at), "guard in the Zone at {at}");
                assert!(!near_wall_or_checkpoint(at, 0.5), "guard in a wall at {at}");
                assert!(!sites::clear_footprint(at, 0.5), "guard on a site at {at}");
                assert_eq!(terrain.water_depth_at(at.x, at.y), 0.0, "wet beat at {at}");
            }
        }
    }

    #[test]
    fn a_guard_paces_its_beat_and_rests_at_the_ends() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Terrain::new())
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )))
            .add_systems(Update, patrol_guards);
        let [mut guard, _] = post(YARD_GAP.0, YARD_GAP.1, Vec2::X, Vec2::Y);
        let start = guard.beat[0];
        guard.rest = 0.0;
        let id = app
            .world_mut()
            .spawn((guard, Transform::from_translation(start.extend(0.0).xzy())))
            .id();
        let mut rested = false;
        let mut farthest = 0.0_f32;
        for _ in 0..1200 {
            app.update();
            let world = app.world();
            let at = world.get::<Transform>(id).expect("guard").translation.xz();
            farthest = farthest.max(at.distance(start));
            rested |= !world.get::<Guard>(id).expect("guard").walking();
        }
        assert!(rested);
        assert!((farthest - BEAT_LENGTH).abs() < 0.1, "walked {farthest} m");
    }
}
