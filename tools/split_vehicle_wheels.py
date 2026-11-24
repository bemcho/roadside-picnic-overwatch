"""Split a Tripo vehicle scan into a chassis and four rolling wheel nodes for AI traffic.

Each wheel is cut out of the body by a cylinder around its axle (X axis), so the
game can roll a wheel about its local X and steer the front pair about local Y.
The body keeps a small hole behind each wheel; the wheel hides it. Where the scan
models each wheel as its own shell ("shells" below), the wheel takes whole shells
whose centroid lies in the cylinder and is centered on them, so the body keeps no
hole and an unevenly placed wheel still turns about its own axle.

Output matches assets/models/vehicles/medium/armored_vehicle.glb: top-level nodes
Chassis, WheelFrontRight, WheelFrontLeft, WheelRearRight, WheelRearLeft; each wheel
node sits at its axle center with identity rotation; glTF +Z front, +Y up, +X the
vehicle's left; grounded (wheel bottoms at y = 0); length z = -0.5..0.5.

Sources and the reduction that runs before this script (weld, simplify, resize keep
the source's UVs; Blender re-exports tangents for normal maps):

  G="npx @gltf-transform/cli@4"
  T=<scratchpad>/traffic

  # sedan: the UHD scan (its nose points to -Z, so it is turned here)
  $G weld ~/Projects/agentic/RoadSide-Picnic/models/vehicles/uhd/vehicle-sedan.glb $T/sedan_weld.glb
  $G simplify $T/sedan_weld.glb $T/sedan_simp.glb --ratio 0.01 --error 0.002
  $G resize $T/sedan_simp.glb $T/sedan_small.glb --width 1024 --height 1024
  blender -b --factory-startup --python-exit-code 1 --python tools/split_vehicle_wheels.py -- \
      $T/sedan_small.glb assets/models/vehicles/medium/sedan-traffic.glb --model sedan

  # street sweeper: the Tripo scan (nose already +Z)
  $G weld ~/Downloads/street-sweeper-3d-model.glb $T/sweep_weld.glb
  $G simplify $T/sweep_weld.glb $T/sweep_simp.glb --ratio 0.0105 --error 0.002
  $G resize $T/sweep_simp.glb $T/sweep_small.glb --width 2048 --height 2048
  $G tangents $T/sweep_small.glb $T/sweep_tan.glb
  blender -b --factory-startup --python-exit-code 1 --python tools/split_vehicle_wheels.py -- \
      $T/sweep_tan.glb assets/models/vehicles/medium/street-sweeper.glb --model sweeper

  # ambulance, fire truck and panel van: three vehicles fused in one Tripo scan,
  # first pulled apart by tools/extract_fused_vehicles.py. Nothing is simplified
  # or resized: every face and the native 4096 texture are kept.
  SRC=~/Projects/agentic/RoadSide-Picnic/models/vehicles/uhd/vintage-emergency-vehicles.glb
  for v in ambulance fire-truck panel-van; do
    blender -b --factory-startup --python-exit-code 1 --python tools/extract_fused_vehicles.py -- \
        $SRC $T/$v.glb --vehicle $v
    blender -b --factory-startup --python-exit-code 1 --python tools/split_vehicle_wheels.py -- \
        $T/$v.glb assets/models/vehicles/medium/$v-traffic.glb --model $v
  done

Finish each GLB last (Blender cannot read the KTX2 output):
  python3 tools/compress_glb_textures.py assets/models/vehicles/medium/<model>.glb
  npx @gltf-transform/cli validate assets/models/vehicles/medium/<model>.glb
The three emergency vehicles share one atlas: finish them together, so it is written once as
textures/<image>.ktx2 beside them (assets::TRAFFIC_EMERGENCY_ATLAS) and each refers to it by URI:
  M=assets/models/vehicles/medium
  python3 tools/compress_glb_textures.py --external-images $M/textures \\
      $M/{ambulance,fire-truck,panel-van}-traffic.glb

Preview before compression (the textures must still be readable):
  blender -b --factory-startup --python-exit-code 1 --python \
      ~/Projects/agentic/agentic-tooling/media/glb-preview/glb_preview.py -- \
      <model>.glb <prefix> --views front,right,top

Measured in the grounded source frame (glTF axes, before scaling to length 1.0):
  sedan   wheel radius 0.077, axle y 0.078, front z +0.32, rear z -0.25, tire |x| 0.18..0.20
  sweeper wheel radius 0.078, axle y 0.085, front z +0.11, rear z -0.27, tire |x| 0.12..0.19
The side brush of the sweeper shares its front bumper zone, so it stays in the chassis.
The emergency vehicles' wheel nodes sit at their own shells' centers; the script prints
them (SPLIT ... node) in the final, length-1 frame for traffic.rs.

Blender coords: Z up. glTF (x, y, z) maps to Blender (x, -z, y).
"""

import argparse
import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.dont_write_bytecode = True  # keep tools/ free of __pycache__
from extract_fused_vehicles import shells  # noqa: E402

# Wheel centers (glTF frame, grounded, before scaling). "cut" is the cylinder radius
# that takes faces out of the body; "band" is the |x| range of the tire.
MODELS = {
    "sedan": {
        "turn_degrees": 180.0,
        "axle_y": 0.078,
        "cut": 0.086,
        "band": (0.12, 0.215),
        "axles": {"Front": 0.32, "Rear": -0.25},
    },
    "sweeper": {
        "turn_degrees": 0.0,
        "axle_y": 0.085,
        "cut": 0.088,
        "band": (0.115, 0.20),
        "axles": {"Front": 0.11, "Rear": -0.27},
    },
    # The emergency vehicles, after tools/extract_fused_vehicles.py: each wheel is
    # its own shell, so "cut" is how far a shell's centroid may sit from the axle.
    "ambulance": {
        "turn_degrees": 0.0,
        "shells": True,
        "axle_y": 0.042,
        "cut": 0.03,
        "band": (0.07, 0.14),
        "axles": {"Front": 0.085, "Rear": -0.144},
    },
    "fire-truck": {
        "turn_degrees": 0.0,
        "shells": True,
        "axle_y": 0.047,
        "cut": 0.03,
        "band": (0.07, 0.13),
        "axles": {"Front": 0.169, "Rear": -0.161},
    },
    "panel-van": {
        "turn_degrees": 0.0,
        "shells": True,
        "axle_y": 0.052,
        "cut": 0.03,
        "band": (0.05, 0.13),
        "axles": {"Front": 0.215, "Rear": -0.18},
    },
}


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("src")
    parser.add_argument("dst")
    parser.add_argument("--model", choices=sorted(MODELS), required=True)
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def to_gltf(v):
    return (v.x, v.z, -v.y)


def to_blender(x, y, z):
    return Vector((x, -z, y))


def import_body(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=path)
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if len(meshes) != 1:
        raise ValueError(f"Expected one mesh object, found {len(meshes)}")
    body = meshes[0]
    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return body


def wheel_key(center, config):
    """Return (axle, side) for a face centroid that lies in a wheel's cylinder, else None."""
    x, y, z = center
    if not config["band"][0] <= abs(x) <= config["band"][1]:
        return None
    for axle, axle_z in config["axles"].items():
        if (y - config["axle_y"]) ** 2 + (z - axle_z) ** 2 <= config["cut"] ** 2:
            return axle, "Left" if x > 0 else "Right"
    return None


def keep_faces(obj, faces):
    """Return a copy of obj that keeps only the given polygon indices."""
    copy = obj.copy()
    copy.data = obj.data.copy()
    bpy.context.collection.objects.link(copy)
    bm = bmesh.new()
    bm.from_mesh(copy.data)
    drop = [face for face in bm.faces if face.index not in faces]
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    bm.to_mesh(copy.data)
    bm.free()
    return copy


def wheel_groups(body, config):
    """Map (axle, side) to the polygon indices of that wheel."""
    groups = {}
    if config.get("shells"):
        for shell in shells(body.data):
            corners = [body.data.vertices[i].co for f in shell for i in body.data.polygons[f].vertices]
            centroid = sum(corners, Vector()) / len(corners)
            key = wheel_key(to_gltf(centroid), config)
            if key:
                groups.setdefault(key, set()).update(shell)
        return groups
    for index, polygon in enumerate(body.data.polygons):
        key = wheel_key(to_gltf(polygon.center), config)
        if key:
            groups.setdefault(key, set()).add(index)
    return groups


def split(body, config):
    groups = wheel_groups(body, config)
    if len(groups) != 4:
        raise ValueError(f"Expected four wheels, found {sorted(groups)}")

    wheels = []
    for (axle, side), faces in sorted(groups.items()):
        wheel = keep_faces(body, faces)
        wheel.name = f"Wheel{axle}{side}"
        points = [to_gltf(v.co) for v in wheel.data.vertices]
        mid = [(min(p[k] for p in points) + max(p[k] for p in points)) / 2 for k in range(3)]
        if config.get("shells"):
            center = tuple(mid)
        else:
            center = (mid[0], config["axle_y"], config["axles"][axle])
        wheel.data.transform(Matrix.Translation(-to_blender(*center)))
        wheel.location = to_blender(*center)
        wheels.append(wheel)
        print(f"SPLIT {wheel.name} tris {sum(len(p.vertices) - 2 for p in wheel.data.polygons)} "
              f"center {tuple(round(c, 4) for c in center)}")

    bm = bmesh.new()
    bm.from_mesh(body.data)
    cut = [face for face in bm.faces if face.index in set().union(*groups.values())]
    bmesh.ops.delete(bm, geom=cut, context="FACES")
    bm.to_mesh(body.data)
    bm.free()
    body.name = "Chassis"
    return [body, *wheels]


def normalize(objects):
    """Scale uniformly so the length runs z = -0.5..0.5, and center it on z.

    Every object keeps identity rotation, so its world point is location + vertex.
    """
    zs = [to_gltf(obj.location + v.co)[2] for obj in objects for v in obj.data.vertices]
    scale = 1.0 / (max(zs) - min(zs))
    shift = -(max(zs) + min(zs)) / 2 * scale
    for obj in objects:
        obj.data.transform(Matrix.Scale(scale, 4))
        obj.location = obj.location * scale + to_blender(0, 0, shift)
    return scale


def has_normal_map():
    return any(
        node.type == "NORMAL_MAP"
        for material in bpy.data.materials
        if material.node_tree
        for node in material.node_tree.nodes
    )


def main():
    args = parse_args()
    config = MODELS[args.model]
    body = import_body(args.src)

    if config["turn_degrees"]:
        body.data.transform(Matrix.Rotation(math.radians(config["turn_degrees"]), 4, "Z"))
    lowest = min(v.co.z for v in body.data.vertices)
    body.data.transform(Matrix.Translation((0, 0, -lowest)))

    objects = split(body, config)
    scale = normalize(objects)
    print(f"SPLIT scale {scale:.5f}")
    for wheel in objects[1:]:
        at = to_gltf(wheel.location)
        radius = max(to_gltf(v.co)[1] for v in wheel.data.vertices)
        print(f"SPLIT {wheel.name} node {tuple(round(c, 4) for c in at)} radius {radius:.4f}")

    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.export_scene.gltf(
        filepath=args.dst,
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        # Tangents only serve a normal map; without one they are dead weight.
        export_tangents=has_normal_map(),
        export_image_format="AUTO",
    )


main()
