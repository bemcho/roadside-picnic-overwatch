"""Ground a GLB without changing topology, materials, images, or texture resolution.

Use for full-quality single-primitive display assets:
    python3 tools/ground_full_asset.py input.glb assets/models/artifacts/input.glb
"""

import argparse
import json
import struct
from pathlib import Path


def prepare(source: Path, output: Path) -> None:
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
    if stride != 12 or position.get("byteOffset", 0) != 0:
        raise ValueError("Expected tightly packed VEC3 positions")

    start = view.get("byteOffset", 0)
    ys = [
        struct.unpack_from("<f", binary, start + i * stride + 4)[0]
        for i in range(position["count"])
    ]
    base = min(ys)
    if base != 0.0:
        for i, y in enumerate(ys):
            struct.pack_into("<f", binary, start + i * stride + 4, y - base)
        position["min"][1] -= base
        position["max"][1] -= base

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
    print(f"{source.name}: shifted Y by {-base:.6f}; wrote {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    arguments = parser.parse_args()
    prepare(arguments.source, arguments.output)
