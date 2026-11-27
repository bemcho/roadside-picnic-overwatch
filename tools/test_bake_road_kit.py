"""Tests for the pure helpers of bake_road_kit.py (no Blender needed).

python3 -m pytest tools/test_bake_road_kit.py -q
"""

import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import bake_road_kit as kit  # noqa: E402


def test_native_density_is_the_area_weighted_texel_density():
    # A quarter of a 1024 px texture over one square metre: 512 px per metre.
    assert kit.native_px_per_metre([0.25], [1.0], [(1024, 1024)]) == 512.0
    # Two pieces weigh by area: 4 m2 at 256 px/m and 1 m2 at 512 px/m.
    density = kit.native_px_per_metre([0.25, 0.25], [4.0, 1.0], [(1024, 1024), (1024, 1024)])
    assert abs(density - np.sqrt((0.25 * 1024**2 * 2) / 5.0)) < 1e-9


def test_blend_period_continues_the_last_row_into_the_first():
    rng = np.random.default_rng(1)
    strip = rng.random((40, 3, 2))
    out = kit.blend_period(strip, 32)
    assert out.shape == (32, 3, 2)
    # The first row is the next copy's continuation of the last one, the overlap's end is untouched.
    assert np.allclose(out[0], strip[32], atol=1e-6)
    assert np.allclose(out[8:], strip[8:32], atol=1e-6)
    # Halfway through the overlap both copies weigh the same.
    assert np.allclose(out[4], (strip[4] + strip[36]) / 2, atol=1e-6)


def test_normal_tilts_away_from_a_ramp_rising_along_the_rows():
    rows = np.linspace(0.0, 1.0, 16)[:, None] * np.ones((1, 8))
    normal = kit.normal_from_height(rows, metres_per_px=0.1, depth_m=1.0)
    assert normal.shape == (16, 8, 2)
    middle = normal[4:12, 2:6]
    assert np.allclose(middle[..., 0], 0.5, atol=1e-6)  # flat across
    assert (middle[..., 1] < 0.5).all()  # leans back toward the lower rows


def test_fill_misses_takes_the_nearest_hit():
    values = np.zeros((5, 5, 1))
    values[2, 2] = 7.0
    hit = np.zeros((5, 5), bool)
    hit[2, 2] = True
    filled = kit.fill_misses(values, hit)
    assert np.allclose(filled, 7.0)


def test_write_png_keeps_the_coverage_alpha(tmp_path):
    from PIL import Image

    rows = np.zeros((4, 4, 4))
    rows[..., 3] = 1.0
    rows[0, 0] = (1.0, 0.5, 0.0, 0.0)
    kit.write_png(tmp_path / "a.png", rows)
    image = Image.open(tmp_path / "a.png")
    assert image.mode == "RGBA"
    assert image.getpixel((0, 0)) == (255, 128, 0, 0)
    assert image.getpixel((1, 0))[3] == 255
