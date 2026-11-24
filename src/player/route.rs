//! The thin line on the ground that shows the exact route of a point-and-click order:
//! from where the stalker (or the car they drive) stands, through every pathfound
//! waypoint still ahead, to the last ring. It shrinks as waypoints are reached and
//! goes with the order.

use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::config::{GizmoConfig, GizmoConfigGroup, GizmoLineConfig};
use bevy::prelude::*;

use crate::states::AppState;
use crate::world::Terrain;

use super::Player;
use super::orders::{MoveOrder, sync_order_markers};

/// Meters between the points of the line: close enough to follow hills.
const ROUTE_STEP: f32 = 2.0;
/// At most this many steps in one leg, so a click across the map stays cheap.
const ROUTE_MAX_STEPS: usize = 64;
/// How far the line floats above the ground (or the water over it), in meters.
const ROUTE_LIFT: f32 = 0.08;
/// The line's width in pixels: thin at every zoom.
const ROUTE_WIDTH: f32 = 2.0;
/// Pulls the line 1 % of its distance toward the camera, so a shallow view does not bury it in the
/// ground, while walls and buildings in front of it still hide it.
const ROUTE_DEPTH_BIAS: f32 = -0.01;
/// The order rings' colour (`OrderMarkerLook`), a little translucent.
const ROUTE_COLOR: Color = Color::srgba(0.95, 0.78, 0.35, 0.8);

/// The gizmo group the route is drawn with, to give it its own width and depth bias.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct RouteGizmos;

pub(super) struct RoutePlugin;

impl Plugin for RoutePlugin {
    fn build(&self, app: &mut App) {
        app.insert_gizmo_config(
            RouteGizmos,
            GizmoConfig {
                line: GizmoLineConfig {
                    width: ROUTE_WIDTH,
                    ..default()
                },
                depth_bias: ROUTE_DEPTH_BIAS,
                ..default()
            },
        )
        // Not only while running: a gizmo is drawn each frame it is submitted, so the line would vanish
        // under the pause menu while the rings stay.
        .add_systems(
            Update,
            draw_order_route
                .after(sync_order_markers)
                .run_if(in_state(AppState::InGame)),
        );
    }
}

/// The points of the leg `from`..`to`, about `ROUTE_STEP` meters apart (`ROUTE_MAX_STEPS` steps at most),
/// both ends exact.
fn segment_points(from: Vec2, to: Vec2) -> impl Iterator<Item = Vec2> {
    let steps = ((from.distance(to) / ROUTE_STEP).ceil() as usize).clamp(1, ROUTE_MAX_STEPS);
    (0..=steps).map(move |i| {
        if i == steps {
            to
        } else {
            from.lerp(to, i as f32 / steps as f32)
        }
    })
}

/// The leg `from`..`to` laid over the ground, or over the water where the stalker wades (as the locator ring is).
fn drape(terrain: &Terrain, from: Vec2, to: Vec2) -> impl Iterator<Item = Vec3> + '_ {
    segment_points(from, to).map(|at| {
        let surface = terrain.mesh_height_at(at) + terrain.water_depth_at(at.x, at.y);
        at.extend(surface + ROUTE_LIFT).xzy()
    })
}

/// The draped line through an order's waypoints from the first one on. The waypoints change only when an order
/// is given, appended to, or reaches one, so the line is laid over the ground then (Precalculate once) rather
/// than every frame; only the leg from the moving stalker to the first waypoint is laid each frame.
#[derive(Default)]
struct RouteCache {
    waypoints: Vec<Vec2>,
    line: Vec<Vec3>,
}

impl RouteCache {
    /// Rebuilds the line if `waypoints` differ from the last ones; whether it did.
    fn refresh(&mut self, terrain: &Terrain, waypoints: &[Vec2]) -> bool {
        if self.waypoints == waypoints {
            return false;
        }
        self.waypoints = waypoints.to_vec();
        self.line.clear();
        match waypoints {
            [] => {}
            [only] => self.line.extend(drape(terrain, *only, *only).take(1)),
            _ => {
                for leg in waypoints.windows(2) {
                    let skip = usize::from(!self.line.is_empty());
                    self.line.extend(drape(terrain, leg[0], leg[1]).skip(skip));
                }
            }
        }
        true
    }
}

/// Draws the route of the player's order while it lasts.
fn draw_order_route(
    mut gizmos: Gizmos<RouteGizmos>,
    terrain: Res<Terrain>,
    mut cache: Local<RouteCache>,
    players: Query<(&Transform, &MoveOrder), With<Player>>,
) {
    let Ok((stalker, order)) = players.single() else {
        return;
    };
    let waypoints = order.waypoints();
    cache.refresh(&terrain, waypoints);
    let Some(&next) = waypoints.first() else {
        return;
    };
    let head = drape(&terrain, stalker.translation.xz(), next);
    gizmos.linestrip(head.chain(cache.line.iter().skip(1).copied()), ROUTE_COLOR);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_is_sampled_evenly_with_exact_ends_and_a_capped_count() {
        let (from, to) = (Vec2::new(3.0, -4.0), Vec2::new(11.0, 2.0));
        let points = segment_points(from, to).collect::<Vec<_>>();
        assert_eq!((points[0], *points.last().unwrap()), (from, to));
        // 10 m long: five steps of 2 m, all on the segment and evenly spaced.
        assert_eq!(points.len(), 6);
        let step = from.distance(to) / 5.0;
        for pair in points.windows(2) {
            assert!((pair[0].distance(pair[1]) - step).abs() < 1e-4);
        }
        assert!(
            points
                .iter()
                .all(|p| (p - from).perp_dot(to - from).abs() < 1e-3)
        );
        // A click across the map costs no more than the cap, and a short hop still has both ends.
        let far = segment_points(Vec2::ZERO, Vec2::new(12_000.0, 0.0)).collect::<Vec<_>>();
        assert_eq!(far.len(), ROUTE_MAX_STEPS + 1);
        assert_eq!(far.last(), Some(&Vec2::new(12_000.0, 0.0)));
        let hop = segment_points(Vec2::ZERO, Vec2::new(0.5, 0.0)).collect::<Vec<_>>();
        assert_eq!(hop, [Vec2::ZERO, Vec2::new(0.5, 0.0)]);
    }

    #[test]
    fn the_route_lies_on_the_ground_or_the_water_above_it() {
        let terrain = Terrain::new();
        let (from, to) = (Vec2::new(40.0, -40.0), Vec2::new(70.0, -10.0));
        let line = drape(&terrain, from, to).collect::<Vec<_>>();
        assert!(line.len() > 10);
        for point in &line {
            let at = point.xz();
            let surface = terrain.mesh_height_at(at) + terrain.water_depth_at(at.x, at.y);
            assert!((point.y - surface - ROUTE_LIFT).abs() < 1e-4, "{point}");
        }
        assert_eq!(line[0].xz(), from);
        assert_eq!(line.last().unwrap().xz(), to);
    }

    #[test]
    fn the_cache_rebuilds_only_when_the_waypoints_change() {
        let terrain = Terrain::new();
        let mut cache = RouteCache::default();
        let route = [
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 8.0),
            Vec2::new(0.0, 8.0),
        ];
        assert!(cache.refresh(&terrain, &route));
        // Waypoint 0 first, then the legs through the rest, each leg's start not repeated.
        assert_eq!(cache.line.first().unwrap().xz(), route[0]);
        assert_eq!(cache.line.last().unwrap().xz(), route[2]);
        let legs = drape(&terrain, route[0], route[1]).count()
            + drape(&terrain, route[1], route[2]).count();
        assert_eq!(cache.line.len(), legs - 1);
        assert!(
            !cache.refresh(&terrain, &route),
            "unchanged: nothing to rebuild"
        );
        assert!(cache.refresh(&terrain, &route[1..]), "a waypoint reached");
        assert_eq!(cache.line.first().unwrap().xz(), route[1]);
        assert!(cache.refresh(&terrain, &route[1..2]), "a single point left");
        assert_eq!(cache.line.len(), 1);
        assert!(cache.refresh(&terrain, &[]), "the order ended");
        assert!(cache.line.is_empty());
        assert!(!cache.refresh(&terrain, &[]));
    }
}
