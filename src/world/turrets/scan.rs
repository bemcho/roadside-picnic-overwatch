//! How a Perimeter gun watches the Zone: its laser sweeps a sector facing it
//! while its reach nods in and out (a Lissajous scan over the 60 to 400 m belt),
//! and whatever unauthorized the beam touches, it locks onto.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::player::{Gait, Locomotion, Player, Respawn, vehicle::Driving};

use super::super::cordon::{CRAWL_HOLE, in_zone};
use super::super::ground_fog::{fog_tendency, in_ground_fog};
use super::super::places::{Describe, Place};
use super::super::terrain::{Terrain, lattice_hash};
use super::super::weather::{Weather, WorldClock};
use super::super::wildlife::Downed;
use super::fire::Lock;
use super::{Turret, ZONE_HEADING};

/// The sweep's half angle either side of the Zone's heading.
const SWEEP: f32 = PI / 3.0;
/// How fast the sweep's phases turn: the heading's and the reach's periods are
/// 26 s and 37 s, so together they cover the whole sector at every reach.
const SWEEP_RATE: f32 = TAU / 26.0;
const NOD_RATE: f32 = TAU / 37.0;
/// The beam's reach along the ground: nearest and farthest.
const REACH: (f32, f32) = (60.0, 400.0);
/// A locked gun lets a target go beyond this range or this far outside the Zone's heading.
const RANGE: f32 = 450.0;
const SECTOR: f32 = SWEEP + 0.26;
/// How likely a gun is to see a body standing in the ground fog, on each pass of its beam
/// over it: the fog scatters the laser.
const SEEN_IN_FOG: f32 = 0.6;
/// How close to the beam's ground track a body is caught, past its own radius.
const CATCH: f32 = 1.0;
/// A body is caught where the beam passes this little below its feet.
const BELOW_FEET: f32 = 0.3;
/// The stalker's height standing and prone, and their radius.
const STANDING: f32 = 1.8;
const PRONE: f32 = 0.5;
const STALKER_RADIUS: f32 = 0.35;
/// How near the crawl hole crossing the Perimeter counts as crawling through it.
const HOLE_REACH: f32 = 10.0;

/// The phases of a gun's scan: its heading's and its reach's.
#[derive(Clone, Copy, Debug)]
pub(super) struct Sweep {
    turn: f32,
    nod: f32,
}

impl Sweep {
    pub(super) fn new(phase: f32) -> Self {
        Self {
            turn: phase,
            nod: phase * 1.7,
        }
    }

    /// Picks the scan up from where the gun points and how far its beam reaches.
    fn resume(yaw: f32, reach: f32) -> Self {
        let (mid, half) = ((REACH.0 + REACH.1) * 0.5, (REACH.1 - REACH.0) * 0.5);
        Self {
            turn: (wrap(yaw - ZONE_HEADING) / SWEEP).clamp(-1.0, 1.0).asin(),
            nod: ((reach - mid) / half).clamp(-1.0, 1.0).asin(),
        }
    }

    pub(super) fn heading(self) -> f32 {
        ZONE_HEADING + SWEEP * self.turn.sin()
    }

    fn reach(self) -> f32 {
        REACH.0 + (REACH.1 - REACH.0) * (0.5 + 0.5 * self.nod.sin())
    }
}

/// The stalker is in the Zone without leave: they came in through the crawl hole.
#[derive(Component)]
pub(crate) struct Trespasser;

/// Something a gun may shoot: where its feet are, how wide and tall it is, which way it faces.
#[derive(Clone, Copy, Debug)]
pub(super) struct Target {
    pub(super) entity: Entity,
    pub(super) feet: Vec3,
    pub(super) radius: f32,
    pub(super) height: f32,
    pub(super) facing: Quat,
    pub(super) stalker: bool,
}

/// The angle `angle` wrapped into -PI..PI.
fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// The heading (`atan2(x, z)`) of a flat direction.
fn heading(direction: Vec2) -> f32 {
    direction.x.atan2(direction.y)
}

/// Whether the beam from `muzzle` down to `end`, swept this frame from heading
/// `from`, touches `target`: its body lies within `CATCH` of the beam's track
/// on the ground, between the muzzle and the end, and the beam passes there
/// between its feet and its head.
pub(super) fn touches(muzzle: Vec3, end: Vec3, from: f32, target: &Target) -> bool {
    let offset = (target.feet - muzzle).xz();
    let track = (end - muzzle).xz();
    let (distance, reach) = (offset.length(), track.length());
    let catch = CATCH + target.radius;
    if distance < catch || reach < 1.0 || distance > reach + catch {
        return false;
    }
    let (bearing, to) = (heading(offset), heading(track));
    let width = (catch / distance).min(1.0).asin();
    // The beam may have swept past the body since the last frame.
    let span = wrap(to - from);
    let swept = span.abs() > 1e-5 && (0.0..=1.0).contains(&(wrap(bearing - from) / span));
    if !swept && wrap(bearing - to).abs() > width && wrap(bearing - from).abs() > width {
        return false;
    }
    let beam = muzzle.y + (end.y - muzzle.y) * (distance / reach).min(1.0);
    (-BELOW_FEET..=target.height).contains(&(beam - target.feet.y))
}

/// A creature the guns shoot: one on the ground in the Zone.
fn creature_target(entity: Entity, pose: &Transform, describe: &Describe) -> Option<Target> {
    let Place::Creature(kind) = describe.place else {
        return None;
    };
    (!kind.is_bird() && in_zone(pose.translation.xz())).then_some(Target {
        entity,
        feet: pose.translation,
        radius: describe.radius,
        height: describe.height,
        facing: pose.rotation,
        stalker: false,
    })
}

/// The stalker as a target: only a trespasser on foot in the Zone; lower while crawling or prone.
fn stalker_target(
    entity: Entity,
    pose: &Transform,
    gait: Gait,
    (trespassing, driving): (bool, bool),
) -> Option<Target> {
    let prone = matches!(gait, Gait::Crawl | Gait::Prone);
    (trespassing && !driving && in_zone(pose.translation.xz())).then_some(Target {
        entity,
        feet: pose.translation,
        radius: STALKER_RADIUS,
        height: if prone { PRONE } else { STANDING },
        facing: pose.rotation,
        stalker: true,
    })
}

/// How a step of the stalker from `from` to `to` changes their leave to be in
/// the Zone: crossing in on foot by the crawl hole makes them a trespasser
/// (`Some(true)`), being on Harmont's side clears it (`Some(false)`). Through
/// the Institute gate, in the car or teleported (a restored save) they stay as they were.
pub(super) fn leave_after(from: Vec2, to: Vec2, driving: bool) -> Option<bool> {
    if !in_zone(to) {
        Some(false)
    } else if !in_zone(from) && !driving && to.distance(CRAWL_HOLE) < HOLE_REACH {
        Some(true)
    } else {
        None
    }
}

/// The stalker, whether they drive and whether they trespass.
type StalkerLeave<'w, 's> =
    Query<'w, 's, (Entity, &'static Transform, Has<Driving>, Has<Trespasser>), With<Player>>;

/// Marks the stalker a trespasser when they crawl into the Zone under the
/// wall, and clears it on Harmont's side or when they respawn.
pub(super) fn mark_trespassers(
    mut commands: Commands,
    mut respawns: MessageReader<Respawn>,
    stalker: StalkerLeave,
    mut last: Local<Option<Vec2>>,
) {
    let respawned = respawns.read().count() > 0;
    let Ok((entity, pose, driving, trespassing)) = stalker.single() else {
        return;
    };
    let at = pose.translation.xz();
    let leave = last.and_then(|from| leave_after(from, at, driving));
    *last = Some(at);
    match (respawned, leave) {
        (true, _) | (_, Some(false)) if trespassing => {
            commands.entity(entity).remove::<Trespasser>();
        }
        (false, Some(true)) if !trespassing => {
            commands.entity(entity).insert(Trespasser);
        }
        _ => {}
    }
}

/// Every body the guns may shoot this frame.
type Creatures<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Transform, &'static Describe),
    (Without<Downed>, Without<Player>),
>;
type Stalkers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static Locomotion,
        Has<Trespasser>,
        Has<Driving>,
    ),
    With<Player>,
>;

/// Sweeps each gun's beam, locks onto what it touches and turns locked guns after their target.
pub(super) fn aim_turrets(
    (time, clock, weather): (Res<Time>, Res<WorldClock>, Res<Weather>),
    terrain: Res<Terrain>,
    mut turrets: Query<&mut Turret>,
    creatures: Creatures,
    stalkers: Stalkers,
    (mut targets, mut touching): (Local<Vec<Target>>, Local<Vec<Target>>),
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let tendency = fog_tendency(&weather, clock.hour());
    targets.clear();
    targets.extend(
        creatures
            .iter()
            .filter_map(|(entity, pose, describe)| creature_target(entity, pose, describe)),
    );
    targets.extend(
        stalkers
            .iter()
            .filter_map(|(entity, pose, motion, trespassing, driving)| {
                stalker_target(entity, pose, motion.gait, (trespassing, driving))
            }),
    );
    for mut turret in &mut turrets {
        turret.swept = turret.yaw;
        let locked = turret
            .lock
            .and_then(|lock| {
                targets
                    .iter()
                    .find(|target| target.entity == lock.target.entity)
            })
            .filter(|target| turret.in_reach(target.feet))
            .copied();
        if let Some(target) = locked {
            turret.track(target, dt);
            continue;
        }
        if turret.lock.is_some() {
            turret.release();
        }
        turret.sweep(&terrain, dt);
        let muzzle = turret.muzzle_at();
        touching.clear();
        touching.extend(
            targets
                .iter()
                .filter(|target| touches(muzzle, turret.end, turret.swept, target)),
        );
        turret.forget_missed(&touching);
        if let Some(&target) = touching.iter().find(|target| turret.sees(target, tendency)) {
            turret.lock = Some(Lock::new(target));
        }
    }
}

impl Turret {
    /// Whether the gun sees `target`, which its beam touches: always in clear air; in the
    /// ground fog on one roll per pass of the beam over it, a miss kept in `unseen` until
    /// the beam leaves the body, so a slow pass does not roll again every frame.
    pub(super) fn sees(&mut self, target: &Target, tendency: f32) -> bool {
        if self.unseen.contains(&target.entity) {
            return false;
        }
        if !in_ground_fog(tendency, target.feet.y) {
            return true;
        }
        self.glances = self.glances.wrapping_add(1);
        let body = target.entity.to_bits() as u32;
        let roll = lattice_hash(self.glances as i32, body as i32, self.base.z.to_bits());
        let seen = roll < SEEN_IN_FOG;
        if !seen {
            self.unseen.push(target.entity);
        }
        seen
    }

    /// Forgets the bodies missed in the fog that the beam no longer touches.
    pub(super) fn forget_missed(&mut self, touching: &[Target]) {
        self.unseen
            .retain(|entity| touching.iter().any(|target| target.entity == *entity));
    }

    /// Turns the scan on and drops the beam onto the ground where it now reaches.
    pub(super) fn sweep(&mut self, terrain: &Terrain, dt: f32) {
        self.scan.turn += SWEEP_RATE * dt;
        self.scan.nod += NOD_RATE * dt;
        self.yaw = self.scan.heading();
        let at = self.base.xz() + self.facing() * self.scan.reach();
        self.end = Vec3::new(at.x, terrain.mesh_height_at(at), at.y);
    }

    /// Turns the head after a locked target, as fast as the gun traverses; the beam ends at its feet.
    fn track(&mut self, target: Target, dt: f32) {
        let offset = (target.feet - self.base).xz();
        let step = self.gun.traverse() * dt;
        self.yaw += wrap(heading(offset) - self.yaw).clamp(-step, step);
        let at = self.base.xz() + self.facing() * offset.length();
        self.end = Vec3::new(at.x, target.feet.y, at.y);
        if let Some(lock) = &mut self.lock {
            lock.target = target;
        }
    }

    /// Lets the target go and resumes the scan from where the gun points.
    pub(super) fn release(&mut self) {
        self.lock = None;
        self.scan = Sweep::resume(self.yaw, (self.end - self.base).xz().length());
    }

    /// Whether a locked gun can still follow a body at `feet`.
    fn in_reach(&self, feet: Vec3) -> bool {
        let offset = (feet - self.base).xz();
        offset.length() <= RANGE && wrap(heading(offset) - ZONE_HEADING).abs() <= SECTOR
    }

    /// How far off its aim the head points from a body at `feet`, in radians.
    pub(super) fn aim_error(&self, feet: Vec3) -> f32 {
        wrap(heading((feet - self.base).xz()) - self.yaw).abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::cordon::{PERIMETER_GATE, PERIMETER_X};

    /// A beam from a muzzle 10.4 m above flat ground at the wall, reaching 100 m into the Zone.
    const MUZZLE: Vec3 = Vec3::new(PERIMETER_X, 10.4, 0.0);
    const END: Vec3 = Vec3::new(PERIMETER_X - 100.0, 0.0, 0.0);

    fn stalker(x: f32, z: f32, height: f32) -> Target {
        Target {
            entity: Entity::PLACEHOLDER,
            feet: Vec3::new(x, 0.0, z),
            radius: STALKER_RADIUS,
            height,
            facing: Quat::IDENTITY,
            stalker: true,
        }
    }

    #[test]
    fn in_fog_a_gun_sees_six_passes_in_ten_and_rolls_once_a_pass() {
        use super::super::{Effects, Gun};

        let placeholder = Entity::PLACEHOLDER;
        let effects = Effects {
            beam: placeholder,
            spot: placeholder,
            flash: placeholder,
        };
        let mut gun = Turret::new(Gun::Gatling, MUZZLE, 0.0, effects);
        let body = stalker(PERIMETER_X - 99.0, 0.0, STANDING);
        assert!(gun.sees(&body, 0.0), "clear air");
        let passes = 4000;
        let mut seen = 0;
        for _ in 0..passes {
            if gun.sees(&body, 1.0) {
                seen += 1;
                continue;
            }
            assert!(
                !gun.sees(&body, 1.0),
                "rolled again while the beam stays on it"
            );
            // The beam moves off the body.
            gun.forget_missed(&[]);
        }
        let share = seen as f32 / passes as f32;
        assert!(
            (0.57..0.63).contains(&share),
            "seen on {share} of the passes"
        );
    }

    #[test]
    fn the_beam_passes_over_a_near_stalker_and_catches_one_where_it_meets_the_ground() {
        let still = ZONE_HEADING;
        let near = stalker(PERIMETER_X - 20.0, 0.0, STANDING);
        assert!(!touches(MUZZLE, END, still, &near), "8 m overhead");
        let at_the_end = stalker(PERIMETER_X - 99.0, 0.0, STANDING);
        assert!(touches(MUZZLE, END, still, &at_the_end));
        // A little nearer the beam is 1 m up: over a crawler, into a standing body.
        let crawler = stalker(PERIMETER_X - 90.0, 0.0, PRONE);
        assert!(!touches(MUZZLE, END, still, &crawler));
        let standing = stalker(PERIMETER_X - 90.0, 0.0, STANDING);
        assert!(touches(MUZZLE, END, still, &standing));
        // Beside the track, or beyond its end, the beam misses.
        assert!(!touches(
            MUZZLE,
            END,
            still,
            &stalker(PERIMETER_X - 99.0, 3.0, STANDING)
        ));
        assert!(!touches(
            MUZZLE,
            END,
            still,
            &stalker(PERIMETER_X - 110.0, 0.0, STANDING)
        ));
    }

    #[test]
    fn a_fast_sweep_still_catches_what_it_passed_over_since_the_last_frame() {
        let beside = stalker(PERIMETER_X - 99.0, 6.0, STANDING);
        assert!(!touches(MUZZLE, END, ZONE_HEADING, &beside));
        // Last frame the beam pointed 6 degrees the other side of the body.
        let from = ZONE_HEADING + 0.1;
        assert!(touches(MUZZLE, END, from, &beside));
        assert!(!touches(MUZZLE, END, ZONE_HEADING - 0.1, &beside));
    }

    #[test]
    fn the_scan_covers_the_sector_and_the_belt_and_resumes_where_it_points() {
        let samples = (0..2000).map(|i| {
            let mut sweep = Sweep::new(0.4);
            sweep.turn += SWEEP_RATE * i as f32 * 0.25;
            sweep.nod += NOD_RATE * i as f32 * 0.25;
            sweep
        });
        let (mut left, mut right, mut near, mut far) = (false, false, false, false);
        for sweep in samples {
            let off = wrap(sweep.heading() - ZONE_HEADING);
            assert!(off.abs() <= SWEEP + 1e-4);
            assert!((REACH.0 - 1e-3..=REACH.1 + 1e-3).contains(&sweep.reach()));
            left |= off < -SWEEP * 0.95;
            right |= off > SWEEP * 0.95;
            near |= sweep.reach() < 70.0;
            far |= sweep.reach() > 390.0;
        }
        assert!(left && right && near && far);
        let resumed = Sweep::resume(ZONE_HEADING + 0.3, 250.0);
        assert!((resumed.heading() - (ZONE_HEADING + 0.3)).abs() < 1e-4);
        assert!((resumed.reach() - 250.0).abs() < 1e-2);
    }

    #[test]
    fn only_trespassers_on_foot_and_ground_creatures_in_the_zone_are_targets() {
        use crate::world::CreatureKind;
        let zone = Transform::from_xyz(PERIMETER_X - 50.0, 0.0, 0.0);
        let harmont = Transform::from_xyz(PERIMETER_X + 50.0, 0.0, 0.0);
        let id = Entity::PLACEHOLDER;
        let standing = stalker_target(id, &zone, Gait::Walk, (true, false));
        assert_eq!(standing.map(|t| t.height), Some(STANDING));
        let crawling = stalker_target(id, &zone, Gait::Crawl, (true, false));
        assert_eq!(crawling.map(|t| t.height), Some(PRONE));
        assert!(stalker_target(id, &zone, Gait::Walk, (false, false)).is_none());
        assert!(stalker_target(id, &zone, Gait::Walk, (true, true)).is_none());
        assert!(stalker_target(id, &harmont, Gait::Walk, (true, false)).is_none());
        let describe = |place| Describe {
            place,
            radius: 1.0,
            height: 1.0,
        };
        let deer = describe(Place::Creature(CreatureKind::Hexadeer));
        assert!(creature_target(id, &zone, &deer).is_some());
        assert!(creature_target(id, &harmont, &deer).is_none());
        let mutt = describe(Place::Creature(CreatureKind::Mutt));
        assert!(creature_target(id, &zone, &mutt).is_some());
        let hawk = describe(Place::Creature(CreatureKind::Hawk));
        assert!(creature_target(id, &zone, &hawk).is_none());
        for place in [Place::Vehicle, Place::Patrol, Place::Carrier, Place::Guard] {
            assert!(creature_target(id, &zone, &describe(place)).is_none());
        }
    }

    #[test]
    fn the_crawl_hole_makes_a_trespasser_the_gate_and_the_car_do_not() {
        let side = |x: f32, at: Vec2| Vec2::new(x, at.y);
        let hole = CRAWL_HOLE;
        let (harmont, zone) = (PERIMETER_X + 2.0, PERIMETER_X - 2.0);
        assert_eq!(
            leave_after(side(harmont, hole), side(zone, hole), false),
            Some(true)
        );
        let gate = PERIMETER_GATE;
        assert_eq!(
            leave_after(side(harmont, gate), side(zone, gate), false),
            None
        );
        assert_eq!(
            leave_after(side(harmont, hole), side(zone, hole), true),
            None
        );
        // Inside the Zone nothing changes; back on Harmont's side the mark clears.
        assert_eq!(
            leave_after(side(zone - 5.0, hole), side(zone - 6.0, hole), false),
            None
        );
        assert_eq!(
            leave_after(side(zone, hole), side(harmont, hole), false),
            Some(false)
        );
    }

    #[test]
    fn crawling_through_the_hole_marks_the_stalker_and_a_respawn_clears_it() {
        let mut app = App::new();
        app.add_message::<Respawn>()
            .add_systems(Update, mark_trespassers);
        let outside = Vec3::new(PERIMETER_X + 2.0, 0.0, CRAWL_HOLE.y);
        let id = app
            .world_mut()
            .spawn((Player::default(), Transform::from_translation(outside)))
            .id();
        app.update();
        app.world_mut()
            .get_mut::<Transform>(id)
            .expect("stalker")
            .translation
            .x = PERIMETER_X - 2.0;
        app.update();
        assert!(app.world().get::<Trespasser>(id).is_some());
        app.update();
        assert!(app.world().get::<Trespasser>(id).is_some(), "stays marked");
        app.world_mut().write_message(Respawn);
        app.update();
        assert!(app.world().get::<Trespasser>(id).is_none());
    }
}
