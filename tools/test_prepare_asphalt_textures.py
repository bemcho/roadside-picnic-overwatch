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


PHOTO_SIZE = (1408, 768)  # image_5b974df1.jpg


def photo_pixels_per_output_pixel(box):
    """Photo pixels stepped per output pixel along u and along v, at every pixel of `box`.

    The photo's own coordinates are rectified as two float images (its column and its row
    at each pixel), cropped like the real maps, and differentiated: where the step is
    under 1 photo pixel the map is made of interpolated, not photographed, pixels.
    """
    width, height = PHOTO_SIZE
    columns = np.tile(np.arange(width, dtype=np.float32), (height, 1))
    rows = np.tile(np.arange(height, dtype=np.float32)[:, None], (1, width))
    where_x, where_y = (
        np.asarray(asphalt.rectify(Image.fromarray(plane, "F")).crop(box), np.float64) for plane in (columns, rows)
    )
    along_u = np.hypot(np.diff(where_x, axis=1), np.diff(where_y, axis=1))
    along_v = np.hypot(np.diff(where_x, axis=0), np.diff(where_y, axis=0))
    return along_u.min(), along_v.min()


class NativeResolution(unittest.TestCase):
    """Ruling A9: no step may upsample the photo; every map is made of its own pixels."""

    def test_the_grain_crop_is_not_upsampled(self):
        for axis, step in zip("uv", photo_pixels_per_output_pixel(asphalt.GRAIN_BOX)):
            self.assertGreaterEqual(step, 1.0, f"grain box: {step:.2f} photo px per output px along {axis}")

    def test_the_crack_crop_is_not_upsampled(self):
        for axis, step in zip("uv", photo_pixels_per_output_pixel(asphalt.CRACK_BOX)):
            self.assertGreaterEqual(step, 1.0, f"crack box: {step:.2f} photo px per output px along {axis}")

    def test_maps_are_native_crops_in_whole_bc7_blocks(self):
        self.assertEqual(asphalt.MAP_SIZE % 4, 0)
        for box in (asphalt.GRAIN_BOX, asphalt.CRACK_BOX):
            left, top, right, bottom = box
            self.assertEqual((right - left, bottom - top), (asphalt.MAP_SIZE, asphalt.MAP_SIZE))
            width, height = asphalt.RECTIFIED_SIZE
            self.assertTrue(0 <= left and right <= width and 0 <= top and bottom <= height, box)

    def test_the_grain_maps_keep_the_size_of_the_crop(self):
        for map_ in asphalt.grain_maps(synthetic_photo(asphalt.MAP_SIZE, 70)):
            self.assertEqual(map_.size, (asphalt.MAP_SIZE, asphalt.MAP_SIZE))


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


def crack_photo(size=192):
    """Grey noisy asphalt with one dark crack running across and out through the right edge.

    Native density: the real crack is about 4 px wide, its stones 3 to 8 px.
    """
    photo = np.full((size, size), 90.0)
    photo += 8 * smooth_noise(size, 0.6, 5)
    columns = np.arange(size)
    rows = (size * 0.45 + 0.12 * (columns - size / 2) + 5 * np.sin(columns / 8)).astype(int)
    for width in range(4):  # as wide as the real crack, so it survives the smoothing
        photo[np.clip(rows + width, 0, size - 1), columns] = 6
    photo[14:20, 20:26] = 8  # a dark pit apart from the crack
    return Image.fromarray(photo.astype(np.uint8), "L").convert("RGB")


def border_mask(size, margin):
    """Mask of the pixels within `margin` px of the border of a size x size image."""
    border = np.ones((size, size), bool)
    border[margin:-margin, margin:-margin] = False
    return border


class CrackDecal(unittest.TestCase):
    def test_crack_decal_is_cut_at_the_size_of_its_crop(self):
        for size in (144, 192):
            self.assertEqual(asphalt.crack_decal(crack_photo(size)).size, (size, size))

    def test_crack_border_is_transparent(self):
        alpha = np.asarray(asphalt.crack_decal(crack_photo()))[..., 3]
        self.assertGreater(alpha.max(), 200)  # a crack was found
        self.assertEqual(int(alpha[border_mask(alpha.shape[0], asphalt.DECAL_MARGIN)].max()), 0)

    def test_crack_is_one_connected_crack_without_the_pit(self):
        decal = np.asarray(asphalt.crack_decal(crack_photo()))
        mask = decal[..., 3] > 0
        self.assertEqual(asphalt.label_components(mask)[1], 1)

    def test_crack_packs_flat_normal_depth_and_mask(self):
        decal = np.asarray(asphalt.crack_decal(crack_photo())).astype(np.float64)
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

    def test_crack_decal_is_transparent_at_the_border_and_opaque_on_the_crack_after_encoding(self):
        decal = asphalt.crack_decal(crack_photo())
        pixels = decode_base_level(self.encode(decal, "data"))
        assert_decal_alpha(self, pixels[..., 3], np.asarray(decal)[..., 3])


def assert_decal_alpha(test, decoded, source):
    """In the `decoded` alpha: the border (DECAL_MARGIN px) is 0 and the crack's core, the pixels
    that were opaque in `source`, is opaque (BC7 is lossy: its mean and its worst pixel are bounded)."""
    test.assertEqual(int(decoded[border_mask(decoded.shape[0], asphalt.DECAL_MARGIN)].max()), 0)
    core = source == 255
    test.assertTrue(core.any(), "the source decal has no opaque crack")
    test.assertGreaterEqual(decoded[core].mean(), 245)
    test.assertGreaterEqual(int(decoded[core].min()), 200)


class ShippedMaps(unittest.TestCase):
    CRACK = Path(__file__).resolve().parent.parent / "assets/textures/terrain/asphalt_crack.ktx2"

    def test_the_shipped_crack_decal_decodes_transparent_at_the_border_and_opaque_on_the_crack(self):
        try:
            import zstandard  # noqa: F401
        except ImportError:
            self.skipTest("needs the zstandard module to read the maps back")
        if not self.CRACK.exists():
            self.skipTest("the maps have not been baked yet")
        pixels = decode_base_level(self.CRACK)
        self.assertEqual(pixels.shape[:2], (asphalt.MAP_SIZE, asphalt.MAP_SIZE))
        alpha = pixels[..., 3]
        self.assertEqual(int(alpha[border_mask(alpha.shape[0], asphalt.DECAL_MARGIN)].max()), 0)
        self.assertGreaterEqual(int(alpha.max()), 250)  # the crack is opaque somewhere
        self.assertGreater(int((alpha > 128).sum()), 300)  # and is a real crack, not a speck


if __name__ == "__main__":
    unittest.main()
