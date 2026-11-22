"""Cut the hangar floor's tiling textures from the industrial hall reference board.

python3 tools/prepare_hangar_floor.py assets/models/hangar/image_1.png \
    assets/textures/hangar

`image_1.png` is the 1024 x 1024 industrial hall reference board with its PBR
swatches of weathered steel floor plates. It is in the unused-asset archive
(see Asset tiers in CLAUDE.md); restore it before running this. Each swatch is
cropped left of its painted yellow stripe (the shader paints the hall's lines),
squared, edge-blended to tile and written as albedo, normal, and a surface map
packing AO, roughness and metallic like the terrain's. Requires Pillow.

`prepare_hangar_textures.py` bakes the older, fuller set (poured concrete,
wall and steel plates, grate, yellow and hazard paint) into `textures/base`,
which the game no longer loads.
"""

import argparse
from pathlib import Path

from PIL import Image

from prepare_terrain_textures import SIZE, tileable

# Albedo, tangent-space normal, roughness, metallic and AO swatches share one
# layout on the board, 168 px apart: the square left of the yellow stripe.
LEFT, TOP, SIDE, STEP = 18, 650, 94, 168.25


def swatch(board, index, mode):
    left = round(LEFT + index * STEP)
    box = (left, TOP, left + SIDE, TOP + SIDE)
    image = board.crop(box).convert(mode).resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    return tileable(image)


def prepare(path, output):
    with Image.open(path) as board:
        if board.size != (1024, 1024):
            raise ValueError(f"{path} must be the 1024 x 1024 hangar reference board")
        albedo, normal = swatch(board, 0, "RGB"), swatch(board, 1, "RGB")
        roughness, metallic, ao = (swatch(board, index, "L") for index in (2, 3, 4))
    surface = Image.merge("RGB", (ao, roughness, metallic))
    for kind, image in (("albedo", albedo), ("normal", normal), ("surface", surface)):
        image.save(output / f"floor_{kind}.png", optimize=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("board", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    prepare(args.board, args.output)
