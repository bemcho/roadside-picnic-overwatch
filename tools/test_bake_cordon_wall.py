"""Tests for bake_cordon_wall.py's measuring, layout and geometry code (no Blender needed).

python3 -m unittest tools/test_bake_cordon_wall.py -v
"""

import io
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))

import bake_cordon_wall as wall_tool  # noqa: E402

HALF = 0.147
TOP = 0.41
CENTRE = (0.044, 0.013)  # where the synthetic wall sits in x and z, and its foot is at -0.3


def synthetic_scan(seed=1):
    """A wall body (faces, ribs, a plateau with coils on it, a tower in the middle) as bare vertices."""
    rng = np.random.default_rng(seed)
    n = 4000
    x = rng.uniform(-0.43, 0.43, n) + CENTRE[0]
    ribs = 0.03 * (rng.random(n) < 0.2)  # a fifth of the lower faces stands on a rib
    faces = np.concatenate(
        [
            np.stack([x, rng.uniform(0.0, 0.31, n) - 0.3, rng.choice([-1, 1], n) * (HALF + ribs) + CENTRE[1]], axis=1),
            np.stack([x, rng.uniform(0.31, TOP, n) - 0.3, rng.choice([-1, 1], n) * (HALF + 0.01) + CENTRE[1]], axis=1),
        ]
    )
    plateau = np.stack(
        [x, np.full(n, TOP - 0.3), rng.choice([-1, 1], n) * rng.uniform(0.1, 0.15, n) + CENTRE[1]], axis=1
    )
    deck = np.stack([x, np.full(n, TOP - 0.004 - 0.3), rng.uniform(-0.05, 0.05, n) + CENTRE[1]], axis=1)
    coils = np.stack(
        [
            x,
            rng.uniform(TOP + 0.01, TOP + 0.045, n) - 0.3,
            rng.choice([-1, 1], n) * rng.uniform(0.1, 0.16, n) + CENTRE[1],
        ],
        axis=1,
    )
    tower_x = rng.uniform(-0.3, -0.15, 400) + CENTRE[0]
    tower = np.stack(
        [tower_x, rng.uniform(0.0, 0.2, 400) + TOP - 0.3, rng.uniform(-0.05, 0.05, 400) + CENTRE[1]], axis=1
    )
    return np.concatenate([faces, plateau, deck, coils, tower]).astype(np.float32)


class MeasureTests(unittest.TestCase):
    def test_measures_the_wall_body_not_the_tower_or_the_ribs(self):
        wall = wall_tool.measure_wall(synthetic_scan())
        self.assertAlmostEqual(wall.half_thickness, HALF, delta=0.006)
        self.assertAlmostEqual(wall.centre_x, CENTRE[0], delta=0.01)
        self.assertAlmostEqual(wall.centre_z, CENTRE[1], delta=0.003)
        self.assertAlmostEqual(wall.top, TOP, delta=0.001)
        self.assertAlmostEqual(wall.deck, TOP - 0.004, delta=0.001)
        self.assertAlmostEqual(wall.wire_top, TOP + 0.045, delta=0.003)  # the tower reaches 0.2 higher
        self.assertAlmostEqual(wall.coil_half, 0.16, delta=0.005)
        self.assertAlmostEqual(wall.rib_height, 0.03, delta=0.008)

    def test_plateau_follows_a_wavy_top(self):
        rng = np.random.default_rng(3)
        x = rng.uniform(-0.4, 0.4, 20000)
        wave = 0.003 * np.sin(x * 12)
        y = TOP + wave
        outer = rng.uniform(0.1, 0.15, x.size)
        found = wall_tool.plateau_heights(x, y, outer > 0.6 * HALF, TOP)
        self.assertLess(np.abs(found - y).max(), 0.002)

    def test_the_scan_is_shifted_onto_the_panel_frame(self):
        """The panel frame has the foot at y = 0 and the wall body at x = z = 0 (the old panel's own)."""
        scan = synthetic_scan()
        wall = wall_tool.measure_wall(scan)
        shifted = scan + np.array(wall_tool.scan_shift(wall), dtype=np.float32)
        self.assertAlmostEqual(float(shifted[:, 1].min()), 0.0, places=6)
        again = wall_tool.measure_wall(shifted)
        self.assertAlmostEqual(again.centre_x, 0.0, places=5)
        self.assertAlmostEqual(again.centre_z, 0.0, places=5)


class LayoutTests(unittest.TestCase):
    wall = wall_tool.Wall(
        min_y=-0.3, centre_x=0, centre_z=0, half_thickness=HALF, rib_height=0.03, top=TOP,
        deck=TOP - 0.004, wire_top=0.45, coil_half=0.16, top_peak=(np.zeros(1), np.zeros(1)),
    )  # fmt: skip

    def test_islands_fit_the_atlas_without_touching(self):
        (width, height), islands = wall_tool.box_layout(self.wall)
        self.assertEqual((width % 4, height % 4), (0, 0))
        boxes = []
        for island in islands.values():
            self.assertGreaterEqual(island.x0, wall_tool.GUTTER)
            self.assertLessEqual(island.x0 + island.width, width - wall_tool.GUTTER + 1e-6)
            self.assertLessEqual(island.y0 + island.height, height - wall_tool.GUTTER + 1e-6)
            boxes.append((island.x0, island.y0, island.x0 + island.width, island.y0 + island.height))
        for i, a in enumerate(boxes):
            for b in boxes[i + 1 :]:
                apart = a[2] + wall_tool.GUTTER <= b[0] or b[2] + wall_tool.GUTTER <= a[0]
                apart = apart or a[3] + wall_tool.GUTTER <= b[1] or b[3] + wall_tool.GUTTER <= a[1]
                self.assertTrue(apart, (a, b))

    def test_every_face_has_the_same_texel_density(self):
        _, islands = wall_tool.box_layout(self.wall)
        density = wall_tool.SPAN_PX / wall_tool.PANEL_SPAN
        for name, thickness in (("front", None), ("top", 2 * HALF), ("end", 2 * HALF)):
            island = islands[name]
            along = island.width / (wall_tool.PANEL_SPAN if thickness is None or name == "top" else thickness)
            self.assertAlmostEqual(along, density, delta=1e-6)
        self.assertAlmostEqual(islands["front"].height / TOP, density, delta=1e-6)

    def test_box_is_a_panel_wide_and_pre_split(self):
        _, islands = wall_tool.box_layout(self.wall)
        geometry = wall_tool.box_geometry(self.wall, islands)
        points = np.array(geometry.positions)
        self.assertEqual(len(points), 84)
        self.assertEqual(len(geometry.faces), 8 * 2 * 2 + 8 + 2 * 2)
        self.assertAlmostEqual(points[:, 0].min(), -0.43, delta=1e-4)
        self.assertAlmostEqual(points[:, 0].max(), 0.43, delta=1e-4)
        self.assertAlmostEqual(points[:, 2].max(), HALF, delta=1e-6)
        self.assertAlmostEqual(points[:, 1].max(), TOP, delta=1e-6)
        self.assertEqual(sorted({round(float(v), 4) for v in points[:, 1]}), sorted({0.0, 0.15, TOP}))
        columns = sorted({round(float(v), 4) for v in points[:, 0] if abs(v) < 0.43})
        self.assertEqual(len(columns), 7)  # 8 columns, 9 cuts, the two outer ones are the ends

    def test_faces_wind_outward_and_have_no_bottom(self):
        _, islands = wall_tool.box_layout(self.wall)
        geometry = wall_tool.box_geometry(self.wall, islands)
        points = np.array(geometry.positions)
        for face, intended in zip(geometry.faces, geometry.normals):
            a, b, c, _ = (points[i] for i in face)
            normal = np.cross(b - a, c - b)
            self.assertGreater(np.dot(normal / np.linalg.norm(normal), intended), 0.999)
        self.assertFalse(any(tuple(n) == (0, -1, 0) for n in geometry.normals))

    def test_u_grows_with_x_on_the_long_faces(self):
        _, islands = wall_tool.box_layout(self.wall)
        geometry = wall_tool.box_geometry(self.wall, islands)
        points, uvs = np.array(geometry.positions), np.array(geometry.uvs)
        for face, intended in zip(geometry.faces, geometry.normals):
            if tuple(intended) in ((0, 0, 1), (0, 0, -1), (0, 1, 0)):
                a, b = face[0], face[1] if points[face[1]][0] != points[face[0]][0] else face[3]
                low, high = sorted((a, b), key=lambda i: points[i][0])
                self.assertLess(uvs[low][0], uvs[high][0])

    def test_uvs_stay_inside_their_island(self):
        _, islands = wall_tool.box_layout(self.wall)
        geometry = wall_tool.box_geometry(self.wall, islands)
        uvs = np.array(geometry.uvs)
        self.assertTrue(((uvs >= 0) & (uvs <= 1)).all())

    def test_wire_strips_span_the_panel(self):
        shape = wall_tool.wire_shape(self.wall)
        _, islands = wall_tool.wire_layout(shape)
        points = np.array(wall_tool.wire_geometry(shape, islands).positions)
        upright, flat = points[: 2 * 9], points[2 * 9 :]
        for strip in (upright, flat):
            self.assertAlmostEqual(strip[:, 0].min(), -0.43, delta=1e-4)
            self.assertAlmostEqual(strip[:, 0].max(), 0.43, delta=1e-4)
            self.assertEqual(len({round(float(v), 4) for v in strip[:, 0]}), 9)
        self.assertTrue((upright[:, 2] == 0).all())
        self.assertTrue(np.allclose(flat[:, 1], shape.middle))
        self.assertGreater(shape.floor, TOP)


class DetrendAndEndsTests(unittest.TestCase):
    def test_detrend_removes_a_lean_and_a_step_but_keeps_relief(self):
        rows, columns = 200, 300
        height = np.arange(rows)[:, None] / rows
        lean = 0.008 * height + np.where(height > 0.75, -0.02, 0.0)  # a ramp and a step at the cap, along x
        joints = np.zeros((rows, columns))
        joints[:, 100] = 0.004  # a vertical joint, recessed
        joints[40, :] = 0.003  # a horizontal one
        ribs = np.zeros((rows, columns))
        ribs[:, 20:60] = -0.03  # proud of the face
        depth = lean + joints + ribs
        flat = wall_tool.detrend_rows(depth, slice(0, columns))
        self.assertLess(float(flat[60:, 150:].max()), 0.0045)  # nothing left but the joints
        self.assertEqual(float(flat[:, 20:60].max()), 0.0)  # the ribs clamp to 0
        self.assertAlmostEqual(float(flat[100, 100]), 0.004, places=6)  # the joint survives
        self.assertAlmostEqual(float(flat[100, 200]), 0.0, places=6)  # the ramp does not
        self.assertAlmostEqual(float(flat[190, 200]), 0.0, places=6)  # nor the step

    def test_both_ends_agree_after_sealing(self):
        rng = np.random.default_rng(5)
        pad, band, width = 6, 10, 120
        albedo = rng.uniform(0.1, 0.5, (20, width + 2 * pad, 4)).astype(np.float32)
        normal = rng.uniform(0, 1, albedo.shape).astype(np.float32)
        depth = rng.uniform(0, 0.01, albedo.shape).astype(np.float32)
        inside = albedo[:, pad + band : pad + 2 * band].copy()
        wall_tool.seal_ends(albedo, normal, depth, pad, band)
        left, right = albedo[:, pad], albedo[:, pad + width - 1]
        self.assertTrue(np.allclose(left, right))  # the outermost columns meet
        self.assertTrue(np.allclose(albedo[:, pad], albedo[:, pad - 3]))  # and the margin carries on
        self.assertTrue(np.allclose(normal[:, pad : pad + band, :3], [0.5, 0.5, 1.0]))
        self.assertTrue(np.allclose(normal[:, pad + width - band : pad + width, :3], [0.5, 0.5, 1.0]))
        self.assertEqual(float(depth[:, pad : pad + band, 0].max()), 0.0)
        self.assertEqual(float(depth[:, pad + width - band : pad + width, 0].max()), 0.0)
        inner_left = albedo[:, pad + band - 1]  # the band's inner edge continues the reflected concrete
        self.assertTrue(np.allclose(inner_left, inside[:, 0], atol=0.03))

    def test_contact_marks_find_dark_and_tilted_blobs_but_not_joints(self):
        colour = np.full((120, 200, 3), 0.3, np.float32)
        colour[:, 150] = 0.05  # a one-pixel joint, the whole height
        colour[60, :] = 0.05  # and a horizontal one
        colour[40:52, 60:72] = 0.04  # a leg's foot: a 12 px dark blob
        normal = np.full(colour.shape, [0.5, 0.5, 1.0], np.float32)
        normal[80:90, 100:120] = [0.9, 0.5, 0.7]  # a dimple the foot left: tilted
        normal[10:20, 30:60] = [0.9, 0.5, 0.7]  # a tilted blob on the shoulder rows, outside the middle
        normal[:, 150] = [0.9, 0.5, 0.7]  # a tilted joint line
        middle = np.zeros(120, bool)
        middle[30:100] = True
        marks = wall_tool.contact_marks(colour, normal, middle)
        self.assertTrue(marks[46, 66])  # the dark foot
        self.assertTrue(marks[85, 110])  # the dimple
        self.assertFalse(marks[15, 45])  # tilt only counts on the middle rows
        self.assertFalse(marks[20, 150] or marks[60, 20] or marks[110, 100])  # joints and concrete are left alone
        self.assertTrue(marks[46, 66 + 12 // 2 + wall_tool.MARK_GROW - 1])  # the mask is grown past the blob

    def test_box_blur_is_a_local_mean(self):
        values = np.arange(36, dtype=np.float32).reshape(6, 6)
        blurred = wall_tool.box_blur(values, 3)
        self.assertAlmostEqual(float(blurred[2, 2]), float(values[1:4, 1:4].mean()), places=5)


class ImageTests(unittest.TestCase):
    def test_depth_is_white_at_the_percentile_and_clamped(self):
        depth = np.linspace(0, 1, 1000).reshape(10, 100)
        image, white = wall_tool.depth_image(depth, np.ones(depth.shape, bool), percentile=90)
        self.assertAlmostEqual(white, 0.9, delta=0.01)
        self.assertEqual(float(image.max()), 1.0)

    def test_settle_restores_half_hit_colour_and_fills_misses(self):
        size = 12
        colour = np.zeros((size, size, 4), np.float32)
        colour[:, :, :3] = [0.4, 0.2, 0.1]
        colour[:, :, 3] = 1
        colour[4, 4] = [0.2, 0.1, 0.05, 0.5]  # half of its samples hit
        colour[8:10, 8:10] = [0, 0, 0, 0]  # missed
        colour[2, 9] = [0, 0, 0, 1]  # hit, but black: an inside face
        wall_tool.settle([colour], colour[..., 3].copy(), colour[..., :3].max(axis=2) < wall_tool.BLACK)
        self.assertTrue(np.allclose(colour[4, 4, :3], [0.4, 0.2, 0.1], atol=1e-5))
        self.assertTrue(np.allclose(colour[8:10, 8:10, :3], [0.4, 0.2, 0.1], atol=1e-5))
        self.assertTrue(np.allclose(colour[2, 9, :3], [0.4, 0.2, 0.1], atol=1e-5))

    def test_unusable_texels_are_inside_or_stained_low(self):
        density = 100.0
        concrete = np.full((60, 80, 3), [0.07, 0.09, 0.10], np.float32)  # blue-grey, 0.7 red over blue
        concrete[30, 40] = 0  # an inside face
        concrete[5, 40] = [0.12, 0.10, 0.08]  # a stain low on the wall
        concrete[50, 41] = [0.12, 0.10, 0.08]  # warm, but above the ground zone: left alone
        flat = np.full(concrete.shape, [0.5, 0.5, 1.0], np.float32)  # tangent normals pointing straight out
        flat[20, 40] = [1.0, 0.5, 0.5]  # facing sideways
        bad = wall_tool.unusable_texels(concrete, flat, density)
        self.assertTrue(bad[30, 40] and bad[5, 40] and bad[20, 40])
        self.assertFalse(bad[50, 41])
        self.assertEqual(int(bad[50].sum()), 0)

    def test_wire_colour_is_unpremultiplied_and_filled_flat(self):
        colour = np.zeros((2, 2, 3), np.float32)
        colour[0, 0] = [0.2, 0.2, 0.2]  # a half-covered texel of 0.4 grey
        colour[0, 1] = [0.4, 0.4, 0.4]
        coverage = np.array([[0.5, 1.0], [0.0, 0.0]], np.float32)
        result = wall_tool.unpremultiply(colour, coverage)
        self.assertTrue(np.allclose(result[0], 0.4))
        self.assertTrue(np.allclose(result[1], 0.4))  # no wire: the mean wire colour

    def test_png_round_trips(self):
        pixels = np.random.default_rng(2).uniform(0, 1, (7, 5, 4)).astype(np.float32)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "x.png"
            wall_tool.write_png(path, pixels)
            with Image.open(io.BytesIO(path.read_bytes())) as image:
                self.assertEqual(image.mode, "RGBA")
                decoded = np.asarray(image)
        expected = np.round(pixels[::-1] * 255).astype(np.uint8)  # rows from the bottom in, from the top out
        self.assertTrue((decoded == expected).all())


if __name__ == "__main__":
    unittest.main()
