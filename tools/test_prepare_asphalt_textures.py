"""Tests for prepare_asphalt_textures.py, run on synthetic images.

python3 -m unittest tools/test_prepare_asphalt_textures.py -v
"""

import io
import shutil
import struct
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent))

import prepare_asphalt_textures as asphalt  # noqa: E402


def smooth_noise(size, sigma, seed):
    """Zero-mean, unit-deviation noise with features about `sigma` pixels wide."""
    field = np.random.default_rng(seed).standard_normal((size, size)).astype(np.float32)
    field = asphalt.blur(field, sigma, wrap=True)
    return (field - field.mean()) / field.std()


def synthetic_photo(size, grey, seed=1):
    """Grey asphalt with slow blotches and fine grain, brighter toward one corner."""
    ramp = np.linspace(0.8, 1.2, size)[None, :] * np.linspace(0.8, 1.2, size)[:, None]
    value = grey * ramp * (1 + 0.15 * smooth_noise(size, 6, seed) + 0.06 * smooth_noise(size, 1, seed + 1))
    rgb = np.clip(value[..., None] * [1.0, 1.0, 1.05], 0, 255)
    return Image.fromarray(rgb.astype(np.uint8), "RGB")


def linear_luminance(image):
    srgb = np.asarray(image.convert("RGB"), dtype=np.float64) / 255
    linear = np.where(srgb <= 0.04045, srgb / 12.92, ((srgb + 0.055) / 1.055) ** 2.4)
    return (linear @ [0.2126, 0.7152, 0.0722]).mean()


def ktx2_header(path):
    """(vkFormat, width, height, levelCount) of a KTX2 file."""
    data = path.read_bytes()
    vk_format, _, width, height, _, _, _, levels = struct.unpack_from("<8I", data, 12)
    return vk_format, width, height, levels


def decode_base_level(path):
    """RGBA pixels of a zstd-compressed BC7 KTX2's first level, decoded through a DDS header."""
    import zstandard

    data = path.read_bytes()
    width, height = struct.unpack_from("<2I", data, 20)
    offset, length, raw_length = struct.unpack_from("<3Q", data, 80)  # the first level index entry
    blocks = zstandard.ZstdDecompressor().decompress(data[offset : offset + length], max_output_size=raw_length)
    header = b"DDS " + struct.pack("<7I", 124, 0x81007, height, width, len(blocks), 0, 1) + bytes(44)
    header += struct.pack("<2I4s5I", 32, 4, b"DX10", 0, 0, 0, 0, 0) + struct.pack("<5I", 0x1000, 0, 0, 0, 0)
    header += struct.pack("<5I", 98, 3, 0, 1, 0)  # DXGI_FORMAT_BC7_UNORM, a 2D texture
    return np.asarray(Image.open(io.BytesIO(header + blocks)).convert("RGBA"))


def decode(normal, channel):
    return np.asarray(normal, dtype=np.float64)[..., channel] / 255 * 2 - 1


class NormalFromHeight(unittest.TestCase):
    def test_normal_from_height_points_downhill(self):
        ramp = np.tile(np.linspace(0, 255, 64), (64, 1)).astype(np.uint8)
        normal = asphalt.normal_from_height(Image.fromarray(ramp, "L"))
        self.assertLess(decode(normal, 0)[32, 32], 0)
        self.assertGreater(decode(normal, 2)[32, 32], 0)

    def test_normal_green_is_height_change_per_row(self):
        # y runs up the texture, against the image rows (OpenGL), so a height
        # that rises toward the bottom rows decodes to +y.
        ramp = np.tile(np.linspace(0, 255, 64)[:, None], (1, 64)).astype(np.uint8)
        normal = asphalt.normal_from_height(Image.fromarray(ramp, "L"))
        self.assertGreater(decode(normal, 1)[32, 32], 0)
        self.assertAlmostEqual(decode(normal, 0)[32, 32], 0, places=2)

    def test_flat_height_is_a_flat_normal(self):
        normal = asphalt.normal_from_height(Image.new("L", (16, 16), 90))
        self.assertEqual(set(map(tuple, np.asarray(normal).reshape(-1, 3))), {(128, 128, 255)})


class Rectify(unittest.TestCase):
    def test_rectify_maps_the_quad_corners_to_the_output_corners(self):
        photo = Image.new("RGB", (200, 120), (20, 20, 20))
        quad = ((60, 30), (140, 30), (190, 100), (10, 100))  # a ground rectangle seen obliquely
        marks = ((255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0))
        for (x, y), colour in zip(quad, marks):
            photo.paste(colour, (x - 15, y - 15, x + 16, y + 16))
        flat = np.asarray(asphalt.rectify(photo, quad, (100, 80)))
        for (x, y), colour in zip(((3, 3), (96, 3), (96, 76), (3, 76)), marks):
            near = flat[y - 1 : y + 2, x - 1 : x + 2].reshape(-1, 3).mean(axis=0)
            self.assertLess(np.abs(near - colour).max(), 60, (x, y, near, colour))


class GrainMaps(unittest.TestCase):
    def test_grain_tiles_without_a_seam(self):
        albedo = np.asarray(asphalt.grain_maps(synthetic_photo(256, 70))[0], dtype=np.float64)
        interior = np.abs(np.diff(albedo, axis=1)).mean()
        across_columns = np.abs(albedo[:, 0] - albedo[:, -1]).mean()
        interior_rows = np.abs(np.diff(albedo, axis=0)).mean()
        across_rows = np.abs(albedo[0] - albedo[-1]).mean()
        self.assertLessEqual(across_columns, 1.5 * interior)
        self.assertLessEqual(across_rows, 1.5 * interior_rows)

    def test_every_grain_map_tiles(self):
        # The normal and the surface are derived from the tiled albedo with wrapping filters.
        normal, surface = asphalt.grain_maps(synthetic_photo(256, 70))[1:]
        for image in (normal, surface):
            data = np.asarray(image, dtype=np.float64)
            self.assertLessEqual(np.abs(data[:, 0] - data[:, -1]).mean(), 1.5 * np.abs(np.diff(data, axis=1)).mean())
            self.assertLessEqual(np.abs(data[0] - data[-1]).mean(), 1.5 * np.abs(np.diff(data, axis=0)).mean())

    def test_albedo_mean_is_dark_asphalt(self):
        for grey in (30, 70, 200):
            with self.subTest(photo_grey=grey):
                albedo = asphalt.grain_maps(synthetic_photo(256, grey))[0]
                self.assertTrue(0.06 <= linear_luminance(albedo) <= 0.12, linear_luminance(albedo))

    def test_surface_packs_ao_roughness_height_with_one_at_the_top(self):
        surface = np.asarray(asphalt.grain_maps(synthetic_photo(256, 70))[2], dtype=np.float64) / 255
        ao, roughness, height = (surface[..., channel] for channel in range(3))
        self.assertGreater(height.max() - height.min(), 0.4)  # the grain relief uses most of the range
        self.assertGreater(roughness.min(), 0.4)  # asphalt is never glossy
        self.assertGreater(ao.min(), 0.2)  # cavities darken, they do not black out
        self.assertGreater(ao.mean(), 0.7)


def crack_photo(size=400):
    """Grey noisy asphalt with one dark crack running across and out through the right edge."""
    photo = np.full((size, size), 90.0)
    photo += 8 * smooth_noise(size, 1.2, 5)
    columns = np.arange(size)
    rows = (size * 0.45 + 0.12 * (columns - size / 2) + 14 * np.sin(columns / 23)).astype(int)
    for width in range(10):  # as wide as the real crack, so it survives the smoothing
        photo[np.clip(rows + width, 0, size - 1), columns] = 6
    photo[30:44, 40:54] = 8  # a dark pit apart from the crack
    return Image.fromarray(photo.astype(np.uint8), "L").convert("RGB")


class CrackDecal(unittest.TestCase):
    def test_crack_border_is_transparent(self):
        decal = np.asarray(asphalt.crack_decal(crack_photo(), (0, 0, 400, 400)))
        alpha = decal[..., 3]
        self.assertGreater(alpha.max(), 200)  # a crack was found
        border = np.ones(alpha.shape, bool)
        border[8:-8, 8:-8] = False
        self.assertEqual(int(alpha[border].max()), 0)

    def test_crack_is_one_connected_crack_without_the_pit(self):
        decal = np.asarray(asphalt.crack_decal(crack_photo(), (0, 0, 400, 400)))
        mask = decal[..., 3] > 0
        self.assertEqual(asphalt.label_components(mask)[1], 1)

    def test_crack_packs_flat_normal_depth_and_mask(self):
        decal = np.asarray(asphalt.crack_decal(crack_photo(), (0, 0, 400, 400))).astype(np.float64)
        outside = decal[..., 3] == 0
        # Off the crack the normal is flat, give or take the faint slope (under 2 degrees)
        # of the depth's blurred tail next to the mask.
        self.assertLessEqual(np.abs(decal[outside][:, :2] - 128).max(), 4)
        self.assertTrue(np.all(decal[outside][:, 2] == 0))  # no depth off the crack
        deepest = decal[..., 2].max()
        self.assertGreater(deepest, 200)  # the crack reaches full depth somewhere
        along = decal[decal[..., 3] > 200]
        self.assertGreater(along[:, 2].mean(), 60)  # inside the mask it is mostly depressed
        self.assertGreater(np.abs(decal[..., :2] - 128).max(), 20)  # the walls tilt the normal


@unittest.skipUnless(shutil.which("ctt"), "needs the ctt CLI")
class EncodeKtx2(unittest.TestCase):
    def encode(self, image, role):
        folder = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, folder)
        image.save(folder / "map.png")
        asphalt.encode_ktx2(folder / "map.png", folder / "map.ktx2", role)
        return folder / "map.ktx2"

    def test_maps_have_full_mips_and_the_right_colour_space(self):
        image = Image.fromarray(np.random.default_rng(3).integers(0, 255, (64, 64, 3), dtype=np.uint8), "RGB")
        bc7_unorm, bc7_srgb = 145, 146
        self.assertEqual(ktx2_header(self.encode(image, "color")), (bc7_srgb, 64, 64, 7))
        self.assertEqual(ktx2_header(self.encode(image, "data")), (bc7_unorm, 64, 64, 7))

    def test_crack_alpha_is_data_so_the_flat_normal_survives_where_it_is_zero(self):
        try:
            import zstandard  # noqa: F401
        except ImportError:
            self.skipTest("needs the zstandard module to read the maps back")
        decal = np.zeros((64, 64, 4), np.uint8)
        decal[..., :2] = 128  # a flat normal under zero alpha
        decal[20:40, 20:40] = (60, 200, 220, 255)
        pixels = decode_base_level(self.encode(Image.fromarray(decal, "RGBA"), "data"))
        self.assertTrue(np.all(np.abs(pixels[:10, :10, :2].astype(int) - 128) <= 2), pixels[0, 0])
        self.assertLessEqual(abs(int(pixels[30, 30, 2]) - 220), 12)


if __name__ == "__main__":
    unittest.main()
