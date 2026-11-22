//! Occasional tree falls near the stalker. Fallen trees are removed from the current run.

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::player::{Player, PlayerCamera};

use super::canopy::CanopyVisual;
use super::scenery::{Scenery, StandingTree};
use super::terrain::{BASE, TILE_SIZE, Terrain, lattice_hash, lerp, smoothstep};

const FALL_TIME: f32 = 1.65;
const SETTLE_TIME: f32 = 0.9;
const SINK_TIME: f32 = 0.55;

#[derive(Resource)]
pub(super) struct TreeBreakClock {
    seed: u32,
    roll: u32,
    remaining: f32,
}

impl TreeBreakClock {
    fn random(&mut self) -> f32 {
        self.roll = self.roll.wrapping_add(1);
        lattice_hash(self.roll as i32, 0, self.seed)
    }

    fn rearm(&mut self) {
        self.remaining = lerp(9.0, 23.0, self.random());
    }
}

pub(super) fn start_break_clock(mut commands: Commands, time: Res<Time<Real>>) {
    let mut clock = TreeBreakClock {
        seed: time.elapsed().as_micros() as u32,
        roll: 0,
        remaining: 0.0,
    };
    clock.rearm();
    commands.insert_resource(clock);
}

#[derive(Message)]
pub(super) struct TreeBreak {
    trunk: Entity,
    tree: StandingTree,
    direction: Vec2,
}

pub(super) fn pick_tree_to_break(
    time: Res<Time>,
    mut clock: ResMut<TreeBreakClock>,
    player: Single<&Transform, With<Player>>,
    camera: Single<&Transform, With<PlayerCamera>>,
    trees: Query<(Entity, &StandingTree)>,
    mut breaks: MessageWriter<TreeBreak>,
) {
    clock.remaining -= time.delta_secs();
    if clock.remaining > 0.0
        || player.translation.xz().distance_squared(Vec2::from(BASE)) < 17.0_f32.powi(2)
    {
        return;
    }
    let candidates: Vec<_> = trees
        .iter()
        .filter(|(_, tree)| {
            let pose = tree.pose();
            let near = pose
                .translation
                .xz()
                .distance_squared(player.translation.xz());
            let sight =
                pose.translation + Vec3::Y * (pose.scale.y * 0.25).min(3.0) - camera.translation;
            (7.0_f32.powi(2)..=52.0_f32.powi(2)).contains(&near)
                && camera.forward().dot(sight.normalize_or_zero()) > 0.85
        })
        .collect();
    if candidates.is_empty() {
        clock.remaining = 1.0;
        return;
    }
    let index = ((clock.random() * candidates.len() as f32) as usize).min(candidates.len() - 1);
    let (trunk, &tree) = candidates[index];
    let at = tree.pose().translation.xz();
    let outside_hangar = at - Vec2::from(BASE);
    let direction = if outside_hangar.length_squared() < 42.0_f32.powi(2) {
        outside_hangar.normalize()
    } else {
        Vec2::from_angle(clock.random() * TAU)
    };
    let direction = Vec2::from_angle(lerp(-0.45, 0.45, clock.random())).rotate(direction);
    breaks.write(TreeBreak {
        trunk,
        tree,
        direction,
    });
    clock.rearm();
}

pub(super) fn break_selected_trees(
    mut commands: Commands,
    mut breaks: MessageReader<TreeBreak>,
    terrain: Res<Terrain>,
    mut scenery: ResMut<Scenery>,
    canopies: Query<(Entity, &Transform), With<CanopyVisual>>,
) {
    for &TreeBreak {
        trunk,
        tree,
        direction,
    } in breaks.read()
    {
        let rest = tree.pose();
        let tile = (rest.translation.xz() / TILE_SIZE).floor().as_ivec2();
        let Some(sites) = scenery.sites.get_mut(&tile) else {
            continue;
        };
        let Some((entity, pose)) = canopies
            .iter()
            .find(|(_, pose)| pose.translation == rest.translation)
        else {
            continue;
        };
        commands
            .entity(entity)
            .remove::<CanopyVisual>()
            .insert(FallingTree::new(&terrain, *pose, rest.scale.y, direction));
        sites.canopies.retain(|other| *other != tree.0);
        commands.entity(trunk).despawn();
        debug!(at = ?rest.translation, "Tree broke");
    }
}

#[derive(Component)]
pub(super) struct FallingTree {
    start: Transform,
    axis: Vec3,
    angle: f32,
    depth: f32,
    age: f32,
}

impl FallingTree {
    fn new(terrain: &Terrain, start: Transform, height: f32, direction: Vec2) -> Self {
        let tip = start.translation.xz() + direction * height;
        let rise = terrain.mesh_height_at(tip) - start.translation.y;
        Self {
            start,
            axis: Vec3::Y.cross(Vec3::new(direction.x, 0.0, direction.y)),
            angle: (rise / height + 0.06).clamp(-0.25, 0.55).acos(),
            depth: height * 0.46 + 0.5,
            age: 0.0,
        }
    }
}

pub(super) fn animate_falling_trees(
    mut commands: Commands,
    time: Res<Time>,
    mut falling: Query<(Entity, &mut Transform, &mut FallingTree)>,
) {
    for (entity, mut pose, mut tree) in &mut falling {
        tree.age += time.delta_secs();
        if tree.age >= FALL_TIME + SETTLE_TIME + SINK_TIME {
            commands.entity(entity).despawn();
            continue;
        }
        let swing = (tree.age / FALL_TIME).min(1.0);
        let snap = smoothstep(0.0, 0.12, tree.age);
        let angle = lerp(0.04 * snap, tree.angle, swing * swing);
        pose.rotation = Quat::from_axis_angle(tree.axis, angle) * tree.start.rotation;
        let sink = smoothstep(
            FALL_TIME + SETTLE_TIME,
            FALL_TIME + SETTLE_TIME + SINK_TIME,
            tree.age,
        );
        pose.translation = tree.start.translation - Vec3::Y * (sink * tree.depth);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::world::canopy::{CanopyModels, ForestTree, Species, spawn_canopies};
    use crate::world::scenery;

    #[test]
    fn a_fallen_tree_falls_and_stays_gone_across_chunk_reload() {
        let tile = crate::world::terrain::base_tile() + IVec2::NEG_Y;
        let terrain = Terrain::new();
        let (mut sites, _, _) = scenery::build_tile(&terrain, tile);
        sites.undergrowth.clear();

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<bevy::world_serialization::WorldAsset>()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )))
            .insert_resource(terrain)
            .init_resource::<Assets<Mesh>>()
            .add_message::<TreeBreak>()
            .add_systems(PreUpdate, scenery::stream_scenery)
            .add_systems(
                Update,
                (animate_falling_trees, break_selected_trees).chain(),
            );

        let world = app.world_mut();
        let start = (tile.as_vec2() + Vec2::splat(0.5)) * TILE_SIZE;
        let player = world
            .spawn((
                Player::default(),
                Transform::from_xyz(start.x, 0.0, start.y),
            ))
            .id();
        let root = world.spawn(Transform::IDENTITY).id();
        world.commands().entity(root).with_children(|objects| {
            spawn_canopies(objects, &CanopyModels::default(), &sites.canopies);
        });
        world.insert_resource(Scenery {
            root,
            sites: [(tile, sites)].into_iter().collect(),
            active: Default::default(),
            centers: Vec::new(),
        });
        app.update();

        let world = app.world_mut();
        let mut trunks = world.query::<(Entity, &StandingTree)>();
        let (trunk, tree) = trunks
            .iter(world)
            .map(|(trunk, &tree)| (trunk, tree))
            .next()
            .expect("an imported tree by the hangar");
        world.write_message(TreeBreak {
            trunk,
            tree,
            direction: Vec2::X,
        });
        app.update();

        let world = app.world_mut();
        assert!(world.get_entity(trunk).is_err());
        let mut falling = world.query::<(&FallingTree, &Transform, Has<CanopyVisual>)>();
        assert_eq!(falling.iter(world).count(), 1);
        assert!(falling.iter(world).all(|(_, _, sways)| !sways));

        for _ in 0..10 {
            app.update();
        }
        assert!(falling.iter(app.world()).all(|(tree, pose, _)| {
            pose.translation == tree.start.translation && pose.rotation != tree.start.rotation
        }));
        for _ in 0..25 {
            app.update();
        }
        assert_eq!(falling.iter(app.world()).count(), 0);

        let world = app.world_mut();
        world
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x += TILE_SIZE * 3.0;
        app.update();
        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x = start.x;
        app.update();
        assert!(
            trunks
                .iter(app.world())
                .all(|(_, other)| other.pose().translation != tree.pose().translation)
        );
    }

    #[test]
    fn only_a_nearby_tree_in_view_is_selected() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs(1));
        app.insert_resource(time)
            .insert_resource(TreeBreakClock {
                seed: 1,
                roll: 0,
                remaining: 0.0,
            })
            .add_message::<TreeBreak>()
            .add_systems(Update, pick_tree_to_break);
        let world = app.world_mut();
        world.spawn((Player::default(), Transform::IDENTITY));
        world.spawn((PlayerCamera, Transform::from_xyz(0.0, 3.0, 0.0)));
        let mut spawn_tree = |z| {
            world
                .spawn(StandingTree(ForestTree {
                    species: Species::Birch,
                    pose: Transform::from_xyz(0.0, 0.0, z).with_scale(Vec3::splat(12.0)),
                }))
                .id()
        };
        spawn_tree(15.0);
        spawn_tree(-2.0);
        spawn_tree(-65.0);
        let visible = spawn_tree(-15.0);
        app.update();
        let messages = app.world().resource::<Messages<TreeBreak>>();
        let mut cursor = messages.get_cursor();
        let picked: Vec<_> = cursor.read(messages).map(|event| event.trunk).collect();
        assert_eq!(picked, [visible]);
        assert!((9.0..=23.0).contains(&app.world().resource::<TreeBreakClock>().remaining));
    }
}
