"""Bake poured concrete and extract hangar reference swatches (requires Pillow).

python3 tools/prepare_hangar_textures.py image_3.png image_2.png image_1.png \
    image_0.png assets/textures/base
"""

import argparse
import math
import random
from pathlib import Path

from PIL import Image, ImageOps

from prepare_terrain_textures import tileable


SIZE = 192
CONCRETE_SIZE = 512
# Board, then source bounds of albedo, normal, roughness, metallic and AO.
SURFACES = {
    "wall": (
        2, (54, 647, 110, 759), (220, 647, 277, 759), (388, 647, 445, 759),
        (555, 647, 612, 759), (722, 647, 779, 759),
    ),
    "steel": (
        0, (47, 608, 91, 665), (216, 608, 260, 665), (384, 608, 428, 665),
        (550, 608, 594, 665), (717, 608, 761, 665),
    ),
}


def poured_concrete():
    """Seamless, slightly polished concrete without plate seams or painted marks."""
    rng = random.Random(0xC0C2E7E)
    size = CONCRETE_SIZE

    def noise(cells):
        samples = [rng.uniform(-1, 1) for _ in range(cells * cells)]
        pad = 2
        width = cells + 2 * pad
        field = Image.new("F", (width, width))
        field.putdata([
            samples[((y - pad) % cells) * cells + (x - pad) % cells]
            for y in range(width)
            for x in range(width)
        ])
        return field.resize(
            (size, size), Image.Resampling.BICUBIC, box=(pad, pad, pad + cells, pad + cells)
        ).getdata()

    albedo, arm, height = [], [], []
    for broad, mottle, grain, pores in zip(noise(5), noise(19), noise(73), noise(251)):
        grey = round(111 + 17 * broad + 10 * mottle + 5 * grain + 3 * pores)
        albedo.append((grey - 2, grey, grey + 2))
        arm.append((round(239 + 6 * grain + 4 * pores), round(137 + 24 * broad + 23 * mottle), 0))
        height.append(0.65 * mottle + 0.56 * grain + 0.16 * pores)

    normal = []
    for y in range(size):
        for x in range(size):
            nx = (height[y * size + (x - 1) % size] - height[y * size + (x + 1) % size]) * 0.22
            ny = (height[((y + 1) % size) * size + x] - height[((y - 1) % size) * size + x]) * 0.22
            length = math.sqrt(nx * nx + ny * ny + 1.0)
            normal.append(tuple(round((component / length + 1.0) * 127.5) for component in (nx, ny, 1.0)))

    for name, pixels in (("albedo", albedo), ("normal", normal), ("arm", arm)):
        image = Image.new("RGB", (size, size))
        image.putdata(pixels)
        yield image, "concrete_" + name


def build(boards, output):
    def sample(index, box, mode="RGB", seamless=True):
        image = boards[index].crop(box).convert(mode).resize((SIZE, SIZE), Image.Resampling.LANCZOS)
        return tileable(image) if seamless else image

    def save(image, name):
        image.save(output / f"{name}.png", optimize=True)

    for image, name in poured_concrete():
        save(image, name)

    for name, (board, albedo, normal, roughness, metallic, ao) in SURFACES.items():
        save(sample(board, albedo), name + "_albedo")
        save(sample(board, normal), name + "_normal")
        rough = sample(board, roughness, "L").point(lambda x: round(46 + x * 0.74))
        metal = sample(board, metallic, "L")
        ambient = sample(board, ao, "L").point(lambda x: round(78 + x * 0.69))
        save(Image.merge("RGB", (ambient, rough, metal)), name + "_arm")

    save(sample(3, (11, 579, 45, 625)), "grate_albedo")
    wear = sample(3, (523, 930, 617, 992), "L")
    save(ImageOps.colorize(wear, "#603c1d", "#e7ae31"), "yellow_albedo")
    clean = ImageOps.colorize(wear, "#644721", "#f7ba36")
    dark = ImageOps.colorize(wear, "#191718", "#3c3631")
    mask = Image.new("L", (SIZE, SIZE))
    mask.putdata([255 if ((x + y) % SIZE) < SIZE // 2 else 0 for y in range(SIZE) for x in range(SIZE)])
    save(Image.composite(clean, dark, mask), "hazard_albedo")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("boards", nargs=4, type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    boards = [Image.open(path).convert("RGB") for path in args.boards]
    if any(board.size != (1024, 1024) for board in boards):
        parser.error("Expected four 1024 x 1024 reference sheets")
    args.output.mkdir(parents=True, exist_ok=True)
    build(boards, args.output)
