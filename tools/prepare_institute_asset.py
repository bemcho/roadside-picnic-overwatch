"""Paint the Institute scan from its concept image (headless Blender with numpy).

The Tripo scan of the Institute (assets workspace models/landmarks/uhd/institute.glb, 191
untextured parts) has no UVs. This joins the parts and bakes one 2048 px atlas from the
concept image Tripo generated it from (images/institute-b420a284/generated_image.png): the
front and back facades get its window bays (two bays by two floors, cut at column and
spandrel centers), the side walls, parapet and roof precast panels and slabs cut from one
panel of its side wall, each at its own offset and tone. Each face
is projected flat into its region of the atlas by its main axis, so the scan stays one
primitive with one material, as the site's stand-in loader needs (assets::load_single_primitive).
A roughness map from the panes' blue tint lets the glass catch the sun. The geometry is unchanged.

  blender -b --factory-startup --python-exit-code 1 --python tools/prepare_institute_asset.py -- \\
      ~/Projects/agentic/RoadSide-Picnic/models/landmarks/uhd/institute.glb \\
      ~/Projects/agentic/RoadSide-Picnic/images/institute-b420a284/generated_image.png \\
      assets/models/landmarks/uhd/institute.glb
  npx @gltf-transform/cli simplify assets/models/landmarks/uhd/institute.glb /tmp/institute-low.glb \\
      --ratio 0.02 --error 0.002
  npx @gltf-transform/cli resize /tmp/institute-low.glb assets/models/landmarks/low/institute.glb \\
      --width 1024 --height 1024
  python3 tools/compress_glb_textures.py assets/models/landmarks/{uhd,low}/institute.glb

The facade's bays and floors (BAY, FLOOR, BAY_EDGE, WINDOWS_TOP) were measured on the scan in
meters; the crops are concept image pixels. Re-measure both if either source changes.
"""

import argparse
import sys
import tempfile
from pathlib import Path

import bpy
import numpy as np

ATLAS = 2048
# The scan's scale in the game (sites.rs, the Institute's model!), meters per model unit.
SCALE = 46.0
# The concept's window bays: two bays by two floors, from column center to column center and
# spandrel center to spandrel center (x0, y0, x1, y1 in image pixels, top-down).
FACADE_CROP = (852, 692, 1162, 867)
# The inside of one precast panel on the plain side wall, about 2.0 x 2.5 m.
PANEL_CROP = (1340, 735, 1424, 838)
PANEL_CROP_SIZE = (2.0, 2.5)
# The scan's bays and floors in meters: a column center 14.5 m either side of the middle,
# a floor every 2.8 m from the ground to the parapet, the roof slab at ROOF.
BAY = 5.8
FLOOR = 2.8
BAY_EDGE = 14.5
WINDOWS_TOP = 28.0
ROOF = 32.2
# The precast panels of the walls (one a floor) and the roof's slabs, and their joints.
WALL_PANEL = (2.4, FLOOR)
ROOF_SLAB = (6.0, 6.0)
JOINT = 0.05
ROUGH_CONCRETE, ROUGH_GLASS = 0.92, 0.3


def parse_args():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument('scan', type=Path)
    p.add_argument('concept', type=Path)
    p.add_argument('output', type=Path)
    return p.parse_args(argv)


def load_pixels(path):
    """The image's RGB as floats, row 0 at the top."""
    image = bpy.data.images.load(str(path))
    w, h = image.size
    flat = np.empty(w * h * 4, np.float32)
    image.pixels.foreach_get(flat)
    return flat.reshape(h, w, 4)[::-1, :, :3]


def overlap_tile(module, bx, by):
    """A seamless tile: the last `bx` columns and `by` rows fade into the first ones, so the
    tile repeats every (w - bx, h - by) pixels without a seam."""
    h, w, _ = module.shape
    out = module.copy()
    fade = np.linspace(0.0, 1.0, bx)[None, :, None]
    out[:, :bx] = module[:, w - bx:] * (1 - fade) + module[:, :bx] * fade
    out = out[:, :w - bx]
    fade = np.linspace(0.0, 1.0, by)[:, None, None]
    out[:by] = out[h - by:] * (1 - fade) + out[:by] * fade
    return out[:h - by]


def blur(tile, radius):
    """A wrapping box blur of `tile`."""
    for axis in (0, 1):
        tile = sum(np.roll(tile, i, axis) for i in range(-radius, radius + 1)) / (2 * radius + 1)
    return tile


def hash2(i, j):
    """A repeatable pseudo-random number in [0, 1) for each integer cell (i, j)."""
    return (np.sin(i * 127.1 + j * 311.7) * 43758.5453) % 1.0


def sample(tile, s, t):
    """Bilinear, wrapping samples of `tile` at fractions (s, t), t = 0 at the tile's top."""
    h, w, _ = tile.shape
    x = (s % 1.0) * w - 0.5
    y = (t % 1.0) * h - 0.5
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = (x - x0)[..., None], (y - y0)[..., None]
    x0, x1 = x0 % w, (x0 + 1) % w
    y0, y1 = y0 % h, (y0 + 1) % h
    top = tile[y0, x0] * (1 - fx) + tile[y0, x1] * fx
    bottom = tile[y1, x0] * (1 - fx) + tile[y1, x1] * fx
    return top * (1 - fy) + bottom * fy


class Layout:
    """Where each surface lies in the atlas, in pixels per meter: the facade (W x H) at the
    top left, the side walls (D x H) beside it and the roof (W x D) below the facade."""

    def __init__(self, low, high, height):
        self.x0, self.z0 = low
        self.w, self.d = high[0] - low[0], high[1] - low[1]
        self.h = height
        self.k = ATLAS / max(self.w + self.d, height + self.d)

    def facade(self, along, up):
        return along * self.k, (self.h - up) * self.k

    def side(self, along, up):
        return (self.w + along) * self.k, (self.h - up) * self.k

    def roof(self, along, across):
        return along * self.k, (self.h + across) * self.k


def paint(layout, concept):
    """The atlas's color and roughness, both (ATLAS, ATLAS, 3) floats, row 0 at the top."""
    def crop(box):
        x0, y0, x1, y1 = box
        return concept[y0:y1, x0:x1]
    windows = overlap_tile(crop(FACADE_CROP), 6, 8)
    # The panel's weathering without the concept's light falling across it, in the tone of
    # the concrete between the windows.
    panel = overlap_tile(crop(PANEL_CROP), 8, 8)
    panel = panel / np.maximum(blur(panel, 12), 1e-3)
    frame = windows[(windows[..., 2] - windows[..., 0]) < 0.01]
    panel *= np.median(frame, 0) / panel.mean((0, 1))
    rows, cols = np.mgrid[0:ATLAS, 0:ATLAS] + 0.5
    color = np.zeros((ATLAS, ATLAS, 3), np.float32)
    k, w, d, h = layout.k, layout.w, layout.d, layout.h

    def concrete(along, up, size):
        """Panels of `size` meters, each sampling the weathering at its own offset, mirroring
        and tone, with a dark joint around it."""
        i, j = np.floor(along / size[0]), np.floor(up / size[1])
        mirror = np.where(hash2(i + 1.0, j + 5.0) < 0.5, -1.0, 1.0)
        flip = np.where(hash2(i + 3.0, j + 2.0) < 0.5, -1.0, 1.0)
        s = mirror * (along - i * size[0]) / PANEL_CROP_SIZE[0] + hash2(i, j)
        t = flip * (j * size[1] - up) / PANEL_CROP_SIZE[1] + hash2(j, i)
        tone = 0.9 + 0.2 * hash2(i + 7.0, j + 3.0)
        edge = np.minimum(
            np.minimum(along % size[0], size[0] - along % size[0]),
            np.minimum(up % size[1], size[1] - up % size[1]),
        )
        shade = tone * (0.55 + 0.45 * np.clip(edge / JOINT, 0.0, 1.0))
        return sample(panel, s, t) * shade[:, None]

    def panel_wall(along, up):
        return concrete(along, up, WALL_PANEL)

    # Facade: window bays up to the parapet, panels above.
    facade = (cols < w * k) & (rows < h * k)
    along, up = cols[facade] / k, h - rows[facade] / k
    bays = sample(windows, (along + layout.x0 + BAY_EDGE) / (2 * BAY), -up / (2 * FLOOR))
    color[facade] = np.where((up < WINDOWS_TOP)[:, None], bays, panel_wall(along, up))
    side = (cols >= w * k) & (rows < h * k)
    color[side] = panel_wall(cols[side] / k - w, h - rows[side] / k)
    roof = rows >= h * k
    color[roof] = concrete(cols[roof] / k, rows[roof] / k - h, ROOF_SLAB)
    # The panes are blue-grey, the concrete grey: smooth where the pane tint shows.
    tint = color[..., 2] - color[..., 0]
    glass = np.clip((tint - 0.01) / 0.05, 0.0, 1.0) * facade
    rough = ROUGH_CONCRETE + (ROUGH_GLASS - ROUGH_CONCRETE) * glass
    surface = np.stack([np.ones_like(rough), rough, np.zeros_like(rough)], -1)
    return color, surface


def save_image(name, pixels, folder, non_color):
    image = bpy.data.images.new(name, ATLAS, ATLAS)
    rgba = np.concatenate([pixels[::-1], np.ones((ATLAS, ATLAS, 1), np.float32)], -1)
    image.pixels.foreach_set(rgba.astype(np.float32).ravel())
    image.filepath_raw = str(folder / f'{name}.png')
    image.file_format = 'PNG'
    image.save()
    image.colorspace_settings.name = 'Non-Color' if non_color else 'sRGB'
    return image


def joined_scan(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(path))
    parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
    for o in bpy.context.scene.objects:
        o.select_set(o in parts)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    scan = bpy.context.view_layer.objects.active
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    scan.name = scan.data.name = 'institute'
    return scan


def gltf_frame(blender_xyz):
    """Blender's Z-up coordinates as the GLB's Y-up ones, in meters."""
    x, y, z = blender_xyz.T
    return np.stack([x, z, -y], -1) * SCALE


def map_uvs(mesh, layout):
    """Projects each face into its atlas region by its main axis."""
    n = len(mesh.polygons)
    normals = np.empty(n * 3)
    mesh.polygons.foreach_get('normal', normals)
    normals = gltf_frame(normals.reshape(-1, 3)) / SCALE
    centers = np.empty(n * 3)
    mesh.polygons.foreach_get('center', centers)
    centers = gltf_frame(centers.reshape(-1, 3))
    totals = np.empty(n, int)
    mesh.polygons.foreach_get('loop_total', totals)
    loop_vertex = np.empty(len(mesh.loops), int)
    mesh.loops.foreach_get('vertex_index', loop_vertex)
    coords = np.empty(len(mesh.vertices) * 3)
    mesh.vertices.foreach_get('co', coords)
    p = gltf_frame(coords.reshape(-1, 3))[loop_vertex]
    face = np.repeat(np.arange(n), totals)
    axis = np.abs(normals).argmax(1)[face]
    sign = np.sign(normals[np.arange(n), np.abs(normals).argmax(1)])[face]
    above = centers[face, 1] > layout.h
    w, d, h = layout.w, layout.d, layout.h
    x = np.clip(p[:, 0] - layout.x0, 0, w)
    z = np.clip(p[:, 2] - layout.z0, 0, d)
    up = np.clip(p[:, 1], 0, h)
    # Each wall reads left to right as seen from outside.
    front = layout.facade(np.where(sign > 0, x, w - x), up)
    side = layout.side(np.where(sign > 0, d - z, z), up)
    roof = layout.roof(x, d - z)
    px = np.where(above | (axis == 1), roof[0], np.where(axis == 2, front[0], side[0]))
    py = np.where(above | (axis == 1), roof[1], np.where(axis == 2, front[1], side[1]))
    uv = np.stack([px / ATLAS, 1.0 - py / ATLAS], -1)
    layer = mesh.uv_layers.new(name='UVMap')
    layer.data.foreach_set('uv', uv.astype(np.float32).ravel())


def material(color, surface):
    mat = bpy.data.materials.new('institute')
    mat.use_backface_culling = True  # exported single-sided, as the scan was
    mat.use_nodes = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    bsdf = nodes['Principled BSDF']
    albedo = nodes.new('ShaderNodeTexImage')
    albedo.image = color
    links.new(albedo.outputs['Color'], bsdf.inputs['Base Color'])
    rough = nodes.new('ShaderNodeTexImage')
    rough.image = surface
    split = nodes.new('ShaderNodeSeparateColor')
    links.new(rough.outputs['Color'], split.inputs['Color'])
    links.new(split.outputs['Green'], bsdf.inputs['Roughness'])
    links.new(split.outputs['Blue'], bsdf.inputs['Metallic'])
    return mat


def main():
    args = parse_args()
    scan = joined_scan(args.scan)
    mesh = scan.data
    coords = np.empty(len(mesh.vertices) * 3)
    mesh.vertices.foreach_get('co', coords)
    p = gltf_frame(coords.reshape(-1, 3))
    body = p[p[:, 1] < ROOF]  # the walls, below the antennas
    layout = Layout(body.min(0)[[0, 2]], body.max(0)[[0, 2]], ROOF)
    print(f'institute: {layout.w:.2f} x {layout.d:.2f} m, roof {ROOF} m, {layout.k:.1f} px/m')
    color, surface = paint(layout, load_pixels(args.concept))
    folder = Path(tempfile.mkdtemp())
    mesh.materials.clear()
    mesh.materials.append(material(
        save_image('institute_color', color, folder, False),
        save_image('institute_surface', surface, folder, True),
    ))
    map_uvs(mesh, layout)
    bpy.ops.export_scene.gltf(
        filepath=str(args.output), export_format='GLB', export_image_format='AUTO',
        export_texcoords=True, export_normals=True, export_materials='EXPORT',
    )
    print('EXPORTED', args.output)


main()
