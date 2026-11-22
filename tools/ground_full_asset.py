"""Ground a GLB without changing topology, materials, images, or texture resolution.

Use for full-quality single-primitive display assets:
    python3 tools/ground_full_asset.py input.glb assets/models/artifacts/input.glb

`--center` also centers the footprint (x, z) on the origin in the vertex data and drops
the mesh node's translation, for assets loaded by mesh (`load_single_primitive`), which
ignores node transforms. It only moves positions, so it works on KTX2-compressed GLBs.
"""

import argparse
import json
import struct
from pathlib import Path


def prepare(source: Path, output: Path, center: bool = False) -> None:
    data = source.read_bytes()
    magic, version, _, json_len, json_kind = struct.unpack_from("<4sIIII", data)
    if (magic, version, json_kind) != (b"glTF", 2, 0x4E4F534A):
        raise ValueError("Expected a glTF 2.0 binary")
    document = json.loads(data[20 : 20 + json_len])
    binary_len, binary_kind = struct.unpack_from("<II", data, 20 + json_len)
    if binary_kind != 0x004E4942:
        raise ValueError("Expected a GLB binary buffer")
    binary = bytearray(data[28 + json_len : 28 + json_len + binary_len])

    meshes = document["meshes"]
    primitives = [primitive for mesh in meshes for primitive in mesh["primitives"]]
    if len(primitives) != 1:
        raise ValueError("Expected one primitive across the GLB")
    position = document["accessors"][primitives[0]["attributes"]["POSITION"]]
    if position["componentType"] != 5126 or position["type"] != "VEC3":
        raise ValueError("Expected float VEC3 positions")
    view = document["bufferViews"][position["bufferView"]]
    stride = view.get("byteStride", 12)
    start = view.get("byteOffset", 0) + position.get("byteOffset", 0)
    count = position["count"]
    points = [struct.unpack_from("<3f", binary, start + i * stride) for i in range(count)]
    shift = [0.0, -min(p[1] for p in points), 0.0]
    if center:
        for axis in (0, 2):
            values = [p[axis] for p in points]
            shift[axis] = -(min(values) + max(values)) / 2
        for node in document.get("nodes", []):
            if "mesh" in node:
                node.pop("translation", None)
    if any(shift):
        for i, p in enumerate(points):
            struct.pack_into("<3f", binary, start + i * stride, *(v + d for v, d in zip(p, shift)))
        for axis in range(3):
            position["min"][axis] += shift[axis]
            position["max"][axis] += shift[axis]

    metadata = json.dumps(document, separators=(",", ":")).encode()
    metadata += b" " * (-len(metadata) % 4)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(
        struct.pack(
            "<4sIIII",
            b"glTF",
            2,
            28 + len(metadata) + len(binary),
            len(metadata),
            json_kind,
        )
        + metadata
        + struct.pack("<II", len(binary), binary_kind)
        + binary
    )
    print(f"{source.name}: shifted by {[round(d, 6) for d in shift]}; wrote {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--center", action="store_true", help="also center the footprint")
    arguments = parser.parse_args()
    prepare(arguments.source, arguments.output, arguments.center)
