"""Bake the terrain's rock layer from a rock scan's texture (requires Pillow, numpy and the ctt CLI).

The terrain shader dresses steep ground (cut banks, fill faces, quarry walls) in rock instead
of the ground layers, whose top-down look streaks down a near-vertical face. The maps are one
native square of a rock scan's base colour (Ruling A9: nothing is upsampled): a clean island of
fractured stone in rock-17m, a rock the game places. Its slow baked shading is divided out,
its edges are blended to tile, and its colour is graded to the mean colour of the two cut
scans' stone (the Golden Sphere's crater and the Quarry rim's pit), so a bank meets the scan
stone above it in the same colour. Each map ships as KTX2 with a full mip chain (its 1 x 1
mip is the map's average); the PNGs they were encoded from stay in the assets workspace.

    python3 tools/prepare_rock_textures.py \
        ~/Projects/agentic/RoadSide-Picnic/models/rocks/uhd/rock-17m.glb \
        assets/textures/terrain --sources ~/Projects/agentic/RoadSide-Picnic/textures/rock

Output maps (the same layout as the asphalt grain maps):
  rock_albedo   sRGB colour, tileable
  rock_normal   RGB tangent normal, y up the texture (OpenGL), tileable
  rock_surface  R ambient occlusion, G roughness, B height (1 at the top), tileable
"""

import argparse
import io
from pathlib import Path

import numpy as np
from PIL import Image

from compress_glb_textures import read_glb
from prepare_asphalt_textures import LUMINANCE, blur, encode_ktx2, flatten_lighting, normal_from_height, to_linear, to_srgb
from prepare_terrain_textures import tileable

# rock-17m's base colour (4096 x 4096): the square inside its largest island of blue-grey,
# fractured plates, clear of the island's smeared border and of the deep shaded crease above it.
CROP = (1210, 1470, 1530, 1790)  # left, top, right, bottom: 320 px, a multiple of 4 for BC7 blocks
# Mean linear colour of the cut scans' base colour textures (landmark-core 0.0434 0.0412 0.0348,
# landmark-quarry-pit 0.0702 0.0547 0.0370, each over its whole 8K atlas), averaged.
STONE = np.array([0.0568, 0.0479, 0.0359], np.float32)
HEIGHT_BLUR = 8.0  # px (Gaussian sigma): the plates' relief is what stays above this blur
HEIGHT_SIGMAS = 3.0  # luminance deviations that span the height range
CAVITY_BLUR = 2.0  # px
AO_STRENGTH = 0.6
ROUGHNESS = (0.7, 0.95)  # shaded cracks .. dusty faces
NORMAL_STRENGTH = 3.0


def base_colour(glb):
    """The first material's base colour image of a GLB."""
    gltf, binary = read_glb(glb)
    texture = gltf["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    view = gltf["bufferViews"][gltf["images"][gltf["textures"][texture]["source"]]["bufferView"]]
    start = view.get("byteOffset", 0)
    return Image.open(io.BytesIO(binary[start : start + view["byteLength"]])).convert("RGB")


def grade(linear, target=STONE):
    """Scale each channel so the map's mean is `target`, keeping its relative variation."""
    return linear * (target / linear.reshape(-1, 3).mean(axis=0))


def rock_maps(patch):
    """(albedo, normal, surface) from a square native patch; all three tile."""
    albedo = to_srgb(grade(to_linear(tileable(flatten_lighting(patch)))))
    luminance = to_linear(albedo) @ LUMINANCE
    luminance /= luminance.mean()

    detail = luminance - blur(luminance, HEIGHT_BLUR, wrap=True)
    height = np.clip(0.5 + detail / (2 * HEIGHT_SIGMAS * detail.std()), 0, 1)

    cavity = np.maximum(blur(luminance, CAVITY_BLUR, wrap=True) - luminance, 0)
    ao = 1 - AO_STRENGTH * np.clip(cavity / (HEIGHT_SIGMAS * cavity.std()), 0, 1)
    roughness = ROUGHNESS[0] + (ROUGHNESS[1] - ROUGHNESS[0]) * height

    surface = np.dstack([ao, roughness, height])
    normal = normal_from_height(Image.fromarray(np.round(height * 255).astype(np.uint8), "L"), NORMAL_STRENGTH)
    return albedo, normal, Image.fromarray(np.round(surface * 255).astype(np.uint8), "RGB")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("glb", type=Path, help="the rock scan (rock-17m.glb, UHD tier)")
    parser.add_argument("output", type=Path, help="folder for the KTX2 maps")
    parser.add_argument("--sources", type=Path, required=True, help="folder for the source PNGs")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    args.sources.mkdir(parents=True, exist_ok=True)

    patch = base_colour(args.glb).crop(CROP)
    patch.save(args.sources / "rock_crop.png")
    albedo, normal, surface = rock_maps(patch)
    for name, image, role in (("albedo", albedo, "color"), ("normal", normal, "data"), ("surface", surface, "data")):
        png = args.sources / f"rock_{name}.png"
        image.save(png, optimize=True)
        encode_ktx2(png, args.output / f"rock_{name}.ktx2", role)
        print(f"rock_{name}: {image.size[0]}x{image.size[1]} {role}")


if __name__ == "__main__":
    main()
