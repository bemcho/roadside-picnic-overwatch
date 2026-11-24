use bevy::prelude::*;

use super::downed::Downed;
use super::mutants::Mutant;
use super::sound::MotionSound;
use super::{BASE, CreatureSound, Cue, HALF_SIZE, Kind, Route, Terrain};

const REACH: f32 = 2.15;
const STRIKE_TIME: f32 = 0.60;
const HUNT_RANGE: f32 = 23.0;
const PURSUIT_LEASH: f32 = 31.0;

#[derive(Clone, Copy)]
pub(super) struct Flight {
    pub cruise: f32,
    pub lands: bool,
}

impl Flight {
    fn lift(self, age: f32) -> f32 {
        if !self.lands {
            return 1.0;
        }
        let t = age.rem_euclid(36.0);
        match t {
            ..18.0 => 1.0,
            ..22.0 => 1.0 - smooth((t - 18.0) / 4.0),
            ..31.0 => 0.0,
            _ => smooth((t - 31.0) / 5.0),
        }
    }
}

#[derive(Clone, Copy, Default)]
enum Hunt {
    #[default]
    Patrol,
    Chase {
        prey: Entity,
        until: f32,
    },
    Strike {
        prey: Entity,
        since: f32,
    },
    Recover {
        until: f32,
    },
}

impl Hunt {
    fn target(self) -> Option<Entity> {
        match self {
            Self::Chase { prey, .. } | Self::Strike { prey, .. } => Some(prey),
            _ => None,
        }
    }
}

#[derive(Component)]
pub(in crate::world) struct Wildlife {
    pub(super) kind: Kind,
    route: Route,
    flight: Option<Flight>,
    hunt: Hunt,
    route_time: f32,
    pub(super) age: f32,
    pub(super) gait: f32,
    pub(super) pace: f32,
    pub(super) run: f32,
    pub(super) air: f32,
    pub(super) lunge: f32,
    pub(super) look: f32,
    pub(super) attention: f32,
}

impl Wildlife {
    pub(super) fn new(kind: Kind, route: Route, flight: Option<Flight>) -> Self {
        Self {
            kind,
            route,
            flight,
            hunt: Hunt::Recover {
                until: 4.0 + route.phase.rem_euclid(4.0),
            },
            route_time: 0.0,
            age: 0.0,
            gait: route.phase.fract(),
            pace: 0.0,
            run: 0.0,
            air: flight.map_or(0.0, |flight| flight.lift(0.0)),
            lunge: 0.0,
            look: 0.0,
            attention: 0.0,
        }
    }

    /// A fresh animal of the same kind at the start of the same route.
    pub(super) fn reborn(&self) -> Self {
        Self::new(self.kind, self.route, self.flight)
    }

    pub(super) fn initial_pose(&self, terrain: &Terrain) -> Transform {
        let (at, direction) = self.route.sample(0.0);
        Transform::from_xyz(at.x, self.height(terrain, at), at.y)
            .with_rotation(heading(direction))
            .with_scale(Vec3::splat(self.kind.model().1))
    }

    pub(super) fn phase(&self) -> f32 {
        self.route.phase
    }

    pub(super) fn call_seed(&self) -> f32 {
        let cell = self.route.center.as_ivec2();
        let salt = ((self.phase() + 10.0) * 247.0) as u32;
        crate::world::terrain::lattice_hash(cell.x, cell.y, 0xCA11 + salt + self.kind as u32)
    }

    fn height(&self, terrain: &Terrain, at: Vec2) -> f32 {
        let ground = terrain.mesh_height_at(at) + 0.012;
        self.flight.map_or(ground, |flight| {
            let wave = ((self.age + self.route.phase) * 0.72).sin();
            ground + self.air * (flight.cruise + 0.65 * wave - ground)
        })
    }

    fn step(
        &mut self,
        entity: Entity,
        pose: &mut Transform,
        terrain: &Terrain,
        dt: f32,
        nearby: &[Animal],
    ) {
        self.age += dt;
        self.air = self.flight.map_or(0.0, |flight| flight.lift(self.age));
        self.route_time += dt
            * self.flight.map_or(1.0, |flight| {
                if flight.lands {
                    0.14 + 0.86 * self.air
                } else {
                    1.0
                }
            });
        if self.kind == Kind::Chameleon {
            return;
        }
        let (patrol, tangent) = self.route.sample(self.route_time);
        let from = pose.translation.xz();
        let (at, facing) = if self.flight.is_some() {
            (patrol, tangent)
        } else {
            self.step_ground(entity, terrain, from, patrol, dt, nearby)
        };
        self.apply_travel(pose, terrain, at, facing, dt);
    }

    pub(super) fn apply_travel(
        &mut self,
        pose: &mut Transform,
        terrain: &Terrain,
        at: Vec2,
        facing: Vec2,
        dt: f32,
    ) {
        let from = pose.translation.xz();
        let delta = at - from;
        let speed = delta.length() / dt;
        let fade = 1.0 - (-10.0 * dt).exp();
        self.pace += (smooth(speed / 0.28) - self.pace) * fade;
        self.run += (smooth((speed - 1.55) / 2.75) - self.run) * fade;
        let stride = super::rig::stride(self.kind, self.run) * pose.scale.x;
        self.gait = (self.gait + delta.length() * 0.59 / stride).fract();
        pose.translation = Vec3::new(at.x, self.height(terrain, at), at.y);
        if facing.length_squared() > 0.00001 {
            let bank = self.air * (-0.16 + ((self.age + self.phase()) * 1.3).sin() * 0.075);
            pose.rotation = pose.rotation.slerp(
                heading(facing) * Quat::from_rotation_z(bank),
                1.0 - (-8.0 * dt).exp(),
            );
        }
    }

    fn step_ground(
        &mut self,
        entity: Entity,
        terrain: &Terrain,
        from: Vec2,
        patrol: Vec2,
        dt: f32,
        nearby: &[Animal],
    ) -> (Vec2, Vec2) {
        self.lunge = 0.0;
        if self.kind == Kind::Frostwolf
            && let Some(step) = self.hunt(terrain, from, dt, nearby)
        {
            return step;
        }
        if self.kind == Kind::Hexadeer
            && from.distance(self.route.center) < HUNT_RANGE
            && let Some(wolf) = nearby
                .iter()
                .filter(|other| other.hunt.target() == Some(entity))
                .min_by(|a, b| {
                    from.distance_squared(a.at)
                        .total_cmp(&from.distance_squared(b.at))
                })
        {
            let away = (from - wolf.at).normalize_or_zero();
            let dodging = matches!(wolf.hunt, Hunt::Strike { .. });
            let flee = if dodging {
                Vec2::from_angle(0.52).rotate(away)
            } else {
                away
            };
            let at = land_step(
                terrain,
                from,
                flee * (if dodging { 4.1 } else { 3.65 } * dt),
            );
            return (at, at - from);
        }
        let delta = (patrol - from).clamp_length_max(1.8 * dt);
        // The prechecked orbit is safe even on the scenic side of the border.
        let at = if (patrol - from).length() < 1.8 * dt {
            patrol
        } else {
            land_step(terrain, from, delta)
        };
        (at, at - from)
    }

    fn hunt(
        &mut self,
        terrain: &Terrain,
        from: Vec2,
        dt: f32,
        animals: &[Animal],
    ) -> Option<(Vec2, Vec2)> {
        if let Hunt::Recover { until } = self.hunt
            && self.age >= until
        {
            self.hunt = Hunt::Patrol;
        }
        if matches!(self.hunt, Hunt::Patrol)
            && let Some(prey) = animals
                .iter()
                .filter(|animal| {
                    animal.kind == Kind::Hexadeer
                        && from.distance_squared(animal.at) < HUNT_RANGE * HUNT_RANGE
                        && from.distance_squared(self.route.center) < 10.0 * 10.0
                        && dry_ground(terrain, (animal.at + from) * 0.5)
                })
                .min_by(|a, b| {
                    from.distance_squared(a.at)
                        .total_cmp(&from.distance_squared(b.at))
                })
        {
            self.hunt = Hunt::Chase {
                prey: prey.entity,
                until: self.age + 8.5,
            };
        }
        let target = self.hunt.target()?;
        let prey = animals.iter().find(|animal| animal.entity == target);
        if let Some(prey) = prey {
            let facing = prey.at - from;
            match self.hunt {
                Hunt::Chase { until, .. }
                    if self.age < until && from.distance(self.route.center) < PURSUIT_LEASH =>
                {
                    if facing.length_squared() <= REACH * REACH {
                        self.hunt = Hunt::Strike {
                            prey: target,
                            since: self.age,
                        };
                    } else {
                        let at = land_step(terrain, from, facing.normalize_or_zero() * (6.4 * dt));
                        return Some((at, facing));
                    }
                }
                _ => {}
            }
            if let Hunt::Strike { since, .. } = self.hunt {
                let progress = (self.age - since) / STRIKE_TIME;
                if progress < 1.0 {
                    self.lunge = (progress * std::f32::consts::PI).sin();
                    let burst = (self.lunge * 3.6 * dt).min((facing.length() - 0.9).max(0.0));
                    let at = land_step(terrain, from, facing.normalize_or_zero() * burst);
                    return Some((at, facing));
                }
            }
        }
        self.hunt = Hunt::Recover {
            until: self.age + 34.0 + self.phase().rem_euclid(11.0),
        };
        None
    }
}

#[derive(Clone, Copy)]
pub(in crate::world) struct Animal {
    entity: Entity,
    at: Vec2,
    kind: Kind,
    hunt: Hunt,
}

/// The animals on their routes: not mutants, nor any a gun brought down.
type Animals<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static mut Transform, &'static mut Wildlife),
    (Without<Mutant>, Without<Downed>),
>;

pub(in crate::world) fn move_wildlife(
    time: Res<Time>,
    terrain: Res<Terrain>,
    mut animals: Animals,
    mut nearby: Local<Vec<Animal>>,
    mut sounds: MessageWriter<CreatureSound>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    nearby.clear();
    nearby.extend(animals.iter().filter_map(|(entity, at, motion)| {
        (matches!(motion.kind, Kind::Frostwolf | Kind::Hexadeer)
            && motion.route.center.abs().max_element() < HALF_SIZE - HUNT_RANGE)
            .then_some(Animal {
                entity,
                at: at.translation.xz(),
                kind: motion.kind,
                hunt: motion.hunt,
            })
    }));
    for (entity, mut pose, mut motion) in &mut animals {
        let before = MotionSound::before(pose.translation, &motion);
        let prior = motion.hunt;
        motion.step(entity, &mut pose, &terrain, dt, &nearby);
        let target = motion.hunt.target();
        let pursued = motion.kind == Kind::Hexadeer
            && nearby
                .iter()
                .any(|animal| animal.hunt.target() == Some(entity));
        before.emit(
            pose.translation,
            &motion,
            target.is_some() || pursued,
            &mut sounds,
        );
        let voice = pose.translation + Vec3::Y * 0.65;
        match (prior, motion.hunt) {
            (Hunt::Strike { .. }, _) => {}
            (_, Hunt::Strike { .. }) => {
                sounds.write(CreatureSound::new(motion.kind, Cue::Strike, voice));
            }
            (_, Hunt::Chase { .. }) if prior.target().is_none() => {
                sounds.write(CreatureSound::new(motion.kind, Cue::Growl, voice));
            }
            _ => {}
        }
        if prior.target().is_none()
            && let Some(prey) = nearby.iter().find(|animal| Some(animal.entity) == target)
        {
            let at = prey.at;
            let height = terrain.mesh_height_at(at);
            sounds.write(CreatureSound::new(
                prey.kind,
                Cue::Alarm,
                Vec3::new(at.x, height + 0.85, at.y),
            ));
        }
    }
}

pub(super) fn dry_ground(terrain: &Terrain, at: Vec2) -> bool {
    (at - Vec2::from(BASE)).abs().max_element() > 22.0
        && !crate::world::cordon::near_wall_or_checkpoint(at, 1.0)
        && terrain.water_depth_at(at.x, at.y) <= 0.02
        && terrain.normal_at(at.x, at.y).y > 0.87
}

fn land_step(terrain: &Terrain, from: Vec2, delta: Vec2) -> Vec2 {
    [0.0, 0.72, -0.72, 1.25, -1.25]
        .into_iter()
        .map(|turn| from + Vec2::from_angle(turn).rotate(delta))
        .find(|&at| dry_ground(terrain, at))
        .unwrap_or(from)
}

fn heading(facing: Vec2) -> Quat {
    Quat::from_rotation_y(facing.x.atan2(facing.y))
}

pub(super) fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::world::wildlife::{flight_routes, ground_routes};

    #[test]
    fn hangar_wolf_chases_lunges_and_recovers_while_deer_evades() {
        let terrain = Terrain::new();
        let ground = ground_routes(&terrain);
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(terrain.clone())
            .add_message::<CreatureSound>()
            .add_systems(Update, move_wildlife);
        let ids = [Kind::Frostwolf, Kind::Hexadeer].map(|kind| {
            let (_, route) = ground
                .iter()
                .copied()
                .find(|(k, route)| *k == kind && route.center.distance(Vec2::from(BASE)) < 63.0)
                .expect("nearby hangar patrol");
            let animal = Wildlife::new(kind, route, None);
            app.world_mut()
                .spawn((animal.initial_pose(&terrain), animal))
                .id()
        });
        let mut chased = false;
        let mut fled = false;
        let mut lunged = false;
        let mut recovered = false;
        let mut encounter_sounds = Vec::new();
        for _ in 0..980 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            app.update();
            let wolf = app.world().get::<Wildlife>(ids[0]).expect("wolf");
            let deer = app.world().get::<Wildlife>(ids[1]).expect("deer");
            let position = app
                .world()
                .get::<Transform>(ids[0])
                .expect("wolf position")
                .translation
                .xz();
            assert!(
                dry_ground(&terrain, position),
                "wolf left safe terrain at {position:?}"
            );
            chased |=
                matches!(wolf.hunt, Hunt::Chase { prey, .. } if prey == ids[1]) && wolf.run > 0.6;
            fled |= deer.run > 0.5;
            lunged |= wolf.lunge > 0.8;
            recovered |= lunged && matches!(wolf.hunt, Hunt::Recover { .. }) && wolf.lunge == 0.0;
            encounter_sounds.extend(
                app.world_mut()
                    .resource_mut::<Messages<CreatureSound>>()
                    .drain()
                    .filter(|sound| matches!(sound.cue, Cue::Alarm | Cue::Growl | Cue::Strike))
                    .map(|sound| (sound.kind, sound.cue)),
            );
            if recovered {
                break;
            }
        }
        assert!(
            chased && fled && lunged && recovered,
            "{chased} {fled} {lunged} {recovered}"
        );
        assert_eq!(
            encounter_sounds,
            [
                (Kind::Frostwolf, Cue::Growl),
                (Kind::Hexadeer, Cue::Alarm),
                (Kind::Frostwolf, Cue::Strike)
            ]
        );
    }

    #[test]
    fn raven_descends_puts_feet_on_ground_and_takes_off_again() {
        let terrain = Terrain::new();
        let (kind, route, flight) = flight_routes(&terrain)[0];
        assert!(flight.lands);
        let bird = Wildlife::new(kind, route, Some(flight));
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .insert_resource(terrain.clone())
            .add_message::<CreatureSound>()
            .add_systems(Update, move_wildlife);
        let id = app
            .world_mut()
            .spawn((bird.initial_pose(&terrain), bird))
            .id();
        let mut before = app.world().get::<Transform>(id).unwrap().translation.xz();
        let mut on_ground = 0.0;
        let (mut steps, mut calls, mut landings, mut takeoffs) = (0, 0, 0, 0);
        for _ in 0..720 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            app.update();
            let bird = app.world().get::<Wildlife>(id).unwrap();
            let air = bird.air;
            let transform = app.world().get::<Transform>(id).unwrap();
            let at = transform.translation.xz();
            let dy = transform.translation.y - terrain.mesh_height_at(at);
            if (22.1..30.9).contains(&bird.age) {
                assert!(dy < 0.014 && bird.air == 0.0 && bird.pace > 0.7);
                on_ground += before.distance(at);
            }
            for sound in app
                .world_mut()
                .resource_mut::<Messages<CreatureSound>>()
                .drain()
            {
                match sound.cue {
                    Cue::Step(_) => {
                        assert!(air <= 0.025 && before.distance(at) > 0.001);
                        steps += 1;
                    }
                    Cue::Call => calls += 1,
                    Cue::Land => landings += 1,
                    Cue::Wings => takeoffs += 1,
                    other => panic!("unexpected raven cue {other:?}"),
                }
            }
            before = at;
        }
        let bird = app.world().get::<Wildlife>(id).unwrap();
        let transform = app.world().get::<Transform>(id).unwrap();
        assert!(on_ground > 3.0 && bird.air > 0.99);
        assert!(steps > 4 && calls >= 2);
        assert_eq!((landings, takeoffs), (1, 1));
        assert!(
            (transform.rotation * Vec3::Z)
                .xz()
                .dot(route.sample(bird.route_time).1.normalize())
                > 0.95
        );
        assert!(
            transform.translation.y - terrain.mesh_height_at(transform.translation.xz()) > 10.0
        );
    }
}
