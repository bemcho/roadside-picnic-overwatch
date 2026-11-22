//! Skeletal animation of the stalker: Crawl, Idle, Walk, Run, and Jump clips
//! baked into the rigged model (see `tools/rig_stalker.py`), cross-faded by
//! gait. Crawling also pitches the whole body face-down onto the ground. A rig
//! built with coat bones (`--coat`) keeps them out of the clips: the skirt
//! swings with the stalker's motion and the wind. The soldier rig has none.

use std::f32::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::{PreloadedAssets, STALKER_MODEL};
use crate::world::Wind;

/// How the stalker is moving; selects the animation clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Gait {
    #[default]
    Idle,
    Walk,
    Run,
    Jump,
    /// Crawling along the ground.
    Crawl,
    /// Lying still, ready to crawl.
    Prone,
    /// Scrambling up a solid: the crawl clip, upright.
    Climb,
}

impl Gait {
    fn is_prone(self) -> bool {
        matches!(self, Self::Crawl | Self::Prone)
    }

    /// The clip showing this gait; lying still holds the crawl pose, and
    /// climbing plays the crawl standing up.
    fn clip(self) -> Self {
        match self {
            Self::Prone | Self::Climb => Self::Crawl,
            gait => gait,
        }
    }
}

/// Clip order in the model (the Blender exporter sorts actions by name).
const CLIPS: [Gait; 5] = [Gait::Crawl, Gait::Idle, Gait::Jump, Gait::Run, Gait::Walk];

/// Forward pitch of the body when prone, short of flat so the crawl pose's
/// knees and elbows don't dig into the ground.
const PRONE_PITCH: f32 = 82.0_f32.to_radians();

/// How the prone body sits relative to the player's origin, in meters: raised
/// off the ground by its chest depth and pulled back so the hips, not the
/// feet, sit over the origin.
const PRONE_LIFT: f32 = 0.22;
const PRONE_PULL_BACK: f32 = 0.85;

/// How quickly the body drops prone or gets up, per second.
const PITCH_RATE: f32 = 7.0;

/// Cross-fade time between clips.
const BLEND: Duration = Duration::from_millis(180);

/// Coat swing, in radians, per m/s of air against the stalker.
const COAT_DRAG: f32 = 0.06;
/// The coat swings at most this far from hanging straight, in radians.
const COAT_LIMIT: f32 = 0.6;
/// How quickly the coat follows the air, per second.
const COAT_RATE: f32 = 6.0;

/// The stalker's animation graph and the node playing each [`Gait`].
#[derive(Resource)]
pub(super) struct StalkerAnimations {
    graph: Handle<AnimationGraph>,
    nodes: Vec<(Gait, AnimationNodeIndex)>,
}

impl StalkerAnimations {
    fn node(&self, gait: Gait) -> Option<AnimationNodeIndex> {
        self.nodes
            .iter()
            .find_map(|&(g, node)| (g == gait).then_some(node))
    }
}

/// Points from the player's model child to the gait driving it.
#[derive(Component)]
pub(super) struct StalkerModel {
    player: Entity,
    /// Current forward pitch of the body, in radians.
    pitch: f32,
}

impl StalkerModel {
    pub fn new(player: Entity) -> Self {
        Self { player, pitch: 0.0 }
    }
}

/// One link of a trench-coat chain. Its bone points down to the hem: turning
/// about local +X swings the hem back, about local +Z to the stalker's right.
#[derive(Component)]
pub(super) struct CoatBone {
    player: Entity,
    rest: Quat,
    /// How much of the swing this link takes; the lower link adds to the upper.
    reach: f32,
    phase: f32,
    /// Current swing: back (+x) or forward, right (+y) or left.
    swing: Vec2,
}

/// The animation player inside the stalker's model, and the gait it shows.
#[derive(Component)]
pub(super) struct StalkerAnimator {
    player: Entity,
    shown: Gait,
}

pub(super) fn load_stalker_animations(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut preloaded: ResMut<PreloadedAssets>,
) {
    let clips: [_; CLIPS.len()] = std::array::from_fn(|i| {
        asset_server.load::<AnimationClip>(GltfAssetLabel::Animation(i).from_asset(STALKER_MODEL))
    });
    preloaded
        .0
        .extend(clips.iter().cloned().map(Handle::untyped));
    let (graph, nodes) = AnimationGraph::from_clips(clips);
    commands.insert_resource(StalkerAnimations {
        graph: graphs.add(graph),
        nodes: CLIPS.into_iter().zip(nodes).collect(),
    });
}

/// Hooks the animation graph up to the model's `AnimationPlayer` once the
/// glTF scene has spawned, starting in [`Gait::Idle`].
#[allow(
    clippy::too_many_arguments,
    reason = "One observer wires up the whole rig"
)]
pub(super) fn attach_stalker_animator(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    animations: Res<StalkerAnimations>,
    models: Query<&StalkerModel>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok(model) = models.get(ready.entity) else {
        return;
    };
    for entity in children.iter_descendants(ready.entity) {
        let (Ok(name), Ok(rest)) = (names.get(entity), transforms.get(entity)) else {
            continue;
        };
        if let Some(link) = name.as_str().strip_prefix("Coat") {
            commands.entity(entity).insert(CoatBone {
                player: model.player,
                rest: rest.rotation,
                reach: if link.ends_with(".2") { 0.6 } else { 1.0 },
                phase: link.len() as f32 * 1.3,
                swing: Vec2::ZERO,
            });
        }
    }
    for entity in children.iter_descendants(ready.entity) {
        let Ok(mut animation_player) = players.get_mut(entity) else {
            continue;
        };
        let mut transitions = AnimationTransitions::new();
        if let Some(idle) = animations.node(Gait::Idle) {
            transitions
                .play(&mut animation_player, idle, Duration::ZERO)
                .repeat();
        }
        commands.entity(entity).insert((
            AnimationGraphHandle(animations.graph.clone()),
            transitions,
            StalkerAnimator {
                player: model.player,
                shown: Gait::Idle,
            },
        ));
    }
}

/// Cross-fades to the clip for the stalker's current gait.
pub(super) fn animate_stalker(
    animations: Res<StalkerAnimations>,
    gaits: Query<&super::Locomotion>,
    mut animators: Query<(
        &mut StalkerAnimator,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
    )>,
) {
    for (mut animator, mut animation_player, mut transitions) in &mut animators {
        let Ok(locomotion) = gaits.get(animator.player) else {
            continue;
        };
        let gait = locomotion.gait;
        if gait == animator.shown {
            continue;
        }
        let Some(node) = animations.node(gait.clip()) else {
            continue;
        };
        let clip = if gait.clip() == animator.shown.clip() {
            animation_player.animation_mut(node)
        } else {
            Some(transitions.play(&mut animation_player, node, BLEND))
        };
        if let Some(clip) = clip {
            // The jump clip tucks once and holds the pose until landing.
            if gait != Gait::Jump {
                clip.repeat();
            }
            // Lying still freezes the crawl mid-stroke.
            clip.set_speed(if gait == Gait::Prone { 0.0 } else { 1.0 });
        }
        animator.shown = gait;
    }
}

/// Tips the body face-down while crawling and stands it back up otherwise.
pub(super) fn pose_stalker_body(
    time: Res<Time>,
    gaits: Query<&super::Locomotion>,
    mut models: Query<(&mut StalkerModel, &mut Transform)>,
) {
    for (mut model, mut transform) in &mut models {
        let Ok(locomotion) = gaits.get(model.player) else {
            continue;
        };
        let target = if locomotion.gait.is_prone() {
            PRONE_PITCH
        } else {
            0.0
        };
        let blend = 1.0 - (-PITCH_RATE * time.delta_secs()).exp();
        model.pitch += (target - model.pitch) * blend;
        let down = model.pitch / PRONE_PITCH;
        // Pitching about +X tips the head toward +Z, where the model faces.
        transform.rotation = Quat::from_rotation_x(model.pitch);
        transform.translation = Vec3::new(0.0, PRONE_LIFT * down, -PRONE_PULL_BACK * down);
    }
}

/// Swings the coat's skirt back against the air the stalker moves through,
/// sideways in a crosswind, with a flap in each stride and a flutter in gusts.
pub(super) fn swing_coat(
    time: Res<Time>,
    wind: Res<Wind>,
    stalkers: Query<(&Transform, &super::Locomotion)>,
    mut bones: Query<(&mut CoatBone, &mut Transform), Without<super::Locomotion>>,
) {
    let blend = 1.0 - (-COAT_RATE * time.delta_secs()).exp();
    for (mut bone, mut pose) in &mut bones {
        let Ok((body, locomotion)) = stalkers.get(bone.player) else {
            continue;
        };
        let target = if locomotion.gait.is_prone() {
            Vec2::ZERO
        } else {
            let air = wind.velocity - locomotion.planar_velocity;
            // The model faces +Z, with its right hand toward -X.
            let forward = (body.rotation * Vec3::Z).xz();
            let right = (body.rotation * Vec3::NEG_X).xz();
            let speed = locomotion.planar_velocity.length();
            let stride = 0.03 * speed.min(6.0) * (wind.seconds * TAU * 1.6 + bone.phase).sin();
            let flutter = 0.08 * wind.gust * (wind.seconds * 7.0 + bone.phase).sin();
            Vec2::new(-air.dot(forward), air.dot(right)) * COAT_DRAG + Vec2::new(stride, flutter)
        };
        let swing = bone.swing;
        bone.swing = swing.lerp(target.clamp_length_max(COAT_LIMIT), blend);
        let swing = bone.swing * bone.reach;
        pose.rotation = bone.rest * Quat::from_rotation_x(swing.x) * Quat::from_rotation_z(swing.y);
    }
}

pub(super) fn pause_stalker_animation(
    mut players: Query<&mut AnimationPlayer, With<StalkerAnimator>>,
) {
    for mut player in &mut players {
        player.pause_all();
    }
}

pub(super) fn resume_stalker_animation(
    mut players: Query<&mut AnimationPlayer, With<StalkerAnimator>>,
) {
    for mut player in &mut players {
        player.resume_all();
    }
}
