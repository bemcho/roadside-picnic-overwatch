//! The Institute's two helicopters. Each rests on its own pad on the
//! International Institute's roof, spins its rotor up, lifts off and climbs to
//! two storeys above the roof, then flies a survey over the Zone: passes back
//! and forth across it west of the Perimeter, a different stretch each sortie.
//! It flies home, hovers over its pad, sets down and spins down. Rotors are
//! separate nodes (`tools/split_moving_parts.py`); the airframe banks into turns
//! and dips its nose with speed.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;

use super::WorldRoot;
use super::cordon::PERIMETER_X;
use super::places::{Describe, Place};
use super::sites::{institute_helipads, institute_top};
use super::terrain::{HALF_SIZE, Terrain, lattice_hash};

/// Each helicopter's model and meters per model unit (its length over the
/// rotor).
const MODELS: [(&str, f32); 2] = [
    (assets::INSTITUTE_HELICOPTER, 16.0),
    (assets::MILITARY_HELICOPTER, 18.0),
];
/// Two storeys over the Institute's highest point.
const ABOVE_ROOF: f32 = 7.0;
/// It never flies lower over the ground than this.
const CLEARANCE: f32 = 25.0;
/// Seconds ahead it looks at the ground.
const LOOK_AHEAD: f32 = 10.0;
const CRUISE: f32 = 32.0;
const ACCELERATION: f32 = 3.0;
/// Heading change, radians per second.
const TURN_RATE: f32 = 0.3;
/// Vertical speeds: climbing, cruising up and down, setting down.
const CLIMB: f32 = 4.0;
const DESCENT: f32 = 2.5;
/// Seconds for the rotor to spin up or down.
const SPIN_UP: f32 = 8.0;
/// Seconds on the roof between sorties; the second waits longer at first so
/// they fly staggered.
const LANDED: f32 = 120.0;
const FIRST_WAIT: [f32; 2] = [20.0, 110.0];
/// A waypoint counts as passed this close.
const PASSED: f32 = 40.0;
/// Within this of its pad it slides straight over it.
const HOVER_IN: f32 = 30.0;
/// Its passes: how many, how far apart, and where across the Zone they run.
const PASSES: usize = 3;
const PASS_SPACING: f32 = 160.0;
const EAST: f32 = PERIMETER_X - 120.0;
const WEST: f32 = PERIMETER_X - 950.0;
const EDGE: f32 = HALF_SIZE - 150.0;
/// Rotor turns per second at full speed.
const MAIN_ROTOR: f32 = 4.5;
const TAIL_ROTOR: f32 = 3.0 * MAIN_ROTOR;
const MAX_BANK: f32 = 0.35;
const MAX_PITCH: f32 = 0.12;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Landed,
    SpinUp,
    Climb,
    Cruise,
    Descend,
    SpinDown,
}

#[derive(Component)]
pub(super) struct Helicopter {
    pad: Vec3,
    phase: Phase,
    at: Vec3,
    /// Heading as a yaw about +Y (0 faces +Z), and how fast it is turning.
    yaw: f32,
    turning: f32,
    /// Horizontal speed along the heading.
    speed: f32,
    /// The rotor's speed, 0 to 1, and its angle in turns.
    rotor: f32,
    rotor_turns: f32,
    route: Vec<Vec2>,
    next: usize,
    sortie: u32,
    wait: f32,
    seed: u32,
}

impl Helicopter {
    fn new(pad: Vec3, seed: u32, wait: f32) -> Self {
        Self {
            pad,
            phase: Phase::Landed,
            at: pad,
            yaw: -PI / 2.0,
            turning: 0.0,
            speed: 0.0,
            rotor: 0.0,
            rotor_turns: 0.0,
            route: Vec::new(),
            next: 0,
            sortie: 0,
            wait,
            seed,
        }
    }

    /// Advances it by `dt` seconds at `cruise` altitude over `ground`.
    fn advance(&mut self, dt: f32, cruise: f32, ground: impl Fn(Vec2) -> f32) {
        match self.phase {
            Phase::Landed => {
                self.wait -= dt;
                if self.wait <= 0.0 {
                    self.phase = Phase::SpinUp;
                }
            }
            Phase::SpinUp => {
                self.rotor = (self.rotor + dt / SPIN_UP).min(1.0);
                if self.rotor >= 1.0 {
                    self.route = sortie(self.pad.xz(), self.sortie, self.seed);
                    self.sortie += 1;
                    self.next = 0;
                    self.phase = Phase::Climb;
                }
            }
            Phase::Climb => {
                self.at.y = (self.at.y + CLIMB * dt).min(cruise);
                self.steer(dt, 0.0);
                if self.at.y >= cruise {
                    self.phase = Phase::Cruise;
                }
            }
            Phase::Cruise => self.cruise(dt, cruise, ground),
            Phase::Descend => {
                let left = self.at.y - self.pad.y;
                let rate = DESCENT.min(0.3 + left * 0.5);
                self.at.y = (self.at.y - rate * dt).max(self.pad.y);
                if self.at.y <= self.pad.y {
                    self.phase = Phase::SpinDown;
                }
            }
            Phase::SpinDown => {
                self.rotor = (self.rotor - dt / SPIN_UP).max(0.0);
                if self.rotor <= 0.0 {
                    self.wait = LANDED;
                    self.phase = Phase::Landed;
                }
            }
        }
        self.rotor_turns = (self.rotor_turns + self.rotor * MAIN_ROTOR * dt).fract();
    }

    /// Flies the route at `cruise`, never lower than `CLEARANCE` over the
    /// ground, and ends over its pad.
    fn cruise(&mut self, dt: f32, cruise: f32, ground: impl Fn(Vec2) -> f32) {
        let home = self.next >= self.route.len();
        let target = if home {
            self.pad.xz()
        } else {
            self.route[self.next]
        };
        let to = target - self.at.xz();
        let distance = to.length();
        if !home && distance < PASSED {
            self.next += 1;
            return;
        }
        let limit = if home {
            CRUISE.min((2.0 * ACCELERATION * distance).sqrt())
        } else {
            CRUISE
        };
        self.steer(dt, limit);
        if home && distance < HOVER_IN {
            // Over the pad it slides straight in, whatever its heading.
            let step = (self.speed * dt).min(distance);
            self.at += (to.normalize_or_zero() * step).extend(0.0).xzy();
            if distance < 0.05 {
                self.at.x = self.pad.x;
                self.at.z = self.pad.z;
                self.speed = 0.0;
                self.phase = Phase::Descend;
            }
        } else {
            self.at += (self.facing() * self.speed * dt).extend(0.0).xzy();
        }
        // It looks a few seconds ahead so it climbs before a rise, not into it.
        let ahead = self.at.xz() + self.facing() * self.speed * LOOK_AHEAD;
        let floor = ground(self.at.xz()).max(ground(ahead)) + CLEARANCE;
        let height = cruise.max(floor);
        self.at.y += (height - self.at.y).clamp(-CLIMB * dt, CLIMB * dt);
    }

    /// Turns toward the next waypoint (or the pad) and eases its speed toward
    /// `limit`, slower while it is turning hard.
    fn steer(&mut self, dt: f32, limit: f32) {
        let target = self.route.get(self.next).copied().unwrap_or(self.pad.xz());
        let to = target - self.at.xz();
        let error = if to.length_squared() > 1.0 {
            self.facing().angle_to(to.normalize())
        } else {
            0.0
        };
        // `angle_to` is counterclockwise in x, z, which is a negative yaw.
        let turn = (-error).clamp(-TURN_RATE * dt, TURN_RATE * dt);
        self.yaw = (self.yaw + turn).rem_euclid(TAU);
        self.turning = turn / dt.max(1e-4);
        let target_speed = limit * (0.4 + 0.6 * error.cos().max(0.0));
        self.speed += (target_speed - self.speed).clamp(-ACCELERATION * dt, ACCELERATION * dt);
    }

    fn facing(&self) -> Vec2 {
        Vec2::new(self.yaw.sin(), self.yaw.cos())
    }

    /// Its pose: banked into turns, nose dipped with speed.
    fn pose(&self) -> Quat {
        let bank = -MAX_BANK * (self.turning / TURN_RATE).clamp(-1.0, 1.0);
        let pitch = MAX_PITCH * self.speed / CRUISE;
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(bank)
    }
}

/// A survey over the Zone: `PASSES` passes back and forth between `EAST` and
/// `WEST`, each `PASS_SPACING` farther north or south, starting from a stretch
/// picked by the sortie's number.
fn sortie(pad: Vec2, number: u32, seed: u32) -> Vec<Vec2> {
    let pick = |salt: i32| lattice_hash(number as i32, salt, seed);
    let start = pad.y + (pick(0) - 0.5) * 900.0;
    let step = if pick(1) < 0.5 {
        -PASS_SPACING
    } else {
        PASS_SPACING
    };
    let first = start.clamp(
        -EDGE + PASS_SPACING * PASSES as f32,
        EDGE - PASS_SPACING * PASSES as f32,
    );
    (0..PASSES)
        .flat_map(|pass| {
            let z = first + step * pass as f32;
            let (from, to) = if pass % 2 == 0 {
                (EAST, WEST)
            } else {
                (WEST, EAST)
            };
            [Vec2::new(from, z), Vec2::new(to.max(-EDGE), z)]
        })
        .collect()
}

/// A rotor node: the main rotor spins about its +Y, the tail rotor about its +X.
#[derive(Component)]
pub(super) struct Rotor {
    tail: bool,
    rest: Quat,
}

pub(super) fn spawn_helicopters(mut commands: Commands, server: Res<AssetServer>) {
    for (i, (pad, (model, scale))) in institute_helipads().into_iter().zip(MODELS).enumerate() {
        let helicopter = Helicopter::new(pad, 0x4e11 + i as u32, FIRST_WAIT[i]);
        commands
            .spawn((
                Name::new("Institute helicopter"),
                Transform::from_translation(pad)
                    .with_rotation(helicopter.pose())
                    .with_scale(Vec3::splat(scale)),
                helicopter,
                Describe {
                    place: Place::Helicopter,
                    radius: 0.45 * scale,
                    height: 0.3 * scale,
                },
                WorldRoot,
                WorldAssetRoot(assets::load_vehicle_scene(&server, model)),
            ))
            .observe(attach_rotors);
    }
}

fn attach_rotors(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    parts: Query<(&Name, &Transform)>,
) {
    for entity in children.iter_descendants(ready.entity) {
        let Ok((name, transform)) = parts.get(entity) else {
            continue;
        };
        let tail = match name.as_str() {
            "Rotor" => false,
            "TailRotor" => true,
            _ => continue,
        };
        commands.entity(entity).insert(Rotor {
            tail,
            rest: transform.rotation,
        });
    }
}

/// Flies each helicopter over the rendered ground.
pub(super) fn fly_helicopters(
    time: Res<Time>,
    terrain: Res<Terrain>,
    mut helicopters: Query<(&mut Helicopter, &mut Transform), Without<Rotor>>,
) {
    let dt = time.delta_secs();
    // The cruise height depends only on the Institute's fixed pose.
    let cruise = institute_top() + ABOVE_ROOF;
    for (mut helicopter, mut transform) in &mut helicopters {
        helicopter.advance(dt, cruise, |at| terrain.mesh_height_at(at));
        transform.translation = helicopter.at;
        transform.rotation = helicopter.pose();
    }
}

/// Turns each rotor to its helicopter's rotor angle.
pub(super) fn spin_rotors(
    helicopters: Query<&Helicopter>,
    parents: Query<&ChildOf>,
    mut rotors: Query<(Entity, &Rotor, &mut Transform)>,
) {
    for (entity, rotor, mut transform) in &mut rotors {
        let Some(helicopter) = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| helicopters.get(ancestor).ok())
        else {
            continue;
        };
        let spin = if rotor.tail {
            Quat::from_rotation_x(helicopter.rotor_turns * TAIL_ROTOR / MAIN_ROTOR * TAU)
        } else {
            Quat::from_rotation_y(helicopter.rotor_turns * TAU)
        };
        transform.rotation = rotor.rest * spin;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::in_zone;

    #[test]
    fn a_sortie_flies_back_and_forth_over_the_zone() {
        for number in 0..20 {
            let route = sortie(Vec2::new(850.0, 510.0), number, 7);
            assert_eq!(route.len(), 2 * PASSES);
            for point in &route {
                assert!(in_zone(*point), "{point}");
                assert!(point.abs().max_element() < HALF_SIZE, "{point}");
            }
            assert!(route[0].distance(route[1]) > 600.0);
        }
    }

    #[test]
    fn a_helicopter_lifts_off_surveys_the_zone_and_lands_on_its_pad() {
        let pad = Vec3::new(850.0, 30.0, 510.0);
        let cruise = 52.0;
        let mut helicopter = Helicopter::new(pad, 3, 5.0);
        let (mut highest, mut over_zone, mut seconds) = (0.0_f32, false, 0.0);
        let dt = 0.05;
        while seconds < 30.0 || helicopter.phase != Phase::Landed {
            helicopter.advance(dt, cruise, |_| 0.0);
            highest = highest.max(helicopter.at.y);
            over_zone |= in_zone(helicopter.at.xz());
            seconds += dt;
            assert!(
                seconds < 900.0,
                "stuck in {:?} at {}",
                helicopter.phase,
                helicopter.at
            );
        }
        assert!(over_zone);
        assert!((highest - cruise).abs() < 0.5, "{highest}");
        assert!(helicopter.at.distance(pad) < 0.01, "{}", helicopter.at);
        assert_eq!(helicopter.rotor, 0.0);
    }

    #[test]
    fn it_keeps_clear_of_high_ground() {
        let pad = Vec3::new(850.0, 30.0, 510.0);
        let mut helicopter = Helicopter::new(pad, 3, 0.0);
        let hill = |at: Vec2| if at.x < 600.0 { 60.0 } else { 0.0 };
        for _ in 0..(400.0 / 0.05) as usize {
            helicopter.advance(0.05, 52.0, hill);
            if helicopter.at.x < 560.0 && helicopter.phase == Phase::Cruise {
                assert!(
                    helicopter.at.y > 60.0 + CLEARANCE - 5.0,
                    "{}",
                    helicopter.at
                );
            }
        }
    }
}
