//! The UN patrol: four white armored cars (`UN_VEHICLE`, the stalkers' car
//! repainted with a UN marking on the roof) lapping a long loop beside the
//! Perimeter wall on Harmont's side (`world::PATROL_TURNS`), a quarter lap
//! apart. They drive north along the wall from south of the hangar's latitude
//! past the Institute gate, turn out beyond the Institute and come back on the
//! far lane. Each eases to a halt while the stalker, their car or another
//! patrol stands in its way. Both flanks carry a UN plate: their faces share
//! texture pixels in the model, so the plates are separate quads.
//!
//! `assets/textures/decals/un_flank.png` keeps only the blue of the supplied
//! decal photo (`un_decal_blue_on_white.jpg`) as its alpha, over a flat UN blue.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;
use crate::world::{Collider, Describe, PATROL_RADIUS, PATROL_TURNS, Place, Terrain};

use super::super::Player;
use super::carrier::Carrier;
use super::{
    CAR, COLLISION, MODEL_SCALE, Vehicle, WHEEL_RADIUS, WHEELBASE, contact, pose_wheel, wheel_slot,
};

const CRUISE: f32 = 7.0;
const TURNING: f32 = 4.0;
const ACCELERATION: f32 = 2.5;
/// How quickly the front wheels follow the road, per second.
const STEER_RATE: f32 = 3.0;
/// It stops for anything this far ahead and this close to its middle line.
const LOOK_AHEAD: f32 = 11.0;
const LOOK_SIDE: f32 = 3.0;
/// How many cars share the loop, spaced evenly around it.
const PATROLS: usize = 4;
/// The flank plates, in model units: size, and where the doors face out.
const PLATE: Vec2 = Vec2::new(0.25, 0.25 * 175.0 / 512.0);
const PLATE_AT: Vec3 = Vec3::new(0.219, 0.255, 0.015);

#[derive(Component, Default)]
pub(crate) struct Patrol {
    /// Meters driven into the current lap.
    along: f32,
    speed: f32,
    steer: f32,
    roll: f32,
    suspension: [Vec3; 4],
}

/// A wheel pivot of a car the game drives: a patrol or Harmont's traffic.
#[derive(Component)]
pub(crate) struct CarWheel {
    pub(super) slot: usize,
    pub(super) rest: Vec3,
}

/// One lap's length: two lanes and two half circles.
fn lap() -> f32 {
    let [south, north] = PATROL_TURNS;
    2.0 * (north.y - south.y) + 2.0 * PI * PATROL_RADIUS
}

/// Where the loop is `along` meters into a lap, which way it heads there, and
/// whether it turns.
fn on_loop(along: f32) -> (Vec2, Vec2, bool) {
    let [south, north] = PATROL_TURNS;
    let lane = north.y - south.y;
    let arc = PI * PATROL_RADIUS;
    let s = along.rem_euclid(lap());
    if s < lane {
        // North along the wall.
        (
            Vec2::new(south.x - PATROL_RADIUS, south.y + s),
            Vec2::Y,
            false,
        )
    } else if s < lane + arc {
        // Out from the wall around the north turn.
        let (sin, cos) = ((s - lane) / PATROL_RADIUS).sin_cos();
        let at = north + PATROL_RADIUS * Vec2::new(-cos, sin);
        (at, Vec2::new(sin, cos), true)
    } else if s < 2.0 * lane + arc {
        // South on the far lane.
        let t = s - lane - arc;
        (
            Vec2::new(north.x + PATROL_RADIUS, north.y - t),
            Vec2::NEG_Y,
            false,
        )
    } else {
        // Back toward the wall around the south turn.
        let (sin, cos) = ((s - 2.0 * lane - arc) / PATROL_RADIUS).sin_cos();
        let at = south + PATROL_RADIUS * Vec2::new(cos, -sin);
        (at, Vec2::new(-sin, -cos), true)
    }
}

/// True when `obstacle` stands in the lane just ahead of a car at `at`.
pub(super) fn in_the_way(at: Vec2, heading: Vec2, obstacle: Vec2) -> bool {
    let offset = obstacle - at;
    let ahead = offset.dot(heading);
    (0.0..LOOK_AHEAD).contains(&ahead) && heading.perp_dot(offset).abs() < LOOK_SIDE
}

pub(crate) fn spawn_patrol(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let plate = meshes.add(Rectangle::from_size(PLATE));
    let paint = materials.add(StandardMaterial {
        base_color_texture: Some(server.load(assets::UN_DECAL)),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.55,
        depth_bias: 50.0,
        ..default()
    });
    for i in 0..PATROLS {
        let along = lap() * i as f32 / PATROLS as f32;
        let (at, heading, _) = on_loop(along);
        let mut patrol = commands.spawn((
            Name::new("UN patrol"),
            Patrol { along, ..default() },
            Collider(COLLISION),
            CAR,
            Describe {
                place: Place::Patrol,
                radius: 2.4,
                height: 2.6,
            },
            Transform::from_translation(at.extend(0.0).xzy())
                .looking_to(heading.extend(0.0).xzy(), Vec3::Y)
                .with_scale(Vec3::splat(MODEL_SCALE)),
            WorldAssetRoot(assets::load_vehicle_scene(&server, assets::UN_VEHICLE)),
        ));
        // A quad faces +Z; turned a quarter about Y it faces a flank, lettering
        // reading front to back on the right and back to front on the left.
        for side in [1.0, -1.0] {
            patrol.with_child((
                Mesh3d(plate.clone()),
                MeshMaterial3d(paint.clone()),
                Transform::from_translation(PLATE_AT * Vec3::new(side, 1.0, 1.0))
                    .with_rotation(Quat::from_rotation_y(side * FRAC_PI_2)),
                NotShadowCaster,
            ));
        }
        patrol.observe(attach_wheels);
    }
}

pub(super) fn attach_wheels(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    wheels: Query<(&Name, &Transform)>,
) {
    for entity in children.iter_descendants(ready.entity) {
        let Ok((name, transform)) = wheels.get(entity) else {
            continue;
        };
        if let Some(slot) = wheel_slot(name) {
            commands.entity(entity).insert(CarWheel {
                slot,
                rest: transform.translation,
            });
        }
    }
}

/// What the patrol stops for: the stalker, their car and the Institute's carrier.
type InTheWay<'w, 's> = Query<
    'w,
    's,
    &'static Transform,
    (
        Or<(With<Player>, With<Vehicle>, With<Carrier>)>,
        Without<Patrol>,
    ),
>;

type Wheels<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static CarWheel, &'static mut Transform),
    (
        Without<Patrol>,
        Without<Player>,
        Without<Vehicle>,
        Without<Carrier>,
    ),
>;

/// Drives each patrol around the loop over the rendered ground, then poses
/// their wheels.
pub(crate) fn drive_patrol(
    time: Res<Time>,
    terrain: Res<Terrain>,
    others: InTheWay,
    mut patrols: Query<(Entity, &mut Patrol, &mut Transform), Without<CarWheel>>,
    mut wheels: Wheels,
    parents: Query<&ChildOf>,
) {
    let dt = time.delta_secs();
    let cars: Vec<(Entity, Vec2)> = patrols
        .iter()
        .map(|(entity, _, transform)| (entity, transform.translation.xz()))
        .collect();
    for (entity, mut patrol, mut transform) in &mut patrols {
        let (at, heading, turning) = on_loop(patrol.along);
        let blocked = others
            .iter()
            .map(|other| other.translation.xz())
            .chain(cars.iter().filter(|car| car.0 != entity).map(|car| car.1))
            .any(|other| in_the_way(at, heading, other));
        let target = match (blocked, turning) {
            (true, _) => 0.0,
            (false, true) => TURNING,
            (false, false) => CRUISE,
        };
        let change = (target - patrol.speed).clamp(-ACCELERATION * dt, ACCELERATION * dt);
        patrol.speed += change;
        let step = patrol.speed * dt;
        patrol.along = (patrol.along + step).rem_euclid(lap());
        patrol.roll += step / WHEEL_RADIUS;
        // The rear axle follows the loop; the front wheels turn with it.
        let lean = if turning {
            (WHEELBASE / PATROL_RADIUS).atan()
        } else {
            0.0
        };
        patrol.steer += (lean - patrol.steer) * (1.0 - (-STEER_RATE * dt).exp());

        let (at, heading, _) = on_loop(patrol.along);
        let yaw = Quat::from_rotation_y(heading.x.atan2(heading.y));
        let support = contact::sample(at, yaw, |p| terrain.mesh_height_at(p));
        transform.translation = at.extend(support.height).xzy();
        transform.rotation = support.rotation;
        patrol.suspension = support.suspension();
    }
    for (entity, wheel, mut transform) in &mut wheels {
        // Wheel pivots sit a level or two below the patrol's root.
        let patrol = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| patrols.get(ancestor).ok());
        if let Some((_, patrol, _)) = patrol {
            let gear = (patrol.steer, patrol.roll, patrol.suspension);
            pose_wheel(&mut transform, wheel.slot, wheel.rest, gear);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loop_closes_and_turns_smoothly() {
        let steps = 4000;
        let ds = lap() / steps as f32;
        for i in 0..steps {
            let (a, heading, _) = on_loop(i as f32 * ds);
            let (b, _, _) = on_loop((i + 1) as f32 * ds);
            // Each step moves the way the loop heads, by the step's length.
            assert!((a.distance(b) - ds).abs() < 1e-2, "jump at {a}");
            assert!(
                (b - a).normalize().dot(heading) > 0.99,
                "heading off at {a}"
            );
        }
        let (start, _, _) = on_loop(0.0);
        let (end, _, _) = on_loop(lap() - 1e-3);
        assert!(start.distance(end) < 0.01);
    }

    #[test]
    fn the_patrol_stops_only_for_what_is_ahead_in_its_lane() {
        let at = Vec2::new(707.0, 0.0);
        assert!(in_the_way(at, Vec2::Y, at + Vec2::new(1.0, 6.0)));
        assert!(!in_the_way(at, Vec2::Y, at + Vec2::new(0.0, -4.0)));
        assert!(!in_the_way(at, Vec2::Y, at + Vec2::new(5.0, 6.0)));
        assert!(!in_the_way(at, Vec2::Y, at + Vec2::new(0.0, 30.0)));
    }

    #[test]
    fn the_loop_runs_on_harmonts_side_over_dry_ground() {
        let terrain = Terrain::new();
        let mut along = 0.0;
        while along < lap() {
            let (at, _, _) = on_loop(along);
            assert!(!crate::world::in_zone(at), "patrol in the Zone at {at}");
            assert!(
                terrain.water_depth_at(at.x, at.y) < 0.1,
                "patrol wades at {at}"
            );
            along += 5.0;
        }
    }
}
