//! The Perimeter guns: twelve automatic guns on top of the Perimeter wall,
//! alternating a rotary gatling and a naval mount, spread evenly from 50 map
//! tiles south of the hangar to 50 past the Institute.
//!
//! Each sweeps the Zone with a red laser (`scan.rs`). Whatever the beam
//! touches that has no business there, a ground creature or a stalker who came
//! in through the crawl hole, the gun locks onto: it turns after it, spins up
//! and fires tracer bursts until it brings it down (`fire.rs`). The beam is the
//! only eye the gun has, so it can be avoided: near the wall it passes
//! overhead, and further out a crawling stalker slips under it.
//!
//! The models (`GATLING_TURRET`, `NAVAL_TURRET` and their light `_FAR` tiers)
//! have a static `Base` and a `Head` that yaws about its local +Y, holding the
//! `Barrels`, which spin about their local +Z, and an empty `Muzzle` at their
//! tips. The barrels point +Z at rest; the models stand about a unit tall.

mod fire;
mod scan;

use std::f32::consts::FRAC_PI_2;

use bevy::camera::visibility::VisibilityRange;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;
use crate::states::{AppState, InGamePhase};

use super::collision::{Collider, Pillar, Unclimbable};
use super::cordon::{PERIMETER_GATE, PERIMETER_GATE_HALF, PERIMETER_X, perimeter_top};
use super::map::METERS_PER_PIXEL;
use super::places::{Describe, Place};
use super::sites::{self, Kind as SiteKind};
use super::terrain::{BASE, Terrain};
use super::{WorldRoot, range_scene_meshes};

pub(crate) use fire::GunSound;

/// How far the guarded stretch reaches past the hangar and past the Institute: 50 map tiles.
pub(super) const REACH_PAST: f32 = 50.0 * METERS_PER_PIXEL;
const GUNS: usize = 12;
/// How far from the middle of the Institute gate a gun stands at least: past
/// the opening's half width and the wall's end cap.
const GATE_CLEAR: f32 = PERIMETER_GATE_HALF + 6.0;
/// Meters per model unit: a gun about 3.5 m long, big enough to read from the
/// tactical camera on the 2.2 m-thick wall top.
const SCALE: f32 = 3.5;
/// The heading (`atan2(x, z)`) that faces the Zone, -X.
const ZONE_HEADING: f32 = -FRAC_PI_2;
/// The full-quality model shows within `NEAR` meters of the camera, the light one beyond, up to `FAR`.
const NEAR: f32 = 150.0;
const FAR: f32 = 700.0;
const FADE: f32 = 10.0;
const PILLARS: &[Pillar] = &[Pillar {
    x: 0.0,
    z: 0.0,
    radius: 1.1,
    bottom: 0.0,
    top: 2.4,
}];
/// The muzzle from the foot of the yaw axis, facing +Z, in meters, until the model gives its own.
const MUZZLE: Vec3 = Vec3::new(0.0, 0.62 * SCALE, 0.55 * SCALE);
/// The beam's radius, and how high the ground spot floats to stay clear of the terrain.
const BEAM_RADIUS: f32 = 0.07;
const SPOT_RADIUS: f32 = 0.55;
const SPOT_LIFT: f32 = 0.06;
/// Emissive in physical units, tuned for the daytime exposure (EV 13, about
/// 1e-4): the beam reads near 2 and its spot near 6 in daylight, both blooming
/// brighter at night (EV 9). Tracers and the muzzle flash are brighter still.
const BEAM_GLOW: f32 = 18_000.0;
const SPOT_GLOW: f32 = 60_000.0;
const TRACER_GLOW: f32 = 60_000.0;
const FLASH_GLOW: f32 = 150_000.0;
/// Tracers in flight at once, across all the guns.
const TRACERS: usize = 64;

pub(super) struct TurretPlugin;

impl Plugin for TurretPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<GunSound>()
            .add_systems(OnEnter(AppState::InGame), spawn_turrets)
            .add_systems(
                Update,
                (
                    scan::mark_trespassers,
                    scan::aim_turrets,
                    fire::fire_turrets,
                    fire::fly_tracers,
                    (draw_beams, pose_turret_parts),
                )
                    .chain()
                    .run_if(in_state(InGamePhase::Running)),
            );
    }
}

/// The two guns on the wall.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Gun {
    /// A rotary gun: quick to turn, firing fast streams.
    Gatling,
    /// A heavier naval mount: slower to turn and fire.
    Naval,
}

impl Gun {
    /// The full-quality model and the light one shown far away.
    fn models(self) -> [&'static str; 2] {
        match self {
            Gun::Gatling => [assets::GATLING_TURRET, assets::GATLING_TURRET_FAR],
            Gun::Naval => [assets::NAVAL_TURRET, assets::NAVAL_TURRET_FAR],
        }
    }
}

/// Where a gun stands along the Perimeter and which it is.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Station {
    z: f32,
    gun: Gun,
}

/// The stretch of the Perimeter the guns watch, south to north (z): from
/// `REACH_PAST` south of the hangar to as far past the Institute.
pub(super) fn guarded_span() -> (f32, f32) {
    let institute = sites::outlines()
        .find(|&(_, _, _, place)| place == Place::Site(SiteKind::Institute))
        .map(|(center, ..)| center)
        .expect("the Institute is one of the map's landmarks");
    (BASE.1 - REACH_PAST, institute.y + REACH_PAST)
}

/// The guns, evenly spaced along the guarded stretch and alternating, except
/// that one falling in the Institute gate's opening steps clear of it.
fn stations() -> [Station; GUNS] {
    let (south, north) = guarded_span();
    std::array::from_fn(|i| {
        let even = south + (north - south) * i as f32 / (GUNS - 1) as f32;
        let from_gate = even - PERIMETER_GATE.y;
        let z = if from_gate.abs() < GATE_CLEAR {
            PERIMETER_GATE.y + GATE_CLEAR.copysign(from_gate)
        } else {
            even
        };
        let gun = if i % 2 == 0 { Gun::Gatling } else { Gun::Naval };
        Station { z, gun }
    })
}

/// The gun's effects: its laser beam, the spot where it meets the ground and the muzzle flash.
#[derive(Clone, Copy)]
struct Effects {
    beam: Entity,
    spot: Entity,
    flash: Entity,
}

/// A gun on the Perimeter wall, its root standing on the wall top under its yaw axis.
#[derive(Component)]
#[require(Unclimbable)]
pub(super) struct Turret {
    gun: Gun,
    /// The foot of the yaw axis on the wall top.
    base: Vec3,
    /// The muzzle from `base` while the head faces +Z, in meters.
    muzzle: Vec3,
    /// Where the head points (`atan2(x, z)`), and where it pointed last frame.
    yaw: f32,
    swept: f32,
    /// Where the beam ends: on the ground, or at the locked target's feet.
    end: Vec3,
    scan: scan::Sweep,
    lock: Option<fire::Lock>,
    /// How fast the barrels turn, from 0 to 1 (firing speed), and how far they have turned.
    spin: f32,
    turn: f32,
    /// Rounds fired, seeding each tracer's spread.
    rounds: u32,
    /// Whether the muzzle flashes this frame.
    flash: bool,
    effects: Effects,
}

impl Turret {
    fn new(gun: Gun, base: Vec3, phase: f32, effects: Effects) -> Self {
        let scan = scan::Sweep::new(phase);
        Self {
            gun,
            base,
            muzzle: MUZZLE,
            yaw: scan.heading(),
            swept: scan.heading(),
            end: base,
            scan,
            lock: None,
            spin: 0.0,
            turn: 0.0,
            rounds: 0,
            flash: false,
            effects,
        }
    }

    /// Where the muzzle is now, as the head points.
    fn muzzle_at(&self) -> Vec3 {
        self.base + Quat::from_rotation_y(self.yaw) * self.muzzle
    }

    /// Which way the head points, flat.
    fn facing(&self) -> Vec2 {
        Vec2::new(self.yaw.sin(), self.yaw.cos())
    }
}

/// A model node that moves with its gun: the yawing head or the spinning barrels.
#[derive(Component)]
struct TurretPart {
    turret: Entity,
    rest: Transform,
    barrels: bool,
}

/// A tracer round in flight, idle in the pool while `life` is spent.
#[derive(Component, Default)]
struct Tracer {
    velocity: Vec3,
    life: f32,
}

/// The muzzle flash's flickering glow and light.
#[derive(Component)]
struct MuzzleFlash;

/// Shared meshes and glowing materials of the guns' effects.
struct Looks {
    beam: (Handle<Mesh>, Handle<StandardMaterial>),
    spot: (Handle<Mesh>, Handle<StandardMaterial>),
    flash: (Handle<Mesh>, Handle<StandardMaterial>),
    tracer: (Handle<Mesh>, Handle<StandardMaterial>),
}

impl Looks {
    fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let mut glow = |color: LinearRgba| {
            materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: color,
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                alpha_mode: AlphaMode::Add,
                fog_enabled: false,
                ..default()
            })
        };
        let red = LinearRgba::rgb(1.0, 0.03, 0.02);
        let fire = LinearRgba::rgb(1.0, 0.55, 0.16);
        Self {
            beam: (
                meshes.add(Cylinder::new(BEAM_RADIUS, 1.0)),
                glow(red * BEAM_GLOW),
            ),
            spot: (meshes.add(Circle::new(SPOT_RADIUS)), glow(red * SPOT_GLOW)),
            flash: (meshes.add(Sphere::new(0.3)), glow(fire * FLASH_GLOW)),
            tracer: (
                meshes.add(Cuboid::new(0.07, 0.07, 2.6)),
                glow(fire * TRACER_GLOW),
            ),
        }
    }
}

fn spawn_turrets(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let looks = Looks::new(&mut meshes, &mut materials);
    let near = VisibilityRange {
        end_margin: NEAR..NEAR + FADE,
        use_aabb: true,
        ..default()
    };
    let far = VisibilityRange {
        start_margin: NEAR..NEAR + FADE,
        end_margin: FAR..FAR + FADE,
        use_aabb: true,
    };
    for (i, station) in stations().into_iter().enumerate() {
        let base = Vec3::new(PERIMETER_X, perimeter_top(&terrain, station.z), station.z);
        let effects = spawn_effects(&mut commands, &looks, &far);
        let root = commands
            .spawn((
                WorldRoot,
                Name::new("Perimeter gun"),
                Turret::new(station.gun, base, i as f32 * 2.39, effects),
                Collider(PILLARS),
                Describe {
                    place: Place::Turret,
                    radius: 1.3,
                    height: 2.4,
                },
                Transform::from_translation(base)
                    .with_rotation(Quat::from_rotation_y(ZONE_HEADING)),
                Visibility::default(),
            ))
            .id();
        for (path, range) in station.gun.models().into_iter().zip([&near, &far]) {
            commands
                .spawn((
                    ChildOf(root),
                    WorldAssetRoot(assets::load_vehicle_scene(&server, path)),
                    Transform::from_scale(Vec3::splat(SCALE)),
                    Visibility::default(),
                    range.clone(),
                ))
                .observe(range_scene_meshes)
                .observe(attach_turret_parts);
        }
    }
    for _ in 0..TRACERS {
        commands.spawn((
            WorldRoot,
            Tracer::default(),
            Mesh3d(looks.tracer.0.clone()),
            MeshMaterial3d(looks.tracer.1.clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

/// The beam, its ground spot and the muzzle flash of one gun, posed each frame by `draw_beams`.
fn spawn_effects(commands: &mut Commands, looks: &Looks, range: &VisibilityRange) -> Effects {
    let mut glow = |(mesh, material): &(Handle<Mesh>, Handle<StandardMaterial>)| {
        commands
            .spawn((
                WorldRoot,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Visibility::default(),
                range.clone(),
                NotShadowCaster,
                NotShadowReceiver,
            ))
            .id()
    };
    let effects = Effects {
        beam: glow(&looks.beam),
        spot: glow(&looks.spot),
        flash: glow(&looks.flash),
    };
    commands.entity(effects.flash).insert((
        MuzzleFlash,
        Visibility::Hidden,
        PointLight {
            color: Color::srgb(1.0, 0.7, 0.4),
            intensity: 0.0,
            range: 14.0,
            shadow_maps_enabled: false,
            ..default()
        },
    ));
    effects
}

/// Finds the moving nodes of a gun's model once it is in the world, and where its muzzle is.
fn attach_turret_parts(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    nodes: Query<(&Name, &Transform)>,
    poses: Query<&Transform>,
    mut turrets: Query<&mut Turret>,
) {
    let Ok(turret) = parents.get(ready.entity).map(ChildOf::parent) else {
        return;
    };
    for entity in children.iter_descendants(ready.entity) {
        let Ok((name, rest)) = nodes.get(entity) else {
            continue;
        };
        match name.as_str() {
            "Head" | "Barrels" => {
                commands.entity(entity).insert(TurretPart {
                    turret,
                    rest: *rest,
                    barrels: name.as_str() == "Barrels",
                });
            }
            "Muzzle" => {
                // Up the hierarchy to the gun's root: the scene's scale included, its heading not.
                let mut at = Vec3::ZERO;
                let mut node = entity;
                while node != turret {
                    let (Ok(pose), Ok(parent)) = (poses.get(node), parents.get(node)) else {
                        break;
                    };
                    at = pose.transform_point(at);
                    node = parent.parent();
                }
                if let Ok(mut gun) = turrets.get_mut(turret) {
                    gun.muzzle = at;
                }
            }
            _ => {}
        }
    }
}

/// Turns the heads to where their guns point and spins the barrels, on both model tiers.
fn pose_turret_parts(turrets: Query<&Turret>, mut parts: Query<(&TurretPart, &mut Transform)>) {
    for (part, mut pose) in &mut parts {
        let Ok(turret) = turrets.get(part.turret) else {
            continue;
        };
        let turn = if part.barrels {
            Quat::from_rotation_z(turret.turn)
        } else {
            Quat::from_rotation_y(turret.yaw - ZONE_HEADING)
        };
        pose.rotation = part.rest.rotation * turn;
    }
}

/// Stretches each beam from its muzzle to where it ends, lays the spot there and flickers the flash.
fn draw_beams(
    turrets: Query<&Turret>,
    mut effects: Query<(&mut Transform, &mut Visibility, Option<&mut PointLight>), Without<Turret>>,
) {
    for turret in &turrets {
        let muzzle = turret.muzzle_at();
        let span = turret.end - muzzle;
        if let Ok((mut pose, ..)) = effects.get_mut(turret.effects.beam) {
            *pose = Transform::from_translation(muzzle + span * 0.5)
                .with_rotation(Quat::from_rotation_arc(
                    Vec3::Y,
                    span.normalize_or(Vec3::NEG_Y),
                ))
                .with_scale(Vec3::new(1.0, span.length(), 1.0));
        }
        if let Ok((mut pose, ..)) = effects.get_mut(turret.effects.spot) {
            *pose = Transform::from_translation(turret.end + Vec3::Y * SPOT_LIFT)
                .with_rotation(Quat::from_rotation_x(-FRAC_PI_2));
        }
        if let Ok((mut pose, mut visibility, Some(mut light))) =
            effects.get_mut(turret.effects.flash)
        {
            let flicker = fire::flicker(turret.turn);
            *visibility = if turret.flash {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            light.intensity = if turret.flash { 4.0e6 * flicker } else { 0.0 };
            let ahead = turret.facing().extend(0.0).xzy() * 0.3;
            *pose =
                Transform::from_translation(muzzle + ahead).with_scale(Vec3::splat(0.5 + flicker));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map::map;

    #[test]
    fn twelve_guns_alternate_along_the_wall_from_past_the_hangar_to_past_the_institute() {
        let guns = stations();
        assert_eq!(guns.len(), 12);
        // The Institute where the Harmont map draws it.
        let institute = map(349.5, 170.5);
        assert!(
            (institute.y - 510.0).abs() < 1.0,
            "Institute at {institute}"
        );
        let (south, north) = guarded_span();
        assert!((south - (BASE.1 - 120.0)).abs() < 0.01);
        assert!((north - (institute.y + 120.0)).abs() < 0.01);
        assert_eq!(guns[0].z, south);
        assert_eq!(guns[GUNS - 1].z, north);
        let spacing = (north - south) / (GUNS - 1) as f32;
        assert!((75.0..82.0).contains(&spacing), "{spacing} m apart");
        for (i, pair) in guns.windows(2).enumerate() {
            assert_ne!(pair[0].gun, pair[1].gun, "guns {i} and {} match", i + 1);
            assert!(pair[1].z - pair[0].z > 40.0, "guns {i} too close");
        }
        let moved: Vec<_> = guns
            .iter()
            .enumerate()
            .filter(|(i, gun)| (gun.z - (south + spacing * *i as f32)).abs() > 0.01)
            .collect();
        assert!(moved.len() <= 1, "only a gun in the gate steps aside");
        for gun in guns {
            assert!(
                (gun.z - PERIMETER_GATE.y).abs() >= GATE_CLEAR,
                "a gun stands in the gate at {}",
                gun.z
            );
        }
    }

    #[test]
    fn guns_stand_on_top_of_the_perimeter_wall() {
        let terrain = Terrain::new();
        for station in stations() {
            let at = Vec2::new(PERIMETER_X, station.z);
            let above = perimeter_top(&terrain, station.z) - terrain.mesh_height_at(at);
            assert!((8.5..9.6).contains(&above), "wall top {above} m up at {at}");
        }
    }

    #[test]
    fn a_gun_finds_a_deer_in_its_beam_shoots_it_and_brings_it_down() {
        use std::time::Duration;

        use crate::player::Respawn;
        use crate::world::CreatureKind;
        use crate::world::wildlife::Downed;

        const STEP: Duration = Duration::from_millis(50);
        let terrain = Terrain::new();
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(terrain.clone())
            .add_message::<Respawn>()
            .add_message::<GunSound>()
            .add_systems(
                Update,
                (
                    scan::mark_trespassers,
                    scan::aim_turrets,
                    fire::fire_turrets,
                    fire::fly_tracers,
                    (draw_beams, pose_turret_parts),
                )
                    .chain(),
            );
        let world = app.world_mut();
        let mut effect = || {
            world
                .spawn((Transform::default(), Visibility::default()))
                .id()
        };
        let effects = Effects {
            beam: effect(),
            spot: effect(),
            flash: effect(),
        };
        world
            .entity_mut(effects.flash)
            .insert((Visibility::Hidden, PointLight::default()));
        let tracer = world
            .spawn((Tracer::default(), Transform::default(), Visibility::Hidden))
            .id();
        let z = stations()[0].z;
        let base = Vec3::new(PERIMETER_X, perimeter_top(&terrain, z), z);
        let gun = Turret::new(Gun::Gatling, base, 0.0, effects);
        // Where the beam will meet the ground after the first step.
        let mut probe = Turret::new(Gun::Gatling, base, 0.0, effects);
        probe.sweep(&terrain, STEP.as_secs_f32());
        world.spawn(gun);
        let deer = world
            .spawn((
                Transform::from_translation(probe.end),
                Describe {
                    place: Place::Creature(CreatureKind::Hexadeer),
                    radius: 0.74,
                    height: 0.99,
                },
            ))
            .id();
        let (mut bursts, mut traced) = (0, false);
        for _ in 0..100 {
            app.world_mut().resource_mut::<Time>().advance_by(STEP);
            app.update();
            let world = app.world_mut();
            bursts += world.resource_mut::<Messages<GunSound>>().drain().count();
            traced |= world.get::<Visibility>(tracer) == Some(&Visibility::Inherited);
            if world.get::<Downed>(deer).is_some() {
                break;
            }
        }
        assert!(
            app.world().get::<Downed>(deer).is_some(),
            "the deer was not shot"
        );
        assert!(bursts >= 1 && traced, "{bursts} bursts, tracers {traced}");
        assert!(app.world().resource::<Messages<Respawn>>().is_empty());
    }
}
