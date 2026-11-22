"""Bake detailed vehicle PBR maps from the reference boards and split the wheel pivots.

blender -b --python tools/prepare_vehicle_asset.py -- source.glb out.glb \
  textture_vehicle.png 'image_3 (1).png' 'image_2 (1).png' 'image_1 (1).png'
"""

import math
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

import bpy
import numpy as np

SOURCE, OUTPUT, *BOARDS = sys.argv[sys.argv.index("--") + 1 :]
if len(BOARDS) != 4:
    raise ValueError("Expected the primary texture sheet, then image_3, image_2, and image_1")

SIZE, PAD = 1024, 16
PLAIN, SIDE, FRONT, REAR, WHEEL, TREAD = range(6)
VIEWS = {SIDE: (-150, 30, 850, 540), FRONT: (60, 30, 640, 540), REAR: (60, 30, 640, 540)}

# Crops are (sheet, left, top, right, bottom) at the reference boards' 1024 px size.
# A swatch has base color, normal, roughness, metallic and occlusion, in that order.
ARMOR = ((0, 123, 48, 147, 68), (0, 379, 48, 403, 68), (0, 635, 48, 659, 68),
         (0, 891, 48, 915, 68), (0, 123, 304, 147, 324))
GLASS = ((0, 16, 761, 104, 794), (0, 272, 761, 360, 794), (0, 528, 761, 616, 794),
         0.05, 0.95)
GRILLE = ((2, 102, 351, 191, 379), (2, 443, 351, 532, 379), (2, 785, 351, 874, 379),
          0.55, 0.75)
LOUVERS = ((1, 181, 227, 227, 283), (1, 693, 227, 739, 283), (1, 181, 566, 227, 622),
           0.10, 0.86)
HEADLIGHT = ((1, 316, 284, 338, 302), (128, 128, 255), 0.3, 0.05, 1.0)
TAIL_LIGHT = ((1, 520, 853, 540, 881), (128, 128, 255), 0.31, 0.04, 1.0)
WHEEL_FACE = ((3, 10, 597, 181, 769), (3, 266, 597, 437, 769), (3, 522, 597, 693, 769),
              0.16, 0.92)
RUBBER = ((3, 181, 604, 246, 812), (3, 437, 604, 502, 812), (3, 693, 604, 758, 812),
          0.0, 1.0)


def solid(color, roughness):
    return color, (128, 128, 255), roughness, 0.0, 1.0


TRIM = solid((29, 31, 32), 0.68)
BUMPER = solid((61, 64, 67), 0.76)
HATCH = solid((76, 80, 84), 0.61)
AMBER = solid((194, 109, 39), 0.32)


def load_board(path):
    image = bpy.data.images.load(path)
    if tuple(image.size) != (1024, 1024):
        raise ValueError(f"{path} must be a 1024 x 1024 reference sheet")
    pixels = np.empty(1024 * 1024 * 4, dtype=np.float32)
    image.pixels.foreach_get(pixels)
    bpy.data.images.remove(image)
    return pixels.reshape(1024, 1024, 4)[::-1, :, :3]


def paint(tile, swatch, boards, points=None):
    if points is None:
        rows, cols = slice(None), slice(None)
        height = width = SIZE - 2 * PAD
        mask = np.ones((height, width), dtype=bool)
    else:
        points = np.asarray(points, dtype=float)
        lo, hi = np.floor(points.min(axis=0)).astype(int), np.ceil(points.max(axis=0)).astype(int)
        width, height = hi - lo
        cols, rows = slice(lo[0], hi[0]), slice(lo[1], hi[1])
        y, x = np.mgrid[rows, cols] + 0.5
        mask = np.zeros((height, width), dtype=bool)
        for (x1, y1), (x2, y2) in zip(points, np.roll(points, -1, axis=0)):
            if y1 != y2:
                mask ^= ((y1 > y) != (y2 > y)) & (x < (x2 - x1) * (y - y1) / (y2 - y1) + x1)

    def sample(spec):
        if not isinstance(spec, tuple) or len(spec) == 3:
            return np.asarray(spec, dtype=np.float32) / (255 if isinstance(spec, tuple) else 1)
        board, left, top, right, bottom = spec
        x = np.linspace(left, right - 1, width)
        y = np.linspace(top, bottom - 1, height)
        # Bilinear resampling keeps enlarged thumbnail crops from pixelating.
        xi, yi = x.astype(int), y.astype(int)
        dx = (x - xi).astype(np.float32)[None, :, None]
        dy = (y - yi).astype(np.float32)[:, None, None]
        src = boards[board]
        a = src[yi[:, None], xi] * (1 - dx) + src[yi[:, None], xi + 1] * dx
        b = src[(yi + 1)[:, None], xi] * (1 - dx) + src[(yi + 1)[:, None], xi + 1] * dx
        return a * (1 - dy) + b * dy

    def channel(spec):
        pixels = sample(spec)
        return pixels.mean(axis=-1) if pixels.ndim else pixels

    albedo, normal, roughness, metallic, occlusion = swatch
    rough = channel(roughness)
    metal = channel(metallic)
    if swatch is ARMOR:
        rough = 0.45 + 0.45 * rough
        metal *= 0.22
    elif swatch is RUBBER:
        rough = 0.53 + 0.42 * rough
    data = np.stack(np.broadcast_arrays(channel(occlusion), rough, metal), axis=-1)
    for surface, values in zip(tile, (sample(albedo), sample(normal), data)):
        surface[rows, cols][mask] = np.broadcast_to(values, (height, width, 3))[mask]


def noise(rng, size, cells):
    lattice = rng.uniform(-1, 1, (cells + 1, cells + 1)).astype(np.float32)
    uv = np.linspace(0, cells, size, endpoint=False, dtype=np.float32)
    i = uv.astype(int)
    t = uv - i
    t = (t * t * (3 - 2 * t)).astype(np.float32)
    rows = lattice[i] * (1 - t[:, None]) + lattice[i + 1] * t[:, None]
    return rows[:, i] * (1 - t) + rows[:, i + 1] * t


def weather_armor(tile, view):
    albedo, normal, arm = tile
    size = albedo.shape[0]
    rng = np.random.default_rng(42 + view)
    mottling = noise(rng, size, 22)
    grain = noise(rng, size, 190)
    pores = noise(rng, size, 395)
    scratches = np.zeros_like(grain)
    for _ in range(80):
        x, y = rng.integers(30, size - 60, 2)
        length = rng.integers(6, 48)
        tilt = rng.uniform(-0.25, 0.32)
        xx, yy = np.arange(length)[None, :], np.arange(-16, 17)[:, None]
        width = rng.uniform(0.65, 1.8)
        stroke = np.clip(1 - np.abs(yy - tilt * xx) / width, 0, 1)
        stroke *= np.sin(np.pi * (xx + 0.5) / length) * rng.uniform(0.28, 1.0)
        area = scratches[y - 16:y + 17, x:x + length]
        np.maximum(area, stroke, out=area)

    albedo *= (1 + 0.060 * mottling + 0.035 * grain + 0.015 * pores)[:, :, None]
    albedo += 0.036 * scratches[:, :, None]
    if view != PLAIN:
        height = np.linspace(0, 1, size, dtype=np.float32)[:, None]
        dust = np.clip((height - 0.58) * 3 + 0.18 * mottling, 0, 1) * (0.10 + 0.05 * grain)
        albedo += dust[:, :, None] * (np.array((0.42, 0.35, 0.29), dtype=np.float32) - albedo)
        arm[:, :, 1] += 0.25 * dust

    arm[:, :, 1] += 0.028 * mottling + 0.020 * grain - 0.13 * scratches
    arm[:, :, 2] += 0.20 * scratches
    # Height slopes use +Y up in the normal map, opposite the image rows.
    dy, dx = np.gradient(0.09 * grain + 0.035 * pores - 0.15 * scratches)
    tangent = normal * 2 - 1
    tangent[:, :, 0] -= dx
    tangent[:, :, 1] += dy
    normal[:] = 0.5 + 0.5 * tangent / np.linalg.norm(tangent, axis=-1, keepdims=True)


def box(left, top, right, bottom):
    return ((left, top), (right, top), (right, bottom), (left, bottom))


def circle(x, y, radius):
    return [(x + radius * math.cos(angle), y + radius * math.sin(angle))
            for angle in np.linspace(0, math.tau, 24, endpoint=False)]


def trace(points, width):
    for (x1, y1), (x2, y2) in zip(points, points[1:]):
        scale = width / (2 * math.hypot(x2 - x1, y2 - y1))
        dx, dy = (y1 - y2) * scale, (x2 - x1) * scale
        yield ((x1 + dx, y1 + dy), (x2 + dx, y2 + dy), (x2 - dx, y2 - dy), (x1 - dx, y1 - dy))


def make_atlas():
    boards = [load_board(path) for path in BOARDS]
    inner = SIZE - 2 * PAD
    tiles = np.empty((6, 3, inner, inner, 3), dtype=np.float32)
    for tile in (PLAIN, SIDE, FRONT, REAR):
        paint(tiles[tile], ARMOR, boards)
        tiles[tile, 0] += (0.065, 0.071, 0.078)
    paint(tiles[WHEEL], WHEEL_FACE, boards)
    paint(tiles[TREAD], RUBBER, boards)

    def detail(view, swatch, points):
        left, top, right, bottom = VIEWS[view]
        projected = [((x - left) * inner / (right - left), (y - top) * inner / (bottom - top))
                     for x, y in points]
        paint(tiles[view], swatch, boards, projected)

    detail(SIDE, BUMPER, box(188, 351, 520, 365))
    for seam in (
        ((188, 238), (193, 288), (202, 319), (290, 319), (320, 278), (327, 233)),
        ((365, 233), (371, 297), (379, 319), (478, 319), (505, 283), (511, 233)),
    ):
        for segment in trace(seam, 1.8):
            detail(SIDE, BUMPER, segment)
    detail(FRONT, BUMPER, ((94, 313), (241, 327), (461, 327), (596, 313),
                           (473, 380), (227, 380)))
    detail(REAR, BUMPER, ((107, 346), (595, 346), (473, 391), (227, 391)))
    detail(REAR, TRIM, box(237, 241, 462, 336))
    detail(REAR, HATCH, box(240, 244, 459, 333))
    for view in (PLAIN, SIDE, FRONT, REAR):
        weather_armor(tiles[view], view)

    # Match the orthographic projections to the modeled panel recesses.
    for view, windows in (
        (SIDE, [((225, 185), (253, 155), (326, 155), (331, 220), (225, 220)),
                ((376, 150), (494, 150), (501, 220), (375, 220))]),
        (FRONT, [((202, 141), (336, 141), (338, 208), (177, 208)),
                 ((362, 141), (500, 141), (531, 208), (361, 208))]),
        (REAR, [((195, 150), (229, 148), (232, 211), (175, 211)),
                ((284, 154), (415, 154), (421, 211), (281, 211)),
                ((473, 148), (505, 150), (527, 211), (470, 211))]),
    ):
        for pane in windows:
            cx, cy = np.mean(pane, axis=0)
            detail(view, TRIM, [(cx + (x - cx) * 1.09, cy + (y - cy) * 1.09) for x, y in pane])
            detail(view, GLASS, pane)
    detail(SIDE, TRIM, ((524, 135), (593, 135), (610, 205), (524, 205)))
    detail(SIDE, LOUVERS, ((529, 140), (589, 140), (603, 200), (529, 200)))
    detail(SIDE, TRIM, box(434, 374, 492, 432))
    detail(SIDE, LOUVERS, box(438, 378, 488, 428))
    detail(SIDE, AMBER, box(355, 385, 369, 396))
    detail(SIDE, AMBER, box(-87, 259, -69, 270))
    detail(FRONT, TRIM, box(240, 255, 460, 324))
    detail(FRONT, GRILLE, box(248, 261, 452, 319))
    for x in (198, 503):
        detail(FRONT, TRIM, box(x - 25, 253, x + 25, 307))
        detail(FRONT, HEADLIGHT, circle(x, 281, 19))
        marker = x + (-34 if x < 350 else 34)
        detail(FRONT, AMBER, box(marker - 6, 266, marker + 6, 281))
    for left in (184, 485):
        detail(REAR, TRIM, box(left - 4, 268, left + 35, 319))
        detail(REAR, AMBER, box(left, 272, left + 31, 287))
        detail(REAR, TAIL_LIGHT, box(left, 290, left + 31, 314))
    detail(REAR, TRIM, box(308, 253, 392, 260))
    for left in (251, 404):
        detail(REAR, TAIL_LIGHT, box(left, 355, left + 43, 369))
    atlas = np.empty((3, 2 * SIZE, 3 * SIZE, 3), dtype=np.float32)
    for index, tile in enumerate(tiles):
        x, y = index % 3 * SIZE, index // 3 * SIZE
        atlas[:, y:y + SIZE, x:x + SIZE] = np.pad(
            tile, ((0, 0), (PAD, PAD), (PAD, PAD), (0, 0)), mode="edge"
        )
    return atlas


def vehicle_material(atlas, directory):
    material = bpy.data.materials.new("Weathered armor")
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    surface = nodes.get("Principled BSDF")
    surface.inputs["Metallic"].default_value = 1.0
    surface.inputs["Roughness"].default_value = 1.0
    color, normal, arm = (nodes.new("ShaderNodeTexImage") for _ in range(3))
    for slot, pixels, name in zip((color, normal, arm), atlas, ("albedo", "normal", "arm")):
        h, w, _ = pixels.shape
        rgba = np.ones((h, w, 4), dtype=np.float32)
        rgba[:, :, :3] = pixels[::-1]
        image = bpy.data.images.new(name, width=w, height=h, alpha=False)
        image.pixels.foreach_set(rgba.ravel())
        path = str(Path(directory) / f"armored_vehicle_{name}.png")
        image.filepath_raw = path
        image.file_format = "PNG"
        image.save()
        bpy.data.images.remove(image)
        image = bpy.data.images.load(path)
        if name != "albedo":
            image.colorspace_settings.name = "Non-Color"
        image.pack()
        slot.image = image
        slot.extension = "EXTEND"

    links.new(color.outputs["Color"], surface.inputs["Base Color"])
    normal_map = nodes.new("ShaderNodeNormalMap")
    links.new(normal.outputs["Color"], normal_map.inputs["Color"])
    links.new(normal_map.outputs["Normal"], surface.inputs["Normal"])
    channels = nodes.new("ShaderNodeSeparateColor")
    links.new(arm.outputs["Color"], channels.inputs["Color"])
    links.new(channels.outputs["Green"], surface.inputs["Roughness"])
    links.new(channels.outputs["Blue"], surface.inputs["Metallic"])
    settings = nodes.new("ShaderNodeGroup")
    settings.node_tree = bpy.data.node_groups.new("glTF Material Output", "ShaderNodeTree")
    settings.node_tree.interface.new_socket(name="Occlusion", in_out="INPUT", socket_type="NodeSocketFloat")
    links.new(channels.outputs["Red"], settings.inputs["Occlusion"])
    return material


def mesh_parts(mesh):
    adjacency = [[] for _ in mesh.vertices]
    for edge in mesh.edges:
        a, b = edge.vertices
        adjacency[a].append(b)
        adjacency[b].append(a)
    remaining = set(range(len(mesh.vertices)))
    wheels = {}
    while remaining:
        stack = [remaining.pop()]
        island = []
        while stack:
            vertex = stack.pop()
            island.append(vertex)
            for neighbor in adjacency[vertex]:
                if neighbor in remaining:
                    remaining.remove(neighbor)
                    stack.append(neighbor)
        points = np.array([mesh.vertices[i].co[:] for i in island])
        if len(island) > 500 and points[:, 2].max() < 0.21:
            center = (points.min(axis=0) + points.max(axis=0)) / 2
            name = "Wheel" + ("Front" if center[1] < 0 else "Rear") + ("Left" if center[0] > 0 else "Right")
            wheels[name] = (set(island), center)
    if len(wheels) != 4:
        raise ValueError(f"Expected four wheel islands, found {list(wheels)}")
    hull = set(range(len(mesh.vertices))).difference(*(vertices for vertices, _ in wheels.values()))
    return {"Chassis": (hull, np.zeros(3)), **wheels}


def map_face(face, points, wheel):
    nx, ny, nz = map(abs, face.normal)
    if wheel:
        if nx > 0.62:
            return WHEEL, [(0.5 + y / 0.205, 0.5 - z / 0.205) for x, y, z in points]
        angles = [(math.atan2(y, z) / math.tau) % 1.0 for _, y, z in points]
        if max(angles) - min(angles) > 0.5:
            angles = [angle if angle < 0.5 else angle - 1.0 for angle in angles]
        return TREAD, [(0.5 + p[0] / 0.09, angle) for p, angle in zip(points, angles)]

    _, _, z = face.center
    if ny > nx * 0.85 and (ny > nz or (0.32 < z < 0.415 and ny > 0.45)):
        front = face.normal.y < 0
        kind = FRONT if front else REAR
        coords = [(350 + (x if front else -x) * 1000, 540 - z * 1000) for x, _, z in points]
    elif nx >= nz:
        kind = SIDE
        coords = [(350 + y * 1000, 540 - z * 1000) for _, y, z in points]
    else:
        return PLAIN, [(0.5 + x / 0.58, 0.5 - y) for x, y, _ in points]
    left, top, right, bottom = VIEWS[kind]
    return kind, [((x - left) / (right - left), (y - top) / (bottom - top)) for x, y in coords]


def export_part(source, name, selection, center, material):
    faces = [face for face in source.polygons if face.vertices[0] in selection]
    indices = sorted({index for face in faces for index in face.vertices})
    remap = {index: i for i, index in enumerate(indices)}
    points = [np.asarray(source.vertices[i].co[:]) - center for i in indices]
    data = bpy.data.meshes.new(name)
    data.from_pydata(points, [], [tuple(remap[i] for i in f.vertices) for f in faces])
    data.update()
    data.materials.append(material)
    data.normals_split_custom_set([source.corner_normals[i].vector for f in faces for i in f.loop_indices])
    uv = data.uv_layers.new().data
    for group, face in zip(data.polygons, faces):
        group.use_smooth = True
        tile, mapped = map_face(face, [points[i] for i in group.vertices], name != "Chassis")
        for i, (u, v) in zip(group.loop_indices, np.clip(mapped, 0.0, 1.0)):
            px = (tile % 3) * SIZE + PAD + u * (SIZE - 2 * PAD)
            py = (tile // 3) * SIZE + PAD + v * (SIZE - 2 * PAD)
            uv[i].uv = (px / (3 * SIZE), 1 - py / (2 * SIZE))
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    obj.location = center
    obj.select_set(True)


bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=SOURCE)
source = next(obj for obj in bpy.context.scene.objects if obj.type == "MESH")
bpy.ops.object.select_all(action="DESELECT")
with TemporaryDirectory() as directory:
    material = vehicle_material(make_atlas(), directory)
    for name, (selection, center) in mesh_parts(source.data).items():
        export_part(source.data, name, selection, center, material)
    bpy.ops.export_scene.gltf(
        filepath=OUTPUT, export_format="GLB", use_selection=True, export_tangents=True,
    )
