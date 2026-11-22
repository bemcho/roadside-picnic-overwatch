use std::f32::consts::PI;

use bevy::prelude::*;

use crate::assets;
use crate::world::{detail_range, range_scene_meshes};

use super::{Collider, Pillar};

const SLAB: f32 = 0.03;
const WEST_WALL_INWARD: f32 = 1.415;
const PROP_LIGHT_INTENSITY: f32 = 6.4e6;

struct Prop {
    model: &'static str,
    at: Vec3,
    yaw: f32,
    scale: f32,
    pillars: &'static [Pillar],
}

impl Prop {
    fn pose(&self) -> Transform {
        Transform::from_translation(self.at)
            .with_rotation(Quat::from_rotation_y(self.yaw))
            .with_scale(Vec3::splat(self.scale))
    }

    fn bundle(&self, server: &AssetServer) -> (WorldAssetRoot, Transform, Collider) {
        (
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(self.model))),
            self.pose(),
            Collider(self.pillars),
        )
    }

    fn spotlight(&self) -> (SpotLight, Transform) {
        let pose = self.pose();
        let target = pose.transform_point(Vec3::Y * 0.50);
        let lamp = target + Vec3::Y * 3.1 + pose.rotation * Vec3::Z * 1.95;
        (
            SpotLight {
                color: Color::srgb(1.0, 0.87, 0.71),
                intensity: PROP_LIGHT_INTENSITY,
                range: 5.8,
                inner_angle: 0.16,
                outer_angle: 0.34,
                ..default()
            },
            Transform::from_translation(lamp).looking_at(target, Vec3::Y),
        )
    }
}

const fn column(x: f32, z: f32, radius: f32, bottom: f32, top: f32) -> Pillar {
    Pillar {
        x,
        z,
        radius,
        bottom,
        top,
    }
}

// Coordinates are in the hangar frame; pillar dimensions are in model units.
const FURNITURE: &[Prop] = &[
    Prop {
        model: assets::HANGAR_WORKSHOP,
        at: Vec3::new(-10.98, SLAB, -8.65),
        yaw: WEST_WALL_INWARD,
        scale: 1.78,
        pillars: &[
            column(-0.23, 0.0, 0.17, 0.0, 0.52),
            column(0.0, 0.0, 0.20, 0.0, 0.52),
            column(0.23, 0.0, 0.17, 0.0, 0.52),
            column(-0.23, -0.13, 0.13, 0.73, 0.96),
            column(0.0, -0.13, 0.13, 0.73, 0.96),
            column(0.23, -0.13, 0.13, 0.73, 0.96),
        ],
    },
    Prop {
        model: assets::HANGAR_REPAIR_STAND,
        at: Vec3::new(-8.8, SLAB, -7.3),
        yaw: PI,
        scale: 1.40,
        pillars: &[
            column(-0.24, 0.0, 0.21, 0.0, 0.22),
            column(0.16, 0.0, 0.24, 0.0, 0.34),
            column(0.22, 0.0, 0.12, 0.22, 0.63),
            column(0.36, 0.0, 0.10, 0.60, 0.97),
        ],
    },
    Prop {
        model: assets::HANGAR_MAP_BOARD,
        at: Vec3::new(-4.9, 0.037, 3.8),
        yaw: 1.95,
        scale: 1.95,
        pillars: &[
            column(-0.34, -0.06, 0.09, 0.0, 0.70),
            column(0.34, -0.06, 0.09, 0.0, 0.70),
            column(-0.34, 0.07, 0.14, 0.34, 0.96),
            column(0.0, 0.07, 0.18, 0.34, 0.96),
            column(0.34, 0.07, 0.14, 0.34, 0.96),
        ],
    },
    Prop {
        model: assets::HANGAR_LAB,
        at: Vec3::new(10.95, SLAB, -8.6),
        yaw: -WEST_WALL_INWARD,
        scale: 1.82,
        pillars: &[
            column(-0.26, 0.0, 0.22, 0.0, 0.57),
            column(0.0, 0.0, 0.23, 0.0, 0.57),
            column(0.26, 0.0, 0.22, 0.0, 0.57),
            column(0.29, -0.07, 0.12, 0.51, 0.86),
        ],
    },
    Prop {
        model: assets::HANGAR_TABLE,
        at: Vec3::new(9.9, SLAB, -5.45),
        yaw: -0.17,
        scale: 1.81,
        pillars: &[
            column(0.0, -0.28, 0.21, 0.38, 0.51),
            column(0.0, 0.0, 0.22, 0.38, 0.51),
            column(0.0, 0.28, 0.21, 0.38, 0.51),
            column(-0.16, -0.39, 0.075, 0.0, 0.40),
            column(0.16, -0.39, 0.075, 0.0, 0.40),
            column(-0.16, 0.39, 0.075, 0.0, 0.40),
            column(0.16, 0.39, 0.075, 0.0, 0.40),
        ],
    },
    Prop {
        model: assets::HANGAR_TOILET,
        at: Vec3::new(9.7, SLAB, -16.85),
        yaw: -0.10,
        scale: 0.94,
        pillars: &[
            column(0.0, 0.19, 0.24, 0.0, 0.51),
            column(0.0, -0.10, 0.23, 0.0, 0.58),
            column(0.0, -0.27, 0.18, 0.42, 0.97),
        ],
    },
];

const TOOL_RACK: Prop = Prop {
    model: assets::HANGAR_TOOLS,
    at: Vec3::new(-12.305, 1.02, -11.205),
    yaw: WEST_WALL_INWARD,
    scale: 1.10,
    pillars: &[
        column(-0.41, -0.05, 0.12, -0.04, 0.95),
        column(-0.21, -0.05, 0.12, -0.04, 0.95),
        column(0.0, -0.05, 0.12, -0.04, 0.95),
        column(0.21, -0.05, 0.12, -0.04, 0.95),
        column(0.41, -0.05, 0.12, -0.04, 0.95),
    ],
};

pub(super) fn spawn_hangar_props(
    commands: &mut Commands,
    hall: Entity,
    server: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // The supplied tools are loose meshes; a backing secures them to the wall.
    let panel = meshes.add(Cuboid::new(1.07, 0.99, 0.065));
    let backing = materials.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.19, 0.15),
        perceptual_roughness: 0.90,
        ..default()
    });
    commands.entity(hall).with_children(|interior| {
        for prop in FURNITURE {
            interior
                .spawn((prop.bundle(server), detail_range(75.0, 15.0)))
                .observe(range_scene_meshes);
        }
        interior
            .spawn((TOOL_RACK.bundle(server), detail_range(75.0, 15.0)))
            .observe(range_scene_meshes)
            .with_child((
                Mesh3d(panel),
                MeshMaterial3d(backing),
                Transform::from_xyz(0.0, 0.455, -0.052),
            ));
        for prop in FURNITURE.iter().chain([&TOOL_RACK]) {
            interior.spawn(prop.spotlight());
        }
    });
}

#[cfg(test)]
pub(super) fn colliders() -> impl Iterator<Item = (Collider, GlobalTransform)> {
    FURNITURE
        .iter()
        .chain([&TOOL_RACK])
        .map(|prop| (Collider(prop.pillars), prop.pose().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Body, push_out};

    #[test]
    fn table_blocks_walking_but_leaves_the_space_between_its_legs_open() {
        let table = FURNITURE
            .iter()
            .find(|prop| prop.model == assets::HANGAR_TABLE)
            .expect("the hangar has a table");
        let pose = table.pose();
        let collider = Collider(table.pillars);
        let transform = GlobalTransform::from(pose);
        let push = |at, height| {
            push_out(
                at,
                0.0,
                Body {
                    radius: 0.35,
                    height,
                },
                [(&collider, &transform)],
            )
        };
        let center = pose.translation.xz();
        assert_ne!(push(center, 1.8), center);
        assert_eq!(push(center, 0.5), center);
        let leg = pose.transform_point(Vec3::new(0.16, 0.0, 0.39)).xz();
        assert_ne!(push(leg, 0.5), leg);
    }
}
