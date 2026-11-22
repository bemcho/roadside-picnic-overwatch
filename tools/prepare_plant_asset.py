"""Ground an optimized single-mesh prop or plant GLB and repair degenerate tangents.

Also remove the thornbush plaque and bake glowweed emission for those assets.

Use after glTF Transform simplify, resize and tangents with --vertex-layout separate:
    python3 tools/prepare_plant_asset.py input.glb assets/models/plants/glowweed.glb
"""

import argparse
import io
import json
import math
import struct
from pathlib import Path

from PIL import Image, ImageChops


def remove_plaque(positions, indices):
    # Weld for connectivity only, leaving the exported normals and UV seams in place.
    points = {}
    parent = list(range(len(positions)))

    def root(index):
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    for i, vertex in enumerate(positions):
        parent[i] = points.setdefault(tuple(round(v, 5) for v in vertex), i)
    for a, b, c in zip(*(iter(indices),) * 3, strict=True):
        parent[root(b)] = root(a)
        parent[root(c)] = root(a)
    spikes = {root(i) for i, (_, y, z) in enumerate(positions) if y > 0.035 or z > -0.29}
    filtered = [i for i in indices if root(i) in spikes]
    if not filtered or len(filtered) == len(indices):
        raise ValueError("Could not separate the display plaque from the thornbush")
    return filtered


def add_emission(doc, binary, material):
    albedo = material["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    texture = doc["textures"][albedo]
    view = doc["bufferViews"][doc["images"][texture["source"]]["bufferView"]]
    start = view.get("byteOffset", 0)
    with Image.open(io.BytesIO(binary[start : start + view["byteLength"]])) as image:
        red, green, blue = image.convert("RGB").split()
    # Only the cyan markings emit, not the blue leaves or the gray roots.
    luminance = blue.point(lambda x: round(255 * min(1, max(0, (x - 116) / 113))))
    cyan = ImageChops.subtract(green, red).point(lambda x: round(255 * min(1, max(0, (x - 31) / 76))))
    mask = ImageChops.multiply(luminance, cyan)
    image = io.BytesIO()
    mask.convert("RGB").save(image, format="PNG", optimize=True)
    binary.extend(b"\0" * (-len(binary) % 4))
    data = image.getvalue()
    doc["bufferViews"].append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data)})
    binary.extend(data)
    doc["images"].append({"bufferView": len(doc["bufferViews"]) - 1, "mimeType": "image/png"})
    doc["textures"].append({**texture, "source": len(doc["images"]) - 1})
    material["emissiveTexture"] = {"index": len(doc["textures"]) - 1}
    material["emissiveFactor"] = [0.09, 0.63, 1.0]
    extension = "KHR_materials_emissive_strength"
    material.setdefault("extensions", {})[extension] = {"emissiveStrength": 2.25}
    doc.setdefault("extensionsUsed", []).append(extension)


def prepare(source, output):
    data = source.read_bytes()
    magic, version, _, json_len, json_kind = struct.unpack_from("<4sIIII", data)
    if (magic, version, json_kind) != (b"glTF", 2, 0x4E4F534A):
        raise ValueError("Expected a glTF 2.0 binary")
    doc = json.loads(data[20 : 20 + json_len])
    binary_len, binary_kind = struct.unpack_from("<II", data, 20 + json_len)
    if binary_kind != 0x004E4942:
        raise ValueError("Expected a GLB binary buffer")
    binary = bytearray(data[28 + json_len : 28 + json_len + binary_len])
    (mesh,) = doc["meshes"]
    (primitive,) = mesh["primitives"]
    attributes = primitive["attributes"]

    def layout(index):
        accessor = doc["accessors"][index]
        view = doc["bufferViews"][accessor["bufferView"]]
        code = {5123: "H", 5125: "I", 5126: "f"}[accessor["componentType"]]
        count = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[accessor["type"]]
        fmt = struct.Struct("<" + code * count)
        start = view.get("byteOffset", 0)
        if accessor.get("byteOffset", 0) != 0 or view.get("byteStride", fmt.size) != fmt.size:
            raise ValueError("Expected --vertex-layout separate")
        return accessor, view, fmt, start

    def read(index):
        accessor, _, fmt, start = layout(index)
        return [fmt.unpack_from(binary, start + i * fmt.size) for i in range(accessor["count"])]

    def write(index, values):
        accessor, view, fmt, start = layout(index)
        accessor["count"] = len(values)
        view["byteLength"] = len(values) * fmt.size
        for i, v in enumerate(values):
            fmt.pack_into(binary, start + i * fmt.size, *v)
        for kind, calc in (("min", min), ("max", max)):
            if kind in accessor:
                accessor[kind] = [calc(xs) for xs in zip(*values, strict=True)]

    pos = read(attributes["POSITION"])
    normals = read(attributes["NORMAL"])
    tangents = read(attributes["TANGENT"])
    indices = [i for (i,) in read(primitive["indices"])]
    if output.stem == "thornbush":
        indices = remove_plaque(pos, indices)
    used = sorted(set(indices))
    base = min(pos[i][1] for i in used)
    remap = {i: new for new, i in enumerate(used)}
    for i in used:
        x, y, z = pos[i]
        pos[i] = (x, y - base, z)
        tx, ty, tz, sign = tangents[i]
        if math.hypot(tx, ty, tz) < 1e-5:
            nx, ny, nz = normals[i]
            tx, ty, tz = (0, -nz, ny) if abs(nx) < 0.8 else (nz, 0, -nx)
            length = math.hypot(tx, ty, tz)
            tangents[i] = (tx / length, ty / length, tz / length, sign)

    for name, values in [
        ("POSITION", pos),
        ("NORMAL", normals),
        ("TANGENT", tangents),
        ("TEXCOORD_0", read(attributes["TEXCOORD_0"])),
    ]:
        write(attributes[name], [values[i] for i in used])
    write(primitive["indices"], [(remap[i],) for i in indices])
    if output.stem == "glowweed":
        add_emission(doc, binary, doc["materials"][primitive["material"]])
    doc["nodes"][doc["scenes"][doc.get("scene", 0)]["nodes"][0]]["name"] = output.stem
    doc["buffers"][0]["byteLength"] = len(binary)
    binary.extend(b"\0" * (-len(binary) % 4))
    metadata = json.dumps(doc, separators=(",", ":")).encode()
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
