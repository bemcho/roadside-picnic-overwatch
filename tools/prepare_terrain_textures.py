"""Crop and edge-blend the terrain reference swatches (requires Pillow).

python3 tools/prepare_terrain_textures.py terrain_texture_image_2.webp \
    terrain_texture_1.webp assets/textures/terrain
"""

import argparse
from pathlib import Path

from PIL import Image, ImageChops


# Albedo, tangent-space normal, roughness, AO, and height, in sheet pixels.
CROPS = {
    "grass": (
        (395, 32, 532, 175), (548, 32, 686, 175), (705, 32, 847, 175),
        (868, 32, 1010, 175), (397, 207, 531, 347),
    ),
    "earth": (
        (391, 9, 536, 154), (703, 9, 851, 154), (547, 9, 694, 154),
        (391, 203, 536, 348), (863, 9, 1008, 154),
    ),
}
SIZE = 128
DETAIL_CROPS = {
    "grass": (277, 756, 499, 959),
    "earth": (806, 663, 993, 914),
}


def tileable(image):
    size = image.width

    def weight(x):
        length = min(x, size - x)
        t = min(1.0, length / (size * 0.2))
        return round(255 * t * t * (3.0 - 2.0 * t))

    strip = Image.new("L", (size, 1))
    strip.putdata([weight(x) for x in range(size)])
    mask = strip.resize(image.size, Image.Resampling.NEAREST)
    # Each edge samples the middle of a second, offset copy so opposing
    # edges meet without a seam, while preserving the original center.
    for dx, dy, blend in ((size // 2, 0, mask), (0, size // 2, mask.transpose(Image.Transpose.TRANSPOSE))):
        image = Image.composite(image, ImageChops.offset(image, dx, dy), blend)
    return image


def prepare(path, detail_path, layer, output):
    with Image.open(path) as sheet:
        if sheet.size != (1024, 1024):
            raise ValueError(f"{path} must be a 1024 x 1024 terrain reference sheet")
        albedo, normal, roughness, ao, height = [
            tileable(sheet.crop(box).convert(mode).resize((SIZE, SIZE), Image.Resampling.LANCZOS))
            for box, mode in zip(CROPS[layer], ("RGB", "RGB", "L", "L", "L"))
        ]
    with Image.open(detail_path) as sheet:
        detail = sheet.crop(DETAIL_CROPS[layer]).convert("RGB")
        detail = tileable(detail.resize((SIZE, SIZE), Image.Resampling.LANCZOS))
        albedo = Image.blend(albedo, detail, 0.32)
    surface = Image.merge("RGB", (ao, roughness, height))
    for kind, image in (("albedo", albedo), ("normal", normal), ("surface", surface)):
        image.save(output / f"{layer}_{kind}.png", optimize=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mossy", type=Path)
    parser.add_argument("bare", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    prepare(args.mossy, args.bare, "grass", args.output)
    prepare(args.bare, args.mossy, "earth", args.output)
