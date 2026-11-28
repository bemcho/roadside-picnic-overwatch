"""Smooth the knobs off the Golden Sphere's UHD scan.

The generated scan (the workspace's models/artifacts/uhd/artifact-golden-sphere.glb)
carries four knobs: two bosses standing about 6 cm proud of the 0.49 m ball with a
square socket sunk to the ball's surface, and two domed screw heads about 3 cm proud.
The ball is a mirror in the game, so each shows as a door knob on the Sphere. The
medium model has none.

This fits the ball's centre (least squares, again without the knobs) and finds each
knob as a cluster of directions standing more than KNOB_PEAK proud of the ball's
median radius (the scan's raised seams, under 1%, are no knobs and stay). Every vertex within the knob's reach plus MARGIN, its socket and
the fillet at its foot included, goes onto the ball with its normal turned outward;
over the next BAND the share falls off along a smoothstep, so the patch meets the
ball without a step. The rest of the scan, its UVs, indices and texture are left as
they are.

    python3 tools/smooth_golden_sphere.py SOURCE.glb OUT.glb
    python3 tools/compress_glb_textures.py OUT.glb   # last, as for every runtime GLB
"""

import json
import struct
import sys

import numpy as np
from scipy.sparse import coo_matrix
from scipy.sparse.csgraph import connected_components
from scipy.spatial import cKDTree

# Shares of the radius: the ball's own surface keeps within KNOB_FOOT; a knob's
# points stand past KNOB_PEAK.
KNOB_FOOT = 0.005
KNOB_PEAK = 0.02
# Angles in radians on the ball: points this close join one knob; past a knob's
# reach the patch is whole for MARGIN (7 mm) and fades out over BAND.
LINK = 0.02
MARGIN = 0.015
BAND = 0.015

COMPONENTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
FLOAT = 5126


def read_glb(data):
    """The GLB's JSON and its binary chunk."""
    json_length = struct.unpack_from("<I", data, 12)[0]
    gltf = json.loads(data[20 : 20 + json_length])
    bin_start = 20 + json_length
    bin_length = struct.unpack_from("<I", data, bin_start)[0]
    return gltf, bytearray(data[bin_start + 8 : bin_start + 8 + bin_length])


def write_glb(gltf, binary):
    """A GLB from `gltf` and `binary`, each chunk padded to 4 bytes."""
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    binary = bytes(binary) + b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    return (
        struct.pack("<III", 0x46546C67, 2, total)
        + struct.pack("<II", len(text), 0x4E4F534A)
        + text
        + struct.pack("<II", len(binary), 0x004E4942)
        + binary
    )


def float_view(gltf, binary, index):
    """A writable (count, n) float32 view of accessor `index` into `binary`."""
    accessor = gltf["accessors"][index]
    if accessor["componentType"] != FLOAT:
        raise ValueError(f"accessor {index} is not float")
    view = gltf["bufferViews"][accessor["bufferView"]]
    n = COMPONENTS[accessor["type"]]
    offset = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    stride = view.get("byteStride", 4 * n)
    whole = np.frombuffer(binary, dtype=np.uint8)
    rows = np.lib.stride_tricks.as_strided(
        whole[offset:], shape=(accessor["count"], 4 * n), strides=(stride, 1), writeable=True
    )
    return rows, n


def smoothstep(edge0, edge1, x):
    t = np.clip((x - edge0) / (edge1 - edge0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def fit_sphere(points):
    """The least-squares sphere through `points`: its centre and radius."""
    a = np.column_stack([2 * points, np.ones(len(points))])
    b = (points**2).sum(axis=1)
    solution = np.linalg.lstsq(a, b, rcond=None)[0]
    centre = solution[:3]
    return centre, np.sqrt(solution[3] + centre @ centre)


def ball_centre(positions):
    """The ball's centre, fitted again without the knobs, which would drag a bounding box's."""
    centre, radius = fit_sphere(positions)
    on_ball = np.abs(np.linalg.norm(positions - centre, axis=1) - radius) < KNOB_FOOT * radius
    return fit_sphere(positions[on_ball])[0]


def knobs(directions, proud):
    """Each knob's axis and reach (radians): the clusters of points raised past KNOB_PEAK."""
    raised = np.flatnonzero(proud > KNOB_PEAK)
    pairs = cKDTree(directions[raised]).query_pairs(LINK, output_type="ndarray")
    links = coo_matrix((np.ones(len(pairs)), (pairs[:, 0], pairs[:, 1])), shape=(len(raised),) * 2)
    count, labels = connected_components(links, directed=False)
    found = []
    for label in range(count):
        members = raised[labels == label]
        axis = directions[members].mean(axis=0)
        axis /= np.linalg.norm(axis)
        found.append((axis, np.arccos(np.clip(directions[members] @ axis, -1, 1)).max()))
    return found


def smooth_knobs(positions, normals):
    """Positions and normals with every knob laid onto the ball; the share moved."""
    centre = ball_centre(positions)
    offset = positions - centre
    radius = np.linalg.norm(offset, axis=1)
    ball = np.median(radius)
    outward = offset / np.maximum(radius, 1e-9)[:, None]
    share = np.zeros(len(positions))
    for axis, reach in knobs(outward, (radius - ball) / ball):
        angle = np.arccos(np.clip(outward @ axis, -1, 1))
        share = np.maximum(share, 1 - smoothstep(reach + MARGIN, reach + MARGIN + BAND, angle))
    new_radius = radius + (ball - radius) * share
    new_positions = centre + outward * new_radius[:, None]
    turned = normals + (outward - normals) * share[:, None]
    new_normals = turned / np.maximum(np.linalg.norm(turned, axis=1), 1e-9)[:, None]
    return new_positions, new_normals, share


def main(source, out):
    with open(source, "rb") as file:
        gltf, binary = read_glb(file.read())
    primitive = gltf["meshes"][0]["primitives"][0]
    position_rows, _ = float_view(gltf, binary, primitive["attributes"]["POSITION"])
    normal_rows, _ = float_view(gltf, binary, primitive["attributes"]["NORMAL"])
    positions = position_rows.copy().view(np.float32).reshape(-1, 3).astype(np.float64)
    normals = normal_rows.copy().view(np.float32).reshape(-1, 3).astype(np.float64)

    new_positions, new_normals, share = smooth_knobs(positions, normals)
    position_rows[:] = new_positions.astype(np.float32).view(np.uint8).reshape(position_rows.shape)
    normal_rows[:] = new_normals.astype(np.float32).view(np.uint8).reshape(normal_rows.shape)
    for name, values in (("POSITION", new_positions), ("NORMAL", new_normals)):
        accessor = gltf["accessors"][primitive["attributes"][name]]
        stored = values.astype(np.float32)
        if "min" in accessor or name == "POSITION":
            accessor["min"] = stored.min(0).tolist()
            accessor["max"] = stored.max(0).tolist()

    with open(out, "wb") as file:
        file.write(write_glb(gltf, binary))
    moved = share > 0
    print(f"{out}: {moved.sum()} of {len(share)} vertices moved, {(share >= 1).sum()} fully")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2])
