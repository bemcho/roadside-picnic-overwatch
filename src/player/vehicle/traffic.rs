//! Harmont's traffic: a street sweeper up and down New Harmont's plaza avenue,
//! two sedans and the fire engine round the town's ring street, and two more
//! sedans, the ambulance and a black panel van between the Institute and the
//! Borscht (`world::traffic_routes`). They keep to the right, turn round at a
//! route's dead ends, slow for corners and stop for the stalker, their car, a
//! patrol or another car in their way. Their wheels are separate nodes
//! (`tools/split_vehicle_wheels.py`) that roll and steer; the ambulance's and
//! the fire engine's roof beacons flash.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::assets;
use crate::world::{
    Collider, Describe, Pedestrian, Pillar, Place, Terrain, Unclimbable, traffic_routes,
};

use super::super::Player;
use super::carrier::Carrier;
use super::patrol::{CarWheel, Patrol, attach_wheels, in_the_way};
use super::{Vehicle, pose_wheel};

/// How far right of a route's middle line the cars keep, in meters.
const KEEP_RIGHT: f32 = 2.5;
/// Corners are rounded to this radius where the road allows.
const TURN_RADIUS: f32 = 9.0;
const CORNER_STEPS: usize = 8;
/// Cars slow to this speed for a bend sharper than `SHARP`.
const TURNING: f32 = 3.0;
const SHARP: f32 = 1.0 / 30.0;
const ACCELERATION: f32 = 2.0;
const MAX_STEER: f32 = 0.6;
/// How quickly the front wheels follow the road, per second.
const STEER_RATE: f32 = 4.0;

/// One kind of car: its model, and the model's measurements in model units
/// (front +Z, one unit long).
struct Model {
    path: &'static str,
    place: Place,
    /// Meters per model unit: the car's length.
    scale: f32,
    cruise: f32,
    wheel_radius: f32,
    /// The front and rear axles along the car, and half the track.
    axles: (f32, f32),
    track: f32,
    collider: &'static [Pillar],
    /// Its roof beacons: where each glows over its dome, and how big.
    beacons: &'static [(Vec3, f32)],
}

/// The beacons glow this bright, in nits: they read against daylight (EV 13).
const BEACON_GLOW: f32 = 30_000.0;
/// Flashes per second; each beacon is lit for half of its cycle.
const BEACON_RATE: f32 = 1.5;

/// Four pillars in a row down the car's middle.
const fn body(radius: f32, top: f32) -> [Pillar; 4] {
    let mut pillars = [Pillar {
        x: 0.0,
        z: -0.32,
        radius,
        bottom: 0.0,
        top,
    }; 4];
    let mut i = 1;
    while i < 4 {
        pillars[i].z = -0.32 + 0.64 * i as f32 / 3.0;
        i += 1;
    }
    pillars
}

const SWEEPER: Model = Model {
    path: assets::STREET_SWEEPER,
    place: Place::Sweeper,
    scale: 5.5,
    cruise: 3.5,
    wheel_radius: 0.080,
    axles: (0.113, -0.275),
    track: 0.134,
    collider: &body(0.24, 0.56),
    beacons: &[],
};

const SEDAN: Model = Model {
    path: assets::TRAFFIC_SEDAN,
    place: Place::Sedan,
    scale: 4.6,
    cruise: 8.0,
    wheel_radius: 0.079,
    axles: (0.327, -0.255),
    track: 0.164,
    collider: &body(0.2, 0.34),
    beacons: &[],
};

// The emergency vehicles come out of one stubby Tripo scan; the ambulance is
// as long as the UAZ-452 it draws on.
const AMBULANCE: Model = Model {
    path: assets::TRAFFIC_AMBULANCE,
    place: Place::Ambulance,
    scale: 4.4,
    cruise: 8.5,
    wheel_radius: 0.093,
    axles: (0.234, -0.273),
    track: 0.228,
    collider: &body(0.27, 0.6),
    beacons: &[
        (Vec3::new(-0.033, 0.566, 0.129), 0.04),
        (Vec3::new(0.166, 0.567, 0.118), 0.038),
    ],
};

const FIRE_TRUCK: Model = Model {
    path: assets::TRAFFIC_FIRE_TRUCK,
    place: Place::FireTruck,
    scale: 7.0,
    cruise: 7.0,
    wheel_radius: 0.085,
    axles: (0.305, -0.289),
    track: 0.176,
    collider: &body(0.2, 0.49),
    beacons: &[(Vec3::new(0.0, 0.428, 0.167), 0.026)],
};

const PANEL_VAN: Model = Model {
    path: assets::TRAFFIC_PANEL_VAN,
    place: Place::PanelVan,
    scale: 4.8,
    cruise: 7.5,
    wheel_radius: 0.075,
    axles: (0.320, -0.267),
    track: 0.121,
    collider: &body(0.17, 0.37),
    beacons: &[],
};

/// Each car: its model, the index of its route and how far into the route's
/// lap it starts, as a share of the lap.
const FLEET: [(&Model, usize, f32); 8] = [
    (&SWEEPER, 1, 0.0),
    (&SEDAN, 0, 0.0),
    (&SEDAN, 0, 0.5),
    (&FIRE_TRUCK, 0, 0.25),
    (&SEDAN, 2, 0.0),
    (&SEDAN, 2, 0.5),
    (&AMBULANCE, 2, 0.25),
    (&PANEL_VAN, 2, 0.75),
];

/// A closed lane on the right of a route, its corners rounded.
#[derive(Clone)]
struct Lane {
    points: Vec<Vec2>,
    /// The distance driven to each point, then the whole lap.
    reached: Vec<f32>,
}

impl Lane {
    /// The lane along `route`: round a closed route, or there and back.
    fn new(route: &[Vec2], closed: bool) -> Self {
        let mut lap = route.to_vec();
        if !closed {
            lap.extend(route[1..route.len() - 1].iter().rev());
        }
        let points = round_corners(&keep_right(&lap, KEEP_RIGHT, true), true);
        let mut reached = vec![0.0];
        for (i, point) in points.iter().enumerate() {
            let next = points[(i + 1) % points.len()];
            reached.push(reached[i] + point.distance(next));
        }
        Self { points, reached }
    }

    fn length(&self) -> f32 {
        self.reached[self.points.len()]
    }

    fn point(&self, along: f32) -> Vec2 {
        let along = along.rem_euclid(self.length());
        // A hair below zero, `rem_euclid` rounds up to the whole lap: that is
        // the end of the last leg.
        let i = self
            .reached
            .partition_point(|&reached| reached <= along)
            .clamp(1, self.points.len())
            - 1;
        let (a, b) = (self.points[i], self.points[(i + 1) % self.points.len()]);
        a.lerp(
            b,
            (along - self.reached[i]) / (self.reached[i + 1] - self.reached[i]),
        )
    }

    fn heading(&self, along: f32) -> Vec2 {
        (self.point(along + 1.5) - self.point(along - 1.5)).normalize_or(Vec2::Y)
    }

    /// How fast the lane turns at `along`, in radians per meter: positive
    /// to the left, the way a positive yaw (and steer) turns.
    fn bend(&self, along: f32) -> f32 {
        -self
            .heading(along - 1.0)
            .angle_to(self.heading(along + 1.0))
            / 2.0
    }
}

/// The polyline `lap` moved `by` meters to the right of its direction of
/// travel; `closed` joins its last point back to its first. At a dead end,
/// where it doubles back, the lane crosses the road.
pub(super) fn keep_right(lap: &[Vec2], by: f32, closed: bool) -> Vec<Vec2> {
    let n = lap.len();
    let mut lane = Vec::with_capacity(n + 2);
    for (i, &point) in lap.iter().enumerate() {
        let into = (point - lap[(i + n - 1) % n]).normalize();
        let away = (lap[(i + 1) % n] - point).normalize();
        let (right_in, right_out) = (into.perp(), away.perp());
        if !closed && (i == 0 || i == n - 1) {
            // An open end follows its one leg.
            lane.push(point + if i == 0 { right_out } else { right_in } * by);
        } else if into.dot(away) < -0.9 {
            lane.extend([point + right_in * by, point + right_out * by]);
        } else {
            // The miter: `by` from both the road in and the road out.
            let miter = (right_in + right_out) / (1.0 + right_in.dot(right_out));
            lane.push(point + miter * by);
        }
    }
    lane
}

/// Rounds each corner of the polyline `lane` with a curve; an open one keeps
/// its two ends.
pub(super) fn round_corners(lane: &[Vec2], closed: bool) -> Vec<Vec2> {
    let n = lane.len();
    let mut rounded = Vec::with_capacity(n * (CORNER_STEPS + 1));
    for (i, &corner) in lane.iter().enumerate() {
        if !closed && (i == 0 || i == n - 1) {
            rounded.push(corner);
            continue;
        }
        let into = corner - lane[(i + n - 1) % n];
        let away = lane[(i + 1) % n] - corner;
        let turn = into.angle_to(away).abs();
        if turn < 0.05 {
            rounded.push(corner);
            continue;
        }
        let cut = (TURN_RADIUS * (turn / 2.0).tan())
            .min(into.length() / 2.0)
            .min(away.length() / 2.0);
        let (start, end) = (
            corner - into.normalize() * cut,
            corner + away.normalize() * cut,
        );
        // A quadratic Bézier from the road in to the road out, pulled toward
        // the corner.
        rounded.extend((0..=CORNER_STEPS).map(|k| {
            let t = k as f32 / CORNER_STEPS as f32;
            start.lerp(corner, t).lerp(corner.lerp(end, t), t)
        }));
    }
    rounded
}

#[derive(Component)]
#[require(Unclimbable)]
pub(crate) struct Traffic {
    model: &'static Model,
    lane: Lane,
    /// Meters driven into the current lap.
    along: f32,
    speed: f32,
    steer: f32,
    roll: f32,
}

/// A roof beacon's glow, lit for half of each flash; `phase` staggers a pair.
#[derive(Component)]
pub(crate) struct Beacon {
    phase: f32,
}

pub(crate) fn spawn_traffic(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dome = meshes.add(Sphere::new(1.0));
    let glow = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.08, 0.04),
        emissive: LinearRgba::rgb(1.0, 0.08, 0.03) * BEACON_GLOW,
        ..default()
    });
    let routes = traffic_routes();
    for (model, route, start) in FLEET {
        let (points, closed) = &routes[route];
        let lane = Lane::new(points, *closed);
        let along = lane.length() * start;
        let (at, heading) = (lane.point(along), lane.heading(along));
        commands
            .spawn((
                Name::new("Harmont traffic"),
                Traffic {
                    model,
                    lane,
                    along,
                    speed: 0.0,
                    steer: 0.0,
                    roll: 0.0,
                },
                Collider(model.collider),
                Describe {
                    place: model.place,
                    radius: 0.5 * model.scale,
                    height: 0.5 * model.scale,
                },
                Transform::from_translation(at.extend(0.0).xzy())
                    .with_rotation(Quat::from_rotation_y(heading.x.atan2(heading.y)))
                    .with_scale(Vec3::splat(model.scale)),
                WorldAssetRoot(assets::load_vehicle_scene(&server, model.path)),
            ))
            .observe(attach_wheels)
            .with_children(|car| {
                for (i, &(at, radius)) in model.beacons.iter().enumerate() {
                    car.spawn((
                        Beacon {
                            phase: 0.5 * i as f32,
                        },
                        Mesh3d(dome.clone()),
                        MeshMaterial3d(glow.clone()),
                        Transform::from_translation(at).with_scale(Vec3::splat(radius)),
                        NotShadowCaster,
                    ));
                }
            });
    }
}

/// Flashes the roof beacons; a pair takes turns.
pub(crate) fn flash_beacons(time: Res<Time>, mut beacons: Query<(&Beacon, &mut Visibility)>) {
    let cycle = time.elapsed_secs() * BEACON_RATE;
    for (beacon, mut visibility) in &mut beacons {
        let lit = (cycle + beacon.phase).fract() < 0.5;
        visibility.set_if_neq(if lit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Where a car at `at` heading `heading` rests on the ground: its height and
/// its attitude, pitched and banked to its four tires.
fn settle(model: &Model, at: Vec2, heading: Vec2, height_at: impl Fn(Vec2) -> f32) -> (f32, Quat) {
    let axles = (model.axles.0 * model.scale, model.axles.1 * model.scale);
    settle_footprint(at, heading, axles, model.track * model.scale, height_at)
}

/// Where a body at `at` heading `heading` rests on four contact points, the
/// `front` and `rear` meters along it and `track` meters either side: its
/// height and its attitude.
pub(super) fn settle_footprint(
    at: Vec2,
    heading: Vec2,
    (front, rear): (f32, f32),
    track: f32,
    height_at: impl Fn(Vec2) -> f32,
) -> (f32, Quat) {
    let right = heading.perp();
    let [fr, fl, rr, rl] = [
        (front, track),
        (front, -track),
        (rear, track),
        (rear, -track),
    ]
    .map(|(along, side)| height_at(at + heading * along + right * side));
    let pitch = ((fr + fl) - (rr + rl)) / (2.0 * (front - rear));
    let bank = ((fr + rr) - (fl + rl)) / (4.0 * track);
    let forward = Vec3::new(heading.x, pitch, heading.y).normalize();
    let up = Vec3::new(right.x, bank, right.y).cross(forward).normalize();
    let rotation = Quat::from_mat3(&Mat3::from_cols(up.cross(forward), up, forward));
    ((fr + fl + rr + rl) / 4.0, rotation)
}

/// What traffic stops for besides other traffic: the stalker, their car, the
/// patrols, the Institute's carrier and the townsfolk.
type Others<'w, 's> = Query<
    'w,
    's,
    &'static Transform,
    (
        Or<(
            With<Player>,
            With<Vehicle>,
            With<Patrol>,
            With<Carrier>,
            With<Pedestrian>,
        )>,
        Without<Traffic>,
    ),
>;

type Wheels<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static CarWheel, &'static mut Transform),
    (
        Without<Traffic>,
        Without<Player>,
        Without<Vehicle>,
        Without<Patrol>,
        Without<Carrier>,
        Without<Pedestrian>,
    ),
>;

/// Whether a car at `at` heading `heading` waits for another car: when the
/// two stand in each other's way, the first entity drives on.
fn yields(me: (Entity, Vec2, Vec2), other: (Entity, Vec2, Vec2)) -> bool {
    let (entity, at, heading) = me;
    let (other, other_at, other_heading) = other;
    other != entity
        && in_the_way(at, heading, other_at)
        && !(entity < other && in_the_way(other_at, other_heading, at))
}

/// Drives each car along its lane over the rendered ground, then poses
/// their wheels.
pub(crate) fn drive_traffic(
    time: Res<Time>,
    terrain: Res<Terrain>,
    others: Others,
    mut cars: Query<(Entity, &mut Traffic, &mut Transform), Without<CarWheel>>,
    mut wheels: Wheels,
    parents: Query<&ChildOf>,
) {
    let dt = time.delta_secs();
    let fleet: Vec<(Entity, Vec2, Vec2)> = cars
        .iter()
        .map(|(entity, car, transform)| {
            (
                entity,
                transform.translation.xz(),
                car.lane.heading(car.along),
            )
        })
        .collect();
    for (entity, mut car, mut transform) in &mut cars {
        let me = (
            entity,
            transform.translation.xz(),
            car.lane.heading(car.along),
        );
        let blocked = others
            .iter()
            .any(|other| in_the_way(me.1, me.2, other.translation.xz()))
            || fleet.iter().any(|&other| yields(me, other));
        let sharp = (0..5).any(|k| car.lane.bend(car.along + 3.0 * k as f32).abs() > SHARP);
        let target = match (blocked, sharp) {
            (true, _) => 0.0,
            (false, true) => TURNING.min(car.model.cruise),
            (false, false) => car.model.cruise,
        };
        let change = (target - car.speed).clamp(-ACCELERATION * dt, ACCELERATION * dt);
        car.speed += change;
        let step = car.speed * dt;
        let length = car.lane.length();
        car.along = (car.along + step).rem_euclid(length);
        car.roll += step / (car.model.wheel_radius * car.model.scale);
        let wheelbase = (car.model.axles.0 - car.model.axles.1) * car.model.scale;
        let lean = (wheelbase * car.lane.bend(car.along)).atan();
        let lean = lean.clamp(-MAX_STEER, MAX_STEER);
        car.steer += (lean - car.steer) * (1.0 - (-STEER_RATE * dt).exp());

        let (at, heading) = (car.lane.point(car.along), car.lane.heading(car.along));
        let (height, rotation) = settle(car.model, at, heading, |p| terrain.mesh_height_at(p));
        transform.translation = at.extend(height).xzy();
        transform.rotation = rotation;
    }
    for (entity, wheel, mut transform) in &mut wheels {
        let car = parents
            .iter_ancestors(entity)
            .find_map(|ancestor| cars.get(ancestor).ok());
        if let Some((_, car, _)) = car {
            let gear = (car.steer, car.roll, [Vec3::ZERO; 4]);
            pose_wheel(&mut transform, wheel.slot, wheel.rest, gear);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walks the lane in small steps: each step is about as long as asked,
    /// and the lane keeps `KEEP_RIGHT` from the route.
    fn assert_smooth(lane: &Lane) {
        let steps = (lane.length() / 0.5) as usize;
        let ds = lane.length() / steps as f32;
        for i in 0..steps {
            let (a, b) = (lane.point(i as f32 * ds), lane.point((i + 1) as f32 * ds));
            assert!(a.distance(b) <= ds + 1e-3, "jump at {a}");
            assert!(a.distance(b) > 0.5 * ds, "stall at {a}");
        }
    }

    #[test]
    fn a_closed_route_keeps_right_round_its_corners() {
        // Heading +X the right hand points to +Z: round a square turning right.
        let square = [
            Vec2::ZERO,
            Vec2::new(100.0, 0.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(0.0, 100.0),
        ];
        let lane = Lane::new(&square, true);
        assert_smooth(&lane);
        // The lane runs inside the square, a little shorter, its corners cut.
        let at = lane.point(40.0);
        assert!((at.y - KEEP_RIGHT).abs() < 1e-3, "{at}");
        assert!(lane.bend(40.0).abs() < 1e-3, "straight");
        assert!(
            lane.bend(lane.length() / 4.0 + 2.0) < -SHARP,
            "a right turn"
        );
        let edge = 100.0 - 2.0 * KEEP_RIGHT;
        assert!(lane.length() < 4.0 * edge && lane.length() > 4.0 * edge - 16.0);
    }

    #[test]
    fn a_hair_before_the_lap_start_is_the_lap_start() {
        let road = [Vec2::ZERO, Vec2::new(0.0, 200.0)];
        let lane = Lane::new(&road, false);
        // `rem_euclid` rounds this up to the whole lap, one past the last point.
        let along: f32 = -1e-6;
        assert_eq!(along.rem_euclid(lane.length()), lane.length());
        assert!(lane.point(along).distance(lane.point(0.0)) < 1e-3);
    }

    #[test]
    fn an_open_route_comes_back_on_the_other_side() {
        let road = [Vec2::ZERO, Vec2::new(0.0, 200.0)];
        let lane = Lane::new(&road, false);
        assert_smooth(&lane);
        let out = lane.point(100.0);
        let back = lane.point(lane.length() / 2.0 + 100.0);
        assert!((out.x + KEEP_RIGHT).abs() < 1e-3, "out on the right: {out}");
        assert!(
            (back.x - KEEP_RIGHT).abs() < 1e-3,
            "back on the right: {back}"
        );
        assert!(
            lane.heading(100.0)
                .dot(lane.heading(lane.length() / 2.0 + 100.0))
                < -0.99
        );
    }

    #[test]
    fn harmont_routes_stay_on_their_roads() {
        for (route, closed) in traffic_routes() {
            let lane = Lane::new(&route, closed);
            assert_smooth(&lane);
            let mut road = route.clone();
            if closed {
                road.push(route[0]);
            }
            let off_road = |at: Vec2| {
                road.array_windows()
                    .map(|&[a, b]| {
                        let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                        at.distance(a.lerp(b, t))
                    })
                    .fold(f32::INFINITY, f32::min)
            };
            let steps = (lane.length() / 5.0) as usize;
            for i in 0..steps {
                let at = lane.point(i as f32 * 5.0);
                // Roads are paved 8.5 m either side of their middle line;
                // corners are cut, but a car stays clear of the edge.
                assert!(off_road(at) < 6.0, "{at} strays off the road");
            }
        }
    }

    #[test]
    fn cars_sharing_a_route_start_well_apart() {
        let routes = traffic_routes();
        for (i, &(model, route, start)) in FLEET.iter().enumerate() {
            for &(other, other_route, other_start) in &FLEET[i + 1..] {
                if route != other_route {
                    continue;
                }
                let (points, closed) = &routes[route];
                let lap = Lane::new(points, *closed).length();
                let apart = (start - other_start).rem_euclid(1.0);
                let gap = lap * apart.min(1.0 - apart);
                // Bumper to bumper, beyond the distance a car stops at.
                let clear = gap - 0.5 * (model.scale + other.scale);
                assert!(clear > 40.0, "route {route}: only {clear} m apart");
            }
        }
    }

    #[test]
    fn the_emergency_vehicles_share_one_loaded_atlas() {
        use bevy::asset::AssetPath;
        use bevy::gltf::gltf::{Gltf, image::Source};
        use bevy::image::ImageLoaderSettings;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>();
        let server = app.world().resource::<AssetServer>().clone();
        let atlas = server.load::<Image>(assets::TRAFFIC_EMERGENCY_ATLAS);
        assert!(
            std::path::Path::new("assets")
                .join(assets::TRAFFIC_EMERGENCY_ATLAS)
                .is_file()
        );
        for model in [&AMBULANCE, &FIRE_TRUCK, &PANEL_VAN] {
            let bytes = std::fs::read(std::path::Path::new("assets").join(model.path))
                .expect("the model is in assets/");
            let gltf = Gltf::from_slice_without_validation(&bytes).expect("a GLB");
            for image in gltf.images() {
                let Source::Uri { uri, .. } = image.source() else {
                    panic!("{}: an embedded image", model.path);
                };
                // As the glTF loader resolves and loads it, with its own settings.
                let path = AssetPath::parse(model.path)
                    .resolve_embed_str(uri)
                    .expect("a relative URI");
                assert_eq!(path, AssetPath::parse(assets::TRAFFIC_EMERGENCY_ATLAS));
                let loaded = server
                    .load_builder()
                    .with_settings(|settings: &mut ImageLoaderSettings| settings.is_srgb = true)
                    .load::<Image>(path);
                assert_eq!(loaded.id(), atlas.id(), "{}", model.path);
            }
        }
    }

    #[test]
    fn beacons_sit_on_their_roofs() {
        for (model, _, _) in FLEET {
            for &(at, radius) in model.beacons {
                assert!(at.x.abs() < 0.5 && at.z.abs() < 0.5, "{at}");
                let roof = model.collider[0].top;
                assert!(at.y < roof && at.y + radius > roof - 0.05, "{at}");
            }
        }
    }

    #[test]
    fn a_car_settles_on_a_slope_pitched_up_it() {
        let slope = |p: Vec2| 0.1 * p.y;
        let (height, rotation) = settle(&SEDAN, Vec2::new(5.0, 20.0), Vec2::Y, slope);
        assert!((height - 2.0).abs() < 0.05, "{height}");
        let forward = rotation * Vec3::Z;
        assert!((forward.y / forward.xz().length() - 0.1).abs() < 1e-3);
        assert!((rotation * Vec3::Y).y > 0.99);
    }

    #[test]
    fn of_two_cars_in_each_others_way_one_drives_on() {
        let mut world = World::new();
        let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
        let first = (a, Vec2::ZERO, Vec2::Y);
        let second = (b, Vec2::new(0.0, 5.0), Vec2::NEG_Y);
        assert!(yields(second, first) != yields(first, second));
        // A car behind another waits; the one ahead drives on.
        let behind = (b, Vec2::new(0.0, -5.0), Vec2::Y);
        assert!(yields(behind, first) && !yields(first, behind));
    }
}
