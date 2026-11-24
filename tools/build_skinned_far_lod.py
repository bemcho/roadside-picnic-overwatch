"""Build a texture-free far LOD of a rigged, single-material GLB.

The game draws the far mesh with the near model's material (see `world/townsfolk.rs`) and drives
its skeleton with the near model's clips, so the LOD must keep the source's nodes, skin and
animations unchanged: glTF Transform's simplify touches only the mesh, unlike a Blender round
trip, which may re-pose the bones. The images are dropped first, so the far file holds geometry,
skin and clips only (about 1 MB instead of 30 MB of 4K textures).

    python3 tools/build_skinned_far_lod.py \
        assets/models/townsfolk/medium/scientist.glb assets/models/townsfolk/low/scientist.glb

`--error` is meshoptimizer's error bound relative to the model's size (default 0.001, about
2 mm on a townsperson): the far tier is seen from 35 m and beyond. Static models use
`tools/build_far_lod.py` instead. Finish with `compress_glb_textures.py` like any runtime GLB.
"""

import argparse
import json
import struct
import subprocess
import tempfile
from pathlib import Path

GLB_MAGIC, JSON_CHUNK = 0x46546C67, 0x4E4F534A


def without_textures(source, target):
    """Copy a GLB, dropping its images, textures and every material's texture slots.

    The image bytes stay in the binary chunk unreferenced; glTF Transform drops them on reading.
    """
    data = source.read_bytes()
    magic, _, _ = struct.unpack_from("<III", data)
    if magic != GLB_MAGIC:
        raise ValueError(f"{source} is not a GLB")
    length, kind = struct.unpack_from("<II", data, 12)
    if kind != JSON_CHUNK:
        raise ValueError(f"{source} has no JSON chunk first")
    gltf = json.loads(data[20 : 20 + length])
    rest = data[20 + length :]
    if len(gltf.get("materials", [])) != 1:
        raise ValueError(f"{source} must have exactly one material")
    for key in ("images", "textures", "samplers"):
        gltf.pop(key, None)
    for material in gltf["materials"]:
        pbr = material.get("pbrMetallicRoughness", {})
        for slot in ("baseColorTexture", "metallicRoughnessTexture"):
            pbr.pop(slot, None)
        for slot in ("normalTexture", "occlusionTexture", "emissiveTexture"):
            material.pop(slot, None)
    text = json.dumps(gltf).encode()
    text += b" " * (-len(text) % 4)
    header = struct.pack("<III", GLB_MAGIC, 2, 12 + 8 + len(text) + len(rest))
    target.write_bytes(header + struct.pack("<II", len(text), JSON_CHUNK) + text + rest)


def gltf_transform(*args):
    subprocess.run(["npx", "--yes", "@gltf-transform/cli", *map(str, args)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("source", type=Path)
    parser.add_argument("target", type=Path)
    parser.add_argument("--error", type=float, default=0.001)
    args = parser.parse_args()

    args.target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as scratch:
        plain = Path(scratch) / "plain.glb"
        without_textures(args.source, plain)
        # No prune: with the textures gone it would drop the UVs and tangents the near
        # material needs.
        gltf_transform("simplify", plain, args.target, "--ratio", 0, "--error", args.error)


if __name__ == "__main__":
    main()
