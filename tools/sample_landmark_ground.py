"""Measure a dense (UHD) landmark GLB's walkable ground and its collision pillars.

Reads geometry only, so it works on GLBs whose textures are already KTX2. Coordinates
stay in the model's own units and axes (glTF Y up), matching the runtime transform.

    python3 tools/sample_landmark_ground.py assets/models/landmarks/uhd/<name>.glb \
        src/world/pillars/<name>.rs --cell 0.012 [--relief src/world/pillars/<name>_ground.rs \
        --opening 0.03 --scale 120]

Pillars: the surface is binned into upright columns `--cell` wide, split wherever a column
has a vertical gap of more than two cells (roofs over open bays, overhangs). Each column
spans its lowest to highest geometry.

Relief (dioramas with their own ground, such as a town on a slope or a quarry): the top
surface is opened (eroded, then dilated) by `--opening` model units, which removes
buildings, posts and trees narrower than twice that while keeping slopes, then stored as
a 64 x 64 height grid over the model's footprint. The game lays the middle of the edge
facing the road on the land there, and the terrain just below the rest. Pillars whose top
stays within `--step` meters of that ground (at `--scale`) are walkable surface and are
dropped.
"""

import argparse
import json
import struct
from pathlib import Path

import numpy as np
from scipy import ndimage

COMPONENTS = {5126: np.float32, 5125: np.uint32, 5123: np.uint16, 5121: np.uint8}
WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
GRID = 64
FINE = 384


def load(path):
    data = path.read_bytes()
    json_length, = struct.unpack_from("<I", data, 12)
    gltf = json.loads(data[20 : 20 + json_length])
    bin_length, = struct.unpack_from("<I", data, 20 + json_length)
    return gltf, data[28 + json_length : 28 + json_length + bin_length]


def accessor(gltf, binary, index):
    meta = gltf["accessors"][index]
    view = gltf["bufferViews"][meta["bufferView"]]
    dtype = np.dtype(COMPONENTS[meta["componentType"]])
    width = WIDTH[meta["type"]]
    offset = view.get("byteOffset", 0) + meta.get("byteOffset", 0)
    stride = view.get("byteStride", width * dtype.itemsize)
    rows = np.frombuffer(binary, np.uint8, meta["count"] * stride, offset).reshape(-1, stride)
    return rows[:, : width * dtype.itemsize].copy().view(dtype).reshape(-1, width)


def surface_points(path, spacing=0.002):
    """Vertices, triangle centroids, and a barycentric lattice over every triangle wider
    than `spacing`, so large flat faces (streets, slabs, walls) leave no empty columns."""
    gltf, binary = load(path)
    points = []
    for mesh in gltf["meshes"]:
        for primitive in mesh["primitives"]:
            positions = accessor(gltf, binary, primitive["attributes"]["POSITION"]).astype(np.float64)
            corners = positions[accessor(gltf, binary, primitive["indices"]).reshape(-1, 3)]
            points += [positions, corners.mean(axis=1)]
            edges = np.linalg.norm(corners - np.roll(corners, 1, axis=1), axis=2).max(axis=1)
            steps = np.ceil(edges / spacing).astype(int)
            for n in np.unique(steps[steps > 1]):
                a, b, c = (corners[steps == n][:, k] for k in range(3))
                i, j = np.meshgrid(np.arange(n + 1), np.arange(n + 1))
                keep = i + j <= n
                u, v = i[keep] / n, j[keep] / n
                points.append(
                    (a[:, None] + (b - a)[:, None] * u[None, :, None] + (c - a)[:, None] * v[None, :, None]).reshape(-1, 3)
                )
    return np.concatenate(points)


def fill_empty(grid):
    """Give empty cells the value of the nearest filled cell."""
    empty = np.isnan(grid)
    if empty.all():
        raise ValueError("no geometry")
    _, (rows, cols) = ndimage.distance_transform_edt(empty, return_indices=True)
    return grid[rows, cols]


def relief(points, lo, hi, opening):
    """Opened top surface on a FINE grid, downsampled (by minimum) to GRID x GRID."""
    span = hi - lo
    cells = ((points[:, [0, 2]] - lo) / span * FINE).astype(int).clip(0, FINE - 1)
    top = np.full((FINE, FINE), -np.inf)
    np.maximum.at(top, (cells[:, 1], cells[:, 0]), points[:, 1])
    empty = np.isinf(top)
    # Open with the empty cells sunk below everything, so posts at the footprint's edge
    # erode away instead of spreading into the gaps; fill the gaps afterwards.
    top[empty] = top[~empty].min() - 1.0
    radius = np.maximum(1, np.round(opening / span * FINE)).astype(int)
    footprint = (2 * radius[1] + 1, 2 * radius[0] + 1)
    opened = ndimage.grey_opening(top, size=footprint, mode="nearest")
    opened[empty | (opened < top[~empty].min())] = np.nan
    opened = fill_empty(opened)
    block = FINE // GRID
    return opened.reshape(GRID, block, GRID, block).min(axis=(1, 3))


def sample_relief(grid, lo, cell, x, z):
    """Bilinear lookup matching the game's sampler (cell centers at lo + (i + 0.5) * cell)."""
    u = np.clip((x - lo[0]) / cell[0] - 0.5, 0, GRID - 1)
    v = np.clip((z - lo[1]) / cell[1] - 0.5, 0, GRID - 1)
    i0, j0 = np.minimum(u.astype(int), GRID - 2), np.minimum(v.astype(int), GRID - 2)
    fu, fv = u - i0, v - j0
    a, b = grid[j0, i0], grid[j0, i0 + 1]
    c, d = grid[j0 + 1, i0], grid[j0 + 1, i0 + 1]
    return (a * (1 - fu) + b * fu) * (1 - fv) + (c * (1 - fu) + d * fu) * fv


def pillars(points, cell):
    keys = np.round(points[:, [0, 2]] / cell).astype(np.int64)
    order = np.lexsort((points[:, 1], keys[:, 1], keys[:, 0]))
    keys, heights = keys[order], points[order, 1]
    starts = np.flatnonzero(np.r_[True, np.any(keys[1:] != keys[:-1], axis=1)])
    ends = np.r_[starts[1:], len(keys)]
    for start, end in zip(starts, ends):
        if end - start < 3:
            continue
        column = heights[start:end]
        breaks = np.flatnonzero(np.diff(column) > 2 * cell)
        lows = np.r_[column[0], column[breaks + 1]]
        highs = np.r_[column[breaks], column[-1]]
        for low, high in zip(lows, highs):
            yield keys[start, 0] * cell, keys[start, 1] * cell, low, high


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("glb", type=Path)
    parser.add_argument("pillars", type=Path)
    parser.add_argument("--cell", type=float, default=0.03, help="pillar column width, model units")
    parser.add_argument("--relief", type=Path, help="write the ground grid here")
    parser.add_argument("--opening", type=float, default=0.03, help="feature width removed from the ground, model units")
    parser.add_argument("--scale", type=float, default=1.0, help="meters per model unit in the game")
    parser.add_argument("--step", type=float, default=0.28, help="meters above the ground that stay walkable")
    args = parser.parse_args()

    points = surface_points(args.glb)
    lo3, hi3 = points.min(axis=0), points.max(axis=0)
    columns = list(pillars(points, args.cell))
    header = f"// Generated by tools/sample_landmark_ground.py from {args.glb.name}; do not edit.\n"

    if args.relief:
        lo, hi = lo3[[0, 2]], hi3[[0, 2]]
        cell = (hi - lo) / GRID
        ground = relief(points, lo, hi, args.opening)
        xs = np.array([c[0] for c in columns])
        zs = np.array([c[1] for c in columns])
        floor = sample_relief(ground, lo, cell, xs, zs)
        columns = [c for c, f in zip(columns, floor) if (c[3] - f) * args.scale >= args.step]
        with args.relief.open("w") as out:
            out.write(header)
            out.write(f"Relief {{\n    min: Vec2::new({lo[0]:.5f}, {lo[1]:.5f}),\n")
            out.write(f"    cell: Vec2::new({cell[0]:.6f}, {cell[1]:.6f}),\n")
            out.write("    heights: &[\n")
            for row in ground:
                out.write("        " + ", ".join(f"{h:.4f}" for h in row) + ",\n")
            out.write("    ],\n}\n")
        center = sample_relief(ground, lo, cell, np.array(0.0), np.array(0.0))
        print(f"RELIEF min {ground.min():.4f} max {ground.max():.4f} center {float(center):.4f}")

    radius = 0.6 * args.cell
    with args.pillars.open("w") as out:
        out.write(header + "&[\n")
        for x, z, low, high in columns:
            out.write(
                f"    Pillar {{ x: {x:.4f}, z: {z:.4f}, radius: {radius:.4f}, "
                f"bottom: {low:.4f}, top: {high:.4f} }},\n"
            )
        out.write("]\n")
    low_1 = np.percentile(points[:, 1], 1)
    print(f"BOUNDS {np.round(lo3, 4)} {np.round(hi3, 4)} Y1% {low_1:.4f} PILLARS {len(columns)}")


if __name__ == "__main__":
    main()
