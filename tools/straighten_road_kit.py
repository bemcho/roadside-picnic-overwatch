"""Put the old-road kit pieces back together as one straight, even-width road module.

The road scan (Tripo, split into parts with split-parts) is one stretch of cracked asphalt
broken into slabs that fit together like a jigsaw, so the pieces cannot be squared up one by
one. Generated from a perspective image, the stretch also widens from one end to the other.
This takes each kit piece's pose from the assembled split scan and rewrites the piece in place
in a shared road frame: +Z runs along the road, +X across it, y = 0 is the typical asphalt
top. The frame is levelled on the asphalt top and the taper is straightened to the mean width,
so copies of the whole module, all at one transform, tile end to end along a road.

    python3 tools/straighten_road_kit.py <assembled_split_scan.glb> assets/models/road/uhd/road_*.glb

A piece's mesh index in the scan is the number in its file name (road_07.glb is mesh 7).
The source pieces must still have the scan's mesh-local coordinates, apart from a vertical
grounding shift, which is detected and undone. Prints the module's length, width and depths.
Run it before tools/compress_glb_textures.py (it reads only positions and normals, so either
order works).
"""

import argparse
import json
import re
import struct
from pathlib import Path

import numpy as np

# Cell size (scan units) for the top-down rasters of the asphalt top.
CELL = 0.004
BINS = 10


def read(path):
    data = path.read_bytes()
    json_len = struct.unpack_from("<I", data, 12)[0]
    document = json.loads(data[20 : 20 + json_len])
    binary_len = struct.unpack_from("<I", data, 20 + json_len)[0]
    return document, bytearray(data[28 + json_len : 28 + json_len + binary_len])


def write(path, document, binary):
    metadata = json.dumps(document, separators=(",", ":")).encode()
    metadata += b" " * (-len(metadata) % 4)
    header = struct.pack("<4sIIII", b"glTF", 2, 28 + len(metadata) + len(binary), len(metadata), 0x4E4F534A)
    path.write_bytes(header + metadata + struct.pack("<II", len(binary), 0x004E4942) + binary)


def vec3(document, binary, accessor_index):
    """A writable (count, 3) float32 view of a VEC3 accessor, strided or packed."""
    accessor = document["accessors"][accessor_index]
    view = document["bufferViews"][accessor["bufferView"]]
    stride = view.get("byteStride", 12)
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    rows = np.frombuffer(binary, dtype=np.uint8, count=stride * accessor["count"], offset=start)
    return accessor, rows.reshape(accessor["count"], stride)[:, :12].view("<f4")


def scan_parts(path):
    """Each mesh's vertices in the assembled scan, and its mesh-local minimum y."""
    document, binary = read(path)
    parts = {}
    for node in document["nodes"]:
        if "mesh" not in node:
            continue
        if any(key in node for key in ("rotation", "scale", "matrix")):
            raise ValueError("expected translation-only part nodes")
        primitive = document["meshes"][node["mesh"]]["primitives"][0]
        accessor, local = vec3(document, binary, primitive["attributes"]["POSITION"])
        offset = np.array(node.get("translation", [0.0, 0.0, 0.0]))
        parts[node["mesh"]] = (local.astype(np.float64) + offset, offset, accessor["min"][1])
    return parts


class Frame:
    """Scan coordinates to the straight road frame, for points and normals."""

    def __init__(self, points):
        flat = points[:, [0, 2]]
        self.center = flat.mean(axis=0)
        _, vectors = np.linalg.eigh(np.cov((flat - self.center).T))
        along = vectors[:, 1] if vectors[0, 1] > 0 else -vectors[:, 1]
        # Right-handed: x = y × z with z along the road.
        self.rotation = np.array([[along[1], 0.0, -along[0]], [0.0, 1.0, 0.0], [along[0], 0.0, along[1]]])
        turned = self.turn(points)
        self.length_range = (turned[:, 2].min(), turned[:, 2].max())
        self.level = self.fit_top_plane(turned)
        levelled = self.apply_level(turned)
        self.edges = self.fit_edges(levelled)
        self.width = float(np.mean([hi - lo for lo, hi in self.edge_samples]))

    def turn(self, points):
        shifted = points - np.array([self.center[0], 0.0, self.center[1]])
        return shifted @ self.rotation.T

    @staticmethod
    def top_surface(points):
        """The highest point in each top-down cell: (x, z, top y) rows."""
        cells = np.floor(points[:, [0, 2]] / CELL).astype(np.int64)
        order = np.lexsort((points[:, 1], cells[:, 1], cells[:, 0]))
        cells, ordered = cells[order], points[order]
        last = np.r_[np.any(cells[1:] != cells[:-1], axis=1), True]
        return ordered[last]

    def fit_top_plane(self, turned):
        top = self.top_surface(turned)
        design = np.c_[top[:, 0], top[:, 2], np.ones(len(top))]
        coefficients, *_ = np.linalg.lstsq(design, top[:, 1], rcond=None)
        return coefficients  # y = a x + b z + c

    def apply_level(self, turned):
        a, b, c = self.level
        levelled = turned.copy()
        levelled[:, 1] -= a * turned[:, 0] + b * turned[:, 2] + c
        return levelled

    def fit_edges(self, levelled):
        low, high = self.length_range
        middles, lows, highs = [], [], []
        self.edge_samples = []
        for start in np.linspace(low, high, BINS + 1)[:-1]:
            inside = (levelled[:, 2] >= start) & (levelled[:, 2] < start + (high - low) / BINS)
            across = levelled[inside, 0]
            lo, hi = np.percentile(across, 1), np.percentile(across, 99)
            middles.append(start + (high - low) / BINS / 2)
            lows.append(lo)
            highs.append(hi)
            self.edge_samples.append((lo, hi))
        return np.polyfit(middles, lows, 1), np.polyfit(middles, highs, 1)

    def straighten(self, levelled, normals):
        (l1, l0), (h1, h0) = self.edges
        z = levelled[:, 2]
        width = (h1 - l1) * z + (h0 - l0)
        middle = ((l1 + h1) * z + (l0 + h0)) / 2
        scale = self.width / width
        d_scale = -self.width * (h1 - l1) / width**2
        d_middle = (l1 + h1) / 2
        across = levelled[:, 0] - middle
        out = levelled.copy()
        out[:, 0] = scale * across
        # Normals take the inverse transpose of the deformation's Jacobian.
        shear = (d_scale * across - scale * d_middle) / scale
        bent = normals.copy()
        bent[:, 0] = normals[:, 0] / scale
        bent[:, 2] = normals[:, 2] - normals[:, 0] * shear
        return out, bent

    def points(self, points, normals):
        turned = self.turn(points)
        turned_normals = normals @ self.rotation.T
        levelled = self.apply_level(turned)
        a, b, _ = self.level
        level_normals = turned_normals.copy()
        level_normals[:, 0] += a * turned_normals[:, 1]
        level_normals[:, 2] += b * turned_normals[:, 1]
        straight, straight_normals = self.straighten(levelled, level_normals)
        straight[:, 2] -= sum(self.length_range) / 2
        lengths = np.linalg.norm(straight_normals, axis=1, keepdims=True)
        return straight, straight_normals / np.maximum(lengths, 1e-12)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("scan", type=Path, help="the assembled, textured split scan")
    parser.add_argument("pieces", type=Path, nargs="+")
    arguments = parser.parse_args()

    parts = scan_parts(arguments.scan)
    frame = Frame(np.concatenate([points for points, _, _ in parts.values()]))
    kit = []
    for path in arguments.pieces:
        mesh = int(re.search(r"(\d+)\.glb$", path.name).group(1))
        document, binary = read(path)
        attributes = document["meshes"][0]["primitives"][0]["attributes"]
        if "TANGENT" in attributes:
            raise ValueError(f"{path}: straighten before baking tangents")
        accessor, positions = vec3(document, binary, attributes["POSITION"])
        _, normals = vec3(document, binary, attributes["NORMAL"])
        _, offset, scan_min_y = parts[mesh]
        grounding = accessor["min"][1] - scan_min_y
        scan = positions.astype(np.float64) + offset - np.array([0.0, grounding, 0.0])
        kit.append((path, document, binary, accessor, positions, normals, scan))

    module = [frame.points(scan, normals.astype(np.float64)) for *_, normals, scan in kit]
    top = Frame.top_surface(np.concatenate([points for points, _ in module]))
    surface = np.median(top[:, 1])
    for (path, document, binary, accessor, positions, normals, _), (points, bent) in zip(kit, module):
        points[:, 1] -= surface
        positions[:] = points.astype(np.float32)
        normals[:] = bent.astype(np.float32)
        accessor["min"] = points.min(axis=0).tolist()
        accessor["max"] = points.max(axis=0).tolist()
        for node in document.get("nodes", []):
            if "mesh" in node:
                for key in ("translation", "rotation", "scale", "matrix"):
                    node.pop(key, None)
        write(path, document, binary)
        size = points.max(axis=0) - points.min(axis=0)
        print(f"{path.name}: x {points[:, 0].min():+.4f}..{points[:, 0].max():+.4f} "
              f"z {points[:, 2].min():+.4f}..{points[:, 2].max():+.4f} "
              f"y {points[:, 1].min():+.4f}..{points[:, 1].max():+.4f} size {size.round(4).tolist()}")

    heights = top[:, 1] - surface
    everything = np.concatenate([points for points, _ in module])
    print(f"module: length {np.ptp(everything[:, 2]):.4f} mean width {frame.width:.4f} "
          f"(scan tapered {frame.edge_samples[0][1] - frame.edge_samples[0][0]:.4f}"
          f"..{frame.edge_samples[-1][1] - frame.edge_samples[-1][0]:.4f})")
    print(f"asphalt top relief p5/p50/p95/max: {np.percentile(heights, 5):+.4f} {np.percentile(heights, 50):+.4f} "
          f"{np.percentile(heights, 95):+.4f} {heights.max():+.4f}; lowest point {everything[:, 1].min():+.4f}")


if __name__ == "__main__":
    main()
