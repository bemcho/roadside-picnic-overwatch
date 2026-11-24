"""Bake the old-asphalt maps from one oblique photo (requires Pillow, numpy and the ctt CLI).

The terrain shader paints every `Surface::Asphalt` road with a tiling grain set plus
scattered crack decals. Each map is 512 x 512 and ships as KTX2 with a full mip chain
(`ctt` presets from compress_glb_textures.py); the PNGs they were encoded from stay in
the assets workspace.

    python3 tools/prepare_asphalt_textures.py ~/Downloads/image_5b974df1.jpg \
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
# The ground quad that is flattened, clockwise from the top left. Its sides converge
# upward because the road recedes from the camera.
SHARP_QUAD = ((211, 340), (1197, 340), (1408, 768), (0, 768))
RECTIFIED_SIZE = (1408, 1000)
# Boxes in the rectified image, (left, top, right, bottom).
GRAIN_BOX = (1008, 596, 1408, 996)  # crack-free gravel at the bottom right (about native resolution)
CRACK_BOX = (780, 0, 1180, 400)  # the long thin crack that runs down from the top edge

MAP_SIZE = 512
LIGHTING_BLUR = 0.15  # sigma of the wide blur that is divided out, as a fraction of the width
ALBEDO_LUMINANCE = 0.085  # mean linear luminance: weathered, dark asphalt
ALBEDO_CHROMA = 0.5  # share of the photo's colour kept; the tan pebbles would repeat like landmarks
HEIGHT_BLUR = 10  # px (Gaussian sigma); the grain relief is what stays above this blur
HEIGHT_SIGMAS = 3.0  # luminance deviations that span the height range
CAVITY_BLUR = 3
AO_STRENGTH = 0.65
ROUGHNESS = (0.6, 0.95)  # tar binder .. dusty stone
NORMAL_STRENGTH = 6.0  # normal slope per unit of height change per pixel
CRACK_SMOOTH = 3  # px (sigma); blurs away the narrow gaps between stones, so only a wide dark line is left
CRACK_DARK = 22  # luminance (0..255) below which smoothed pixels belong to a crack
CRACK_GROW = 3  # px; width of the dilation filter (3 grows the kept crack 1 px each side)
DEPTH_BLUR = 3  # px (sigma); the depth is the mask blurred
DEPTH_ALPHA = 0.5  # depth at which the mask is fully opaque
CRACK_NORMAL_STRENGTH = 10.0
DECAL_MARGIN = 12  # px kept fully transparent at the decal's border
DECAL_FADE = 36  # px over which the crack fades in from the margin

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


def border_window(size):
    """0 within DECAL_MARGIN px of the border, easing to 1 DECAL_FADE px further in."""
    edge = np.minimum(np.arange(size), np.arange(size)[::-1]).astype(np.float32)
    distance = np.minimum(edge[:, None], edge[None, :])
    t = np.clip((distance - DECAL_MARGIN) / DECAL_FADE, 0, 1)
    return t * t * (3 - 2 * t)


def crack_decal(image, box):
    """RGBA decal of the longest dark, thin crack inside `box` (left, top, right, bottom).

    Smoothing leaves a crack (dark over several pixels) darker than the gaps between
    stones, so the threshold keeps the crack and the longest blob of it is the one crack.
    """
    patch = flatten_lighting(image.crop(box).resize((MAP_SIZE, MAP_SIZE), Image.Resampling.LANCZOS))
    smooth = blur(np.asarray(patch.convert("L"), np.float32), CRACK_SMOOTH)
    mask = dilate(largest_component(smooth < CRACK_DARK), CRACK_GROW)

    blurred = blur(mask.astype(np.float32), DEPTH_BLUR)
    depth = blurred / max(blurred.max(), 1e-6) * border_window(MAP_SIZE)
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

    patch = rectified.crop(GRAIN_BOX).resize((MAP_SIZE, MAP_SIZE), Image.Resampling.LANCZOS)
    albedo, normal, surface = grain_maps(patch)
    maps = {
        "albedo": (albedo, "color"),
        "normal": (normal, "data"),
        "surface": (surface, "data"),
        "crack": (crack_decal(rectified, CRACK_BOX), "data"),
    }
    for name, (image, role) in maps.items():
        png = args.sources / f"asphalt_{name}.png"
        image.save(png, optimize=True)
        encode_ktx2(png, args.output / f"asphalt_{name}.ktx2", role)
        print(f"asphalt_{name}: {image.size[0]}x{image.size[1]} {role}")


if __name__ == "__main__":
    main()
