//! Point-and-click movement: left-click the ground to send the stalker there,
//! Shift + left click to add a waypoint after the orders already given. Each
//! waypoint shows a pulsing ring on the ground until it is reached. A route
//! started with Shift waits until Shift is released. WASD still steers
//! directly and cancels the order.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::states::AppState;
use crate::world::{Body, Boom, Collider, InstitutePass, Terrain, Weight, in_zone};

use super::path::{Mobility, Reach, find_path};
use super::vehicle::{CAR, CARRIER_ROUTING, Carrier, Driving, FORD_DEPTH, MAX_GRIP_GRADE};
use super::{Player, PlayerCamera, STALKER, STANDING};

/// Orders farther than this make the stalker run instead of walk.
const RUN_DISTANCE: f32 = 9.0;
/// How close the stalker and the vehicle get to their targets before stopping.
pub(super) const STALKER_ARRIVAL: f32 = 0.35;
pub(super) const VEHICLE_ARRIVAL: f32 = 3.0;
/// Intermediate waypoints count as reached this many arrival radii away.
const WAYPOINT_SLACK: f32 = 1.7;
/// Water the stalker can still wade through (matches `move_player`).
const WADE_DEPTH: f32 = 1.05;
/// The car's footprint for pathfinding.
const VEHICLE_BODY: Body = Body {
    radius: 1.7,
    height: 2.4,
};
/// How click orders route the stalker: through water they wade, on ground no
/// steeper than 45°. `move_player` lets them walk up anything; a steeper wall
/// is a scramble, taken only when no gentler way joins the trip (see `route`).
const ON_FOOT: Mobility = Mobility {
    body: STANDING,
    max_depth: WADE_DEPTH,
    max_grade: 1.0,
};
/// How click orders route the car: through water it fords, on ground no
/// steeper than its tires grip (it slides down anything steeper).
const BY_CAR: Mobility = Mobility {
    body: VEHICLE_BODY,
    max_depth: FORD_DEPTH,
    max_grade: MAX_GRIP_GRADE,
};
/// An order that makes this little of its intended progress is blocked.
const STALL_RATIO: f32 = 0.15;
/// How long the stalker may stay blocked before giving up the order.
const STALL_SECONDS: f32 = 0.8;
/// Farthest point on the ground a click can reach, in meters.
const CLICK_REACH: f32 = 12_000.0;
/// The colour of the stop rings and the route line.
pub(super) const ORDER_COLOR: Color = Color::srgb(0.95, 0.78, 0.35);
/// How far a blocked mover backs away before taking the new leg, in meters: a
/// car pressed nose-first against a rock cannot turn onto it in place, and
/// needs more than its own length of room to swing round.
const BACKOFF_DISTANCE: f32 = 6.0;
/// Back-off travel an order may spend getting unstuck, in meters: three full
/// back-offs. A car's heading turns with the distance it travels (about 0.19
/// rad a meter at full lock: tan(0.48) / 2.8 m wheelbase), so where less room
/// is left behind it, each back-and-turn swings it less and it may take
/// proportionally more of them: 18 with 1 m behind it.
const BACKOFF_BUDGET: f32 = 3.0 * BACKOFF_DISTANCE;
/// The least a back-off is charged, in meters: 0.3 m still swings a car about
/// 3 degrees, and caps a boxed-in car at 60 attempts.
const MIN_BACKOFF_CHARGE: f32 = 0.3;
/// The longest a back-off lasts, in seconds, should it neither get that far
/// nor stall against something behind.
const BACKOFF_SECONDS: f32 = 4.0;
/// One pulse of the waypoint rings, in seconds, and how far they swell.
const RING_PULSE_PERIOD: f32 = 1.4;
const RING_PULSE_SWELL: f32 = 0.25;

/// How a blocked mover turns onto a new leg.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Turning {
    /// On the spot, as the stalker does: the room behind does not matter, and
    /// every attempt costs a full back-off.
    InPlace,
    /// Backing and filling, as a car does: an attempt costs only the room it
    /// found behind, so a tight spot buys more of them.
    Reversing,
}

/// Where the stalker was told to go: the remaining path, target last, and
/// the clicked stops along it still ahead.
#[derive(Component, Default)]
pub struct MoveOrder {
    waypoints: Vec<Vec2>,
    stops: Vec<Vec2>,
    run: bool,
    arrival: f32,
    stalled: f32,
    /// Waiting for Shift to be released before setting off.
    held: bool,
    /// The leg to the next stop should be pathfound again from where the
    /// mover stands: scenery colliders stream in only near the stalker, so a
    /// far leg was planned without them.
    replan: bool,
    /// Back-off travel spent so far, in meters, out of [`BACKOFF_BUDGET`].
    spent: f32,
    /// Seconds left backing away from the obstacle that blocked the mover.
    backoff: f32,
    /// Whether that back-off reverses the mover, as a car's does; the stalker
    /// walks on toward the new leg instead.
    reversing: bool,
    /// Where the mover stalled, to measure how far it has backed away.
    stalled_at: Vec2,
}

impl MoveOrder {
    pub(super) fn give(&mut self, waypoints: Vec<Vec2>, from: Vec2, run: bool, arrival: f32) {
        *self = Self {
            run: run || path_length(from, &waypoints) > RUN_DISTANCE,
            stops: waypoints.last().copied().into_iter().collect(),
            waypoints,
            arrival,
            ..default()
        };
    }

    /// Holds the stalker in place until [`MoveOrder::release`].
    pub(super) fn hold(&mut self) {
        self.held = !self.waypoints.is_empty();
    }

    pub(super) fn release(&mut self) {
        self.held = false;
    }

    /// The next stop, when its leg must be pathfound again.
    fn wants_replan(&self) -> Option<Vec2> {
        self.stops.first().copied().filter(|_| self.replan)
    }

    /// Swaps the path to the next stop for `leg`, which ends at that stop (or
    /// the nearest free spot to it).
    fn replace_leg(&mut self, leg: Vec<Vec2>) {
        self.replan = false;
        let (Some(&stop), Some(&end)) = (self.stops.first(), leg.last()) else {
            return;
        };
        let Some(reach) = self.waypoints.iter().position(|&point| point == stop) else {
            return;
        };
        self.waypoints.splice(..=reach, leg);
        self.stops[0] = end;
    }

    /// Adds a leg after the current target; the whole trip decides whether
    /// the stalker runs.
    pub(super) fn append(&mut self, leg: Vec<Vec2>, from: Vec2) {
        self.stops.extend(leg.last().copied());
        self.waypoints.extend(leg);
        self.run = self.run || path_length(from, &self.waypoints) > RUN_DISTANCE;
    }

    /// Distance still to travel along the remaining waypoints.
    pub(super) fn remaining(&self, at: Vec2) -> f32 {
        path_length(at, &self.waypoints)
    }

    /// The path still ahead, target last.
    pub(super) fn waypoints(&self) -> &[Vec2] {
        &self.waypoints
    }

    fn target(&self) -> Option<Vec2> {
        self.waypoints.last().copied()
    }

    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }

    /// Ground direction toward the next waypoint, or `None` once arrived, idle
    /// or held.
    pub(super) fn steer(&mut self, at: Vec2) -> Option<Vec2> {
        if self.held {
            return None;
        }
        while let Some(&next) = self.waypoints.first() {
            let reach = if self.waypoints.len() == 1 {
                self.arrival
            } else {
                self.arrival * WAYPOINT_SLACK
            };
            // Reversing away from what blocked it, the mover passes no
            // waypoint: the leg planned there may turn off right beside it.
            let reversing = self.backing_off() && self.reversing;
            if reversing || at.distance(next) > reach {
                return Some((next - at).normalize_or_zero());
            }
            self.waypoints.remove(0);
            if self.stops.first() == Some(&next) {
                self.stops.remove(0);
                // The next leg was planned from afar: plan it again from here.
                self.replan = !self.stops.is_empty();
            }
        }
        self.cancel();
        None
    }

    pub(super) fn runs(&self) -> bool {
        !self.waypoints.is_empty() && self.run
    }

    /// Whether the mover is backing away from an obstacle before the new leg.
    pub(super) fn backing_off(&self) -> bool {
        self.backoff > 0.0
    }

    /// Pathfinds the leg again (after backing away) when the mover at `at`
    /// keeps pushing against an obstacle, and drops the order once the
    /// attempts have spent [`BACKOFF_BUDGET`].
    pub(super) fn track_progress(
        &mut self,
        at: Vec2,
        moved: f32,
        intended: f32,
        dt: f32,
        turning: Turning,
    ) {
        if self.waypoints.is_empty() || self.held || intended <= 0.0 {
            return;
        }
        self.stalled = if moved < intended * STALL_RATIO {
            self.stalled + dt
        } else {
            0.0
        };
        if self.backing_off() {
            self.backoff -= dt;
            let backed = at.distance(self.stalled_at);
            // Far enough back, backed into something too, or out of time.
            if backed >= BACKOFF_DISTANCE || self.stalled > STALL_SECONDS || self.backoff <= 0.0 {
                self.backoff = 0.0;
                self.stalled = 0.0;
                self.spent += match turning {
                    Turning::InPlace => BACKOFF_DISTANCE,
                    Turning::Reversing => backed.clamp(MIN_BACKOFF_CHARGE, BACKOFF_DISTANCE),
                };
            }
            return;
        }
        if self.stalled > STALL_SECONDS {
            // Half a charge of slack absorbs the rounding of many small ones.
            if self.spent < BACKOFF_BUDGET - MIN_BACKOFF_CHARGE / 2.0 {
                self.replan = true;
                self.stalled = 0.0;
                self.stalled_at = at;
                self.backoff = BACKOFF_SECONDS;
                self.reversing = turning == Turning::Reversing;
            } else {
                self.cancel();
            }
        }
    }
}

fn path_length(from: Vec2, waypoints: &[Vec2]) -> f32 {
    waypoints
        .iter()
        .fold((from, 0.0), |(at, sum), &next| {
            (next, sum + at.distance(next))
        })
        .1
}

/// A ground ring showing one of the order's stops.
#[derive(Component)]
pub(super) struct OrderMarker;

/// The rings' shared mesh and glow.
#[derive(Resource)]
pub(super) struct OrderMarkerLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

pub(super) fn prepare_order_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(OrderMarkerLook {
        mesh: meshes.add(Annulus::new(0.32, 0.45)),
        material: materials.add(StandardMaterial {
            base_color: ORDER_COLOR,
            emissive: LinearRgba::rgb(900.0, 620.0, 180.0),
            unlit: true,
            ..default()
        }),
    });
}

/// What a route may be blocked by, and whether it is a gate's boom or carries
/// a pass through the Institute gate.
type Solids<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Collider,
        &'static GlobalTransform,
        Option<&'static Weight>,
        Option<&'static Boom>,
        Has<InstitutePass>,
    ),
>;

/// Turns a left click on the ground into a pathfound move order for the stalker,
/// or for the vehicle while the stalker drives it. A Shift-click on a mover
/// standing still starts a held route that sets off once Shift is released.
pub(super) fn issue_move_order(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<Terrain>,
    window: Single<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    (colliders, carriers): (Solids, Query<(), With<Carrier>>),
    mut players: Query<(&Transform, &mut MoveOrder, Option<&Driving>), With<Player>>,
) {
    let Ok((stalker, mut order, driving)) = players.single_mut() else {
        return;
    };
    // Shift queues the new leg after the current target.
    let queue = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if !queue {
        order.release();
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (Some(cursor), Ok((camera, eye))) = (window.cursor_position(), cameras.single()) else {
        return;
    };
    let Some(target) = camera
        .viewport_to_world(eye, cursor)
        .ok()
        .and_then(|ray| cast_to_ground(&terrain, ray))
    else {
        return;
    };
    let from = stalker.translation.xz();
    let start = match order.target() {
        Some(last) if queue => last,
        _ => from,
    };
    let vehicle = driving.map(|driving| driving.0);
    let Some(path) = route(
        &terrain,
        (&colliders, &carriers),
        vehicle,
        start,
        target.xz(),
    ) else {
        return;
    };
    if start == from {
        let arrival = if vehicle.is_some() {
            VEHICLE_ARRIVAL
        } else {
            STALKER_ARRIVAL
        };
        order.give(path, from, false, arrival);
        if queue {
            order.hold();
        }
    } else {
        order.append(path, from);
    }
}

/// Pathfinds the leg to the next stop again from where the mover stands, now
/// that the scenery colliders around it have streamed in.
pub(super) fn replan_order(
    terrain: Res<Terrain>,
    (colliders, carriers): (Solids, Query<(), With<Carrier>>),
    mut players: Query<(&Transform, &mut MoveOrder, Option<&Driving>), With<Player>>,
) {
    let Ok((stalker, mut order, driving)) = players.single_mut() else {
        return;
    };
    let Some(stop) = order.wants_replan() else {
        return;
    };
    let vehicle = driving.map(|driving| driving.0);
    match route(
        &terrain,
        (&colliders, &carriers),
        vehicle,
        stalker.translation.xz(),
        stop,
    ) {
        Some(leg) => order.replace_leg(leg),
        None => order.cancel(),
    }
}

/// A path for the stalker, or for the vehicle they drive (the car, or the
/// wider Institute carrier), around everything that blocks it. Where no route
/// keeps to ground the stalker is routed on, they get one over any slope, as
/// `move_player` always let them walk; a vehicle drives as near as it can get
/// and stops there.
fn route(
    terrain: &Terrain,
    (colliders, carriers): (&Solids, &Query<(), With<Carrier>>),
    vehicle: Option<Entity>,
    from: Vec2,
    to: Vec2,
) -> Option<Vec<Vec2>> {
    let (mobility, mover) = match vehicle {
        Some(carrier) if carriers.contains(carrier) => CARRIER_ROUTING,
        Some(_) => (BY_CAR, CAR),
        None => (ON_FOOT, STALKER),
    };
    // The driven vehicle must not block its own route, nor anything lighter,
    // nor a gate's boom that lifts for it (the yard's always does; the
    // Institute's for its vehicles and for the stalker walking out of the Zone).
    let pass = match vehicle {
        Some(car) => colliders.get(car).is_ok_and(|(.., pass)| pass),
        None => in_zone(from),
    };
    let obstacles: Vec<_> = colliders
        .iter()
        .filter(|&(entity, _, _, weight, boom, _)| {
            Some(entity) != vehicle
                && mover.blocked_by(weight)
                && !boom.is_some_and(|boom| boom.opens_for(pass))
        })
        .map(|(_, collider, pose, ..)| (collider, pose))
        .collect();
    let find = |mobility, reach| {
        find_path(
            terrain,
            obstacles.iter().copied(),
            from,
            to,
            mobility,
            reach,
        )
    };
    match vehicle {
        None => find(mobility, Reach::Goal).or_else(|| {
            let any_slope = Mobility {
                max_grade: f32::INFINITY,
                ..mobility
            };
            find(any_slope, Reach::Goal)
        }),
        Some(_) => find(mobility, Reach::Nearest),
    }
}

/// Shows a pulsing ring at each stop still ahead, spawning more as orders queue up;
/// boarding or leaving a vehicle drops the order.
pub(super) fn sync_order_markers(
    mut commands: Commands,
    time: Res<Time>,
    terrain: Res<Terrain>,
    look: Res<OrderMarkerLook>,
    mut was_driving: Local<bool>,
    mut players: Query<(&mut MoveOrder, Has<Driving>), With<Player>>,
    mut markers: Query<(&mut Transform, &mut Visibility), With<OrderMarker>>,
) {
    let Ok((mut order, driving)) = players.single_mut() else {
        return;
    };
    if driving != *was_driving {
        *was_driving = driving;
        order.cancel();
    }
    let beat = 0.5 - 0.5 * (time.elapsed_secs() * TAU / RING_PULSE_PERIOD).cos();
    let place = |at: Vec2| {
        Transform::from_translation(at.extend(terrain.mesh_height_at(at) + 0.06).xzy())
            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
            .with_scale(Vec3::splat(1.0 + RING_PULSE_SWELL * beat))
    };
    let mut stops = order.stops.iter().copied();
    for (mut marker, mut visibility) in &mut markers {
        let shown = match stops.next() {
            Some(at) => {
                *marker = place(at);
                Visibility::Inherited
            }
            None => Visibility::Hidden,
        };
        visibility.set_if_neq(shown);
    }
    for at in stops {
        commands.spawn((
            OrderMarker,
            Mesh3d(look.mesh.clone()),
            MeshMaterial3d(look.material.clone()),
            place(at),
            DespawnOnExit(AppState::InGame),
        ));
    }
}

/// Where a view ray first meets the rendered terrain.
pub(crate) fn cast_to_ground(terrain: &Terrain, ray: Ray3d) -> Option<Vec3> {
    let below = |t: f32| {
        let point = ray.get_point(t);
        point.y <= terrain.mesh_height_at(point.xz())
    };
    let (mut near, mut far) = (0.0, 0.0);
    while far < CLICK_REACH {
        let point = ray.get_point(far);
        let clearance = point.y - terrain.mesh_height_at(point.xz());
        if clearance <= 0.0 {
            break;
        }
        near = far;
        far += (clearance * 0.5).clamp(0.25, 64.0);
    }
    if !below(far) {
        return None;
    }
    for _ in 0..24 {
        let middle = (near + far) * 0.5;
        if below(middle) {
            far = middle;
        } else {
            near = middle;
        }
    }
    Some(ray.get_point(far))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{CutScan, crater_ramp, cut_scan, push_out, quarry_ramp};

    #[test]
    fn the_carrier_is_routed_around_a_gap_the_car_takes() {
        use crate::world::Pillar;
        use bevy::ecs::system::SystemState;
        const POST: [Pillar; 1] = [Pillar {
            x: 0.0,
            z: 0.0,
            radius: 0.5,
            bottom: -1.0,
            top: 3.0,
        }];
        let terrain = Terrain::new();
        let mut world = World::new();
        // A wall of posts across the yard, 1 m apart, with a 4 m gap in it.
        let gap = Vec2::new(843.0, -70.0);
        for step in -15_i32..=15 {
            if step.abs() > 2 {
                let at = gap + Vec2::Y * step as f32;
                let y = terrain.height_at(at.x, at.y);
                world.spawn((Collider(&POST), GlobalTransform::from_xyz(at.x, y, at.y)));
            }
        }
        let car = world.spawn_empty().id();
        let carrier = world.spawn(Carrier::new()).id();
        let mut state = SystemState::<(Solids, Query<(), With<Carrier>>)>::new(&mut world);
        let (colliders, carriers) = state.get(&world).expect("valid queries");
        let (from, to) = (gap - Vec2::X * 18.0, gap + Vec2::X * 18.0);
        let widest = |vehicle| {
            let path = route(&terrain, (&colliders, &carriers), Some(vehicle), from, to)
                .expect("a way across");
            path.iter().map(|p| (p.y - gap.y).abs()).fold(0.0, f32::max)
        };
        assert!(widest(car) < 2.0, "the car takes the gap");
        assert!(widest(carrier) > 15.0, "the carrier goes round the wall");
    }

    /// Where a mover of `body` walking `path` from `from` first runs into
    /// `solids` at the ground it walks on, pushed back out of them.
    fn first_contact<'a>(
        terrain: &Terrain,
        solids: &[(&'a Collider, &'a GlobalTransform)],
        body: Body,
        from: Vec2,
        path: &[Vec2],
    ) -> Option<Vec2> {
        let mut previous = from;
        for &point in path {
            let steps = (previous.distance(point) / 0.25).ceil().max(1.0);
            for i in 0..=steps as usize {
                let at = previous.lerp(point, i as f32 / steps);
                let clear = push_out(at, terrain.mesh_height_at(at), body, solids.iter().copied());
                if clear.distance(at) > 0.05 {
                    return Some(clear);
                }
            }
            previous = point;
        }
        None
    }

    /// The steepest grade on the rendered ground under a mover of `body`
    /// walking `path` from `from`, in 1 m steps along its middle and its
    /// sides, and where.
    fn steepest_grade(terrain: &Terrain, body: Body, from: Vec2, path: &[Vec2]) -> (f32, Vec2) {
        let mut steepest = (0.0, from);
        let mut previous = from;
        for &point in path {
            let steps = previous.distance(point).ceil().max(1.0);
            let step = previous.distance(point) / steps;
            let across = (point - previous).normalize_or_zero().perp() * body.radius;
            for side in [-1.0, 0.0, 1.0] {
                let at = |i: f32| previous.lerp(point, i / steps) + across * side;
                for i in 1..=steps as usize {
                    let (a, b) = (at(i as f32 - 1.0), at(i as f32));
                    let grade =
                        (terrain.mesh_height_at(b) - terrain.mesh_height_at(a)).abs() / step;
                    if grade > steepest.0 {
                        steepest = (grade, b);
                    }
                }
            }
            previous = point;
        }
        steepest
    }

    #[test]
    // Click orders for the stalker, the car and the carrier, through the
    // game's own `route`, to the crater's floor: with both cut scans streamed
    // in, from both junctions and from the road in from the hangar; and as the
    // game gives them at the Quarry rim's junction, where the crater's scan
    // has not streamed in yet (should the order run into its stone, it is
    // planned again from there, as `replan_order` does). Each route keeps
    // clear of the scans' stone at the height each leg is walked on, and to
    // ground no steeper than its mover climbs: down the crater's spiral road,
    // not its walls (up to a 1.64 grade).
    fn click_orders_reach_the_crater_floor() {
        use bevy::ecs::system::SystemState;
        let terrain = Terrain::new();
        let world_of = |scans: &[CutScan]| {
            let mut world = World::new();
            let car = world.spawn_empty().id();
            let carrier = world.spawn(Carrier::new()).id();
            for &scan in scans {
                let (pose, _) = cut_scan(scan);
                world.spawn((Collider(scan.pillars()), GlobalTransform::from(pose)));
            }
            (world, car, carrier)
        };
        let (mut streamed, car, carrier) = world_of(&CutScan::ALL);
        let (mut unstreamed, ..) = world_of(&[CutScan::Quarry]);
        let mut both = SystemState::<(Solids, Query<(), With<Carrier>>)>::new(&mut streamed);
        let mut quarry = SystemState::<(Solids, Query<(), With<Carrier>>)>::new(&mut unstreamed);
        let (solids, carriers) = both.get(&streamed).expect("valid queries");
        let early = quarry.get(&unstreamed).expect("valid queries");
        let stone: Vec<_> = solids.iter().map(|(_, c, pose, ..)| (c, pose)).collect();
        let crater = crater_ramp();
        let floor = crater.last().copied().expect("a road");
        let junction = quarry_ramp()[0];
        let keeps_to_its_grade = |mover: &str, from: Vec2, path: &[Vec2], mobility: Mobility| {
            let (grade, at) = steepest_grade(&terrain, mobility.body, from, path);
            let limit = mobility.max_grade;
            assert!(
                grade <= limit,
                "the {mover}'s route from {from} takes a {grade} grade at {at}, over its {limit}"
            );
        };
        let reaches_the_floor = |mover: &str, from: Vec2, vehicle, mobility: Mobility, stalled| {
            let path = route(&terrain, (&solids, &carriers), vehicle, from, floor)
                .unwrap_or_else(|| panic!("no route for the {mover} from {from}"));
            assert_eq!(path.last(), Some(&floor), "the {mover} from {from}");
            let contact = first_contact(&terrain, &stone, mobility.body, from, &path);
            assert!(
                contact.is_none(),
                "the {mover}'s route from {from} runs into stone at {contact:?}"
            );
            if stalled {
                // From where it ran into stone the mover first sets off to the
                // nearest free spot, over whatever ground it stands on (as the
                // stalker steps off the rim's stone below).
                keeps_to_its_grade(mover, path[0], &path[1..], mobility);
            } else {
                keeps_to_its_grade(mover, from, &path, mobility);
            }
        };
        let movers = [
            ("stalker", None, ON_FOOT),
            ("car", Some(car), BY_CAR),
            ("carrier", Some(carrier), CARRIER_ROUTING.0),
        ];
        for (mover, vehicle, mobility) in movers {
            for from in [crater[1], junction, Vec2::new(-844.8, 156.0)] {
                reaches_the_floor(mover, from, vehicle, mobility, false);
            }
            let planned = route(&terrain, (&early.0, &early.1), vehicle, junction, floor)
                .unwrap_or_else(|| panic!("no route for the {mover} before the crater streams in"));
            match first_contact(&terrain, &stone, mobility.body, junction, &planned) {
                Some(stalled) => reaches_the_floor(mover, stalled, vehicle, mobility, true),
                None => {
                    assert_eq!(
                        planned.last(),
                        Some(&floor),
                        "the {mover} plans to the floor"
                    );
                    keeps_to_its_grade(mover, junction, &planned, mobility);
                }
            }
        }
        // Up on the stone it climbed, the stalker first steps down off it.
        let rim = Vec2::new(-1172.4, 99.3);
        let path = route(&terrain, (&solids, &carriers), None, rim, floor)
            .expect("a route from the crater's rim");
        assert_eq!(path.last(), Some(&floor));
        assert!(rim.distance(path[0]) < 3.0, "steps off at {}", path[0]);
        assert_eq!(
            first_contact(&terrain, &stone, STANDING, path[0], &path[1..]),
            None
        );
        keeps_to_its_grade("stalker", path[0], &path[1..], ON_FOOT);
    }

    #[test]
    // A click from the Golden Sphere's junction drives the car down the
    // crater's spiral road to the floor: the autopilot slows for the bends
    // (9.5-15 m) rather than run off the road's edge at 20 m/s at the first.
    fn a_car_ordered_down_the_crater_road_arrives() {
        use std::time::Duration;

        use bevy::ecs::system::SystemState;
        use bevy::time::TimeUpdateStrategy;

        use super::super::vehicle::{Vehicle, drive_vehicle};

        let terrain = Terrain::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (replan_order, drive_vehicle).chain());
        for scan in CutScan::ALL {
            let (pose, _) = cut_scan(scan);
            app.world_mut()
                .spawn((Collider(scan.pillars()), pose, GlobalTransform::from(pose)));
        }
        let crater = crater_ramp();
        let (from, floor) = (crater[1], crater[crater.len() - 1]);
        let heading = crater[2] - from;
        let car = app
            .world_mut()
            .spawn((
                Vehicle::default(),
                Transform::from_xyz(from.x, terrain.mesh_height_at(from), from.y)
                    .with_rotation(Quat::from_rotation_y(heading.x.atan2(heading.y))),
            ))
            .id();
        let mut state = SystemState::<(Solids, Query<(), With<Carrier>>)>::new(app.world_mut());
        let (solids, carriers) = state.get(app.world()).expect("valid queries");
        let path = route(&terrain, (&solids, &carriers), Some(car), from, floor)
            .expect("a route down the crater road");
        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::default(), Driving(car)))
            .id();
        app.world_mut()
            .get_mut::<MoveOrder>(player)
            .expect("order")
            .give(path, from, false, VEHICLE_ARRIVAL);
        for _ in 0..3600 {
            app.update();
            if app
                .world()
                .get::<MoveOrder>(player)
                .expect("order")
                .waypoints()
                .is_empty()
            {
                break;
            }
        }
        let at = app.world().get::<Transform>(car).expect("car").translation;
        assert!(
            at.xz().distance(floor) < VEHICLE_ARRIVAL + 2.0,
            "the car stopped at {at}, {} m from the floor",
            at.xz().distance(floor)
        );
    }

    #[test]
    // `move_player` has no slope limit, so a click on foot may not fail where
    // the stalker could have walked: ground steeper than `ON_FOOT` routes is
    // used when nothing else joins the two. Trips sampled from the Zone, where
    // no 1.0 grade route does.
    fn a_foot_click_falls_back_to_any_slope() {
        use bevy::ecs::system::SystemState;
        let terrain = Terrain::new();
        let mut world = World::new();
        let mut state = SystemState::<(Solids, Query<(), With<Carrier>>)>::new(&mut world);
        let (solids, carriers) = state.get(&world).expect("valid queries");
        let trips = [
            (Vec2::new(-797.0, -550.0), Vec2::new(-837.0, -662.0)),
            (Vec2::new(-660.0, 434.0), Vec2::new(-547.0, 427.0)),
        ];
        for (from, to) in trips {
            assert!(
                find_path(&terrain, [], from, to, ON_FOOT, Reach::Goal).is_none(),
                "a graded route joins {from} and {to}"
            );
            let path = route(&terrain, (&solids, &carriers), None, from, to)
                .unwrap_or_else(|| panic!("no foot route from {from} to {to}"));
            assert_eq!(path.last(), Some(&to));
        }
    }

    #[test]
    // No route keeps to the car's grip all the way (see the foot click above):
    // it drives as near as it can get and stops there.
    fn a_car_click_beyond_its_grip_stops_short() {
        use bevy::ecs::system::SystemState;
        let terrain = Terrain::new();
        let mut world = World::new();
        let car = world.spawn_empty().id();
        let mut state = SystemState::<(Solids, Query<(), With<Carrier>>)>::new(&mut world);
        let (solids, carriers) = state.get(&world).expect("valid queries");
        let (from, to) = (Vec2::new(-797.0, -550.0), Vec2::new(-837.0, -662.0));
        let path = route(&terrain, (&solids, &carriers), Some(car), from, to)
            .expect("a route toward the click");
        let end = *path.last().expect("waypoints");
        assert!(
            end.distance(to) > VEHICLE_ARRIVAL && end.distance(to) < from.distance(to),
            "the car stops short of {to}, nearer than it set off: at {end}"
        );
    }

    #[test]
    fn clicks_land_on_the_rendered_ground_and_skip_the_sky() {
        let terrain = Terrain::new();
        let ground = Vec2::new(40.0, -40.0);
        let eye = ground.extend(terrain.mesh_height_at(ground) + 60.0).xzy() + Vec3::Z * 35.0;
        let aim = ground.extend(terrain.mesh_height_at(ground)).xzy();
        let hit = cast_to_ground(&terrain, Ray3d::new(eye, Dir3::new(aim - eye).unwrap()))
            .expect("ground hit");
        assert!(hit.distance(aim) < 0.5, "hit {hit} aim {aim}");
        assert!((hit.y - terrain.mesh_height_at(hit.xz())).abs() < 0.01);
        assert!(cast_to_ground(&terrain, Ray3d::new(eye, Dir3::Y)).is_none());
    }

    #[test]
    fn orders_walk_short_run_long_and_stop_on_arrival_or_when_blocked() {
        let mut order = MoveOrder::default();
        order.give(
            vec![Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        assert!(!order.runs());
        assert_eq!(order.steer(Vec2::ZERO), Some(Vec2::X));
        assert_eq!(
            order.steer(Vec2::new(2.0, -0.3)),
            Some(Vec2::Y),
            "next waypoint"
        );
        assert_eq!(order.steer(Vec2::new(2.0, 1.8)), None);
        assert!(order.target().is_none());

        order.give(
            vec![Vec2::new(30.0, 0.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        assert_eq!(order.remaining(Vec2::ZERO), 30.0);
        assert!(order.runs());
        let stuck = Vec2::ZERO;
        for replan in 0..3 {
            order.track_progress(stuck, 0.0, 0.2, 0.5, Turning::InPlace);
            assert_eq!(order.wants_replan(), None, "not blocked long enough");
            order.track_progress(stuck, 0.0, 0.2, 0.5, Turning::InPlace);
            assert_eq!(
                order.wants_replan(),
                Some(Vec2::new(30.0, 0.0)),
                "blocked legs are pathfound again ({replan})"
            );
            order.replace_leg(vec![Vec2::new(0.0, 5.0), Vec2::new(30.0, 0.0)]);
            assert!(order.backing_off(), "backs away first");
            order.track_progress(stuck, 0.0, 0.2, STALL_SECONDS + 0.1, Turning::InPlace);
            assert!(!order.backing_off(), "nothing to back into: stop");
        }
        order.track_progress(stuck, 0.0, 0.2, 0.5, Turning::InPlace);
        order.track_progress(stuck, 0.0, 0.2, 0.5, Turning::InPlace);
        assert!(
            order.target().is_none(),
            "an order still blocked after replanning is dropped"
        );
    }

    #[test]
    fn a_mover_reversing_away_passes_no_waypoint() {
        let mut order = MoveOrder::default();
        let (corner, target) = (Vec2::new(4.0, 0.0), Vec2::new(4.0, 20.0));
        order.give(vec![corner, target], Vec2::ZERO, false, VEHICLE_ARRIVAL);
        order.track_progress(
            Vec2::ZERO,
            0.0,
            0.2,
            STALL_SECONDS + 0.1,
            Turning::Reversing,
        );
        assert!(order.backing_off());
        // Within the corner's reach, yet still headed for it.
        let at = Vec2::new(1.5, 0.0);
        assert_eq!(order.steer(at), Some(Vec2::X));
        order.track_progress(at, 6.0, 0.2, BACKOFF_SECONDS + 0.1, Turning::Reversing);
        assert!(!order.backing_off());
        assert_eq!(order.steer(at), Some((target - at).normalize()));
    }

    #[test]
    fn a_walking_mover_backing_off_passes_the_waypoint_beside_it() {
        let mut order = MoveOrder::default();
        let (corner, target) = (Vec2::new(2.0, 0.0), Vec2::new(2.0, 10.0));
        order.give(vec![corner, target], Vec2::ZERO, false, STALKER_ARRIVAL);
        order.track_progress(Vec2::ZERO, 0.0, 0.2, STALL_SECONDS + 0.1, Turning::InPlace);
        assert!(order.backing_off());
        // Within the corner's reach: the stalker takes it and walks on.
        let at = Vec2::new(1.7, 0.0);
        assert_eq!(order.steer(at), Some((target - at).normalize()));
    }

    #[test]
    fn a_blocked_mover_backs_off_six_meters_unless_blocked_behind() {
        let mut order = MoveOrder::default();
        order.give(
            vec![Vec2::new(60.0, 0.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        order.track_progress(Vec2::ZERO, 0.0, 0.2, STALL_SECONDS + 0.1, Turning::InPlace);
        assert!(order.backing_off());
        // Backing freely: a short way back is not enough room to turn.
        let mut at = Vec2::ZERO;
        for _ in 0..10 {
            at.x -= 0.4;
            order.track_progress(at, 0.4, 0.2, 0.1, Turning::InPlace);
        }
        assert!(order.backing_off(), "only {} m back", -at.x);
        while order.backing_off() {
            at.x -= 0.4;
            order.track_progress(at, 0.4, 0.2, 0.1, Turning::InPlace);
        }
        assert!(
            (BACKOFF_DISTANCE..BACKOFF_DISTANCE + 0.4).contains(&-at.x),
            "stopped {} m back",
            -at.x
        );
        // The next stall finds something behind too: it stops backing there.
        order.track_progress(at, 0.0, 0.2, STALL_SECONDS + 0.1, Turning::InPlace);
        assert!(order.backing_off());
        order.track_progress(at, 0.0, 0.2, 0.5, Turning::InPlace);
        assert!(order.backing_off(), "a moment's stillness is not a wall");
        order.track_progress(at, 0.0, 0.2, 0.5, Turning::InPlace);
        assert!(!order.backing_off(), "backed into something");
    }

    /// How many back-offs an order makes against an obstacle that never
    /// moves, with `room` meters free behind the mover.
    fn attempts(turning: Turning, room: f32) -> u32 {
        let mut order = MoveOrder::default();
        order.give(
            vec![Vec2::new(60.0, 0.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        let mut at = Vec2::ZERO;
        for attempt in 0..100 {
            order.track_progress(at, 0.0, 0.2, STALL_SECONDS + 0.1, turning);
            if order.target().is_none() {
                return attempt;
            }
            assert!(order.backing_off());
            if let Some(stop) = order.wants_replan() {
                order.replace_leg(vec![stop]);
            }
            // Back up into whatever lies behind, then forward into the obstacle.
            at.x -= room;
            order.track_progress(at, room, 0.2, 0.1, turning);
            if order.backing_off() {
                order.track_progress(at, 0.0, 0.2, STALL_SECONDS + 0.1, turning);
            }
            assert!(!order.backing_off());
            at.x += room;
        }
        panic!("never gave up");
    }

    #[test]
    fn the_less_room_behind_a_car_the_more_attempts_it_makes() {
        assert_eq!(attempts(Turning::Reversing, BACKOFF_DISTANCE), 3);
        assert_eq!(attempts(Turning::Reversing, 2.0), 9);
        assert_eq!(attempts(Turning::Reversing, 1.0), 18);
        assert_eq!(attempts(Turning::Reversing, 0.3), 60);
        assert_eq!(
            attempts(Turning::Reversing, 0.05),
            60,
            "boxed in: 0.3 m each"
        );
        assert_eq!(
            attempts(Turning::InPlace, 0.3),
            3,
            "the stalker turns on the spot"
        );
    }

    #[test]
    fn a_reached_stop_replans_the_next_leg_from_where_the_stalker_stands() {
        let mut order = MoveOrder::default();
        order.give(
            vec![Vec2::new(4.0, 0.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        order.append(
            vec![Vec2::new(40.0, 0.0), Vec2::new(40.0, 30.0)],
            Vec2::ZERO,
        );
        assert_eq!(order.wants_replan(), None);
        assert_eq!(order.steer(Vec2::new(4.1, 0.0)), Some(Vec2::X));
        assert_eq!(order.wants_replan(), Some(Vec2::new(40.0, 30.0)));
        // A tree streamed in on the way: the new leg ends beside the blocked stop.
        order.replace_leg(vec![Vec2::new(20.0, 10.0), Vec2::new(40.5, 30.0)]);
        assert_eq!(order.wants_replan(), None);
        assert_eq!(
            order.waypoints,
            [Vec2::new(20.0, 10.0), Vec2::new(40.5, 30.0)]
        );
        assert_eq!(order.stops, [Vec2::new(40.5, 30.0)]);
        assert!(
            order.steer(Vec2::new(20.0, 10.1)).is_some(),
            "on along the new leg"
        );
        assert_eq!(order.steer(Vec2::new(40.5, 30.1)), None, "arrives");
        assert_eq!(order.wants_replan(), None, "nothing left to plan");
    }

    #[test]
    fn a_held_route_waits_for_shift_to_be_released() {
        let mut order = MoveOrder::default();
        order.hold();
        assert!(!order.held, "nothing to hold");
        order.give(
            vec![Vec2::new(4.0, 0.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        order.hold();
        order.append(vec![Vec2::new(4.0, 3.0)], Vec2::ZERO);
        assert_eq!(order.steer(Vec2::ZERO), None, "held in place");
        order.track_progress(Vec2::ZERO, 0.0, 0.2, 2.0, Turning::InPlace);
        assert_eq!(order.target(), Some(Vec2::new(4.0, 3.0)), "not a stall");
        order.release();
        assert_eq!(order.steer(Vec2::ZERO), Some(Vec2::X));
    }

    #[test]
    fn shift_clicks_queue_stops_that_clear_as_they_are_reached() {
        let mut order = MoveOrder::default();
        order.give(
            vec![Vec2::new(4.0, 0.0)],
            Vec2::ZERO,
            false,
            STALKER_ARRIVAL,
        );
        order.append(vec![Vec2::new(4.0, 3.0), Vec2::new(6.0, 3.0)], Vec2::ZERO);
        assert_eq!(order.stops, [Vec2::new(4.0, 0.0), Vec2::new(6.0, 3.0)]);
        assert_eq!(order.target(), Some(Vec2::new(6.0, 3.0)));
        assert!(!order.runs(), "a short trip walks");
        order.append(vec![Vec2::new(16.0, 3.0)], Vec2::ZERO);
        assert!(order.runs(), "a long trip runs");
        // Reaching the first stop clears it and heads on to the next leg.
        assert_eq!(order.steer(Vec2::new(4.0, 0.2)), Some(Vec2::Y));
        assert_eq!(order.stops, [Vec2::new(6.0, 3.0), Vec2::new(16.0, 3.0)]);
        assert_eq!(order.steer(Vec2::new(4.0, 3.0)), Some(Vec2::X));
        assert_eq!(order.stops.len(), 2, "a path point is no stop");
        assert_eq!(order.steer(Vec2::new(6.0, 3.0)), Some(Vec2::X));
        assert_eq!(order.stops, [Vec2::new(16.0, 3.0)]);
    }
}
