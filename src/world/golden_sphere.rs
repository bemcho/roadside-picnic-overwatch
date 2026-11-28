//! The Golden Sphere: a 1 m mirror-gold ball hovering and turning over a
//! ferrofluid pool on the crater's floor, one clock driving both.

mod ferrofluid;
mod mirror;
mod reflection;
mod uhd;

use std::f32::consts::TAU;

use bevy::camera::Exposure;
use bevy::camera::visibility::{NoAutoAabb, RenderLayers};
use bevy::diagnostic::FrameCount;
use bevy::gltf::GltfMaterial;
use bevy::light::NotShadowCaster;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets;
use crate::assets::UhdFiles;
use crate::assets::gpu_scans::{GpuTier, TierLevel, settle_gpu_tier};
use crate::assets::ram_source::start_ram_fill;
use crate::player::vehicle::{Carrier, Driving, Seat};
use crate::player::{Locomotion, Player, PlayerCamera};
use crate::states::{AppState, InGamePhase};

use super::collision::{Collider, Pillar, Unclimbable};
use super::sites::{SPHERE_POOL_RADIUS, golden_sphere_spot};
use super::{Sun, Terrain, WorldRoot, detail_range};
use ferrofluid::{Ferrofluid, FerrofluidMaterial};
use mirror::{
    CAR_PAINT, CARRIER_PAINT, Hull, Mirror, MirrorMaterial, REACH, Stalker, stand_in_light,
    stand_ins,
};
use reflection::{
    ProbeFace, ReflectionProbe, spawn_reflection_probe, steer_reflection_probe, watch,
};
pub(crate) use reflection::{SPHERE_LAYER, SPHERE_SHADOW_LAYER};
use uhd::{SphereSkin, on_exceptional_gpu, stream_uhd_sphere};

/// The Sphere's diameter in metres (the user asked for 1 m).
pub(super) const SPHERE_DIAMETER: f32 = 1.0;
/// The medium model's diameter and the height of its centre above its origin,
/// which lies at its bottom, in model units.
pub(super) const SOURCE_DIAMETER: f32 = 0.9996;
const SOURCE_CENTRE: f32 = 0.49685;
/// The Sphere's centre above the pool: its middle (its underside 1.6 to 1.9 m
/// up, just over the 1.4 m spike the user asked for), how far it bobs either
/// way and the seconds one bob takes.
const HOVER: f32 = 2.25;
const BOB: f32 = 0.15;
const BOB_SECONDS: f32 = 7.0;
/// Radians a second the Sphere turns, its axis's lean and the seconds its lean
/// takes to circle once.
const SPIN: f32 = 0.3;
const WOBBLE_DEGREES: f32 = 4.0;
const WOBBLE_SECONDS: f32 = 23.0;
/// The field at the top of the bob, against 1 at the bottom.
pub(super) const WEAKEST_FIELD: f32 = 1.0 / 3.0;
/// How far above the highest floor under it the liquid lies, in metres.
const LIFT: f32 = 0.005;

/// The Sphere's pose and the field it puts on the pool at one moment.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hover {
    /// Seconds the Sphere has hovered.
    pub seconds: f32,
    /// The Sphere's centre above the pool, in metres.
    pub height: f32,
    /// Its turn about the vertical, in radians.
    pub spin: f32,
    /// The lean of its axis.
    pub tilt: Quat,
    /// The field on the pool: 1 with the Sphere lowest, a third with it highest.
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
            seconds,
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
        app.add_plugins((
            MaterialPlugin::<FerrofluidMaterial>::default(),
            MaterialPlugin::<MirrorMaterial>::default(),
        ))
        .init_resource::<HoverClock>()
        .add_systems(
            Startup,
            list_uhd_sphere
                .after(settle_gpu_tier)
                .before(start_ram_fill),
        )
        .add_systems(
            OnEnter(AppState::InGame),
            (
                spawn_reflection_probe,
                spawn_golden_sphere,
                spawn_ferrofluid_pool,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                pose_golden_sphere,
                pull_ferrofluid,
                reflect_surroundings,
                gather_stand_ins.run_if(on_modest_gpu),
                stream_uhd_sphere.run_if(on_exceptional_gpu),
            )
                .chain()
                .run_if(in_state(InGamePhase::Running)),
        )
        .add_systems(
            Update,
            hold_ferrofluid.run_if(in_state(InGamePhase::Paused)),
        );
    }
}

/// The pool's centre at the liquid's level.
fn pool_centre(terrain: &Terrain) -> Vec3 {
    let spot = golden_sphere_spot();
    spot.extend(pool_level(terrain, spot)).xzy()
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
    mut mirrors: ResMut<Assets<MirrorMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    probe: Option<Res<ReflectionProbe>>,
) {
    let pool = pool_centre(&terrain);
    let (mesh, paint) = assets::load_single_primitive(&server, assets::ARTIFACT_PREVIEWS[1]);
    let Some(source) = paints.get(&paint) else {
        warn!("the Golden Sphere's material is not loaded; leaving the Sphere out");
        return;
    };
    let gold = gild(standard_material_from_gltf_material(source));
    let at = Transform::from_translation(Vec3::NEG_Y * SOURCE_CENTRE);
    commands
        .spawn((
            Name::new("Golden Sphere"),
            GoldenSphere { pool },
            sphere_pose(pool, Hover::at(0.0)),
            Visibility::default(),
            WorldRoot,
        ))
        .with_child((
            Name::new("Golden Sphere shadow"),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial::default())),
            RenderLayers::layer(SPHERE_SHADOW_LAYER),
            at,
            detail_range(300.0, 30.0),
        ))
        .with_child((
            SphereSkin::medium(mesh.clone(), &gold, at),
            Mesh3d(mesh),
            MeshMaterial3d(mirrors.add(MirrorMaterial {
                base: gold,
                extension: Mirror::of(probe.as_deref().map(|probe| &probe.faces)),
            })),
            // Out of its own probe's sight.
            RenderLayers::layer(SPHERE_LAYER),
            // Its shadow comes from the shadow child above.
            NotShadowCaster,
            at,
            detail_range(300.0, 30.0),
        ));
}

/// Lists the Sphere's UHD scan last in `UhdFiles` on exceptional GPUs, the
/// only ones that show it, so the RAM fill reads it and `GpuScans` can
/// prefill it after the scans every tier shows; other tiers never read it.
fn list_uhd_sphere(tier: Option<Res<GpuTier>>, mut files: ResMut<UhdFiles>) {
    if tier.is_some_and(|tier| tier.level() == TierLevel::Exceptional) {
        files.0.push(assets::GOLDEN_SPHERE_UHD);
    }
}

/// The Sphere's gold: its scan's paint as a polished metal mirror.
fn gild(mut gold: StandardMaterial) -> StandardMaterial {
    gold.metallic = 1.0;
    gold.perceptual_roughness = 0.075;
    gold.reflectance = 1.0;
    gold
}

/// The pool under the Sphere, as fine as the GPU's tier asks.
fn spawn_ferrofluid_pool(
    mut commands: Commands,
    terrain: Res<Terrain>,
    tier: Option<Res<GpuTier>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut fluids: ResMut<Assets<FerrofluidMaterial>>,
    probe: Option<Res<ReflectionProbe>>,
) {
    let pool = pool_centre(&terrain);
    // Black metal, as the user asked: a mirror dark enough to read black, the
    // sky and the Sun bright in it.
    let liquid = StandardMaterial {
        base_color: Color::linear_rgb(0.05, 0.05, 0.055),
        metallic: 1.0,
        perceptual_roughness: 0.1,
        ..default()
    };
    let level = tier.map_or(TierLevel::default(), |tier| tier.level());
    let floor = |at: Vec2| terrain.mesh_height_at(pool.xz() + at) - pool.y;
    let mesh = Ferrofluid::pool_mesh(level, floor);
    let bounds = Ferrofluid::bounds(&mesh).unwrap_or_default();
    commands.spawn((
        Name::new("Ferrofluid pool"),
        FerrofluidPool,
        Mesh3d(meshes.add(mesh)),
        // Up to the spikes the vertex shader raises, kept from Bevy's own.
        bounds,
        NoAutoAabb,
        MeshMaterial3d(fluids.add(FerrofluidMaterial {
            base: liquid,
            extension: Ferrofluid::under(Hover::at(0.0)).mirroring(
                probe.as_deref().map(|probe| &probe.faces),
                SPHERE_DIAMETER / 2.0,
            ),
        })),
        Transform::from_translation(pool),
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

/// Raises the pool's spikes as the Sphere sinks, turns them with it and moves
/// the small ones in.
fn pull_ferrofluid(
    clock: Res<HoverClock>,
    pools: Query<(&MeshMaterial3d<FerrofluidMaterial>, &Transform), With<FerrofluidPool>>,
    mut fluids: ResMut<Assets<FerrofluidMaterial>>,
) {
    let hover = Hover::at(clock.0);
    let pulled = Ferrofluid::under(hover);
    for (pool, at) in &pools {
        if let Some(mut fluid) = fluids.get_mut(&pool.0) {
            // Only what moves: the probe's faces stay bound.
            let liquid = &mut fluid.extension;
            liquid.follow(&pulled);
            if liquid.probe.w > 0.0 {
                let centre = at.translation + Vec3::Y * hover.height;
                liquid.probe = centre.extend(liquid.probe.w);
            }
        }
    }
}

/// Stills the pool's motion vectors while paused: the spikes stand as they
/// stood a frame ago.
fn hold_ferrofluid(
    pools: Query<&MeshMaterial3d<FerrofluidMaterial>, With<FerrofluidPool>>,
    mut fluids: ResMut<Assets<FerrofluidMaterial>>,
) {
    for pool in &pools {
        let moving = fluids
            .get(&pool.0)
            .is_some_and(|fluid| !fluid.extension.is_still());
        if let Some(mut fluid) = fluids.get_mut(&pool.0).filter(|_| moving) {
            fluid.extension.hold();
        }
    }
}

/// Where the player's camera is and its exposure.
type Viewer = (&'static GlobalTransform, &'static Exposure);

/// Keeps the probe at the Sphere, in the player's exposure, rendering its
/// faces while the player can see the Sphere: often while the stalker or a
/// vehicle is near enough to show in it, now and then otherwise.
fn reflect_surroundings(
    tier: Option<Res<GpuTier>>,
    frame: Res<FrameCount>,
    spheres: Query<&Transform, (With<GoldenSphere>, Without<ProbeFace>)>,
    stalkers: Query<&Transform, (With<Player>, Without<ProbeFace>)>,
    seats: Query<&Transform, (With<Seat>, Without<ProbeFace>)>,
    viewers: Query<Viewer, (With<PlayerCamera>, Without<ProbeFace>)>,
    mut cameras: Query<(&ProbeFace, &mut Camera, &mut Transform, &mut Exposure)>,
) {
    let Ok(sphere) = spheres.single() else {
        return;
    };
    let centre = sphere.translation;
    let someone_near = stalkers
        .iter()
        .chain(&seats)
        .any(|at| at.translation.distance(centre) <= REACH);
    let viewer = viewers.single().ok();
    let distance = viewer.map(|(at, _)| at.translation().distance(centre));
    let exposure = viewer.map_or(Exposure::default(), |(_, exposure)| *exposure);
    let level = tier.map_or(TierLevel::default(), |tier| tier.level());
    let watch = watch(distance, someone_near);
    steer_reflection_probe(centre, watch, level, &frame, exposure, &mut cameras);
}

/// Modest GPUs get no live reflection map: the Sphere mirrors stand-ins.
fn on_modest_gpu(tier: Option<Res<GpuTier>>) -> bool {
    tier.is_some_and(|tier| tier.level() == TierLevel::Modest)
}

/// Hands the Sphere's mirror the stalker and vehicles near it, and the Sun.
fn gather_stand_ins(
    spheres: Query<&Transform, With<GoldenSphere>>,
    stalkers: Query<(&Transform, &Locomotion, Has<Driving>), With<Player>>,
    seats: Query<(&Transform, &Seat, Has<Carrier>)>,
    suns: Query<(&DirectionalLight, &GlobalTransform), With<Sun>>,
    mirrors: Query<&MeshMaterial3d<MirrorMaterial>>,
    mut materials: ResMut<Assets<MirrorMaterial>>,
) {
    let Ok(sphere) = spheres.single() else {
        return;
    };
    let stalker = stalkers
        .single()
        .ok()
        .map(|(at, locomotion, driving)| Stalker {
            feet: at.translation,
            prone: locomotion.gait.is_prone(),
            driving,
        });
    let hulls: Vec<Hull> = seats
        .iter()
        .map(|(at, seat, carrier)| Hull {
            base: at.translation,
            yaw: at.rotation.to_euler(EulerRot::YXZ).0,
            half: seat.half_hull(),
            paint: if carrier { CARRIER_PAINT } else { CAR_PAINT },
        })
        .collect();
    let shown = stand_ins(sphere.translation, stalker, &hulls);
    let sun = suns.single().map_or(Vec4::ZERO, |(light, at)| {
        stand_in_light(*at.back(), light.illuminance)
    });
    for mirror in &mirrors {
        if let Some(mut material) = materials.get_mut(&mirror.0) {
            material.extension.stand_ins = shown;
            material.extension.sun = sun;
        }
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
        assert!((low.height - 2.1).abs() < 0.01, "lowest {}", low.height);
        assert!((high.height - 2.4).abs() < 0.01, "highest {}", high.height);
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
    fn only_exceptional_gpus_read_and_prefill_the_uhd_sphere_last() {
        for (level, listed) in [
            (TierLevel::Modest, false),
            (TierLevel::Strong, false),
            (TierLevel::Exceptional, true),
        ] {
            let mut app = App::new();
            app.insert_resource(UhdFiles(vec!["tree", "site"]))
                .insert_resource(GpuTier::fallback(6.0, Some(level)))
                .add_systems(Update, list_uhd_sphere);
            app.update();
            let files = &app.world().resource::<UhdFiles>().0;
            let expected = if listed {
                vec!["tree", "site", assets::GOLDEN_SPHERE_UHD]
            } else {
                vec!["tree", "site"]
            };
            assert_eq!(files, &expected, "{level:?}");
        }
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
            running.translation.y > 2.0,
            "the Sphere hovers over the stalker's head"
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
