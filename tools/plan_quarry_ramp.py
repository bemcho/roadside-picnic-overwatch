"""Plan the quarry's haul road: a centerline of one steady grade from the pit's front edge down to its floor.

The "Quarry rim" diorama carries its own ground, a 64x64 relief grid (uhd_quarry_pit_ground.rs). The dirt
ramp the game cuts into that ground follows the centerline baked here once, not searched at every load.
Run it from the repository root (about a second, pure Python):

    python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_quarry_pit_ground.rs \
        src/world/pillars/quarry_ramp.rs [--grade 0.2] [--scale 110]

--grade is the steepest slope the road may have (0.2 drops 1 m per 5 m); --scale is the metres per model
unit of the quarry (the quarry's scale in src/world/sites.rs). It prints the path's length and grade, its
largest fill and cut (metres, against the smoothed relief the search follows and, in brackets, the relief as
it is) and how close its runs come to each other. It refuses to write a road steeper than --grade.

The road is measured as the game builds it: graded evenly by distance from the drive's level at the
entrance to the ground's level at the road's end, so its grade is the ground's drop over its length.

Search. Dijkstra over states (cell of a 32x32 grid spanning the footprint, heading of 8, distance travelled
in 2 m buckets, steps since the last turn) from the front edge's middle, heading -z. The road's level is its
grade times the distance (the exact one, which rides along with the cheapest way into a state; the buckets
only merge states), so a step costs more the farther that level lies from the (smoothed) relief: per metre,
a fill costs its square over 9, a cut twice that (cuts need banks), and each turn of one heading costs 20. A
turn needs MIN_STRAIGHT_STEPS steps since the last, so the arcs of two corners fit between them. It ends where
the relief is within FLOOR_MARGIN_M of the pit's floor and the level lies END_BELOW_GROUND_M under it, so the
road as built, graded up to that ground, comes out no steeper than the grade.

Corners. The cell path's corners are replaced by circular arcs of TURN_RADIUS_M, tangent to both segments
(the road is 9 m wide, so it must not pivot), and the result is resampled every ~3 m. The tool refuses to
write a road whose baked points turn tighter than MIN_TURN_RADIUS_M (the radius of the circle through each
three consecutive points).

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
DEFAULT_GRADE = 0.2  # the steepest the road may be, end to end
DEFAULT_SCALE = 110.0  # metres per model unit

GRID = 32  # search cells across the footprint, spanning +-HALF_X and +-HALF_Z (model units)
HALF_X, HALF_Z = 0.45, 0.38
HEADINGS = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]  # 45 degrees apart
START_HEADING = 6  # -z, into the quarry
BUCKET_M = 2.0  # a state also holds the distance travelled, in buckets of this many metres
MAX_LENGTH_M = 360.0
TURN_COST = 20.0
DEVIATION_M = 3.0  # a fill this many metres deep costs 1 per metre travelled
CUT_WEIGHT = 2.0  # a cut costs this many times a fill of the same depth
SMOOTH_RADIUS = 0.02  # the relief is a 3x3 mean at this distance (model units): the road ignores clefts
FLOOR_MARGIN_M = 4.0  # how near the pit's floor the road may end
# The search ends with its level this far (min, max metres) below the ground there. Graded up to that ground
# the road is then flatter than the search's grade, which absorbs the arcs at the corners (they shorten the
# road by about 2%, 1 m of level) so it cannot come out steeper.
END_BELOW_GROUND_M = (1.2, 1.8)
ARC_STEP_M = 0.5  # the arcs are laid down as points this far apart, then resampled with the rest
MIN_STRAIGHT_STEPS = 4  # a turn needs this many steps (11 m or more) since the last, room for two arcs of
# the sharpest corner (50 degrees: the cells are not square, so a "45 degree" turn is 40 or 50)
MIN_TURN_RADIUS_M = 9.0  # the road never turns tighter than this
TURN_RADIUS_M = 9.5  # the arcs at the corners, a little wider than needed so the baked points stay above it
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

    def height_m(self, x, z):
        """The relief as it is, in metres above the entrance."""
        return self.relief.height(x, z) * self.scale - self.datum

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
    start = (*start_cell, START_HEADING, 0, 1)  # (i, j, heading, bucket, steps since the last turn)
    metres = {start: math.dist(cell_point(*start_cell), ENTRANCE) * ground.scale}  # exact distance of each state

    def is_goal(state):
        i, j = state[:2]
        below_ground = reliefs[i][j] + grade * metres[state]
        return reliefs[i][j] < goal_relief and END_BELOW_GROUND_M[0] <= below_ground <= END_BELOW_GROUND_M[1]

    def moves(state):
        i, j, heading, _, straight = state
        for turn in (-1, 0, 1):
            if turn and straight < MIN_STRAIGHT_STEPS:
                continue
            heading_next = (heading + turn) % len(HEADINGS)
            dx, dz = HEADINGS[heading_next]
            i_next, j_next = i + dx, j + dz
            if not (0 <= i_next < GRID and 0 <= j_next < GRID):
                continue
            step_m = math.hypot(dx * cell_m[0], dz * cell_m[1])
            metres_next = metres[state] + step_m
            dev = -grade * metres_next - reliefs[i_next][j_next]  # > 0 fill, < 0 cut
            penalty = (dev / DEVIATION_M) ** 2 * (CUT_WEIGHT if dev < 0 else 1.0)
            straight_next = 1 if turn else min(straight + 1, MIN_STRAIGHT_STEPS)
            following = (i_next, j_next, heading_next, round(metres_next / BUCKET_M), straight_next)
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


def corner_points(points):
    """The polyline without the points that lie on a straight line between their neighbours."""
    kept = [points[0]]
    for (ax, az), (bx, bz), (cx, cz) in zip(points, points[1:], points[2:]):
        if abs((bx - ax) * (cz - bz) - (bz - az) * (cx - bx)) > 1e-9:
            kept.append((bx, bz))
    kept.append(points[-1])
    return kept


def round_corners(points, scale, radius_m):
    """Replace each corner of the polyline by a circular arc of `radius_m` tangent to both its segments (a
    smaller one where the segments are too short to hold it), so the road never turns tighter than that."""
    metres = [(x * scale, z * scale) for x, z in points]
    rounded = [metres[0]]
    taken = 0.0  # how much of the segment the arc before this corner already uses
    for (ax, az), (bx, bz), (cx, cz) in zip(metres, metres[1:], metres[2:]):
        len_in, len_out = math.dist((ax, az), (bx, bz)), math.dist((bx, bz), (cx, cz))
        din, dout = ((bx - ax) / len_in, (bz - az) / len_in), ((cx - bx) / len_out, (cz - bz) / len_out)
        turn = math.atan2(din[0] * dout[1] - din[1] * dout[0], din[0] * dout[0] + din[1] * dout[1])  # + is left
        tangent = min(radius_m * math.tan(abs(turn) / 2), len_in - taken, len_out)
        radius = tangent / math.tan(abs(turn) / 2)
        taken = tangent
        start = (bx - din[0] * tangent, bz - din[1] * tangent)
        side = math.copysign(1.0, turn)
        centre = (start[0] - din[1] * radius * side, start[1] + din[0] * radius * side)
        steps = math.ceil(abs(turn) * radius / ARC_STEP_M)
        for k in range(steps + 1):
            angle = turn * k / steps
            rx, rz = start[0] - centre[0], start[1] - centre[1]
            rounded.append(
                (
                    centre[0] + rx * math.cos(angle) - rz * math.sin(angle),
                    centre[1] + rx * math.sin(angle) + rz * math.cos(angle),
                )
            )
    rounded.append(metres[-1])
    return [(x / scale, z / scale) for x, z in rounded]


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
    """The ramp's centerline in model units: the entrance, then the search result with its corners rounded
    into arcs, resampled."""
    cells = [ENTRANCE] + [cell_point(i, j) for i, j in find_cells(ground, grade)]
    return resample(round_corners(corner_points(cells), ground.scale, TURN_RADIUS_M), ground.scale, SAMPLE_M)


def turn_radii(points, scale):
    """The radius in metres of the circle through each three consecutive points (inf where they are in line)."""
    radii = []
    for a, b, c in zip(points, points[1:], points[2:]):
        sides = math.dist(a, b) * scale, math.dist(b, c) * scale, math.dist(a, c) * scale
        double_area = abs((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) * scale**2
        radii.append(math.inf if double_area < 1e-9 else sides[0] * sides[1] * sides[2] / (2 * double_area))
    return radii


@dataclass
class Stats:
    length: float  # m
    grade: float  # the road's constant slope from the entrance down to the ground at its end
    max_fill: float  # m of road above the smoothed relief
    max_cut: float  # m of road below it
    max_fill_raw: float  # the same against the relief as it is, clefts and all
    max_cut_raw: float
    closest_runs: float  # m in plan between runs that are far apart along the path
    min_radius: float  # m, the tightest turn


def measure(ground, points):
    """How the road sits against the relief, graded as the game builds it: evenly by distance from the
    entrance's level to the ground's level at the end."""
    along = arc_lengths(points, ground.scale)
    grade = -ground.height_m(*points[-1]) / along[-1]
    levels = [-grade * s for s in along]
    smooth = [level - ground.smoothed_m(*p) for level, p in zip(levels, points)]  # > 0 fill, < 0 cut
    raw = [level - ground.height_m(*p) for level, p in zip(levels, points)]
    closest = min(
        (
            math.dist(points[i], points[j]) * ground.scale
            for i in range(len(points))
            for j in range(i)
            if along[i] - along[j] >= KEEP_APART_ALONG_M
        ),
        default=math.inf,
    )
    tightest = min(turn_radii(points, ground.scale))
    return Stats(along[-1], grade, max(smooth), -min(smooth), max(raw), -min(raw), closest, tightest)


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
    parser.add_argument("--grade", type=float, default=DEFAULT_GRADE, help="the steepest slope of the road")
    parser.add_argument("--scale", type=float, default=DEFAULT_SCALE, help="metres per model unit")
    args = parser.parse_args()

    ground = Ground(parse_relief(args.relief.read_text()), args.scale)
    points = plan_path(ground, args.grade)
    stats = measure(ground, points)
    if stats.grade > args.grade:
        raise SystemExit(f"the planned road is steeper than asked: {stats.grade:.3f} > {args.grade}")
    if stats.min_radius < MIN_TURN_RADIUS_M:
        raise SystemExit(f"the planned road turns too tightly: {stats.min_radius:.1f} m < {MIN_TURN_RADIUS_M} m")
    write_points(args.output, points, args.relief.name)
    print(
        f"{len(points)} points, length {stats.length:.0f} m, grade {stats.grade:.3f}, "
        f"max fill {stats.max_fill:.1f} m (raw {stats.max_fill_raw:.1f}), "
        f"max cut {stats.max_cut:.1f} m (raw {stats.max_cut_raw:.1f}), closest runs {stats.closest_runs:.1f} m, "
        f"tightest turn {stats.min_radius:.1f} m"
    )


if __name__ == "__main__":
    main()
