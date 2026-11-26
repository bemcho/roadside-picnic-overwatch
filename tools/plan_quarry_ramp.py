"""Plan the quarry's haul road: a constant-grade centerline from the pit's front edge down to its floor.

The "Quarry rim" diorama carries its own ground, a 64x64 relief grid (uhd_quarry_pit_ground.rs). The dirt
ramp the game cuts into that ground follows the centerline baked here once, not searched at every load.
Run it from the repository root (about a second, pure Python):

    python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_quarry_pit_ground.rs \
        src/world/pillars/quarry_ramp.rs [--grade 0.2] [--scale 110]

--grade is the constant slope of the road (0.2 drops 1 m per 5 m); --scale is the metres per model unit
of the quarry (the quarry's scale in src/world/sites.rs). It prints the path's length, its largest fill
and cut against the relief (metres) and how close its runs come to each other.

Search. Dijkstra over states (cell of a 32x32 grid spanning the footprint, heading of 8, distance travelled
in 2 m buckets) from the front edge's middle, heading -z. The road's level is its grade times the distance
(the exact one, which rides along with the cheapest way into a state; the buckets only merge states), so a
step costs more the farther that level lies from the (smoothed) relief: per metre, a fill costs its square
over 25, a cut adds its square over 9 (cuts need banks), and each 45 degree turn costs 20. It ends where
the relief is within FLOOR_MARGIN_M of the pit's floor and the level meets it. The cell path is then
rounded with two Chaikin passes and resampled every ~3 m.

Output. quarry_ramp.rs holds one `&[Vec2]` literal (x, z in model units, the first point on the front
edge's middle), which sites.rs includes as a constant and moves with the quarry's pose.
"""

import argparse
import heapq
import math
import re
from dataclasses import dataclass
from pathlib import Path

ENTRANCE = (0.0, 0.41655)  # the front edge's middle, where the quarry's drive arrives: level 0

GRID = 32  # search cells across the footprint, spanning +-HALF_X and +-HALF_Z (model units)
HALF_X, HALF_Z = 0.45, 0.38
HEADINGS = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]  # 45 degrees apart
START_HEADING = 6  # -z, into the quarry
BUCKET_M = 2.0  # a state also holds the distance travelled, in buckets of this many metres
MAX_LENGTH_M = 360.0
TURN_COST = 20.0
FILL_STIFFNESS_M, CUT_STIFFNESS_M = 5.0, 3.0  # a fill or cut of this many metres costs 1 per metre travelled
SMOOTH_RADIUS = 0.02  # the relief is a 3x3 mean at this distance (model units): the road ignores clefts
FLOOR_MARGIN_M = 4.0  # how near the pit's floor the road may end
LEVEL_TOLERANCE_M = 1.5  # how near the road's level must be to the ground there
CHAIKIN_PASSES = 2
SAMPLE_M = 3.0  # spacing of the baked points
KEEP_APART_ALONG_M = 40.0  # runs this far apart along the path count as separate runs


@dataclass
class Relief:
    """The ground grid of a diorama in model units: `heights[row][col]`, rows along z."""

    min: tuple[float, float]
    cell: tuple[float, float]
    heights: list[list[float]]

    def height(self, x, z):
        """Bilinear height, the way `Relief::height` in src/world/sites.rs reads it (clamped to the grid)."""
        rows, cols = len(self.heights), len(self.heights[0])
        gx = min(max((x - self.min[0]) / self.cell[0] - 0.5, 0.0), cols - 1.0)
        gz = min(max((z - self.min[1]) / self.cell[1] - 0.5, 0.0), rows - 1.0)
        col, row = min(int(gx), cols - 2), min(int(gz), rows - 2)
        fx, fz = gx - col, gz - row
        h = self.heights
        near = h[row][col] + (h[row][col + 1] - h[row][col]) * fx
        far = h[row + 1][col] + (h[row + 1][col + 1] - h[row + 1][col]) * fx
        return near + (far - near) * fz


def parse_relief(text):
    """Read `Relief { min, cell, heights }` from the generated Rust file."""
    floats = r"(-?\d+\.\d+)"
    pair = lambda name: tuple(map(float, re.search(rf"{name}: Vec2::new\({floats}, {floats}\)", text).groups()))
    flat = [float(v) for v in re.findall(floats, text[text.index("heights:") :])]
    size = math.isqrt(len(flat))
    if size * size != len(flat):
        raise ValueError(f"{len(flat)} heights do not form a square grid")
    return Relief(pair("min"), pair("cell"), [flat[row * size : (row + 1) * size] for row in range(size)])


class Ground:
    """A relief in metres, measured from the entrance's ground (level 0)."""

    def __init__(self, relief, scale):
        self.relief = relief
        self.scale = scale
        self.datum = relief.height(*ENTRANCE) * scale
        self.floor_m = min(min(row) for row in relief.heights) * scale - self.datum

    def smoothed_m(self, x, z):
        """The relief the road follows: a 3x3 mean around the point, in metres above the entrance."""
        taps = [(x + dx * SMOOTH_RADIUS, z + dz * SMOOTH_RADIUS) for dx in (-1, 0, 1) for dz in (-1, 0, 1)]
        return sum(self.relief.height(*tap) for tap in taps) / 9 * self.scale - self.datum


def cell_point(i, j):
    """The model-space point of a search cell."""
    return (-HALF_X + 2 * HALF_X * i / (GRID - 1), -HALF_Z + 2 * HALF_Z * j / (GRID - 1))


def find_cells(ground, grade):
    """The cheapest cell path from the front edge down to the floor, as a list of (i, j) cells."""
    reliefs = [[ground.smoothed_m(*cell_point(i, j)) for j in range(GRID)] for i in range(GRID)]
    cell_m = (2 * HALF_X / (GRID - 1) * ground.scale, 2 * HALF_Z / (GRID - 1) * ground.scale)
    goal_relief = ground.floor_m + FLOOR_MARGIN_M
    start_cell = (GRID // 2, GRID - 1)
    start = (*start_cell, START_HEADING, 0)  # (i, j, heading, bucket)
    metres = {start: math.dist(cell_point(*start_cell), ENTRANCE) * ground.scale}  # exact distance of each state

    def is_goal(state):
        i, j = state[:2]
        return reliefs[i][j] < goal_relief and abs(-grade * metres[state] - reliefs[i][j]) < LEVEL_TOLERANCE_M

    def moves(state):
        i, j, heading, _ = state
        for turn in (-1, 0, 1):
            heading_next = (heading + turn) % len(HEADINGS)
            dx, dz = HEADINGS[heading_next]
            i_next, j_next = i + dx, j + dz
            if not (0 <= i_next < GRID and 0 <= j_next < GRID):
                continue
            step_m = math.hypot(dx * cell_m[0], dz * cell_m[1])
            metres_next = metres[state] + step_m
            dev = -grade * metres_next - reliefs[i_next][j_next]  # > 0 fill, < 0 cut
            penalty = dev * dev / FILL_STIFFNESS_M**2 + (dev * dev / CUT_STIFFNESS_M**2 if dev < 0 else 0.0)
            following = (i_next, j_next, heading_next, round(metres_next / BUCKET_M))
            yield following, metres_next, step_m * penalty + TURN_COST * abs(turn)

    cost, previous, queue = {start: 0.0}, {}, [(0.0, start)]
    while queue:
        spent, state = heapq.heappop(queue)
        if spent > cost[state]:
            continue
        if is_goal(state):
            return cells_to(previous, state)
        if metres[state] > MAX_LENGTH_M:
            continue
        for following, metres_next, step_cost in moves(state):
            if spent + step_cost < cost.get(following, math.inf):
                cost[following], metres[following], previous[following] = spent + step_cost, metres_next, state
                heapq.heappush(queue, (cost[following], following))
    raise RuntimeError("no path from the front edge reaches the floor")


def cells_to(previous, state):
    """Walk the search's back-pointers to the cells from the start to `state`."""
    cells = []
    while state:
        cells.append(state[:2])
        state = previous.get(state)
    return cells[::-1]


def chaikin(points, passes):
    """Round the corners of a polyline by cutting each segment at its quarter points (the ends stay)."""
    for _ in range(passes):
        rounded = [points[0]]
        for (ax, az), (bx, bz) in zip(points, points[1:]):
            rounded.append((0.75 * ax + 0.25 * bx, 0.75 * az + 0.25 * bz))
            rounded.append((0.25 * ax + 0.75 * bx, 0.25 * az + 0.75 * bz))
        rounded.append(points[-1])
        points = rounded
    return points


def arc_lengths(points, scale):
    """The distance in metres from the first point to each point along the polyline."""
    lengths = [0.0]
    for a, b in zip(points, points[1:]):
        lengths.append(lengths[-1] + math.dist(a, b) * scale)
    return lengths


def resample(points, scale, spacing_m):
    """Points at equal distances along the polyline, about `spacing_m` apart, both ends kept."""
    lengths = arc_lengths(points, scale)
    count = max(1, round(lengths[-1] / spacing_m))
    sampled, segment = [points[0]], 0
    for k in range(1, count):
        target = lengths[-1] * k / count
        while lengths[segment + 1] < target:
            segment += 1
        t = (target - lengths[segment]) / (lengths[segment + 1] - lengths[segment])
        (ax, az), (bx, bz) = points[segment], points[segment + 1]
        sampled.append((ax + (bx - ax) * t, az + (bz - az) * t))
    sampled.append(points[-1])
    return sampled


def plan_path(ground, grade):
    """The ramp's centerline in model units: the entrance, then the rounded, resampled search result."""
    corners = [ENTRANCE] + [cell_point(i, j) for i, j in find_cells(ground, grade)]
    return resample(chaikin(corners, CHAIKIN_PASSES), ground.scale, SAMPLE_M)


@dataclass
class Stats:
    length: float  # m
    max_fill: float  # m of road above the (smoothed) relief
    max_cut: float  # m of road below it
    closest_runs: float  # m in plan between runs that are far apart along the path


def measure(ground, points, grade):
    """How the road, at a constant grade from the entrance, sits against the relief."""
    along = arc_lengths(points, ground.scale)
    devs = [-grade * s - ground.smoothed_m(*p) for p, s in zip(points, along)]
    closest = min(
        (
            math.dist(points[i], points[j]) * ground.scale
            for i in range(len(points))
            for j in range(i)
            if along[i] - along[j] >= KEEP_APART_ALONG_M
        ),
        default=math.inf,
    )
    return Stats(along[-1], max(devs), -min(devs), closest)


def write_points(path, points, relief_name):
    """Write the points as a `&[Vec2]` literal that sites.rs can `include!`."""
    lines = [f"// Generated by tools/plan_quarry_ramp.py from {relief_name}; do not edit.", "&["]
    lines += [f"    Vec2::new({x:.5f}, {z:.5f})," for x, z in points]
    lines.append("]")
    Path(path).write_text("\n".join(lines) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("relief", type=Path, help="the quarry's relief file (uhd_quarry_pit_ground.rs)")
    parser.add_argument("output", type=Path, help="the Rust file to write (quarry_ramp.rs)")
    parser.add_argument("--grade", type=float, default=0.2, help="the road's constant slope")
    parser.add_argument("--scale", type=float, default=110.0, help="metres per model unit")
    args = parser.parse_args()

    ground = Ground(parse_relief(args.relief.read_text()), args.scale)
    points = plan_path(ground, args.grade)
    write_points(args.output, points, args.relief.name)
    stats = measure(ground, points, args.grade)
    print(
        f"{len(points)} points, length {stats.length:.0f} m, max fill {stats.max_fill:.1f} m, "
        f"max cut {stats.max_cut:.1f} m, closest runs {stats.closest_runs:.1f} m"
    )


if __name__ == "__main__":
    main()
