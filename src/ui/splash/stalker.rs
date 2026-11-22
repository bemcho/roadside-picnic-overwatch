use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::STALKER_MODEL;
use crate::states::AppState;

#[derive(Clone, Copy)]
enum Bone {
    Spine,
    Chest,
    Head,
    UpperArm(f32),
    Forearm(f32),
    Hand(f32),
}

#[derive(Component)]
pub(super) struct SplashJoint {
    bone: Bone,
    rest: Quat,
    rig_from_joint: Quat,
}

#[derive(Component)]
pub(super) struct IdleMotion {
    seed: u32,
    gesture: usize,
    direction: f32,
    elapsed: f32,
    duration: f32,
}

impl IdleMotion {
    fn random(&mut self) -> f32 {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / 16_777_216.0
    }

    fn next_gesture(&mut self) {
        self.gesture = (self.gesture + 1 + usize::from(self.random() < 0.5)) % 3;
        self.direction = if self.random() < 0.5 { -1.0 } else { 1.0 };
        self.duration = 1.8 + self.random() * 1.7;
    }

    fn angles(&self, bone: Bone, weight: f32, breath: f32) -> Vec3 {
        let (look, shift, check) = match self.gesture {
            0 => (weight, 0.0, 0.0),
            1 => (0.0, weight, 0.0),
            _ => (0.0, 0.0, weight),
        };
        let dir = self.direction;
        match bone {
            Bone::Spine => Vec3::new(
                3.0 * shift,
                dir * (4.0 * look - 6.0 * check),
                dir * -6.0 * shift,
            ),
            Bone::Chest => Vec3::new(
                breath * 1.2 + 3.5 * check,
                dir * 7.0 * look,
                dir * -2.0 * shift,
            ),
            Bone::Head => Vec3::new(
                16.0 * check - 4.0 * look + breath,
                dir * (30.0 * look - 15.0 * shift + 16.0 * check),
                dir * 5.0 * shift,
            ),
            Bone::UpperArm(side) => {
                let lift = if side == dir { check } else { 0.0 };
                Vec3::new(
                    -29.0 * lift + 2.0 * breath,
                    -side * 8.0 * lift,
                    -side * 8.0 * lift,
                )
            }
            Bone::Forearm(side) => Vec3::new(
                -7.0 - if side == dir { 42.0 * check } else { 0.0 },
                0.0,
                0.0,
            ),
            Bone::Hand(side) if side == dir => Vec3::new(-12.0 * check, side * 16.0 * check, 0.0),
            Bone::Hand(_) => Vec3::ZERO,
        }
    }
}

pub(super) fn spawn_stalker(mut commands: Commands, server: Res<AssetServer>) {
    commands
        .spawn((
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(STALKER_MODEL))),
            Transform::from_rotation(Quat::from_rotation_y(-0.22)),
            DespawnOnExit(AppState::Loading),
        ))
        .observe(bind_joints);
}

fn bind_joints(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    time: Res<Time<Real>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
) {
    for entity in children.iter_descendants(ready.entity) {
        let (Ok(name), Ok(joint)) = (names.get(entity), transforms.get(entity)) else {
            continue;
        };
        let bone = match name.as_str() {
            "spine" => Bone::Spine,
            "chest" => Bone::Chest,
            "head" => Bone::Head,
            "upper_arm.L" => Bone::UpperArm(1.0),
            "upper_arm.R" => Bone::UpperArm(-1.0),
            "forearm.L" => Bone::Forearm(1.0),
            "forearm.R" => Bone::Forearm(-1.0),
            "hand.L" => Bone::Hand(1.0),
            "hand.R" => Bone::Hand(-1.0),
            _ => continue,
        };
        // Express gesture rotations in rig axes, regardless of each joint's local axes.
        let rig_from_joint = parents
            .iter_ancestors(entity)
            .take_while(|&ancestor| ancestor != ready.entity)
            .filter_map(|ancestor| transforms.get(ancestor).ok())
            .fold(joint.rotation, |rotation, parent| {
                parent.rotation * rotation
            });
        commands.entity(entity).insert(SplashJoint {
            bone,
            rest: joint.rotation,
            rig_from_joint,
        });
    }
    let mut motion = IdleMotion {
        seed: time.elapsed().as_nanos() as u32,
        gesture: 2,
        direction: 1.0,
        elapsed: 0.0,
        duration: 0.0,
    };
    motion.next_gesture();
    commands.entity(ready.entity).insert(motion);
}

pub(super) fn move_stalker(
    time: Res<Time>,
    mut motion: Single<&mut IdleMotion>,
    mut joints: Query<(&SplashJoint, &mut Transform)>,
) {
    motion.elapsed += time.delta_secs();
    if motion.elapsed >= motion.duration {
        motion.elapsed = 0.0;
        motion.next_gesture();
    }
    let ramp = (motion.elapsed / 0.52)
        .min((motion.duration - motion.elapsed) / 0.72)
        .clamp(0.0, 1.0);
    let weight = ramp * ramp * (3.0 - 2.0 * ramp);
    let breath = (time.elapsed_secs() * 1.9).sin();
    for (joint, mut transform) in &mut joints {
        let a = motion.angles(joint.bone, weight, breath) * (std::f32::consts::PI / 180.0);
        let turn = Quat::from_euler(EulerRot::YXZ, a.y, a.x, a.z);
        transform.rotation =
            joint.rest * joint.rig_from_joint.inverse() * turn * joint.rig_from_joint;
    }
}
