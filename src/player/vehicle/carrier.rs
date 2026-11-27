//! The Institute's tracked carrier, its official vehicle into the Zone. It
//! waits in its bay on the hangar's east side, then sets out on an expedition:
//! out of the door, through the yard's boom and the Institute gate (its
//! `InstitutePass` lifts both) and along the highway into the Zone
//! (`world::expedition_route`), keeping right. It waits out there, pivots on
//! its tracks, drives home, pivots again on the apron and backs into its bay.
//! It stops for the stalker, their car, a patrol or traffic in its way; they
//! stop for it. Its road wheels, sprocket and idler are separate nodes
//! (`tools/split_moving_parts.py`) that roll with the tracks, the two sides
//! turning against each other while it pivots, and each belt is a ring of
//! copies of one scanned link (`CarrierLink`) run round the loop measured in
//! `carrier_track` as far as its track has run.
//!
//! The stalker can take it whenever it stands still, in its bay or waiting
//! out in the Zone (`steer_carrier`): it then drives on its tracks, turning on
//! the spot when stopped, and stays wherever they leave it, in the save too. Off road it fords swamp water up to `FORD` deep, climbs grades up
//! to `MAX_CLIMB` and rides over rocks and logs up to `ROCK_STEP` high, slowing
//! in deep water and on climbs. Its four lamps (two headlamps, two roof
//! floods) burn while it is out on an expedition or driven.

use std::f32::consts::{PI, TAU};
use std::sync::LazyLock;

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;
use crate::world::{
    BASE, Body, Collider, Crusher, Describe, InstitutePass, Pedestrian, Pillar, Place,
    TERRAIN_HALF_SIZE, Terrain, Unclimbable, expedition_route,
};

use super::super::Player;
use super::super::camera::OrbitCamera;
use super::super::orders::{MoveOrder, Turning};
use super::super::path::Mobility;
use super::carrier_track::{TRACK_LINKS, TRACK_LOOPS, TRACK_WRAP, TRACK_X};
use super::patrol::{Patrol, in_the_way};
use super::track_loop::{TrackLoop, link_pose};
use super::traffic::{Traffic, keep_right, round_corners, settle_footprint};
use super::{
    AUTOPILOT_MIN_SPEED, DRIVE_KEYS, DriveInput, Driving, LAMP_COLOR, Lamp, Seat, Solids, Vehicle,
    autopilot, hull_blocked, key_input, mount_spotlights, other_colliders,
};

/// Meters per model unit: the carrier is 7 m long.
const SCALE: f32 = 7.0;
/// Half the distance between its tracks, in model units.
const HALF_TRACK: f32 = 0.2;
/// Where its tracks touch the ground, front and rear: its first and last road
/// wheels, in model units (`tools/split_moving_parts.py` has the measurements).
const TRACK_ENDS: (f32, f32) = (0.251, -0.311);
/// Its wheels' radii, told apart by hub height: road wheels run on the
/// track's lower run, the sprocket and idler sit higher at the track's ends,
/// the small return rollers carry its upper run. The belt wraps the sprocket
/// and idler and turns with them, so those roll at the distance from their
/// hub to the belt's loop (`TRACK_WRAP`), not at their own rims.
const ROAD_WHEEL: (f32, f32) = (0.09, 0.049);
const END_WHEEL_HUB: f32 = 0.13;
const RETURN_ROLLER: f32 = 0.019;
/// Its bay and the apron outside the hangar door, in hangar-local meters (the
/// hall is not turned: x across, z out of the door). The bay keeps the central
/// aisle, the exhibit rows and the stalkers' car clear.
const BAY: Vec2 = Vec2::new(3.2, -12.0);
const APRON: Vec2 = Vec2::new(3.2, 24.0);
/// The hangar door's line, in hangar-local meters: inside it the carrier
/// stands on the hall's level floor rather than the ground.
const DOOR_Z: f32 = 14.25;
/// How far right of the road's middle line it keeps.
const KEEP_RIGHT: f32 = 3.0;
const CRUISE: f32 = 7.0;
/// Its speed in the hangar and the yard, slowing for a bend, and backing up.
const YARD_SPEED: f32 = 3.0;
const TURNING: f32 = 3.0;
const BACKING: f32 = 1.5;
/// Within this distance of the hangar it keeps to `YARD_SPEED`.
const YARD_RADIUS: f32 = 75.0;
const SHARP: f32 = 1.0 / 25.0;
const ACCELERATION: f32 = 1.5;
/// How fast it turns on the spot, in radians per second.
const PIVOT_RATE: f32 = 0.35;
/// Seconds parked in the bay between expeditions, and out in the Zone.
const PARKED: f32 = 150.0;
const AWAY: f32 = 90.0;
/// Two rows of pillars down its flanks, in model units.
const COLLISION: &[Pillar] = &hull();
/// Its footprint and hull height, in meters, around those pillars.
const HULL_HALF: Vec2 = Vec2::new(0.26 * SCALE, 0.5 * SCALE);
const HULL_HEIGHT: f32 = 0.56 * SCALE;
/// Its weight and footprint: it flattens plants and knocks down what weighs
/// less, as the stalkers' car does.
const BODY: Crusher = Crusher {
    mass: 18_000.0,
    half_size: HULL_HALF,
};
/// Driven by the stalker: its top speeds ahead and astern (m/s), how hard it
/// speeds up and brakes (m/s²) and how fast its tracks turn it (rad/s).
const DRIVEN_TOP: f32 = 10.0;
const DRIVEN_REVERSE: f32 = 3.0;
const DRIVEN_ACCELERATION: f32 = 3.0;
const DRIVEN_BRAKING: f32 = 8.0;
const DRIVEN_TURN: f32 = 0.6;
/// Its speed through a bend on a click order: at its 0.6 rad/s turn rate it
/// turns an 8.3 m circle at 5 m/s (the crater road's bends are 9.5-15 m), at
/// its 10 m/s top speed 16.7 m.
const BEND_SPEED: f32 = 5.0;
/// Off road: the deepest water it fords (m), the steepest grade its tracks
/// climb (45°), and the tallest rock or log they ride over (m). A solid may
/// reach `BELLY` above its tracks' line under the hull; anything taller
/// stops it.
const FORD: f32 = 1.5;
const MAX_CLIMB: f32 = 1.0;
const ROCK_STEP: f32 = 1.0;
const BELLY: f32 = 0.5;
/// How click orders route it: clear of solids by its hull's half-width (1.8 m)
/// and a margin for its 7 m length, through the water it fords, on ground no
/// steeper than it climbs, and over what it knocks down.
pub(in crate::player) const CARRIER_ROUTING: (Mobility, Crusher) = (
    Mobility {
        body: Body {
            radius: 2.3,
            height: HULL_HEIGHT,
        },
        max_depth: FORD,
        max_grade: MAX_CLIMB,
    },
    BODY,
);
/// How much of its speed water as deep as `FORD`, and a climb as steep as
/// `MAX_CLIMB`, take off.
const WATER_DRAG: f32 = 0.6;
const CLIMB_DRAG: f32 = 0.6;

const fn hull() -> [Pillar; 8] {
    let mut pillars = [Pillar {
        x: 0.0,
        z: 0.0,
        radius: 0.13,
        bottom: 0.0,
        top: 0.56,
    }; 8];
    let mut i = 0;
    while i < 8 {
        pillars[i].x = if i < 4 { -0.13 } else { 0.13 };
        pillars[i].z = -0.37 + 0.74 * (i % 4) as f32 / 3.0;
        i += 1;
    }
    pillars
}

/// An open polyline driven by the distance along it.
struct Path {
    points: Vec<Vec2>,
    /// The distance driven to each point.
    reached: Vec<f32>,
}

impl Path {
    fn new(points: Vec<Vec2>) -> Self {
        let mut reached = vec![0.0];
        for [a, b] in points.array_windows() {
            reached.push(reached[reached.len() - 1] + a.distance(*b));
        }
        Self { points, reached }
    }

    fn length(&self) -> f32 {
        self.reached[self.reached.len() - 1]
    }

    fn point(&self, along: f32) -> Vec2 {
        let along = along.clamp(0.0, self.length());
        let i = self
            .reached
            .partition_point(|&reached| reached <= along)
            .clamp(1, self.points.len() - 1)
            - 1;
        let leg = self.reached[i + 1] - self.reached[i];
        let t = if leg > 0.0 {
            (along - self.reached[i]) / leg
        } else {
            0.0
        };
        self.points[i].lerp(self.points[i + 1], t)
    }

    fn heading(&self, along: f32) -> Vec2 {
        let (before, after) = ((along - 1.5).max(0.0), (along + 1.5).min(self.length()));
        (self.point(after) - self.point(before)).normalize_or(Vec2::Y)
    }

    /// How sharply it bends at `along`, in radians per meter.
    fn bend(&self, along: f32) -> f32 {
        self.heading(along - 1.0)
            .angle_to(self.heading(along + 1.0))
            .abs()
            / 2.0
    }
}

/// The three paths of an expedition, in world meters.
struct Paths {
    /// From the bay out to the far end of the route.
    out: Path,
    /// From the far end back to the apron.
    home: Path,
    /// From the bay to the apron: backed along on the way in.
    bay: Path,
}

impl Paths {
    /// The path a leg follows, and which way it runs along it.
    fn of(&self, leg: Leg) -> Option<(&Path, f32)> {
        match leg {
            Leg::Out => Some((&self.out, 1.0)),
            Leg::Home => Some((&self.home, 1.0)),
            Leg::Backing => Some((&self.bay, -1.0)),
            Leg::Parked | Leg::Away | Leg::Pivot { .. } | Leg::Taken { .. } => None,
        }
    }

    fn new() -> Self {
        let base = Vec2::from(BASE);
        let (bay, apron) = (base + BAY, base + APRON);
        let road = expedition_route();
        let mut back = road;
        back.reverse();
        let out = [bay, apron]
            .into_iter()
            .chain(keep_right(&road, KEEP_RIGHT, false))
            .collect::<Vec<_>>();
        // Home starts where it turned round, crossing to the other lane.
        let far_end = out[out.len() - 1];
        let home = [far_end]
            .into_iter()
            .chain(keep_right(&back, KEEP_RIGHT, false).into_iter().skip(1))
            .chain([apron])
            .collect::<Vec<_>>();
        Self {
            out: Path::new(round_corners(&out, false)),
            home: Path::new(round_corners(&home, false)),
            bay: Path::new(vec![bay, apron]),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Leg {
    Parked,
    Out,
    Away,
    /// Turning on the spot toward the yaw.
    Pivot {
        to: f32,
        then: Next,
    },
    Home,
    /// Backing into the bay.
    Backing,
    /// The stalker's, standing or driven, off its expedition for good.
    Taken {
        at: Vec2,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Next {
    Home,
    Backing,
}

#[derive(Component)]
#[require(Unclimbable, Seat = Seat::new(HULL_HALF))]
pub(crate) struct Carrier {
    paths: Paths,
    leg: Leg,
    /// Meters along the leg's path.
    along: f32,
    /// Signed: negative while backing.
    speed: f32,
    /// Seconds left of a wait.
    wait: f32,
    /// Facing, as a yaw about +Y (0 faces +Z).
    yaw: f32,
    /// How far each track has run, left then right, in meters.
    tracks: [f32; 2],
}

impl Carrier {
    /// Parked in its bay, at the start of its schedule.
    pub(crate) fn new() -> Self {
        Self {
            paths: Paths::new(),
            leg: Leg::Parked,
            along: 0.0,
            speed: 0.0,
            wait: PARKED,
            yaw: 0.0,
            tracks: [0.0; 2],
        }
    }

    /// The path the current leg follows, and which way it runs along it.
    fn path(&self) -> Option<(&Path, f32)> {
        self.paths.of(self.leg)
    }

    /// Where it stands on the ground plane.
    fn at(&self) -> Vec2 {
        match self.leg {
            Leg::Parked => self.paths.bay.point(0.0),
            Leg::Away
            | Leg::Pivot {
                then: Next::Home, ..
            } => self.paths.out.point(self.paths.out.length()),
            Leg::Pivot {
                then: Next::Backing,
                ..
            } => self.paths.home.point(self.paths.home.length()),
            Leg::Out | Leg::Home | Leg::Backing => {
                let (path, _) = self.path().expect("a driving leg has a path");
                path.point(self.along)
            }
            Leg::Taken { at } => at,
        }
    }

    /// Whether the stalker can get in: it stands still, parked, waiting out in
    /// the Zone or where they left it.
    fn boardable(&self) -> bool {
        matches!(self.leg, Leg::Parked | Leg::Away | Leg::Taken { .. })
    }

    /// Takes it off its expedition where it stands, for the stalker to drive.
    fn take(&mut self) {
        if !matches!(self.leg, Leg::Taken { .. }) {
            self.leg = Leg::Taken { at: self.at() };
            self.speed = 0.0;
        }
    }

    /// Where the stalker left it and the way it faces (its yaw), once they
    /// have taken it.
    pub(crate) fn left_at(&self) -> Option<(Vec2, f32)> {
        match self.leg {
            Leg::Taken { at } => Some((at, self.yaw)),
            _ => None,
        }
    }

    /// Stands it, the stalker's, at `at` facing `yaw`, as a save left it.
    pub(crate) fn leave(
        &mut self,
        (at, yaw): (Vec2, f32),
        pose: &mut Transform,
        terrain: &Terrain,
    ) {
        self.leg = Leg::Taken { at };
        self.yaw = yaw;
        self.speed = 0.0;
        place(self, pose, terrain, &[]);
    }

    /// Whether it is out on an expedition, rather than parked or the stalker's.
    pub(crate) fn on_expedition(&self) -> bool {
        !matches!(self.leg, Leg::Parked | Leg::Taken { .. })
    }

    /// How hard its engine works while the stalker drives it (0-1).
    pub(crate) fn engine_load(&self) -> f32 {
        (self.speed.abs() / DRIVEN_TOP).min(1.0)
    }

    /// Drives it by `input` for `dt` seconds while it is the stalker's, at up
    /// to `going` (0-1) of its top speed; `fits` tells whether it may stand at
    /// a spot facing a way. Like the car, it turns the other way backing up;
    /// standing, its tracks turn it on the spot. Returns how far it turned.
    fn steer(
        &mut self,
        input: DriveInput,
        going: f32,
        dt: f32,
        fits: impl Fn(Vec2, Vec2) -> bool,
    ) -> f32 {
        let Leg::Taken { at } = self.leg else {
            return 0.0;
        };
        let target = going
            * if input.brake {
                0.0
            } else if input.throttle >= 0.0 {
                input.throttle * DRIVEN_TOP
            } else {
                input.throttle * DRIVEN_REVERSE
            };
        let rate = if input.brake || target * self.speed < 0.0 {
            DRIVEN_BRAKING
        } else {
            DRIVEN_ACCELERATION
        };
        self.speed += (target - self.speed).clamp(-rate * dt, rate * dt);
        let astern = self.speed < -0.1 || (self.speed.abs() <= 0.1 && input.throttle < 0.0);
        let turn = input.steering * DRIVEN_TURN * dt * if astern { -1.0 } else { 1.0 };
        let step = self.speed * dt;
        let yaw = self.yaw;
        let facing = |turn: f32| Vec2::new((yaw + turn).sin(), (yaw + turn).cos());
        let (to, step, turn) = if fits(at + facing(turn) * step, facing(turn)) {
            (at + facing(turn) * step, step, turn)
        } else if turn != 0.0 && step != 0.0 && fits(at + facing(0.0) * step, facing(0.0)) {
            // As the car: a turn that would swing a corner into a rock goes
            // straight instead, so nosed against one it still backs out.
            (at + facing(0.0) * step, step, 0.0)
        } else if turn != 0.0 && fits(at, facing(turn)) {
            // Blocked ahead, it can still turn where it stands.
            self.speed = 0.0;
            (at, 0.0, turn)
        } else {
            self.speed = 0.0;
            return 0.0;
        };
        let yaw = self.yaw + turn;
        // Turning left (+yaw), the left track runs back, the right forward.
        let run = turn * HALF_TRACK * SCALE;
        self.tracks[0] += step - run;
        self.tracks[1] += step + run;
        self.yaw = yaw.rem_euclid(TAU);
        self.leg = Leg::Taken { at: to };
        turn
    }

    /// The way it faces.
    fn facing(&self) -> Vec2 {
        Vec2::new(self.yaw.sin(), self.yaw.cos())
    }

    /// Advances it by `dt` seconds; `blocked` tells whether something stands
    /// in the way it would move from `at` heading `toward`.
    fn advance(&mut self, dt: f32, blocked: impl Fn(Vec2, Vec2) -> bool) {
        match self.leg {
            Leg::Taken { .. } => {}
            Leg::Parked | Leg::Away => {
                self.wait -= dt;
                if self.wait <= 0.0 {
                    self.leg = if self.leg == Leg::Parked {
                        self.along = 0.0;
                        Leg::Out
                    } else {
                        Leg::Pivot {
                            to: self.yaw + PI,
                            then: Next::Home,
                        }
                    };
                }
            }
            Leg::Pivot { to, then } => {
                let turn = (to - self.yaw).clamp(-PIVOT_RATE * dt, PIVOT_RATE * dt);
                self.yaw += turn;
                // Turning left (+yaw), the left track runs back, the right forward.
                let run = turn * HALF_TRACK * SCALE;
                self.tracks[0] -= run;
                self.tracks[1] += run;
                if (to - self.yaw).abs() < 1e-4 {
                    self.yaw = to.rem_euclid(2.0 * PI);
                    self.leg = match then {
                        Next::Home => Leg::Home,
                        Next::Backing => Leg::Backing,
                    };
                    self.along = match then {
                        Next::Home => 0.0,
                        Next::Backing => self.paths.bay.length(),
                    };
                }
            }
            Leg::Out | Leg::Home | Leg::Backing => self.drive(dt, blocked),
        }
    }

    fn drive(&mut self, dt: f32, blocked: impl Fn(Vec2, Vec2) -> bool) {
        let (path, sign) = self.paths.of(self.leg).expect("a driving leg has a path");
        let (at, heading) = (path.point(self.along), path.heading(self.along));
        let left = if sign > 0.0 {
            path.length() - self.along
        } else {
            self.along
        };
        let sharp = (0..5).any(|k| path.bend(self.along + sign * 3.0 * k as f32) > SHARP);
        let limit = if blocked(at, heading * sign) {
            0.0
        } else if sign < 0.0 {
            BACKING
        } else if at.distance(Vec2::from(BASE)) < YARD_RADIUS || sharp {
            YARD_SPEED.min(TURNING)
        } else {
            CRUISE
        };
        // Ease to a stop at the leg's end.
        let target = limit.min((2.0 * ACCELERATION * left).sqrt()) * sign;
        self.speed += (target - self.speed).clamp(-ACCELERATION * dt, ACCELERATION * dt);
        let step = (self.speed * dt).clamp(-left, left);
        self.along = (self.along + step).clamp(0.0, path.length());
        self.tracks[0] += step;
        self.tracks[1] += step;
        let heading = path.heading(self.along);
        self.yaw = heading.x.atan2(heading.y);
        let arrived = if sign > 0.0 {
            self.along >= path.length() - 1e-3
        } else {
            self.along <= 1e-3
        };
        if arrived {
            self.speed = 0.0;
            self.leg = match self.leg {
                Leg::Out => {
                    self.wait = AWAY;
                    Leg::Away
                }
                Leg::Home => Leg::Pivot {
                    to: self.yaw + PI,
                    then: Next::Backing,
                },
                _ => {
                    self.wait = PARKED;
                    Leg::Parked
                }
            };
        }
    }
}

/// A road wheel, sprocket or idler: `side` 0 on the left (+X), 1 on the right.
#[derive(Component)]
pub(crate) struct CarrierWheel {
    side: usize,
    radius: f32,
}

/// One link of a track belt: link `index` of `TRACK_LINKS[side]`, a copy of the
/// scanned `TrackLinkL`/`TrackLinkR`.
#[derive(Component)]
pub(crate) struct CarrierLink {
    side: usize,
    index: usize,
}

/// Each track's belt loop, derived from the baked `TRACK_LOOPS` once, at first use.
static TRACKS: LazyLock<[TrackLoop; 2]> = LazyLock::new(|| TRACK_LOOPS.map(TrackLoop::new));

/// The pose of `link` in the carrier's model frame: its place on its belt, run round
/// as far as its track has run (meters, the same as its wheels roll with).
fn link_transform(carrier: &Carrier, link: &CarrierLink) -> Transform {
    link_pose(
        &TRACKS[link.side],
        TRACK_X[link.side],
        TRACK_LINKS[link.side],
        link.index,
        carrier.tracks[link.side] / SCALE,
    )
}

/// The radius a wheel on `side` rolls at, in model units, from its hub: the
/// sprocket in front and the idler behind take their belt's wrap radius.
fn wheel_radius(side: usize, hub: Vec3) -> f32 {
    if hub.y < ROAD_WHEEL.0 {
        ROAD_WHEEL.1
    } else if hub.y < END_WHEEL_HUB {
        let [sprocket, idler] = TRACK_WRAP[side];
        if hub.z > 0.0 { sprocket } else { idler }
    } else {
        RETURN_ROLLER
    }
}

pub(crate) fn spawn_carrier(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let carrier = Carrier::new();
    let mut pose = Transform::from_scale(Vec3::splat(SCALE));
    place(&carrier, &mut pose, &terrain, &[]);
    let mut spawned = commands.spawn((
        Name::new("Institute carrier"),
        carrier,
        Collider(COLLISION),
        BODY,
        InstitutePass,
        Describe {
            place: Place::Carrier,
            radius: 3.6,
            height: 4.0,
        },
        pose,
        WorldAssetRoot(assets::load_vehicle_scene(
            &server,
            assets::INSTITUTE_CARRIER,
        )),
    ));
    spawned.observe(attach_carrier_wheels);
    spawned.observe(attach_carrier_links);
    mount_spotlights(&mut spawned, &mut meshes, &mut materials, &carrier_lamps());
}

/// Its lamps, in model units (7 m): two headlamps on the upper glacis that
/// light the ground just ahead, and two floods on the cab roof that throw a
/// long, wide spot over rough country.
fn carrier_lamps() -> [Lamp; 4] {
    let headlamp = |x: f32| Lamp {
        mount: Transform::from_xyz(x, 0.215, 0.455).looking_to(Vec3::Z - Vec3::Y * 0.12, Vec3::Y),
        size: 0.022,
        light: SpotLight {
            color: LAMP_COLOR,
            intensity: 3.5e7,
            range: 50.0,
            radius: 0.15,
            inner_angle: 0.25,
            outer_angle: 0.5,
            ..default()
        },
    };
    let flood = |x: f32| Lamp {
        mount: Transform::from_xyz(x, 0.505, 0.2).looking_to(Vec3::Z - Vec3::Y * 0.04, Vec3::Y),
        size: 0.03,
        light: SpotLight {
            color: LAMP_COLOR,
            intensity: 1.2e8,
            range: 180.0,
            radius: 0.25,
            inner_angle: 0.18,
            outer_angle: 0.42,
            shadow_maps_enabled: true,
            ..default()
        },
    };
    [headlamp(-0.17), headlamp(0.17), flood(-0.12), flood(0.12)]
}

fn attach_carrier_wheels(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    parts: Query<(&Name, &Transform)>,
) {
    for entity in children.iter_descendants(ready.entity) {
        let Ok((name, transform)) = parts.get(entity) else {
            continue;
        };
        let side = match name.as_str() {
            name if name.starts_with("WheelL") => 0,
            name if name.starts_with("WheelR") => 1,
            _ => continue,
        };
        let radius = wheel_radius(side, transform.translation);
        commands
            .entity(entity)
            .insert(CarrierWheel { side, radius });
    }
}

/// Replaces each scanned track link node (`TrackLinkL`, `TrackLinkR`) by a ring of
/// copies that run round the belt.
fn attach_carrier_links(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    carriers: Query<&Carrier>,
    children: Query<&Children>,
    parts: Query<(&Name, &ChildOf)>,
    primitives: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
) {
    let Ok(carrier) = carriers.get(ready.entity) else {
        return;
    };
    for entity in children.iter_descendants(ready.entity) {
        let Ok((name, parent)) = parts.get(entity) else {
            continue;
        };
        let side = match name.as_str() {
            "TrackLinkL" => 0,
            "TrackLinkR" => 1,
            _ => continue,
        };
        let Some((mesh, material)) = children
            .iter_descendants(entity)
            .find_map(|part| primitives.get(part).ok())
        else {
            warn!("{name} has no mesh to copy");
            continue;
        };
        spawn_links(
            &mut commands,
            carrier,
            LinkTemplate {
                side,
                node: entity,
                parent: parent.parent(),
                mesh: mesh.clone(),
                material: material.clone(),
            },
        );
    }
}

/// A scanned link node of one belt side: its entity and its parent's, and the mesh
/// and material to copy.
struct LinkTemplate {
    side: usize,
    node: Entity,
    parent: Entity,
    mesh: Mesh3d,
    material: MeshMaterial3d<StandardMaterial>,
}

/// Hides the scanned link and spawns its copies beside it, in the same model frame,
/// where its track has run them. They are siblings, not children, so the hidden
/// template does not hide them.
fn spawn_links(commands: &mut Commands, carrier: &Carrier, template: LinkTemplate) {
    let side = template.side;
    commands.entity(template.node).insert(Visibility::Hidden);
    commands.entity(template.parent).with_children(|belt| {
        for index in 0..TRACK_LINKS[side] {
            let link = CarrierLink { side, index };
            belt.spawn((
                link_transform(carrier, &link),
                link,
                template.mesh.clone(),
                template.material.clone(),
            ));
        }
    });
}

/// Sets the carrier on the ground, or the `footings` it rides over, where it
/// stands, tilted to its tracks.
fn place(carrier: &Carrier, pose: &mut Transform, terrain: &Terrain, footings: &[Footing]) {
    let settled = settled_at(carrier.at(), carrier.facing(), terrain, footings);
    pose.translation = settled.translation;
    pose.rotation = settled.rotation;
}

/// A rock, log or other solid low enough for its tracks to ride over, in the
/// world.
#[derive(Clone, Copy, Debug)]
struct Footing {
    at: Vec2,
    radius: f32,
    top: f32,
}

/// The solids around `at` its tracks can ride over: those standing at most
/// `ROCK_STEP` over the ground.
fn footings<'a>(
    at: Vec2,
    terrain: &Terrain,
    colliders: impl Iterator<Item = (&'a Collider, &'a GlobalTransform)>,
) -> Vec<Footing> {
    // A frame's travel past its footprint's corners.
    let reach = HULL_HALF.length() + 2.0;
    let mut found = Vec::new();
    for (collider, transform) in colliders {
        let (scale, _, _) = transform.to_scale_rotation_translation();
        for pillar in collider.0 {
            let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let footing = Footing {
                at: center.xz(),
                radius: pillar.radius * scale.x,
                top: center.y + pillar.top * scale.y,
            };
            if footing.at.distance(at) < reach + footing.radius
                && footing.top - ground(footing.at, terrain) <= ROCK_STEP
            {
                found.push(footing);
            }
        }
    }
    found
}

/// Its pose standing at `at` facing `facing`, tilted to its tracks: each
/// quarter of its footprint rests on the ground or on the highest of
/// `footings` under it.
fn settled_at(at: Vec2, facing: Vec2, terrain: &Terrain, footings: &[Footing]) -> Transform {
    let ends = (TRACK_ENDS.0 * SCALE, TRACK_ENDS.1 * SCALE);
    let middle = (ends.0 + ends.1) / 2.0;
    let half_length = (ends.0 - ends.1) / 2.0;
    let right = facing.perp();
    let local = |p: Vec2| ((p - at).dot(facing) - middle, (p - at).dot(right));
    let (height, rotation) = settle_footprint(at, facing, ends, HALF_TRACK * SCALE, |corner| {
        let (along, side) = local(corner);
        footings
            .iter()
            .filter(|footing| {
                let (f_along, f_side) = local(footing.at);
                f_along * along >= 0.0
                    && f_side * side >= 0.0
                    && f_along.abs() <= half_length + footing.radius
                    && f_side.abs() <= HULL_HALF.x + footing.radius
            })
            .map(|footing| footing.top)
            .fold(ground(corner, terrain), f32::max)
    });
    Transform::from_translation(at.extend(height).xzy()).with_rotation(rotation)
}

/// The ground under `p`: the hall's level floor inside the hangar door.
fn ground(p: Vec2, terrain: &Terrain) -> f32 {
    let base = Vec2::from(BASE);
    if p.y - base.y < DOOR_Z && (p.x - base.x).abs() < 20.0 {
        terrain.height_at(base.x, base.y)
    } else {
        terrain.mesh_height_at(p)
    }
}

/// What the carrier stops for: the stalker, their car, the patrols and traffic.
type InTheWay<'w, 's> = Query<
    'w,
    's,
    &'static Transform,
    (
        Or<(
            With<Player>,
            With<Vehicle>,
            With<Patrol>,
            With<Traffic>,
            With<Pedestrian>,
        )>,
        Without<Carrier>,
    ),
>;

type Wheels<'w, 's> = Query<
    'w,
    's,
    (&'static CarrierWheel, &'static mut Transform),
    (
        Without<Carrier>,
        Without<Player>,
        Without<Vehicle>,
        Without<Patrol>,
        Without<Traffic>,
        Without<Pedestrian>,
    ),
>;

/// Runs the carrier's expedition over the rendered ground, unless the stalker
/// has taken it, and rolls its wheels.
pub(crate) fn drive_carrier(
    time: Res<Time>,
    terrain: Res<Terrain>,
    others: InTheWay,
    mut carriers: Query<(&mut Carrier, &mut Transform, &mut Seat)>,
    mut wheels: Wheels,
) {
    let dt = time.delta_secs();
    let Ok((mut carrier, mut pose, mut seat)) = carriers.single_mut() else {
        return;
    };
    if !matches!(carrier.leg, Leg::Taken { .. }) {
        carrier.advance(dt, |at, toward| {
            others
                .iter()
                .any(|other| in_the_way(at, toward, other.translation.xz()))
        });
        place(&carrier, &mut pose, &terrain, &[]);
    }
    seat.open = carrier.boardable();
    for (wheel, mut transform) in &mut wheels {
        let roll = carrier.tracks[wheel.side] / (wheel.radius * SCALE);
        transform.rotation = Quat::from_rotation_x(roll);
    }
}

/// Runs the links round their belts when the tracks have run since they were placed.
pub(crate) fn run_carrier_tracks(
    carriers: Query<&Carrier>,
    mut links: Query<(&CarrierLink, &mut Transform)>,
    mut placed: Local<[f32; 2]>,
) {
    let Ok(carrier) = carriers.single() else {
        return;
    };
    if *placed == carrier.tracks {
        return;
    }
    *placed = carrier.tracks;
    for (link, mut transform) in &mut links {
        *transform = link_transform(carrier, link);
    }
}

/// How much of its top speed it makes at `at` heading `toward`: deep water
/// and an uphill grade slow it.
fn going(terrain: &Terrain, at: Vec2, toward: Vec2) -> f32 {
    let depth = terrain.water_depth_at(at.x, at.y) / FORD;
    let grade = (ground(at + toward, terrain) - ground(at, terrain)).max(0.0) / MAX_CLIMB;
    (1.0 - WATER_DRAG * depth.min(1.0)) * (1.0 - CLIMB_DRAG * grade.min(1.0))
}

/// The stalker at the carrier's wheel, carried along with it.
type CarrierDriver<'w, 's> = Query<
    'w,
    's,
    (
        &'static Driving,
        &'static mut Transform,
        &'static mut MoveOrder,
    ),
    (With<Player>, Without<Carrier>),
>;

/// Drives the carrier the stalker sits in, by the keys or a point-and-click
/// order, and carries them along.
pub(crate) fn steer_carrier(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<Terrain>,
    colliders: Solids,
    mut drivers: CarrierDriver,
    mut carriers: Query<(Entity, &mut Carrier, &mut Transform)>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let Ok((driving, mut stalker, mut order)) = drivers.single_mut() else {
        return;
    };
    let Ok((entity, mut carrier, mut pose)) = carriers.get_mut(driving.0) else {
        return;
    };
    carrier.take();
    let dt = time.delta_secs();
    if keys.any_pressed(DRIVE_KEYS) {
        order.cancel();
    }
    let autopilot = autopilot(&mut order, &pose, carrier.speed, BEND_SPEED);
    let input = autopilot.unwrap_or_else(|| key_input(&keys));
    let from = carrier.at();
    let footings = footings(from, &terrain, other_colliders(&colliders, entity, BODY));
    let fits = |at: Vec2, facing: Vec2| {
        at.abs().max_element() <= TERRAIN_HALF_SIZE - HULL_HALF.length()
            && terrain.can_wade(from, at, FORD)
            && ground(at, &terrain) - ground(from, &terrain) <= MAX_CLIMB * at.distance(from) + 0.05
            && !hull_blocked(
                &settled_at(at, facing, &terrain, &footings),
                HULL_HALF,
                (BELLY, HULL_HEIGHT),
                other_colliders(&colliders, entity, BODY),
            )
    };
    let going = going(&terrain, from, carrier.facing() * input.throttle.signum());
    let turned = carrier.steer(input, going, dt, fits);
    order.track_progress(
        carrier.at(),
        carrier.at().distance(from),
        AUTOPILOT_MIN_SPEED * dt,
        dt,
        Turning::Reversing,
    );
    place(&carrier, &mut pose, &terrain, &footings);
    stalker.translation = pose.translation;
    if autopilot.is_none()
        && let Ok(mut orbit) = cameras.single_mut()
    {
        orbit.record_turn(turned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_follow_the_tracks() {
        let loops = TRACK_LOOPS.map(TrackLoop::new);
        let d = 1.0;
        let at = |tracks: [f32; 2], side: usize, index: usize| {
            let carrier = Carrier {
                tracks,
                ..Carrier::new()
            };
            link_transform(&carrier, &CarrierLink { side, index })
        };
        // Each link sits where `link_pose` puts it once its track has run `d` meters,
        // the left forward and the right back, as in a pivot.
        let pivot = [d, -d];
        for (side, run) in [(0, d), (1, -d)] {
            for index in [0, 10, 40] {
                assert_eq!(
                    at(pivot, side, index),
                    link_pose(
                        &loops[side],
                        TRACK_X[side],
                        TRACK_LINKS[side],
                        index,
                        run / SCALE
                    ),
                    "side {side} link {index}"
                );
            }
        }
        // On the lower run (link 10 rests there) the left belt travels rearward and
        // the right forward.
        let rest = at([0.0; 2], 0, 10);
        assert!(rest.translation.y < 0.02, "link 10 is on the lower run");
        assert!(at(pivot, 0, 10).translation.z < rest.translation.z);
        assert!(at(pivot, 1, 10).translation.z > at([0.0; 2], 1, 10).translation.z);
    }

    /// A carrier whose scene holds a template link node per side, replaced by links.
    fn carrier_with_links(world: &mut World) -> (Entity, [Entity; 2], Mesh3d) {
        let mesh = Mesh3d(Assets::<Mesh>::default().add(Cuboid::default()));
        let carrier = world.spawn(Carrier::new()).id();
        let nodes = [0, 1].map(|_| world.spawn((Visibility::default(), ChildOf(carrier))).id());
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, world);
        for (side, node) in nodes.into_iter().enumerate() {
            let template = LinkTemplate {
                side,
                node,
                parent: carrier,
                mesh: mesh.clone(),
                material: MeshMaterial3d(Handle::default()),
            };
            spawn_links(&mut commands, &Carrier::new(), template);
        }
        queue.apply(world);
        (carrier, nodes, mesh)
    }

    #[test]
    fn the_template_links_are_hidden_and_their_copies_are_not() {
        let mut world = World::new();
        // As `VisibilityPlugin` does.
        world.register_required_components::<Mesh3d, Visibility>();
        let (carrier, nodes, mesh) = carrier_with_links(&mut world);
        for node in nodes {
            assert_eq!(world.get::<Visibility>(node), Some(&Visibility::Hidden));
        }
        let mut links = world.query::<(&CarrierLink, &Visibility, &ChildOf, &Mesh3d)>();
        for side in 0..2 {
            let copies: Vec<_> = links
                .iter(&world)
                .filter(|(link, ..)| link.side == side)
                .collect();
            assert_eq!(copies.len(), TRACK_LINKS[side]);
            for (_, visibility, parent, copy) in copies {
                assert_eq!(*visibility, Visibility::Inherited);
                assert_eq!(parent.parent(), carrier, "a sibling of the template");
                assert_eq!(copy.0, mesh.0);
            }
        }
    }

    #[test]
    fn the_links_run_when_the_tracks_do() {
        let mut world = World::new();
        let (carrier, ..) = carrier_with_links(&mut world);
        let mut schedule = Schedule::default();
        schedule.add_systems(run_carrier_tracks);
        let poses = |world: &mut World| {
            let mut links = world.query::<(&CarrierLink, &Transform)>();
            let carrier = world.query::<&Carrier>().single(world).expect("a carrier");
            links
                .iter(world)
                .map(|(link, pose)| (*pose, link_transform(carrier, link)))
                .collect::<Vec<_>>()
        };
        let before = poses(&mut world);
        assert_eq!(before.len(), TRACK_LINKS.iter().sum::<usize>());
        assert!(before.iter().all(|(pose, wanted)| pose == wanted));
        world.get_mut::<Carrier>(carrier).expect("a carrier").tracks = [2.0, -2.0];
        assert!(
            poses(&mut world)
                .iter()
                .any(|(pose, wanted)| pose != wanted)
        );
        schedule.run(&mut world);
        let after = poses(&mut world);
        assert!(after.iter().all(|(pose, wanted)| pose == wanted));
        assert!(after.iter().zip(&before).any(|(now, was)| now.0 != was.0));
    }

    #[test]
    fn the_end_wheels_roll_at_the_radius_their_belt_wraps() {
        // Hubs as the carrier GLB's Wheel nodes have them (x across, then height, then z forward).
        let hub = |y: f32, z: f32| Vec3::new(0.19, y, z);
        let sprocket = [hub(0.1199, 0.3497), hub(0.1206, 0.3524)];
        let idler = [hub(0.1217, -0.4173), hub(0.1229, -0.4177)];
        for side in 0..2 {
            assert_eq!(wheel_radius(side, sprocket[side]), TRACK_WRAP[side][0]);
            assert_eq!(wheel_radius(side, idler[side]), TRACK_WRAP[side][1]);
            // The idler's belt is wrapped wider than the sprocket's, and neither is the road wheels'.
            assert!(TRACK_WRAP[side][1] > TRACK_WRAP[side][0]);
            assert_eq!(wheel_radius(side, hub(0.063, 0.1367)), ROAD_WHEEL.1);
            assert_eq!(wheel_radius(side, hub(0.1405, 0.0722)), RETURN_ROLLER);
        }
    }

    #[test]
    fn the_links_stay_put_while_the_tracks_stand() {
        let mut world = World::new();
        let (carrier, ..) = carrier_with_links(&mut world);
        let mut schedule = Schedule::default();
        schedule.add_systems(run_carrier_tracks);
        world.get_mut::<Carrier>(carrier).expect("a carrier").tracks = [2.0, -2.0];
        schedule.run(&mut world);
        // Move one link by hand: with the tracks where they were, the system must not touch it.
        let link = world
            .query_filtered::<Entity, With<CarrierLink>>()
            .iter(&world)
            .next()
            .expect("a link");
        let moved = Transform::from_xyz(1.0, 2.0, 3.0);
        *world.get_mut::<Transform>(link).expect("a pose") = moved;
        schedule.run(&mut world);
        assert_eq!(world.get::<Transform>(link), Some(&moved));
        // Once the tracks run again it is placed afresh.
        world.get_mut::<Carrier>(carrier).expect("a carrier").tracks = [2.5, -2.5];
        schedule.run(&mut world);
        assert_ne!(world.get::<Transform>(link), Some(&moved));
    }

    fn run(carrier: &mut Carrier, seconds: f32, blocked: bool) {
        let dt = 0.05;
        for _ in 0..(seconds / dt) as usize {
            carrier.advance(dt, |_, _| blocked);
        }
    }

    #[test]
    fn an_expedition_leaves_the_bay_reaches_the_zone_and_backs_in_again() {
        let mut carrier = Carrier::new();
        let bay = carrier.at();
        run(&mut carrier, PARKED - 1.0, false);
        assert_eq!(carrier.leg, Leg::Parked);
        run(&mut carrier, 2.0, false);
        assert_eq!(carrier.leg, Leg::Out);
        // Out of the door, facing the yard.
        run(&mut carrier, 20.0, false);
        assert!(carrier.at().y > bay.y + 20.0);
        assert!(carrier.facing().y > 0.9);
        let mut entered_zone = false;
        let mut seconds = 0.0;
        while carrier.leg != Leg::Parked || seconds < 60.0 {
            run(&mut carrier, 1.0, false);
            entered_zone |= crate::world::in_zone(carrier.at());
            seconds += 1.0;
            assert!(seconds < 1200.0, "stuck on {:?}", carrier.leg);
        }
        assert!(entered_zone, "never crossed the Perimeter");
        assert!(carrier.at().distance(bay) < 0.01);
        // Back in its bay facing the door, ready to leave again.
        assert!(carrier.facing().y > 0.999, "{}", carrier.facing());
    }

    fn drive(carrier: &mut Carrier, seconds: f32, throttle: f32, steering: f32) -> f32 {
        let dt = 0.05;
        let input = DriveInput {
            throttle,
            steering,
            brake: false,
        };
        (0..(seconds / dt) as usize)
            .map(|_| carrier.steer(input, 1.0, dt, |_, _| true))
            .sum()
    }

    #[test]
    fn the_stalker_takes_it_only_standing_and_it_stays_where_left() {
        let mut carrier = Carrier::new();
        assert!(carrier.boardable(), "parked in its bay");
        run(&mut carrier, PARKED + 5.0, false);
        assert!(!carrier.boardable(), "not while it drives out");

        let mut carrier = Carrier::new();
        let bay = carrier.at();
        carrier.take();
        // Its expedition no longer runs; it stays where it stands.
        run(&mut carrier, PARKED + 30.0, false);
        assert_eq!(carrier.at(), bay);
        assert!(carrier.boardable());
    }

    #[test]
    fn driven_it_runs_on_its_tracks_and_turns_on_the_spot() {
        let mut carrier = Carrier::new();
        carrier.take();
        let bay = carrier.at();
        // Ahead, out of the door (+z), up to its top speed.
        drive(&mut carrier, 6.0, 1.0, 0.0);
        assert!(carrier.at().y > bay.y + 20.0, "{}", carrier.at());
        assert!((carrier.speed - DRIVEN_TOP).abs() < 0.01);
        assert!(carrier.tracks[0] > 20.0 && carrier.tracks[0] == carrier.tracks[1]);
        // Standing, A turns it left on the spot, its tracks running apart.
        drive(&mut carrier, 6.0, 0.0, 0.0);
        let (at, tracks) = (carrier.at(), carrier.tracks);
        let turned = drive(&mut carrier, 1.0, 0.0, 1.0);
        assert!((turned - DRIVEN_TURN).abs() < 0.01, "{turned}");
        assert_eq!(carrier.at(), at);
        assert!(carrier.tracks[0] < tracks[0] && carrier.tracks[1] > tracks[1]);
        // Astern it is slower.
        drive(&mut carrier, 6.0, -1.0, 0.0);
        assert!((carrier.speed + DRIVEN_REVERSE).abs() < 0.01);
    }

    #[test]
    fn driven_it_stops_at_what_it_cannot_cross() {
        let mut carrier = Carrier::new();
        carrier.take();
        let bay = carrier.at();
        let input = DriveInput {
            throttle: 1.0,
            steering: 0.0,
            brake: false,
        };
        for _ in 0..40 {
            carrier.steer(input, 1.0, 0.05, |at, _| at.y < bay.y + 1.0);
        }
        assert!(carrier.at().y < bay.y + 1.0);
        assert_eq!(carrier.speed, 0.0);
    }

    #[test]
    fn nosed_against_a_rock_it_backs_out_at_full_lock() {
        let mut carrier = Carrier::new();
        carrier.take();
        let (bay, facing) = (carrier.at(), carrier.facing());
        // Any turn would swing a corner into the rock; straight back is free.
        let fits = |_: Vec2, heading: Vec2| heading.distance(facing) < 1e-4;
        let input = DriveInput {
            throttle: -1.0,
            steering: 1.0,
            brake: false,
        };
        for _ in 0..40 {
            carrier.steer(input, 1.0, 0.05, fits);
        }
        assert!(
            carrier.at().distance(bay) > 1.0,
            "stuck at {}",
            carrier.at()
        );
        assert_eq!(carrier.facing(), facing);
    }

    #[test]
    fn its_tracks_ride_over_a_low_rock_under_one_quarter() {
        let terrain = Terrain::new();
        let at = Vec2::from(BASE) + Vec2::new(3.2, -12.0);
        let level = settled_at(at, Vec2::Y, &terrain, &[]);
        // A 0.8 m rock under the front of the track on +x.
        let floor = ground(at, &terrain);
        let rock = |offset: Vec2| Footing {
            at: at + offset,
            radius: 0.3,
            top: floor + 0.8,
        };
        let front_right = settled_at(at, Vec2::Y, &terrain, &[rock(Vec2::new(1.3, 1.5))]);
        assert!(front_right.translation.y > level.translation.y + 0.15);
        let up = front_right.rotation * Vec3::Y;
        let ahead = front_right.rotation * Vec3::Z;
        assert!(ahead.y > 0.05, "its nose rises: {ahead}");
        assert!(up.x < -0.05, "it leans away from the rock: {up}");
        // Beside its footprint, the rock makes no difference.
        let beside = settled_at(at, Vec2::Y, &terrain, &[rock(Vec2::new(4.0, 0.0))]);
        assert_eq!(beside.translation, level.translation);
    }

    #[test]
    fn it_keeps_its_top_speed_on_the_flat_only() {
        let terrain = Terrain::new();
        let hall = Vec2::from(BASE) + Vec2::new(3.2, -12.0);
        assert_eq!(going(&terrain, hall, Vec2::Y), 1.0);
        let mut carrier = Carrier::new();
        carrier.take();
        let input = DriveInput {
            throttle: 1.0,
            steering: 0.0,
            brake: false,
        };
        for _ in 0..200 {
            carrier.steer(input, 0.5, 0.05, |_, _| true);
        }
        assert!((carrier.speed - DRIVEN_TOP * 0.5).abs() < 0.01);
    }

    #[test]
    fn it_waits_while_something_stands_in_its_way() {
        let mut carrier = Carrier::new();
        run(&mut carrier, PARKED + 1.0, false);
        let at = carrier.at();
        run(&mut carrier, 30.0, true);
        // It coasts to a stop from walking pace within a metre.
        assert!(carrier.at().distance(at) < 1.5);
        run(&mut carrier, 10.0, false);
        assert!(carrier.at().distance(at) > 5.0);
    }

    #[test]
    fn it_keeps_right_and_drives_both_ways_through_the_institute_gate() {
        let paths = Paths::new();
        for (path, right) in [(&paths.out, -1.0), (&paths.home, 1.0)] {
            // Through the gate (z = 79) heading west (-x) its right is -z,
            // and heading home it is +z.
            let points: Vec<Vec2> = (0..path.length() as usize)
                .map(|s| path.point(s as f32))
                .collect();
            let through = points
                .array_windows()
                .find(|[a, b]| crate::world::in_zone(*a) != crate::world::in_zone(*b))
                .map(|[a, _]| *a)
                .expect("the route crosses the Perimeter");
            let off = through.y - 79.0;
            assert!((off * right - KEEP_RIGHT).abs() < 1.0, "{through}");
        }
    }

    #[test]
    fn the_bay_and_its_way_out_stay_inside_the_hangar_door() {
        // The door is 11.2 m either side of the hall's middle; the carrier is
        // about 3.7 m wide.
        assert!(BAY.x.abs() + 0.26 * SCALE < 11.0);
        assert_eq!(BAY.x, APRON.x);
    }
}
