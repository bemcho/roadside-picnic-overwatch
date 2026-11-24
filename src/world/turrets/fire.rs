//! How a Perimeter gun shoots: locked on, it spins its barrels up, fires
//! tracer bursts while aimed and brings the target down once enough rounds
//! have landed; then its barrels spin down and the scan resumes.

use bevy::prelude::*;

use crate::player::Respawn;

use super::super::terrain::lattice_hash;
use super::super::wildlife::Downed;
use super::scan::Target;
use super::{Gun, Tracer, Turret};

/// Seconds for the barrels to spin up to firing speed, and to stop again.
const SPIN_UP: f32 = 1.0;
const SPIN_DOWN: f32 = 1.6;
/// The gun fires only while pointing this close to its target, in radians (3 degrees).
const AIMED: f32 = 0.052;
/// Seconds of fire in a burst and between bursts.
const BURST: f32 = 0.6;
const PAUSE: f32 = 0.35;
/// Seconds of fire on target that bring a creature, or the stalker, down
/// (once the rounds have flown there).
const CREATURE_HITS: f32 = 0.3;
const STALKER_HITS: f32 = 0.5;
/// How far a tracer flies at most, in meters.
const TRACER_RANGE: f32 = 480.0;

/// A gun fires a burst at `at`, its muzzle.
#[derive(Message, Clone, Copy, Debug)]
pub(crate) struct GunSound {
    pub at: Vec3,
    pub gun: Gun,
}

/// A gun's hold on a target, and its fire at it so far.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lock {
    pub(super) target: Target,
    /// Seconds spent firing, bursts and pauses, of which `hits` on target.
    firing: f32,
    hits: f32,
    /// Rounds owed to the next tracer.
    shots: f32,
}

impl Lock {
    pub(super) fn new(target: Target) -> Self {
        Self {
            target,
            firing: 0.0,
            hits: 0.0,
            shots: 0.0,
        }
    }
}

impl Gun {
    /// How fast the head turns, in radians per second.
    pub(super) fn traverse(self) -> f32 {
        match self {
            Gun::Gatling => 1.4,
            Gun::Naval => 0.9,
        }
    }

    /// Tracers a second (one round in several carries one), their speed in m/s and their spread in radians.
    fn tracers(self) -> (f32, f32, f32) {
        match self {
            Gun::Gatling => (14.0, 450.0, 0.010),
            Gun::Naval => (6.0, 380.0, 0.005),
        }
    }

    /// How fast the barrels spin at firing speed, in radians per second, and how big a tracer is.
    fn barrels(self) -> (f32, f32) {
        match self {
            Gun::Gatling => (42.0, 1.0),
            Gun::Naval => (9.0, 1.6),
        }
    }
}

/// What a gun did this frame: started a burst, fired tracers, brought a target down.
#[derive(Default, Debug)]
pub(super) struct Volley {
    burst: bool,
    shots: u32,
    hit: Option<Target>,
}

impl Turret {
    /// Spins the barrels and, locked on and aimed, fires in bursts; the target
    /// falls once enough fire has reached it, and the gun lets it go.
    fn shoot(&mut self, dt: f32) -> Volley {
        let spin = if self.lock.is_some() {
            dt / SPIN_UP
        } else {
            -dt / SPIN_DOWN
        };
        self.spin = (self.spin + spin).clamp(0.0, 1.0);
        self.turn += self.spin * self.gun.barrels().0 * dt;
        self.flash = false;
        let mut volley = Volley::default();
        let Some(mut lock) = self.lock else {
            return volley;
        };
        if self.spin < 1.0 || self.aim_error(lock.target.feet) > AIMED {
            return volley;
        }
        let cycle = BURST + PAUSE;
        let before = lock.firing;
        lock.firing += dt;
        if lock.firing % cycle < BURST {
            volley.burst =
                before == 0.0 || (before / cycle).floor() < (lock.firing / cycle).floor();
            lock.hits += dt;
            lock.shots += dt * self.gun.tracers().0;
            volley.shots = lock.shots as u32;
            lock.shots = lock.shots.fract();
            self.flash = true;
        }
        self.lock = Some(lock);
        let flight = self.muzzle_at().distance(lock.target.feet) / self.gun.tracers().1;
        let needed = if lock.target.stalker {
            STALKER_HITS
        } else {
            CREATURE_HITS
        };
        if lock.hits >= needed + flight {
            volley.hit = Some(lock.target);
            self.release();
        }
        volley
    }

    /// A tracer from the muzzle at `target`, spread by round, flying until it reaches it.
    fn tracer(&self, target: &Target) -> (Tracer, Transform) {
        let (_, speed, spread) = self.gun.tracers();
        let muzzle = self.muzzle_at();
        let aim = target.feet + Vec3::Y * target.height * 0.5;
        let jitter = |channel| {
            (lattice_hash(self.rounds as i32, self.base.z as i32, channel) * 2.0 - 1.0) * spread
        };
        let direction = (aim - muzzle).normalize_or(Vec3::NEG_X);
        let velocity = Quat::from_euler(EulerRot::YXZ, jitter(0x7AC3), jitter(0x7AC4), 0.0)
            * direction
            * speed;
        let pose = Transform::from_translation(muzzle)
            .looking_to(velocity, Vec3::Y)
            .with_scale(Vec3::splat(self.gun.barrels().1));
        let life = muzzle.distance(aim).min(TRACER_RANGE) / speed;
        (Tracer { velocity, life }, pose)
    }
}

/// How bright the muzzle flash flickers, from 0.55 to 1, as the barrels turn.
pub(super) fn flicker(turn: f32) -> f32 {
    0.55 + 0.45 * (turn * 7.3).sin().abs()
}

/// Which side a creature hit from `muzzle` falls to: away from the gun.
fn fall_side(target: &Target, muzzle: Vec3) -> f32 {
    let right = target.facing * Vec3::X;
    if right.dot(target.feet - muzzle) >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// Fires the locked guns: burst sounds, tracers from the pool and what they bring down.
pub(super) fn fire_turrets(
    time: Res<Time>,
    mut commands: Commands,
    mut turrets: Query<&mut Turret>,
    mut tracers: Query<(&mut Tracer, &mut Transform, &mut Visibility)>,
    (mut respawns, mut sounds): (MessageWriter<Respawn>, MessageWriter<GunSound>),
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let mut idle = tracers.iter_mut().filter(|(tracer, ..)| tracer.life <= 0.0);
    for mut turret in &mut turrets {
        let aim = turret.lock.map(|lock| lock.target);
        let volley = turret.shoot(dt);
        let muzzle = turret.muzzle_at();
        if volley.burst {
            sounds.write(GunSound {
                at: muzzle,
                gun: turret.gun,
            });
        }
        if let Some(target) = aim {
            for _ in 0..volley.shots {
                turret.rounds = turret.rounds.wrapping_add(1);
                let Some((mut tracer, mut pose, mut visibility)) = idle.next() else {
                    break;
                };
                (*tracer, *pose) = turret.tracer(&target);
                *visibility = Visibility::Inherited;
            }
        }
        match volley.hit {
            Some(target) if target.stalker => {
                respawns.write(Respawn);
            }
            Some(target) => {
                let side = fall_side(&target, muzzle);
                commands.entity(target.entity).try_insert(Downed::new(side));
            }
            None => {}
        }
    }
}

/// Flies the tracers in flight and puts them back in the pool when spent.
pub(super) fn fly_tracers(
    time: Res<Time>,
    mut tracers: Query<(&mut Tracer, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    for (mut tracer, mut pose, mut visibility) in &mut tracers {
        if tracer.life <= 0.0 {
            continue;
        }
        tracer.life -= dt;
        pose.translation += tracer.velocity * dt;
        if tracer.life <= 0.0 {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::cordon::PERIMETER_X;
    use super::super::{Effects, ZONE_HEADING};
    use super::*;

    fn locked_gun(gun: Gun, stalker: bool) -> Turret {
        let effects = Effects {
            beam: Entity::PLACEHOLDER,
            spot: Entity::PLACEHOLDER,
            flash: Entity::PLACEHOLDER,
        };
        let mut turret = Turret::new(gun, Vec3::new(PERIMETER_X, 9.0, 0.0), 0.0, effects);
        turret.yaw = ZONE_HEADING;
        turret.lock = Some(Lock::new(Target {
            entity: Entity::PLACEHOLDER,
            feet: Vec3::new(PERIMETER_X - 150.0, 0.0, 0.0),
            radius: 0.5,
            height: 1.0,
            facing: Quat::IDENTITY,
            stalker,
        }));
        turret
    }

    /// Seconds until the gun brings its target down, with its first burst's time and the tracers fired.
    fn fire_until_hit(turret: &mut Turret) -> (f32, f32, u32) {
        let (mut time, mut first, mut shots) = (0.0, f32::INFINITY, 0);
        while time < 10.0 {
            time += 0.02;
            let volley = turret.shoot(0.02);
            if volley.burst {
                first = first.min(time);
            }
            shots += volley.shots;
            if volley.hit.is_some() {
                return (time, first, shots);
            }
        }
        panic!("the gun never brought its target down");
    }

    #[test]
    fn a_gun_spins_up_fires_bursts_and_brings_a_creature_down_then_spins_down() {
        let mut gun = locked_gun(Gun::Gatling, false);
        let (hit, first, shots) = fire_until_hit(&mut gun);
        assert!((1.0..1.1).contains(&first), "first burst at {first} s");
        let flight = 150.0 / gun.gun.tracers().1;
        assert!(
            hit >= first + CREATURE_HITS + flight - 0.05,
            "hit at {hit} s"
        );
        assert!(hit < 3.5 && shots >= 5, "{hit} s, {shots} tracers");
        assert!(gun.lock.is_none(), "it lets the target go");
        gun.shoot(1.0);
        assert!(gun.spin < 0.5 && !gun.flash);
    }

    #[test]
    fn the_stalker_takes_longer_and_a_gun_off_aim_holds_fire() {
        let (creature, ..) = fire_until_hit(&mut locked_gun(Gun::Naval, false));
        let (stalker, ..) = fire_until_hit(&mut locked_gun(Gun::Naval, true));
        assert!(stalker > creature);
        let mut gun = locked_gun(Gun::Gatling, true);
        gun.yaw += 0.2;
        for _ in 0..200 {
            let volley = gun.shoot(0.02);
            assert!(!volley.burst && volley.shots == 0 && volley.hit.is_none());
        }
    }

    #[test]
    fn tracers_fly_from_the_muzzle_to_the_target_and_no_further() {
        let gun = locked_gun(Gun::Gatling, false);
        let target = gun.lock.map(|lock| lock.target).expect("locked");
        let (tracer, pose) = gun.tracer(&target);
        assert_eq!(pose.translation, gun.muzzle_at());
        let landing = pose.translation + tracer.velocity * tracer.life;
        let aim = target.feet + Vec3::Y * 0.5;
        assert!(
            landing.distance(aim) < 3.0,
            "lands {} m off",
            landing.distance(aim)
        );
        let (_, speed, _) = Gun::Gatling.tracers();
        assert!((tracer.velocity.length() - speed).abs() < 0.01);
    }
}
