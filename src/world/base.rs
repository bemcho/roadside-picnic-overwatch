//! The stalkers' hangar, furniture, parked vehicle, and interior lighting.

mod artifacts;
mod cutaway;
mod props;

use std::sync::LazyLock;

use bevy::gltf::GltfMaterial;
use bevy::light::{EnvironmentMapLight, LightProbe};
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::{self, BASE_HANGAR};
use crate::player::vehicle::{parked_vehicle, spawn_vehicle};

use super::WorldRoot;
use super::collision::{Collider, Pillar};
use super::terrain::{BASE, Terrain};

pub(crate) use artifacts::ArtifactDisplay;
pub(super) use artifacts::{animate_artifacts, stream_full_artifacts, uhd_files};
pub(super) use cutaway::{CutawayMaterial, update_cutaway};

use cutaway::{Cutaway, CutawayShell, RoofFixture};

const MODEL_SCALE: f32 = 38.0;
const FLOOR_DATUM: f32 = 0.0217;
const DOOR_Z: f32 = 0.375;
const DOOR_HALF_WIDTH: f32 = 0.294;
const HALL_PROBE_INTENSITY: f32 = 8_000.0;
const PENDANT_POWER: f32 = 4.9e5;

/// The hangar's root, posed on its pad.
#[derive(Component)]
pub(in crate::world) struct Hall;

struct YardVehicle {
    name: &'static str,
    path: &'static str,
    center: Vec2,
    scale: f32,
    yaw: f32,
    pillars: &'static [Pillar],
}

#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
const YARD_VEHICLES: &[YardVehicle] = &[
    YardVehicle {
        name: "Parked freight train",
        path: assets::VEHICLE_TRAIN,
        center: Vec2::new(BASE.0 - 20.0, BASE.1 - 34.0),
        scale: 40.0,
        yaw: 0.0,
        pillars: include!("pillars/vehicle_train.rs"),
    },
    YardVehicle {
        name: "Tracked crawler",
        path: assets::VEHICLE_CRAWLER,
        center: Vec2::new(BASE.0 + 15.0, BASE.1 - 34.0),
        scale: 8.0,
        yaw: 0.03,
        pillars: include!("pillars/vehicle_crawler.rs"),
    },
    YardVehicle {
        name: "Quarry vehicle",
        path: assets::VEHICLE_QUARRY,
        center: Vec2::new(BASE.0 + 29.0, BASE.1 - 34.0),
        scale: 9.0,
        yaw: -0.05,
        pillars: include!("pillars/vehicle_quarry.rs"),
    },
    YardVehicle {
        name: "Sedan",
        path: assets::VEHICLE_SEDAN,
        center: Vec2::new(BASE.0 + 42.0, BASE.1 - 34.0),
        scale: 6.0,
        yaw: -0.08,
        pillars: include!("pillars/vehicle_sedan.rs"),
    },
];

/// Inner and outer steel faces measured from the GLB in model units.
static WALLS: LazyLock<Vec<Pillar>> = LazyLock::new(|| {
    let mut walls = Vec::new();
    let mut run = |start: Vec2, end: Vec2| {
        let length = start.distance(end) * MODEL_SCALE;
        let steps = (length / 0.38).ceil() as u32;
        for i in 0..=steps {
            let p = start.lerp(end, i as f32 / steps as f32) * MODEL_SCALE;
            walls.push(Pillar {
                x: p.x,
                z: p.y,
                radius: 0.32,
                bottom: -0.5,
                top: 11.0,
            });
        }
    };
    const INNER: [(f32, f32); 7] = [
        (0.302, -0.476),
        (0.323, -0.30),
        (0.350, -0.15),
        (0.375, 0.0),
        (0.421, 0.15),
        (0.434, 0.30),
        (0.425, DOOR_Z),
    ];
    const OUTER: [(f32, f32); 5] = [
        (0.402, -0.479),
        (0.418, -0.30),
        (0.425, 0.0),
        (0.444, 0.30),
        (0.445, DOOR_Z),
    ];
    for sign in [-1.0, 1.0] {
        for face in [&INNER[..], &OUTER[..]] {
            for &[a, b] in face.array_windows() {
                run(Vec2::new(sign * a.0, a.1), Vec2::new(sign * b.0, b.1));
            }
        }
        run(
            Vec2::new(sign * DOOR_HALF_WIDTH, DOOR_Z),
            Vec2::new(sign * 0.445, DOOR_Z),
        );
    }
    run(Vec2::new(-0.405, -0.476), Vec2::new(0.405, -0.476));
    walls
});

pub(super) fn spawn_base(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    paints: Res<Assets<GltfMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    (mut materials, mut shells): (
        ResMut<Assets<StandardMaterial>>,
        ResMut<Assets<CutawayMaterial>>,
    ),
    mut images: ResMut<Assets<Image>>,
) {
    let pad = Vec3::new(BASE.0, terrain.height_at(BASE.0, BASE.1), BASE.1);
    let (shell_mesh, shell_paint) = assets::load_single_primitive(&server, BASE_HANGAR);
    let shell = shells.add(CutawayMaterial {
        base: standard_material_from_gltf_material(
            paints
                .get(&shell_paint)
                .expect("the hangar finishes preloading before the Zone opens"),
        ),
        extension: Cutaway::new(&server, pad),
    });
    let hall = commands
        .spawn((
            Hall,
            CutawayShell(shell.clone()),
            WorldRoot,
            Transform::from_translation(pad),
            Visibility::default(),
            Collider(WALLS.as_slice()),
        ))
        .id();
    commands.spawn((
        Mesh3d(shell_mesh),
        MeshMaterial3d(shell),
        Transform::from_xyz(0.0, -FLOOR_DATUM * MODEL_SCALE, 0.0)
            .with_scale(Vec3::splat(MODEL_SCALE)),
        ChildOf(hall),
    ));
    let vehicle = spawn_vehicle(
        &mut commands,
        &server,
        parked_vehicle(pad),
        &mut meshes,
        &mut materials,
    );
    commands.entity(vehicle).insert(WorldRoot);
    spawn_yard_vehicles(&mut commands, &server, &terrain);
    props::spawn_hangar_props(&mut commands, hall, &server, &mut meshes, &mut materials);
    artifacts::spawn_artifact_showcase(
        &mut commands,
        hall,
        &server,
        &paints,
        &mut meshes,
        &mut materials,
    );
    spawn_hall_probe(&mut commands, hall, &mut images);
    spawn_lamps(&mut commands, hall, &mut meshes, &mut materials);
}

fn spawn_yard_vehicles(commands: &mut Commands, server: &AssetServer, terrain: &Terrain) {
    for vehicle in YARD_VEHICLES {
        commands.spawn((
            WorldRoot,
            Name::new(vehicle.name),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(vehicle.path))),
            yard_vehicle_pose(terrain, vehicle),
            Collider(vehicle.pillars),
        ));
    }
}

fn yard_vehicle_pose(terrain: &Terrain, vehicle: &YardVehicle) -> Transform {
    Transform::from_xyz(
        vehicle.center.x,
        terrain.mesh_height_at(vehicle.center),
        vehicle.center.y,
    )
    .with_rotation(Quat::from_rotation_y(vehicle.yaw))
    .with_scale(Vec3::splat(vehicle.scale))
}

fn spawn_hall_probe(commands: &mut Commands, hall: Entity, images: &mut Assets<Image>) {
    commands.spawn((
        LightProbe {
            falloff: Vec3::new(0.04, 0.14, 0.08),
        },
        EnvironmentMapLight::hemispherical_gradient(
            images,
            Color::linear_rgb(0.79, 0.87, 1.0),
            Color::linear_rgb(0.62, 0.68, 0.76),
            Color::linear_rgb(0.25, 0.26, 0.30),
        )
        .with_intensity(HALL_PROBE_INTENSITY),
        Transform::from_xyz(0.0, 8.0, -2.0).with_scale(Vec3::new(29.0, 17.0, 33.0)),
        ChildOf(hall),
    ));
}

fn spawn_lamps(
    commands: &mut Commands,
    hall: Entity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let steel = materials.add(StandardMaterial {
        base_color: Color::srgb(0.20, 0.21, 0.22),
        metallic: 0.58,
        perceptual_roughness: 0.67,
        ..default()
    });
    let lens = materials.add(StandardMaterial {
        emissive: LinearRgba::rgb(1.0, 0.88, 0.72) * 42_000.0,
        ..default()
    });
    let housing = meshes.add(Cuboid::new(0.98, 0.14, 0.37));
    let panel = meshes.add(Cuboid::new(0.83, 0.024, 0.27));
    for (x, z, roof) in [
        (0.0, 9.0, 20.60),
        (-8.6, 6.0, 18.00),
        (8.6, 6.0, 18.00),
        (-8.2, -9.6, 14.13),
        (8.2, -9.6, 14.13),
        (0.0, -13.0, 15.88),
    ] {
        let y = roof * 0.68;
        let lamp = Vec3::new(x, y, z);
        commands.spawn((
            RoofFixture,
            Mesh3d(meshes.add(Cylinder::new(0.026, roof - y))),
            MeshMaterial3d(steel.clone()),
            Transform::from_xyz(x, (y + roof) * 0.5, z),
            ChildOf(hall),
        ));
        commands.spawn((
            RoofFixture,
            Mesh3d(housing.clone()),
            MeshMaterial3d(steel.clone()),
            Transform::from_translation(lamp),
            ChildOf(hall),
        ));
        commands.spawn((
            RoofFixture,
            Mesh3d(panel.clone()),
            MeshMaterial3d(lens.clone()),
            Transform::from_translation(lamp - Vec3::Y * 0.072),
            ChildOf(hall),
        ));
        commands.spawn((
            SpotLight {
                color: if z < 0.0 {
                    Color::srgb(1.0, 0.84, 0.67)
                } else {
                    Color::srgb(1.0, 0.95, 0.86)
                },
                intensity: PENDANT_POWER * y * y,
                range: y + 9.0,
                radius: 0.34,
                outer_angle: 0.64,
                inner_angle: 0.28,
                shadow_maps_enabled: z == 9.0,
                ..default()
            },
            Transform::from_translation(lamp - Vec3::Y * 0.12).looking_to(Vec3::NEG_Y, Vec3::Z),
            ChildOf(hall),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::vehicle::{blocked, vehicle_distance};
    use crate::world::cordon::inside_yard;
    use crate::world::{Body, SPAWN, push_out};

    #[test]
    fn imported_walls_block_both_approaches_and_leave_the_door_open() {
        let walls = Collider(WALLS.as_slice());
        let at = GlobalTransform::IDENTITY;
        for height in [0.5, 1.8] {
            let body = Body {
                radius: 0.35,
                height,
            };
            let push = |p: Vec2| push_out(p * MODEL_SCALE, 0.0, body, [(&walls, &at)]);
            for p in [
                Vec2::new(0.375, 0.0),
                Vec2::new(-0.425, 0.0),
                Vec2::new(0.0, -0.476),
                Vec2::new(0.32, DOOR_Z),
            ] {
                assert_ne!(push(p), p * MODEL_SCALE, "wall clear at {p}");
            }
            for p in [Vec2::new(0.0, DOOR_Z), Vec2::new(0.25, DOOR_Z)] {
                assert_eq!(push(p), p * MODEL_SCALE, "door blocked at {p}");
            }
        }
    }

    #[test]
    fn supplied_vehicles_are_grounded_behind_hangar_without_blocking_its_gate() {
        let terrain = Terrain::new();
        let back = BASE.1 - 0.476 * MODEL_SCALE;
        let extents = [
            Vec2::new(0.50, 0.11),
            Vec2::new(0.50, 0.391),
            Vec2::new(0.50, 0.221),
            Vec2::new(0.50, 0.221),
        ];
        for (i, (vehicle, extent)) in YARD_VEHICLES.iter().zip(extents).enumerate() {
            let pose = yard_vehicle_pose(&terrain, vehicle);
            assert!(inside_yard(vehicle.center));
            assert!(vehicle.center.y < back - 12.0);
            assert!(!vehicle.pillars.is_empty());
            let origin = terrain.mesh_height_at(vehicle.center);
            for x in [-extent.x, extent.x] {
                for z in [-extent.y, extent.y] {
                    let point = pose.transform_point(Vec3::new(x, 0.0, z));
                    assert!(inside_yard(point.xz()));
                    assert!(point.z < back - 3.0);
                    assert!(
                        (terrain.mesh_height_at(point.xz()) - origin).abs() < 0.05,
                        "{:?} slopes under {}",
                        point.xz(),
                        vehicle.name
                    );
                }
            }
            for (other, other_extent) in YARD_VEHICLES[..i].iter().zip(extents[..i].iter().copied())
            {
                assert!(
                    (other.center - vehicle.center).abs().x
                        > other.scale * other_extent.x + vehicle.scale * extent.x + 2.0
                );
            }
        }
        assert!(
            YARD_VEHICLES
                .iter()
                .all(|vehicle| vehicle.center.distance(Vec2::new(BASE.0, BASE.1 + 60.0)) > 50.0)
        );
    }

    #[test]
    fn player_spawn_and_vehicle_route_clear_the_furnished_hangar() {
        let fixtures: Vec<_> = [(Collider(WALLS.as_slice()), GlobalTransform::IDENTITY)]
            .into_iter()
            .chain(props::colliders())
            .chain(artifacts::colliders())
            .collect();
        let obstacles = || fixtures.iter().map(|(collider, pose)| (collider, pose));
        let mut vehicle = parked_vehicle(Vec3::ZERO);
        let spawn = Vec2::from(SPAWN) - Vec2::from(BASE);
        assert!(DOOR_Z * MODEL_SCALE - spawn.y > 5.0);
        let start = Vec3::new(spawn.x, 0.0, spawn.y);
        assert!(vehicle_distance(&vehicle, start) > 0.35);
        let body = Body {
            radius: 0.35,
            height: 1.8,
        };
        let mut walk = spawn;
        while walk.y <= DOOR_Z * MODEL_SCALE + 1.0 {
            assert_eq!(push_out(walk, 0.0, body, obstacles()), walk);
            walk.y += 0.2;
        }
        let forward = vehicle.rotation * Vec3::Z;
        while vehicle.translation.z <= MODEL_SCALE * 0.4932 + 3.0 {
            assert!(
                !blocked(&vehicle, obstacles()),
                "blocked at {}",
                vehicle.translation
            );
            vehicle.translation += forward * 0.2;
        }
    }
}
