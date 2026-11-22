"""Square up a scanned kit piece so copies can be laid end to end along a line.

Tripo part splits keep each part's pose in the source scan: a stretch of track lies at an
angle and on the slope it was cut from. This rotates each single-primitive GLB so its flattest
direction is +Y and its longest is +Z (principal axes of the vertices), centers the footprint
on the origin and grounds it at y = 0, rewriting only positions and normals in place. It then
prints the piece's length (z), width (x) and height (y) in GLB units.

    python3 tools/align_kit_piece.py assets/models/railway/uhd/rail_48.glb [more.glb ...]

`--keep-level` only turns the piece about Y, for pieces that should keep their own tilt
(a heap, a leaning post). Works before or after tools/compress_glb_textures.py.
"""

import argparse
import json
import struct
from pathlib import Path

import numpy as np


def read(path):
    data = path.read_bytes()
    json_len = struct.unpack_from("<I", data, 12)[0]
    document = json.loads(data[20 : 20 + json_len])
    binary_len = struct.unpack_from("<I", data, 20 + json_len)[0]
    binary = bytearray(data[28 + json_len : 28 + json_len + binary_len])
    return document, binary


def write(path, document, binary):
    metadata = json.dumps(document, separators=(",", ":")).encode()
    metadata += b" " * (-len(metadata) % 4)
    header = struct.pack("<4sIIII", b"glTF", 2, 28 + len(metadata) + len(binary), len(metadata), 0x4E4F534A)
    path.write_bytes(header + metadata + struct.pack("<II", len(binary), 0x004E4942) + binary)


def vec3_view(document, binary, accessor_index):
    """A writable (count, 3) float32 view of a VEC3 accessor, strided or packed."""
    accessor = document["accessors"][accessor_index]
    view = document["bufferViews"][accessor["bufferView"]]
    stride = view.get("byteStride", 12)
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    rows = np.frombuffer(binary, dtype=np.uint8, count=stride * accessor["count"], offset=start)
    rows = rows.reshape(accessor["count"], stride)
    return accessor, rows[:, :12].view("<f4")


def principal_rotation(points, keep_level):
    """Rows are the new x, y, z axes: y the flattest direction, z the longest."""
    centered = points - points.mean(axis=0)
    if keep_level:
        _, vectors = np.linalg.eigh(np.cov(centered[:, [0, 2]].T))
        z = np.array([vectors[0, 1], 0.0, vectors[1, 1]])
        y = np.array([0.0, 1.0, 0.0])
    else:
        _, vectors = np.linalg.eigh(np.cov(centered.T))
        y, z = vectors[:, 0], vectors[:, 2]
        if y[1] < 0:
            y = -y
    x = np.cross(y, z)
    return np.array([x, y, z])


def align(path, keep_level):
    document, binary = read(path)
    primitives = [p for mesh in document["meshes"] for p in mesh["primitives"]]
    if len(primitives) != 1:
        raise ValueError(f"{path}: expected one primitive")
    attributes = primitives[0]["attributes"]
    if "TANGENT" in attributes:
        raise ValueError(f"{path}: rotate before baking tangents")
    position, positions = vec3_view(document, binary, attributes["POSITION"])
    points = positions.astype(np.float64)
    rotation = principal_rotation(points, keep_level)
    turned = points @ rotation.T
    low, high = turned.min(axis=0), turned.max(axis=0)
    turned -= np.array([(low[0] + high[0]) / 2, low[1], (low[2] + high[2]) / 2])
    positions[:] = turned.astype(np.float32)
    position["min"] = turned.min(axis=0).tolist()
    position["max"] = turned.max(axis=0).tolist()
    if "NORMAL" in attributes:
        _, normals = vec3_view(document, binary, attributes["NORMAL"])
        normals[:] = (normals.astype(np.float64) @ rotation.T).astype(np.float32)
    for node in document.get("nodes", []):
        if "mesh" in node:
            for key in ("translation", "rotation", "scale", "matrix"):
                node.pop(key, None)
    write(path, document, binary)
    size = turned.max(axis=0) - turned.min(axis=0)
    print(f"{path.name}: length {size[2]:.5f} width {size[0]:.5f} height {size[1]:.5f}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("glbs", type=Path, nargs="+")
    parser.add_argument("--keep-level", action="store_true", help="only turn the piece about Y")
    arguments = parser.parse_args()
    for glb in arguments.glbs:
        align(glb, arguments.keep_level)
