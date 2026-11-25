//! The International Institute's lights, lit like the hangar: the hangar's
//! pendant fixture (`LampKit`) on brackets along its front and back façades and
//! under its two porch canopies, and a floodlight over each roof helipad. They
//! burn day and night, as the hangar's do, and cast no shadows.

use bevy::prelude::*;

use super::WorldRoot;
use super::base::{LampKit, PENDANT_POWER};
use super::sites::{institute_helipads, institute_pose};

/// Façade lamps in the scan's model units (46 m each): x along the façade and
/// the façade's depth there at `FACADE_HEIGHT`, measured from
/// `models/landmarks/uhd/institute.glb`. The scan stands a little skewed, so the
/// depth runs with x; the porches (|x| < 0.08) have lamps of their own.
const FRONT_LAMPS: [Vec2; 4] = [
    Vec2::new(-0.27, 0.132),
    Vec2::new(-0.17, 0.145),
    Vec2::new(0.17, 0.186),
    Vec2::new(0.27, 0.198),
];
const BACK_LAMPS: [Vec2; 4] = [
    Vec2::new(-0.27, -0.204),
    Vec2::new(-0.17, -0.192),
    Vec2::new(0.17, -0.157),
    Vec2::new(0.27, -0.150),
];
/// The façade lamps' height (4.6 m) and how far out from the wall they hang.
const FACADE_HEIGHT: f32 = 0.10;
const STANDOFF: f32 = 0.012;
/// Lamps under the front and back porch canopies, whose undersides are at
/// 0.072 (open below), in model units.
const PORCH_LAMPS: [Vec3; 4] = [
    Vec3::new(-0.04, 0.066, 0.215),
    Vec3::new(0.04, 0.066, 0.215),
    Vec3::new(-0.04, 0.066, -0.215),
    Vec3::new(0.04, 0.066, -0.215),
];
/// How far ahead of the wall a façade lamp aims, in meters.
const FACADE_AIM: f32 = 4.0;
/// A helipad floodlight's height over its pad. It has no fixture, as the
/// helicopters rise straight through it.
const PAD_FLOOD_HEIGHT: f32 = 14.0;
const WARM: Color = Color::srgb(1.0, 0.90, 0.78);

pub(super) fn spawn_institute_lights(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let kit = LampKit::new(&mut meshes, &mut materials);
    let root = commands
        .spawn((
            WorldRoot,
            Name::new("Institute lights"),
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    let pose = institute_pose();
    // The level pad the building stands on (its datum is its floor).
    let floor = pose.translation.y;
    for (lamps, out) in [(FRONT_LAMPS, 1.0), (BACK_LAMPS, -1.0)] {
        // The façade's outward normal follows its skew.
        let (first, last) = (lamps[0], lamps[lamps.len() - 1]);
        let along = Vec3::new(last.x - first.x, 0.0, last.y - first.y);
        let normal = (pose.rotation * along.cross(Vec3::Y * out)).normalize();
        for at in lamps {
            let wall = pose.transform_point(Vec3::new(at.x, FACADE_HEIGHT, at.y));
            let lamp = wall + normal * STANDOFF * pose.scale.z;
            let target = lamp.with_y(floor) + normal * FACADE_AIM;
            spawn_lamp(&mut commands, &kit, root, lamp, target, Some(wall));
        }
    }
    for at in PORCH_LAMPS {
        let lamp = pose.transform_point(at);
        spawn_lamp(&mut commands, &kit, root, lamp, lamp.with_y(floor), None);
    }
    for pad in institute_helipads() {
        let height = PAD_FLOOD_HEIGHT;
        commands.spawn((
            ChildOf(root),
            SpotLight {
                color: WARM,
                intensity: PENDANT_POWER * height * height,
                range: height + 9.0,
                radius: 0.5,
                outer_angle: 0.62,
                inner_angle: 0.3,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(pad + Vec3::Y * height).looking_to(Vec3::NEG_Y, Vec3::Z),
        ));
    }
}

/// One fixture at `lamp`, its light aimed at `target` on the floor, on a
/// bracket back to `wall` when it hangs from one.
fn spawn_lamp(
    commands: &mut Commands,
    kit: &LampKit,
    root: Entity,
    lamp: Vec3,
    target: Vec3,
    wall: Option<Vec3>,
) {
    let height = lamp.y - target.y;
    // Porch lamps aim straight down; they face the way the porch does.
    let facing = (target - lamp).with_y(0.0).normalize_or(Vec3::Z);
    let fixture = Transform::from_translation(lamp).looking_to(facing, Vec3::Y);
    commands.spawn((
        ChildOf(root),
        Mesh3d(kit.housing.clone()),
        MeshMaterial3d(kit.steel.clone()),
        fixture,
    ));
    commands.spawn((
        ChildOf(root),
        Mesh3d(kit.panel.clone()),
        MeshMaterial3d(kit.lens.clone()),
        fixture.with_translation(lamp - Vec3::Y * LampKit::LENS_DROP),
    ));
    if let Some(wall) = wall {
        let span = lamp - wall;
        commands.spawn((
            ChildOf(root),
            Mesh3d(kit.housing.clone()),
            MeshMaterial3d(kit.steel.clone()),
            Transform::from_translation(wall + span * 0.5)
                .looking_to(span.normalize_or(Vec3::Z), Vec3::Y)
                .with_scale(Vec3::new(0.08, 0.6, span.length() / 0.37)),
        ));
    }
    commands.spawn((
        ChildOf(root),
        SpotLight {
            color: WARM,
            intensity: PENDANT_POWER * height * height,
            range: height + 9.0,
            radius: 0.34,
            outer_angle: 0.64,
            inner_angle: 0.28,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(lamp - Vec3::Y * LampKit::LIGHT_DROP)
            .looking_at(target, facing),
    ));
}
