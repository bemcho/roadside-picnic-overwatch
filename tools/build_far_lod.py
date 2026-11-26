"""Build a light, texture-free far LOD of a single-primitive GLB in headless Blender.

The game draws the far mesh with the near model's material (see `world/plants.rs`), so the LOD
keeps only geometry: positions, smooth normals and UVs in the source's exact frame, as one
primitive with a plain material and no images. glTF Transform's simplify stalls on foliage
(UV seams and loose leaf cards); Blender's collapse decimation reaches any triangle budget.

    BLENDER=~/.local/blender-4.5.16-linux-x64/blender
    "$BLENDER" -b --factory-startup --python-exit-code 1 -P tools/build_far_lod.py -- \
        assets/models/plants/medium/shrub.glb assets/models/plants/low/shrub.glb --triangles 450

The source may embed KTX2 textures (Blender cannot read them): they are stripped before import.
"""

import argparse
import json
import struct
import sys
import tempfile
from pathlib import Path

import bpy

GLB_MAGIC, JSON_CHUNK, BIN_CHUNK = 0x46546C67, 0x4E4F534A, 0x004E4942


def without_textures(source, target):
    """Copy a GLB, dropping its images, textures and every material's texture slots."""
    data = source.read_bytes()
    magic, _, _ = struct.unpack_from("<III", data)
    if magic != GLB_MAGIC:
        raise ValueError(f"{source} is not a GLB")
    length, kind = struct.unpack_from("<II", data, 12)
    if kind != JSON_CHUNK:
        raise ValueError(f"{source} has no JSON chunk first")
    gltf = json.loads(data[20 : 20 + length])
    rest = data[20 + length :]
    for key in ("images", "textures", "samplers"):
        gltf.pop(key, None)
    for material in gltf.get("materials", []):
        pbr = material.get("pbrMetallicRoughness", {})
        for slot in ("baseColorTexture", "metallicRoughnessTexture"):
            pbr.pop(slot, None)
        for slot in ("normalTexture", "occlusionTexture", "emissiveTexture"):
            material.pop(slot, None)
        material.pop("extensions", None)
    gltf.pop("extensionsUsed", None)
    gltf.pop("extensionsRequired", None)
    text = json.dumps(gltf).encode()
    text += b" " * (-len(text) % 4)
    header = struct.pack("<III", GLB_MAGIC, 2, 12 + 8 + len(text) + len(rest))
    target.write_bytes(header + struct.pack("<II", len(text), JSON_CHUNK) + text + rest)


def triangles(obj):
    return sum(len(polygon.vertices) - 2 for polygon in obj.data.polygons)


def decimated(obj, budget):
    """Collapse-decimate a copy of `obj` down to at most `budget` triangles."""
    ratio = min(1.0, budget / max(triangles(obj), 1))
    for _ in range(8):
        trial = obj.copy()
        trial.data = obj.data.copy()
        bpy.context.collection.objects.link(trial)
        modifier = trial.modifiers.new("far", "DECIMATE")
        modifier.decimate_type = "COLLAPSE"
        modifier.use_collapse_triangulate = True
        modifier.ratio = ratio
        bpy.context.view_layer.objects.active = trial
        bpy.ops.object.modifier_apply(modifier="far")
        count = triangles(trial)
        if count <= budget:
            return trial
        bpy.data.objects.remove(trial)
        ratio *= budget / count * 0.95
    raise RuntimeError(f"could not decimate below {budget} triangles")


def main():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("target", type=Path)
    parser.add_argument("--triangles", type=int, default=450)
    args = parser.parse_args(argv)

    bpy.ops.wm.read_factory_settings(use_empty=True)
    with tempfile.TemporaryDirectory() as scratch:
        plain = Path(scratch) / "plain.glb"
        without_textures(args.source.resolve(), plain)
        bpy.ops.import_scene.gltf(filepath=str(plain))
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if len(meshes) != 1 or len(meshes[0].data.materials) > 1:
        raise ValueError(f"{args.source} is not a single-primitive model")
    source = meshes[0]
    if any(abs(v) > 1e-6 for v in (*source.location, *source.rotation_euler)):
        raise ValueError(f"{args.source} has a node transform; bake it first")

    far = decimated(source, args.triangles)
    bpy.data.objects.remove(source)
    bpy.context.view_layer.objects.active = far
    far.select_set(True)
    # The imported custom normals do not survive collapsing; shade from the new faces.
    if far.data.has_custom_normals:
        bpy.ops.mesh.customdata_custom_splitnormals_clear()
    bpy.ops.object.shade_smooth()

    args.target.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(args.target.resolve()),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_materials="EXPORT",
        export_image_format="NONE",
        export_animations=False,
        export_skins=False,
        export_morph=False,
    )
    print(f"{args.target}: {triangles(far)} triangles")


main()
