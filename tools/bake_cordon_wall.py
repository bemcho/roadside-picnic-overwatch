"""Bake the cordon wall: a box plus razor-wire strips that carry the UHD scan's detail (headless Blender).

The Perimeter repeats one wall panel thousands of times, so the 2,875-triangle scanned panel it used to
repeat is replaced by an 84-vertex box and two wire strips whose maps are baked from the UHD scan. Run it
from the repository root (Blender 4.5; `ctt`, `node` and the glb-bake-tangents tool must exist;
about 3 minutes with a GPU):

    BLENDER=~/.local/blender-4.5.16-linux-x64/blender
    "$BLENDER" -b --factory-startup --python-exit-code 1 --python tools/bake_cordon_wall.py -- \
        ~/Projects/agentic/RoadSide-Picnic/models/sites/uhd/site-cordon-wall.glb \
        assets/models/sites/medium assets/textures/cordon \
        [--sources ~/Projects/agentic/RoadSide-Picnic/textures/cordon-wall] [--span-px 2048] [--only sandbags]

Arguments: the UHD scan, the folder for the GLBs and the folder for the depth map. Every
intermediate PNG goes to --sources (the assets workspace, not the game repo). `--only sandbags` bakes
just the sandbag row (about 25 s) and leaves the box, the wire and the depth map as they are. The panel frame is the
old medium panel's (archived, `assets/models/sites/medium/site-cordon-wall.glb`): its lowest point is
y = 0 and its centre x = z = 0.

Outputs
  site-cordon-wall-box.glb   one primitive, one material (base colour + normal map, baked tangents).
  site-cordon-wall-wire.glb  one primitive, one MASK + double-sided material (colour + coverage).
  wall_depth.ktx2            linear BC4 with mips, the box's UV layout, white = deepest.
  site-cordon-wall-sandbags.glb  one primitive, one material (colour + normal map, baked tangents): the
                             sandbag row along the foot of both faces, at most 1,000 triangles.
The GLBs and the depth map are KTX2 (tools/compress_glb_textures.py presets); the PNGs stay out.

Frame. The scan lies along z, so it is turned a quarter about the vertical, stood on its lowest
point and centred on the wall body (not on the base lip or the sandbags): x runs along the panel,
y up from the foot, z across the thickness. The box spans exactly +-PANEL_SPAN / 2 in x; the scan's
body is 0.85 long, so its rounded ends fall inside the END_BAND the box seals at each end.
The box has no bottom; front is +z, back -z, plus the top and the two ends. Its vertical faces
sit at the median |z| of the scan's faces; the scan itself leans a centimetre and its pilaster ribs
stand 3 cm proud of that, so the depth map is 0 on them (it cannot show relief outside the box).

Box. 8 equal columns along x and rows at y = 0, FOUNDATION_ROW and the top, so the runtime shear
follows the ground and the water-foundation row stretches alone. On the front, back and top UV u
grows with +x (so the tangent runs along the panel everywhere and the back reads mirrored); on the
top v grows toward -z; on the ends u runs away from the face's corner, as seen from outside.
Atlas: one island per look, GUTTER px apart, SPAN_PX px along the panel, the same px per unit on
every face (rows from the top: front, back, top, end). Both ends use the one end island, baked
from the right end: the left end has the sandbag pile and a ladder against it.

Wire. Two strips of 8 columns spanning the panel: an upright one at z = 0 (the side view of the
coils) and a flat one at the coils' mid-height (the view from above). Both are baked from a copy
of the scan that holds only the coil rows (no concrete, and no tower, guards or hatch from the
middle of the thickness), since no stretch of the scan is both tower-free and as long as the panel.
Coverage is emission white on the wire and black elsewhere (alpha); the colour is un-premultiplied
by it and flat-filled where there is no wire, so mips show no dark halo.

Concrete source. Rays are cast at a copy of the scan from which the wire, the tower and the
sandbags are cut: what is above the flat top (measured in 4 cm columns: it wanders by a few mm),
and the warm (sandbag, tape) vertices low and beside the wall. Rays that still miss, hit an
inside face, hit the scan's own brownish stains, or (on the top) land on a contact mark of the
tower's legs or the coils (contact_marks: dark blobs, and dimples in the middle of the deck, wider
than a joint), take the colour, normal and depth of the hits around them. The faces' vertex counts cannot find the concrete top (the cap
steps out, the coils touch the faces), so TOP is the busiest height of the plateau the coils stand
on along both edges, printed with its histogram; DECK, a few mm lower, is the middle of the top.

Depth. Emission whose strength is the signed depth below the box's face (plus a bias that numpy
takes off), per face. The scan's lean, the cap's step and the foundation are bands along the wall,
so each row's median is subtracted (detrend_rows) and only the relief on them is kept (0 where the
surface is proud of its row); the end island is flat, and so are the top's shoulders (the outer
fifths along each edge, where the coils stood): only the deck's slab joints carry relief. The
result is scaled so DEPTH_RANGE model units are white.

Ends. The last END_BAND at both ends of the front, back and top faces is the body's ragged, rounded
end (and a ladder at one): there the normal is flat, the depth 0 and the albedo reflects the concrete
inside each end, blended toward the other end's mirror image so panels join without a seam (seal_ends).

Sandbags. The scan has no row of them: one block (two bags wide, four layers high) is piled against
each face at the left end, with a gap of a few cm behind it, plus a few single bags and strips of tape.
measure_bag_block finds the block (the longest run of warm low vertices that stand deep beside the front
face), bag_module builds one module as a low-poly loft of five stations (joint, crown, joint, crown,
joint) over a profile that goes from the wall over the crown and down the front, as deep as the block
from its back to its front, and one UV island over the whole atlas (u along x, v along the profile).
The module is baked once (colour and tangent normal) from the block, moved against the wall, trimmed by
BAG_TRIM at each end (the ends are ragged) and stretched to one module (BAG_COUNT of them span a panel);
holes in the scan (inside faces, concrete showing through, and a cable's dark loop round them) and
paint and tape on the front are filled from around (cold_texels, bright_marks, settle). The row is
BAG_COUNT copies along x on the front and mirrored on the back, so panels meet at the same profile.

The tool prints the measurements Task 5's constants need (HALF_THICKNESS, TOP, the wire's extent,
DEPTH_RANGE, and the sandbags' reach and height).
"""

import argparse
import math
import struct
import subprocess
import sys
import zlib
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

try:
    import bmesh
    import bpy
    from mathutils import Matrix, Vector
except ImportError:  # the measurement, layout and geometry code below runs (and is tested) without Blender
    bpy = None

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compress_glb_textures import ctt_arguments, read_glb  # noqa: E402

PANEL_SPAN = 0.86  # src/world/cordon.rs: the wall body's length, along x
COLUMNS = 8
FOUNDATION_ROW = 0.15  # the runtime stretches the rows below this height alone, over water
SPAN_PX = 2048  # px along the panel; the same density on every face and axis
GUTTER = 12  # px between islands and around them: the bake margin that mips and filtering bleed into

BAND = (0.10, 0.30)  # heights above the foot where the faces are clean concrete
TOP_BIN = 0.001  # height resolution of the search for the concrete top
WIRE_CLEARANCE = 0.0015  # the wire strips start this far above the concrete top
CONCRETE_NOISE = 0.001  # the wire is cut from the concrete source this far above the plateau (the scan's roughness)
WIRE_LIFT = 0.003  # the wire source starts this far above the plateau: the concrete's bumps stay out of it
PLATEAU_COLUMN = 0.04  # the plateau and the deck wander by a few mm along the wall: measured in columns this wide
PLATEAU_REACH = 0.01  # ... within this much of their overall height
MIDDLE_SHARE = 0.6  # the inner share of the thickness holds the tower and guards, not the coil rows
WARM = 1.25  # red over blue (linear) above which the scan's texture is sandbag or tape, not blue-grey concrete
STAINED = 1.08  # ... and above which a baked texel low on the wall is a brownish stain, not concrete (about 0.7)
BLACK = 0.003  # a baked colour darker than this (linear) is an inside face seen through a hole, not concrete
SETTLE_STEPS = 150  # px a hit spreads into the misses around it (the rib feet leave gaps about 100 px across)
GROUND_ZONE = 0.2  # sandbags and tape lie below this height
END_ZONE = 0.07  # sandbags are also piled against the wall's ends: this much of each end
SIDEWAYS = 0.15  # a baked normal whose tangent-space z is below this share of its length faces away: an inside face
END_BAND = 0.03  # the body's ends are ragged and rounded (and a ladder stands at one): this much is not baked
MARK_WINDOW = 41  # px; a dark mark is darker than MARK_SHARE of the mean this wide around it ...
MARK_SHARE = 0.7
MARK_OPEN = 2  # ... and wider than 2 x this many px, which the joints between slabs are not
MARK_GROW = 10  # px the mark's mask is grown by: past the dark halo painted round a foot
MARK_TILT = 0.08  # in the middle of the top, a normal tilted more than this (1 - z) is a dimple, not the flat deck
MARK_TILT_GROW = 12  # px a dimple's mask is grown by: a foot's thin arms reach far past its core
WIRE_MARGIN = 0.003  # padding of the strips around the measured wire

RIB_MARGIN = 0.004  # side rays start this far outside the pilaster ribs that stand proud of the faces
TOP_CAGE = 0.004  # top rays start this far above the concrete top (the deck's middle lies a few mm below it)
REACH = 0.1  # how far past the face a ray looks; the nearest surface wins, so more only fills in misses
WIRE_CAGE = 0.012  # the wire strips' rays start this far outside the coil rows
SAMPLES = 32  # antialiases the 8K scan texture down to the atlas density
ROUGHNESS = 0.9  # as the panel it replaces
BAG_COUNT = 7  # bag blocks along a panel: the scan's block is 0.117 wide, within 5 % of a 7th of the span
BAG_SLICE = 0.002  # x resolution of the search for the bag block
BAG_DEEP = 0.75  # a slice belongs to the block where the bags stand this share of the deepest slice out from the wall
BAG_PINCH = 0.003  # the front of a bag bulges this much past the block's depth, and a joint between bags is this far in
BAG_SAG = 0.002  # a joint between bags lies this much lower than the bags' crowns
BAG_ROUND = 0.007  # the rounding of the crown's front edge and of the front's foot
BAG_BACK = 0.004  # the row starts this far inside the wall's face (hidden), so sloping ground shows no gap
BAG_REAR = 5  # the pile's back stands where this percentile of the block's vertices lie out from the wall (a gap
# of a few cm lies between the scan's wall and its pile; the row is moved up against the wall)
BAG_DENSITY = 6000  # px per unit of the block's atlas
BAG_CAGE = 0.014  # rays start this far outside the row's low-poly surface ...
BAG_DEPTH_IN = 0.02  # ... and look this far inside it
BAG_COLD = 1.1  # a baked texel with red under this share of blue (plus 0.002) is concrete seen through a hole in a bag
BAG_TRIM = (
    0.004  # the block's ends are ragged and rounded, so a module is baked from the block without this much of each
)
BAG_BRIGHT = 2.2  # a texel on a bag's front brighter than this many times the cloth's median is paint or tape
BAG_BRIGHT_GROW = 8  # px a bright mark is grown by: past its pale rim
BAG_FRONT = 3  # index of the profile point where the front face starts
BAG_COLD_GROW = 24  # px a cold patch is grown by: past the dark loop (a cable or tie) round the hole it hides
KTX2_QUALITY = "slow"
DEPTH_BIAS = 0.1  # the depth shader emits depth + this (it cannot emit less than 0); numpy takes it off again
DEPTH_PERCENTILE = 99  # share of the long faces' detrended depth below white
VALIDATE_IGNORE = "TEXTURE_INVALID_IMAGE_MIME_TYPE,VALUE_NOT_IN_LIST,IMAGE_UNRECOGNIZED_FORMAT"  # KTX2 in a GLB
TANGENT_BAKER = Path("~/Projects/agentic/agentic-tooling/media/glb-bake-tangents/glb_bake_tangents.py").expanduser()
DEFAULT_SOURCES = Path("~/Projects/agentic/RoadSide-Picnic/textures/cordon-wall").expanduser()


# ---------------------------------------------------------------- measuring (numpy only)


@dataclass(frozen=True)
class Wall:
    """What the scan's wall body measures, in the scan's turned frame (x along the wall, y up)."""

    min_y: float  # the scan's lowest point, the foot
    centre_x: float
    centre_z: float
    half_thickness: float  # median |z| of the faces, from the wall's middle
    rib_height: float  # how far the pilaster ribs stand proud of the faces
    top: float  # the concrete top (the plateau along both edges), above the foot
    deck: float  # the middle of the top, a few mm lower: the tower's legs stand on it
    wire_top: float
    coil_half: float  # how far the coil rows reach from the middle
    top_peak: tuple  # (bin edges, counts) around the plateau's peak, for the printed check


def median_column_maximum(x, values, mask, width=0.02):
    """Median over x columns of the largest of `values` in each: a few columns' outliers (the tower) do not count."""
    columns = np.floor((x[mask] - x[mask].min()) / width).astype(int)
    largest = np.zeros(columns.max() + 1)
    np.maximum.at(largest, columns, values[mask])
    return float(np.median(largest[largest > 0]))


def busiest_height(y, strip):
    """(height, (slice edges, counts) around it) of the busiest TOP_BIN slice of the heights of `strip`'s vertices."""
    edges = np.arange(BAND[1], y[strip].max() + TOP_BIN, TOP_BIN)
    counts, _ = np.histogram(y[strip], bins=edges)
    peak = int(counts.argmax())
    height = float(np.median(y[strip & (y >= edges[peak]) & (y < edges[peak] + TOP_BIN)]))
    around = slice(max(peak - 2, 0), peak + 3)
    return height, (edges[around], counts[around])


def measure_wall(points):
    """Measure the scan's wall body from its vertices `points` (N x 3), already turned along x."""
    x, y, z = points[:, 0], points[:, 1] - points[:, 1].min(), points[:, 2]
    band = (y >= BAND[0]) & (y <= BAND[1])
    # The faces hold most of the band's vertices: the outer quarters' medians are on them.
    low, high = np.percentile(z[band], [12.5, 87.5])
    centre_z = (low + high) / 2
    half = float(np.median(np.abs(z[band] - centre_z)))
    rib = float(np.percentile(np.abs(z[band] - centre_z), 99)) - half
    left, right = np.percentile(x[band], [0.5, 99.5])

    # The concrete top is the plateau the coils stand on along both edges; the deck between them lies
    # a few mm lower. Each is the busiest slice above the band in its part of the thickness. (The faces'
    # vertex count cannot show it: the cap steps out at the band's top and the coils touch the faces.)
    outer = np.abs(z - centre_z)
    above = y > BAND[1]
    top, top_peak = busiest_height(y, (outer > MIDDLE_SHARE * half) & above)
    deck, _ = busiest_height(y, (outer < MIDDLE_SHARE * half) & above)

    # The coil rows stand on it either side of the middle.
    wire = (y > top + WIRE_CLEARANCE) & (outer > MIDDLE_SHARE * half) & (outer < 2 * half)
    return Wall(
        min_y=float(points[:, 1].min()),
        centre_x=float((left + right) / 2),
        centre_z=float(centre_z),
        half_thickness=half,
        rib_height=rib,
        top=top,
        deck=deck,
        wire_top=median_column_maximum(x, y, wire),
        coil_half=median_column_maximum(x, outer, wire),
        top_peak=top_peak,
    )


def plateau_heights(x, y, strip, level):
    """Per vertex, the height of the flat top near `level` in its x column (from the `strip` that lies on it)."""
    near = strip & (np.abs(y - level) < PLATEAU_REACH)
    bins = np.arange(level - PLATEAU_REACH, level + PLATEAU_REACH + TOP_BIN, TOP_BIN)
    centres, heights = [], []
    for low in np.arange(x.min(), x.max(), PLATEAU_COLUMN):
        column = near & (x >= low) & (x < low + PLATEAU_COLUMN)
        if column.sum() > 100:
            counts, _ = np.histogram(y[column], bins=bins)
            centres.append(low + PLATEAU_COLUMN / 2)
            heights.append(bins[counts.argmax()] + TOP_BIN / 2)
    return np.interp(x, centres, heights)


# ---------------------------------------------------------------- layout and geometry (numpy only)


@dataclass(frozen=True)
class Island:
    """A rectangle of the atlas in px (x0, y0 from the top-left, then width and height)."""

    x0: float
    y0: float
    width: float
    height: float
    atlas: tuple  # (width, height) px

    def uv(self, s, t):
        """UV of the point `s` along the island's width and `t` up its height (both 0..1)."""
        return (
            (self.x0 + s * self.width) / self.atlas[0],
            (self.atlas[1] - self.y0 - self.height + t * self.height) / self.atlas[1],
        )

    def pixels(self, pad=GUTTER // 2):
        """Pixel rows (from the bottom, as Blender stores them) and columns of the island, `pad` px beyond it."""
        bottom = self.atlas[1] - round(self.y0 + self.height) - pad
        left = round(self.x0) - pad
        return (
            slice(max(bottom, 0), bottom + round(self.height) + 2 * pad),
            slice(max(left, 0), left + round(self.width) + 2 * pad),
        )


def up4(value):
    return int(math.ceil(value / 4) * 4)  # BC7 blocks


def stack(sizes, density, atlas_width=None):
    """Islands of `sizes` (units) stacked in rows from the top, each row a list of sizes, GUTTER apart."""
    rows = [[(width * density, height * density) for width, height in row] for row in sizes]
    width = atlas_width or up4(max(sum(w for w, _ in row) + (len(row) + 1) * GUTTER for row in rows))
    height = up4(sum(max(h for _, h in row) + GUTTER for row in rows) + GUTTER)
    islands, y = [], GUTTER
    for row in rows:
        x = GUTTER
        for w, h in row:
            islands.append(Island(x, y, w, h, (width, height)))
            x += w + GUTTER
        y += max(h for _, h in row) + GUTTER
    return (width, height), islands


@dataclass
class Geometry:
    positions: list = field(default_factory=list)  # panel frame (x along the panel, y up, z across)
    uvs: list = field(default_factory=list)
    faces: list = field(default_factory=list)
    normals: list = field(default_factory=list)  # one intended outward normal per face

    def add_grid(self, island, origin, edge_u, edge_v, u_cuts, v_cuts, facing):
        """A quad grid on origin + s * edge_u + t * edge_v, cut at the fractions given.

        `s` follows u across the island and `t` runs up it. The faces wind counterclockwise as seen
        from where `facing` points (flipped when the edges say otherwise); no vertex is shared.
        """
        origin, edge_u, edge_v, facing = (np.asarray(v, float) for v in (origin, edge_u, edge_v, facing))
        flip = np.dot(np.cross(edge_u, edge_v), facing) < 0
        first, stride = len(self.positions), len(u_cuts)
        for t in v_cuts:
            for s in u_cuts:
                self.positions.append(origin + s * edge_u + t * edge_v)
                self.uvs.append(island.uv(s, t))
        for j in range(len(v_cuts) - 1):
            for i in range(stride - 1):
                a = first + j * stride + i
                quad = (a, a + 1, a + stride + 1, a + stride)
                self.faces.append(quad[::-1] if flip else quad)
                self.normals.append(facing)


def columns():
    return np.linspace(0, 1, COLUMNS + 1)


FACING = {  # each face's outward normal (panel frame)
    "front": (0, 0, 1),
    "back": (0, 0, -1),
    "top": (0, 1, 0),
    "end_left": (-1, 0, 0),
    "end_right": (1, 0, 0),
}


# The atlas has one island per look: the two ends share one, baked from the right end (the left end has the
# sandbag pile and the ladder against it, and ends mostly sit hidden against a neighbouring panel).
ISLANDS = {"front": "front", "back": "back", "top": "top", "end": "end_right"}  # island: the face it is baked from


def plane_offset(face, wall):
    """Distance of the box's `face` plane from the middle: the face's normal . any point on it."""
    return float(np.dot(np.abs(FACING[face]), (PANEL_SPAN / 2, wall.top, wall.half_thickness)))


def box_layout(wall, span_px=SPAN_PX):
    """(atlas size, {island name: Island}) for the box: front, back, top, then the end."""
    density = span_px / PANEL_SPAN
    thickness = 2 * wall.half_thickness
    size, islands = stack(
        [[(PANEL_SPAN, wall.top)], [(PANEL_SPAN, wall.top)], [(PANEL_SPAN, thickness)], [(thickness, wall.top)]],
        density,
        atlas_width=up4(span_px + 2 * GUTTER),
    )
    return size, dict(zip(ISLANDS, islands))


def box_geometry(wall, islands):
    half_span, h, top = PANEL_SPAN / 2, wall.half_thickness, wall.top
    rows = [0, FOUNDATION_ROW / top, 1]
    geometry = Geometry()
    along, up = (PANEL_SPAN, 0, 0), (0, top, 0)
    geometry.add_grid(islands["front"], (-half_span, 0, h), along, up, columns(), rows, FACING["front"])
    geometry.add_grid(islands["back"], (-half_span, 0, -h), along, up, columns(), rows, FACING["back"])
    geometry.add_grid(islands["top"], (-half_span, top, h), along, (0, 0, -2 * h), columns(), [0, 1], FACING["top"])
    geometry.add_grid(islands["end"], (-half_span, 0, -h), (0, 0, 2 * h), up, [0, 1], rows, FACING["end_left"])
    geometry.add_grid(islands["end"], (half_span, 0, h), (0, 0, -2 * h), up, [0, 1], rows, FACING["end_right"])
    return geometry


@dataclass(frozen=True)
class WireShape:
    floor: float  # the strips start here, clear of the concrete
    top: float
    half_width: float  # the flat strip's reach either side of the middle

    @property
    def middle(self):
        return (self.floor + self.top) / 2


def wire_shape(wall):
    return WireShape(
        floor=wall.top + WIRE_CLEARANCE,
        top=wall.wire_top + WIRE_MARGIN,
        half_width=wall.coil_half + WIRE_MARGIN,
    )


def wire_layout(shape, span_px=SPAN_PX):
    density = span_px / PANEL_SPAN
    size, islands = stack(
        [[(PANEL_SPAN, shape.top - shape.floor)], [(PANEL_SPAN, 2 * shape.half_width)]],
        density,
        atlas_width=up4(span_px + 2 * GUTTER),
    )
    return size, dict(zip(("upright", "flat"), islands))


def wire_geometry(shape, islands):
    half_span = PANEL_SPAN / 2
    geometry = Geometry()
    geometry.add_grid(
        islands["upright"], (-half_span, shape.floor, 0), (PANEL_SPAN, 0, 0), (0, shape.top - shape.floor, 0),
        columns(), [0, 1], (0, 0, 1),
    )  # fmt: skip
    geometry.add_grid(
        islands["flat"], (-half_span, shape.middle, shape.half_width), (PANEL_SPAN, 0, 0), (0, 0, -2 * shape.half_width),
        columns(), [0, 1], (0, 1, 0),
    )  # fmt: skip
    return geometry


@dataclass(frozen=True)
class BagBlock:
    """The scan's block of sandbags, measured on its front (z > 0): x ends, how far its back and its front stand out
    from the wall's face, and its height."""

    x0: float
    x1: float
    back: float
    depth: float
    top: float


def measure_bag_block(points, warm, half_thickness):
    """The widest run of x slices (BAG_SLICE) where the warm low vertices beside the front face stand deep.

    The scan has no row of sandbags: one block, two bags wide and four layers high, is piled against each
    face at the left end (and a few single bags and strips of tape lie elsewhere), so the block is the
    longest run of slices whose front stands out BAG_DEEP of the deepest slice's.
    """
    x, y, z = points.T
    beside = warm & (y < GROUND_ZONE) & (z > half_thickness)
    x, height, out = x[beside], y[beside], z[beside] - half_thickness
    slices = np.floor((x - x.min()) / BAG_SLICE).astype(int)
    front, top = np.zeros(slices.max() + 1), np.zeros(slices.max() + 1)
    np.maximum.at(front, slices, out)
    np.maximum.at(top, slices, height)
    deep = front >= BAG_DEEP * front.max()
    edges = np.flatnonzero(np.diff(np.concatenate([[0], deep.astype(int), [0]])))
    starts, ends = edges[::2], edges[1::2]
    longest = int(np.argmax(ends - starts))
    run = slice(starts[longest], ends[longest])
    x0, x1 = float(x.min() + run.start * BAG_SLICE), float(x.min() + run.stop * BAG_SLICE)
    return BagBlock(
        x0=x0,
        x1=x1,
        back=float(np.percentile(out[(x >= x0) & (x < x1)], BAG_REAR)),
        depth=float(np.median(front[run])),
        top=float(np.median(top[run])),
    )


BAG_STATIONS = (
    (0.0, -1),
    (0.25, 1),
    (0.5, -1),
    (0.75, 1),
    (1.0, -1),
)  # along a module: (fraction, +1 crown / -1 joint)


def bag_profile(depth, top):
    """A bag row's side profile, from the wall over the crown and down the front to the ground: (out, y) points."""
    return [
        (-BAG_BACK, top),
        (0.4 * depth, top),
        (depth - BAG_ROUND, top),
        (depth, top - BAG_ROUND),
        (depth + 0.001, top / 2),
        (depth, BAG_ROUND),
        (depth - 0.003, 0.0),
    ]


def profile_lengths(profile):
    """Distance along the polyline `profile` (N x 2) to each of its points."""
    return np.concatenate([[0], np.cumsum(np.linalg.norm(np.diff(profile, axis=0), axis=1))])


def bag_front_rows(block, size):
    """How many rows of a module's atlas (from the bottom) are the front face: v runs along the profile."""
    lengths = profile_lengths(np.array(bag_profile(block.depth - block.back + BAG_PINCH, block.top)))
    return round(size[1] * (1 - lengths[BAG_FRONT] / lengths[-1]))


def bag_module(block, half_thickness):
    """(Geometry, atlas size) of one module on the front (z > 0): two bags long, one UV island over the whole atlas.

    The bags stand against the wall, so the module is as deep as the block is from its back to its front.
    Five stations along x (joint, crown, joint, crown, joint), each a bag_profile that bulges at the crowns
    and pinches at the joints, so modules meet at identical profiles. u grows with +x; v runs along the
    profile from the wall (1) to the ground (0).
    """
    period = PANEL_SPAN / BAG_COUNT
    depth = block.depth - block.back
    crown = np.array(bag_profile(depth + BAG_PINCH, block.top))
    lengths = profile_lengths(crown)
    size = (up4(period * BAG_DENSITY), up4(lengths[-1] * BAG_DENSITY))
    geometry = Geometry()
    for fraction, sign in BAG_STATIONS:
        station_depth, top = depth + sign * BAG_PINCH, block.top + (sign - 1) / 2 * BAG_SAG
        for (out, y), length in zip(bag_profile(station_depth, top), lengths):
            geometry.positions.append((period * (fraction - 0.5), y, half_thickness + out))
            geometry.uvs.append((fraction, 1 - length / lengths[-1]))
    rows = len(crown)
    for station in range(len(BAG_STATIONS) - 1):
        for row in range(rows - 1):
            a = station * rows + row
            quad = [a, a + rows, a + rows + 1, a + 1]
            corners = np.array([geometry.positions[i] for i in quad])
            normal = np.cross(corners[2] - corners[0], corners[3] - corners[1])
            normal /= np.linalg.norm(normal)
            step = (
                crown[row + 1] - crown[row]
            )  # the profile runs from the wall over the top and down: out is to its left
            outward = np.array([0.0, step[0], -step[1]])
            if np.dot(normal, outward) < 0:
                quad, normal = quad[::-1], -normal
            geometry.faces.append(tuple(quad))
            geometry.normals.append(normal)
    return geometry, size


def bag_cap(row, module, station, shift, side, direction):
    """Close the row's end: `module`'s end profile at `station`, moved `shift` along x and mirrored for the back,
    with its foot against the wall, as one flat polygon facing `direction` (+1 or -1 along x).

    The row is a hollow loft and its faces are single-sided, so without this an end seen end-on (a gate's jamb,
    the corner of a run) shows nothing but the wall behind it. Vertices are not shared with the sides, so the
    cap stays flat under smooth shading.
    """
    rows = len(module.positions) // len(BAG_STATIONS)
    ring = range(station * rows, (station + 1) * rows)
    points = [module.positions[i] for i in ring]
    uvs = [module.uvs[i] for i in ring]
    points.append((points[0][0], 0.0, points[0][2]))  # the foot, below where the profile starts at the wall
    uvs.append((uvs[0][0], 0.0))
    first = len(row.positions)
    row.positions += [(x + shift, y, side * z) for x, y, z in points]
    row.uvs += uvs
    face = tuple(range(first, first + len(points)))
    corners = np.array(row.positions[first:])
    area = sum(np.cross(a, b) for a, b in zip(corners, np.roll(corners, -1, axis=0)))
    if area[0] * direction < 0:
        face = face[::-1]
    row.faces.append(face)
    row.normals.append(np.array([direction, 0.0, 0.0]))


def bag_row(module):
    """The whole row of one panel: BAG_COUNT copies of `module` along x on the front, and mirrored on the back,
    each side closed at both ends."""
    period = PANEL_SPAN / BAG_COUNT
    row = Geometry()
    for side in (1, -1):
        for copy in range(BAG_COUNT):
            first = len(row.positions)
            shift = (copy - (BAG_COUNT - 1) / 2) * period
            row.positions += [(x + shift, y, side * z) for x, y, z in module.positions]
            row.uvs += module.uvs
            row.faces += [tuple(first + i for i in (face if side > 0 else face[::-1])) for face in module.faces]
            row.normals += [normal * np.array([1, 1, side]) for normal in module.normals]
        shift = (BAG_COUNT - 1) / 2 * period
        bag_cap(row, module, 0, -shift, side, -1)
        bag_cap(row, module, len(BAG_STATIONS) - 1, shift, side, 1)
    return row


def triangle_count(faces):
    """Triangles in `faces` (quads and polygons count as fans)."""
    return sum(len(face) - 2 for face in faces)


def check_row_extent(row):
    """Fail unless the row spans exactly +-PANEL_SPAN / 2 in x (within 1e-4), so it repeats from panel to panel."""
    x = np.array(row.positions)[:, 0]
    if abs(x.min() + PANEL_SPAN / 2) > 1e-4 or abs(x.max() - PANEL_SPAN / 2) > 1e-4:
        raise SystemExit(f"sandbag row x extent is {x.min():.5f}..{x.max():.5f}, not +-{PANEL_SPAN / 2}")


def bright_marks(colour):
    """Mask of the paint and tape marks on baked bag cloth `colour` (rows x columns x RGB): far brighter than its
    median, grown past their pale rim."""
    brightest = colour.max(axis=2)
    return dilate(brightest > BAG_BRIGHT * np.median(brightest), BAG_BRIGHT_GROW)


def cold_texels(colour):
    """Mask of baked `colour` (rows x columns x RGB, linear) that is not a sandbag's warm cloth: black (an inside
    face seen through a hole) or concrete's blue-grey seen through one."""
    return (colour.max(axis=2) < BLACK) | (colour[..., 0] < BAG_COLD * colour[..., 2] + 0.002)


def depth_image(depth, mask, percentile=DEPTH_PERCENTILE):
    """(0..1 map, the depth that is white): `depth` scaled so `percentile` of the pixels in `mask` stay below white."""
    white = float(np.percentile(depth[mask], percentile))
    return np.clip(depth / white, 0, 1), white


def detrend_rows(depth, columns):
    """`depth` below each row's median over `columns` (0 where the surface stands proud of its row).

    The scan's lean, the cap's step and the foundation are bands along the wall, so a row's median
    is the surface at that height and only the relief on it is left (the ribs stand proud: 0).
    """
    return np.maximum(depth - np.median(depth[:, columns], axis=1, keepdims=True), 0)


def seal_ends(albedo, normal, depth, pad, band):
    """Make both ends of a long face agree where panels abut, in place (each map has `pad` px margin each side).

    The normal is flat and the depth 0 in the last `band` columns. The albedo there reflects the
    concrete just inside each end, then each end is blended toward the other's mirror image (half way
    at the outer edge, not at all at the inner one), so the two edges are equal and the join is seamless.
    """
    width = albedo.shape[1]
    left, right = slice(pad, pad + band), slice(width - pad - band, width - pad)
    albedo[:, left] = albedo[:, pad + band : pad + 2 * band][:, ::-1]
    albedo[:, right] = albedo[:, width - pad - 2 * band : width - pad - band][:, ::-1]
    weight = (0.5 * (1 - np.arange(band) / band))[None, :, None]  # from the outer edge inward
    outer_left, outer_right = albedo[:, left].copy(), albedo[:, right][:, ::-1].copy()
    albedo[:, left] = (1 - weight) * outer_left + weight * outer_right
    albedo[:, right] = ((1 - weight) * outer_right + weight * outer_left)[:, ::-1]
    normal[:, left, :3] = normal[:, right, :3] = (0.5, 0.5, 1.0)
    depth[:, left, 0] = depth[:, right, 0] = 0
    for pixels in (albedo, normal, depth):  # the margin carries on from the sealed edges
        pixels[:, :pad] = pixels[:, pad : pad + 1]
        pixels[:, width - pad :] = pixels[:, width - pad - 1 : width - pad]


def box_blur(values, size):
    """Mean of `values` over a `size` x `size` window (odd `size`; the edges repeat)."""
    pad = size // 2
    integral = np.pad(
        np.pad(values, pad, mode="edge").astype(np.float64).cumsum(axis=0).cumsum(axis=1), ((1, 0), (1, 0))
    )
    rows, columns = values.shape
    window = (
        integral[size : size + rows, size : size + columns]
        - integral[:rows, size : size + columns]
        - integral[size : size + rows, :columns]
        + integral[:rows, :columns]
    )
    return window / size**2


def dilate(mask, radius):
    """`mask` grown by `radius` px (a diamond)."""
    for _ in range(radius):
        mask = mask | shifted(mask, 0, 1) | shifted(mask, 0, -1) | shifted(mask, 1, 1) | shifted(mask, 1, -1)
    return mask


def contact_marks(colour, normal, middle):
    """Mask of the scan's contact marks in a baked top face (rows x columns x RGB `colour` and `normal`).

    The tower's legs and the coils leave dark paint on the concrete, and the legs dimples in it. A mark is
    a blob that is darker than its surroundings, or (on the `middle` rows, where the legs stood) whose normal
    is tilted off the flat deck, and wider than the joints between slabs (an opening removes those thin lines).
    """
    luminance = colour @ np.array([0.2126, 0.7152, 0.0722])
    vectors = normal * 2 - 1
    tilted = (vectors[..., 2] < (1 - MARK_TILT) * np.linalg.norm(vectors, axis=2)) & middle[:, None]

    def blobs(mask, grow):
        return dilate(~dilate(~mask, MARK_OPEN), MARK_OPEN + grow)

    return blobs(luminance < MARK_SHARE * box_blur(luminance, MARK_WINDOW), MARK_GROW) | blobs(tilted, MARK_TILT_GROW)


def shifted(array, axis, step):
    """`array` moved `step` pixels along `axis`, zeros coming in."""
    moved = np.zeros_like(array)
    source, target = [slice(None)] * array.ndim, [slice(None)] * array.ndim
    source[axis], target[axis] = (
        (slice(0, -step), slice(step, None)) if step > 0 else (slice(-step, None), slice(0, step))
    )
    moved[tuple(target)] = array[tuple(source)]
    return moved


def settle(maps, alpha, dark, steps=SETTLE_STEPS):
    """Finish baked `maps` (in place) that share a coverage `alpha`: the bake averaged misses in as black.

    Pixels half hit get back the colour of the hits; pixels no ray hit, or that hit an inside face
    (`dark`), take the average of their hit neighbours, a pixel further per step.
    """
    hit = (alpha >= 0.5) & ~dark
    for pixels in maps:
        pixels[hit] /= alpha[hit][:, None]
    for _ in range(steps):
        sides = [(axis, step) for axis in (0, 1) for step in (1, -1)]
        count = sum(shifted(hit, axis, step).astype(np.float32) for axis, step in sides)
        grow = ~hit & (count > 0)
        if not grow.any():
            break
        for pixels in maps:
            total = sum(shifted(pixels * hit[:, :, None], axis, step) for axis, step in sides)
            pixels[grow] = total[grow] / count[grow][:, None]
        hit = hit | grow


def unpremultiply(colour, coverage):
    """Colour of the wire itself from a bake that averaged misses in as black; a flat fill elsewhere."""
    wire = coverage > 0.02
    flat = colour[wire].sum(axis=0) / max(coverage[wire].sum(), 1e-6)
    return np.where(wire[..., None], np.clip(colour / np.maximum(coverage, 1e-6)[..., None], 0, 1), np.clip(flat, 0, 1))


def srgb_encode(linear):
    linear = np.clip(linear, 0, 1)
    return np.where(linear <= 0.0031308, linear * 12.92, 1.055 * linear ** (1 / 2.4) - 0.055)


def write_png(path, rows_from_bottom):
    """An 8-bit PNG (RGB or RGBA, float 0..1 rows from the bottom as Blender stores them); no Pillow here."""
    pixels = np.round(np.clip(rows_from_bottom[::-1], 0, 1) * 255).astype(np.uint8)
    height, width, channels = pixels.shape
    raw = b"".join(b"\0" + pixels[row].tobytes() for row in range(height))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, 8, 6 if channels == 4 else 2, 0, 0, 0)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    )


# ---------------------------------------------------------------- Blender: scene, bake, export


def to_blender(point):
    """Panel frame (x, y up, z) -> Blender (x, -z, y up), which the glTF exporter turns back."""
    return (point[0], -point[2], point[1])


def to_panel(coordinates):
    """Blender vertex coordinates (N x 3) -> the panel frame."""
    return np.stack([coordinates[:, 0], coordinates[:, 2], -coordinates[:, 1]], axis=1)


def vertex_coordinates(mesh):
    flat = np.empty(len(mesh.vertices) * 3, np.float32)
    mesh.vertices.foreach_get("co", flat)
    return flat.reshape(-1, 3)


def import_glb(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    imported = [o for o in bpy.data.objects if o not in before and o.type == "MESH"]
    if len(imported) != 1:
        raise SystemExit(f"{path}: expected one mesh object, found {len(imported)}")
    return imported[0]


def scan_shift(wall):
    """The move onto the panel frame: the scan's lowest point to y = 0 and its wall body's centre to x = z = 0."""
    return (-wall.centre_x, -wall.min_y, -wall.centre_z)


def stand_scan_on_panel_frame(scan):
    """Turn the scan along x, measure its wall body, and stand it on the panel frame."""
    scan.data.transform(Matrix.Rotation(math.radians(90), 4, "Z"))  # (x, y) -> (-y, x): length onto x
    wall = measure_wall(to_panel(vertex_coordinates(scan.data)))
    scan.data.transform(Matrix.Translation(to_blender(scan_shift(wall))))
    return wall


def mesh_object(name, geometry):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata([to_blender(p) for p in geometry.positions], [], geometry.faces)
    layer = mesh.uv_layers.new(name="UVMap")
    for loop in mesh.loops:
        layer.data[loop.index].uv = geometry.uvs[loop.vertex_index]
    for polygon, normal in zip(mesh.polygons, geometry.normals):
        if Vector(polygon.normal).dot(Vector(to_blender(normal))) < 0.99:
            raise SystemExit(f"{name}: face {polygon.index} winds away from its intended normal")
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


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


def emission_material(name, strength=None):
    """White emission; `strength` builds the socket value (None: 1) from the material's node tree."""
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    nodes.clear()
    emission = nodes.new("ShaderNodeEmission")
    emission.inputs["Color"].default_value = (1, 1, 1, 1)
    output = nodes.new("ShaderNodeOutputMaterial")
    links.new(emission.outputs["Emission"], output.inputs["Surface"])
    if strength:
        links.new(strength(nodes, links), emission.inputs["Strength"])
    return material


def depth_strength(face, wall):
    """Strength = depth below the box's `face` plus DEPTH_BIAS: the plane's offset minus the hit point's, along the normal.

    The depth is signed (negative where the scan stands proud of the face, as the ribs do); the bias keeps
    the emission above 0 and numpy takes it off again.
    """
    normal, offset = to_blender(FACING[face]), plane_offset(face, wall) + DEPTH_BIAS

    def build(nodes, links):
        position = nodes.new("ShaderNodeNewGeometry")
        along = nodes.new("ShaderNodeVectorMath")
        along.operation = "DOT_PRODUCT"
        links.new(position.outputs["Position"], along.inputs[0])
        along.inputs[1].default_value = normal
        behind = nodes.new("ShaderNodeMath")
        behind.operation = "SUBTRACT"
        behind.inputs[0].default_value = offset
        links.new(along.outputs["Value"], behind.inputs[1])
        return behind.outputs["Value"]

    return build


@dataclass(frozen=True)
class Ray:
    extrusion: float  # how far outside the face the ray starts
    distance: float  # its length


def bake(target, source, size, kind, ray, source_material=None, margin=GUTTER):
    """One selected-to-active bake of `source` onto `target`; float32 RGBA rows from the bottom.

    A pixel no ray hit keeps alpha 0, so the alpha channel is the coverage.
    """
    image = bpy.data.images.new("bake", *size, alpha=True, float_buffer=True)
    material = bpy.data.materials.new("bake")
    material.use_nodes = True
    node = material.node_tree.nodes.new("ShaderNodeTexImage")
    node.image = image
    material.node_tree.nodes.active = node
    target.data.materials.clear()
    target.data.materials.append(material)
    original = source.data.materials[0]
    if source_material:
        source.data.materials[0] = source_material

    bpy.ops.object.select_all(action="DESELECT")
    source.select_set(True)
    target.select_set(True)
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.bake(
        type=kind,
        pass_filter={"COLOR"} if kind == "DIFFUSE" else set(),
        margin=margin,
        margin_type="EXTEND",
        use_selected_to_active=True,
        max_ray_distance=ray.distance,
        cage_extrusion=ray.extrusion,
        normal_space="TANGENT",
        use_clear=True,
    )
    source.data.materials[0] = original
    pixels = np.empty(size[0] * size[1] * 4, np.float32)
    image.pixels.foreach_get(pixels)
    bpy.data.images.remove(image)
    bpy.data.materials.remove(material)
    return pixels.reshape(size[1], size[0], 4)


def face_rays(wall):
    """The rays of each box face: the sides start outside the ribs, the top just above the concrete."""
    side = Ray(wall.rib_height + RIB_MARGIN, wall.rib_height + RIB_MARGIN + REACH)
    top = Ray(TOP_CAGE, TOP_CAGE + REACH)
    return {face: top if face == "top" else side for face in FACING}


def unusable_texels(colour, normal, density):
    """Mask of the baked `colour` and `normal` (an island and its padding, rows from the bottom) that is not concrete.

    Black, or a normal facing away, is an inside face seen through a hole; a warm texel low on the
    wall is the scan's brownish stain.
    """
    vectors = normal * 2 - 1
    unusable = (colour.max(axis=2) < BLACK) | (vectors[..., 2] < SIDEWAYS * np.linalg.norm(vectors, axis=2))
    low = slice(0, round(GROUND_ZONE * density) + GUTTER)
    unusable[low] |= colour[low, :, 0] > STAINED * colour[low, :, 2] + 0.002
    return unusable


def bake_box(source, box, size, islands, wall):
    """(colour, tangent normal, depth in model units), baked face by face with that face's rays and settled."""
    rays = face_rays(wall)
    maps = {kind: np.zeros((*size[::-1], 4), np.float32) for kind in ("albedo", "normal", "depth")}
    for name, island in islands.items():
        face, where = ISLANDS[name], island.pixels()
        for kind_name, kind, material in (
            ("albedo", "DIFFUSE", None),
            ("normal", "NORMAL", None),
            ("depth", "EMIT", emission_material(f"depth {name}", depth_strength(face, wall))),
        ):
            maps[kind_name][where] = bake(box, source, size, kind, rays[face], material)[where]
    albedo, normal, depth = maps["albedo"], maps["normal"], maps["depth"]
    density = islands["front"].height / wall.top  # px per unit
    pad, band = GUTTER // 2, round(END_BAND * density)
    for name, island in islands.items():
        where = island.pixels()
        alpha = albedo[where][..., 3].copy()
        if name == "top":  # only the deck's slab joints carry relief: the shoulders along the edges are flat
            rows = np.arange(alpha.shape[0]) - (alpha.shape[0] - 1) / 2
            middle = np.abs(rows) < MIDDLE_SHARE * island.height / 2
            normal[where][~middle, :, :3] = np.array([0.5, 0.5, 1.0]) * alpha[~middle][..., None]
            depth[where][~middle, :, 0] = DEPTH_BIAS * alpha[~middle]
        colour = albedo[where][..., :3] / np.maximum(alpha, 1e-6)[..., None]
        direction = normal[where][..., :3] / np.maximum(alpha, 1e-6)[..., None]
        unusable = unusable_texels(colour, direction, density)
        if name == "top":  # the contact marks under the tower's legs and the coils
            unusable |= contact_marks(colour, direction, middle)
        settle([albedo[where], normal[where], depth[where]], alpha, unusable)

        crop = depth[where]
        inner = slice(pad + band, crop.shape[1] - pad - band)
        crop[..., 0] = 0 if name == "end" else detrend_rows(crop[..., 0] - DEPTH_BIAS, inner)
        if name != "end":
            seal_ends(albedo[where], normal[where], crop, pad, band)
    vectors = normal[..., :3] * 2 - 1
    unit = vectors / np.maximum(np.linalg.norm(vectors, axis=2, keepdims=True), 1e-6)
    return albedo[..., :3], unit * 0.5 + 0.5, depth[..., 0]


def bake_wire(wire_scan, wire, size, shape, islands):
    """Colour and coverage of both strips, each baked with the rays its view needs, merged by island."""
    thickness = 2 * shape.half_width
    views = {
        "upright": Ray(thickness / 2 + WIRE_CAGE, thickness + 2 * WIRE_CAGE),  # from the side, through both coil rows
        "flat": Ray(
            shape.top - shape.middle + WIRE_CAGE, shape.top - shape.floor + WIRE_CAGE
        ),  # from above, down to the floor
    }
    colour, coverage = np.zeros((*size[::-1], 4), np.float32), np.zeros((*size[::-1], 4), np.float32)
    white = emission_material("coverage")
    for name, ray in views.items():
        for result, kind, material in ((colour, "DIFFUSE", None), (coverage, "EMIT", white)):
            baked = bake(wire, wire_scan, size, kind, ray, material)
            where = islands[name].pixels()
            result[where] = baked[where]
    return colour, coverage


def bag_source(scan, block, half_thickness):
    """A copy of the scan with only the bag block and the wall face behind it, centred in x and stretched to
    one module's length, and moved against the wall's face: what a module's rays hit."""
    x, y, z = to_panel(vertex_coordinates(scan.data)).T
    first, last = block.x0 + BAG_TRIM, block.x1 - BAG_TRIM
    near = (x > first - BAG_SLICE) & (x < last + BAG_SLICE) & (y < GROUND_ZONE) & (z > half_thickness - BAG_BACK)
    copy = copy_without(scan, ~near)
    stretch = PANEL_SPAN / BAG_COUNT / (last - first)
    move = Matrix.Scale(stretch, 4, Vector((1, 0, 0))) @ Matrix.Translation(
        to_blender((-(block.x0 + block.x1) / 2, 0, -block.back))
    )
    copy.data.transform(move)
    return copy


def bake_bags(source, module, size, front_rows):
    """(colour, tangent normal) of one module baked from the scan's bags; holes in the scan, and paint and tape
    on the front (its first `front_rows` rows), are filled from around."""
    ray = Ray(BAG_CAGE, BAG_CAGE + BAG_DEPTH_IN)
    albedo, normal = (bake(module, source, size, kind, ray) for kind in ("DIFFUSE", "NORMAL"))
    alpha = albedo[..., 3].copy()
    colour = albedo[..., :3] / np.maximum(alpha, 1e-6)[..., None]
    vectors = normal[..., :3] / np.maximum(alpha, 1e-6)[..., None] * 2 - 1
    sideways = vectors[..., 2] < SIDEWAYS * np.linalg.norm(vectors, axis=2)
    unusable = dilate(cold_texels(colour), BAG_COLD_GROW) | sideways
    unusable[:front_rows] |= bright_marks(colour[:front_rows])
    settle([albedo, normal], alpha, unusable)
    vectors = normal[..., :3] * 2 - 1
    unit = vectors / np.maximum(np.linalg.norm(vectors, axis=2, keepdims=True), 1e-6)
    return albedo[..., :3], unit * 0.5 + 0.5


def warm_vertices(scan):
    """Mask of the scan's vertices whose texture colour is warm: sandbags and tape, where the concrete is blue-grey."""
    image = next(n.image for n in scan.data.materials[0].node_tree.nodes if n.type == "TEX_IMAGE").copy()
    image.scale(1024, 1024)
    colours = np.empty(1024 * 1024 * 4, np.float32)
    image.pixels.foreach_get(colours)
    colours = colours.reshape(1024, 1024, 4)
    bpy.data.images.remove(image)

    mesh = scan.data
    corner_vertex = np.empty(len(mesh.loops), np.int32)
    mesh.loops.foreach_get("vertex_index", corner_vertex)
    uv = np.empty(len(mesh.loops) * 2, np.float32)
    mesh.uv_layers[0].data.foreach_get("uv", uv)
    column, row = (np.clip((uv.reshape(-1, 2) * 1024).astype(int), 0, 1023)).T
    colour = colours[row, column]
    warm = np.zeros(len(mesh.vertices), bool)
    warm[corner_vertex[colour[:, 0] > WARM * colour[:, 2] + 0.002]] = True
    return warm


def copy_without(scan, doomed):
    """A copy of the scan (object and mesh) without the vertices in the `doomed` mask."""
    copy = scan.copy()
    copy.data = scan.data.copy()
    bpy.context.scene.collection.objects.link(copy)
    mesh = bmesh.new()
    mesh.from_mesh(copy.data)
    mesh.verts.ensure_lookup_table()
    bmesh.ops.delete(mesh, geom=[mesh.verts[i] for i in np.nonzero(doomed)[0]], context="VERTS")
    mesh.to_mesh(copy.data)
    mesh.free()
    return copy


def top_heights(scan, wall):
    """The scan's panel-frame points (x, y, z), a mask of those in the middle of the thickness, and the height
    of the flat top in each one's x column: the plateau along the edges, the deck in the middle."""
    x, y, z = to_panel(vertex_coordinates(scan.data)).T
    middle = np.abs(z) < MIDDLE_SHARE * wall.half_thickness
    flat = np.where(middle, plateau_heights(x, y, middle, wall.deck), plateau_heights(x, y, ~middle, wall.top))
    return (x, y, z), middle, flat


def concrete_source(scan, wall):
    """The scan without what stands above the concrete (wire, tower) or lies by the ground (sandbags, tape)."""
    (x, y, z), _, flat = top_heights(scan, wall)
    beside_wall = (np.abs(z) > wall.half_thickness) | (np.abs(x) > PANEL_SPAN / 2 - END_ZONE)
    ground_litter = warm_vertices(scan) & (y < GROUND_ZONE) & beside_wall
    return copy_without(scan, (y > flat + CONCRETE_NOISE) | ground_litter)


def wire_source(scan, wall):
    """Only the coil rows: the scan without the concrete, and without the tower, guards and hatch in the middle."""
    (_, y, _), middle, flat = top_heights(scan, wall)
    return copy_without(scan, middle | (y <= flat + WIRE_LIFT))


def shade_smooth(obj):
    for polygon in obj.data.polygons:
        polygon.use_smooth = True


def textured_material(name, albedo, normal=None, cutout=False):
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    nodes, links = material.node_tree.nodes, material.node_tree.links
    shader = nodes["Principled BSDF"]
    shader.inputs["Roughness"].default_value = ROUGHNESS
    shader.inputs["Metallic"].default_value = 0
    colour = nodes.new("ShaderNodeTexImage")
    colour.image = albedo
    links.new(colour.outputs["Color"], shader.inputs["Base Color"])
    if normal:
        relief = nodes.new("ShaderNodeTexImage")
        relief.image = normal
        converter = nodes.new("ShaderNodeNormalMap")
        links.new(relief.outputs["Color"], converter.inputs["Color"])
        links.new(converter.outputs["Normal"], shader.inputs["Normal"])
    if cutout:  # the exporter reads Alpha -> Round -> Principled as alphaMode MASK
        rounded = nodes.new("ShaderNodeMath")
        rounded.operation = "ROUND"
        links.new(colour.outputs["Alpha"], rounded.inputs[0])
        links.new(rounded.outputs["Value"], shader.inputs["Alpha"])
        material.use_backface_culling = False  # doubleSided
    return material


def load_image(path, colorspace, alpha=False):
    image = bpy.data.images.load(str(path))
    image.colorspace_settings.name = colorspace
    if alpha:
        image.alpha_mode = "CHANNEL_PACKED"  # keep the straight colour of low-coverage texels
    return image


def export_glb(obj, path):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_apply=False,
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_materials="EXPORT",
        export_image_format="AUTO",
        export_cameras=False,
        export_lights=False,
        export_animations=False,
        export_yup=True,
    )


# ---------------------------------------------------------------- finishing: KTX2, tangents, checks


def set_alpha_cutoff(path, cutoff=0.5):
    """Write the cutoff out: the exporter leaves the glTF default (0.5) implicit."""
    from compress_glb_textures import write_glb

    gltf, binary = read_glb(path)
    for material in gltf["materials"]:
        if material.get("alphaMode") == "MASK":
            material["alphaCutoff"] = cutoff
    write_glb(path, gltf, binary)


def check_box_extent(box):
    """Fail before anything is written unless the box spans exactly +-PANEL_SPAN / 2 in x (within 1e-4)."""
    x = to_panel(vertex_coordinates(box.data))[:, 0]
    if abs(x.min() + PANEL_SPAN / 2) > 1e-4 or abs(x.max() - PANEL_SPAN / 2) > 1e-4:
        raise SystemExit(f"box x extent is {x.min():.5f}..{x.max():.5f}, not +-{PANEL_SPAN / 2}")


def encode_depth(png, ktx2):
    subprocess.run(
        ["ctt", str(png), "-o", str(ktx2), "--quality", KTX2_QUALITY, "--zstd=19"] + ctt_arguments("depth"),
        check=True,
        capture_output=True,
    )


def validate(path):
    """glTF Transform validate, ignoring the three issues every KTX2-in-GLB reports."""
    result = subprocess.run(
        ["npx", "@gltf-transform/cli", "validate", "--ignore", VALIDATE_IGNORE, str(path)],
        capture_output=True,
        text=True,
    )
    text = result.stdout + result.stderr
    print(f"validate {path.name}: {'0 errors' if 'No errors found' in text else 'ERRORS'}")
    if result.returncode != 0 or "No errors found" not in text:
        raise SystemExit(text)


def finish_glbs(box_glb, wire_glb):
    set_alpha_cutoff(wire_glb)
    subprocess.run(
        ["python3", str(Path(__file__).with_name("compress_glb_textures.py")), str(box_glb), str(wire_glb)], check=True
    )
    subprocess.run(["python3", str(TANGENT_BAKER), str(box_glb), str(wire_glb)], check=True)
    for path in (box_glb, wire_glb):
        validate(path)


def finish_sandbags(glb):
    subprocess.run(["python3", str(Path(__file__).with_name("compress_glb_textures.py")), str(glb)], check=True)
    subprocess.run(["python3", str(TANGENT_BAKER), str(glb)], check=True)
    validate(glb)


# ---------------------------------------------------------------- main


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("scan", type=Path, help="the UHD cordon wall scan")
    parser.add_argument("models", type=Path, help="folder for the box and wire GLBs")
    parser.add_argument("textures", type=Path, help="folder for wall_depth.ktx2")
    parser.add_argument("--sources", type=Path, default=DEFAULT_SOURCES, help="folder for the intermediate PNGs")
    parser.add_argument(
        "--span-px", type=int, default=SPAN_PX, help="px along the panel (a smaller value to try things out)"
    )
    parser.add_argument(
        "--only", choices=["sandbags"], help="bake just this part, leaving the other outputs as they are"
    )
    return parser.parse_args(sys.argv[sys.argv.index("--") + 1 :])


def print_measurements(wall, shape, depth_range, spread, device):
    heights, counts = wall.top_peak
    print(f"cordon wall measurements (model units, panel frame; baked on {device}):")
    print(f"  HALF_THICKNESS = {wall.half_thickness:.4f}  (the ribs stand {wall.rib_height:.4f} proud of it)")
    print(
        f"  TOP = {wall.top:.4f}  (plateau vertices per {TOP_BIN} slice from y = {heights[0]:.3f}: {counts.tolist()})"
    )
    print(f"  DECK = {wall.deck:.4f}  (the middle of the top, below TOP: the tower stands on it)")
    print(f"  WIRE_FLOOR = {shape.floor:.4f}  WIRE_TOP = {shape.top:.4f}  WIRE_MIDDLE = {shape.middle:.4f}")
    print(f"  WIRE_HALF_WIDTH = {shape.half_width:.4f}")
    print(f"  DEPTH_RANGE = {depth_range:.4f}  (white; detrended long faces' depth at 50/90/99/99.9 %: {spread})")
    print(f"  box x extent = +-{PANEL_SPAN / 2}, y 0..{wall.top:.4f}, z +-{wall.half_thickness:.4f}")


def print_sandbag_measurements(block, row, size):
    points = np.array(row.positions)
    print("sandbag row measurements (model units, panel frame):")
    print(
        f"  block x {block.x0:.4f}..{block.x1:.4f} in the scan, {block.back:.4f}..{block.depth:.4f} out from the wall,"
        f" {block.top:.4f} high"
    )
    print(
        f"  x extent = {points[:, 0].min():.4f}..{points[:, 0].max():.4f}, max |z| = {np.abs(points[:, 2]).max():.4f}"
    )
    print(f"  top y = {points[:, 1].max():.4f}, {triangle_count(row.faces)} triangles, atlas {size[0]} x {size[1]} px")


def bake_wall_and_wire(scan, wall, args, device):
    """The box, its depth map and the wire strips: baked, written and finished."""
    shape = wire_shape(wall)

    size, islands = box_layout(wall, args.span_px)
    box = mesh_object("site-cordon-wall-box", box_geometry(wall, islands))
    check_box_extent(box)
    albedo, normal, depth = bake_box(concrete_source(scan, wall), box, size, islands, wall)
    long_faces = np.zeros(depth.shape, bool)
    for face in ("front", "back", "top"):  # the ends are mostly hidden and would set the scale
        long_faces[islands[face].pixels(0)] = True
    depth_map, depth_range = depth_image(depth, long_faces)
    spread = " / ".join(f"{v:.4f}" for v in np.percentile(depth[long_faces], [50, 90, 99, 99.9]))

    wire_size, wire_islands = wire_layout(shape, args.span_px)
    wire = mesh_object("site-cordon-wall-wire", wire_geometry(shape, wire_islands))
    colour, coverage = bake_wire(wire_source(scan, wall), wire, wire_size, shape, wire_islands)
    rgba = np.dstack([srgb_encode(unpremultiply(colour[..., :3], coverage[..., 0])), coverage[..., 0]])

    pngs = {
        "box_albedo": srgb_encode(albedo),
        "box_normal": normal,
        "wall_depth": np.dstack([depth_map] * 3),
        "wire_albedo": rgba,
    }
    for name, pixels in pngs.items():
        write_png(args.sources / f"{name}.png", pixels)

    box_glb, wire_glb = args.models / "site-cordon-wall-box.glb", args.models / "site-cordon-wall-wire.glb"
    box.data.materials.clear()
    box.data.materials.append(
        textured_material(
            "site-cordon-wall-box",
            load_image(args.sources / "box_albedo.png", "sRGB"),
            load_image(args.sources / "box_normal.png", "Non-Color"),
        )
    )
    wire.data.materials.clear()
    wire.data.materials.append(
        textured_material(
            "site-cordon-wall-wire", load_image(args.sources / "wire_albedo.png", "sRGB", alpha=True), cutout=True
        )
    )
    export_glb(box, box_glb)
    export_glb(wire, wire_glb)
    encode_depth(args.sources / "wall_depth.png", args.textures / "wall_depth.ktx2")
    finish_glbs(box_glb, wire_glb)
    print_measurements(wall, shape, depth_range, spread, device)


def bake_sandbags(scan, wall, args):
    """The row of sandbags along the wall's foot: one module baked from the scan's block, repeated and mirrored."""
    block = measure_bag_block(to_panel(vertex_coordinates(scan.data)), warm_vertices(scan), wall.half_thickness)
    module, size = bag_module(block, wall.half_thickness)
    module_object = mesh_object("sandbags-module", module)
    shade_smooth(module_object)
    source = bag_source(scan, block, wall.half_thickness)
    albedo, normal = bake_bags(source, module_object, size, bag_front_rows(block, size))
    write_png(args.sources / "sandbags_albedo.png", srgb_encode(albedo))
    write_png(args.sources / "sandbags_normal.png", normal)

    row = bag_row(module)
    check_row_extent(row)
    sandbags = mesh_object("site-cordon-wall-sandbags", row)
    shade_smooth(sandbags)
    material = textured_material(
        "site-cordon-wall-sandbags",
        load_image(args.sources / "sandbags_albedo.png", "sRGB"),
        load_image(args.sources / "sandbags_normal.png", "Non-Color"),
    )
    material.use_backface_culling = True  # every face looks out; none looks at the wall or the ground
    sandbags.data.materials.clear()
    sandbags.data.materials.append(material)
    glb = args.models / "site-cordon-wall-sandbags.glb"
    export_glb(sandbags, glb)
    finish_sandbags(glb)
    print_sandbag_measurements(block, row, size)


def main():
    args = parse_args()
    for folder in (args.models, args.textures, args.sources):
        folder.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = SAMPLES
    scene.cycles.use_denoising = False
    device = use_gpu(scene)

    scan = import_glb(args.scan)
    wall = stand_scan_on_panel_frame(scan)
    if args.only != "sandbags":
        bake_wall_and_wire(scan, wall, args, device)
    bake_sandbags(scan, wall, args)


if __name__ == "__main__" and bpy is not None:
    main()
