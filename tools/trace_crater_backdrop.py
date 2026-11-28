"""Trace the dark backdrop the Golden Sphere's crater scan was generated with.

The core scan (landmark-core.glb) carries the painted Sphere of its generation on a
smooth, near-black face at the crater's foot, ringed by tiers of stone blocks. Where
that face stands above the terrain the game draws, it shows as a black flat shape
(and a second, painted Sphere) beside the real one, and the real Sphere mirrors it.
The game cuts the face away as it cuts the road's corridor (`quarry_ramp.rs`), so the
terrain shows there instead.

Seen from above, the face is every column whose top surface is dark (luminance under
DARK) and smooth (deviation under SMOOTH over 5 cells), joined to the painted Sphere.
Its thin arms are opened off (OPENING cells), it takes in the empty cells and the dark
ones at the hole within SKIRT of it (its edge hangs down as a dark skirt there) and its outline is simplified
to within TOLERANCE. Coordinates stay in the model's units (x, z).

Needs the scan with its source texture (the workspace's
models/landmarks/uhd/landmark-core.glb): the runtime copy's is KTX2, which PIL cannot read.

    python3 tools/trace_crater_backdrop.py ~/Projects/agentic/RoadSide-Picnic/models/landmarks/uhd/landmark-core.glb \
        src/world/pillars/golden_crater_backdrop.rs
"""

import io
import sys
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage
from skimage import measure

sys.path.insert(0, str(Path(__file__).resolve().parent))

from sample_landmark_ground import accessor, load  # noqa: E402

# The painted Sphere's centroid in model units (x, z): `sites::SPHERE_SMUDGE`.
SEED = (-0.0172, -0.0138)
SCALE = 120.0
CELL = 0.5 / SCALE
REACH = 30.0 / SCALE
SEED_REACH = 3.0 / SCALE
DARK = 0.16
SMOOTH = 0.05
OPENING = 3
SKIRT = 4
TOLERANCE = 0.4 / SCALE


def top_colours(path):
    """Each cell's top-most triangle colour over a square of `REACH` around the seed."""
    gltf, binary = load(Path(path))
    primitive = gltf["meshes"][0]["primitives"][0]
    positions = accessor(gltf, binary, primitive["attributes"]["POSITION"]).astype(np.float64)
    uvs = accessor(gltf, binary, primitive["attributes"]["TEXCOORD_0"]).astype(np.float64)
    triangles = accessor(gltf, binary, primitive["indices"]).reshape(-1, 3).astype(np.int64)
    image = gltf["images"][gltf["textures"][0]["source"]]
    view = gltf["bufferViews"][image["bufferView"]]
    start = view.get("byteOffset", 0)
    Image.MAX_IMAGE_PIXELS = None
    texture = Image.open(io.BytesIO(binary[start : start + view["byteLength"]])).convert("RGB")
    texture = np.asarray(texture.reduce(8), dtype=np.float64) / 255.0

    centre = positions[triangles].mean(axis=1)
    uv = uvs[triangles].mean(axis=1) % 1.0
    rows, columns = texture.shape[:2]
    colour = texture[(uv[:, 1] * rows).astype(int).clip(0, rows - 1), (uv[:, 0] * columns).astype(int).clip(0, columns - 1)]
    luminance = colour @ [0.2126, 0.7152, 0.0722]

    corner = np.array(SEED) - REACH
    size = int(2 * REACH / CELL)
    cells = ((centre[:, [0, 2]] - corner) / CELL).astype(int)
    inside = (cells >= 0).all(axis=1) & (cells < size).all(axis=1)
    grid = np.full((size, size), np.nan)
    for i in np.flatnonzero(inside)[np.argsort(centre[inside, 1])]:
        grid[cells[i, 1], cells[i, 0]] = luminance[i]
    return grid, corner


def backdrop_mask(grid, seed_cell):
    """The dark, smooth face around `seed_cell` (row, column), with its thin arms opened off."""
    filled = np.nan_to_num(grid, nan=1.0)
    rough = ndimage.generic_filter(filled, np.std, size=5)
    mask = (filled < DARK) & (rough < SMOOTH)
    mask = ndimage.binary_closing(ndimage.binary_opening(mask), iterations=3)
    mask = ndimage.binary_opening(mask, iterations=OPENING)
    labels, _ = ndimage.label(mask)
    # The painted Sphere itself is bright: take the face that fills most of a disc around it.
    rows, columns = np.ogrid[: mask.shape[0], : mask.shape[1]]
    near = (rows - seed_cell[0]) ** 2 + (columns - seed_cell[1]) ** 2 <= (SEED_REACH / CELL) ** 2
    found = np.bincount(labels[near & mask])
    if found.size < 2:
        raise ValueError("no dark, smooth face around the seed")
    face = labels == found[1:].argmax() + 1
    # Where the scan has its hole the face's edge hangs down as a dark skirt,
    # too close to the hole to count as smooth: take in the empty cells and the
    # dark ones at the hole beside it (not the crevices between the stone tiers).
    hole = np.isnan(grid)
    beside = hole | ((filled < DARK) & ndimage.binary_dilation(hole, iterations=SKIRT))
    return face | (ndimage.binary_dilation(face, iterations=SKIRT) & beside)


def outline(mask, corner):
    """The mask's outline in model units (x, z), simplified to within `TOLERANCE`."""
    padded = np.pad(mask, 1).astype(float)
    ring = max(measure.find_contours(padded, 0.5), key=len)
    ring = measure.approximate_polygon(ring, TOLERANCE / CELL)[:-1]
    # Contour (row, column) in padded cells, cell centres at whole numbers.
    return (ring[:, ::-1] - 1 + 0.5) * CELL + corner


def main(source, out):
    grid, corner = top_colours(source)
    seed_cell = tuple(((np.array(SEED) - corner) / CELL).astype(int)[::-1])
    mask = backdrop_mask(grid, seed_cell)
    ring = outline(mask, corner)
    area = mask.sum() * (CELL * SCALE) ** 2
    lines = [
        "// Generated by tools/trace_crater_backdrop.py from landmark-core.glb; do not edit.",
        f"// The core scan's dark backdrop in model units (x, z): {area:.0f} m2 traced, {len(ring)} corners.",
        "&[",
        *(f"    Vec2::new({x:.5f}, {z:.5f})," for x, z in ring),
        "]",
    ]
    Path(out).write_text("\n".join(lines) + "\n")
    print(f"{out}: {len(ring)} corners around {area:.0f} m2")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
