use std::f32::consts::TAU;

use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;
use crate::world::collision::{Collider, Pillar};
use crate::world::{Describe, Place, detail_range, range_scene_meshes};

const TABLE_TOP: f32 = 0.84;
const TABLE_HALF: f32 = 0.72;
const ARTIFACT_REST: f32 = 0.97;
const FULL_ARTIFACT_RANGE: f32 = 8.0;
const ARTIFACT_LIGHT_INTENSITY: f32 = 2.3e6;
const TABLE_Z: [f32; 7] = [-13.0, -9.0, -5.0, -1.0, 3.0, 7.0, 11.0];
const TABLE_PILLARS: &[Pillar] = &[Pillar {
    x: 0.0,
    z: 0.0,
    radius: TABLE_HALF,
    bottom: 0.0,
    top: TABLE_TOP,
}];

#[derive(Component)]
pub(crate) struct ArtifactDisplay {
    pub(crate) index: usize,
}

#[derive(Component)]
pub(in crate::world) struct ArtifactPreview {
    index: usize,
}

#[derive(Component)]
pub(in crate::world) struct FullArtifact {
    table: Entity,
    index: usize,
}

struct ArtifactModel {
    name: &'static str,
    path: &'static str,
    scale: f32,
    spin: f32,
    bob: f32,
    tilt: f32,
    light: usize,
}

const ARTIFACT_MODELS: &[ArtifactModel] = &[
    ArtifactModel {
        name: "Death lamp",
        path: assets::ARTIFACTS[0],
        scale: 0.83,
        spin: 0.18,
        bob: 0.045,
        tilt: 0.025,
        light: 0,
    },
    ArtifactModel {
        name: "Golden sphere",
        path: assets::ARTIFACTS[1],
        scale: 0.76,
        spin: 0.34,
        bob: 0.035,
        tilt: 0.012,
        light: 1,
    },
    ArtifactModel {
        name: "Itcher",
        path: assets::ARTIFACTS[2],
        scale: 0.82,
        spin: 0.14,
        bob: 0.055,
        tilt: 0.028,
        light: 2,
    },
    ArtifactModel {
        name: "Lobster eyes",
        path: assets::ARTIFACTS[3],
        scale: 0.76,
        spin: 0.20,
        bob: 0.040,
        tilt: 0.022,
        light: 0,
    },
    ArtifactModel {
        name: "Spinning ring",
        path: assets::ARTIFACTS[4],
        scale: 0.91,
        spin: 0.72,
        bob: 0.045,
        tilt: 0.014,
        light: 2,
    },
    ArtifactModel {
        name: "Sponge",
        path: assets::ARTIFACTS[5],
        scale: 0.88,
        spin: 0.12,
        bob: 0.035,
        tilt: 0.035,
        light: 3,
    },
    ArtifactModel {
        name: "White ring",
        path: assets::ARTIFACTS[6],
        scale: 0.86,
        spin: 0.44,
        bob: 0.040,
        tilt: 0.018,
        light: 2,
    },
    ArtifactModel {
        name: "Wriggling magnet",
        path: assets::ARTIFACTS[7],
        scale: 0.86,
        spin: 0.32,
        bob: 0.052,
        tilt: 0.028,
        light: 0,
    },
    ArtifactModel {
        name: "Blackspark",
        path: assets::ARTIFACTS[8],
        scale: 0.90,
        spin: 0.24,
        bob: 0.050,
        tilt: 0.020,
        light: 0,
    },
    ArtifactModel {
        name: "Bracelet",
        path: assets::ARTIFACTS[9],
        scale: 0.92,
        spin: 0.48,
        bob: 0.030,
        tilt: 0.012,
        light: 1,
    },
    ArtifactModel {
        name: "Empty",
        path: assets::ARTIFACTS[10],
        scale: 0.84,
        spin: 0.16,
        bob: 0.045,
        tilt: 0.030,
        light: 3,
    },
    ArtifactModel {
        name: "Full Empty",
        path: assets::ARTIFACTS[11],
        scale: 0.95,
        spin: 0.20,
        bob: 0.055,
        tilt: 0.025,
        light: 1,
    },
    ArtifactModel {
        name: "Pins and Needles",
        path: assets::ARTIFACTS[12],
        scale: 0.88,
        spin: 0.26,
        bob: 0.040,
        tilt: 0.022,
        light: 2,
    },
    ArtifactModel {
        name: "Soso",
        path: assets::ARTIFACTS[13],
        scale: 0.92,
        spin: 0.15,
        bob: 0.050,
        tilt: 0.020,
        light: 3,
    },
];

#[derive(Component, Clone)]
pub(in crate::world) struct ArtifactIdle {
    rest: Transform,
    phase: f32,
    spin: f32,
    bob: f32,
    tilt: f32,
}

impl ArtifactIdle {
    fn pose_at(&self, seconds: f32) -> Transform {
        let mut pose = self.rest;
        pose.translation.y += (seconds * 1.35 + self.phase).sin() * self.bob;
        let spin = Quat::from_rotation_y(seconds * self.spin);
        let sway = Quat::from_euler(
            EulerRot::XYZ,
            (seconds * 0.71 + self.phase).sin() * self.tilt,
            0.0,
            (seconds * 0.53 + self.phase * 1.7).sin() * self.tilt * 0.7,
        );
        pose.rotation = self.rest.rotation * sway * spin;
        pose
    }
}

pub(super) fn spawn_artifact_showcase(
    commands: &mut Commands,
    hall: Entity,
    server: &AssetServer,
    paints: &Assets<GltfMaterial>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let pedestal = meshes.add(Cuboid::new(0.48, 0.74, 0.48));
    let top = meshes.add(Cuboid::new(1.42, 0.10, 1.42));
    let pedestal_paint = materials.add(StandardMaterial {
        base_color: Color::srgb(0.055, 0.070, 0.075),
        metallic: 0.78,
        perceptual_roughness: 0.38,
        ..default()
    });
    let top_paint = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.14, 0.15),
        metallic: 0.62,
        perceptual_roughness: 0.26,
        emissive: LinearRgba::rgb(0.012, 0.022, 0.026),
        ..default()
    });
    commands.entity(hall).with_children(|hangar| {
        for (index, model) in ARTIFACT_MODELS.iter().enumerate() {
            let center = table_center(index);
            hangar
                .spawn((
                    Name::new(format!("Artifact display: {}", model.name)),
                    Transform::from_translation(center),
                    Visibility::default(),
                    Collider(TABLE_PILLARS),
                    ArtifactDisplay { index },
                    Describe {
                        place: Place::Artifact(index),
                        radius: 0.55,
                        height: 1.5,
                    },
                ))
                .with_children(|table| {
                    table.spawn((
                        Mesh3d(pedestal.clone()),
                        MeshMaterial3d(pedestal_paint.clone()),
                        Transform::from_xyz(0.0, 0.37, 0.0),
                    ));
                    table.spawn((
                        Mesh3d(top.clone()),
                        MeshMaterial3d(top_paint.clone()),
                        Transform::from_xyz(0.0, 0.79, 0.0),
                    ));
                    let rest = Transform::from_xyz(0.0, ARTIFACT_REST, 0.0)
                        .with_rotation(Quat::from_rotation_y(index as f32 * 0.61))
                        .with_scale(Vec3::splat(model.scale));
                    spawn_artifact(table, server, paints, materials, model, rest, index);
                    let target = Vec3::new(0.0, TABLE_TOP + model.scale * 0.48, 0.0);
                    let lamp = target + Vec3::new(0.48, 2.65, 0.68);
                    table.spawn((
                        SpotLight {
                            color: spotlight_color(model.light),
                            intensity: ARTIFACT_LIGHT_INTENSITY,
                            range: 4.7,
                            radius: 0.22,
                            inner_angle: 0.17,
                            outer_angle: 0.48,
                            ..default()
                        },
                        Transform::from_translation(lamp).looking_at(target, Vec3::Y),
                    ));
                });
        }
    });
}

fn spawn_artifact(
    table: &mut ChildSpawnerCommands,
    server: &AssetServer,
    paints: &Assets<GltfMaterial>,
    materials: &mut Assets<StandardMaterial>,
    model: &ArtifactModel,
    rest: Transform,
    index: usize,
) {
    let path = assets::ARTIFACT_PREVIEWS[index];
    let idle = ArtifactIdle {
        rest,
        phase: index as f32 * (TAU / 7.0),
        spin: model.spin,
        bob: model.bob,
        tilt: model.tilt,
    };
    let range = detail_range(90.0, 18.0);
    if index == 1 {
        let (mesh, paint) = assets::load_single_primitive(server, path);
        let source = paints
            .get(&paint)
            .expect("preloaded golden sphere material");
        let mut gold = standard_material_from_gltf_material(source);
        gold.metallic = 1.0;
        gold.perceptual_roughness = 0.075;
        gold.reflectance = 1.0;
        table.spawn((
            Name::new(model.name),
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(gold)),
            rest,
            idle,
            range,
            ArtifactPreview { index },
        ));
    } else {
        let mut artifact = table.spawn((
            Name::new(model.name),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))),
            rest,
            idle,
            range,
        ));
        artifact.insert(ArtifactPreview { index });
        artifact.observe(range_scene_meshes);
    }
}

pub(in crate::world) fn stream_full_artifacts(
    mut commands: Commands,
    player: Query<&Transform, With<crate::player::Player>>,
    displays: Query<(Entity, &ArtifactDisplay, &GlobalTransform)>,
    fulls: Query<(Entity, &FullArtifact)>,
    previews: Query<(Entity, &ArtifactPreview)>,
    server: Res<AssetServer>,
) {
    let selected = player.single().ok().and_then(|player| {
        displays
            .iter()
            .filter_map(|(table, display, pose)| {
                let distance = player.translation.xz().distance(pose.translation().xz());
                (distance <= FULL_ARTIFACT_RANGE).then_some((distance, table, display.index))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    });
    let active = selected.map(|(_, table, index)| (table, index));

    for (entity, full) in &fulls {
        if active.is_none_or(|(table, _)| table != full.table) {
            commands.entity(entity).despawn();
            for (preview, marker) in &previews {
                if marker.index == full.index {
                    commands.entity(preview).insert(Visibility::Inherited);
                }
            }
        }
    }

    let Some((table, index)) = active else {
        return;
    };
    if fulls.iter().any(|(_, full)| full.table == table) {
        return;
    }
    let model = &ARTIFACT_MODELS[index];
    let rest = artifact_rest(model, index);
    commands
        .spawn((
            FullArtifact { table, index },
            ChildOf(table),
            Name::new(model.name),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(model.path))),
            rest,
            artifact_idle(model, rest, index),
            detail_range(90.0, 18.0),
            Visibility::Hidden,
        ))
        .observe(range_scene_meshes)
        .observe(reveal_full_artifact);
}

fn reveal_full_artifact(
    ready: On<WorldInstanceReady>,
    fulls: Query<&FullArtifact>,
    previews: Query<(Entity, &ArtifactPreview)>,
    children: Query<&Children>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let Ok(full) = fulls.get(ready.entity) else {
        return;
    };
    if full.index == 1 {
        for child in children.iter_descendants(ready.entity) {
            let Ok(material) = mesh_materials.get(child) else {
                continue;
            };
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.metallic = 1.0;
                material.perceptual_roughness = 0.075;
                material.reflectance = 1.0;
            }
        }
    }
    commands.entity(ready.entity).insert(Visibility::Inherited);
    for (preview, marker) in &previews {
        if marker.index == full.index {
            commands.entity(preview).insert(Visibility::Hidden);
        }
    }
}

fn artifact_rest(model: &ArtifactModel, index: usize) -> Transform {
    Transform::from_xyz(0.0, ARTIFACT_REST, 0.0)
        .with_rotation(Quat::from_rotation_y(index as f32 * 0.61))
        .with_scale(Vec3::splat(model.scale))
}

fn artifact_idle(model: &ArtifactModel, rest: Transform, index: usize) -> ArtifactIdle {
    ArtifactIdle {
        rest,
        phase: index as f32 * (TAU / 7.0),
        spin: model.spin,
        bob: model.bob,
        tilt: model.tilt,
    }
}

pub(in crate::world) fn animate_artifacts(
    time: Res<Time>,
    mut artifacts: Query<(&ArtifactIdle, &mut Transform)>,
) {
    let seconds = time.elapsed_secs();
    for (idle, mut pose) in &mut artifacts {
        *pose = idle.pose_at(seconds);
    }
}

fn table_center(index: usize) -> Vec3 {
    let x = if index < TABLE_Z.len() { -7.35 } else { 13.0 };
    Vec3::new(x, 0.0, TABLE_Z[index % TABLE_Z.len()])
}

fn spotlight_color(index: usize) -> Color {
    match index {
        0 => Color::srgb(0.58, 0.82, 1.0),
        1 => Color::srgb(1.0, 0.75, 0.36),
        2 => Color::srgb(0.72, 1.0, 0.91),
        _ => Color::srgb(1.0, 0.91, 0.78),
    }
}

#[cfg(test)]
pub(super) fn colliders() -> impl Iterator<Item = (Collider, GlobalTransform)> {
    (0..ARTIFACT_MODELS.len()).map(|index| {
        (
            Collider(TABLE_PILLARS),
            Transform::from_translation(table_center(index)).into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_tables_fill_two_rows_without_entering_the_vehicle_lane() {
        assert_eq!(ARTIFACT_MODELS.len(), assets::ARTIFACTS.len());
        assert_eq!(ARTIFACT_MODELS.len(), assets::ARTIFACT_PREVIEWS.len());
        assert_eq!(ARTIFACT_MODELS.len(), crate::ui::text::ARTIFACT_COPY.len());
        for (index, model) in ARTIFACT_MODELS.iter().enumerate() {
            let center = table_center(index);
            if center.x < 0.0 {
                assert!(center.x.abs() - TABLE_HALF > 6.0);
            } else {
                assert!(center.x - TABLE_HALF > 12.0);
                assert!(center.x + TABLE_HALF < 14.0);
            }
            assert!(center.z.abs() < 14.0);
            assert!(model.scale > 0.0 && model.spin > 0.0 && model.bob > 0.0);
        }
        for index in 0..TABLE_Z.len() {
            let west = table_center(index);
            let east = table_center(index + TABLE_Z.len());
            assert!(west.x < 0.0 && east.x > 0.0);
            assert_eq!(west.z, east.z);
            if index > 0 {
                assert!(west.z - table_center(index - 1).z >= 3.9);
            }
        }
    }

    #[test]
    fn idle_motion_bobs_and_turns_from_a_stable_rest_pose() {
        let rest = Transform::from_xyz(0.0, ARTIFACT_REST, 0.0)
            .with_rotation(Quat::from_rotation_y(0.4))
            .with_scale(Vec3::splat(0.8));
        let idle = ArtifactIdle {
            rest,
            phase: 0.7,
            spin: 0.5,
            bob: 0.04,
            tilt: 0.02,
        };
        let start = idle.pose_at(0.0);
        let later = idle.pose_at(1.0);
        assert_eq!(start.scale, rest.scale);
        assert_eq!(idle.pose_at(2.0).scale, rest.scale);
        assert_ne!(later.translation.y, start.translation.y);
        assert!(later.rotation.angle_between(start.rotation) > 0.1);
    }
}
