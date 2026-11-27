"""Tests for prepare_rock_textures.py, run on a synthetic patch.

python3 -m unittest tools/test_prepare_rock_textures.py -v
"""

import sys
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))

import prepare_rock_textures as rock  # noqa: E402
from prepare_asphalt_textures import blur, to_linear  # noqa: E402


def blue_grey_patch(size=128, seed=3):
    """Blue-grey stone with plates and a light gradient, like the crop of rock-17m."""
    noise = np.random.default_rng(seed).standard_normal((size, size)).astype(np.float32)
    plates = blur(noise, 4, wrap=True)
    plates /= plates.std()
    ramp = np.linspace(0.85, 1.15, size)[None, :]
    value = 95 * ramp * (1 + 0.2 * plates)
    rgb = np.clip(value[..., None] * [0.85, 0.95, 1.05], 0, 255)
    return Image.fromarray(rgb.astype(np.uint8), "RGB")


class RockMaps(unittest.TestCase):
    def setUp(self):
        self.albedo, self.normal, self.surface = rock.rock_maps(blue_grey_patch())

    def test_the_albedo_takes_the_scans_stone_colour(self):
        mean = to_linear(self.albedo).reshape(-1, 3).mean(axis=0)
        np.testing.assert_allclose(mean, rock.STONE, rtol=0.08)

    def test_every_map_keeps_the_patchs_native_size(self):
        for image in (self.albedo, self.normal, self.surface):
            self.assertEqual(image.size, (128, 128))

    def test_the_maps_tile(self):
        # Across the wrap, neighbouring pixels differ no more than inside the map.
        for image in (self.albedo, self.surface):
            pixels = np.asarray(image, np.float32)
            inside = np.abs(np.diff(pixels, axis=1)).mean()
            across = np.abs(pixels[:, 0] - pixels[:, -1]).mean()
            self.assertLess(across, 2.5 * inside)

    def test_the_surface_spans_its_ranges(self):
        surface = np.asarray(self.surface, np.float32) / 255
        ao, roughness, height = surface[..., 0], surface[..., 1], surface[..., 2]
        self.assertGreater(ao.min(), 1 - rock.AO_STRENGTH - 0.01)
        self.assertTrue((roughness >= rock.ROUGHNESS[0] - 0.01).all())
        self.assertTrue((roughness <= rock.ROUGHNESS[1] + 0.01).all())
        self.assertGreater(height.std(), 0.05)


if __name__ == "__main__":
    unittest.main()
