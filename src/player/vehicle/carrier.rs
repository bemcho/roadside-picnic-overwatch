//! The Institute's tracked carrier, its official vehicle into the Zone. It
//! waits in its bay on the hangar's east side, then sets out on an expedition:
//! out of the door, through the yard's boom and the Institute gate (its
//! `InstitutePass` lifts both) and along the highway into the Zone
//! (`world::expedition_route`), keeping right. It waits out there, pivots on
//! its tracks, drives home, pivots again on the apron and backs into its bay.
//! It stops for the stalker, their car, a patrol or traffic in its way; they
//! stop for it. Its road wheels, sprocket and idler are separate nodes
//! (`tools/split_moving_parts.py`) that roll with the tracks, the two sides
//! turning against each other while it pivots.

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;
use crate::world::{
    BASE, Collider, Describe, InstitutePass, Pedestrian, Pillar, Place, Terrain, Unclimbable,
    expedition_route,
};

use super::super::Player;
use super::Vehicle;
use super::patrol::{Patrol, in_the_way};
use super::traffic::{Traffic, keep_right, round_corners, settle_footprint};

/// Meters per model unit: the carrier is 7 m long.
const SCALE: f32 = 7.0;
/// Half the distance between its tracks, in model units.
const HALF_TRACK: f32 = 0.2;
/// Where its tracks touch the ground, front and rear: its first and last road
/// wheels, in model units (`tools/split_moving_parts.py` has the measurements).
const TRACK_ENDS: (f32, f32) = (0.251, -0.311);
/// Its wheels' radii, told apart by hub height: road wheels run on the
/// track's lower run, the sprocket and idler sit higher at the track's ends,
/// the small return rollers carry its upper run.
const ROAD_WHEEL: (f32, f32) = (0.09, 0.049);
const END_WHEEL: (f32, f32) = (0.13, 0.044);
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
            Leg::Parked | Leg::Away | Leg::Pivot { .. } => None,
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
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Next {
    Home,
    Backing,
}

#[derive(Component)]
#[require(Unclimbable)]
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
    fn new() -> Self {
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
        }
    }

    /// The way it faces.
    fn facing(&self) -> Vec2 {
        Vec2::new(self.yaw.sin(), self.yaw.cos())
    }

    /// Advances it by `dt` seconds; `blocked` tells whether something stands
    /// in the way it would move from `at` heading `toward`.
    fn advance(&mut self, dt: f32, blocked: impl Fn(Vec2, Vec2) -> bool) {
        match self.leg {
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

/// A wheel's radius from its hub height, in model units.
fn wheel_radius(hub: f32) -> f32 {
    if hub < ROAD_WHEEL.0 {
        ROAD_WHEEL.1
    } else if hub < END_WHEEL.0 {
        END_WHEEL.1
    } else {
        RETURN_ROLLER
    }
}

pub(crate) fn spawn_carrier(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
) {
    let carrier = Carrier::new();
    let mut pose = Transform::from_scale(Vec3::splat(SCALE));
    place(&carrier, &mut pose, &terrain);
    commands
        .spawn((
            Name::new("Institute carrier"),
            carrier,
            Collider(COLLISION),
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
        ))
        .observe(attach_carrier_wheels);
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
        let radius = wheel_radius(transform.translation.y);
        commands
            .entity(entity)
            .insert(CarrierWheel { side, radius });
    }
}

/// Sets the carrier on the ground where it stands, tilted to its tracks.
fn place(carrier: &Carrier, pose: &mut Transform, terrain: &Terrain) {
    let (at, facing) = (carrier.at(), carrier.facing());
    let ends = (TRACK_ENDS.0 * SCALE, TRACK_ENDS.1 * SCALE);
    let base = Vec2::from(BASE);
    let floor = terrain.height_at(base.x, base.y);
    let ground = |p: Vec2| {
        if p.y - base.y < DOOR_Z && (p.x - base.x).abs() < 20.0 {
            floor
        } else {
            terrain.mesh_height_at(p)
        }
    };
    let (height, rotation) = settle_footprint(at, facing, ends, HALF_TRACK * SCALE, ground);
    pose.translation = at.extend(height).xzy();
    pose.rotation = rotation;
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

/// Runs the carrier's expedition over the rendered ground and rolls its wheels.
pub(crate) fn drive_carrier(
    time: Res<Time>,
    terrain: Res<Terrain>,
    others: InTheWay,
    mut carriers: Query<(&mut Carrier, &mut Transform)>,
    mut wheels: Wheels,
) {
    let dt = time.delta_secs();
    let Ok((mut carrier, mut pose)) = carriers.single_mut() else {
        return;
    };
    carrier.advance(dt, |at, toward| {
        others
            .iter()
            .any(|other| in_the_way(at, toward, other.translation.xz()))
    });
    place(&carrier, &mut pose, &terrain);
    for (wheel, mut transform) in &mut wheels {
        let roll = carrier.tracks[wheel.side] / (wheel.radius * SCALE);
        transform.rotation = Quat::from_rotation_x(roll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
