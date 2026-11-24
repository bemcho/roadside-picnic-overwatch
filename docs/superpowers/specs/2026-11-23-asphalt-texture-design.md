# Asphalt and cordon wall from textures, not repeated scans

**Status:** approved by the user on 2026-11-23:
- asphalt, option A;
- the cordon wall, option A;
- relief mapping on both, so they look 3D.

It is built after phase 1 (Task 9 and the VRAM follow-up), so it does not disturb
their measurements.

## Intent

Two kinds of scenery repeat one scanned model over kilometres. A texture that
looks 3D draws them for much less, at every zoom.

**The old asphalt roads** (`src/world/old_road.rs`):
- A scanned road kit paves every `Surface::Asphalt` leg: 18 slab and curb pieces, copied about every 60 m.
- Each piece is spawned twice: the full scan within 40 m and a simplified copy out to 900 m.
- That is roughly 1,700 mesh entities and 46 MB of models, always loaded (an estimate from the routes).

**The cordon and Perimeter walls** (`src/world/cordon.rs`):
- About 2,300 copies of one scanned panel: 7.4 m long, 2,875 triangles, colour texture only.
- Each copy's vertices are copied into a chunk mesh when its tile streams in.
- The UHD scan of the same wall has the detail (blocks, joints, cracks, razor wire) in 1.8M triangles of geometry.

The game is seen mostly from the tactical camera. A textured surface with
normal and height maps, relief-mapped so its recesses shift with the view,
reads like the geometry from there.

## Goals

- **Roads.**
  - Remove the road kit and paint the old asphalt in the terrain shader: weathered grain, plus cracks like the reference photo.
  - The cracks must not visibly repeat along kilometres of road.
  - New Harmont's smooth streets (`Surface::Street`, `town::paved`) stay smooth.
- **Wall.**
  - Each panel becomes a simple box.
  - Its colour, normal and height maps are baked from the UHD scan.
  - The razor wire keeps its outline.
  - Collision, streaming, the crawl hole, the gates and the checkpoints are unchanged.
- **Both look 3D up close:**
  - relief mapping on the asphalt (grain and crack depth) and on the wall (blocks and joints);
  - a cheaper path farther out.

## Non-goals

- Raised curbs and heaved slab edges at the closest zoom. The road is flat apart from relief.
- The scan's ragged outline up close. The wall's outline is a box, with the wire strips on top.
- The sandbags and rubble at the foot of today's panel. They go; props can dress the foot later.
- Changes to the road network, grading, vehicle speeds, tooltips or wall layout.

## Approaches

| | Roads | Trade-off |
|---|---|---|
| **A (chosen)** | Remove the kit. Paint asphalt in `terrain.wesl` from a tiling grain set, with cracks scattered separately. | Cheapest, and works at every zoom. Flat outline close up. |
| B | Texture far away, the 3D scan within 40 m. | Keeps close relief; keeps 36 MB of scans and their entities. |
| C | Keep the kit. | No savings. |

| | Wall | Trade-off |
|---|---|---|
| **A (chosen)** | A box per panel with maps baked from the UHD scan, relief-mapped. The razor wire is on see-through strips. | ~2,875 → ~50 triangles per panel, and trivial chunk building. Box outline up close. |
| B | A, but the scanned panel within 40 m. | True outline up close; keeps its triangles near the view. |
| C | Keep the panel; add a normal map baked from the UHD scan. | Smallest change; no savings, less depth. |

## Design: roads

1. **Source.**
   - The reference is `image_5b974df1.jpg` (1408×768, an oblique photo, sharp in its lower half).
   - It is copied into the assets workspace. Like the other terrain sources, it is not checked in.
   - A straight-down, seamless 4K version from the user can replace it later; the tool re-runs.
2. **Maps.** A new tool, `tools/prepare_asphalt_textures.py`, reuses `tileable()` from `tools/prepare_terrain_textures.py`. *(Builder's call: the asphalt source is one perspective photo with its own steps, so it gets its own script.)*
   - **Grain set:**
     - perspective-correct the sharp foreground (a four-point transform);
     - flatten its lighting, take a crack-free patch and make it tileable;
     - derive the normal and height from the luminance detail, and the roughness and AO from luminance and cavity;
     - pack them like the other terrain maps: `asphalt_albedo.png`, `asphalt_normal.png`, and `asphalt_surface.png` (R AO, G roughness, B height).
   - **Crack decal:** cut the photo's crack into one RGBA map, `asphalt_crack.png`, with its depth and the crack mask.
3. **Mask.**
   - `RoadSample::surface` writes the old-asphalt share into the road mask's unused alpha channel, for `Surface::Asphalt` only.
   - Streets and `town::paved` texels keep alpha 0, so New Harmont stays smooth.
4. **Shader** (`terrain.wesl`).
   - Where the alpha is set, sample the grain set in world XZ, about 3 m per tile, over today's tar.
   - **Cracks:** in world cells of about 9 m, a hash picks whether a cell has a crack (about 40%), plus its offset and rotation. Each crack fades toward the road's edge and fades out at a distance.
   - **Relief:** close up, the grain height minus the crack depth is relief-mapped along the view ray. Farther out, the ground's shallow parallax is used.
   - Streets keep today's smooth tar.
5. **Removal.**
   - Delete `old_road.rs` and its registrations.
   - Move `assets/models/road/` into the unused-assets archive zip.
   - Update the "Old roads" docs.

## Design: wall

1. **Bake** (headless Blender, `tools/bake_cordon_wall.py`).
   - **Source:** the UHD scan `models/sites/uhd/site-cordon-wall.glb` from the assets workspace (also in the archive zip).
   - **Box:** fitted to the wall's body and posed in today's panel frame (+X along the wall, 1 unit long, grounded at y = 0), so the layout and colliders stay as they are.
   - **Bake:** colour, tangent-space normal and height are baked onto the box's front, back, top and end faces with a short ray distance, so the watchtower, the wire and the foot's sandbags stay out of it. The atlas has about 2048 px along the panel.
   - **Wire:** two see-through strips along the top are cut from a tower-free stretch and made to tile along the wall:
     - an upright one with the wire's side outline;
     - a flat one at the coils' mid-height with its look from above.
   - **Output:** finished like every runtime GLB (KTX2 textures, baked tangents): the box and the wire strips as GLBs, plus the height map.
2. **Runtime.**
   - The chunk mesh is built from the box and the strips instead of the scan. It follows the ground the same way: the box is split about every metre along the panel, and the water-foundation row is kept.
   - The tangents are carried and sheared with the panel.
   - The wall material gets the height map with Bevy's relief parallax; the wire uses an alpha mask.
   - Collision is unchanged.
3. **Removal.** The old medium panel moves to the archive zip. Update the "Cordons" docs.

## Left to the builder

- **Roads:** the crop and transform corners, the tile size and crack density (tune by eye), and the crack decal's packing.
- **Wall:** the box's measured thickness and height, the atlas layout, the bake ray distance, the wire strips' size, and the relief strength and step counts (tune by eye).

## Verification

- `cargo test`, clippy and fmt pass.
- Mask test: the alpha is set on the highway and is zero on New Harmont's streets and plaza.
- Wall test: built chunks keep their tangents along the panel and perpendicular to the normals.
- Screenshots at 6 m, 40 m and 250 m zoom:
  - on the highway and on a New Harmont street;
  - along the Perimeter, from the side and from the tactical view.

  Compare them with the current look at the same poses.
- Traced runs (a highway drive and a roam along the Perimeter wall), measured before and after: frame times, chunk-streaming spans and VRAM. Expect fewer entities and triangles, and no frame over 33 ms.
