"""Level the hangar slab and apron while preserving the original PBR UVs.

Run after mesh simplification and texture resizing:
    npx @gltf-transform/cli@4 tangents resized.glb tangents.glb
    python3 tools/prepare_hangar_asset.py tangents.glb assets/models/base/hangar.glb
"""

import argparse
import json
import math
import struct
from pathlib import Path


# The slab and apron are irregular but shallow. Compress only this band, so
# the exported walkable surface sits just above the game's y=0 terrain pad.
PROFILE = ((0.0, 0.0), (0.003, 0.0220), (0.035, 0.0230), (0.065, 0.065))


def floor_profile(y):
    for (low, bottom), (high, top) in zip(PROFILE, PROFILE[1:]):
        if y < high:
            slope = (top - bottom) / (high - low)
            return bottom + (y - low) * slope, slope
    return y, 1.0


def prepare(source, output):
    data = source.read_bytes()
    magic, version, _, json_len, json_kind = struct.unpack_from("<4sIIII", data)
    if (magic, version, json_kind) != (b"glTF", 2, 0x4E4F534A):
        raise ValueError("Expected a glTF 2.0 binary")
    scene = json.loads(data[20 : 20 + json_len])
    binary_len, binary_kind = struct.unpack_from("<II", data, 20 + json_len)
    if binary_kind != 0x004E4942:
        raise ValueError("Expected a GLB binary buffer")
    binary = bytearray(data[28 + json_len : 28 + json_len + binary_len])
    xyz = struct.Struct("<3f")
    xyzw = struct.Struct("<4f")

    def offsets(accessor, code):
        view = scene["bufferViews"][accessor["bufferView"]]
        start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", code.size)
        return range(start, start + stride * accessor["count"], stride)

    (mesh,) = scene["meshes"]
    (primitive,) = mesh["primitives"]
    positions = scene["accessors"][primitive["attributes"]["POSITION"]]
    normals = scene["accessors"][primitive["attributes"]["NORMAL"]]
    tangents = scene["accessors"][primitive["attributes"]["TANGENT"]]
    lowest = float("inf")
    for pos_at, norm_at, tan_at in zip(
        offsets(positions, xyz), offsets(normals, xyz), offsets(tangents, xyzw), strict=True,
    ):
        x, y, z = xyz.unpack_from(binary, pos_at)
        height, slope = floor_profile(y)
        lowest = min(lowest, height)
        nx, ny, nz = xyz.unpack_from(binary, norm_at)
        tx, ty, tz, sign = xyzw.unpack_from(binary, tan_at)
        if slope != 1.0:
            ny /= slope
            length = math.hypot(nx, ny, nz)
            nx, ny, nz = nx / length, ny / length, nz / length
            xyz.pack_into(binary, norm_at, nx, ny, nz)
            xyz.pack_into(binary, pos_at, x, height, z)
        if slope != 1.0 or math.hypot(tx, ty, tz) < 1e-5:
            ty *= slope
            dot = tx * nx + ty * ny + tz * nz
            tx, ty, tz = tx - dot * nx, ty - dot * ny, tz - dot * nz
            length = math.hypot(tx, ty, tz)
            if length < 1e-5:
                # UV-degenerate seams have no MikkTSpace tangent; use a perpendicular.
                tx, ty, tz = (0.0, -nz, ny) if abs(nx) < 0.8 else (nz, 0.0, -nx)
                length = math.hypot(tx, ty, tz)
            xyzw.pack_into(binary, tan_at, tx / length, ty / length, tz / length, sign)
    positions["min"][1] = lowest
    scene["nodes"][scene["scenes"][scene.get("scene", 0)]["nodes"][0]]["name"] = "Hangar"
    metadata = json.dumps(scene, separators=(",", ":")).encode()
    metadata += b" " * (-len(metadata) % 4)
    output.write_bytes(
        struct.pack("<4sIIII", b"glTF", 2, 28 + len(metadata) + len(binary), len(metadata), json_kind)
        + metadata + struct.pack("<II", len(binary), binary_kind) + binary
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    prepare(args.source, args.output)
