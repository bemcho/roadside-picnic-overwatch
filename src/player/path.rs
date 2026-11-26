//! Grid A* around solid pillars and deep water for point-and-click orders.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::prelude::*;

use crate::world::{Body, Collider, STEP_HEIGHT, Terrain};

/// Finest grid spacing, in meters; coarser for long trips to bound the search.
const CELL: f32 = 0.5;
/// Extra room around the trip's bounding box for detours, in meters.
const MARGIN: f32 = 28.0;
/// Upper bound on searched cells, keeping a click well under a frame.
const MAX_CELLS: usize = 260_000;

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

/// Waypoints from `from` to `to` (excluding the start), or `None` if unreachable.
pub(super) fn find_path<'a>(
    terrain: &Terrain,
    colliders: impl IntoIterator<Item = (&'a Collider, &'a GlobalTransform)>,
    from: Vec2,
    to: Vec2,
    body: Body,
    max_depth: f32,
) -> Option<Vec<Vec2>> {
    let low = from.min(to) - MARGIN;
    let high = from.max(to) + MARGIN;
    let area = (high - low).x * (high - low).y;
    let cell = CELL.max((area / MAX_CELLS as f32).sqrt());
    let size = ((high - low) / cell).ceil().as_uvec2().max(UVec2::ONE);
    let obstacles = obstacles(terrain, colliders, low, high, body);
    let mut grid = Grid {
        origin: low,
        cell,
        size,
        blocked: vec![false; (size.x * size.y) as usize],
    };
    rasterize(&mut grid, terrain, &obstacles, max_depth);

    let start = grid.cell_at(from);
    let goal = nearest_free(&grid, grid.cell_at(to))?;
    if let Some(i) = grid.index(start) {
        grid.blocked[i] = false;
    }
    let cells = search(&grid, start, goal)?;
    let mut points: Vec<Vec2> = cells.iter().map(|&c| grid.center(c)).collect();
    if goal == grid.cell_at(to) {
        *points.last_mut()? = to;
    }
    Some(smooth(&grid, from, points))
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
            // Same rule as `push_out`: low pillars are stepped over, high ones passed under.
            let feet = terrain.mesh_height_at(center.xz());
            let top = center.y + pillar.top * scale.y;
            let bottom = center.y + pillar.bottom * scale.y;
            if top > feet + STEP_HEIGHT && bottom < feet + body.height {
                found.push(Obstacle {
                    center: center.xz(),
                    reach,
                });
            }
        }
    }
    found
}

fn rasterize(grid: &mut Grid, terrain: &Terrain, obstacles: &[Obstacle], max_depth: f32) {
    // Grow by half a cell diagonal so every point of a free cell stays clear.
    let grow = grid.cell * std::f32::consts::FRAC_1_SQRT_2;
    for obstacle in obstacles {
        let reach = obstacle.reach + grow;
        let low = grid.cell_at(obstacle.center - reach);
        let high = grid.cell_at(obstacle.center + reach);
        for y in low.y..=high.y {
            for x in low.x..=high.x {
                let cell = IVec2::new(x, y);
                if let Some(i) = grid.index(cell)
                    && grid.center(cell).distance(obstacle.center) < reach
                {
                    grid.blocked[i] = true;
                }
            }
        }
    }
    for y in 0..grid.size.y as i32 {
        for x in 0..grid.size.x as i32 {
            let cell = IVec2::new(x, y);
            let at = grid.center(cell);
            if let Some(i) = grid.index(cell)
                && terrain.water_depth_at(at.x, at.y) > max_depth
            {
                grid.blocked[i] = true;
            }
        }
    }
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
fn search(grid: &Grid, start: IVec2, goal: IVec2) -> Option<Vec<IVec2>> {
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
    while let Some((_, here)) = open.pop() {
        let cell = grid.cell_of(here);
        if cell == goal {
            let mut path = vec![cell];
            let mut i = here;
            while came_from[i] != usize::MAX {
                i = came_from[i];
                path.push(grid.cell_of(i));
            }
            path.reverse();
            return Some(path);
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
    None
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
    use crate::world::{Pillar, quarry_pillars, quarry_ramp, quarry_scan};

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

        let open = find_path(&terrain, [], from, to, body, 1.0).expect("open path");
        assert_eq!(open, vec![to], "an open field needs no detour");

        let collider = Collider(WALL);
        let path = find_path(&terrain, [(&collider, &wall)], from, to, body, 1.0).expect("detour");
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
    fn a_click_order_routes_down_the_ramp() {
        let terrain = Terrain::new();
        let (pose, _) = quarry_scan();
        let scan = GlobalTransform::from(pose);
        let collider = Collider(quarry_pillars());
        let ramp = &quarry_ramp()[1..];
        let (entrance, pit) = (ramp[0], ramp[ramp.len() - 1]);

        let path = find_path(&terrain, [(&collider, &scan)], entrance, pit, STANDING, 1.0)
            .expect("a route down the ramp");
        let route = length(&[&[entrance], path.as_slice()].concat());
        let limit = length(ramp) * 1.6;
        assert!(
            route <= limit,
            "the route is {route} m, the limit {limit} m"
        );
    }

    fn push_free(at: Vec2, collider: &Collider, wall: &GlobalTransform, body: Body) -> Vec2 {
        let feet = wall.translation().y;
        crate::world::push_out(at, feet, body, [(collider, wall)])
    }
}
