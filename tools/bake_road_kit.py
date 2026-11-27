"""Bake the retired road kit into one road-aligned strip of colour, normal and height (headless Blender).

The Zone's old asphalt is painted, not modelled: the terrain shader tiles this strip along every
asphalt leg (`src/world/roads.rs::leg_frame`, `assets/shaders/terrain.wesl::old_road`). The kit is
the stretch of cracked road scanned in slabs that `old_road.rs` laid as meshes until 6fb8d3f; its
straightened pieces (one shared frame: +Z along the road, +X across it, y = 0 on the asphalt) are
in the unused-assets zip, with KTX2 textures Blender cannot read, and the workspace keeps the
originals with the same UVs and JPEG colour. Run it from the repository root (Blender 4.5, `ctt`):

    BLENDER=~/.local/blender-4.5.16-linux-x64/blender
    "$BLENDER" -b --factory-startup --python-exit-code 1 --python tools/bake_road_kit.py -- \
        ~/Projects/agentic/RoadSide-Picnic/archived/roadside-picnic-unused-assets-2026-11-22.zip \
        ~/Projects/agentic/RoadSide-Picnic/models/road/uhd assets/textures/terrain \
        --sources ~/Projects/agentic/RoadSide-Picnic/textures/old-road

Arguments: the zip, the folder of the JPEG originals, the folder for the maps. The PNGs the maps
are encoded from go to --sources (the assets workspace, not the game repo).

How. Each archived piece gets its workspace JPEG back (the UVs are checked equal), is scaled as
`old_road.rs` laid it (SCALE across and along, VERTICAL up) and is baked from a flat plane above
the module, casting straight down: the colour as emission, the height as emission strength. Rays
that miss (gaps between slabs) take the nearest hit. Copies of the module advanced OVERLAP of its
length, so the strip is one period long and its first rows cross-fade into the next copy's start.

Outputs (one strip each, columns across the road, rows along it toward +Z, so u = 0.5 is the
centreline; KTX2 with mips through tools/compress_glb_textures.py presets):
  old_road_albedo   BC7 sRGB colour; alpha 1 on the asphalt, 0 in potholes and the gaps between
                    slabs, where the kit has nothing under it and the terrain shows.
  old_road_normal   BC5 tangent normal in the road frame: R toward +u (across), G toward +rows
                    (along); 0.5 is flat. No OpenGL flip: the shader decodes it in that frame.
  old_road_height   BC4, 1 at the top and 0 deepest (and in the gaps), over the STRIP_DEPTH printed.
The script prints the native density, the strip size, STRIP_WIDTH, STRIP_PERIOD and STRIP_DEPTH;
the shader's constants (terrain.wesl) and `old_road_band` (terrain_material.rs) copy them.
"""

import argparse
import json
import struct
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

import numpy as np

try:
    import bpy
except ImportError:  # the helpers below run (and are tested) without Blender
    bpy = None

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compress_glb_textures import ctt_arguments, read_glb, write_glb  # noqa: E402

# src/world/old_road.rs before 6fb8d3f: metres per scan unit across and along the road, upward,
# the module's length in units and the share of it each copy advanced.
SCALE = 63.0
VERTICAL = 25.0
LENGTH = 0.9864
OVERLAP = 0.97
# The module's slabs, curbs and edge strips; the kit's other pieces lie buried under them.
PIECES = ("00", "01", "02", "03", "04", "05", "06", "07", "08", "10", "11", "15", "18", "22", "26", "27", "29", "33")
ZIP_FOLDER = "assets/models/road/uhd"
MAX_SIDE = 8192  # px: the longest side the GPU formats take everywhere
HEIGHT_BIAS = 10.0  # m added to the baked height, so a miss (0) is told from the deepest hit
HEIGHT_PERCENTILES = (0.1, 99.9)  # the depth range ignores a few stray scan vertices
BAKE_SAMPLES = 4
KTX2_QUALITY = "slow"

COMPONENTS = {5121: np.uint8, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


# ---------------------------------------------------------------- pure helpers


def accessor(gltf, binary, index):
    meta = gltf["accessors"][index]
    view = gltf["bufferViews"][meta["bufferView"]]
    dtype = np.dtype(COMPONENTS[meta["componentType"]])
    width = WIDTH[meta["type"]]
    offset = view.get("byteOffset", 0) + meta.get("byteOffset", 0)
    stride = view.get("byteStride", width * dtype.itemsize)
    rows = np.frombuffer(binary, np.uint8, meta["count"] * stride, offset).reshape(-1, stride)
    return rows[:, : width * dtype.itemsize].copy().view(dtype).reshape(-1, width)


def triangle_areas(points, triangles):
    a, b, c = (points[triangles[:, k]] for k in range(3))
    if points.shape[1] == 2:
        e, f = b - a, c - a
        return np.abs(e[:, 0] * f[:, 1] - e[:, 1] * f[:, 0]) / 2
    return np.linalg.norm(np.cross(b - a, c - a), axis=1) / 2


def native_px_per_metre(uv_areas, world_areas, texture_sizes):
    """Texels per metre over all pieces: their texel area over their world area, square-rooted."""
    texels = sum(uv * w * h for uv, (w, h) in zip(uv_areas, texture_sizes))
    return float(np.sqrt(texels / sum(world_areas)))


def blend_period(strip, period_px):
    """One period of `strip` (rows along the road), its first rows cross-faded with the rows past the
    period, which the next copy laid over them: row 0 is row `period_px`, the overlap's end is untouched."""
    overlap = strip.shape[0] - period_px
    out = strip[:period_px].copy()
    weight = (1 - np.arange(overlap) / overlap).reshape(-1, *([1] * (strip.ndim - 1)))
    out[:overlap] = strip[:overlap] * (1 - weight) + strip[period_px:] * weight
    return out


def normal_from_height(height, metres_per_px, depth_m):
    """RG tangent normal (0.5 flat) of a 0..1 height spanning `depth_m`: R toward +columns, G toward
    +rows. Rows wrap (the strip tiles along the road), columns stop at the road's edges."""
    metres = height * depth_m
    along = (np.roll(metres, -1, axis=0) - np.roll(metres, 1, axis=0)) / (2 * metres_per_px)
    padded = np.pad(metres, ((0, 0), (1, 1)), mode="edge")
    across = (padded[:, 2:] - padded[:, :-2]) / (2 * metres_per_px)
    normal = np.dstack([-across, -along, np.ones_like(metres)])
    normal /= np.linalg.norm(normal, axis=2, keepdims=True)
    return normal[..., :2] * 0.5 + 0.5


def fill_misses(values, hit):
    """`values` (rows, cols, channels) where `hit`, and the mean of the nearest hits elsewhere, grown
    one pixel ring at a time."""
    values = np.where(hit[..., None], values, 0.0)
    known = hit.copy()
    while not known.all():
        total = np.zeros_like(values)
        count = np.zeros(known.shape)
        for axis, step in ((0, 1), (0, -1), (1, 1), (1, -1)):
            total += np.roll(values * known[..., None], step, axis=axis)
            count += np.roll(known, step, axis=axis)
        grow = ~known & (count > 0)
        if not grow.any():
            raise ValueError("no hit to fill from")
        values[grow] = total[grow] / count[grow][:, None]
        known |= grow
    return values


def srgb_encode(linear):
    linear = np.clip(linear, 0, 1)
    return np.where(linear <= 0.0031308, linear * 12.92, 1.055 * linear ** (1 / 2.4) - 0.055)


def write_png(path, rows):
    """An 8-bit RGB or RGBA PNG from float 0..1 rows (top row first); no Pillow in Blender's Python."""
    import zlib

    pixels = np.round(np.clip(rows, 0, 1) * 255).astype(np.uint8)
    height, width, channels = pixels.shape
    raw = b"".join(b"\0" + pixels[row].tobytes() for row in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6 if channels == 4 else 2, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def with_jpeg(archived, original):
    """The archived (straightened) piece's glTF with the original's JPEG colour in place of its KTX2."""
    gltf, binary = read_glb(archived)
    source, source_binary = read_glb(original)
    primitive = gltf["meshes"][0]["primitives"][0]
    source_primitive = source["meshes"][0]["primitives"][0]
    uv, source_uv = (
        accessor(g, b, p["attributes"]["TEXCOORD_0"]) for g, b, p in ((gltf, binary, primitive), (source, source_binary, source_primitive))
    )
    if uv.shape != source_uv.shape or not np.array_equal(uv, source_uv):
        raise ValueError(f"{archived.name}: its UVs differ from the original's")
    image = source["images"][source["textures"][source["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"]]["source"]]
    view = source["bufferViews"][image["bufferView"]]
    jpeg = source_binary[view.get("byteOffset", 0) : view.get("byteOffset", 0) + view["byteLength"]]
    padding = b"\0" * (-len(binary) % 4)
    gltf["bufferViews"].append({"buffer": 0, "byteOffset": len(binary) + len(padding), "byteLength": len(jpeg)})
    binary = binary + padding + jpeg
    gltf["buffers"][0]["byteLength"] = len(binary)
    for entry in gltf["images"]:
        entry.update(bufferView=len(gltf["bufferViews"]) - 1, mimeType="image/jpeg")
    gltf.pop("extensionsUsed", None)
    gltf.pop("extensionsRequired", None)
    return gltf, binary


def piece_areas(gltf, binary):
    """(UV area, world area in m2 as laid) of one piece."""
    primitive = gltf["meshes"][0]["primitives"][0]
    triangles = accessor(gltf, binary, primitive["indices"]).reshape(-1, 3)
    positions = accessor(gltf, binary, primitive["attributes"]["POSITION"]).astype(np.float64)
    positions *= (SCALE, VERTICAL, SCALE)
    uv = accessor(gltf, binary, primitive["attributes"]["TEXCOORD_0"]).astype(np.float64)
    return triangle_areas(uv, triangles).sum(), triangle_areas(positions, triangles).sum()


def module_width(gltf_pieces):
    """The module's width in metres, symmetric about x = 0, where old_road.rs laid the road's centreline."""
    xs = np.concatenate(
        [accessor(g, b, g["meshes"][0]["primitives"][0]["attributes"]["POSITION"])[:, 0] for g, b in gltf_pieces]
    )
    return float(np.abs(xs).max()) * 2 * SCALE


def up4(value):
    return int(value) // 4 * 4


# ---------------------------------------------------------------- Blender


def use_gpu(scene):
    preferences = bpy.context.preferences.addons["cycles"].preferences
    for kind in ("OPTIX", "CUDA"):
        try:
            preferences.compute_device_type = kind
            preferences.get_devices()
        except (TypeError, RuntimeError):
            continue
        devices = [d for d in preferences.devices if d.type == kind]
        if devices:
            for device in preferences.devices:
                device.use = device.type == kind
            scene.cycles.device = "GPU"
            return f"{kind} {devices[0].name}"
    scene.cycles.device = "CPU"
    return "CPU"


def import_pieces(paths):
    pieces = []
    for path in paths:
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(path))
        pieces += [o for o in bpy.data.objects if o not in before and o.type == "MESH"]
    for piece in pieces:
        # glTF (x, y up, z) arrives as Blender (x, -z, y up): SCALE across and along, VERTICAL up.
        piece.scale = (SCALE, SCALE, VERTICAL)
        piece.select_set(True)
    bpy.context.view_layer.objects.active = pieces[0]
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return pieces


def colour_materials(pieces):
    """Each piece's base colour as emission: the bake reads the JPEG's linear colour."""
    sizes = []
    for piece in pieces:
        material = piece.active_material
        nodes, links = material.node_tree.nodes, material.node_tree.links
        image = next(n for n in nodes if n.type == "TEX_IMAGE")
        sizes.append(tuple(image.image.size))
        for node in [n for n in nodes if n.name != image.name]:
            nodes.remove(node)
        emission = nodes.new("ShaderNodeEmission")
        output = nodes.new("ShaderNodeOutputMaterial")
        links.new(image.outputs["Color"], emission.inputs["Color"])
        links.new(emission.outputs["Emission"], output.inputs["Surface"])
    return [p.active_material for p in pieces], sizes


def height_material():
    """White emission whose strength is the hit's height plus HEIGHT_BIAS."""
    material = bpy.data.materials.new("height")
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    nodes.clear()
    geometry = nodes.new("ShaderNodeNewGeometry")
    split = nodes.new("ShaderNodeSeparateXYZ")
    lift = nodes.new("ShaderNodeMath")
    lift.operation = "ADD"
    lift.inputs[1].default_value = HEIGHT_BIAS
    emission = nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = (1, 1, 1, 1)
    output = nodes.new("ShaderNodeOutputMaterial")
    links.new(geometry.outputs["Position"], split.inputs["Vector"])
    links.new(split.outputs["Z"], lift.inputs[0])
    links.new(lift.outputs["Value"], emission.inputs["Strength"])
    links.new(emission.outputs["Emission"], output.inputs["Surface"])
    return material


def bake_plane(x_range, y_range, top):
    """A plane above the module facing up, its UVs over the rectangle (v = 1 at the Blender +y end)."""
    mesh = bpy.data.meshes.new("plane")
    corners = [(x_range[0], y_range[0]), (x_range[1], y_range[0]), (x_range[1], y_range[1]), (x_range[0], y_range[1])]
    mesh.from_pydata([(x, y, top) for x, y in corners], [], [(0, 1, 2, 3)])
    layer = mesh.uv_layers.new(name="UVMap")
    for loop, (u, v) in zip(layer.data, ((0, 0), (1, 0), (1, 1), (0, 1))):
        loop.uv = (u, v)
    plane = bpy.data.objects.new("plane", mesh)
    bpy.context.scene.collection.objects.link(plane)
    return plane


def bake(plane, pieces, size, distance):
    """EMIT from the pieces onto the plane: float RGB, rows from the bottom."""
    image = bpy.data.images.new("bake", size[0], size[1], alpha=False, float_buffer=True)
    image.colorspace_settings.name = "Non-Color"
    image.generated_color = (0, 0, 0, 1)
    material = bpy.data.materials.new("target")
    material.use_nodes = True
    node = material.node_tree.nodes.new("ShaderNodeTexImage")
    node.image = image
    material.node_tree.nodes.active = node
    plane.data.materials.clear()
    plane.data.materials.append(material)
    bpy.ops.object.select_all(action="DESELECT")
    for piece in pieces:
        piece.select_set(True)
    plane.select_set(True)
    bpy.context.view_layer.objects.active = plane
    bpy.ops.object.bake(type="EMIT", use_selected_to_active=True, cage_extrusion=0.0, max_ray_distance=distance, margin=0)
    pixels = np.empty(size[0] * size[1] * 4, np.float32)
    image.pixels.foreach_get(pixels)
    bpy.data.images.remove(image)
    return pixels.reshape(size[1], size[0], 4)[..., :3].astype(np.float64)


def encode(png, ktx2, role):
    # "opaque": the albedo's alpha is coverage data, so ctt must not zero the colour where it is 0.
    subprocess.run(
        ["ctt", str(png), "-o", str(ktx2), "--quality", KTX2_QUALITY, "--zstd=19", "--input-alpha", "opaque"]
        + ctt_arguments(role),
        check=True,
        capture_output=True,
    )


def parse_args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("zip", type=Path)
    parser.add_argument("originals", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("--sources", type=Path, required=True)
    return parser.parse_args(argv)


def main():
    args = parse_args()
    scratch = Path(tempfile.mkdtemp(prefix="road-kit-"))
    with zipfile.ZipFile(args.zip) as archive:
        for piece in PIECES:
            archive.extract(f"{ZIP_FOLDER}/road_{piece}.glb", scratch)
    pieces = []
    for piece in PIECES:
        name = f"road_{piece}.glb"
        pieces.append(with_jpeg(scratch / ZIP_FOLDER / name, args.originals / name))
        write_glb(scratch / name, *pieces[-1])
    width_m = module_width(pieces)
    areas = [piece_areas(g, b) for g, b in pieces]

    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    print("device", use_gpu(scene))
    scene.cycles.samples = BAKE_SAMPLES
    objects = import_pieces([scratch / f"road_{p}.glb" for p in PIECES])
    colours, sizes = colour_materials(objects)

    density = native_px_per_metre([a for a, _ in areas], [w for _, w in areas], sizes)
    length_m = LENGTH * SCALE
    period_m = LENGTH * OVERLAP * SCALE
    period_px = up4(period_m * density)
    if period_px > MAX_SIDE:
        period_px = up4(MAX_SIDE)
    px_per_m = period_px / period_m
    columns = up4(width_m * px_per_m)
    rows = int(np.ceil(length_m * px_per_m))
    strip_width = columns / px_per_m
    print(f"native density {density:.1f} px/m, baked at {px_per_m:.2f}: strip {columns} x {period_px} px")

    # The module's rows run toward glTF +z, which is Blender -y: row 0 lies at Blender y = +length / 2.
    heights = np.concatenate([np.array([v.co.z for v in o.data.vertices]) for o in objects])
    top, bottom = heights.max() + 0.5, heights.min() - 0.5
    x_range = (-strip_width / 2, strip_width / 2)
    y_range = (length_m / 2 - rows / px_per_m, length_m / 2)
    plane = bake_plane(x_range, y_range, top)
    colour = bake(plane, objects, (columns, rows), top - bottom)[::-1]
    shared = height_material()
    for o in objects:
        o.data.materials.clear()
        o.data.materials.append(shared)
    raw_height = bake(plane, objects, (columns, rows), top - bottom)[::-1, :, :1]
    del colours

    # A miss is a pothole or a gap between slabs: the kit has no ground under it, so the shader shows
    # the terrain there (the albedo's alpha) and the height is the floor (0). Its colour is the
    # nearest asphalt's, so filtering and mips do not darken the edges.
    hit = raw_height[..., 0] > HEIGHT_BIAS / 2
    print(f"rays that hit: {hit.mean():.1%}")
    low, high = np.percentile((raw_height[..., 0] - HEIGHT_BIAS)[hit], HEIGHT_PERCENTILES)
    depth = float(high - low)
    height = np.where(hit, np.clip((raw_height[..., 0] - HEIGHT_BIAS - low) / depth, 0, 1), 0.0)
    colour = np.dstack([fill_misses(colour, hit), hit])
    colour = blend_period(colour, period_px)
    height = blend_period(height, period_px)
    normal = normal_from_height(height, 1 / px_per_m, depth)

    args.sources.mkdir(parents=True, exist_ok=True)
    args.out.mkdir(parents=True, exist_ok=True)
    maps = {
        "old_road_albedo": (np.dstack([srgb_encode(colour[..., :3]), colour[..., 3]]), "color"),
        "old_road_normal": (np.dstack([normal, np.ones(height.shape)]), "normal"),
        "old_road_height": (np.repeat(height[..., None], 3, axis=2), "depth"),
    }
    for name, (pixels, role) in maps.items():
        png = args.sources / f"{name}.png"
        write_png(png, pixels)
        encode(png, args.out / f"{name}.ktx2", role)
    print(f"STRIP_WIDTH {strip_width:.4f} m, STRIP_PERIOD {period_m:.4f} m, STRIP_DEPTH {depth:.4f} m")
    print(f"height range {low:.3f}..{high:.3f} m")


if __name__ == "__main__" and bpy is not None:
    main()
