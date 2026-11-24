"""Extract one vehicle from a Tripo scan that fuses several vehicles into one mesh.

The scan's faces are unwelded (every UV island is its own piece), so the faces are
first grouped into shells that share vertex positions. Each shell goes to the vehicle
whose footprint box lies nearest its centroid; where two vehicles touch or overlap,
a shell (a wheel, a beacon, a body panel) stays whole with the vehicle it belongs to.
The chosen vehicle is turned so its nose points to glTF +Z, centered on x and z and
grounded (lowest vertex at y = 0). Every face, vertex and UV of that vehicle is kept,
and so is the scan's texture: nothing is simplified or resized.

The output is one mesh, ready for tools/split_vehicle_wheels.py:

  B="blender -b --factory-startup --python-exit-code 1"
  SRC=~/Projects/agentic/RoadSide-Picnic/models/vehicles/uhd/vintage-emergency-vehicles.glb
  T=<scratchpad>/emergency
  for v in ambulance fire-truck panel-van; do
    $B --python tools/extract_fused_vehicles.py -- $SRC $T/$v.glb --vehicle $v
    $B --python tools/split_vehicle_wheels.py -- $T/$v.glb \\
        assets/models/vehicles/medium/$v-traffic.glb --model $v
  done

(see tools/split_vehicle_wheels.py for the compression and validation that finish them).

Blender coords: Z up. glTF (x, y, z) maps to Blender (x, -z, y).
"""

import argparse
import math
import sys

import bmesh
import bpy
from mathutils import Matrix

# Each vehicle of vintage-emergency-vehicles.glb, measured in the scan's glTF frame:
# the center of its footprint on x/z, the heading of its nose (degrees from +X toward
# +Z), the footprint's half length and half width, and its height span. "pitch" lifts
# the nose by that many degrees where the scan sits nose-down (the ambulance's rear
# tires float 0.014 above the ground otherwise). The police car is not extracted for
# traffic, but its box must claim its own shells.
VEHICLES = {
    "police-car": {"center": (-0.2335, -0.1887), "heading": 67.2, "half": (0.299, 0.120), "y": (0.0, 0.244)},
    "fire-truck": {"center": (0.2378, 0.1736), "heading": 65.2, "half": (0.278, 0.123), "y": (0.268, 0.541)},
    "panel-van": {"center": (0.2162, 0.1639), "heading": 77.6, "half": (0.337, 0.122), "y": (0.008, 0.258)},
    "ambulance": {
        "center": (-0.2041, -0.2733),
        "heading": 59.7,
        "half": (0.226, 0.142),
        "y": (0.260, 0.533),
        "pitch": 3.6,
    },
}


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("src")
    parser.add_argument("dst")
    parser.add_argument("--vehicle", choices=sorted(VEHICLES), required=True)
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def import_scan(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=path)
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if len(meshes) != 1:
        raise ValueError(f"Expected one mesh object, found {len(meshes)}")
    scan = meshes[0]
    bpy.ops.object.select_all(action="DESELECT")
    scan.select_set(True)
    bpy.context.view_layer.objects.active = scan
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return scan


def shells(mesh):
    """Group the polygon indices into shells joined by shared vertex positions (0.5 mm)."""
    position = {}
    welded = [position.setdefault(tuple(round(c * 2000) for c in v.co), len(position)) for v in mesh.vertices]
    parent = list(range(len(position)))

    def root(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    for polygon in mesh.polygons:
        first = root(welded[polygon.vertices[0]])
        for vertex in polygon.vertices[1:]:
            other = root(welded[vertex])
            if other != first:
                parent[other] = first
    groups = {}
    for polygon in mesh.polygons:
        groups.setdefault(root(welded[polygon.vertices[0]]), []).append(polygon.index)
    return list(groups.values())


def box_distance(point, box):
    """How far the glTF point lies outside the vehicle's footprint box (0 inside)."""
    x, y, z = point
    cx, cz = box["center"]
    heading = math.radians(box["heading"])
    along = (x - cx) * math.cos(heading) + (z - cz) * math.sin(heading)
    across = -(x - cx) * math.sin(heading) + (z - cz) * math.cos(heading)
    outside = [
        max(abs(along) - box["half"][0], 0.0),
        max(abs(across) - box["half"][1], 0.0),
        max(box["y"][0] - y, 0.0, y - box["y"][1]),
    ]
    return math.hypot(*outside)


def vehicle_faces(mesh, name):
    faces = set()
    for shell in shells(mesh):
        corners = [mesh.vertices[i].co for f in shell for i in mesh.polygons[f].vertices]
        centroid = [sum(c[k] for c in corners) / len(corners) for k in range(3)]
        gltf = (centroid[0], centroid[2], -centroid[1])
        owner = min(VEHICLES, key=lambda vehicle: box_distance(gltf, VEHICLES[vehicle]))
        if owner == name:
            faces.update(shell)
    return faces


def main():
    args = parse_args()
    scan = import_scan(args.src)
    keep = vehicle_faces(scan.data, args.vehicle)

    bm = bmesh.new()
    bm.from_mesh(scan.data)
    bmesh.ops.delete(bm, geom=[face for face in bm.faces if face.index not in keep], context="FACES")
    bm.to_mesh(scan.data)
    bm.free()

    # Move the footprint's center to the origin, then turn the nose to glTF +Z
    # (Blender -Y): a glTF heading h is the Blender angle -h, turned to -90 degrees.
    box = VEHICLES[args.vehicle]
    cx, cz = box["center"]
    scan.data.transform(Matrix.Translation((-cx, cz, 0.0)))
    scan.data.transform(Matrix.Rotation(math.radians(box["heading"] - 90.0), 4, "Z"))
    # Turning about Blender -X raises the nose (Blender -Y).
    scan.data.transform(Matrix.Rotation(math.radians(-box.get("pitch", 0.0)), 4, "X"))
    lowest = min(v.co.z for v in scan.data.vertices)
    scan.data.transform(Matrix.Translation((0.0, 0.0, -lowest)))
    scan.name = args.vehicle
    print(f"EXTRACT {args.vehicle} faces {len(scan.data.polygons)} vertices {len(scan.data.vertices)}")

    bpy.ops.export_scene.gltf(
        filepath=args.dst,
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        export_image_format="AUTO",
    )


if __name__ == "__main__":
    main()
