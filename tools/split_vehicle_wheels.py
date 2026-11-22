"""Split a Tripo vehicle scan into a chassis and four rolling wheel nodes for AI traffic.

Each wheel is cut out of the body by a cylinder around its axle (X axis), so the
game can roll a wheel about its local X and steer the front pair about local Y.
The body keeps a small hole behind each wheel; the wheel hides it.

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

Finish each GLB last (Blender cannot read the KTX2 output):
  python3 tools/compress_glb_textures.py assets/models/vehicles/medium/<model>.glb
  npx @gltf-transform/cli validate assets/models/vehicles/medium/<model>.glb

Preview before compression (the textures must still be readable):
  blender -b --factory-startup --python-exit-code 1 --python \
      ~/Projects/agentic/agentic-tooling/media/glb-preview/glb_preview.py -- \
      <model>.glb <prefix> --views front,right,top

Measured in the grounded source frame (glTF axes, before scaling to length 1.0):
  sedan   wheel radius 0.077, axle y 0.078, front z +0.32, rear z -0.25, tire |x| 0.18..0.20
  sweeper wheel radius 0.078, axle y 0.085, front z +0.11, rear z -0.27, tire |x| 0.12..0.19
The side brush of the sweeper shares its front bumper zone, so it stays in the chassis.

Blender coords: Z up. glTF (x, y, z) maps to Blender (x, -z, y).
"""

import argparse
import math
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

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


def split(body, config):
    groups = {}
    for index, polygon in enumerate(body.data.polygons):
        key = wheel_key(to_gltf(polygon.center), config)
        if key:
            groups.setdefault(key, set()).add(index)

    wheels = []
    for (axle, side), faces in sorted(groups.items()):
        wheel = keep_faces(body, faces)
        wheel.name = f"Wheel{axle}{side}"
        points = [to_gltf(v.co) for v in wheel.data.vertices]
        x_mid = (min(p[0] for p in points) + max(p[0] for p in points)) / 2
        center = (x_mid, config["axle_y"], config["axles"][axle])
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

    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.export_scene.gltf(
        filepath=args.dst,
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        export_tangents=True,
        export_image_format="AUTO",
    )


main()
