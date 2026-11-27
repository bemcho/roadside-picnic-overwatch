//! The Golden Sphere: a 1 m mirror-gold ball hovering and turning over a
//! ferrofluid pool on the crater's floor, one clock driving both.

use std::f32::consts::TAU;

use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets;
use crate::states::{AppState, InGamePhase};

use super::collision::{Collider, Pillar, Unclimbable};
use super::sites::{SPHERE_POOL_RADIUS, golden_sphere_spot};
use super::{Terrain, WorldRoot, detail_range};

/// The Sphere's diameter in metres (the user asked for 1 m).
const SPHERE_DIAMETER: f32 = 1.0;
/// The medium model's diameter and the height of its centre above its origin,
/// which lies at its bottom, in model units.
const SOURCE_DIAMETER: f32 = 0.9996;
const SOURCE_CENTRE: f32 = 0.49685;
/// The Sphere's centre above the pool: its middle (a 1.8 m stalker's head, as
/// the user asked), how far it bobs either way and the seconds one bob takes.
const HOVER: f32 = 1.7;
const BOB: f32 = 0.15;
const BOB_SECONDS: f32 = 7.0;
/// Radians a second the Sphere turns, its axis's lean and the seconds its lean
/// takes to circle once.
const SPIN: f32 = 0.3;
const WOBBLE_DEGREES: f32 = 4.0;
const WOBBLE_SECONDS: f32 = 23.0;
/// The field at the top of the bob, against 1 at the bottom.
const WEAKEST_FIELD: f32 = 1.0 / 3.0;
/// How far above the highest floor under it the liquid lies, in metres.
const LIFT: f32 = 0.005;

/// The Sphere's pose and the field it puts on the pool at one moment.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hover {
    /// The Sphere's centre above the pool, in metres.
    pub height: f32,
    /// Its turn about the vertical, in radians.
    pub spin: f32,
    /// The lean of its axis.
    pub tilt: Quat,
    /// The field on the pool: 1 with the Sphere lowest, a third with it highest.
    #[expect(dead_code, reason = "the ferrofluid's spikes read it next")]
    pub field: f32,
}

impl Hover {
    pub fn at(seconds: f32) -> Self {
        let height = HOVER - BOB * (seconds * TAU / BOB_SECONDS).cos();
        // A magnet's pull falls with the cube of distance, scaled so the bob's
        // ends give the full field and a third of it.
        let pull = |height: f32| ((HOVER - BOB) / height).powi(3);
        let weakest = pull(HOVER + BOB);
        let field =
            WEAKEST_FIELD + (1.0 - WEAKEST_FIELD) * (pull(height) - weakest) / (1.0 - weakest);
        let lean = Quat::from_rotation_y(seconds * TAU / WOBBLE_SECONDS) * Vec3::X;
        Self {
            height,
            spin: SPIN * seconds,
            tilt: Quat::from_axis_angle(lean, WOBBLE_DEGREES.to_radians()),
            field,
        }
    }
}

/// Seconds the Sphere has hovered, counted only while the game runs (virtual
/// time runs on through the pause).
#[derive(Resource, Default)]
pub(super) struct HoverClock(pub f32);

/// The Sphere, hovering over the pool whose centre (at the liquid's level) is
/// `pool`.
#[derive(Component)]
pub(super) struct GoldenSphere {
    pub pool: Vec3,
}

/// The ferrofluid pool under the Sphere.
#[derive(Component)]
pub(super) struct FerrofluidPool;

/// The pool's solid: the stalker walks around the pool and cannot pass the
/// Sphere above it.
static POOL_SOLID: [Pillar; 1] = [Pillar {
    x: 0.0,
    z: 0.0,
    radius: SPHERE_POOL_RADIUS,
    bottom: -0.1,
    top: HOVER + BOB + SPHERE_DIAMETER,
}];

pub(super) struct GoldenSpherePlugin;

impl Plugin for GoldenSpherePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverClock>()
            .add_systems(OnEnter(AppState::InGame), spawn_golden_sphere)
            .add_systems(
                Update,
                pose_golden_sphere.run_if(in_state(InGamePhase::Running)),
            );
    }
}

/// The liquid's level over `at`: the highest floor under the pool, so the
/// floor never shows through it.
fn pool_level(terrain: &Terrain, at: Vec2) -> f32 {
    let step = SPHERE_POOL_RADIUS / 8.0;
    (-8..=8)
        .flat_map(|i| (-8..=8).map(move |j| Vec2::new(i as f32, j as f32) * step))
        .filter(|offset| offset.length() <= SPHERE_POOL_RADIUS)
        .map(|offset| terrain.mesh_height_at(at + offset))
        .fold(f32::NEG_INFINITY, f32::max)
        + LIFT
}

fn spawn_golden_sphere(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    paints: Res<Assets<GltfMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let spot = golden_sphere_spot();
    let pool = spot.extend(pool_level(&terrain, spot)).xzy();
    let (mesh, paint) = assets::load_single_primitive(&server, assets::ARTIFACT_PREVIEWS[1]);
    let Some(source) = paints.get(&paint) else {
        warn!("the Golden Sphere's material is not loaded; leaving the Sphere out");
        return;
    };
    let mut gold = standard_material_from_gltf_material(source);
    gold.metallic = 1.0;
    gold.perceptual_roughness = 0.075;
    gold.reflectance = 1.0;
    let hover = Hover::at(0.0);
    commands
        .spawn((
            Name::new("Golden Sphere"),
            GoldenSphere { pool },
            sphere_pose(pool, hover),
            Visibility::default(),
            WorldRoot,
        ))
        .with_child((
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(gold)),
            Transform::from_translation(Vec3::NEG_Y * SOURCE_CENTRE),
            detail_range(300.0, 30.0),
        ));
    let liquid = StandardMaterial {
        base_color: Color::srgb(0.012, 0.012, 0.014),
        perceptual_roughness: 0.06,
        reflectance: 0.5,
        ..default()
    };
    commands.spawn((
        Name::new("Ferrofluid pool"),
        FerrofluidPool,
        Mesh3d(meshes.add(Circle::new(SPHERE_POOL_RADIUS).mesh().resolution(96))),
        MeshMaterial3d(materials.add(liquid)),
        Transform::from_translation(pool).with_rotation(Quat::from_rotation_x(-TAU / 4.0)),
        detail_range(120.0, 20.0),
        Collider(&POOL_SOLID),
        Unclimbable,
        WorldRoot,
    ));
}

/// The Sphere's transform: centred `hover.height` over `pool`, turned and
/// leaning, at its size.
fn sphere_pose(pool: Vec3, hover: Hover) -> Transform {
    Transform::from_translation(pool + Vec3::Y * hover.height)
        .with_rotation(hover.tilt * Quat::from_rotation_y(hover.spin))
        .with_scale(Vec3::splat(SPHERE_DIAMETER / SOURCE_DIAMETER))
}

fn pose_golden_sphere(
    time: Res<Time>,
    mut clock: ResMut<HoverClock>,
    mut spheres: Query<(&GoldenSphere, &mut Transform)>,
) {
    clock.0 += time.delta_secs();
    let hover = Hover::at(clock.0);
    for (sphere, mut transform) in &mut spheres {
        *transform = sphere_pose(sphere.pool, hover);
    }
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;

    fn samples() -> Vec<Hover> {
        (0..700).map(|i| Hover::at(i as f32 * 0.01)).collect()
    }

    #[test]
    fn the_spikes_stand_tallest_when_the_sphere_sinks_lowest() {
        let samples = samples();
        let low = samples
            .iter()
            .min_by(|a, b| a.height.total_cmp(&b.height))
            .expect("samples");
        let high = samples
            .iter()
            .max_by(|a, b| a.height.total_cmp(&b.height))
            .expect("samples");
        assert!((low.height - 1.55).abs() < 0.01, "lowest {}", low.height);
        assert!((high.height - 1.85).abs() < 0.01, "highest {}", high.height);
        assert!((low.field - 1.0).abs() < 0.01, "full field at the bottom");
        assert!((high.field - 1.0 / 3.0).abs() < 0.02, "a third at the top");
    }

    #[test]
    fn the_field_falls_as_the_sphere_rises() {
        let samples = samples();
        for pair in samples.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if b.height > a.height + 1e-5 {
                assert!(
                    b.field < a.field,
                    "rising from {} to {}",
                    a.height,
                    b.height
                );
            }
        }
    }

    #[test]
    fn the_sphere_turns_at_a_steady_rate() {
        assert!((Hover::at(10.0).spin - Hover::at(0.0).spin - 3.0).abs() < 1e-4);
        let tilt = Hover::at(3.0)
            .tilt
            .angle_between(Quat::IDENTITY)
            .to_degrees();
        assert!((tilt - 4.0).abs() < 0.01, "axis tilted {tilt} degrees");
    }

    #[test]
    fn the_sphere_holds_still_while_paused() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .init_state::<AppState>()
            .add_sub_state::<InGamePhase>()
            .init_resource::<HoverClock>()
            .add_systems(
                Update,
                pose_golden_sphere.run_if(in_state(InGamePhase::Running)),
            );
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        let sphere = app
            .world_mut()
            .spawn((GoldenSphere { pool: Vec3::ZERO }, Transform::default()))
            .id();
        app.update();
        app.update();
        let running = *app.world().get::<Transform>(sphere).expect("sphere");
        assert!(
            running.translation.y > 1.5,
            "the Sphere hovers at head height"
        );
        app.world_mut()
            .resource_mut::<NextState<InGamePhase>>()
            .set(InGamePhase::Paused);
        app.update();
        let paused = *app.world().get::<Transform>(sphere).expect("sphere");
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .advance_by(std::time::Duration::from_secs(2));
        app.update();
        assert_eq!(
            *app.world().get::<Transform>(sphere).expect("sphere"),
            paused
        );
        // Resuming carries on from where it stopped, without the paused time.
        let stopped = app.world().resource::<HoverClock>().0;
        app.world_mut()
            .resource_mut::<NextState<InGamePhase>>()
            .set(InGamePhase::Running);
        app.update();
        app.update();
        let resumed = app.world().resource::<HoverClock>().0;
        assert!(
            resumed - stopped < 0.5,
            "jumped from {stopped} s to {resumed} s"
        );
    }
}
