#!/usr/bin/env python3
"""Slim a dense Tripo model for the GPU: attribute-aware simplify, then cap its textures.

Tripo hard-surface models come at 1-2M triangles with 4096 px maps, about 150 MB of VRAM each once
loaded. This keeps their look while cutting most of that:

  * gltfpack simplifies within an error bound (default 0.1 % of the model's extent) and weighs the
    normals while it does, so flat panels stay flat. glTF Transform's simplify keeps vertices by
    position alone and crumples the shading on these scans (compared in renders, 2026-11-24);
  * glTF Transform resizes every texture down to at most --max-texture px (never up).

Named nodes keep their names and transforms (the game finds wheels, rotors and gun parts by name),
and the material and image count must not change; the script checks both. Run it on the source
before texture compression, then finish with `python3 tools/compress_glb_textures.py <target>`.

    python3 tools/slim_dense_model.py source.glb assets/models/vehicles/medium/model.glb \
        [--ratio 0.15] [--error 0.001] [--max-texture 2048]

Requires Node (npx fetches gltfpack and @gltf-transform/cli).
"""

import argparse
import json
import math
import struct
import subprocess
import tempfile
from pathlib import Path


def load(path):
    data = Path(path).read_bytes()
    (length,) = struct.unpack_from("<I", data, 12)
    return json.loads(data[20 : 20 + length])


def triangles(gltf):
    accessors = gltf["accessors"]
    return sum(
        accessors[prim["indices"]]["count"] // 3
        for mesh in gltf["meshes"]
        for prim in mesh["primitives"]
    )


def named_nodes(gltf):
    return {node["name"]: node for node in gltf["nodes"] if "name" in node}


def same_transform(a, b):
    for key, rest in (("translation", [0, 0, 0]), ("rotation", [0, 0, 0, 1]), ("scale", [1, 1, 1])):
        if not all(math.isclose(x, y, abs_tol=1e-6) for x, y in zip(a.get(key, rest), b.get(key, rest))):
            return False
    return True


def verify(before, after):
    """Raises unless the slimmed model keeps every named node, its transform, and the materials."""
    old, new = named_nodes(before), named_nodes(after)
    missing = sorted(set(old) - set(new))
    if missing:
        raise ValueError(f"named nodes lost: {missing}")
    moved = sorted(name for name in old if not same_transform(old[name], new[name]))
    if moved:
        raise ValueError(f"named nodes moved: {moved}")
    for key in ("materials", "images"):
        if len(before.get(key, [])) != len(after.get(key, [])):
            raise ValueError(f"{key} count changed")


def run(*command):
    subprocess.run(["npx", "-y", *command], check=True, capture_output=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("source", type=Path)
    parser.add_argument("target", type=Path)
    parser.add_argument("--ratio", type=float, default=0.15, help="target triangle ratio (the error bound wins)")
    parser.add_argument("--error", type=float, default=0.001, help="max deviation, relative to the model extent")
    parser.add_argument("--max-texture", type=int, default=2048)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as scratch:
        simplified = Path(scratch) / "simplified.glb"
        run("gltfpack", "-i", str(args.source), "-o", str(simplified),
            "-noq", "-kn", "-km", "-ke", "-si", str(args.ratio), "-se", str(args.error))
        resized = Path(scratch) / "resized.glb"
        size = str(args.max_texture)
        run("@gltf-transform/cli", "resize", str(simplified), str(resized), "--width", size, "--height", size)
        before, after = load(args.source), load(resized)
        verify(before, after)
        args.target.write_bytes(resized.read_bytes())
    print(f"{args.target}: {triangles(before)} -> {triangles(after)} triangles, textures <= {size} px")


if __name__ == "__main__":
    main()
