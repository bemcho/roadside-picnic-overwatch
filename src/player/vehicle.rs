//! Driving, collision, and wheel posing for the armored vehicle.

mod carrier;
mod contact;
mod patrol;
mod traffic;

pub(super) use carrier::{drive_carrier, spawn_carrier};
pub(super) use patrol::{Patrol, drive_patrol, spawn_patrol};
pub(super) use traffic::{drive_traffic, flash_beacons, spawn_traffic};

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;
use crate::world::{
    Collider, Crusher, Describe, InstitutePass, Pillar, Place, STEP_HEIGHT, TERRAIN_HALF_SIZE,
    Terrain, Weight, push_out,
};

use super::{
    GRAVITY, Locomotion, Player, STALKER, STANDING,
    camera::OrbitCamera,
    freefall,
    orders::{MoveOrder, Turning},
    response,
};

const MODEL_SCALE: f32 = 5.2;
const WHEEL_RADIUS: f32 = 0.1008 * MODEL_SCALE;
const BLOCKED_WHEEL_SPEED: f32 = 3.6;
const DRIVE_ACCELERATION: f32 = 6.0;
const MAX_STEER: f32 = 0.48;
const FAST_STEER: f32 = 0.27;
const LOW_RANGE_SPEED: f32 = 10.0;
const FORWARD_SPEED: f32 = 16.0;
const ROAD_FORWARD_SPEED: f32 = 24.0;
const REVERSE_SPEED: f32 = 4.0;
const WHEELBASE: f32 = 2.8;
const DRIVE_STEP: f32 = 0.025;
const MAX_STEP_TRAVEL: f32 = 0.25;
const HALF_EXTENTS: Vec2 = Vec2::new(0.283 * MODEL_SCALE, 0.5 * MODEL_SCALE);
const HULL_HEIGHT: f32 = 0.47 * MODEL_SCALE;
const GROUND_CLEARANCE: f32 = 0.035;
const ENTRY_REACH: f32 = 1.65;
pub(super) const FORD_DEPTH: f32 = 0.60;
const MAX_GRIP_GRADE: f32 = 0.66;
const CONTACT_SNAP: f32 = 0.014;
const FORWARD: Vec3 = Vec3::Z;
#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
const COLLISION: &[Pillar] = include!("../world/pillars/armored_vehicle.rs");
/// An armored car's weight and footprint: it runs through and knocks down
/// whatever weighs less, and flattens the plants under it.
pub(super) const CAR: Crusher = Crusher {
    mass: 12_000.0,
    half_size: HALF_EXTENTS,
};

/// Every collider, with what it weighs.
type Solids<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Collider,
        &'static GlobalTransform,
        Option<&'static Weight>,
    ),
>;

#[derive(Component)]
pub(crate) struct Vehicle {
    speed: f32,
    throttle: f32,
    roll: f32,
    steer: f32,
    slide: Vec2,
    vertical_speed: f32,
    grounded: bool,
    road: f32,
    suspension: [Vec3; 4],
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            speed: 0.0,
            throttle: 0.0,
            roll: 0.0,
            steer: 0.0,
            slide: Vec2::ZERO,
            vertical_speed: 0.0,
            grounded: true,
            road: 0.0,
            suspension: [Vec3::ZERO; 4],
        }
    }
}

#[derive(Component)]
pub(crate) struct Driving(pub(crate) Entity);

pub(super) type Driver<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Transform,
        &'static mut Locomotion,
        &'static mut Visibility,
        Option<&'static Driving>,
    ),
    With<Player>,
>;

#[derive(Component)]
pub(super) struct Wheel {
    vehicle: Entity,
    slot: usize,
    rest: Vec3,
}

#[derive(Component)]
pub(super) struct VehicleSpotlight;

pub(crate) fn parked_vehicle(floor: Vec3) -> Transform {
    Transform::from_translation(floor + Vec3::new(8.5, GROUND_CLEARANCE, 8.5))
        .with_rotation(Quat::from_rotation_y(-0.59))
}

impl Vehicle {
    pub(crate) fn engine_load(&self) -> f32 {
        let scale = if self.speed < 0.0 {
            LOW_RANGE_SPEED
        } else {
            FORWARD_SPEED + (ROAD_FORWARD_SPEED - FORWARD_SPEED) * self.road
        };
        (self.speed.abs() / scale).min(1.0)
    }

    pub(super) fn park(&mut self) {
        self.speed = 0.0;
        self.throttle = 0.0;
        self.steer = 0.0;
    }
}

pub(crate) fn spawn_vehicle(
    commands: &mut Commands,
    assets: &AssetServer,
    transform: Transform,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let mut vehicle = commands.spawn((
        Vehicle::default(),
        Collider(COLLISION),
        CAR,
        // The stalkers' car runs on Institute papers: the gate's boom lifts for it.
        InstitutePass,
        Describe {
            place: Place::Vehicle,
            radius: 2.4,
            height: 2.6,
        },
        transform.with_scale(Vec3::splat(MODEL_SCALE)),
        WorldAssetRoot(assets::load_vehicle_scene(assets, assets::BASE_VEHICLE)),
    ));
    vehicle.observe(attach_wheels);
    mount_spotlights(&mut vehicle, meshes, materials);
    vehicle.id()
}

/// The vehicle's two lamps, in model coordinates (local +Z is the nose): a
/// dipped beam at the grille that lights the ground just ahead, and a
/// searchlight on the cab roof that throws a longer, wider spot down the road.
/// Both shine only while someone drives.
fn mount_spotlights(
    vehicle: &mut EntityCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let color = Color::srgb(0.91, 0.96, 1.0);
    let dipped = (
        Transform::from_xyz(0.0, 0.255, 0.473).looking_to(FORWARD - Vec3::Y * 0.19, Vec3::Y),
        0.034,
        SpotLight {
            color,
            intensity: 2.2e7,
            range: 32.0,
            radius: 0.16,
            inner_angle: 0.19,
            outer_angle: 0.37,
            shadow_maps_enabled: true,
            ..default()
        },
    );
    let searchlight = (
        Transform::from_xyz(0.07, 0.436, 0.115).looking_to(FORWARD - Vec3::Y * 0.035, Vec3::Y),
        0.045,
        SpotLight {
            color,
            intensity: 6.0e7,
            range: 120.0,
            radius: 0.22,
            inner_angle: 0.2,
            outer_angle: 0.46,
            shadow_maps_enabled: true,
            ..default()
        },
    );
    let housing = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.19, 0.20),
        metallic: 0.55,
        perceptual_roughness: 0.56,
        ..default()
    });
    let lens = materials.add(StandardMaterial {
        emissive: LinearRgba::rgb(0.85, 0.93, 1.0) * 24_000.0,
        ..default()
    });
    vehicle.with_children(|body| {
        for (mount, size, light) in [dipped, searchlight] {
            body.spawn((
                Mesh3d(meshes.add(Cylinder::new(size, size * 1.3))),
                MeshMaterial3d(housing.clone()),
                mount.with_rotation(mount.rotation * Quat::from_rotation_x(FRAC_PI_2)),
            ));
            body.spawn((
                VehicleSpotlight,
                light,
                Visibility::Hidden,
                Transform::from_translation(mount.transform_point(Vec3::NEG_Z * size * 0.88))
                    .with_rotation(mount.rotation),
            ))
            .with_child((
                Mesh3d(meshes.add(Circle::new(size * 0.82))),
                MeshMaterial3d(lens.clone()),
                Transform::from_xyz(0.0, 0.0, 0.007).with_rotation(Quat::from_rotation_y(PI)),
                NotShadowCaster,
            ));
        }
    });
}

pub(super) fn sync_spotlight(
    driver: Query<&Driving, With<Player>>,
    mut spotlights: Query<(&ChildOf, &mut Visibility), With<VehicleSpotlight>>,
) {
    let occupied = driver.single().ok().map(|driver| driver.0);
    for (vehicle, mut visibility) in &mut spotlights {
        let next = if Some(vehicle.parent()) == occupied {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(next);
    }
}

fn attach_wheels(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    wheels: Query<(&Name, &Transform)>,
) {
    for entity in children.iter_descendants(ready.entity) {
        let Ok((name, transform)) = wheels.get(entity) else {
            continue;
        };
        let Some(slot) = wheel_slot(name) else {
            continue;
        };
        commands.entity(entity).insert(Wheel {
            vehicle: ready.entity,
            slot,
            rest: transform.translation,
        });
    }
}

pub(crate) fn vehicle_distance(at: &Transform, point: Vec3) -> f32 {
    let offset = at.rotation.inverse() * (point - at.translation);
    if offset.y.abs() >= 1.2 {
        return f32::INFINITY;
    }
    (offset.xz().abs() - HALF_EXTENTS).max(Vec2::ZERO).length()
}

pub(crate) fn can_enter_vehicle(locomotion: &Locomotion, at: &Transform, point: Vec3) -> bool {
    locomotion.grounded && vehicle_distance(at, point) <= ENTRY_REACH
}

pub(super) fn interact_vehicle(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<Terrain>,
    colliders: Solids,
    mut vehicles: Query<(Entity, &mut Vehicle, &Transform), Without<Player>>,
    mut players: Driver,
    mut cameras: Query<&mut OrbitCamera>,
) {
    if !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Ok((player, mut feet, mut locomotion, mut visibility, driving)) = players.single_mut()
    else {
        return;
    };
    if let Some(driving) = driving {
        let Ok((entity, mut vehicle, at)) = vehicles.get_mut(driving.0) else {
            return;
        };
        if vehicle.grounded
            && let Some(exit) = exit_position(at, &terrain, |point| {
                push_out(
                    point.xz(),
                    point.y,
                    STANDING,
                    other_colliders(&colliders, entity, STALKER),
                ) == point.xz()
            })
        {
            vehicle.park();
            feet.translation = exit;
            *locomotion = grounded_idle();
            *visibility = Visibility::Inherited;
            commands.entity(player).remove::<Driving>();
            if let Ok(mut orbit) = cameras.single_mut() {
                orbit.cancel_assist();
            }
        }
    } else if let Some((entity, _, at)) = vehicles
        .iter()
        .filter(|(_, _, at)| can_enter_vehicle(&locomotion, at, feet.translation))
        .min_by(|(_, _, a), (_, _, b)| {
            vehicle_distance(a, feet.translation).total_cmp(&vehicle_distance(b, feet.translation))
        })
    {
        *locomotion = grounded_idle();
        *visibility = Visibility::Hidden;
        commands.entity(player).insert(Driving(entity));
        if let Ok(mut orbit) = cameras.single_mut() {
            let forward = at.rotation * FORWARD;
            orbit.face_vehicle(forward);
        }
    }
}

fn grounded_idle() -> Locomotion {
    Locomotion {
        grounded: true,
        ..default()
    }
}

fn exit_position(at: &Transform, terrain: &Terrain, clear: impl Fn(Vec3) -> bool) -> Option<Vec3> {
    let (side, end) = (HALF_EXTENTS.x, HALF_EXTENTS.y);
    let heading = contact::heading(at.rotation);
    [
        Vec3::X * side,
        Vec3::NEG_X * side,
        -FORWARD * end,
        FORWARD * end,
    ]
    .into_iter()
    .find_map(|edge| {
        let sample = |reach: f32| {
            let mut point = at.translation + heading * (edge + edge.normalize() * reach);
            point.y = terrain.mesh_height_at(point.xz());
            point
        };
        let exit = sample(0.9);
        (exit.xz().abs().max_element() < TERRAIN_HALF_SIZE - super::EDGE_MARGIN
            && (exit.y - at.translation.y).abs() <= 1.2
            && terrain.water_depth_at(exit.x, exit.z) <= 0.75
            && clear(sample(STANDING.radius))
            && clear(exit))
        .then_some(exit)
    })
}

pub(crate) fn drive_vehicle(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<Terrain>,
    colliders: Solids,
    mut vehicles: Query<(Entity, &mut Vehicle, &mut Transform), Without<Player>>,
    mut players: Query<(&Driving, &mut Transform, &mut MoveOrder), With<Player>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let mut driver = players.single_mut().ok();
    let dt = time.delta_secs();
    let manual = keys.any_pressed([
        KeyCode::KeyW,
        KeyCode::KeyS,
        KeyCode::KeyA,
        KeyCode::KeyD,
        KeyCode::Space,
    ]);
    for (entity, mut vehicle, mut at) in &mut vehicles {
        let mut order = driver
            .as_mut()
            .filter(|(driving, ..)| driving.0 == entity)
            .map(|(_, _, order)| order);
        let occupied = order.is_some();
        if manual && let Some(order) = order.as_mut() {
            order.cancel();
        }
        let autopilot = order
            .as_mut()
            .and_then(|order| autopilot(order, &at, vehicle.speed));
        let input = autopilot.unwrap_or(DriveInput {
            brake: !occupied || keys.pressed(KeyCode::Space),
            throttle: if occupied {
                axis(&keys, KeyCode::KeyW, KeyCode::KeyS)
            } else {
                0.0
            },
            steering: if occupied {
                axis(&keys, KeyCode::KeyA, KeyCode::KeyD)
            } else {
                0.0
            },
        });
        let from = at.translation.xz();
        // Bound travel per sweep even at the higher top speed and when sliding.
        let predicted_speed =
            vehicle.speed.abs() + vehicle.slide.length() + (DRIVE_ACCELERATION + GRAVITY) * dt;
        let steps = (dt * (1.0 / DRIVE_STEP).max(predicted_speed / MAX_STEP_TRAVEL)).ceil() as u32;
        let mut turned = 0.0;
        for _ in 0..steps {
            turned += drive_step(
                &mut vehicle,
                &mut at,
                &terrain,
                input,
                dt / steps as f32,
                |next| blocked(next, other_colliders(&colliders, entity, CAR)),
            );
        }
        if let Some(order) = order.as_mut() {
            order.track_progress(
                at.translation.xz(),
                at.translation.xz().distance(from),
                AUTOPILOT_MIN_SPEED * dt,
                dt,
                Turning::Reversing,
            );
        }
        if occupied && let Some((_, player, _)) = &mut driver {
            player.translation = at.translation;
            // Point-and-click driving keeps the view steady instead of swinging behind the hull.
            if autopilot.is_none()
                && let Ok(mut orbit) = cameras.single_mut()
            {
                orbit.record_turn(turned);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct DriveInput {
    throttle: f32,
    steering: f32,
    brake: bool,
}

/// Slower progress than this counts as blocked for a point-and-click order.
const AUTOPILOT_MIN_SPEED: f32 = 3.0;
/// Throttle the autopilot backs away from an obstacle with: full, so the car
/// clears its back-off distance in about two seconds.
const AUTOPILOT_REVERSE: f32 = 1.0;
/// Deceleration assumed when planning the stop at the target, in m/s².
const AUTOPILOT_BRAKING: f32 = 8.0;

/// Steers toward the next waypoint of a point-and-click order, easing off for
/// sharp turns and braking in time to stop at the target. After a stall it
/// first backs away, swinging the nose toward the new leg.
fn autopilot(order: &mut MoveOrder, at: &Transform, speed: f32) -> Option<DriveInput> {
    let position = at.translation.xz();
    let toward = order.steer(position)?;
    let forward = (contact::heading(at.rotation) * FORWARD)
        .xz()
        .normalize_or_zero();
    let angle = (forward.y * toward.x - forward.x * toward.y).atan2(forward.dot(toward));
    if order.backing_off() {
        // Reversing turns the hull the other way for the same wheel angle.
        return Some(DriveInput {
            throttle: -AUTOPILOT_REVERSE,
            steering: (-angle * 2.2).clamp(-1.0, 1.0),
            brake: false,
        });
    }
    let remaining = order.remaining(position);
    let stopping = speed * speed / (2.0 * AUTOPILOT_BRAKING);
    let brake = remaining < stopping + 1.0;
    let corner = if angle.abs() > 1.2 { 0.45 } else { 1.0 };
    Some(DriveInput {
        throttle: if brake {
            0.0
        } else {
            (remaining / 25.0).clamp(0.3, 1.0) * corner
        },
        steering: (angle * 2.2).clamp(-1.0, 1.0),
        brake,
    })
}

fn drive_step(
    vehicle: &mut Vehicle,
    at: &mut Transform,
    terrain: &Terrain,
    input: DriveInput,
    dt: f32,
    collides: impl Fn(&Transform) -> bool,
) -> f32 {
    let from = at.translation.xz();
    let yaw = contact::heading(at.rotation);
    let sample = |center, heading| contact::sample(center, heading, |p| terrain.mesh_height_at(p));
    let contact = sample(from, yaw);
    let grounded = vehicle.grounded && at.translation.y <= contact.height + CONTACT_SNAP;
    if grounded {
        apply_traction(
            vehicle,
            terrain,
            from,
            yaw * FORWARD,
            contact.up(),
            input,
            dt,
        );
    }
    let turn = if grounded {
        vehicle.steer.tan() * vehicle.speed * dt / WHEELBASE
    } else {
        0.0
    };
    let pushing = vehicle.speed != 0.0 || vehicle.slide != Vec2::ZERO;
    // The hull after a step turning it by `turn`, unless a bank too steep to
    // grip, the map's edge, deep water or a solid stops it there.
    let step = |turn: f32| {
        let heading = Quat::from_rotation_y(turn) * yaw;
        let delta = ((heading * FORWARD).xz() * vehicle.speed + vehicle.slide) * dt;
        let to = from + delta;
        let destination = sample(to, heading);
        let climbs_bank =
            grounded && destination.height - contact.height > MAX_GRIP_GRADE * delta.length();
        if !pushing
            || climbs_bank
            || to.abs().max_element() > TERRAIN_HALF_SIZE - HALF_EXTENTS.length()
            || !terrain.can_wade(from, to, FORD_DEPTH)
        {
            return None;
        }
        let mut next = *at;
        next.translation.x = to.x;
        next.translation.z = to.y;
        let settled = settle(
            &mut next,
            at,
            &destination,
            vehicle.vertical_speed,
            grounded,
            turn,
            dt,
        );
        (!collides(&next)).then_some((next, destination, settled, turn))
    };
    // A turning hull can swing a corner into a rock (or up a bank) it could
    // still back or pull straight away from: then the step goes without the
    // turn.
    let stepped = step(turn).or_else(|| (turn != 0.0).then(|| step(0.0)).flatten());
    let moved = stepped.is_some();
    let (next, support, (vy, landed), turn) = stepped.unwrap_or_else(|| {
        let mut next = *at;
        let settled = settle(
            &mut next,
            at,
            &contact,
            vehicle.vertical_speed,
            grounded,
            0.0,
            dt,
        );
        (next, contact, settled, 0.0)
    });
    if !moved && pushing {
        vehicle.speed = 0.0;
        vehicle.slide = Vec2::ZERO;
    }
    if grounded && landed {
        let distance = if moved {
            let forward = (next.rotation * FORWARD).xz();
            (next.translation.xz() - from).dot(forward) / forward.length_squared()
        } else if !input.brake {
            vehicle.throttle * BLOCKED_WHEEL_SPEED * dt
        } else {
            0.0
        };
        vehicle.roll = (vehicle.roll + distance / WHEEL_RADIUS).rem_euclid(TAU);
    }
    vehicle.grounded = landed;
    vehicle.vertical_speed = vy;
    vehicle.suspension = if landed {
        support.suspension()
    } else {
        [Vec3::ZERO; 4]
    };
    *at = next;
    turn
}

fn apply_traction(
    vehicle: &mut Vehicle,
    terrain: &Terrain,
    at: Vec2,
    forward: Vec3,
    up: Vec3,
    input: DriveInput,
    dt: f32,
) {
    apply_drive_controls(
        vehicle,
        input,
        terrain.water_depth_at(at.x, at.y),
        terrain.road_mask(at.x, at.y),
        dt,
    );

    let downhill = up.xz();
    if downhill.length() > MAX_GRIP_GRADE * up.y && downhill.dot(forward.xz()) * vehicle.speed < 0.0
    {
        vehicle.speed = 0.0;
    }
    // Resolve gravity and available tire friction along the ground plane.
    vehicle.slide = (vehicle.slide + downhill * (GRAVITY * up.y * dt))
        .move_towards(Vec2::ZERO, MAX_GRIP_GRADE * GRAVITY * up.y * up.y * dt)
        .clamp_length_max(12.0);
}

fn apply_drive_controls(vehicle: &mut Vehicle, input: DriveInput, depth: f32, road: f32, dt: f32) {
    let reversing = input.throttle * vehicle.speed < 0.0;
    let pedal = if input.brake || reversing {
        0.0
    } else {
        input.throttle
    };
    if pedal == 0.0 || pedal * vehicle.throttle < 0.0 {
        vehicle.throttle = 0.0;
    }
    vehicle.throttle += (pedal - vehicle.throttle) * response(9.0, dt);

    let water = depth.min(FORD_DEPTH);
    // Drop to the original speed range in wadable water.
    let wading = (water / 0.25).clamp(0.0, 1.0);
    vehicle.road = road.clamp(0.0, 1.0);
    let dry_speed = FORWARD_SPEED + (ROAD_FORWARD_SPEED - FORWARD_SPEED) * vehicle.road;
    let top_speed = if pedal >= 0.0 {
        dry_speed + (LOW_RANGE_SPEED - dry_speed) * wading
    } else {
        REVERSE_SPEED
    };
    let target = pedal * top_speed * (1.0 - 0.60 * water);
    let acceleration = if input.brake {
        18.0
    } else if reversing {
        8.0
    } else if pedal == 0.0 || vehicle.speed.abs() > target.abs() {
        3.5
    } else {
        DRIVE_ACCELERATION * vehicle.throttle.abs()
    };
    let error = target - vehicle.speed;
    let step = if pedal != 0.0 && error.abs() >= 0.02 {
        error * response(5.5, dt)
    } else {
        error
    };
    let limit = acceleration * dt;
    vehicle.speed += step.clamp(-limit, limit);

    let speed = vehicle.speed.abs();
    let fast = (speed / LOW_RANGE_SPEED).min(1.0);
    let maximum = if speed <= LOW_RANGE_SPEED {
        MAX_STEER + (FAST_STEER - MAX_STEER) * fast
    } else {
        // Keep a full steering input from increasing the old maximum turn rate.
        (FAST_STEER.tan() * LOW_RANGE_SPEED / speed).atan()
    };
    let target_steer = input.steering * maximum;
    let steer_error = target_steer - vehicle.steer;
    if steer_error.abs() < 0.002 {
        vehicle.steer = target_steer;
    } else {
        vehicle.steer += steer_error * response(8.5 - fast * 2.5, dt);
    }
}

fn settle(
    next: &mut Transform,
    from: &Transform,
    support: &contact::Contact,
    vy: f32,
    grounded: bool,
    turn: f32,
    dt: f32,
) -> (f32, bool) {
    let prior_up = from.rotation * Vec3::Y;
    let travel = next.translation.xz() - from.translation.xz();
    let vy = if grounded {
        -prior_up.xz().dot(travel) / (prior_up.y * dt)
    } else {
        vy
    };
    let (fall_y, vy) = freefall(from.translation.y, vy, dt);
    if fall_y <= support.height + CONTACT_SNAP {
        next.translation.y = support.height;
        if travel != Vec2::ZERO || from.rotation.dot(support.rotation).abs() < 0.9999999 {
            next.rotation = support.rotation;
        }
        (0.0, true)
    } else {
        next.translation.y = fall_y;
        next.rotation = Quat::from_rotation_y(turn) * from.rotation;
        (vy, false)
    }
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f32 {
    f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative))
}

/// The solids that stop `body`, leaving out the vehicle itself.
fn other_colliders<'a>(
    colliders: &'a Solids,
    vehicle: Entity,
    body: Crusher,
) -> impl Iterator<Item = (&'a Collider, &'a GlobalTransform)> {
    colliders
        .iter()
        .filter(move |&(entity, _, _, weight)| entity != vehicle && body.blocked_by(weight))
        .map(|(_, collider, transform, _)| (collider, transform))
}

pub(crate) fn blocked<'a>(
    at: &Transform,
    colliders: impl Iterator<Item = (&'a Collider, &'a GlobalTransform)>,
) -> bool {
    let heading = contact::heading(at.rotation);
    let world_to_vehicle = heading.inverse();
    let up = at.rotation * Vec3::Y;
    colliders.into_iter().any(|(collider, transform)| {
        let (scale, _, _) = transform.to_scale_rotation_translation();
        collider.0.iter().any(|pillar| {
            let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let local = (world_to_vehicle * (center - at.translation)).xz();
            if (local.abs() - HALF_EXTENTS)
                .max(Vec2::ZERO)
                .length_squared()
                >= (pillar.radius * scale.x).powi(2)
            {
                return false;
            }
            let p = local.clamp(-HALF_EXTENTS, HALF_EXTENTS);
            let offset = heading * Vec3::new(p.x, 0.0, p.y);
            let base = at.translation.y - up.dot(offset) / up.y;
            let top = center.y + pillar.top * scale.y;
            let bottom = center.y + pillar.bottom * scale.y;
            top > base + STEP_HEIGHT && bottom < base + HULL_HEIGHT
        })
    })
}

/// The suspension slot of a wheel pivot in the car's model, front wheels first.
fn wheel_slot(name: &Name) -> Option<usize> {
    match name.as_str() {
        "WheelFrontRight" => Some(0),
        "WheelFrontLeft" => Some(1),
        "WheelRearRight" => Some(2),
        "WheelRearLeft" => Some(3),
        _ => None,
    }
}

/// Springs a wheel pivot from its rest position, rolls it, and steers it if
/// it is a front wheel.
fn pose_wheel(transform: &mut Transform, slot: usize, rest: Vec3, gear: (f32, f32, [Vec3; 4])) {
    let (steer, roll, suspension) = gear;
    let steer = if slot < 2 { steer } else { 0.0 };
    transform.translation = rest + suspension[slot];
    transform.rotation = Quat::from_rotation_y(steer) * Quat::from_rotation_x(roll);
}

pub(super) fn pose_wheels(vehicles: Query<&Vehicle>, mut wheels: Query<(&Wheel, &mut Transform)>) {
    for (wheel, mut transform) in &mut wheels {
        let Ok(vehicle) = vehicles.get(wheel.vehicle) else {
            continue;
        };
        let gear = (vehicle.steer, vehicle.roll, vehicle.suspension);
        pose_wheel(&mut transform, wheel.slot, wheel.rest, gear);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::world::{BASE, SWAMP};

    #[test]
    fn road_boost_preserves_steering_ford_limits_and_reverse_speed() {
        let forward = DriveInput {
            throttle: 1.0,
            steering: 1.0,
            brake: false,
        };
        let mut dry = Vehicle::default();
        let mut road = Vehicle::default();
        let mut ford = Vehicle::default();
        for _ in 0..250 {
            apply_drive_controls(&mut dry, forward, 0.0, 0.0, 0.02);
            apply_drive_controls(&mut road, forward, 0.0, 1.0, 0.02);
            apply_drive_controls(&mut ford, forward, 0.32, 1.0, 0.02);
        }
        assert!((15.9..=16.0).contains(&dry.speed));
        assert!((23.9..=24.0).contains(&road.speed));
        assert!((8.0..8.2).contains(&ford.speed));
        assert!((0.99..=1.0).contains(&road.engine_load()));
        let turn_rate = road.steer.tan() * road.speed / WHEELBASE;
        assert!((0.85..1.0).contains(&turn_rate));

        let mut reverse = Vehicle::default();
        for _ in 0..150 {
            apply_drive_controls(
                &mut reverse,
                DriveInput {
                    throttle: -1.0,
                    ..forward
                },
                0.0,
                1.0,
                0.02,
            );
        }
        assert!((-4.0..-3.95).contains(&reverse.speed));
    }

    #[test]
    fn live_terrain_road_mask_selects_the_faster_forward_speed() {
        let terrain = Terrain::new();
        let off_road = Vec2::new(BASE.0, BASE.1);
        // On the drive north from the yard gate.
        let road = Vec2::new(BASE.0 - 1.0, BASE.1 + 120.0);
        assert!(terrain.road_mask(off_road.x, off_road.y) < 0.05);
        assert!(terrain.road_mask(road.x, road.y) > 0.99);

        let input = DriveInput {
            throttle: 1.0,
            steering: 0.0,
            brake: false,
        };
        let mut dirt = Vehicle::default();
        let mut paved = Vehicle::default();
        for _ in 0..250 {
            apply_traction(&mut dirt, &terrain, off_road, Vec3::Z, Vec3::Y, input, 0.02);
            apply_traction(&mut paved, &terrain, road, Vec3::Z, Vec3::Y, input, 0.02);
        }
        assert!((15.9..=16.0).contains(&dirt.speed));
        assert!((23.9..=24.0).contains(&paved.speed));
    }

    #[test]
    fn throttle_eases_in_and_fast_steering_stays_gentle() {
        let terrain = Terrain::new();
        let origin = Vec2::new(BASE.0, BASE.1);
        let support = contact::sample(origin, Quat::IDENTITY, |at| terrain.mesh_height_at(at));
        let mut at =
            Transform::from_xyz(origin.x, support.height, origin.y).with_rotation(support.rotation);
        let mut vehicle = Vehicle::default();
        let input = DriveInput {
            throttle: 1.0,
            steering: 0.0,
            brake: false,
        };
        drive_step(&mut vehicle, &mut at, &terrain, input, 0.02, |_| false);
        assert!(
            (0.005..0.08).contains(&vehicle.speed),
            "abrupt launch: {}",
            vehicle.speed
        );
        for _ in 0..105 {
            drive_step(&mut vehicle, &mut at, &terrain, input, 0.02, |_| false);
        }

        assert!(vehicle.speed > 9.0, "throttle never built up");
        let turn = DriveInput {
            steering: 1.0,
            ..input
        };
        drive_step(&mut vehicle, &mut at, &terrain, turn, 0.02, |_| false);
        assert!(vehicle.steer > 0.0 && vehicle.steer < 0.046);
        for _ in 0..20 {
            drive_step(&mut vehicle, &mut at, &terrain, turn, 0.02, |_| false);
        }
        assert!(
            (0.17..0.32).contains(&vehicle.steer),
            "steering at speed: {}",
            vehicle.steer
        );
        let steer = vehicle.steer;
        drive_step(&mut vehicle, &mut at, &terrain, input, 0.02, |_| false);
        assert!((0.7 * steer..steer).contains(&vehicle.steer));

        let mut parked = Vehicle::default();
        let turn_in_place = DriveInput {
            throttle: 0.0,
            ..turn
        };
        for _ in 0..25 {
            apply_drive_controls(&mut parked, turn_in_place, 0.0, 0.0, 0.02);
        }
        assert!(parked.steer > 0.44, "lost low-speed steering lock");
    }

    #[test]
    fn switching_direction_stops_before_reversing_and_the_brake_takes_priority() {
        let mut vehicle = Vehicle {
            speed: 5.0,
            throttle: 1.0,
            ..default()
        };
        let reverse = DriveInput {
            throttle: -1.0,
            steering: 0.0,
            brake: false,
        };
        apply_drive_controls(&mut vehicle, reverse, 0.0, 0.0, 0.02);
        assert!(vehicle.speed > 4.7);
        let mut stopped = false;
        for _ in 0..54 {
            apply_drive_controls(&mut vehicle, reverse, 0.0, 0.0, 0.02);
            stopped |= vehicle.speed == 0.0;
        }
        assert!(stopped && vehicle.speed < -1.0);
        let pre_brake = vehicle.speed;
        let brake = DriveInput {
            brake: true,
            ..reverse
        };
        apply_drive_controls(&mut vehicle, brake, 0.0, 0.0, 0.02);
        assert!(vehicle.speed - pre_brake >= 0.35);
        for _ in 0..20 {
            apply_drive_controls(&mut vehicle, brake, 0.0, 0.0, 0.02);
        }
        assert_eq!(vehicle.speed, 0.0);
    }

    #[test]
    fn throttle_spins_blocked_wheels_in_either_direction_but_braking_stops_them() {
        let terrain = Terrain::new();
        let origin = Vec2::new(BASE.0, BASE.1);
        let support = contact::sample(origin, Quat::IDENTITY, |at| terrain.mesh_height_at(at));
        let rest =
            Transform::from_xyz(origin.x, support.height, origin.y).with_rotation(support.rotation);
        for throttle in [-1.0, 1.0] {
            let mut at = rest;
            let mut vehicle = Vehicle::default();
            for _ in 0..10 {
                let before = vehicle.roll;
                drive_step(
                    &mut vehicle,
                    &mut at,
                    &terrain,
                    DriveInput {
                        throttle,
                        steering: 0.0,
                        brake: false,
                    },
                    0.02,
                    |_| true,
                );
                let spin = (vehicle.roll - before + PI).rem_euclid(TAU) - PI;
                assert!(spin * throttle > 0.005);
                assert_eq!(at.translation, rest.translation);
                assert_eq!(vehicle.speed, 0.0);
            }
            let before = vehicle.roll;
            for brake in [true, false] {
                drive_step(
                    &mut vehicle,
                    &mut at,
                    &terrain,
                    DriveInput {
                        throttle: if brake { throttle } else { 0.0 },
                        steering: 0.0,
                        brake,
                    },
                    0.02,
                    |_| true,
                );
                assert_eq!(vehicle.roll, before);
            }
        }
    }

    #[test]
    fn enters_drives_and_exits_with_one_press_each() {
        let flat = Vec2::new(BASE.0, BASE.1);
        let terrain = Terrain::new();
        let floor = terrain.height_at(flat.x, flat.y);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(terrain)
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(
                Update,
                (interact_vehicle, drive_vehicle, pose_wheels, sync_spotlight).chain(),
            );
        let original = Transform::from_xyz(flat.x, floor + GROUND_CLEARANCE, flat.y);
        let vehicle = app.world_mut().spawn((Vehicle::default(), original)).id();
        let spotlight = app
            .world_mut()
            .spawn((VehicleSpotlight, Visibility::Hidden, ChildOf(vehicle)))
            .id();
        let wheel = app
            .world_mut()
            .spawn((
                Transform::default(),
                Wheel {
                    vehicle,
                    slot: 0,
                    rest: Vec3::ZERO,
                },
            ))
            .id();
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                grounded_idle(),
                Transform::from_xyz(flat.x + 10.0, floor, flat.y),
                Visibility::Inherited,
            ))
            .id();

        press(&mut app, KeyCode::KeyE);
        app.update();
        assert!(app.world().get::<Driving>(player).is_none());
        assert_eq!(
            app.world().get::<Visibility>(spotlight),
            Some(&Visibility::Hidden)
        );
        release(&mut app, KeyCode::KeyE);
        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x = flat.x + HALF_EXTENTS.x + 0.5;

        press(&mut app, KeyCode::KeyE);
        app.update();
        assert_eq!(
            app.world().get::<Driving>(player).map(|d| d.0),
            Some(vehicle)
        );
        assert_eq!(
            app.world().get::<Visibility>(player),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(spotlight),
            Some(&Visibility::Inherited)
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear_just_pressed(KeyCode::KeyE);
        press(&mut app, KeyCode::KeyW);
        press(&mut app, KeyCode::KeyA);
        for _ in 0..15 {
            app.update();
        }
        let driven = *app.world().get::<Transform>(vehicle).expect("vehicle");
        assert!(driven.translation.z > original.translation.z + 1.0);
        assert!(driven.translation.x > original.translation.x);
        assert_ne!(
            app.world().get::<Transform>(wheel).expect("wheel").rotation,
            Quat::IDENTITY
        );
        assert_eq!(
            app.world()
                .get::<Transform>(player)
                .expect("player")
                .translation,
            driven.translation
        );
        release(&mut app, KeyCode::KeyW);
        release(&mut app, KeyCode::KeyA);
        release(&mut app, KeyCode::KeyE);
        press(&mut app, KeyCode::KeyE);
        app.update();
        assert!(app.world().get::<Driving>(player).is_none());
        assert_eq!(
            app.world().get::<Visibility>(player),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            app.world().get::<Visibility>(spotlight),
            Some(&Visibility::Hidden)
        );
        let feet = app
            .world()
            .get::<Transform>(player)
            .expect("player")
            .translation;
        assert!(vehicle_distance(&driven, feet) >= STANDING.radius);
        assert_eq!(app.world().get::<Transform>(vehicle), Some(&driven));
    }

    #[test]
    fn an_unsupported_vehicle_falls_on_earth_even_when_unoccupied() {
        let mut app = App::new();
        let terrain = Terrain::new();
        let ground = terrain.height_at(BASE.0, BASE.1);
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(terrain)
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, drive_vehicle);
        let car = app
            .world_mut()
            .spawn((
                Vehicle::default(),
                Transform::from_xyz(BASE.0, ground + 8.0, BASE.1),
            ))
            .id();
        app.update();
        for _ in 0..10 {
            app.update();
        }
        let height = app
            .world()
            .get::<Transform>(car)
            .expect("falling car")
            .translation
            .y
            - ground;
        assert!(
            (height - (8.0 - 0.5 * 9.81 * 0.5_f32.powi(2))).abs() < 0.015,
            "car is {height} m above ground"
        );
        for _ in 0..30 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(car)
            .expect("landed car")
            .translation;
        assert!((at.y - ground - GROUND_CLEARANCE).abs() < 0.005);
    }

    #[test]
    fn throttle_and_brakes_cannot_hold_the_car_on_a_mountain_wall() {
        let terrain = Terrain::new();
        // The north face of the mountain south of the pass.
        let from = Vec2::new(-800.0, -290.0);
        let yaw = Quat::from_rotation_y(PI);
        let support = contact::sample(from, yaw, |at| terrain.mesh_height_at(at));
        assert!(support.up().z > MAX_GRIP_GRADE * support.up().y);
        for brake in [false, true] {
            let mut at =
                Transform::from_xyz(from.x, support.height, from.y).with_rotation(support.rotation);
            let mut vehicle = Vehicle::default();
            let input = DriveInput {
                throttle: 1.0,
                steering: 0.0,
                brake,
            };
            for _ in 0..50 {
                drive_step(&mut vehicle, &mut at, &terrain, input, 0.02, |_| false);
            }
            assert!(
                at.translation.z > from.y + 0.7,
                "car clung to the wall at {at:?}"
            );
            assert!(at.translation.y < support.height - 0.7);
        }
    }

    #[test]
    fn point_and_click_drives_around_to_the_target_and_stops_there() {
        use crate::player::orders::VEHICLE_ARRIVAL;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, drive_vehicle);
        let terrain = Terrain::new();
        let (x, z) = (40.0, -40.0);
        let vehicle = app
            .world_mut()
            .spawn((
                Vehicle::default(),
                Transform::from_xyz(x, terrain.height_at(x, z), z),
            ))
            .id();
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::default(), Driving(vehicle)))
            .id();
        // The hull faces +Z; the target lies off to its right, behind a turn.
        let target = Vec2::new(x + 30.0, z + 20.0);
        app.world_mut()
            .get_mut::<MoveOrder>(player)
            .expect("order")
            .give(vec![target], Vec2::new(x, z), false, VEHICLE_ARRIVAL);
        for _ in 0..400 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(vehicle)
            .expect("car")
            .translation;
        assert!(
            at.xz().distance(target) < VEHICLE_ARRIVAL + 2.0,
            "car stopped at {at}, target {target}"
        );
        assert!(
            app.world()
                .get::<Vehicle>(vehicle)
                .expect("car")
                .speed
                .abs()
                < 1.0
        );
        let mut order = app.world_mut().get_mut::<MoveOrder>(player).expect("order");
        assert_eq!(order.steer(at.xz()), None, "arrival clears the order");
    }

    #[test]
    fn a_boxed_in_car_backs_and_fills_its_way_out() {
        use crate::player::orders::{VEHICLE_ARRIVAL, replan_order};
        use crate::world::{Collider, Pillar};

        static BOULDER: [Pillar; 1] = [Pillar {
            x: 0.0,
            z: 0.0,
            radius: 2.5,
            bottom: -2.0,
            top: 3.0,
        }];
        const fn post(x: f32) -> Pillar {
            Pillar {
                x,
                z: 0.0,
                radius: 0.6,
                bottom: -2.0,
                top: 3.0,
            }
        }
        static WALL: [Pillar; 13] = [
            post(-6.0),
            post(-5.0),
            post(-4.0),
            post(-3.0),
            post(-2.0),
            post(-1.0),
            post(0.0),
            post(1.0),
            post(2.0),
            post(3.0),
            post(4.0),
            post(5.0),
            post(6.0),
        ];

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (replan_order, drive_vehicle).chain());
        let terrain = Terrain::new();
        let (x, z) = (40.0, -40.0);
        let vehicle = app
            .world_mut()
            .spawn((
                Vehicle::default(),
                Transform::from_xyz(x, terrain.height_at(x, z), z),
            ))
            .id();
        // A boulder 0.3 m off the nose and a wall about a meter behind the tail.
        let half_length = HALF_EXTENTS.y;
        for (collider, dz) in [
            (&BOULDER[..], half_length + 0.3 + 2.5),
            (&WALL[..], -(half_length + 1.0 + 0.6)),
        ] {
            let place = Transform::from_xyz(x, terrain.height_at(x, z + dz), z + dz);
            app.world_mut()
                .spawn((Collider(collider), place, GlobalTransform::from(place)));
        }
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::default(), Driving(vehicle)))
            .id();
        // Planned straight through the boulder, as if it had not streamed in.
        let target = Vec2::new(x, z + 25.0);
        app.world_mut()
            .get_mut::<MoveOrder>(player)
            .expect("order")
            .give(vec![target], Vec2::new(x, z), false, VEHICLE_ARRIVAL);
        for _ in 0..2400 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(vehicle)
            .expect("car")
            .translation;
        assert!(
            at.xz().distance(target) < VEHICLE_ARRIVAL + 2.0,
            "car stopped at {at}, target {target}"
        );
    }

    #[test]
    fn a_car_finds_its_way_through_three_unplanned_boulders() {
        use crate::player::orders::{VEHICLE_ARRIVAL, replan_order};
        use crate::world::{Collider, Pillar};

        static BOULDER: [Pillar; 1] = [Pillar {
            x: 0.0,
            z: 0.0,
            radius: 2.5,
            bottom: -2.0,
            top: 3.0,
        }];

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (replan_order, drive_vehicle).chain());
        let terrain = Terrain::new();
        let (x, z) = (40.0, -40.0);
        let vehicle = app
            .world_mut()
            .spawn((
                Vehicle::default(),
                Transform::from_xyz(x, terrain.height_at(x, z), z),
            ))
            .id();
        // Staggered across the way: dead ahead, then off either side of it.
        for (dx, dz) in [(0.0, 10.0), (-5.0, 20.0), (5.0, 30.0)] {
            let (bx, bz) = (x + dx, z + dz);
            let boulder = Transform::from_xyz(bx, terrain.height_at(bx, bz), bz);
            app.world_mut()
                .spawn((Collider(&BOULDER), boulder, GlobalTransform::from(boulder)));
        }
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::default(), Driving(vehicle)))
            .id();
        // Planned straight through them, as for a leg whose scenery had not
        // streamed in yet. The hull faces +Z, at the first boulder.
        let target = Vec2::new(x, z + 45.0);
        app.world_mut()
            .get_mut::<MoveOrder>(player)
            .expect("order")
            .give(vec![target], Vec2::new(x, z), false, VEHICLE_ARRIVAL);
        for _ in 0..1200 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(vehicle)
            .expect("car")
            .translation;
        assert!(
            at.xz().distance(target) < VEHICLE_ARRIVAL + 2.0,
            "car stopped at {at}, target {target}"
        );
    }

    #[test]
    fn a_car_blocked_by_an_unplanned_wall_backs_off_and_drives_around_it() {
        use crate::player::orders::{VEHICLE_ARRIVAL, replan_order};
        use crate::world::{Collider, Pillar};

        const fn post(x: f32) -> Pillar {
            Pillar {
                x,
                z: 0.0,
                radius: 0.6,
                bottom: -2.0,
                top: 3.0,
            }
        }
        // Twelve meters of wall across the car's way, posts a meter apart.
        static WALL: [Pillar; 13] = [
            post(-6.0),
            post(-5.0),
            post(-4.0),
            post(-3.0),
            post(-2.0),
            post(-1.0),
            post(0.0),
            post(1.0),
            post(2.0),
            post(3.0),
            post(4.0),
            post(5.0),
            post(6.0),
        ];

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (replan_order, drive_vehicle).chain());
        let terrain = Terrain::new();
        let (x, z) = (40.0, -40.0);
        let vehicle = app
            .world_mut()
            .spawn((
                Vehicle::default(),
                Transform::from_xyz(x, terrain.height_at(x, z), z),
            ))
            .id();
        let wall = Transform::from_xyz(x, terrain.height_at(x, z + 9.0), z + 9.0);
        app.world_mut()
            .spawn((Collider(&WALL), wall, GlobalTransform::from(wall)));
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::default(), Driving(vehicle)))
            .id();
        // Planned straight through the wall, as for a leg whose scenery had
        // not streamed in yet. The hull faces +Z, at the wall.
        let target = Vec2::new(x, z + 25.0);
        app.world_mut()
            .get_mut::<MoveOrder>(player)
            .expect("order")
            .give(vec![target], Vec2::new(x, z), false, VEHICLE_ARRIVAL);
        for _ in 0..1200 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(vehicle)
            .expect("car")
            .translation;
        assert!(
            at.xz().distance(target) < VEHICLE_ARRIVAL + 2.0,
            "car stopped at {at}, target {target}"
        );
    }

    /// The deepest pool of the marsh, and dry land due south of it.
    fn deep_pool_and_its_south_shore(terrain: &Terrain) -> (Vec2, Vec2) {
        let depth = |at: Vec2| terrain.water_depth_at(at.x, at.y);
        let pool = (-30..=30)
            .flat_map(|i| (-30..=30).map(move |j| SWAMP + Vec2::new(i as f32, j as f32) * 6.0))
            .max_by(|a, b| depth(*a).total_cmp(&depth(*b)))
            .expect("the marsh has pools");
        let shore = (1..)
            .map(|step| pool - Vec2::Y * step as f32 * 2.0)
            .find(|&at| (0..8).all(|k| depth(at - Vec2::Y * k as f32 * 2.0) <= 0.0))
            .expect("dry land south of the pool");
        (pool, shore - Vec2::Y * 12.0)
    }

    #[test]
    fn drives_into_the_marsh_but_stops_at_a_deep_pool() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, drive_vehicle);
        let terrain = Terrain::new();
        let start = |x: f32, z: f32| Transform::from_xyz(x, terrain.height_at(x, z), z);
        let (pool, shore) = deep_pool_and_its_south_shore(&terrain);
        assert!(terrain.water_depth_at(pool.x, pool.y) > FORD_DEPTH + 0.3);
        let vehicle = app
            .world_mut()
            .spawn((Vehicle::default(), start(shore.x, shore.y)))
            .id();
        app.world_mut()
            .spawn((Player::default(), Transform::default(), Driving(vehicle)));
        press(&mut app, KeyCode::KeyW);
        for _ in 0..500 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(vehicle)
            .expect("at the bank")
            .translation;
        assert!(at.z > shore.y + 5.0, "car never set off: {at}");
        assert!(at.z < pool.y, "car drove into the deep pool: {at}");
        assert!(terrain.water_depth_at(at.x, at.z) <= FORD_DEPTH);
        assert_eq!(
            app.world()
                .get::<Vehicle>(vehicle)
                .expect("parked at the bank")
                .speed,
            0.0
        );
    }

    #[test]
    fn vehicle_stops_at_obstacles_but_clears_their_high_and_low_edges() {
        const SHAPE: &[Pillar] = &[Pillar {
            top: 1.0,
            ..Pillar::centered(0.35, 0.0)
        }];
        let obstacle = Collider(SHAPE);
        let at = Transform::default().with_rotation(Quat::from_rotation_y(0.5));
        let forward = at.rotation * FORWARD;
        let placed = |y, z| GlobalTransform::from_translation(forward * z + Vec3::Y * y);
        assert!(blocked(
            &at,
            [(&obstacle, &placed(0.0, HALF_EXTENTS.y + 0.2))].into_iter()
        ));
        for (height, distance) in [(-0.7, 0.0), (HULL_HEIGHT + 0.1, 0.0), (0.0, 3.2)] {
            assert!(!blocked(
                &at,
                [(&obstacle, &placed(height, distance))].into_iter()
            ));
        }
    }

    #[test]
    fn a_blocked_driver_side_exits_on_the_other_side() {
        let terrain = Terrain::new();
        let y = terrain.height_at(BASE.0, BASE.1);
        let at = Transform::from_xyz(BASE.0, y, BASE.1);
        let exit = exit_position(&at, &terrain, |point| point.x < at.translation.x);
        assert!(exit.is_some_and(|point| point.x < at.translation.x - HALF_EXTENTS.x));
        assert!(exit_position(&at, &terrain, |_| false).is_none());
    }

    fn press(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }

    fn release(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
    }
}
