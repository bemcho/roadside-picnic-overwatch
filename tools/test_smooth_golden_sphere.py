"""Tests for smooth_golden_sphere.py, run on a synthetic knobbed ball.

python3 -m unittest tools/test_smooth_golden_sphere.py -v
"""

import sys
import unittest
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import smooth_golden_sphere as smooth  # noqa: E402

KNOB_AXIS = np.array([0.78, 0.0, 0.63]) / np.linalg.norm([0.78, 0.0, 0.63])
KNOB_REACH = 0.12
SEAM_AXIS = np.array([0.0, 1.0, 0.0])


def knobbed_ball(count=60000, radius=0.49, seed=7):
    """A scanned-looking ball (0.3% noise) with a 6% knob, its socket sunk to the ball,
    and a 0.6% raised seam; with its normals and each point's angle to the knob."""
    rng = np.random.default_rng(seed)
    directions = rng.standard_normal((count, 3))
    directions /= np.linalg.norm(directions, axis=1, keepdims=True)
    noise = 0.003 * rng.uniform(-1, 1, count)
    angle = np.arccos(np.clip(directions @ KNOB_AXIS, -1, 1))
    knob = np.where((angle < KNOB_REACH) & (angle > 0.03), 0.06, 0.0)
    seam = np.where(np.abs(directions @ SEAM_AXIS) < 0.01, 0.006, 0.0)
    positions = directions * (radius * (1 + noise + knob + seam))[:, None]
    return positions, directions.copy(), angle, seam > 0


class SmoothKnobs(unittest.TestCase):
    def test_the_knob_and_its_socket_go_onto_the_ball(self):
        positions, normals, angle, _ = knobbed_ball()
        new_positions, _, share = smooth.smooth_knobs(positions, normals)
        patch = angle < KNOB_REACH + smooth.MARGIN / 2
        self.assertTrue((share[patch] == 1).all())
        radius = np.linalg.norm(new_positions - smooth.ball_centre(positions), axis=1)
        np.testing.assert_allclose(radius[patch], np.median(radius), rtol=1e-6)

    def test_the_seams_and_the_rest_are_left_alone(self):
        positions, normals, angle, on_seam = knobbed_ball()
        new_positions, new_normals, _ = smooth.smooth_knobs(positions, normals)
        far = angle > KNOB_REACH + smooth.MARGIN + smooth.BAND + 0.01
        self.assertTrue(on_seam[far].any())
        np.testing.assert_allclose(new_positions[far], positions[far], atol=1e-12)
        np.testing.assert_allclose(new_normals[far], normals[far], atol=1e-12)

    def test_moved_normals_point_outward_and_stay_unit(self):
        positions, normals, angle, _ = knobbed_ball()
        tilted = normals.copy()
        tilted[angle < KNOB_REACH] = np.array([0.0, 1.0, 0.0])
        new_positions, new_normals, share = smooth.smooth_knobs(positions, tilted)
        np.testing.assert_allclose(np.linalg.norm(new_normals, axis=1), 1.0, atol=1e-9)
        full = share >= 1
        offset = new_positions[full] - smooth.ball_centre(positions)
        outward = offset / np.linalg.norm(offset, axis=1, keepdims=True)
        np.testing.assert_allclose(new_normals[full], outward, atol=1e-9)

    def test_the_centre_ignores_the_knob(self):
        positions, _, _, _ = knobbed_ball()
        self.assertLess(np.linalg.norm(smooth.ball_centre(positions)), 0.001)

    def test_a_glb_round_trips_through_read_and_write(self):
        gltf = {"asset": {"version": "2.0"}, "buffers": [{"byteLength": 12}]}
        binary = bytearray(np.arange(3, dtype=np.float32).tobytes())
        again, binary_again = smooth.read_glb(smooth.write_glb(gltf, binary))
        self.assertEqual(again, gltf)
        self.assertEqual(bytes(binary_again), bytes(binary))


if __name__ == "__main__":
    unittest.main()
