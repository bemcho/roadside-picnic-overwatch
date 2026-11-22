//! Point-and-click movement: left-click the ground to send the stalker there.
//! WASD still steers directly and cancels the order.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::states::AppState;
use crate::world::{Body, Collider, Terrain, Weight};

use super::path::find_path;
use super::vehicle::{CAR, Driving, FORD_DEPTH};
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
/// The vehicle's footprint for pathfinding.
const VEHICLE_BODY: Body = Body {
    radius: 1.7,
    height: 2.4,
};
/// An order that makes this little of its intended progress is blocked.
const STALL_RATIO: f32 = 0.15;
/// How long the stalker may stay blocked before giving up the order.
const STALL_SECONDS: f32 = 0.8;
/// Farthest point on the ground a click can reach, in meters.
const CLICK_REACH: f32 = 12_000.0;

/// Where the stalker was told to go: the remaining waypoints, target last.
#[derive(Component, Default)]
pub struct MoveOrder {
    waypoints: Vec<Vec2>,
    run: bool,
    arrival: f32,
    stalled: f32,
}

impl MoveOrder {
    pub(super) fn give(&mut self, waypoints: Vec<Vec2>, from: Vec2, run: bool, arrival: f32) {
        *self = Self {
            run: run || path_length(from, &waypoints) > RUN_DISTANCE,
            waypoints,
            arrival,
            stalled: 0.0,
        };
    }

    /// Distance still to travel along the remaining waypoints.
    pub(super) fn remaining(&self, at: Vec2) -> f32 {
        path_length(at, &self.waypoints)
    }

    fn target(&self) -> Option<Vec2> {
        self.waypoints.last().copied()
    }

    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }

    /// Ground direction toward the next waypoint, or `None` once arrived or idle.
    pub(super) fn steer(&mut self, at: Vec2) -> Option<Vec2> {
        while let Some(&next) = self.waypoints.first() {
            let reach = if self.waypoints.len() == 1 {
                self.arrival
            } else {
                self.arrival * WAYPOINT_SLACK
            };
            if at.distance(next) > reach {
                return Some((next - at).normalize());
            }
            self.waypoints.remove(0);
        }
        self.cancel();
        None
    }

    pub(super) fn runs(&self) -> bool {
        !self.waypoints.is_empty() && self.run
    }

    /// Drops the order when the stalker keeps pushing against an obstacle.
    pub(super) fn track_progress(&mut self, moved: f32, intended: f32, dt: f32) {
        if self.waypoints.is_empty() || intended <= 0.0 {
            return;
        }
        self.stalled = if moved < intended * STALL_RATIO {
            self.stalled + dt
        } else {
            0.0
        };
        if self.stalled > STALL_SECONDS {
            self.cancel();
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

/// Ground ring showing the current move target.
#[derive(Component)]
pub(super) struct OrderMarker;

pub(super) fn spawn_order_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        OrderMarker,
        Mesh3d(meshes.add(Annulus::new(0.32, 0.45))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.78, 0.35),
            emissive: LinearRgba::rgb(900.0, 620.0, 180.0),
            unlit: true,
            ..default()
        })),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Visibility::Hidden,
        DespawnOnExit(AppState::InGame),
    ));
}

/// Turns a left click on the ground into a pathfound move order for the stalker,
/// or for the vehicle while the stalker drives it.
pub(super) fn issue_move_order(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<Terrain>,
    window: Single<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    colliders: Query<(Entity, &Collider, &GlobalTransform, Option<&Weight>)>,
    mut players: Query<(&Transform, &mut MoveOrder, Option<&Driving>), With<Player>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (Some(cursor), Ok((camera, eye)), Ok((stalker, mut order, driving))) = (
        window.cursor_position(),
        cameras.single(),
        players.single_mut(),
    ) else {
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
    let vehicle = driving.map(|driving| driving.0);
    let (body, depth, arrival, mover) = match vehicle {
        Some(_) => (VEHICLE_BODY, FORD_DEPTH, VEHICLE_ARRIVAL, CAR),
        None => (STANDING, WADE_DEPTH, STALKER_ARRIVAL, STALKER),
    };
    // The driven vehicle must not block its own route, nor anything lighter.
    let obstacles = colliders
        .iter()
        .filter(|&(entity, _, _, weight)| Some(entity) != vehicle && mover.blocked_by(weight))
        .map(|(_, collider, pose, _)| (collider, pose));
    let Some(path) = find_path(&terrain, obstacles, from, target.xz(), body, depth) else {
        return;
    };
    let run = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    order.give(path, from, run, arrival);
}

/// Shows the marker at an active target; boarding or leaving a vehicle drops the order.
pub(super) fn sync_order_marker(
    terrain: Res<Terrain>,
    mut was_driving: Local<bool>,
    mut players: Query<(&mut MoveOrder, Has<Driving>), With<Player>>,
    mut markers: Query<(&mut Transform, &mut Visibility), With<OrderMarker>>,
) {
    let (Ok((mut order, driving)), Ok((mut marker, mut visibility))) =
        (players.single_mut(), markers.single_mut())
    else {
        return;
    };
    if driving != *was_driving {
        *was_driving = driving;
        order.cancel();
    }
    let shown = match order.target() {
        Some(at) => {
            marker.translation = at.extend(terrain.mesh_height_at(at) + 0.06).xzy();
            Visibility::Inherited
        }
        None => Visibility::Hidden,
    };
    visibility.set_if_neq(shown);
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
        order.track_progress(0.0, 0.2, 0.5);
        assert!(order.target().is_some());
        order.track_progress(0.0, 0.2, 0.5);
        assert!(order.target().is_none(), "a blocked order is dropped");
    }
}
