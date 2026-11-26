"""Tests for plan_quarry_ramp.py's relief parsing, path search and output: on the quarry's real relief grid,
and on a synthetic conical pit in metres for the options a second site needs (end point, no-go boxes).

python3 -m unittest tools/test_plan_quarry_ramp.py -v
"""

import contextlib
import io
import math
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import plan_quarry_ramp as ramp_tool  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
RELIEF_FILE = REPO / "src/world/pillars/uhd_quarry_pit_ground.rs"
RAMP_FILE = REPO / "src/world/pillars/quarry_ramp.rs"
CORE_RELIEF_FILE = REPO / "src/world/pillars/uhd_core_ground.rs"
GRADE = ramp_tool.DEFAULT_GRADE
SCALE = ramp_tool.DEFAULT_SCALE
MIN_RUN_GAP_M = 18.0  # the bed and shoulder (9 m each side) of two runs must not overlap
MIN_TURN_RADIUS_M = 9.0  # a 9 m wide bed turning on less is a pivot, not a road


class RampTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.relief = ramp_tool.parse_relief(RELIEF_FILE.read_text())
        cls.ground = ramp_tool.Ground(cls.relief, SCALE)
        cls.points = ramp_tool.plan_path(cls.ground, GRADE)

    def test_parses_the_relief_grid(self):
        relief = self.relief
        self.assertEqual((len(relief.heights), {len(row) for row in relief.heights}), (64, {64}))
        self.assertEqual(relief.min, (-0.49051, -0.41655))
        self.assertEqual(relief.cell, (0.015331, 0.013017))
        self.assertEqual(relief.heights[0][:3], [0.1666, 0.1666, 0.1666])
        self.assertEqual(relief.heights[63][-1], -0.1755)

    def test_height_is_bilinear_and_clamped(self):
        relief = ramp_tool.Relief(min=(0.0, 0.0), cell=(1.0, 1.0), heights=[[0.0, 2.0], [4.0, 6.0]])
        self.assertAlmostEqual(relief.height(1.0, 1.0), 3.0)  # the middle of the four samples
        self.assertAlmostEqual(relief.height(0.5, 0.5), 0.0)  # a sample sits half a cell in
        self.assertAlmostEqual(relief.height(-9.0, 9.0), 4.0)  # clamped to the nearest corner

    def test_path_descends_to_the_floor(self):
        stats = ramp_tool.measure(self.ground, self.points)
        self.assertTrue(180 <= stats.length <= 320, stats.length)
        floor = min(min(row) for row in self.relief.heights) * SCALE - self.ground.datum
        self.assertLessEqual(self.ground.smoothed_m(*self.points[-1]) - floor, 4.0)

    def test_road_as_built_is_no_steeper_than_the_grade(self):
        # The game grades the ramp evenly between its end levels, so the ground's drop over the length is its grade.
        stats = ramp_tool.measure(self.ground, self.points)
        drop = -self.ground.height_m(*self.points[-1])
        self.assertAlmostEqual(stats.grade, drop / stats.length)
        self.assertLessEqual(stats.grade, GRADE)

    def test_path_stays_near_the_smoothed_relief(self):
        stats = ramp_tool.measure(self.ground, self.points)
        self.assertLessEqual(stats.max_fill, 8.0)
        self.assertLessEqual(stats.max_cut, 3.0)

    def test_path_stays_near_the_relief_as_it_is(self):
        stats = ramp_tool.measure(self.ground, self.points)
        self.assertLessEqual(stats.max_fill_raw, 8.0)
        self.assertLessEqual(stats.max_cut_raw, 4.5)

    def test_runs_keep_apart(self):
        along = ramp_tool.arc_lengths(self.points, SCALE)
        for i, a in enumerate(self.points):
            for j in range(i):
                if along[i] - along[j] >= ramp_tool.KEEP_APART_ALONG_M:
                    self.assertGreaterEqual(math.dist(a, self.points[j]) * SCALE, MIN_RUN_GAP_M, (j, i))

    def test_turns_are_wide_enough(self):
        self.assertGreaterEqual(min(ramp_tool.turn_radii(self.points, SCALE)), MIN_TURN_RADIUS_M)
        self.assertGreaterEqual(ramp_tool.measure(self.ground, self.points).min_radius, MIN_TURN_RADIUS_M)

    def test_turn_radii_read_a_circle(self):
        circle = [(10 * math.cos(a / 10) / SCALE, 10 * math.sin(a / 10) / SCALE) for a in range(8)]  # 10 m radius
        for radius in ramp_tool.turn_radii(circle, SCALE):
            self.assertAlmostEqual(radius, 10.0, places=6)
        self.assertEqual(ramp_tool.turn_radii([(0.0, 0.0), (0.1, 0.0), (0.2, 0.0)], SCALE), [math.inf])

    def test_a_corner_becomes_an_arc_of_the_asked_radius(self):
        corner = [(0.0, 0.0), (0.3, 0.0), (0.3, 0.3)]  # a right-angle turn, 33 m legs
        rounded = ramp_tool.resample(ramp_tool.round_corners(corner, SCALE, 12.0), SCALE, 1.0)
        self.assertEqual((rounded[0], rounded[-1]), (corner[0], corner[-1]))
        radii = ramp_tool.turn_radii(rounded, SCALE)
        self.assertAlmostEqual(min(radii), 12.0, delta=0.2)

    def test_writes_rust_points(self):
        with tempfile.TemporaryDirectory() as folder:
            out = Path(folder) / "quarry_ramp.rs"
            ramp_tool.write_points(out, self.points, RELIEF_FILE.name)
            lines = out.read_text().splitlines()
        self.assertEqual(
            lines[0], "// Generated by tools/plan_quarry_ramp.py from uhd_quarry_pit_ground.rs; do not edit."
        )
        self.assertEqual((lines[1], lines[-1]), ("&[", "]"))
        written = [
            tuple(map(float, m.groups()))
            for m in (re.fullmatch(r"\s+Vec2::new\((-?\d+\.\d+), (-?\d+\.\d+)\),", line) for line in lines[2:-1])
            if m
        ]
        self.assertEqual(len(written), len(lines) - 3)
        self.assertEqual(len(written), len(self.points))
        self.assertEqual(written[0], (0.0, 0.41655))
        for (wx, wz), (x, z) in zip(written, self.points):
            self.assertAlmostEqual(wx, x, places=5)
            self.assertAlmostEqual(wz, z, places=5)

    def test_quarry_ramp_regenerates_unchanged(self):
        with tempfile.TemporaryDirectory() as folder:
            out = Path(folder) / "quarry_ramp.rs"
            run_tool(RELIEF_FILE, out, "--grade", GRADE, "--scale", SCALE)
            self.assertEqual(out.read_text(), RAMP_FILE.read_text())

    def test_site_defaults_are_the_quarrys(self):
        site = ramp_tool.Site()
        self.assertEqual(
            (site.entrance, site.centre, site.half, site.grid, site.heading, site.max_length_m, site.end, site.avoid),
            (ramp_tool.ENTRANCE, (0.0, 0.0), (0.45, 0.38), 32, 6, 360.0, None, ()),
        )
        self.assertEqual((site.shoulder_m, site.turn, site.min_separation_m), (9.0, None, None))

    def test_refuses_steeper_than_grade(self):
        # The search itself keeps to the grade, so hand the tool a road that drops 48 m in 50 m.
        steep = [ramp_tool.ENTRANCE, (0.0, 0.41655 - 25 / SCALE), (0.0, 0.41655 - 50 / SCALE)]
        with tempfile.TemporaryDirectory() as folder, mock.patch.object(ramp_tool, "plan_path", return_value=steep):
            out = Path(folder) / "steep.rs"
            with self.assertRaisesRegex(SystemExit, "steeper"):
                run_tool(RELIEF_FILE, out, "--grade", GRADE, "--scale", SCALE)
            self.assertFalse(out.exists())


# A conical pit in metres, centred on the origin: ground at level 0 outside a 150 m radius, falling to a flat floor
# 40 m down inside 30 m. The road enters from the west at (-160, 0), heading +x, and ends on the floor.
CONE_SPAN_M, CONE_RIM_M, CONE_FLOOR_M, CONE_DEPTH_M = 340.0, 150.0, 30.0, 40.0
CONE_ENTRANCE, CONE_END, CONE_GRADE = (-160.0, 0.0), (12.0, -9.0), 0.15


def cone_relief(size=64, span=CONE_SPAN_M, rim=CONE_RIM_M, floor=CONE_FLOOR_M, depth=CONE_DEPTH_M):
    cell = span / size
    centres = [-span / 2 + (k + 0.5) * cell for k in range(size)]

    def height(x, z):
        down = (rim - math.hypot(x, z)) / (rim - floor)
        return -depth * min(1.0, max(0.0, down))

    heights = [[height(x, z) for x in centres] for z in centres]
    return ramp_tool.Relief((-span / 2,) * 2, (cell, cell), heights)


def relief_file_text(relief):
    """The relief in the generated files' format, the one `parse_relief` reads."""
    values = ", ".join(f"{h:.4f}" for row in relief.heights for h in row)
    return (
        f"Relief {{\n    min: Vec2::new({relief.min[0]:.4f}, {relief.min[1]:.4f}),\n"
        f"    cell: Vec2::new({relief.cell[0]:.6f}, {relief.cell[1]:.6f}),\n    heights: &[\n        {values}\n    ],\n}}\n"
    )


def run_tool(*args):
    """Run `main` with these command-line arguments; the text it prints."""
    with contextlib.redirect_stdout(io.StringIO()) as printed:
        ramp_tool.main([str(arg) for arg in args])
    return printed.getvalue()


def turn_sides(points):
    """The side each bend of a polyline turns to, as a driver sees it in the game (y up, seen from above): -1 for
    left (from +x towards -z), +1 for right, one per point where the polyline bends."""
    sides = []
    for a, b, c in zip(points, points[1:], points[2:]):
        cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
        if abs(cross) > 1e-9 * math.dist(a, b) * math.dist(b, c):
            sides.append(1 if cross > 0 else -1)
    return sides


def gap_to_box(point, box):
    """Distance in metres from a point to an axis-aligned box (xmin, zmin, xmax, zmax), 0 inside it."""
    return math.hypot(max(box[0] - point[0], 0.0, point[0] - box[2]), max(box[1] - point[1], 0.0, point[1] - box[3]))


class ConePitTests(unittest.TestCase):
    """The options a second site needs, on a pit whose floor is not the road's end by accident."""

    @classmethod
    def setUpClass(cls):
        cls.relief = cone_relief()

    def site(self, **options):
        defaults = dict(entrance=CONE_ENTRANCE, heading=0, half=(165.0, 165.0), grid=64, end=CONE_END)
        return ramp_tool.Site(**{**dict(max_length_m=450.0), **defaults, **options})

    def plan(self, **options):
        """The planned road and its numbers, for the site of these options."""
        site = self.site(**options)
        ground = ramp_tool.Ground(self.relief, 1.0, site.start)
        points = ramp_tool.plan_path(ground, CONE_GRADE, site)
        return points, ramp_tool.measure(ground, points, site)

    def test_a_node_lies_on_the_end_point_and_the_inside_entrance_snaps_to_a_node(self):
        site = self.site(entrance=(-157.3, 4.1), end=(31.7, 23.9))
        i, j = site.nearest(site.end)
        self.assertAlmostEqual(site.node(i, j)[0], 31.7)
        self.assertAlmostEqual(site.node(i, j)[1], 23.9)
        shift = [low - (-165.0) for low in site.low]
        self.assertTrue(all(abs(move) < 2 * 165.0 / 63 for move in shift), shift)
        self.assertEqual(site.start, site.node(*site.nearest(site.entrance)))
        self.assertTrue(all(abs(a - b) < 4.0 for a, b in zip(site.start, site.entrance)), site.start)
        self.assertEqual(ramp_tool.Site().start, ramp_tool.ENTRANCE)  # the quarry's entrance lies outside its box

    def test_ends_at_the_given_point(self):
        points, stats = self.plan()
        self.assertEqual(points[-1], CONE_END)
        self.assertLessEqual(stats.grade, CONE_GRADE)
        self.assertGreaterEqual(stats.min_radius, MIN_TURN_RADIUS_M)

    def test_ends_at_a_point_between_cells(self):
        for end in [(31.7, 23.9), (-8.3, -17.7), (0.4, 11.2)]:  # on the floor, off the search cells
            points, stats = self.plan(end=end)
            self.assertEqual(points[-1], end)
            self.assertLessEqual(stats.grade, CONE_GRADE, end)
            self.assertGreaterEqual(stats.min_radius, MIN_TURN_RADIUS_M, end)

    def test_keeps_out_of_avoid_boxes(self):
        box = (-110.0, -25.0, -70.0, 25.0)  # across the straight line from the entrance to the end
        free, _ = self.plan()
        self.assertLess(min(gap_to_box(p, box) for p in free), 9.0, "the box is not in the way")
        points, stats = self.plan(avoid=(box,))
        self.assertEqual(points[-1], CONE_END)
        for point in points:
            self.assertGreaterEqual(gap_to_box(point, box), 9.0, point)
        self.assertAlmostEqual(stats.clearance, min(gap_to_box(p, box) for p in points))
        self.assertGreaterEqual(stats.min_radius, MIN_TURN_RADIUS_M)

    def test_keeps_out_of_several_boxes_by_the_shoulder(self):
        boxes = ((-100.0, -10.0, -60.0, 30.0), (-40.0, -45.0, -10.0, -15.0))
        free, _ = self.plan()
        self.assertLess(min(gap_to_box(p, box) for p in free for box in boxes), 15.0, "the boxes are not in the way")
        points, stats = self.plan(avoid=boxes, shoulder_m=15.0)
        for box in boxes:
            self.assertGreaterEqual(min(gap_to_box(p, box) for p in points), 15.0)
        self.assertGreaterEqual(stats.clearance, 15.0)

    def test_runs_keep_apart_is_reported(self):
        points, stats = self.plan()
        along = ramp_tool.arc_lengths(points, 1.0)
        apart = [
            math.dist(a, points[j])
            for i, a in enumerate(points)
            for j in range(i)
            if along[i] - along[j] >= ramp_tool.KEEP_APART_ALONG_M
        ]
        self.assertTrue(apart, "the pit's road never doubles back on itself")
        self.assertAlmostEqual(stats.closest_runs, min(apart))
        ground = ramp_tool.Ground(self.relief, 1.0, CONE_ENTRANCE)
        short = [(0.0, 0.0), (0.0, 15.0), (0.0, 30.0)]  # no two points 40 m apart along it: no separate runs
        self.assertEqual(ramp_tool.measure(ground, short, self.site()).closest_runs, math.inf)

    def test_tool_prints_the_closest_runs_and_the_clearance(self):
        with tempfile.TemporaryDirectory() as folder:
            relief_file, out = Path(folder) / "cone.rs", Path(folder) / "ramp.rs"
            relief_file.write_text(relief_file_text(self.relief))
            printed = run_tool(
                relief_file, out, "--grade", CONE_GRADE, "--scale", 1, "--entrance=-160,0", "--heading", 0,
                "--half", "165,165", "--grid", 64, "--end", "12,-9", "--avoid=-110,-25,-70,25", "--max-length", 450,
            )  # fmt: skip
            written = out.read_text().splitlines()
        self.assertRegex(printed, r"closest runs \d+\.\d m")
        self.assertRegex(printed, r"clearance \d+\.\d m")
        self.assertEqual(written[-2], "    Vec2::new(12.00000, -9.00000),")
        # A run other than the quarry's records how it was made and what came out.
        self.assertEqual(written[0], "// Generated by tools/plan_quarry_ramp.py from cone.rs; do not edit.")
        self.assertIn("--end 12,-9", written[1])
        self.assertIn("--avoid=-110,-25,-70,25", written[1])
        self.assertEqual(written[2], "// Result: " + printed.strip())
        self.assertEqual(written[3], "&[")

    def test_says_so_when_no_road_reaches_the_end(self):
        with tempfile.TemporaryDirectory() as folder:
            relief_file, out = Path(folder) / "cone.rs", Path(folder) / "ramp.rs"
            relief_file.write_text(relief_file_text(self.relief))
            with self.assertRaisesRegex(SystemExit, "no path"):
                run_tool(
                    relief_file, out, "--scale", 1, "--grade", CONE_GRADE, "--entrance=-160,0", "--heading", 0,
                    "--half", "165,165", "--grid", 64, "--end", "12,-9", "--max-length", 100,
                )  # fmt: skip
            self.assertFalse(out.exists())

    def test_refuses_a_road_that_enters_an_avoid_box(self):
        through = [CONE_ENTRANCE, (-90.0, 0.0), CONE_END]  # straight through the box
        with tempfile.TemporaryDirectory() as folder:
            relief_file, out = Path(folder) / "cone.rs", Path(folder) / "ramp.rs"
            relief_file.write_text(relief_file_text(self.relief))
            with mock.patch.object(ramp_tool, "plan_path", return_value=through):
                with self.assertRaisesRegex(SystemExit, "avoid"):
                    run_tool(
                        relief_file, out, "--scale", 1, "--grade", 0.5, "--entrance=-160,0", "--half", "165,165",
                        "--end", "12,-9", "--avoid=-110,-25,-70,25",
                    )  # fmt: skip
            self.assertFalse(out.exists())

    def test_corners_keep_room_for_their_arcs_on_a_fine_grid(self):
        # Cells of 1.5 m in a small pit: four steps between corners leave too little room for two arcs of 9.5 m,
        # and a road that ended right after its last corner would cut that arc short.
        relief = cone_relief(span=90.0, rim=40.0, floor=8.0, depth=14.0)
        site = self.site(entrance=(-41.0, 0.0), half=(42.0, 42.0), grid=56, end=(6.0, -4.0), max_length_m=400.0)
        ground = ramp_tool.Ground(relief, 1.0, site.start)
        points = ramp_tool.plan_path(ground, CONE_GRADE, site)
        self.assertGreaterEqual(ramp_tool.measure(ground, points, site).min_radius, MIN_TURN_RADIUS_M)

    def test_command_line_pairs_and_boxes_read_negative_numbers(self):
        args = ramp_tool.parse_args(
            ["in.rs", "out.rs", "--centre=-1090,-100", "--half=170,230", "--entrance=-1194.7,97.2", "--end=-1150,-40"]
            + ["--avoid=-1095,97,-1003,205", "--avoid=-1,-2,3,4"]
        )
        self.assertEqual((args.centre, args.entrance, args.end), ((-1090.0, -100.0), (-1194.7, 97.2), (-1150.0, -40.0)))
        self.assertEqual(args.avoid, [(-1095.0, 97.0, -1003.0, 205.0), (-1.0, -2.0, 3.0, 4.0)])
        self.assertEqual(ramp_tool.parse_args(["in.rs", "out.rs"]).avoid, [])

    def test_an_end_outside_the_search_box_or_a_reversed_box_is_refused(self):
        for bad in (["--end=500,0", "--half=165,165"], ["--avoid=10,0,-10,5"], ["--avoid=0,9,5,3"]):
            with self.subTest(bad), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                ramp_tool.parse_args(["in.rs", "out.rs", *bad])


# A deep, narrow pit whose road must wind its way down: 50 m deep inside a 70 m rim, a 20 m floor. The road
# enters on the west rim along it, heading -z, so a right-hand spiral (+x, then +z, ...) circles the floor.
SPIRAL_PIT = dict(span=160.0, rim=70.0, floor=20.0, depth=50.0)
SPIRAL_ENTRANCE, SPIRAL_END, SPIRAL_GRADE = (-74.0, 0.0), (8.0, 8.0), 0.15
SPIRAL_OPTIONS = [
    "--scale", 1, "--grade", SPIRAL_GRADE, "--entrance=-74,0", "--half", "76,76", "--grid", 32, "--end", "8,8",
    "--max-length", 700,
]  # fmt: skip


class SpiralTests(unittest.TestCase):
    """A road that winds down a deep pit: it turns one way, keeps its laps apart and keeps to its grade as baked."""

    @classmethod
    def setUpClass(cls):
        cls.relief = cone_relief(**SPIRAL_PIT)
        # Grid 32 is the coarsest on which the right-hand spiral's laps, left free, come closer than 18 m.
        cls.free_spiral, _ = cls.run_spiral("--heading", 6, "--turn", "right")

    @classmethod
    def run_spiral(cls, *options):
        """Run the tool on the spiral pit with these options more; the text printed and the file written."""
        with tempfile.TemporaryDirectory() as folder:
            relief_file, out = Path(folder) / "spiral.rs", Path(folder) / "ramp.rs"
            relief_file.write_text(relief_file_text(cls.relief))
            printed = run_tool(relief_file, out, *SPIRAL_OPTIONS, *options)
            return printed, out.read_text()

    def plan(self, **options):
        defaults = dict(entrance=SPIRAL_ENTRANCE, heading=6, half=(76.0, 76.0), grid=24, end=SPIRAL_END)
        site = ramp_tool.Site(**{**dict(max_length_m=700.0), **defaults, **options})
        ground = ramp_tool.Ground(self.relief, 1.0, site.start)
        points = ramp_tool.plan_path(ground, SPIRAL_GRADE, site)
        return points, ramp_tool.measure(ground, points, site)

    def test_turns_one_way_only(self):
        # Straight in, free to turn, the road winds both ways; told to, it turns only left (or right) to the
        # same entrance's end on that hand.
        free, _ = self.plan(heading=0)
        self.assertEqual(set(turn_sides(free)), {-1, 1}, "the free road already turns one way")
        for turn, end, side in (("left", SPIRAL_END, -1), ("right", (8.0, -8.0), 1)):
            with self.subTest(turn):
                points, stats = self.plan(turn=turn, heading=0, end=end)
                self.assertEqual(points[-1], end)
                self.assertEqual(set(turn_sides(points)), {side})
                self.assertLessEqual(stats.grade, SPIRAL_GRADE)
                self.assertGreaterEqual(stats.min_radius, MIN_TURN_RADIUS_M)

    def test_the_run_in_bends_once_against_the_turn(self):
        # The entrance lies 14 m west of the box: the run-in heads +x, and the bend into -z (a left one) is rounded
        # like any corner; from there on the road turns right only.
        points, stats = self.plan(entrance=(-84.0, 0.0), half=(70.0, 70.0), grid=28, turn="right")
        self.assertEqual(points[0], (-84.0, 0.0))
        sides = turn_sides(points)
        bend = sides.index(1)
        self.assertGreater(bend, 0, "the run-in does not bend left")
        self.assertEqual(set(sides[:bend]), {-1})
        self.assertEqual(set(sides[bend:]), {1})
        self.assertGreaterEqual(stats.min_radius, MIN_TURN_RADIUS_M)
        self.assertLessEqual(stats.grade, SPIRAL_GRADE)

    def test_keeps_laps_apart(self):
        self.assertLess(closest_runs(self.free_spiral), MIN_RUN_GAP_M, "the free spiral's laps already keep apart")
        printed, written = self.run_spiral("--heading", 6, "--turn", "right", "--min-separation", MIN_RUN_GAP_M)
        self.assertGreaterEqual(closest_runs(printed), MIN_RUN_GAP_M)
        self.assertIn("--min-separation 18.0", written.splitlines()[1])

    def test_a_road_whose_laps_cannot_keep_apart_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            relief_file, out = Path(folder) / "spiral.rs", Path(folder) / "ramp.rs"
            relief_file.write_text(relief_file_text(self.relief))
            options = [*SPIRAL_OPTIONS, "--heading", 6, "--turn", "right"]
            with self.assertRaisesRegex(SystemExit, "no path"):  # no spiral down this pit keeps 60 m apart
                run_tool(relief_file, out, *options, "--min-separation", 60)
            self.assertFalse(out.exists())
            # The search keeps its laps apart, so hand the tool a spiral whose laps run 5 m apart: 1.5 laps, every
            # 3 m, its radius closing from 30 m to 22 m (no turn under 9 m).
            angles = [k * 0.1 for k in range(95)]
            close = [(30.0 * math.cos(a) * (1 - a / 36), 30.0 * math.sin(a) * (1 - a / 36)) for a in angles]
            with mock.patch.object(ramp_tool, "plan_path", return_value=close):
                with self.assertRaisesRegex(SystemExit, "apart"):
                    run_tool(relief_file, out, *options, "--grade", 0.9, "--min-separation", MIN_RUN_GAP_M)
            self.assertFalse(out.exists())

    def test_baked_grade_meets_the_target(self):
        # Down the Golden Sphere's crater to its floor: a long road with many corners, whose arcs shorten it by
        # metres, ending where the ground as it is lies below the smoothed relief the search follows.
        relief = ramp_tool.parse_relief(CORE_RELIEF_FILE.read_text())
        site = ramp_tool.Site(entrance=(0.0, 0.45), half=(0.43, 0.47), grid=32, heading=6, max_length_m=700.0)
        ground = ramp_tool.Ground(relief, 120.0, site.start)
        points = ramp_tool.plan_path(ground, 0.2, site)
        self.assertGreaterEqual(len(ramp_tool.corner_points(points)), 12, "the road has few corners")
        self.assertLessEqual(ramp_tool.measure(ground, points, site).grade, 0.2)


def closest_runs(printed):
    """The closest approach of separate runs that the tool printed, in metres."""
    return float(re.search(r"closest runs (\d+\.\d) m", printed).group(1))


if __name__ == "__main__":
    unittest.main()
