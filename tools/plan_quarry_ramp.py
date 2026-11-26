"""Plan a haul road: a centerline of one steady grade from a pit's rim down to its floor, or to a given point.

The "Quarry rim" diorama carries its own ground, a 64x64 relief grid (uhd_quarry_pit_ground.rs). The dirt
ramp the game cuts into that ground follows the centerline baked here once, not searched at every load. The
Golden Sphere's pit has no scan: its land is exported once from the terrain as a relief in metres, and the
same search plans its road with the options below. Run it from the repository root (pure Python: a fraction of
a second at the quarry or on a 64-cell grid over a 340 x 460 m pit, a few seconds when no road exists).

The quarry (the defaults of every option below):

    python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_quarry_pit_ground.rs \
        src/world/pillars/quarry_ramp.rs [--grade 0.2] [--scale 110]

Another pit, from a relief in metres, to a given point, round the footprints of other sites:

    python3 tools/plan_quarry_ramp.py RELIEF.rs OUTPUT.rs --scale 1 --grade 0.15 --centre=-1090,-100 \
        --half=170,230 --grid 64 --entrance=-1190,-60 --heading 0 --end=-1150,-40 --max-length 450 \
        --avoid=-1095,97,-1003,205 [--avoid ...]

Options. --grade is the steepest slope the road may have (0.2 drops 1 m per 5 m); --scale is the metres per
model unit of the relief (the quarry's scale in src/world/sites.rs; 1 for a relief in metres). The others
are in model units, the relief's own, and a value that starts with a minus sign needs the `=` form
(--end=-5,2), or argparse reads it as another option.
  --entrance  where the road starts, at level 0.
  --heading   the way it sets out, an index into HEADINGS: 0 is +x, 2 is +z, 4 is -x, 6 is -z, the diagonals
              between.
  --centre, --half, --grid
              the search runs over a grid of cells, --grid across, spanning the box --centre +- --half.
  --end       the road ends at this point (it must lie in the box) instead of on the pit's floor.
  --avoid XMIN,ZMIN,XMAX,ZMAX
              a box that no point of the road may enter, nor come within --shoulder metres of. Repeatable.
  --shoulder  that clearance, 9 m: the road's shoulder (RAMP_SHOULDER in src/world/roads.rs).
  --max-length  the longest road the search follows, in metres.

Where the road starts and ends. A kink at the road's ends would be tighter than the road may turn, so the
grid is made to fit them: it moves by under a cell to put a node exactly on --end, and an --entrance inside
the box snaps to its nearest node (so the first point is that node, under a cell from the entrance). The
quarry's entrance lies outside its box, 4 m from the front edge: it runs in to the nearest node as it is. An
entrance just outside the box may therefore be refused for a kink: put it inside.

It prints the path's length and grade, its largest fill and cut (metres, against the smoothed relief the
search follows and, in brackets, the relief as it is), how close its runs come to each other and, with
--avoid, how close it comes to a box. It refuses to write a road steeper than --grade, turning tighter than
MIN_TURN_RADIUS_M or closer to a box than --shoulder. A run with options other than the defaults records them
and its numbers in the output's header.

The road is measured as the game builds it: graded evenly by distance from the drive's level at the
entrance to the ground's level at the road's end, so its grade is the ground's drop over its length.

Search. Dijkstra over states (cell of the grid, heading of 8, distance travelled in 2 m buckets, steps since
the last turn) from the start. The road's level is its grade times the distance (the exact one, which rides
along with the cheapest way into a state; the buckets only merge states), so a step costs more the farther
that level lies from the (smoothed) relief: per metre, a fill costs its square over 9, a cut twice that (cuts
need banks), and each turn of one heading costs 20. A turn needs MIN_STRAIGHT_STEPS steps since the last, so
the arcs of two corners fit between them. It ends where the relief is within FLOOR_MARGIN_M of the pit's floor
(with --end: at the end point's node) and the level lies END_BELOW_GROUND_M under the ground there, so the
road as built, graded up to that ground, comes out no steeper than the grade. A step whose line comes within
the shoulder (plus AVOID_MARGIN_M, for the corners' arcs) of an --avoid box is not taken.

Corners. The cell path's corners are replaced by circular arcs of TURN_RADIUS_M, tangent to both segments
(the road is 9 m wide, so it must not pivot), and the result is resampled every ~3 m. The tool refuses to
write a road whose baked points turn tighter than MIN_TURN_RADIUS_M (the radius of the circle through each
three consecutive points).

Output. The output holds one `&[Vec2]` literal (x, z in model units, the first point the start), which the
game includes as a constant (sites.rs moves the quarry's with the quarry's pose).
"""

import argparse
import functools
import heapq
import math
import re
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path

ENTRANCE = (0.0, 0.41655)  # the front edge's middle, where the quarry's drive arrives: level 0
DEFAULT_GRADE = 0.2  # the steepest the road may be, end to end
DEFAULT_SCALE = 110.0  # metres per model unit

# The quarry's search, the defaults of a `Site`.
GRID = 32  # search cells across the footprint, spanning +-HALF_X and +-HALF_Z (model units)
HALF_X, HALF_Z = 0.45, 0.38
HEADINGS = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]  # 45 degrees apart
START_HEADING = 6  # -z, into the quarry
MAX_LENGTH_M = 360.0
DEFAULT_SHOULDER_M = 9.0  # RAMP_SHOULDER in src/world/roads.rs: the road keeps this far from an --avoid box
BUCKET_M = 2.0  # a state also holds the distance travelled, in buckets of this many metres
TURN_COST = 20.0
DEVIATION_M = 3.0  # a fill this many metres deep costs 1 per metre travelled
CUT_WEIGHT = 2.0  # a cut costs this many times a fill of the same depth
SMOOTH_RADIUS_M = 2.2  # the relief is a 3x3 mean at this distance: the road ignores clefts
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
# The search keeps this far beyond the shoulder from an --avoid box: the arcs at the corners cut up to 1 m inside
# the cell path (a 50 degree corner of TURN_RADIUS_M), the baked chords 0.1 m, and the steps are checked every
# ARC_STEP_M, which can miss 0.25 m.
AVOID_MARGIN_M = 1.5


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


@dataclass(frozen=True)
class Site:
    """Where a road is searched, in the relief's model units (the quarry's by default)."""

    entrance: tuple[float, float] = ENTRANCE  # where the road starts, at level 0
    centre: tuple[float, float] = (0.0, 0.0)  # of the search box
    half: tuple[float, float] = (HALF_X, HALF_Z)  # half its size
    grid: int = GRID  # search cells across each side of the box
    heading: int = START_HEADING  # index into HEADINGS: the way the road sets out
    max_length_m: float = MAX_LENGTH_M
    end: tuple[float, float] | None = None  # the road ends here; None: on the pit's floor
    avoid: tuple[tuple[float, float, float, float], ...] = ()  # boxes (xmin, zmin, xmax, zmax) to keep clear of
    shoulder_m: float = DEFAULT_SHOULDER_M  # how clear, in metres

    @functools.cached_property
    def low(self):
        """The grid's lower corner: the box's, moved by under a cell to put a node exactly on the end point."""
        low = tuple(centre - half for centre, half in zip(self.centre, self.half))
        if self.end:
            cell = self.nearest(self.end, low)
            node = [lo + 2 * half * k / (self.grid - 1) for lo, half, k in zip(low, self.half, cell)]
            low = tuple(lo + end - n for lo, end, n in zip(low, self.end, node))
        return low

    def node(self, i, j):
        """The model-space point of a search cell."""
        return tuple(lo + 2 * half * k / (self.grid - 1) for lo, half, k in zip(self.low, self.half, (i, j)))

    def nearest(self, point, low=None):
        """The search cell nearest a point, clamped into the grid (a point midway between two cells takes the higher)."""
        cell = [
            math.floor((p - lo) / (2 * half) * (self.grid - 1) + 0.5)
            for p, lo, half in zip(point, self.low if low is None else low, self.half)
        ]
        return tuple(min(max(k, 0), self.grid - 1) for k in cell)

    @functools.cached_property
    def start(self):
        """Where the road starts: the entrance, or, if that lies inside the box, the node nearest it, so the first
        leg runs straight along the heading (a kink at the start would be tighter than the road may turn)."""
        inside = all(lo <= p <= lo + 2 * half for p, lo, half in zip(self.entrance, self.low, self.half))
        return self.node(*self.nearest(self.entrance)) if inside else self.entrance


class Ground:
    """A relief in metres, measured from the entrance's ground (level 0)."""

    def __init__(self, relief, scale, entrance=ENTRANCE):
        self.relief = relief
        self.scale = scale
        self.smooth_radius = SMOOTH_RADIUS_M / scale
        self.datum = relief.height(*entrance) * scale
        self.floor_m = min(min(row) for row in relief.heights) * scale - self.datum

    def height_m(self, x, z):
        """The relief as it is, in metres above the entrance."""
        return self.relief.height(x, z) * self.scale - self.datum

    def smoothed_m(self, x, z):
        """The relief the road follows: a 3x3 mean around the point, in metres above the entrance."""
        taps = [(x + dx * self.smooth_radius, z + dz * self.smooth_radius) for dx in (-1, 0, 1) for dz in (-1, 0, 1)]
        return sum(self.relief.height(*tap) for tap in taps) / 9 * self.scale - self.datum


def box_gap(point, box):
    """The distance from a point to an axis-aligned box (xmin, zmin, xmax, zmax): 0 inside it."""
    return math.hypot(max(box[0] - point[0], 0.0, point[0] - box[2]), max(box[1] - point[1], 0.0, point[1] - box[3]))


def clearance_m(points, boxes, scale):
    """The distance in metres from the points to the nearest of the boxes (inf without boxes)."""
    return min((box_gap(point, box) * scale for point in points for box in boxes), default=math.inf)


def find_cells(ground, grade, site=Site()):
    """The cheapest cell path from the entrance down to the floor (or to `site.end`), as a list of (i, j) cells."""
    grid, scale = site.grid, ground.scale
    reliefs = [[ground.smoothed_m(*site.node(i, j)) for j in range(grid)] for i in range(grid)]
    cell_m = (2 * site.half[0] / (grid - 1) * scale, 2 * site.half[1] / (grid - 1) * scale)
    start_cell = site.nearest(site.start)
    start = (*start_cell, site.heading, 0, 1)  # (i, j, heading, bucket, steps since the last turn)
    metres = {start: math.dist(site.node(*start_cell), site.start) * scale}  # exact distance of each state

    if site.end:
        end_cell = site.nearest(site.end)
        at_target = lambda i, j: (i, j) == end_cell
        # The level rule below pins the road's length at the end cell: no longer road can end there.
        longest_m = min(site.max_length_m, (END_BELOW_GROUND_M[1] - reliefs[end_cell[0]][end_cell[1]]) / grade)
    else:
        goal_relief = ground.floor_m + FLOOR_MARGIN_M
        at_target = lambda i, j: reliefs[i][j] < goal_relief
        longest_m = site.max_length_m

    def is_goal(state):
        i, j = state[:2]
        below_ground = reliefs[i][j] + grade * metres[state]
        return at_target(i, j) and END_BELOW_GROUND_M[0] <= below_ground <= END_BELOW_GROUND_M[1]

    @functools.lru_cache(maxsize=None)
    def step_is_clear(i, j, heading):
        """Whether the step from a cell keeps its whole line out of the avoid boxes' shoulders."""
        if not site.avoid:
            return True
        (ax, az), (bx, bz) = site.node(i, j), site.node(i + HEADINGS[heading][0], j + HEADINGS[heading][1])
        count = math.ceil(math.dist((ax, az), (bx, bz)) * scale / ARC_STEP_M)
        line = [(ax + (bx - ax) * k / count, az + (bz - az) * k / count) for k in range(count + 1)]
        return clearance_m(line, site.avoid, scale) >= site.shoulder_m + AVOID_MARGIN_M

    def moves(state):
        i, j, heading, _, straight = state
        for turn in (-1, 0, 1):
            if turn and straight < MIN_STRAIGHT_STEPS:
                continue
            heading_next = (heading + turn) % len(HEADINGS)
            dx, dz = HEADINGS[heading_next]
            i_next, j_next = i + dx, j + dz
            if not (0 <= i_next < grid and 0 <= j_next < grid) or not step_is_clear(i, j, heading_next):
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
        if metres[state] > longest_m:
            continue
        for following, metres_next, step_cost in moves(state):
            if spent + step_cost < cost.get(following, math.inf):
                cost[following], metres[following], previous[following] = spent + step_cost, metres_next, state
                heapq.heappush(queue, (cost[following], following))
    raise RuntimeError(f"no path from the entrance reaches the {'end point' if site.end else 'floor'}")


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


def plan_path(ground, grade, site=Site()):
    """The road's centerline in model units: the start, then the search result with its corners rounded into
    arcs, resampled. With `site.end` it ends exactly there."""
    cells = [site.start] + [site.node(i, j) for i, j in find_cells(ground, grade, site)]
    path = resample(round_corners(corner_points(cells), ground.scale, TURN_RADIUS_M), ground.scale, SAMPLE_M)
    if site.end:
        path[-1] = site.end  # the grid has a node there, but its arithmetic may differ in the last digit
    return path


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
    clearance: float  # m, from the nearest avoid box to the closest point of the road (inf without boxes)


def measure(ground, points, site=Site()):
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
    clearance = clearance_m(points, site.avoid, ground.scale)
    return Stats(along[-1], grade, max(smooth), -min(smooth), max(raw), -min(raw), closest, tightest, clearance)


def write_points(path, points, relief_name, notes=()):
    """Write the points as a `&[Vec2]` literal that sites.rs can `include!`, under a header and any notes."""
    lines = [f"// Generated by tools/plan_quarry_ramp.py from {relief_name}; do not edit."]
    lines += [f"// {note}" for note in notes]
    lines.append("&[")
    lines += [f"    Vec2::new({x:.5f}, {z:.5f})," for x, z in points]
    lines.append("]")
    Path(path).write_text("\n".join(lines) + "\n")


def describe(stats):
    """The one-line summary of a planned road."""
    text = (
        f"length {stats.length:.0f} m, grade {stats.grade:.3f}, "
        f"max fill {stats.max_fill:.1f} m (raw {stats.max_fill_raw:.1f}), "
        f"max cut {stats.max_cut:.1f} m (raw {stats.max_cut_raw:.1f}), closest runs {stats.closest_runs:.1f} m, "
        f"tightest turn {stats.min_radius:.1f} m"
    )
    return text + (f", clearance {stats.clearance:.1f} m" if stats.clearance < math.inf else "")


def xz(text):
    """`X,Z` as two floats."""
    x, z = map(float, text.split(","))
    return x, z


def box(text):
    """`XMIN,ZMIN,XMAX,ZMAX` as four floats."""
    xmin, zmin, xmax, zmax = map(float, text.split(","))
    return xmin, zmin, xmax, zmax


def parse_args(argv=None):
    quarry = Site()
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("relief", type=Path, help="the relief file (the quarry's is uhd_quarry_pit_ground.rs)")
    parser.add_argument("output", type=Path, help="the Rust file to write (the quarry's is quarry_ramp.rs)")
    parser.add_argument("--grade", type=float, default=DEFAULT_GRADE, help="the steepest slope of the road")
    parser.add_argument("--scale", type=float, default=DEFAULT_SCALE, help="metres per model unit")
    parser.add_argument("--entrance", type=xz, default=quarry.entrance, metavar="X,Z", help="where the road starts")
    parser.add_argument("--centre", type=xz, default=quarry.centre, metavar="X,Z", help="of the search box")
    parser.add_argument("--half", type=xz, default=quarry.half, metavar="X,Z", help="half the size of the search box")
    parser.add_argument("--grid", type=int, default=quarry.grid, metavar="N", help="search cells across the box")
    parser.add_argument(
        "--heading",
        type=int,
        choices=range(len(HEADINGS)),
        default=quarry.heading,
        metavar="H",
        help="the way the road sets out: an index into HEADINGS (0 is +x, 2 is +z, 4 is -x, 6 is -z)",
    )
    parser.add_argument("--end", type=xz, default=None, metavar="X,Z", help="end the road here, not on the floor")
    parser.add_argument(
        "--avoid",
        type=box,
        action="append",
        default=[],
        metavar="XMIN,ZMIN,XMAX,ZMAX",
        help="a box the road keeps --shoulder metres clear of (repeatable)",
    )
    parser.add_argument(
        "--max-length", type=float, default=quarry.max_length_m, metavar="M", help="longest road, metres"
    )
    parser.add_argument(
        "--shoulder", type=float, default=quarry.shoulder_m, help="clearance from --avoid boxes, metres"
    )
    args = parser.parse_args(argv)
    if args.end and any(abs(e - c) > h for e, c, h in zip(args.end, args.centre, args.half)):
        parser.error("--end lies outside the search box (--centre +- --half)")
    if any(xmin >= xmax or zmin >= zmax for xmin, zmin, xmax, zmax in args.avoid):
        parser.error("--avoid wants XMIN,ZMIN,XMAX,ZMAX with the minimum below the maximum")
    return args


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    args = parse_args(argv)
    site = Site(
        entrance=args.entrance,
        centre=args.centre,
        half=args.half,
        grid=args.grid,
        heading=args.heading,
        max_length_m=args.max_length,
        end=args.end,
        avoid=tuple(args.avoid),
        shoulder_m=args.shoulder,
    )

    ground = Ground(parse_relief(args.relief.read_text()), args.scale, site.start)
    try:
        points = plan_path(ground, args.grade, site)
    except RuntimeError as no_path:  # the search found no road: say so without a traceback
        raise SystemExit(str(no_path))
    stats = measure(ground, points, site)
    if stats.grade > args.grade:
        raise SystemExit(f"the planned road is steeper than asked: {stats.grade:.3f} > {args.grade}")
    if stats.min_radius < MIN_TURN_RADIUS_M:
        raise SystemExit(f"the planned road turns too tightly: {stats.min_radius:.1f} m < {MIN_TURN_RADIUS_M} m")
    if stats.clearance < site.shoulder_m:
        raise SystemExit(f"the planned road comes too near an avoid box: {stats.clearance:.1f} m < {site.shoulder_m} m")

    summary = f"{len(points)} points, {describe(stats)}"
    notes = []
    if (site, args.grade, args.scale) != (Site(), DEFAULT_GRADE, DEFAULT_SCALE):  # the quarry's own run needs none
        options = [arg for arg in argv if arg not in (str(args.relief), str(args.output))]
        notes = [f"Options: {shlex.join(options)}", f"Result: {summary}"]
    write_points(args.output, points, args.relief.name, notes)
    print(summary)


if __name__ == "__main__":
    main()
