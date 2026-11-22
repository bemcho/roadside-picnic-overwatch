"""Ground the warehouse GLB's imported supports and freestanding floor props.

python3 tools/ground_warehouse.py original_warehouse.glb assets/models/base/warehouse.glb
"""

import argparse
import json
import math
import struct
from pathlib import Path


POST_BAND = 0.15
FLOOR_PROP_HEIGHT = 0.21
MAX_FLOOR_GAP = 0.055


def connected_parts(adjacency, vertices):
    remaining = set(vertices)
    while remaining:
        stack = [remaining.pop()]
        part = []
        while stack:
            vertex = stack.pop()
            part.append(vertex)
            neighbors = adjacency[vertex] & remaining
            remaining.difference_update(neighbors)
            stack.extend(neighbors)
        yield part


def ground(in_path, out_path):
    data = in_path.read_bytes()
    _, length, kind = struct.unpack_from("<III", data, 8)
    binary_length, binary_kind = struct.unpack_from("<II", data, 20 + length)
    if data[:8] != b"glTF\x02\0\0\0" or kind != 0x4E4F534A or binary_kind != 0x004E4942:
        raise ValueError("Expected a GLB with JSON and binary chunks")
    scene = json.loads(data[20 : 20 + length])
    binary = bytearray(data[28 + length : 28 + length + binary_length])
    (mesh,) = scene["meshes"]
    (primitive,) = mesh["primitives"]
    attributes = primitive["attributes"]

    def layout(index, code):
        accessor = scene["accessors"][index]
        view = scene["bufferViews"][accessor["bufferView"]]
        start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", code.size)
        return range(start, start + accessor["count"] * stride, stride)

    def read(index, fmt):
        code = struct.Struct(fmt)
        return [code.unpack_from(binary, offset) for offset in layout(index, code)]

    positions = read(attributes["POSITION"], "<3f")
    normals = read(attributes["NORMAL"], "<3f")
    index = primitive["indices"]
    indices = read(index, {5123: "<H", 5125: "<I"}[scene["accessors"][index]["componentType"]])

    # Weld for connectivity only; keep the original UV and normal seams in the output.
    welded = {}
    heights = []
    vertex_ids = []
    for position in positions:
        key = tuple(round(axis, 5) for axis in position)
        vertex_id = welded.setdefault(key, len(heights))
        if vertex_id == len(heights):
            heights.append(position[1])
        heights[vertex_id] = min(position[1], heights[vertex_id])
        vertex_ids.append(vertex_id)

    adjacency = [set() for _ in heights]
    for a, b, c in zip(*(iter(indices),) * 3):
        a, b, c = (vertex_ids[index[0]] for index in (a, b, c))
        adjacency[a].update((b, c))
        adjacency[b].update((a, c))
        adjacency[c].update((a, b))

    offsets = {}
    counts = {"supports": 0, "floor props": 0}
    for part in connected_parts(adjacency, range(len(heights))):
        low, high = min(heights[i] for i in part), max(heights[i] for i in part)
        if len(part) < 64 or low >= MAX_FLOOR_GAP:
            continue
        if high <= FLOOR_PROP_HEIGHT:
            if low > 0.0001:
                offsets.update((index, (low, 1.0)) for index in part)
                counts["floor props"] += 1
            continue
        for foot in connected_parts(adjacency, (index for index in part if heights[index] < POST_BAND)):
            gap = min(heights[index] for index in foot)
            if len(foot) < 64 or not 0.0001 < gap < MAX_FLOOR_GAP:
                continue
            if max(heights[index] for index in foot) < POST_BAND - 0.02:
                continue
            # Keep each footplate intact and ease the extension to zero up the post.
            plate_top = gap + 0.012
            span = POST_BAND - plate_top
            for index in foot:
                t = max(0.0, (heights[index] - plate_top) / span)
                drop = gap * (1.0 - t * t * (3.0 - 2.0 * t))
                stretch = 1.0 + gap * 6.0 * t * (1.0 - t) / span
                offsets[index] = (drop, stretch)
            counts["supports"] += 1

    code = struct.Struct("<3f")
    for i, (pos, normal) in enumerate(zip(layout(attributes["POSITION"], code), layout(attributes["NORMAL"], code))):
        if vertex_ids[i] not in offsets:
            continue
        drop, stretch = offsets[vertex_ids[i]]
        x, y, z = positions[i]
        positions[i] = (x, y - drop, z)
        code.pack_into(binary, pos, *positions[i])
        if stretch != 1.0:
            nx, ny, nz = normals[i]
            vector = (nx, ny / stretch, nz)
            scale = math.hypot(*vector)
            if scale:
                code.pack_into(binary, normal, *(value / scale for value in vector))

    position = scene["accessors"][attributes["POSITION"]]
    position["min"] = [min(values) for values in zip(*positions)]
    position["max"] = [max(values) for values in zip(*positions)]
    metadata = json.dumps(scene, separators=(",", ":")).encode()
    metadata += b" " * (-len(metadata) % 4)
    out_path.write_bytes(
        struct.pack("<4sIIII", b"glTF", 2, 28 + len(metadata) + len(binary), len(metadata), kind)
        + metadata + struct.pack("<II", len(binary), binary_kind) + binary
    )
    print(f"Grounded {counts['supports']} supports and {counts['floor props']} floor props")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    ground(args.source, args.output)
