use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::player::{Player, Respawn, vehicle::Driving};
use crate::world::{
    Body, Collider,
    cordon::{PERIMETER_GATE, PERIMETER_X, in_zone, inside_yard, near_wall_or_checkpoint},
    push_out,
    terrain::{QUARRY_CENTER, lattice_hash},
};

use super::downed::Downed;
use super::movement::{Wildlife, smooth};
use super::sound::MotionSound;
use super::{BASE, CreatureSound, Cue, HALF_SIZE, Kind, Route, SiteFauna, Terrain, site_fauna};

pub(super) const VIEW_DISTANCE: f32 = 125.0;
const SANCTUARY: f32 = 40.0;
const LEASH: f32 = 37.0;
const WINDUP: f32 = 0.52;
const LUNGE: f32 = 0.38;
const IMPACT: f32 = WINDUP + LUNGE * 0.58;
const SPECIES: [Kind; 4] = [Kind::Bovine, Kind::Chimera, Kind::Fleshdeer, Kind::Mutt];

pub(super) fn is_mutant(kind: Kind) -> bool {
    SPECIES.contains(&kind)
}

#[derive(Clone, Copy)]
enum Temperament {
    Neutral,
    Curious,
    Aggressive {
        notice: f32,
        reach: f32,
        sprint: f32,
    },
}

#[derive(Clone, Copy, Default)]
enum Activity {
    #[default]
    Patrol,
    Inspect(f32),
    Chase(f32),
    Attack {
        elapsed: f32,
        aim: Vec2,
    },
    Recover(f32),
}

#[derive(Component)]
pub(in crate::world) struct Mutant {
    home: Route,
    patrol: f32,
    activity: Activity,
    temperament: Temperament,
    walk: f32,
    body: Body,
}

impl Mutant {
    pub(super) fn new(kind: Kind, home: Route) -> Self {
        let (temperament, walk, radius) = match kind {
            Kind::Bovine => (Temperament::Neutral, 0.85, 0.59),
            Kind::Fleshdeer => (Temperament::Curious, 1.15, 0.41),
            Kind::Mutt => (
                Temperament::Aggressive {
                    notice: 22.0,
                    reach: 1.76,
                    sprint: 5.3,
                },
                1.25,
                0.36,
            ),
            Kind::Chimera => (
                Temperament::Aggressive {
                    notice: 27.0,
                    reach: 2.45,
                    sprint: 6.1,
                },
                1.1,
                0.53,
            ),
            _ => unreachable!("only mutant species have temperaments"),
        };
        Self {
            home,
            patrol: 0.0,
            activity: Activity::Patrol,
            temperament,
            walk,
            body: Body {
                radius,
                height: 1.0,
            },
        }
    }

    /// A fresh mutant of `kind` patrolling the same home.
    pub(super) fn reborn(&self, kind: Kind) -> Self {
        Self::new(kind, self.home)
    }

    fn decide(&mut self, from: Vec2, forward: Vec2, target: Option<Vec2>, dt: f32) -> Intent {
        let home_distance = from.distance(self.home.center);
        let toward = target.map(|at| at - from);
        let distance = toward.map_or(f32::INFINITY, |delta| delta.length());
        if matches!(self.activity, Activity::Patrol) && home_distance < LEASH - 5.0 {
            match self.temperament {
                Temperament::Neutral if distance < 5.0 => {
                    let away = -toward
                        .unwrap_or_default()
                        .try_normalize()
                        .unwrap_or(forward);
                    return Intent::toward(away * 5.0, 1.65 * dt);
                }
                Temperament::Curious if distance < 18.0 => self.activity = Activity::Inspect(7.5),
                Temperament::Aggressive { notice, .. } if distance < notice => {
                    self.activity = Activity::Chase(8.0);
                }
                _ => {}
            }
        }

        if let Some(toward) = toward.filter(|_| home_distance < LEASH) {
            let heading = toward.normalize_or_zero();
            match (self.activity, self.temperament) {
                (Activity::Inspect(left), Temperament::Curious)
                    if left > 0.0 && distance < 22.0 =>
                {
                    self.activity = Activity::Inspect(left - dt);
                    return Intent {
                        travel: heading * (distance - 4.2).clamp(0.0, 1.55 * dt),
                        facing: heading,
                        inspect: true,
                        ..default()
                    };
                }
                (
                    Activity::Chase(left),
                    Temperament::Aggressive {
                        notice,
                        reach,
                        sprint,
                    },
                ) if left > 0.0 && distance < notice + 8.0 => {
                    self.activity = if distance <= reach + 0.1 && forward.dot(heading) > 0.75 {
                        Activity::Attack {
                            elapsed: 0.0,
                            aim: heading,
                        }
                    } else {
                        Activity::Chase(left - dt)
                    };
                    return Intent {
                        travel: heading * (distance - reach).clamp(0.0, sprint * dt),
                        facing: heading,
                        ..default()
                    };
                }
                (Activity::Attack { elapsed, aim }, Temperament::Aggressive { reach, .. }) => {
                    return self.attack(elapsed, aim, toward, reach, dt);
                }
                _ => {}
            }
        }

        self.patrol_or_return(from, home_distance, dt)
    }

    fn attack(&mut self, elapsed: f32, aim: Vec2, toward: Vec2, reach: f32, dt: f32) -> Intent {
        let time = elapsed + dt;
        if time >= WINDUP + LUNGE {
            self.activity = Activity::Recover(6.0);
            return Intent::toward(-aim, self.walk * dt);
        }
        // Track the player while bracing, then commit to one direction so the strike can be dodged.
        let aim = if elapsed < WINDUP && toward != Vec2::ZERO {
            toward.normalize()
        } else {
            aim
        };
        self.activity = Activity::Attack { elapsed: time, aim };
        let progress = ((time - WINDUP) / LUNGE).max(0.0);
        let thrust = (progress * PI).sin();
        let travel = if time >= WINDUP {
            aim * (thrust * 5.2 * dt).min((toward.dot(aim) - reach * 0.52).max(0.0))
        } else {
            Vec2::ZERO
        };
        Intent {
            travel,
            facing: aim,
            lunge: if time < WINDUP {
                -smooth(time / WINDUP)
            } else {
                thrust - (1.0 - smooth(progress / 0.26))
            },
            impact: (elapsed < IMPACT && time >= IMPACT).then_some((aim, reach)),
            ..default()
        }
    }

    fn patrol_or_return(&mut self, from: Vec2, home_distance: f32, dt: f32) -> Intent {
        self.activity = match self.activity {
            Activity::Patrol if home_distance < LEASH - 1.0 => Activity::Patrol,
            Activity::Recover(left) if left > 0.0 || home_distance > 13.0 => {
                Activity::Recover(left - dt)
            }
            Activity::Recover(_) => Activity::Patrol,
            Activity::Inspect(_) => Activity::Recover(12.0),
            _ => Activity::Recover(6.0),
        };
        if matches!(self.activity, Activity::Recover(_)) && home_distance > 13.0 {
            return Intent::toward(self.home.center - from, self.walk * 1.4 * dt);
        }
        self.patrol += dt;
        let (goal, tangent) = self.home.sample(self.patrol);
        let mut intent = Intent::toward(goal - from, self.walk * dt);
        if intent.travel.length_squared() < (self.walk * dt * 0.3).powi(2) {
            intent.facing = tangent.normalize_or_zero();
        }
        intent
    }
}

#[derive(Default)]
struct Intent {
    travel: Vec2,
    facing: Vec2,
    lunge: f32,
    inspect: bool,
    impact: Option<(Vec2, f32)>,
}

impl Intent {
    fn toward(delta: Vec2, budget: f32) -> Self {
        Self {
            travel: delta.clamp_length_max(budget),
            facing: delta.normalize_or_zero(),
            ..default()
        }
    }
}

fn outside_base(at: Vec2) -> bool {
    at.distance_squared(Vec2::from(BASE)) > SANCTUARY * SANCTUARY
}

fn traversable(terrain: &Terrain, at: Vec2) -> bool {
    outside_base(at)
        && !near_wall_or_checkpoint(at, 1.0)
        && at.abs().max_element() < HALF_SIZE - 4.0
        && terrain.water_depth_at(at.x, at.y) < 0.08
        && terrain.normal_at(at.x, at.y).y > 0.88
}

fn step(
    terrain: &Terrain,
    from: Vec2,
    home: Vec2,
    intent: &Intent,
    body: Body,
    obstacles: &[(&Collider, &GlobalTransform)],
) -> Vec2 {
    let turns: &[f32] = if intent.lunge != 0.0 || intent.travel == Vec2::ZERO {
        &[0.0]
    } else {
        &[0.0, 0.58, -0.58, 1.14, -1.14]
    };
    if !obstacles.is_empty() {
        let clear = push_out(
            from,
            terrain.mesh_height_at(from),
            body,
            obstacles.iter().copied(),
        );
        if clear != from
            && clear.distance_squared(from) < 1.5_f32.powi(2)
            && clear.distance_squared(home) <= (LEASH + 2.0).powi(2)
            && traversable(terrain, clear)
        {
            return clear;
        }
    }
    let reach = intent.travel.length();
    for &turn in turns {
        let delta = Vec2::from_angle(turn).rotate(intent.travel);
        let point = from + delta;
        if point.distance_squared(home) > (LEASH + 2.0).powi(2) || !traversable(terrain, point) {
            continue;
        }
        if obstacles.is_empty() {
            return point;
        }
        let at = push_out(
            point,
            terrain.mesh_height_at(point),
            body,
            obstacles.iter().copied(),
        );
        if at.distance_squared(home) <= (LEASH + 2.0).powi(2)
            && (at == point
                || (at.distance_squared(from) <= (reach + 0.04).powi(2)
                    && (at - from).dot(delta) > reach * reach * 0.16
                    && traversable(terrain, at)))
        {
            return at;
        }
    }
    from
}

fn lands_hit(
    terrain: &Terrain,
    at: Vec3,
    player: Vec3,
    aim: Vec2,
    reach: f32,
    obstacles: &[(&Collider, &GlobalTransform)],
) -> bool {
    let delta = player.xz() - at.xz();
    if delta.length_squared() > reach * reach
        || delta.normalize_or_zero().dot(aim) < 0.80
        || !(-0.9..0.58).contains(&(player.y - at.y))
    {
        return false;
    }
    [0.25, 0.50, 0.75].into_iter().all(|t| {
        let point = at.xz() + t * delta;
        let body = Body {
            radius: 0.14,
            height: 1.0,
        };
        push_out(
            point,
            terrain.mesh_height_at(point),
            body,
            obstacles.iter().copied(),
        ) == point
    })
}

/// The mutants still on their feet.
type Mutants<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Mutant,
        &'static mut Wildlife,
        &'static mut Transform,
    ),
    (Without<Player>, Without<Downed>),
>;

pub(in crate::world) fn move_mutants(
    time: Res<Time>,
    terrain: Res<Terrain>,
    player: Query<(&Transform, Option<&Driving>), With<Player>>,
    mut mutants: Mutants,
    colliders: Query<(&Collider, &GlobalTransform)>,
    (mut respawns, mut sounds): (MessageWriter<Respawn>, MessageWriter<CreatureSound>),
) {
    let dt = time.delta_secs().min(0.050);
    let Ok((player, driving)) = player.single() else {
        return;
    };
    if dt <= 0.0 {
        return;
    }
    let target = (driving.is_none() && outside_base(player.translation.xz()))
        .then_some(player.translation.xz());
    let mut caught = false;
    for (mut mutant, mut animation, mut pose) in &mut mutants {
        let from = pose.translation.xz();
        if from.distance_squared(player.translation.xz()) > (VIEW_DISTANCE + 57.0).powi(2) {
            continue;
        }
        // Aggressive mutants leave the stalker be in the yard and in Harmont.
        let target = target.filter(|&at| {
            !(inside_yard(at) || !in_zone(at))
                || !matches!(mutant.temperament, Temperament::Aggressive { .. })
        });
        let before = MotionSound::before(pose.translation, &animation);
        let prior = mutant.activity;
        let intent = mutant.decide(from, (pose.rotation * Vec3::Z).xz(), target, dt);
        let obstacles: Vec<_> = colliders
            .iter()
            .filter(|(_, pose)| pose.translation().xz().distance_squared(from) < 18.0_f32.powi(2))
            .collect();
        let at = step(
            &terrain,
            from,
            mutant.home.center,
            &intent,
            mutant.body,
            &obstacles,
        );
        let facing = if intent.lunge == 0.0 && !intent.inspect && (at - from).length() > dt * 0.4 {
            at - from
        } else {
            intent.facing
        };
        animation.age += dt;
        animation.lunge = intent.lunge;
        animation.apply_travel(&mut pose, &terrain, at, facing, dt);
        let voice = pose.translation + Vec3::Y * animation.kind.model().1 * 0.36;
        match (prior, mutant.activity) {
            (Activity::Patrol, Activity::Chase(_)) => {
                sounds.write(CreatureSound::new(animation.kind, Cue::Growl, voice));
            }
            (Activity::Patrol, Activity::Inspect(_)) => {
                sounds.write(CreatureSound::new(animation.kind, Cue::Call, voice));
            }
            (Activity::Attack { elapsed: start, .. }, Activity::Attack { elapsed: end, .. })
                if start < WINDUP && end >= WINDUP =>
            {
                sounds.write(CreatureSound::new(animation.kind, Cue::Strike, voice));
            }
            _ => {}
        }
        before.emit(
            pose.translation,
            &animation,
            !matches!(mutant.activity, Activity::Patrol | Activity::Recover(_)),
            &mut sounds,
        );
        let gaze = if intent.inspect {
            let relative =
                pose.rotation.inverse() * Vec3::new(intent.facing.x, 0.0, intent.facing.y);
            relative.x.atan2(relative.z).clamp(-0.55, 0.55)
        } else {
            0.0
        };
        let blend = 1.0 - (-5.0 * dt).exp();
        animation.look += (gaze - animation.look) * blend;
        animation.attention +=
            (if intent.inspect { 1.0 } else { 0.0 } - animation.attention) * blend;
        if let Some((aim, reach)) = intent.impact
            && lands_hit(
                &terrain,
                pose.translation,
                player.translation,
                aim,
                reach,
                &obstacles,
            )
        {
            if !caught {
                sounds.write(CreatureSound::new(
                    animation.kind,
                    Cue::Impact,
                    player.translation + Vec3::Y * 0.65,
                ));
            }
            caught = true;
        }
    }
    if caught {
        respawns.write(Respawn);
    }
}

pub(super) fn routes(terrain: &Terrain) -> Vec<(Kind, Route)> {
    // Just across the Perimeter from the hangar, and in the quarry.
    let gate = PERIMETER_GATE;
    let quarry = Vec2::from(QUARRY_CENTER);
    let mut routes = [
        (Kind::Bovine, gate + Vec2::new(-181.0, 84.5), 16.0),
        (Kind::Fleshdeer, gate + Vec2::new(-181.0, -84.5), 22.0),
        (Kind::Mutt, gate + Vec2::new(-52.0, -193.0), 24.0),
        (Kind::Chimera, gate + Vec2::new(-52.0, 193.0), 28.0),
        (Kind::Chimera, quarry + Vec2::new(39.0, 26.0), 28.0),
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(i, (kind, site, spread))| {
        pick_route(terrain, kind, site, spread, i as i32, &[]).map(|route| (kind, route))
    })
    .collect::<Vec<_>>();
    for (index, site) in crate::world::sites::LANDMARKS.iter().enumerate() {
        if site_fauna(index) != SiteFauna::Mutant {
            continue;
        }
        let kind = SPECIES[(index / 3) % SPECIES.len()];
        if let Some(route) = pick_route(
            terrain,
            kind,
            site.center,
            90.0,
            900 + index as i32,
            &routes,
        ) {
            routes.push((kind, route));
        }
    }
    // A loose grid over the rest of the Zone.
    for z in -4_i32..=4 {
        for x in 0_i32..4 {
            let kind = SPECIES[(x + 2 * z).rem_euclid(4) as usize];
            let site = Vec2::new(PERIMETER_X - 190.0 - x as f32 * 380.0, z as f32 * 380.0);
            if let Some(route) = pick_route(terrain, kind, site, 154.0, 25 + x + 7 * z, &routes) {
                routes.push((kind, route));
            }
        }
    }
    routes
}

fn pick_route(
    terrain: &Terrain,
    kind: Kind,
    site: Vec2,
    spread: f32,
    seed: i32,
    placed: &[(Kind, Route)],
) -> Option<Route> {
    let mut candidates: Vec<_> = (0..24)
        .filter_map(|i| {
            let noise = |channel| lattice_hash(seed, i, 127_u32 + channel);
            let at = site + Vec2::new(noise(0) - 0.5, noise(1) - 0.5) * (2.0 * spread);
            // Mutants keep to the Zone, off the Perimeter wall.
            if !in_zone(at)
                || (at.x - PERIMETER_X).abs() < 40.0
                || !traversable(terrain, at)
                || crate::world::sites::clear_footprint(at, 8.0)
                || placed
                    .iter()
                    .any(|(_, route)| route.center.distance_squared(at) < 142.0_f32.powi(2))
            {
                return None;
            }
            let wood = terrain.woodland_at(at.x, at.y);
            let habitat = match kind {
                Kind::Bovine => 1.0 - wood,
                Kind::Fleshdeer => 1.0 - (wood - 0.68).abs(),
                Kind::Mutt => 1.0 - (wood - 0.45).abs(),
                _ => wood,
            };
            let route = Route {
                center: at,
                radius: Vec2::new(9.0, 6.5),
                phase: noise(2) * TAU,
                speed: 0.091 + noise(3) * 0.02,
            };
            Some((habitat + 0.27 * noise(4), route))
        })
        .collect();
    candidates.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    candidates
        .into_iter()
        .map(|(_, route)| route)
        .find(|route| {
            route.circuit().all(|at| {
                traversable(terrain, at)
                    && terrain.road_mask(at.x, at.y) < 0.36
                    && !crate::world::sites::clear_footprint(at, 5.0)
            })
        })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::world::Pillar;

    #[test]
    fn patrols_cover_the_zone_without_clustering_or_blocking_the_pad_and_roads() {
        let terrain = Terrain::new();
        let patrols = routes(&terrain);
        assert!(patrols.len() >= 36, "patrols: {}", patrols.len());
        for kind in SPECIES {
            let sites: Vec<_> = patrols.iter().filter(|(k, _)| *k == kind).collect();
            assert!(
                sites.len() >= 6,
                "{kind:?} has only {} patrols",
                sites.len()
            );
            assert!(
                sites
                    .iter()
                    .any(|(_, route)| { route.center.distance(PERIMETER_GATE) < 240.0 })
            );
        }
        for (i, (_, route)) in patrols.iter().enumerate() {
            assert!(route.circuit().all(|at| {
                traversable(&terrain, at)
                    && terrain.road_mask(at.x, at.y) < 0.36
                    && !crate::world::sites::clear_footprint(at, 5.0)
            }));
            assert!(
                patrols[..i]
                    .iter()
                    .all(|(_, other)| { route.center.distance(other.center) > 30.0 })
            );
        }
        // Mutants roam the whole Zone, north and south, and leave Harmont be.
        assert!(patrols.iter().all(|(_, route)| in_zone(route.center)));
        for z in [1.0, -1.0] {
            assert!(
                patrols
                    .iter()
                    .filter(|(_, route)| route.center.y * z > 300.0)
                    .count()
                    >= 8
            );
        }
    }

    struct Encounter {
        app: App,
        player: Entity,
        mutant: Entity,
        origin: Vec2,
        heading: Vec2,
        terrain: Terrain,
        hits: usize,
    }

    impl Encounter {
        fn new(kind: Kind, distance: f32) -> Self {
            let terrain = Terrain::new();
            let route = routes(&terrain)
                .into_iter()
                .find(|(k, _)| *k == kind)
                .unwrap()
                .1;
            let motion = Wildlife::new(kind, route, None);
            let mut pose = motion.initial_pose(&terrain);
            let origin = pose.translation.xz();
            // The levelest open approach, so a jump clears the strike as on flat ground.
            let rise = |direction: Vec2| {
                (terrain.mesh_height_at(origin + direction * 10.0) - terrain.mesh_height_at(origin))
                    .abs()
            };
            let heading = (0..24)
                .map(|i| Vec2::from_angle(i as f32 * TAU / 24.0))
                .filter(|&direction| {
                    (-12..24).all(|n| traversable(&terrain, origin + direction * (n as f32 * 0.5)))
                })
                .min_by(|a, b| rise(*a).total_cmp(&rise(*b)))
                .expect("an open approach beside the patrol");
            pose.rotation = Quat::from_rotation_y(heading.x.atan2(heading.y));
            let mut app = App::new();
            app.insert_resource(Time::<()>::default())
                .insert_resource(terrain.clone())
                .add_message::<Respawn>()
                .add_message::<CreatureSound>()
                .add_systems(Update, move_mutants);
            let player = app
                .world_mut()
                .spawn((Player::default(), Transform::IDENTITY))
                .id();
            let mutant = app
                .world_mut()
                .spawn((Mutant::new(kind, route), motion, pose))
                .id();
            let mut encounter = Self {
                app,
                player,
                mutant,
                origin,
                heading,
                terrain,
                hits: 0,
            };
            encounter.place_player(distance, 0.0);
            encounter
        }

        fn place_player(&mut self, distance: f32, lift: f32) {
            let point = self.origin + self.heading * distance;
            let mut pose = self
                .app
                .world_mut()
                .get_mut::<Transform>(self.player)
                .unwrap();
            pose.translation =
                Vec3::new(point.x, self.terrain.mesh_height_at(point) + lift, point.y);
        }

        fn tick(&mut self) -> (f32, f32) {
            self.app
                .world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(25));
            self.app.update();
            self.hits += self
                .app
                .world_mut()
                .resource_mut::<Messages<Respawn>>()
                .drain()
                .count();
            let pose = self.app.world().get::<Transform>(self.mutant).unwrap();
            let motion = self.app.world().get::<Wildlife>(self.mutant).unwrap();
            (
                (pose.translation.xz() - self.origin).dot(self.heading),
                motion.lunge,
            )
        }
    }

    #[test]
    fn a_neutral_bovine_yields_while_a_curious_fleshdeer_approaches_and_inspects() {
        let mut bovine = Encounter::new(Kind::Bovine, 3.5);
        let mut deer = Encounter::new(Kind::Fleshdeer, 9.0);
        for _ in 0..140 {
            assert!(bovine.tick().1 == 0.0 && deer.tick().1 == 0.0);
        }
        assert!(bovine.tick().0 < -0.5, "the bovine didn't yield");
        let (approach, _) = deer.tick();
        assert!((4.0..5.0).contains(&approach), "approached by {approach} m");
        let motion = deer.app.world().get::<Wildlife>(deer.mutant).unwrap();
        assert!(
            motion.attention > 0.9 && motion.pace < 0.05,
            "didn't pause to inspect"
        );
        assert_eq!(bovine.hits + deer.hits, 0);
    }

    #[test]
    fn predators_telegraph_one_strike_then_recover_and_the_player_can_jump_it() {
        for kind in [Kind::Mutt, Kind::Chimera] {
            for dodge in [false, true] {
                let mut encounter = Encounter::new(kind, 10.0);
                let mut windup_seen = false;
                let mut charged = false;
                let mut peak_lunge: f32 = 0.0;
                let mut fight_sounds = Vec::new();
                for _ in 0..160 {
                    let (travel, pose) = encounter.tick();
                    charged |= travel > 5.0;
                    if pose < -0.3 && !windup_seen {
                        assert_eq!(encounter.hits, 0, "hit before the warning");
                        windup_seen = true;
                        if dodge {
                            encounter.place_player(10.0, 0.68);
                        }
                    }
                    peak_lunge = peak_lunge.max(pose);
                    for sound in encounter
                        .app
                        .world_mut()
                        .resource_mut::<Messages<CreatureSound>>()
                        .drain()
                    {
                        if matches!(sound.cue, Cue::Growl | Cue::Strike | Cue::Impact) {
                            assert_eq!(sound.kind, kind);
                            assert!(sound.cue != Cue::Strike || windup_seen);
                            fight_sounds.push(sound.cue);
                        }
                    }
                }
                assert!(charged && windup_seen && peak_lunge > 0.8);
                assert_eq!(
                    encounter.hits,
                    usize::from(!dodge),
                    "{kind:?}, dodge: {dodge}"
                );
                let expected = if dodge {
                    &[Cue::Growl, Cue::Strike][..]
                } else {
                    &[Cue::Growl, Cue::Strike, Cue::Impact]
                };
                assert_eq!(fight_sounds, expected);
                assert_eq!(encounter.tick().1, 0.0);
            }
        }
    }

    #[test]
    fn armor_and_cover_protect_the_stalker() {
        let mut encounter = Encounter::new(Kind::Mutt, 8.0);
        let car = encounter.app.world_mut().spawn_empty().id();
        encounter
            .app
            .world_mut()
            .entity_mut(encounter.player)
            .insert(Driving(car));
        for _ in 0..100 {
            assert_eq!(encounter.tick().1, 0.0);
        }
        assert_eq!(encounter.hits, 0);

        let terrain = Terrain::new();
        let point = Vec2::from(BASE) + Vec2::new(60.0, 0.0);
        let feet = Vec3::new(point.x, terrain.mesh_height_at(point), point.y);
        let player = feet + 1.4 * Vec3::X;
        const TRUNK: &[Pillar] = &[Pillar::centered(0.25, 4.0)];
        let obstacle = GlobalTransform::from_translation(feet + 0.7 * Vec3::X);
        assert!(lands_hit(&terrain, feet, player, Vec2::X, 1.8, &[]));
        assert!(!lands_hit(
            &terrain,
            feet,
            player,
            Vec2::X,
            1.8,
            &[(&Collider(TRUNK), &obstacle)]
        ));
        assert!(!lands_hit(&terrain, feet, player, Vec2::NEG_X, 1.8, &[]));
    }
}
