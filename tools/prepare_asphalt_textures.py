"""Bake the old-asphalt maps from one oblique photo (requires Pillow, numpy and the ctt CLI).

The terrain shader paints every `Surface::Asphalt` road with a tiling grain set plus
scattered crack decals. Every map is a native crop of the photo (Ruling A9: nothing is
upsampled, so MAP_SIZE x MAP_SIZE texels, one photographed pixel or more each) and ships
as KTX2 with a full mip chain (`ctt` presets from compress_glb_textures.py); the PNGs they
were encoded from stay in the assets workspace.

    python3 tools/prepare_asphalt_textures.py \
        ~/Projects/agentic/RoadSide-Picnic/textures/asphalt/image_5b974df1.jpg \
        assets/textures/terrain --sources ~/Projects/agentic/RoadSide-Picnic/textures/asphalt

Output maps. Tangent x runs along u (right) and y UP the texture, against the image rows
(OpenGL), so a normal's green is +dh/drow. The crack's RG uses the same frame, so one
decode serves both normals; a shader that rotates the decal rotates that xy with it.
  asphalt_albedo   sRGB colour, tileable
  asphalt_normal   RGB tangent normal, tileable
  asphalt_surface  R ambient occlusion, G roughness, B height (1 at the top), tileable
  asphalt_crack    RG tangent normal xy (0.5 is flat), B crack depth (0 at the surface,
                   1 deepest), A crack mask; transparent at the border so a decal never
                   shows its cell edge. All four channels are data: alpha is not blended.
"""

import argparse
import subprocess
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

from compress_glb_textures import ctt_arguments
from prepare_terrain_textures import tileable

# Photo pixel coordinates (image_5b974df1.jpg, 1408 x 768: sharp in its lower half).
# The road is seen obliquely, so a photo row covers about 1.5 to 2 times more ground than a
# photo column here, and the farther rows hold less detail still. The strip flattened here
# is the nearest ground that is still wide enough for the gravel and the long thin crack:
# photo rows 533-762, columns 600-1397 at the bottom (a few pixels inside the photo, where
# resampling needs neighbours), clockwise from the top left. Its sides converge upward.
SHARP_QUAD = ((617, 533), (1286, 533), (1397, 762), (600, 762))
# 0.42 output pixels per ground unit of the earlier 1408 x 1000 flattening of rows 340-768,
# which made pebbles round. At that scale the strip's far edge (the worst row) steps 1.02
# photo pixels per output pixel along the rows (2 along the columns), so rectify never
# upsamples; test_prepare_asphalt_textures.py measures it.
RECTIFIED_SIZE = (336, 189)
MAP_SIZE = 184  # px: the native crop of every map, a multiple of 4 for BC7 blocks
# Boxes in the rectified image, (left, top, right, bottom).
GRAIN_BOX = (152, 3, 152 + MAP_SIZE, 3 + MAP_SIZE)  # crack-free gravel at the bottom right
CRACK_BOX = (36, 2, 36 + MAP_SIZE, 2 + MAP_SIZE)  # the long thin crack, running down through it

# The pixel constants below are the values tuned at the first 512 px maps (1.28 output
# pixels per ground unit) times 0.33, the ratio to the 0.42 of the native strip, so each
# keeps its width in real size: the old value is in the comment.
LIGHTING_BLUR = 0.15  # sigma of the wide blur that is divided out, as a fraction of the width
ALBEDO_LUMINANCE = 0.085  # mean linear luminance: weathered, dark asphalt
ALBEDO_CHROMA = 0.5  # share of the photo's colour kept; the tan pebbles would repeat like landmarks
HEIGHT_BLUR = 3.3  # px (Gaussian sigma); the grain relief is what stays above this blur (was 10)
HEIGHT_SIGMAS = 3.0  # luminance deviations that span the height range
CAVITY_BLUR = 1.0  # (was 3)
AO_STRENGTH = 0.65
ROUGHNESS = (0.6, 0.95)  # tar binder .. dusty stone
NORMAL_STRENGTH = 2.0  # normal slope per unit of height change per pixel (was 6)
CRACK_SMOOTH = 1.5  # px (sigma); blurs away the narrow gaps between stones, so only a wide dark line is left (was 3;
# the rescaled 1.0 no longer blurs a 2 px gap away at this density)
CRACK_DARK = 24  # luminance (0..255) below which smoothed pixels belong to a crack
CRACK_JOIN = 7  # px; width of the dilation filter that joins the crack's broken pieces (3 px each side); the mask
# is shrunk back by CRACK_JOIN - 2, so it keeps the old net growth of 1 px each side (was a 3 px dilation)
DEPTH_BLUR = 1.0  # px (sigma); the depth is the mask blurred (was 3)
DEPTH_ALPHA = 0.5  # depth at which the mask is fully opaque
CRACK_NORMAL_STRENGTH = 3.3  # (was 10)
DECAL_MARGIN = 4  # px kept fully transparent at the decal's border: one BC7 block (was 12)
DECAL_FADE = 12  # px over which the crack fades in from the margin (was 36)

KTX2_QUALITY = "slow"
LUMINANCE = np.array([0.2126, 0.7152, 0.0722], np.float32)


def perspective_coefficients(output_corners, source_corners):
    """The eight coefficients PIL wants: they map output pixels back to source pixels."""
    rows, values = [], []
    for (x, y), (u, v) in zip(output_corners, source_corners):
        rows.append([x, y, 1, 0, 0, 0, -u * x, -u * y])
        rows.append([0, 0, 0, x, y, 1, -v * x, -v * y])
        values += [u, v]
    return tuple(np.linalg.solve(rows, values))


def rectify(photo, corners=SHARP_QUAD, size=RECTIFIED_SIZE):
    """Flatten the ground quad `corners` (clockwise from the top left) of an oblique photo."""
    width, height = size
    output = ((0, 0), (width, 0), (width, height), (0, height))
    coefficients = perspective_coefficients(output, corners)
    return photo.transform(size, Image.Transform.PERSPECTIVE, coefficients, Image.Resampling.BICUBIC)


def to_linear(image):
    srgb = np.asarray(image.convert("RGB"), np.float32) / 255
    return np.where(srgb <= 0.04045, srgb / 12.92, ((srgb + 0.055) / 1.055) ** 2.4).astype(np.float32)


def to_srgb(linear):
    linear = np.clip(linear, 0, 1)
    srgb = np.where(linear <= 0.0031308, linear * 12.92, 1.055 * linear ** (1 / 2.4) - 0.055)
    return Image.fromarray(np.round(srgb * 255).astype(np.uint8), "RGB")


def blur(values, sigma, wrap=False):
    """Gaussian blur of a 2D float array in the frequency domain; `wrap` treats it as a tile."""
    pad = 0 if wrap else int(3 * sigma) + 1
    padded = values if wrap else np.pad(values, pad, mode="reflect")
    freq_y = np.fft.fftfreq(padded.shape[0])[:, None]
    freq_x = np.fft.fftfreq(padded.shape[1])[None, :]
    gaussian = np.exp(-2 * (np.pi * sigma) ** 2 * (freq_x**2 + freq_y**2))
    blurred = np.fft.ifft2(np.fft.fft2(padded) * gaussian).real.astype(np.float32)
    return blurred if wrap else blurred[pad:-pad, pad:-pad]


def flatten_lighting(image):
    """Divide out the slow lighting (a wide blur) so the grain is evenly lit, keeping the mean."""
    linear = to_linear(image)
    luminance = linear @ LUMINANCE
    wide = blur(luminance, LIGHTING_BLUR * image.width)
    return to_srgb(linear * (luminance.mean() / np.maximum(wide, 1e-4))[..., None])


def grade_albedo(linear):
    """Neutral grey (the photo has a blue cast) at the mean luminance of weathered asphalt."""
    neutral = linear * (linear.mean() / linear.mean(axis=(0, 1)))
    grey = (neutral @ LUMINANCE)[..., None]
    muted = grey + ALBEDO_CHROMA * (neutral - grey)
    return muted * (ALBEDO_LUMINANCE / grey.mean())


def normal_from_height(height, strength=NORMAL_STRENGTH):
    """Tangent normal `normalize(-dh/dx, -dh/dy, 1)` with y up the texture (OpenGL).

    Rows run down the image, so green is +dh/drow. `height` is an "L" (0..255) or "F"
    (0..1) image. Gradients wrap: the grain maps tile, and the crack decal is flat at its
    border.
    """
    values = np.asarray(height, np.float32)
    if height.mode == "L":
        values = values / 255
    slope_x = (np.roll(values, -1, axis=1) - np.roll(values, 1, axis=1)) / 2 * strength
    slope_rows = (np.roll(values, -1, axis=0) - np.roll(values, 1, axis=0)) / 2 * strength
    normal = np.dstack([-slope_x, slope_rows, np.ones_like(values)])
    normal /= np.linalg.norm(normal, axis=2, keepdims=True)
    return Image.fromarray(np.floor((normal * 0.5 + 0.5) * 255 + 0.5).astype(np.uint8), "RGB")


def grain_maps(patch):
    """(albedo, normal, surface) from a crack-free square patch (MAP_SIZE px); all three tile."""
    albedo_image = to_srgb(grade_albedo(to_linear(tileable(flatten_lighting(patch)))))
    luminance = np.asarray(albedo_image.convert("L"), np.float32) / 255

    detail = luminance - blur(luminance, HEIGHT_BLUR, wrap=True)
    spread = HEIGHT_SIGMAS * detail.std()
    height = np.clip(0.5 + detail / (2 * spread), 0, 1)

    cavity = np.maximum(blur(luminance, CAVITY_BLUR, wrap=True) - luminance, 0)
    ao = 1 - AO_STRENGTH * np.clip(cavity / (HEIGHT_SIGMAS * cavity.std()), 0, 1)

    brightness = np.clip(0.5 + (luminance - luminance.mean()) / (2 * HEIGHT_SIGMAS * luminance.std()), 0, 1)
    roughness = ROUGHNESS[0] + (ROUGHNESS[1] - ROUGHNESS[0]) * brightness

    surface = np.dstack([ao, roughness, height])
    normal = normal_from_height(Image.fromarray(np.round(height * 255).astype(np.uint8), "L"))
    return albedo_image, normal, Image.fromarray(np.round(surface * 255).astype(np.uint8), "RGB")


def label_components(mask):
    """(labels, count) of the 8-connected blobs of a boolean mask; labels start at 1."""
    labels = np.zeros(mask.shape, np.int32)
    count = 0
    for start in zip(*np.nonzero(mask)):
        if labels[start]:
            continue
        count += 1
        labels[start] = count
        stack = [start]
        while stack:
            y, x = stack.pop()
            for ny in range(max(y - 1, 0), min(y + 2, mask.shape[0])):
                for nx in range(max(x - 1, 0), min(x + 2, mask.shape[1])):
                    if mask[ny, nx] and not labels[ny, nx]:
                        labels[ny, nx] = count
                        stack.append((ny, nx))
    return labels, count


def largest_component(mask):
    labels, count = label_components(mask)
    if count == 0:
        return mask
    return labels == 1 + int(np.argmax(np.bincount(labels.ravel())[1:]))


def dilate(mask, size):
    image = Image.fromarray(mask.astype(np.uint8) * 255, "L")
    return np.asarray(image.filter(ImageFilter.MaxFilter(size | 1))) > 0


def erode(mask, size):
    image = Image.fromarray(mask.astype(np.uint8) * 255, "L")
    return np.asarray(image.filter(ImageFilter.MinFilter(size | 1))) > 0


def border_window(size):
    """0 within DECAL_MARGIN px of the border, easing to 1 DECAL_FADE px further in."""
    edge = np.minimum(np.arange(size), np.arange(size)[::-1]).astype(np.float32)
    distance = np.minimum(edge[:, None], edge[None, :])
    t = np.clip((distance - DECAL_MARGIN) / DECAL_FADE, 0, 1)
    return t * t * (3 - 2 * t)


def crack_decal(image):
    """RGBA decal, the size of `image` (a native crop), of the longest dark, thin crack in it.

    Smoothing leaves a crack (dark over several pixels) darker than the gaps between
    stones, so the threshold keeps the crack. It still breaks in places, so the pieces are
    joined by growing the mask first, and the longest blob of that is the one crack.
    """
    patch = flatten_lighting(image)
    smooth = blur(np.asarray(patch.convert("L"), np.float32), CRACK_SMOOTH)
    joined = largest_component(dilate(smooth < CRACK_DARK, CRACK_JOIN))
    mask = erode(joined, CRACK_JOIN - 2)

    blurred = blur(mask.astype(np.float32), DEPTH_BLUR)
    depth = blurred / max(blurred.max(), 1e-6) * border_window(image.width)
    alpha = np.clip(depth / DEPTH_ALPHA, 0, 1)
    # The same frame as asphalt_normal, from the height 1 - depth: the crack is a dip.
    normal = np.asarray(normal_from_height(Image.fromarray(1 - depth, "F"), CRACK_NORMAL_STRENGTH))
    decal = np.dstack([normal[..., 0], normal[..., 1], np.round(depth * 255), np.round(alpha * 255)])
    return Image.fromarray(decal.astype(np.uint8), "RGBA")


def encode_ktx2(png, ktx2, role):
    # "opaque" makes ctt treat alpha as one more data channel. Straight alpha would zero the
    # colour where the crack mask is 0 (losing the flat normal) and weight its mips by the mask.
    subprocess.run(
        ["ctt", str(png), "-o", str(ktx2), "--quality", KTX2_QUALITY, "--zstd=19", "--input-alpha", "opaque"]
        + ctt_arguments(role),
        check=True,
        capture_output=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("photo", type=Path)
    parser.add_argument("output", type=Path, help="folder for the KTX2 maps")
    parser.add_argument("--sources", type=Path, required=True, help="folder for the source PNGs")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    args.sources.mkdir(parents=True, exist_ok=True)

    with Image.open(args.photo) as photo:
        rectified = rectify(photo.convert("RGB"))
    rectified.save(args.sources / "rectified.png")

    albedo, normal, surface = grain_maps(rectified.crop(GRAIN_BOX))
    maps = {
        "albedo": (albedo, "color"),
        "normal": (normal, "data"),
        "surface": (surface, "data"),
        "crack": (crack_decal(rectified.crop(CRACK_BOX)), "data"),
    }
    for name, (image, role) in maps.items():
        png = args.sources / f"asphalt_{name}.png"
        image.save(png, optimize=True)
        encode_ktx2(png, args.output / f"asphalt_{name}.ktx2", role)
        print(f"asphalt_{name}: {image.size[0]}x{image.size[1]} {role}")


if __name__ == "__main__":
    main()
