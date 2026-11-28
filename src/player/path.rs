//! Grid A* around solid pillars, deep water and ground too steep to climb for
//! point-and-click orders.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::prelude::*;

use crate::world::{Body, Collider, STEP_HEIGHT, Terrain};

/// Finest grid spacing, in meters; coarser for long trips to bound the search.
const CELL: f32 = 0.5;
/// Extra room around the trip's bounding box for detours, in meters.
const MARGIN: f32 = 28.0;
/// The room searched when no way lies within `MARGIN`: a winding road's laps
/// (the crater's spiral reaches 44 m past its floor) with room to spare.
const WIDE_MARGIN: f32 = 3.0 * MARGIN;
/// Upper bound on searched cells, keeping a click well under a frame.
const MAX_CELLS: usize = 260_000;

/// What a mover can get through: its footprint, the deepest water it wades or
/// fords, and the steepest ground it climbs or descends (rise over run).
#[derive(Clone, Copy)]
pub(in crate::player) struct Mobility {
    pub(in crate::player) body: Body,
    pub(in crate::player) max_depth: f32,
    pub(in crate::player) max_grade: f32,
}

/// How near a route must come to where it was asked to go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::player) enum Reach {
    /// All the way, or no route.
    Goal,
    /// As near as the mover can get, where it stops: a vehicle clicked onto
    /// ground too steep or deep for it drives to the edge of what it can reach.
    Nearest,
}

/// An upright obstacle footprint in world space, already grown by the body radius.
struct Obstacle {
    center: Vec2,
    reach: f32,
}

struct Grid {
    origin: Vec2,
    cell: f32,
    size: UVec2,
    blocked: Vec<bool>,
}

impl Grid {
    fn index(&self, cell: IVec2) -> Option<usize> {
        (cell.cmpge(IVec2::ZERO).all() && cell.cmplt(self.size.as_ivec2()).all())
            .then(|| (cell.y as u32 * self.size.x + cell.x as u32) as usize)
    }

    fn cell_of(&self, index: usize) -> IVec2 {
        let width = self.size.x as usize;
        IVec2::new((index % width) as i32, (index / width) as i32)
    }

    fn cell_at(&self, at: Vec2) -> IVec2 {
        ((at - self.origin) / self.cell).floor().as_ivec2()
    }

    fn center(&self, cell: IVec2) -> Vec2 {
        self.origin + (cell.as_vec2() + 0.5) * self.cell
    }

    fn free(&self, cell: IVec2) -> bool {
        self.index(cell).is_some_and(|i| !self.blocked[i])
    }

    /// Whether a straight walk between two points stays on free cells.
    fn clear_line(&self, from: Vec2, to: Vec2) -> bool {
        let steps = (from.distance(to) / (self.cell * 0.5)).ceil().max(1.0) as usize;
        (0..=steps).all(|i| self.free(self.cell_at(from.lerp(to, i as f32 / steps as f32))))
    }
}

/// Waypoints from `from` to `to` (excluding the start), or `None` if unreachable;
/// with `Reach::Nearest`, to the reachable spot nearest `to` instead (`None` only
/// if the mover can get no nearer than it stands). The whole footprint keeps to rendered ground no steeper than
/// `mobility.max_grade`, so into a pit the route takes the road graded down
/// its walls. The search keeps to the trip's box and `MARGIN` around it; should no way
/// lie in there, it searches once more within `WIDE_MARGIN`, for a road that
/// winds out of the trip's box, as the crater's spiral does; the nearest spot
/// comes from that wider search.
pub(super) fn find_path<'a>(
    terrain: &Terrain,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
    from: Vec2,
    to: Vec2,
    mobility: Mobility,
    reach: Reach,
) -> Option<Vec<Vec2>> {
    let colliders: Vec<_> = colliders.into_iter().collect();
    let mut nearest = None;
    for margin in [MARGIN, WIDE_MARGIN] {
        let colliders = colliders.iter().copied();
        match find_path_within(terrain, colliders, from, to, mobility, margin) {
            Some((path, true)) => return Some(path),
            found => nearest = found.map(|(path, _)| path),
        }
    }
    nearest.filter(|_| reach == Reach::Nearest)
}

fn find_path_within<'a>(
    terrain: &Terrain,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
    from: Vec2,
    to: Vec2,
    mobility: Mobility,
    margin: f32,
) -> Option<(Vec<Vec2>, bool)> {
    let low = from.min(to) - margin;
    let high = from.max(to) + margin;
    let area = (high - low).x * (high - low).y;
    let cell = CELL.max((area / MAX_CELLS as f32).sqrt());
    let size = ((high - low) / cell).ceil().as_uvec2().max(UVec2::ONE);
    let obstacles = obstacles(terrain, colliders, low, high, mobility.body);
    let mut grid = Grid {
        origin: low,
        cell,
        size,
        blocked: vec![false; (size.x * size.y) as usize],
    };
    rasterize(&mut grid, terrain, &obstacles, mobility);

    // A mover pressed into a solid (or climbing it, after it stalled there)
    // sets off from the nearest free spot, as an order ends at one.
    let start = nearest_free(&grid, grid.cell_at(from))?;
    let goal = nearest_free(&grid, grid.cell_at(to)).unwrap_or(grid.cell_at(to));
    let (cells, whole) = search(&grid, start, goal)?;
    if !whole && cells.len() < 2 {
        return None;
    }
    let mut points: Vec<Vec2> = cells.iter().map(|&c| grid.center(c)).collect();
    if whole && goal == grid.cell_at(to) {
        *points.last_mut()? = to;
    }
    Some((smooth(&grid, from, points), whole))
}

fn obstacles<'a>(
    terrain: &Terrain,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
    low: Vec2,
    high: Vec2,
    body: Body,
) -> Vec<Obstacle> {
    let mut found = Vec::new();
    for (collider, transform) in colliders {
        let (scale, _, _) = transform.to_scale_rotation_translation();
        for pillar in collider.0 {
            let center = transform.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let reach = pillar.radius * scale.x + body.radius;
            if (center.xz() + reach).cmplt(low).any() || (center.xz() - reach).cmpgt(high).any() {
                continue;
            }
            // Same rule as `push_out` (low pillars are stepped over, high ones
            // passed under) for feet anywhere on the ground the body touches
            // the pillar from: on a slope it comes up from below the pillar's
            // foot, or down from above it.
            let top = center.y + pillar.top * scale.y;
            let bottom = center.y + pillar.bottom * scale.y;
            let blocks = |(lowest, highest): (f32, f32)| {
                top > lowest + STEP_HEIGHT && bottom < highest + body.height
            };
            // Most pillars block from the ground at their foot already.
            let foot = terrain.mesh_height_at(center.xz());
            if blocks((foot, foot)) || blocks(ground_around(terrain, center.xz(), reach, foot)) {
                found.push(Obstacle {
                    center: center.xz(),
                    reach,
                });
            }
        }
    }
    found
}

/// The lowest and highest rendered ground at `center` (where it is `foot`
/// high) and at eight points `radius` around it, where a body touching a
/// pillar there stands.
fn ground_around(terrain: &Terrain, center: Vec2, radius: f32, foot: f32) -> (f32, f32) {
    (0..8)
        .map(|i| {
            let around = Vec2::from_angle(i as f32 * std::f32::consts::FRAC_PI_4) * radius;
            terrain.mesh_height_at(center + around)
        })
        .fold((foot, foot), |(low, high), y| (low.min(y), high.max(y)))
}

/// Blocks the cells in reach of an obstacle, under deep water, or where the
/// body would stand partly on rendered ground too steep for it.
fn rasterize(grid: &mut Grid, terrain: &Terrain, obstacles: &[Obstacle], mobility: Mobility) {
    let ground = terrain.mesh_patch(grid.origin, grid.center(grid.size.as_ivec2()));
    for i in 0..grid.blocked.len() {
        let at = grid.center(grid.cell_of(i));
        grid.blocked[i] = ground.grade_at(at) > mobility.max_grade
            || terrain.water_depth_at(at.x, at.y) > mobility.max_depth;
    }
    // Grow by half a cell diagonal so every point of a free cell stays clear.
    let grow = grid.cell * std::f32::consts::FRAC_1_SQRT_2;
    for obstacle in obstacles {
        let reach = obstacle.reach + grow;
        block_near(
            grid,
            obstacle.center - reach,
            obstacle.center + reach,
            |at| at.distance(obstacle.center) < reach,
        );
    }
    // Steep ground is measured exactly, from its triangles: growing it too
    // would close a road cut only a little wider than a vehicle (the crater's
    // deep laps leave 6.1 m of passable ground for the carrier's 4.6 m).
    let reach = mobility.body.radius;
    for rim in ground.steep_rims(mobility.max_grade) {
        let (low, high) = rim.iter().fold((rim[0], rim[0]), |(low, high), &corner| {
            (low.min(corner), high.max(corner))
        });
        block_near(grid, low - reach, high + reach, |at| {
            (0..3).any(|i| distance_to_segment(at, rim[i], rim[(i + 1) % 3]) < reach)
        });
    }
}

/// Blocks every cell between `low` and `high` whose centre is `near`.
fn block_near(grid: &mut Grid, low: Vec2, high: Vec2, near: impl Fn(Vec2) -> bool) {
    let (low, high) = (grid.cell_at(low), grid.cell_at(high));
    for y in low.y..=high.y {
        for x in low.x..=high.x {
            let cell = IVec2::new(x, y);
            if let Some(i) = grid.index(cell)
                && near(grid.center(cell))
            {
                grid.blocked[i] = true;
            }
        }
    }
}

fn distance_to_segment(at: Vec2, a: Vec2, b: Vec2) -> f32 {
    let t = ((at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
    at.distance(a.lerp(b, t))
}

/// The free cell closest to `cell`, searching outward ring by ring.
fn nearest_free(grid: &Grid, cell: IVec2) -> Option<IVec2> {
    (0..48).find_map(|ring: i32| {
        (-ring..=ring)
            .flat_map(|y| (-ring..=ring).map(move |x| IVec2::new(x, y)))
            .filter(|offset| offset.abs().max_element() == ring)
            .map(|offset| cell + offset)
            .filter(|&c| grid.free(c))
            .min_by_key(|&c| (c - cell).length_squared())
    })
}

/// 8-connected A* with an octile heuristic; diagonals may not cut blocked corners.
/// The cells to `goal` and `true`, or, when it can't be reached, to the reached
/// cell nearest it (the cheapest of equals) and `false`.
fn search(grid: &Grid, start: IVec2, goal: IVec2) -> Option<(Vec<IVec2>, bool)> {
    const STEPS: [(IVec2, f32); 8] = [
        (IVec2::new(1, 0), 1.0),
        (IVec2::new(-1, 0), 1.0),
        (IVec2::new(0, 1), 1.0),
        (IVec2::new(0, -1), 1.0),
        (IVec2::new(1, 1), std::f32::consts::SQRT_2),
        (IVec2::new(1, -1), std::f32::consts::SQRT_2),
        (IVec2::new(-1, 1), std::f32::consts::SQRT_2),
        (IVec2::new(-1, -1), std::f32::consts::SQRT_2),
    ];
    let heuristic = |cell: IVec2| {
        let d = (goal - cell).abs().as_vec2();
        d.max_element() + (std::f32::consts::SQRT_2 - 1.0) * d.min_element()
    };
    let start_index = grid.index(start)?;
    let mut cost = vec![f32::INFINITY; grid.blocked.len()];
    let mut came_from = vec![usize::MAX; grid.blocked.len()];
    let mut open = BinaryHeap::new();
    cost[start_index] = 0.0;
    open.push((Reverse((heuristic(start) * 1024.0) as u32), start_index));
    let mut nearest = (heuristic(start), 0.0, start_index);
    while let Some((_, here)) = open.pop() {
        let cell = grid.cell_of(here);
        if cell == goal {
            return Some((trace(grid, &came_from, here), true));
        }
        let left = heuristic(cell);
        if (left, cost[here]) < (nearest.0, nearest.1) {
            nearest = (left, cost[here], here);
        }
        for (step, length) in STEPS {
            let next = cell + step;
            let corners_free = step.x == 0
                || step.y == 0
                || grid.free(cell + IVec2::new(step.x, 0))
                    && grid.free(cell + IVec2::new(0, step.y));
            let Some(there) = grid.index(next).filter(|&i| !grid.blocked[i]) else {
                continue;
            };
            let through = cost[here] + length;
            if corners_free && through < cost[there] {
                cost[there] = through;
                came_from[there] = here;
                open.push((
                    Reverse(((through + heuristic(next)) * 1024.0) as u32),
                    there,
                ));
            }
        }
    }
    Some((trace(grid, &came_from, nearest.2), false))
}

/// The cells from the search's start to cell `index`, following `came_from`.
fn trace(grid: &Grid, came_from: &[usize], mut index: usize) -> Vec<IVec2> {
    let mut path = vec![grid.cell_of(index)];
    while came_from[index] != usize::MAX {
        index = came_from[index];
        path.push(grid.cell_of(index));
    }
    path.reverse();
    path
}

/// Drops waypoints that a straight walk can skip.
fn smooth(grid: &Grid, from: Vec2, points: Vec<Vec2>) -> Vec<Vec2> {
    let mut kept = Vec::new();
    let mut anchor = from;
    for (i, &point) in points.iter().enumerate() {
        let next = points.get(i + 1);
        if next.is_none_or(|&next| !grid.clear_line(anchor, next)) {
            kept.push(point);
            anchor = point;
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::STANDING;
    use crate::world::{CutScan, Pillar, cut_scan, quarry_ramp};

    const WALL: &[Pillar] = &[
        Pillar {
            x: -3.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
        Pillar {
            x: -2.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
        Pillar {
            x: -1.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
        Pillar {
            x: 0.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
        Pillar {
            x: 1.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
        Pillar {
            x: 2.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
        Pillar {
            x: 3.0,
            z: 0.0,
            radius: 0.6,
            bottom: 0.0,
            top: 3.0,
        },
    ];

    #[test]
    fn paths_detour_around_a_wall_and_go_straight_in_the_open() {
        let terrain = Terrain::new();
        let body = Body {
            radius: 0.35,
            height: 1.8,
        };
        let center = Vec2::new(40.0, -60.0);
        let ground = terrain.mesh_height_at(center);
        let wall = GlobalTransform::from_xyz(center.x, ground, center.y);
        let from = center - Vec2::Y * 5.0;
        let to = center + Vec2::Y * 5.0;

        let mobility = Mobility {
            body,
            max_depth: 1.0,
            max_grade: 1.0,
        };
        let open = find_path(&terrain, [], from, to, mobility, Reach::Goal).expect("open path");
        assert_eq!(open, vec![to], "an open field needs no detour");

        let collider = Collider(WALL);
        let path = find_path(
            &terrain,
            [(&collider, &wall)],
            from,
            to,
            mobility,
            Reach::Goal,
        )
        .expect("detour");
        assert_eq!(path.last(), Some(&to));
        assert!(path.len() >= 2, "the walk bends around the wall: {path:?}");
        let mut previous = from;
        for &point in &path {
            for i in 0..=20 {
                let at = previous.lerp(point, i as f32 / 20.0);
                assert_eq!(
                    push_free(at, &collider, &wall, body),
                    at,
                    "path crosses the wall at {at}"
                );
            }
            previous = point;
        }
    }

    fn length(points: &[Vec2]) -> f32 {
        points.windows(2).map(|leg| leg[0].distance(leg[1])).sum()
    }

    #[test]
    // Down the ramp, not the pit's front wall.
    fn a_click_order_reaches_the_pit_floor() {
        let terrain = Terrain::new();
        let (pose, _) = cut_scan(CutScan::Quarry);
        let scan = GlobalTransform::from(pose);
        let collider = Collider(CutScan::Quarry.pillars());
        let ramp = &quarry_ramp()[1..];
        let (entrance, pit) = (ramp[0], ramp[ramp.len() - 1]);

        let on_foot = Mobility {
            body: STANDING,
            max_depth: 1.0,
            max_grade: 1.0,
        };
        let path = find_path(
            &terrain,
            [(&collider, &scan)],
            entrance,
            pit,
            on_foot,
            Reach::Goal,
        )
        .expect("a route down the ramp");
        let route = length(&[&[entrance], path.as_slice()].concat());
        let limit = length(ramp) * 1.6;
        assert!(
            route <= limit,
            "the route is {route} m, the limit {limit} m"
        );
    }

    #[test]
    fn a_walled_in_goal_is_refused_or_neared() {
        let terrain = Terrain::new();
        let center = Vec2::new(40.0, -60.0);
        let ground = terrain.mesh_height_at(center);
        let pen = GlobalTransform::from_xyz(center.x, ground, center.y);
        // A closed ring of posts 6 m across around the goal.
        let ring: Vec<Pillar> = (0..48)
            .map(|i| {
                let at = Vec2::from_angle(i as f32 * std::f32::consts::TAU / 48.0) * 6.0;
                Pillar {
                    x: at.x,
                    z: at.y,
                    radius: 0.6,
                    bottom: 0.0,
                    top: 3.0,
                }
            })
            .collect();
        let collider = Collider(Box::leak(ring.into_boxed_slice()));
        let mobility = Mobility {
            body: Body {
                radius: 0.35,
                height: 1.8,
            },
            max_depth: 1.0,
            max_grade: 1.0,
        };
        let from = center - Vec2::Y * 20.0;
        let find = |reach| find_path(&terrain, [(&collider, &pen)], from, center, mobility, reach);
        assert_eq!(find(Reach::Goal), None, "the goal is walled in");
        let path = find(Reach::Nearest).expect("a route to the ring");
        let end = *path.last().expect("waypoints");
        let gap = end.distance(center);
        assert!(
            (6.6..8.0).contains(&gap),
            "the route stops outside the ring, {gap} m from the goal"
        );
    }

    fn push_free(at: Vec2, collider: &Collider, wall: &GlobalTransform, body: Body) -> Vec2 {
        let feet = wall.translation().y;
        crate::world::push_out(at, feet, body, [(collider, wall)])
    }
}
