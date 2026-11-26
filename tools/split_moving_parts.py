r"""Cut the moving parts out of a fused Tripo scan and re-parent them as pivoted nodes.

Tripo exports one fused mesh, so a part is chosen by geometric region in the source's
glTF frame: a face goes to the first part whose region holds its centroid (cylinder about
any axis, box, unions/intersections), or, where blades droop past any simple bound, to a
flood over position-welded faces from a seed region that a barrier region stops. Each part
becomes its own node with identity rotation and scale and its origin on its pivot. The model
is then turned to face glTF +Z, centered on its reference axis (mast, yaw axis, bounds) and
grounded at y = 0. Native units are kept: the game scales.

Output contracts (top-level nodes unless noted; spin axes are local):
  carrier             Hull; WheelL0.. (+X side) and WheelR0.. (-X side) front to back, axle X;
                      TrackLinkL, TrackLinkR: one link from each lower run, origin at its centre on the
                      belt's mid-line, travel -Z (the belts themselves are cut out of Hull and dropped);
                      --track-out writes each belt's loop, x, link count and the sprocket's and idler's roll
                      radius (the belt's wrap radius about their hubs) to carrier_track.rs
  civil/military heli Body; Rotor (on the mast at hub height, spins Y); TailRotor (hub, axle X)
  gatling/naval       Base; Head (yaw axis at the joint, yaws Y) with children
                      Barrels (barrel axis, spins Z, points +Z) and the empty Muzzle (tip centre)

Rebuild. The full-quality (medium) models keep every triangle of the welded scan and its
native textures, except the carrier, which slim_dense_model.py slims (see its sequence below):
no simplify setting tried (down to --error 0.0001) was indistinguishable up
close, since the scans carry their fine detail (scratches, panel lines) in geometry. Only the
far turret LODs (props/low, beyond ~150 m) are reduced: simplified as a whole, then split, so
a few cut-line triangles may bridge parts there, which cannot be seen at that range.
Tangents: Blender's export bakes MikkTSpace tangents but leaves a zero one on a few sliver
triangles with degenerate UVs (glTF validation rejects it); the tool then writes a unit
tangent perpendicular to the vertex normal there, in place (`repair_tangents`).

  G="npx @gltf-transform/cli@4"
  W=~/Projects/agentic/RoadSide-Picnic/models
  T=<scratchpad>/fleet
  B="blender -b --factory-startup --python-exit-code 1 --python tools/split_moving_parts.py --"

  $G weld $W/vehicles/uhd/institute-carrier.glb $T/carrier_weld.glb
  $B $T/carrier_weld.glb $T/carrier_split.glb --model carrier --track-out src/player/vehicle/carrier_track.rs
  python3 tools/slim_dense_model.py $T/carrier_split.glb assets/models/vehicles/medium/institute_carrier.glb
  $G weld $W/vehicles/uhd/institute-helicopter.glb $T/heli_weld.glb
  $B $T/heli_weld.glb assets/models/vehicles/medium/institute_helicopter.glb --model civil_heli
  $B $W/vehicles/uhd/military-helicopter.glb assets/models/vehicles/medium/military_helicopter.glb \
      --model military_heli

  for m in gatling naval; do
      $G weld $W/props/uhd/$m-turret.glb $T/${m}_weld.glb
      $B $T/${m}_weld.glb assets/models/props/medium/${m}_turret.glb --model $m
      $G simplify $T/${m}_weld.glb $T/${m}_simp.glb --ratio 0.013 --error 0.002
      $G resize $T/${m}_simp.glb $T/${m}_small.glb --width 1024 --height 1024
      $B $T/${m}_small.glb assets/models/props/low/${m}_turret.glb --model $m
  done

Check a split: `--pose-test 35` turns every part 35 degrees about its spin axis; render that
copy (never ship it) to see each pivot sit on its axis and nothing of the body come along
(--focus x,y,z --zoom 6 renders a close-up):
  $B $T/<model>_weld.glb $T/<model>_pose.glb --model <model> --pose-test 35
  blender -b --factory-startup --python-exit-code 1 --python \
      ~/Projects/agentic/agentic-tooling/media/glb-preview/glb_preview.py -- \
      $T/<model>_pose.glb $T/<model>_pose --views front,right,top

Finish each GLB last (Blender cannot read the KTX2 output), then validate:
  python3 tools/compress_glb_textures.py <glb>
  npx @gltf-transform/cli validate <glb>

Measured in the source frame (glTF axes, before turning and centering):
  carrier  road wheels r 0.049, axle y 0.063, z -0.311..+0.251, tyre |x| 0.170..0.228;
           return rollers r 0.019 at y 0.14; sprocket (front) r ~0.042 at (z 0.350, y 0.120),
           cut at 0.040 because the track meshes with its teeth; idler r 0.046 at (z -0.417,
           y 0.122); the track's lower run tops out at y 0.0165, so the wheels stop above it
  belts    |x| 0.1435..0.2398, 0.016 thick (lower run y 0..0.016), link pitch 0.0245 (an end
           connector, then a pair of guide horns at |x| 0.179..0.189 and 0.198..0.208 standing to
           y 0.028); the upper run (y 0.162..0.1775) is open inside, fused to the hull wall at
           |x| 0.175 and the plate under the fender; the road wheels and return rollers keep the
           horn faces their regions held. Loop ~1.89 long: 77.3 pitches, so 77 links a side; its
           mid-line runs y 0.008..0.011 along the lower run, dipping under each road wheel and
           bowing up most between the last two (the scan's own shape, not smoothed away)
  civil    nose -X; mast (x -0.0105, z 0); hub y 0.291; blades droop to y ~0.24;
           tail rotor shaft along Z at (x 0.4385, y 0.157), hub and blades at z 0.050..0.059
  military nose -X; mast (x -0.075, z 0); hub y 0.300; fenestron (x 0.4271, y 0.1762) in a
           duct of r 0.0375 through a fin of |z| < 0.025
  gatling  yaw axis z -0.188 (pedestal); head above y 0.36 plus side arms down to y 0.31;
           barrels level, axis (x 0, y 0.494), r ~0.07, z 0.015..0.490
  naval    yaw axis z -0.105 (base ring); head above y 0.078; barrels elevated 8.6 degrees,
           axis y = 0.1512 z + 0.3503 from the mantlet at z 0.2; re-posed level here

Blender coords: Z up. glTF (x, y, z) maps to Blender (x, -z, y).
"""

import argparse
import json
import math
import struct
import sys
from collections import defaultdict
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
import track_loop  # noqa: E402

INF = float("inf")
X, Y, Z = (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)


# ---- regions: each returns a function of Faces giving a boolean mask over its faces ----


def cylinder(point, axis, radius, span=(-INF, INF)):
    """Faces whose centroid lies within `radius` of the line point + t * axis, t in span."""
    p, a = np.array(point), np.array(axis) / np.linalg.norm(axis)

    def mask(faces):
        d = faces.centers - p
        t = d @ a
        radial = np.linalg.norm(d - np.outer(t, a), axis=1)
        return (radial < radius) & (t > span[0]) & (t < span[1])

    return mask


def box(lo, hi):
    def mask(faces):
        return np.all((faces.centers > np.array(lo)) & (faces.centers < np.array(hi)), axis=1)

    return mask


def any_of(*regions):
    return lambda faces: np.logical_or.reduce([region(faces) for region in regions])


def all_of(*regions):
    return lambda faces: np.logical_and.reduce([region(faces) for region in regions])


def flood(seed, barrier):
    """Faces connected (through shared vertex positions) to the seed without crossing the barrier."""

    def mask(faces):
        blocked = barrier(faces)
        selected = seed(faces) & ~blocked
        frontier = list(np.flatnonzero(selected))
        while frontier:
            for neighbor in faces.neighbors(frontier.pop()):
                if not selected[neighbor] and not blocked[neighbor]:
                    selected[neighbor] = True
                    frontier.append(neighbor)
        return selected

    return mask


def rotation(axis, degrees):
    """A rigid re-pose: turn the part by `degrees` about `axis` through its pivot."""
    return {"axis": axis, "degrees": degrees}


# ---- per-model configs (source glTF frame) ----


def track_wheels(side, sprocket, idler, road, rollers):
    """Wheel specs for one flank (side +1 is +X, the left), named front (+Z) to back."""
    letter = "L" if side > 0 else "R"
    wheels = [(sprocket[0], sprocket[1], 0.040, (0.145, 0.235), -INF), (idler[0], idler[1], 0.047, (0.145, 0.235), -INF)]
    wheels += [(z, y, 0.0505, (0.170, 0.229), 0.0175) for z, y in road]
    wheels += [(z, y, r, (0.172, 0.222), -INF) for z, y, r in rollers]
    wheels.sort(key=lambda wheel: -wheel[0])
    parts = []
    for index, (z, y, radius, band, floor) in enumerate(wheels):
        lo, hi = (band[0], -INF, -INF), (band[1], INF, INF)
        if side < 0:
            lo, hi = (-band[1], -INF, -INF), (-band[0], INF, INF)
        region = all_of(cylinder((0, y, z), X, radius), box(lo, hi), box((-INF, floor, -INF), (INF, INF, INF)))
        parts.append({"name": f"Wheel{letter}{index}", "select": region, "pivot": (None, y, z), "spin": X})
    return parts


ROAD_L = [(-0.3113, 0.0633), (-0.1994, 0.0630), (-0.0864, 0.0634), (0.0254, 0.0627), (0.1367, 0.0628), (0.2504, 0.0620)]
ROAD_R = [(-0.3113, 0.0626), (-0.1995, 0.0624), (-0.0864, 0.0628), (0.0253, 0.0625), (0.1366, 0.0627), (0.2508, 0.0622)]
ROLLERS_L = [(-0.3366, 0.1376, 0.0205), (-0.2543, 0.1403, 0.0205), (-0.1767, 0.1462, 0.016), (-0.1314, 0.1407, 0.0205),
             (-0.0266, 0.1418, 0.0205), (0.0722, 0.1408, 0.0205), (0.1334, 0.1405, 0.0205), (0.2351, 0.1386, 0.0205)]
ROLLERS_R = [(-0.3365, 0.1401, 0.0205), (-0.2541, 0.1399, 0.0205), (-0.1771, 0.1464, 0.016), (-0.1311, 0.1411, 0.0205),
             (-0.0269, 0.1410, 0.0205), (0.0719, 0.1424, 0.0205), (0.1337, 0.1390, 0.0205), (0.2348, 0.1374, 0.0205)]

# The belts, measured in the source frame (see the docstring): |x| 0.1435..0.2398, 0.016 thick, guide horns
# (two prongs, |x| 0.179..0.189 and 0.198..0.208) standing 0.012 in from the inner face, links every 0.0245.
TRACK_WIDTH = (0.141, 0.243)  # |x| the belt lies in, short of the hull's side wall and fender plates
TRACK_COARSE = (-0.011, 0.0245)  # its faces' offset from the hull round the wheels: horns to pads, short of the fender
TRACK_OUTER_COLUMN = 0.213  # |x| beyond the horns and the hull wall the upper run joins: the mid-line is fit here
TRACK_HORNS = (0.177, 0.2095)  # |x| of the guide horns
TRACK_BAND = (-0.0095, 0.012)  # the belt's offset from its mid-line: inner face to the pads on top of the upper run
TRACK_HORN_REACH = -0.0215  # and inward within the horn columns, to the horns' tips


class Track:
    """One flank's belt (side +1 is +X, the left), cut out of what the wheels leave round them.

    Measured from the faces on first use (the wheel parts come first, so `faces.free` lacks them):
      * the coarse belt: faces near the hull round the wheel circles, offset within TRACK_COARSE;
      * the link pitch, from the outermost (ground) faces of the lower run;
      * the loop: the belt's mid-line, fit (weighted by area) to its outer column, where every link has the
        same cross-section: the horns, and the horns the road wheels already took, would make it wander;
      * the belt: free faces within TRACK_BAND of the loop, or TRACK_HORN_REACH in the horn columns;
      * the link: the lower run's belt faces that reach within half a pitch of the link centre nearest
        `link_at`, trimmed to that slab after the split (so its ends are straight and copies tile). Centres
        lie half a pitch from the end connectors' (the ground faces' phase): the cuts fall through the
        connectors, where the pins are. The horns would place them less well: the wheels hold some.
    """

    def __init__(self, side, sprocket, idler, road, rollers, link_at):
        self.side, self.link_at = side, link_at
        self.sprocket, self.idler = sprocket, idler  # hubs (z, y) in the source frame
        self.road_span = (min(z for z, _ in road), max(z for z, _ in road))
        circles = [(*sprocket, 0.042), (*idler, 0.046)] + [(z, y, 0.049) for z, y in road] + list(rollers)
        angles = np.linspace(0, 2 * np.pi, 48, endpoint=False)
        ring = np.concatenate([np.stack([z + r * np.cos(angles), y + r * np.sin(angles)], axis=1) for z, y, r in circles])
        self.wheel_hull = track_loop.start_and_direction(track_loop.resample(track_loop.convex_hull(ring), 0.002))
        self.measured = None

    def measure(self, faces):
        if self.measured is not None:
            return self.measured
        x = faces.centers[:, 0] * self.side
        near = np.flatnonzero(faces.free & (x > TRACK_WIDTH[0]) & (x < TRACK_WIDTH[1]))
        zy, x, areas = faces.centers[near][:, [2, 1]], x[near], faces.areas[near]
        _, offset = track_loop.place_on(zy, self.wheel_hull)
        coarse = (offset > TRACK_COARSE[0]) & (offset < TRACK_COARSE[1])

        on_run = (zy[:, 0] > self.road_span[0]) & (zy[:, 0] < self.road_span[1])
        ground = coarse & on_run & (zy[:, 1] < zy[coarse, 1].min() + 0.002)
        pitch = track_loop.link_pitch(zy[ground, 0])
        column = coarse & (x > TRACK_OUTER_COLUMN)
        loop = track_loop.fit_loop(zy[column], pitch / 2, span=2 * pitch, weights=areas[column])

        _, offset = track_loop.place_on(zy, loop)
        horns = (x > TRACK_HORNS[0]) & (x < TRACK_HORNS[1])
        inner = np.where(horns, TRACK_HORN_REACH, TRACK_BAND[0])
        belt = (offset > inner) & (offset < TRACK_BAND[1])
        lower = belt & on_run & (zy[:, 1] < loop[:, 1].mean())
        turns = (areas[ground] * np.exp(2j * np.pi * zy[ground, 0] / pitch)).sum()
        phase = np.angle(turns) / (2 * np.pi) * pitch + pitch / 2  # connectors to link centres
        centre = phase + round((self.link_at - phase) / pitch) * pitch
        slab = (centre - pitch / 2, centre + pitch / 2)
        z_min, z_max = faces.z_ranges()
        link = lower & (z_max[near] > slab[0]) & (z_min[near] < slab[1])

        run = loop[loop[:, 1] < loop[:, 1].mean()]
        order = np.argsort(run[:, 0])
        mid_x = self.side * (x[lower].min() + x[lower].max()) / 2
        self.measured = {
            "pitch": pitch, "loop": loop, "x": mid_x, "slab": slab,
            "belt": near[belt], "link": near[link],
            "link_pivot": (mid_x, float(np.interp(centre, run[order, 0], run[order, 1])), centre),
        }
        return self.measured

    def link(self, faces):
        return self.mask(faces, "link")

    def belt(self, faces):
        return self.mask(faces, "belt")

    def mask(self, faces, key):
        chosen = np.zeros(len(faces.centers), dtype=bool)
        chosen[self.measure(faces)[key]] = True
        return chosen

    def letter(self):
        return "L" if self.side > 0 else "R"

    def link_part(self):
        return {"name": f"TrackLink{self.letter()}", "select": self.link, "pivot": lambda: self.measured["link_pivot"],
                "trim_z": lambda: self.measured["slab"]}

    def belt_part(self):
        return {"name": f"Belt{self.letter()}", "select": self.belt, "pivot": (0.0, 0.0, 0.0), "drop": True}


CIVIL_MAST = (-0.0105, 0.0, 0.0003)
MILITARY_MAST = (-0.075, 0.0, -0.0007)
NAVAL_ELEVATION = math.degrees(math.atan(0.1512))
NAVAL_AXIS = (0.0, math.sin(math.radians(NAVAL_ELEVATION)), math.cos(math.radians(NAVAL_ELEVATION)))
NAVAL_MANTLET = (0.0, 0.3503 + 0.1512 * 0.2, 0.2)

SPROCKET_L, IDLER_L, SPROCKET_R, IDLER_R = (0.3497, 0.1199), (-0.4173, 0.1217), (0.3524, 0.1206), (-0.4177, 0.1229)
LINK_AT = -0.04  # the link to export: on the lower run, between the 3rd and 4th road wheels from the front
TRACKS = [Track(1, SPROCKET_L, IDLER_L, ROAD_L, ROLLERS_L, LINK_AT), Track(-1, SPROCKET_R, IDLER_R, ROAD_R, ROLLERS_R, LINK_AT)]

MODELS = {
    "carrier": {
        "root": "Hull",
        "turn_degrees": 0.0,
        "center": "bounds",
        # Wheels first (first region wins), then each side's link, then its belt: kept through normalize so
        # the bounds, centring and grounding stay the scan's, and dropped before export.
        "parts": track_wheels(1, SPROCKET_L, IDLER_L, ROAD_L, ROLLERS_L)
        + track_wheels(-1, SPROCKET_R, IDLER_R, ROAD_R, ROLLERS_R)
        + [track.link_part() for track in TRACKS]
        + [track.belt_part() for track in TRACKS],
        "tracks": TRACKS,
    },
    "civil_heli": {
        "root": "Body",
        "turn_degrees": 90.0,  # nose -X to +Z
        "center": (CIVIL_MAST[0], CIVIL_MAST[2]),
        "parts": [
            {
                "name": "Rotor",
                # Hub and blades; the drooping blades pass below the hub, so flood from the hub
                # down to a cut through the mast.
                "select": flood(cylinder(CIVIL_MAST, Y, 0.03, (0.275, 0.33)), cylinder(CIVIL_MAST, Y, 0.03, (-INF, 0.262))),
                "pivot": (CIVIL_MAST[0], 0.291, CIVIL_MAST[2]),
                "spin": Y,
            },
            {
                "name": "TailRotor",
                "select": box((0.384, 0.145, 0.0485), (0.494, 0.18, 0.064)),
                "pivot": (0.4385, 0.157, 0.0545),
                "spin": Z,  # the shaft runs along source Z, which the turn makes X
            },
        ],
    },
    "military_heli": {
        "root": "Body",
        "turn_degrees": 90.0,
        "center": (MILITARY_MAST[0], MILITARY_MAST[2]),
        "parts": [
            {
                "name": "Rotor",
                "select": cylinder(MILITARY_MAST, Y, 0.45, (0.274, INF)),  # the fin top lies beyond 0.49
                "pivot": (MILITARY_MAST[0], 0.300, MILITARY_MAST[2]),
                "spin": Y,
            },
            {
                "name": "TailRotor",
                "select": cylinder((0.4271, 0.1762, 0.0), Z, 0.037, (-0.0205, 0.0205)),  # inside the duct, off the fin skin
                "pivot": (0.4271, 0.1762, 0.0),
                "spin": Z,
            },
        ],
    },
    "gatling": {
        "root": "Base",
        "turn_degrees": 0.0,
        "center": (0.0, -0.188),
        "parts": [
            {
                "name": "Barrels",
                "parent": "Head",
                "select": all_of(cylinder((0, 0.494, 0), Z, 0.08, (0.015, INF)), box((-INF, -INF, -INF), (INF, 0.572, INF))),
                "pivot": (0.0, 0.494, 0.015),
                "spin": Z,
            },
            {
                "name": "Head",
                # the box above the neck, and the side arms that hang down past it
                "select": any_of(box((-INF, 0.36, -INF), (INF, INF, INF)), box((0.12, 0.28, -INF), (INF, INF, INF)),
                                 box((-INF, 0.28, -INF), (-0.12, INF, INF))),
                "pivot": (0.0, 0.36, -0.188),
                "spin": Y,
            },
        ],
        "empties": [{"name": "Muzzle", "parent": "Head", "at": (0.0, 0.494, 0.4903)}],
    },
    "naval": {
        "root": "Base",
        "turn_degrees": 0.0,
        "center": (0.0, -0.105),
        "parts": [
            {
                "name": "Barrels",
                "parent": "Head",
                # the mantlet and barrel cluster, turned level about the mantlet
                "select": cylinder(NAVAL_MANTLET, NAVAL_AXIS, 0.043, (0.002, INF)),
                "pivot": NAVAL_MANTLET,
                "repose": rotation(X, NAVAL_ELEVATION),
                "spin": Z,
            },
            {
                "name": "Head",
                "select": box((-INF, 0.078, -INF), (INF, INF, INF)),
                "pivot": (0.0, 0.078, -0.105),
                "spin": Y,
            },
        ],
        "empties": [
            {"name": "Muzzle", "parent": "Head",
             "at": (0.0, NAVAL_MANTLET[1], NAVAL_MANTLET[2] + (0.4899 - 0.2) / math.cos(math.radians(NAVAL_ELEVATION)))},
        ],
    },
}


# ---- mesh plumbing ----


def to_gltf(v):
    return (v[0], v[2], -v[1])


def to_blender(x, y, z):
    return Vector((x, -z, y))


def turn_matrix(axis, degrees):
    """A rotation about a glTF-frame axis, expressed in Blender coordinates."""
    return Matrix.Rotation(math.radians(degrees), 4, to_blender(*axis))


class Faces:
    """Face centroids in the glTF frame, with lazily built position-welded adjacency."""

    def __init__(self, mesh):
        count = len(mesh.polygons)
        centers = np.zeros(count * 3)
        mesh.polygons.foreach_get("center", centers)
        centers = centers.reshape(-1, 3)
        self.centers = np.stack([centers[:, 0], centers[:, 2], -centers[:, 1]], axis=1)
        self.areas = np.zeros(count)
        mesh.polygons.foreach_get("area", self.areas)
        self.free = np.ones(count, dtype=bool)  # faces no earlier part took (split keeps it current)
        self.mesh = mesh
        self._by_position = None

    def z_ranges(self):
        """Each face's lowest and highest vertex z (glTF frame)."""
        mesh = self.mesh
        corners = np.zeros(len(mesh.loops), dtype=int)
        mesh.loops.foreach_get("vertex_index", corners)
        starts = np.zeros(len(mesh.polygons), dtype=int)
        mesh.polygons.foreach_get("loop_start", starts)
        positions = np.zeros(len(mesh.vertices) * 3)
        mesh.vertices.foreach_get("co", positions)
        z = -positions.reshape(-1, 3)[corners, 1]
        return np.minimum.reduceat(z, starts), np.maximum.reduceat(z, starts)

    def neighbors(self, face):
        if self._by_position is None:
            positions = np.zeros(len(self.mesh.vertices) * 3)
            self.mesh.vertices.foreach_get("co", positions)
            keys = [tuple(key) for key in np.round(positions.reshape(-1, 3), 6)]
            self._keys = keys
            self._by_position = defaultdict(list)
            for polygon in self.mesh.polygons:
                for vertex in polygon.vertices:
                    self._by_position[keys[vertex]].append(polygon.index)
        return {other for vertex in self.mesh.polygons[face].vertices for other in self._by_position[self._keys[vertex]]}


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("src")
    parser.add_argument("dst")
    parser.add_argument("--model", choices=sorted(MODELS), required=True)
    parser.add_argument("--pose-test", type=float, default=0.0, metavar="DEG",
                        help="turn every part DEG about its spin axis (a check render, never a game asset)")
    parser.add_argument("--track-out", type=Path, metavar="RS",
                        help="write the tracks' loops, x, link counts and end-wheel roll radii as Rust (carrier: src/player/vehicle/carrier_track.rs)")
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


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
    bpy.ops.object.parent_clear(type="CLEAR_KEEP_TRANSFORM")
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return body


def separate(body, labels, label):
    """Split the faces labelled `label` off `body` into a new object and return it."""
    before = set(bpy.context.scene.objects)
    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_mode(type="FACE")
    bpy.ops.mesh.select_all(action="DESELECT")
    bm = bmesh.from_edit_mesh(body.data)
    bm.faces.ensure_lookup_table()
    for index in np.flatnonzero(labels == label):
        bm.faces[index].select_set(True)
    bmesh.update_edit_mesh(body.data)
    bpy.ops.mesh.separate(type="SELECTED")
    bpy.ops.object.mode_set(mode="OBJECT")
    (part,) = set(bpy.context.scene.objects) - before
    return part


def triangles(obj):
    return sum(len(polygon.vertices) - 2 for polygon in obj.data.polygons)


def split(body, config):
    """Label every face with the first part whose region holds it, then separate the parts."""
    faces = Faces(body.data)
    labels = np.zeros(len(body.data.polygons), dtype=int)
    for label, part in enumerate(config["parts"], start=1):
        faces.free = labels == 0
        chosen = part["select"](faces) & faces.free
        if not chosen.any():
            raise ValueError(f"{part['name']}: its region holds no faces")
        labels[chosen] = label

    objects = {}
    for label, part in enumerate(config["parts"], start=1):
        obj = separate(body, labels, label)
        if len(obj.data.polygons) != (labels == label).sum():
            raise RuntimeError(f"{part['name']}: separated {len(obj.data.polygons)} faces, expected {(labels == label).sum()}")
        labels = labels[labels != label]  # body keeps its remaining faces in order
        obj.name = obj.data.name = part["name"]
        if "trim_z" in part:
            trim_z(obj, *part["trim_z"]())
        set_pivot(obj, part)
        objects[part["name"]] = obj
    body.name = body.data.name = config["root"]
    objects[config["root"]] = body
    return objects


def trim_z(obj, low, high):
    """Bisect the part at the glTF planes z = low and z = high and keep what lies between: straight ends
    cut through the scan's own surface."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    for z, outward in ((low, -1.0), (high, 1.0)):
        bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=to_blender(0.0, 0.0, z),
                               plane_no=to_blender(0.0, 0.0, outward), clear_outer=True)
    bm.to_mesh(obj.data)
    bm.free()


def set_pivot(obj, part):
    """Re-pose the part rigidly if asked, then move its origin to the pivot (None: bounds centre; a function:
    measured while splitting)."""
    pivot = part["pivot"]() if callable(part["pivot"]) else part["pivot"]
    if pivot[0] is None:
        xs = [to_gltf(v.co)[0] for v in obj.data.vertices]
        pivot = ((min(xs) + max(xs)) / 2, pivot[1], pivot[2])
    origin = to_blender(*pivot)
    if "repose" in part:
        turn = turn_matrix(part["repose"]["axis"], part["repose"]["degrees"])
        obj.data.transform(Matrix.Translation(origin) @ turn @ Matrix.Translation(-origin))
    obj.data.transform(Matrix.Translation(-origin))
    obj.location = origin


def normalize(objects, config):
    """Turn the model to face +Z, center its reference axis on x = z = 0 and ground it.

    Returns the shift it applied after the turn, in glTF axes."""
    turn = turn_matrix(Y, config["turn_degrees"])
    for obj in objects.values():
        if obj.type == "MESH":
            obj.data.transform(turn)
        obj.location = turn @ obj.location

    meshes = [obj for obj in objects.values() if obj.type == "MESH"]
    points = [to_gltf(obj.location + v.co) for obj in meshes for v in obj.data.vertices]
    if config["center"] == "bounds":
        center_x = (min(p[0] for p in points) + max(p[0] for p in points)) / 2
        center_z = (min(p[2] for p in points) + max(p[2] for p in points)) / 2
    else:
        source_x, source_z = config["center"]
        center_x, _, center_z = to_gltf(turn @ to_blender(source_x, 0.0, source_z))
    shift = to_blender(-center_x, -min(p[1] for p in points), -center_z)
    for obj in objects.values():
        obj.location = obj.location + shift
    root = objects[config["root"]]  # the static root sits at the model origin
    root.data.transform(Matrix.Translation(root.location))
    root.location = Vector()
    return to_gltf(shift)


def drop_parts(objects, config):
    """Delete the parts that only stood in for the bounds (the belts)."""
    for part in config["parts"]:
        if part.get("drop"):
            bpy.data.objects.remove(objects.pop(part["name"]))


def write_tracks(path, config, shift):
    """The tracks' loops, x, link counts and end-wheel roll radii, moved into the runtime frame by normalize's
    shift (the same one that moves the Wheel nodes, so the hubs measured from are the nodes' own)."""
    if not config.get("tracks") or config["turn_degrees"]:
        raise ValueError("--track-out needs a model with tracks and no turn")
    loops, xs, links, wraps = [], [], [], []
    for track in config["tracks"]:
        measured = track.measured
        move = np.array([shift[2], shift[1]])
        loop = measured["loop"] + move
        length = track_loop.loop_length(loop)
        loops.append(loop)
        xs.append(measured["x"] + shift[0])
        links.append(int(round(length / measured["pitch"])))
        wraps.append((track_loop.wrap_radius(loop, np.array(track.sprocket) + move, +1),
                      track_loop.wrap_radius(loop, np.array(track.idler) + move, -1)))
        print(f"TRACK side {track.side:+d} x {xs[-1]:.4f} loop {length:.4f} points {len(loop)} pitch "
              f"{measured['pitch']:.5f} length/pitch {length / measured['pitch']:.3f} links {links[-1]} "
              f"wrap sprocket {wraps[-1][0]:.5f} idler {wraps[-1][1]:.5f}")
    path.write_text(track_loop.rust_track_module(loops, xs, links, wraps))
    print(f"TRACK wrote {path}")


def add_empties(objects, config):
    for spec in config.get("empties", []):
        empty = bpy.data.objects.new(spec["name"], None)
        bpy.context.collection.objects.link(empty)
        empty.location = to_blender(*spec["at"])
        objects[spec["name"]] = empty


def parent_nodes(objects, config):
    """Parent children with identity rotation: the local offset is the world difference."""
    specs = config["parts"] + config.get("empties", [])
    for spec in specs:
        if spec.get("parent"):
            child, parent = objects[spec["name"]], objects[spec["parent"]]
            world = child.location.copy()
            child.parent = parent
            child.matrix_parent_inverse = Matrix.Identity(4)
            child.location = world - parent.location


def pose_test(objects, config, degrees):
    turn = turn_matrix(Y, config["turn_degrees"])
    for part in (part for part in config["parts"] if "spin" in part):
        axis = turn @ to_blender(*part["spin"])
        objects[part["name"]].rotation_mode = "QUATERNION"
        objects[part["name"]].rotation_quaternion = Matrix.Rotation(math.radians(degrees), 4, axis).to_quaternion()


def report(objects):
    for name, obj in sorted(objects.items()):
        local = tuple(round(c, 4) for c in to_gltf(obj.location))
        world = tuple(round(c, 4) for c in to_gltf(obj.matrix_world.translation))
        parent = obj.parent.name if obj.parent else "-"
        if obj.type != "MESH":
            print(f"SPLIT {name} empty parent {parent} local {local} world {world}")
            continue
        points = [to_gltf(v.co) for v in obj.data.vertices]
        lo = tuple(round(min(p[i] for p in points), 4) for i in range(3))
        hi = tuple(round(max(p[i] for p in points), 4) for i in range(3))
        print(f"SPLIT {name} tris {triangles(obj)} parent {parent} local {local} world {world} mesh {lo}..{hi}")
    meshes = [obj for obj in objects.values() if obj.type == "MESH"]
    points = [to_gltf(obj.matrix_world @ v.co) for obj in meshes for v in obj.data.vertices]
    lo = tuple(round(min(p[i] for p in points), 4) for i in range(3))
    hi = tuple(round(max(p[i] for p in points), 4) for i in range(3))
    print(f"SPLIT total tris {sum(triangles(obj) for obj in meshes)} bounds {lo}..{hi}")


def repair_tangents(path):
    """Give each zero tangent a unit one perpendicular to its normal, in place in the GLB."""
    with open(path, "r+b") as glb:
        data = bytearray(glb.read())
        json_length = struct.unpack_from("<I", data, 12)[0]
        gltf = json.loads(data[20 : 20 + json_length])
        bin_start = 20 + json_length + 8

        def view(index, width):
            accessor = gltf["accessors"][index]
            buffer_view = gltf["bufferViews"][accessor["bufferView"]]
            stride = buffer_view.get("byteStride", width * 4)
            start = bin_start + buffer_view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
            raw = np.frombuffer(data, np.uint8, (accessor["count"] - 1) * stride + width * 4, start)
            return np.lib.stride_tricks.as_strided(raw[: raw.size // 4 * 4].view(np.float32),
                                                   (accessor["count"], width), (stride, 4))

        repaired = 0
        for mesh in gltf["meshes"]:
            for primitive in mesh["primitives"]:
                attributes = primitive["attributes"]
                if "TANGENT" not in attributes:
                    continue
                tangents = view(attributes["TANGENT"], 4).copy()
                zero = np.linalg.norm(tangents[:, :3], axis=1) < 1e-6
                if not zero.any():
                    continue
                normals = view(attributes["NORMAL"], 3)[zero]
                helper = np.where(np.abs(normals[:, :1]) < 0.9, [[1.0, 0.0, 0.0]], [[0.0, 1.0, 0.0]])
                fixed = np.cross(normals, helper)
                tangents[zero, :3] = fixed / np.linalg.norm(fixed, axis=1, keepdims=True)
                tangents[zero, 3] = 1.0
                accessor = gltf["accessors"][attributes["TANGENT"]]
                buffer_view = gltf["bufferViews"][accessor["bufferView"]]
                stride = buffer_view.get("byteStride", 16)
                start = bin_start + buffer_view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
                for row in np.flatnonzero(zero):
                    struct.pack_into("<4f", data, start + row * stride, *tangents[row])
                repaired += int(zero.sum())
        glb.seek(0)
        glb.write(data)
    print(f"SPLIT repaired {repaired} zero tangents")


def main():
    args = parse_args()
    config = MODELS[args.model]
    body = import_body(args.src)
    objects = split(body, config)
    add_empties(objects, config)
    shift = normalize(objects, config)
    drop_parts(objects, config)
    if args.track_out:
        write_tracks(args.track_out, config, shift)
    parent_nodes(objects, config)
    bpy.context.view_layer.update()
    if args.pose_test:
        pose_test(objects, config, args.pose_test)
        bpy.context.view_layer.update()
    report(objects)

    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects.values():
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[config["root"]]
    bpy.ops.export_scene.gltf(
        filepath=args.dst,
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        export_tangents=True,
        export_image_format="AUTO",
    )
    repair_tangents(args.dst)


main()
