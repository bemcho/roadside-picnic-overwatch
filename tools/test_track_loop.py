"""Tests for track_loop.py's loop fit, link pitch and Rust output (on synthetic belts).

python3 -m unittest tools/test_track_loop.py -v
"""

import math
import re
import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import track_loop  # noqa: E402

# A stadium like the carrier's track: arcs round the sprocket and idler, straight runs between.
ARC_Z, ARC_Y, RADIUS = 0.38, 0.05, 0.045
PERIMETER = 2 * 2 * ARC_Z + 2 * math.pi * RADIUS
STEP = 0.007


def stadium_point(s):
    """The stadium's mid-line at arc length s, from the front end of the lower run, running rearward."""
    straight, arc = 2 * ARC_Z, math.pi * RADIUS
    s %= PERIMETER
    if s < straight:  # lower run, front to back
        return ARC_Z - s, ARC_Y - RADIUS, 0.0, -1.0
    s -= straight
    if s < arc:  # up round the back
        angle = -math.pi / 2 - s / RADIUS
        return -ARC_Z + RADIUS * math.cos(angle), ARC_Y + RADIUS * math.sin(angle), math.cos(angle), math.sin(angle)
    s -= arc
    if s < straight:  # upper run, back to front
        return -ARC_Z + s, ARC_Y + RADIUS, 0.0, 1.0
    angle = math.pi / 2 - (s - straight) / RADIUS
    return ARC_Z + RADIUS * math.cos(angle), ARC_Y + RADIUS * math.sin(angle), math.cos(angle), math.sin(angle)


def scattered_stadium(count=20000, spread=0.004, seed=7):
    """Face centroids scattered across the band about the stadium's mid-line, as (z, y)."""
    rng = np.random.default_rng(seed)
    points = []
    for s, offset in zip(rng.uniform(0, PERIMETER, count), rng.uniform(-spread, spread, count)):
        z, y, nz, ny = stadium_point(s)  # (nz, ny) is the outward normal
        points.append((z + offset * nz, y + offset * ny))
    return np.array(points)


def distance_to_stadium(z, y):
    if abs(z) <= ARC_Z:
        return abs(abs(y - ARC_Y) - RADIUS)
    return abs(math.hypot(abs(z) - ARC_Z, y - ARC_Y) - RADIUS)


class FitLoopTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.loop = track_loop.fit_loop(scattered_stadium(), STEP)

    def test_fit_loop_closes_round_a_stadium(self):
        loop = self.loop
        worst = max(distance_to_stadium(z, y) for z, y in loop)
        self.assertLess(worst, 0.003)
        gaps = np.linalg.norm(np.roll(loop, -1, axis=0) - loop, axis=1)  # includes last -> first
        self.assertAlmostEqual(gaps.sum(), PERIMETER, delta=0.01 * PERIMETER)
        self.assertLessEqual(gaps.max(), 1.5 * STEP)
        self.assertGreater(np.linalg.norm(loop[-1] - loop[0]), 0.0)  # last point is not the first

    def test_fit_loop_runs_rearward_along_the_bottom(self):
        loop = self.loop
        # Start the walk at the front of the lower run, so the bottom points are one stretch.
        bottom = loop[:, 1] < 0.02
        first = np.flatnonzero(bottom & ~np.roll(bottom, 1))
        self.assertEqual(len(first), 1, "the lower run is one stretch of the loop")
        z = np.roll(loop, -first[0], axis=0)[: bottom.sum(), 0]
        self.assertTrue(np.all(np.diff(z) < 0), "the lower run's points go toward -z")


    def test_fit_loop_weighs_faces_by_area(self):
        # The outer half of the upper run meshed ten times finer: its many small faces must not pull the
        # mid-line out once they are weighed by their area.
        points = scattered_stadium()
        rng = np.random.default_rng(11)
        upper = np.abs(points[:, 0]) < ARC_Z * 0.9
        fine = upper & (points[:, 1] > ARC_Y + RADIUS)
        extra = np.repeat(points[fine], 9, axis=0) + rng.uniform(-0.0005, 0.0005, (9 * fine.sum(), 2))
        dense = np.vstack([points, extra])
        areas = np.concatenate([np.where(fine, 0.1, 1.0), np.full(len(extra), 0.1)])
        loop = track_loop.fit_loop(dense, STEP, weights=areas)
        top = loop[(np.abs(loop[:, 0]) < ARC_Z * 0.8) & (loop[:, 1] > ARC_Y)]
        self.assertLess(np.abs(top[:, 1] - (ARC_Y + RADIUS)).max(), 0.001)
        unweighted = track_loop.fit_loop(dense, STEP)
        top = unweighted[(np.abs(unweighted[:, 0]) < ARC_Z * 0.8) & (unweighted[:, 1] > ARC_Y)]
        self.assertGreater(np.abs(top[:, 1] - (ARC_Y + RADIUS)).mean(), 0.001)  # the bias the weights remove

    def test_fit_loop_smooths_out_a_one_pitch_zigzag(self):
        # Links repeat along the belt, so its faces' mean offset swings once per pitch (horns, pads). Over a
        # span of whole pitches the fit must not follow that: of a +-1 mm square wave on the centroids, under
        # 0.05 mm of one-pitch wave is left on the straight runs (a 2.5-pitch box left 0.3-0.9 mm).
        pitch = 0.0245
        rng = np.random.default_rng(5)
        points = []
        for s in rng.uniform(0, PERIMETER, 40000):
            z, y, nz, ny = stadium_point(s)
            offset = 0.001 * (1.0 if (s / pitch) % 1.0 < 0.5 else -1.0) + rng.uniform(-0.004, 0.004)
            points.append((z + offset * nz, y + offset * ny))
        loop = track_loop.fit_loop(np.array(points), pitch / 2, span=2 * pitch)
        straight = np.abs(loop[:, 0]) < ARC_Z - pitch
        for upper in (False, True):
            run = loop[straight & ((loop[:, 1] > ARC_Y) == upper)]
            off = run[:, 1] - ARC_Y - (RADIUS if upper else -RADIUS)
            phase = 2 * np.pi * run[:, 0] / pitch
            wave = 2 * np.hypot(np.mean(off * np.cos(phase)), np.mean(off * np.sin(phase)))
            self.assertLess(wave, 0.00005)
        arcs = [distance_to_stadium(z, y) for z, y in loop[~straight]]
        self.assertLess(max(arcs), 0.001)  # the wider window does not flatten the arcs

class LinkPitchTests(unittest.TestCase):
    def test_link_pitch_finds_the_repeat(self):
        rng = np.random.default_rng(3)
        pitch = 0.0135
        centres = np.arange(-0.3, 0.3, pitch)
        z = np.concatenate([centre + rng.normal(0.0, 0.0012, 60) for centre in centres])
        z = np.concatenate([z, rng.uniform(-0.3, 0.3, 1000)])  # background faces between the clusters
        self.assertAlmostEqual(track_loop.link_pitch(z), pitch, delta=0.02 * pitch)


class WrapRadiusTests(unittest.TestCase):
    def belt_round(self, circles):
        """A belt held out by circles (z, y, radius): their convex hull, as `fit_loop` starts from."""
        angles = np.linspace(0, 2 * np.pi, 720, endpoint=False)
        ring = np.concatenate([np.stack([z + r * np.cos(angles), y + r * np.sin(angles)], axis=1) for z, y, r in circles])
        return track_loop.start_and_direction(track_loop.resample(track_loop.convex_hull(ring), STEP))

    def test_wrap_radius_gives_back_the_circles_a_belt_runs_round(self):
        # A sprocket 0.046 and an idler 0.053 apart: each end's mean distance from its hub is its own circle's
        # radius, not the average of the two nor the distance to the straight runs.
        front, rear = (0.35, 0.12, 0.046), (-0.42, 0.125, 0.053)
        loop = self.belt_round([front, rear])
        self.assertAlmostEqual(track_loop.wrap_radius(loop, front[:2], +1), front[2], delta=0.0005)
        self.assertAlmostEqual(track_loop.wrap_radius(loop, rear[:2], -1), rear[2], delta=0.0005)

    def test_wrap_radius_counts_only_the_arc_at_the_end(self):
        # A belt that also runs round a third, bigger wheel below the sprocket's hub: only the loop points
        # within a sixth of a turn of straight out from the hub count, so that wheel's arc leaves the mean alone.
        front, rear, road = (0.35, 0.12, 0.046), (-0.42, 0.12, 0.046), (0.1, 0.0, 0.07)
        loop = self.belt_round([front, rear, road])
        self.assertAlmostEqual(track_loop.wrap_radius(loop, front[:2], +1), front[2], delta=0.0005)

    def test_wrap_radius_needs_a_loop_that_reaches_the_end(self):
        with self.assertRaises(ValueError):
            track_loop.wrap_radius(np.array([[0.0, 0.0], [0.1, 0.0], [0.0, 0.1]]), (5.0, 5.0), +1)


class RustModuleTests(unittest.TestCase):
    def test_rust_module_round_trips(self):
        loops = [np.array([[0.25, 0.01], [-0.25, 0.01], [0.0, 0.2]]), np.array([[0.3, 0.02], [-0.3, 0.02], [0.0, 0.18]])]
        text = track_loop.rust_track_module(loops, [0.1931, -0.1929], [71, 72], [(0.0471, 0.0540), (0.0451, 0.0537)])

        loops_text = re.search(r"pub\(super\) const TRACK_LOOPS: \[&\[\[f32; 2\]\]; 2\] = \[(.*?)\];\n", text, re.S)
        self.assertIsNotNone(loops_text)
        sides = re.findall(r"&\[(.*?)\]\s*,?\s*(?=&\[|$)", loops_text.group(1).strip(), re.S)
        self.assertEqual(len(sides), 2)
        for side, loop in zip(sides, loops):
            pairs = [(float(a), float(b)) for a, b in re.findall(r"\[(-?[\d.]+), (-?[\d.]+)\]", side)]
            np.testing.assert_allclose(pairs, loop, atol=1e-6)

        xs = re.search(r"pub\(super\) const TRACK_X: \[f32; 2\] = \[(-?[\d.]+), (-?[\d.]+)\];", text)
        self.assertEqual((float(xs.group(1)), float(xs.group(2))), (0.1931, -0.1929))
        links = re.search(r"pub\(super\) const TRACK_LINKS: \[usize; 2\] = \[(\d+), (\d+)\];", text)
        self.assertEqual((int(links.group(1)), int(links.group(2))), (71, 72))
        wraps = re.search(r"pub\(super\) const TRACK_WRAP: \[\[f32; 2\]; 2\] = \[\[(-?[\d.]+), (-?[\d.]+)\], \[(-?[\d.]+), (-?[\d.]+)\]\];", text)
        self.assertIsNotNone(wraps)
        self.assertEqual([float(w) for w in wraps.groups()], [0.0471, 0.0540, 0.0451, 0.0537])
        self.assertTrue(text.startswith("//! Generated by `tools/split_moving_parts.py --model carrier --track-out"))


if __name__ == "__main__":
    unittest.main()
