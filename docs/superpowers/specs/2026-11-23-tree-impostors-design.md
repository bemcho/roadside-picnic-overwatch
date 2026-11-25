# Trees from their UHD scans: a near tier and impostors

> **Superseded on 2026-11-23.** The user chose geometry tiers instead ("I choose Geometry tiers"):
> `2026-11-23-tree-geometry-tiers-design.md`. Kept for the record. Do not build from it.

**Status:** approach chosen by the user on 2026-11-23 ("A for trees"). The design was approved the same day ("yes approve").
The same day the user cut the trees to three kinds: "from the trees lets just leave 3 kinds - birch,dead, small".
Phase 1's Task 11 removes the broadleaf and the tall conifer, so this design covers the birch, the dead tree and
the small tree.
It is built after the asphalt and cordon wall plan
(`docs/superpowers/plans/2026-11-23-asphalt-wall-textures.md`).

**It amends** `docs/superpowers/specs/2026-11-23-vegetation-lod-design.md`, "Tree tiers". For trees, the near,
mid, far and speck chain becomes a near tier plus impostors. That spec still covers plants, rocks, `--detail`,
the bake tool's generic part and the RAM/GPU levels. Its goals, anti-goals and constraints bind here too.

## Intent

The user, in order:
- "lets do the same with the trees if make sense - uhd version we extract textures";
- "about trees as addition to your solution" (the textures-from-UHD work on the asphalt and the wall);
- "A for trees".

Earlier, from the vegetation spec:
- "i don't like low quality trees, better to have 3 uhd than 1000 bad";
- "all vegetation disappears after certain zoom out";
- trees that look as good as Red Alert 2's at the tactical zoom.

Every tree should look like its UHD scan at every zoom, for little cost. A box bake doesn't fit a tree (no
surface to wrap), so the UHD look is baked into pictures. Each tree's picture set holds it as seen from many
directions above the ground. Far trees draw one of those pictures on a quad, lit by baked normals.

## What is there today (from the code)

**Trees.**
- About 10,000 trees in three species (after phase 1's Task 11) (`canopy::Species`), one map-wide `CanopyVisual` entity each, spawned at load.
- Each draws its ~500-triangle crown-card model (`trees/medium/`) to `draw_distance` (125 m and the like) plus
  `STRATEGY_REACH` (75 m).
- So no tree shows at the overview. At the tactical zoom (~90 m) only crown cards show.

**UHD scans and the close tier.**
- The three UHD scans (`trees/uhd/`, ~1.9M triangles each) are pinned on the GPU.
- The scan shows as a child within 8 m (`UHD_RANGE`, fading to 12 m).
- Blender-readable sources are in the workspace, `models/trees/uhd/` (62–69 MB each). The game's copies have KTX2
  textures, which Blender can't read.

**Shadows, sway, falling, collision and culling.**
- A tree casts shadows within 48–55 m of the camera (`SHADOW_ENTER`/`SHADOW_EXIT`). At the tactical zoom the
  camera is farther than that from every tree, so no tree casts a shadow there.
- Sway rotates the `CanopyVisual` transform.
- A falling tree keeps its entity: `FallingTree` replaces `CanopyVisual` and rotates it down.
- Trunk pillars and weights live on the per-tile `StandingTree`.
- `cull_shrouded` drops entities whose `Aabb` lies wholly under the shroud.

**Depth.** The ground fog pass reads the main pass's depth buffer.

## Goals

1. Trees show at every zoom, out to the overview, and look like their UHD scan from every side and from above.
2. Close up nothing changes: the UHD scan within 12 m.
3. Between 12 m and the impostor start, a near tier decimated from the scan, with the scan's normals baked onto it, replaces the
   crown cards. Its swap with the UHD scan only adds detail and never looks like a different tree.
4. Past each species' impostor start (about 55 m, from the resolution rule below), an impostor:
   - two triangles;
   - lit per pixel by the sun and moon through baked normals;
   - fog, the fog of war and the tier crossfade work as on every other model.
5. Frames stay smooth:
   - no frame over 33 ms in the traced fpv walk through the densest wood;
   - average frame time there no worse than today;
   - the overview within 3 ms of today (vegetation spec goals 4 and 7).
6. Everything is baked once, offline. The runtime only picks and blends pictures.

## Non-goals

- **Shadows from impostors.** Trees past ~50 m cast none, as today. A follow-up could render the impostor toward
  the sun in the shadow pass, or measure the shadow limit from the view focus instead of the camera, if the
  tactical view needs tree shadows.
- Impostors for plants, rocks or sites.
- Changing where trees grow. `WOOD_DENSITY` may still be thinned if the near tier can't meet goal 5.
- A per-pixel depth write (pixel-exact ground intersection). The quad's own depth is enough past 50 m.

## Anti-goals

- Trees that read as flat cards or billboards from the fpv or tactical camera: no sliding, no ghosting while the
  camera orbits, no picture popping from one frame to the next.
- A visible shape jump at 12 m or 50 m.
- Trees vanishing while they fall, or standing still on the far side of the map while their trunk is gone.
- New stutter from spawning near tiers.

## Approaches (the user chose A)

| | What | Trade-off |
|---|---|---|
| **A (chosen)** | Octahedral impostors baked from the UHD scans past ~50 m; a near tier from 12 to 50 m; the UHD scan within 12 m. | New bake and shader; the best look at the tactical zoom and the overview, at about 2 triangles per far tree. |
| B | Bake the UHD colour and normals onto today's crown-card meshes. | Small change, but the outline stays the 500-triangle cards. |
| C | Impostors for everything past 8 m. | Cheapest, but impostors close to the fpv camera read flat (anti-goal). |

## Design

### Tiers (amends the vegetation spec's tree table)

| Tier | What | Shows (camera distance) |
|---|---|---|
| UHD | the scan, unchanged, pinned on the GPU | 0 – 12 m (as today) |
| near | decimated from the scan (~60k triangles to start, the vegetation spec's figure), with the scan's colour and roughness and a normal map baked from the scan's geometry, at 2048 px. The scans are opaque and have no normal map of their own, so the near tier is opaque. | 8–12 m in, out where its impostor starts |
| impostor | one quad, picture atlases baked from the scan | from each species' start (the resolution rule; about 55 m), no end |

- **Ranges.** They are `VisibilityRange` margins, so the tiers crossfade with Bevy's dither. All three tiers
  measure distance the same way, to the tree's middle as the UHD tier does today. Otherwise the impostor's sphere
  bounds would shift the handoff by up to ~11 m on narrow species.
- **The resolution rule (my call).** An impostor's picture is never magnified more than 1.5× on a 1920×1080
  screen. This holds at the camera's 45° vertical field of view, both at the impostor's start and at the 90 m
  tactical view.
  - That sets each species' frame size and start distance.
  - Example: a 15 m birch in a 256 px frame would be magnified about 1.7× at 50 m, so it starts at about 56 m.
    Larger frames would let it start closer.
  - Small trees can use 128 px frames.
- **The near tier's triangle count** is the lowest at which the 12 m swap from the UHD scan shows no change of
  shape. The user judges this at the checkpoint below.
- **When goal 5 and the look conflict,** this order applies:
  1. Pull the impostor start in, but never closer than the resolution rule allows.
  2. Thin `WOOD_DENSITY` (the user's call in the vegetation spec: "thin `WOOD_DENSITY` rather than cheapen the
     near tiers").
  3. Only then ask the user.

  The near tier is never cheapened below the look approved at the checkpoint.
- **Files.**
  - The near tier replaces the crown cards in `trees/medium/`, after the old files are moved into the
    unused-assets archive zip.
  - The impostor atlases go to `textures/trees/`.

### The bake

**Near tier.**
- The vegetation spec's generic bake tool, `media/glb-lod-bake` in agentic-tooling. It decimates a copy of the
  scan, keeps its UVs and bakes the scan's normals onto it, in the scan's exact frame.
- It is built here first; phase 2 reuses it for plants and rocks.

**Impostors.** A second generic tool, `media/glb-impostor-bake` (headless Blender 4.5, Cycles, the `blender`
skill's binary).
- **Framing.** It renders the scan orthographically from the directions of a hemi-octahedral grid:
  - the upper hemisphere mapped onto a square of 12 × 12 frames;
  - one scale for all of a species' frames, so the frames blend;
  - frames sized per species by the resolution rule. They may be non-square (e.g. 256 × 384 for a narrow crown)
    when every view of the tree fits a narrower rectangle than its bounding sphere.
- **Atlases** (two per species):
  - colour and coverage (sRGB, alpha = coverage), dilated into the empty pixels so mips don't fringe dark;
  - normal and depth (linear): the tree's normal in its own model frame in RGB, and in alpha the depth along the
    frame's view, relative to the sphere.
- **Outputs.**
  - The atlases are KTX2 with full mips, encoded with the `tools/compress_glb_textures.py` presets (sRGB BC7 and
    linear BC7).
  - The sphere (centre height and radius, model units) and the grid size go into a generated Rust file beside
    the pillar files.

**One command.** A project script, `tools/build_tree_tiers.py`, runs both tools for the three species from the
workspace sources.

**One mapping.** The bake and the shader share the hemi-octahedral mapping. Both sides are tested against the
same table of known direction ↔ frame pairs:
- the bake tool's Python tests;
- a Rust mirror of the shader's function. WGSL can't run under `cargo test`.

A debug mode tints each frame by its index, and a screenshot from known directions confirms that the shader picks
the frames the table says. *(My call.)*

**The look checkpoint (my call).**
1. Build the two hardest species first, running in the game with their near tiers and impostors:
   - the birch: the tallest left, so the resolution rule bites hardest, with a leafy crown;
   - the dead tree: thin bare branches, the hardest to keep from shimmering.
2. Stop the run for the user. Send screenshots at fpv (5, 15, 30, 45 m and its impostor start), at the tactical
   view while orbiting, and at the overview, against today's crown cards and the UHD scan.
3. Only after the user's go: bake the small tree and commit any near-tier or impostor asset to Git LFS.

### The impostor at runtime

- **Material.**
  - A material extending `StandardMaterial`, with its own vertex and fragment shader (`shaders/impostor.wesl`).
  - The fragment builds Bevy's PBR input from the atlases, so the sun, moon, exposure, distance fog and the
    visibility-range dither apply as on every model.
  - Alpha mask; it writes depth in the main pass, which the ground fog reads.
- **Quad.**
  - One shared quad. The vertex shader turns it to face the camera in the tree's own model frame: it faces the
    view direction, its up follows the tree's up.
  - So the instance's yaw, scale, sway and fall all carry through the model matrix.
- **Fragment.**
  - The fragment finds the three frames nearest the model-frame view direction and blends them by weight.
  - Each frame's lookup is shifted by its baked depth, the usual octahedral parallax correction, which removes
    most of the ghosting between frames.
  - A view from below the tree's horizon (a falling tree) is clamped to the horizon frames.
- **Batching, bounds and culling.**
  - One material and one mesh per species, so all trees of a species draw as one instanced batch.
  - Each impostor has an explicit `Aabb` of its sphere plus `NoAutoAabb`, so frustum culling and `cull_shrouded`
    see the tree, not the flat quad.

### Entities

**Map-wide.** One entity per tree, as today:
- the `CanopyVisual` root now draws the impostor;
- it keeps sway and falling (`FallingTree` rotates the same entity);
- it never casts shadows.

**The near tier.**
- A child spawned by distance to the camera, the same way `stream_close_trees` spawns UHD children today.
- It loads a little before its species' impostor start and drops a little past it, at most a few dozen per frame,
  nearest first.
- Until a tree's near child exists, its impostor stays fully visible, as plants keep their model until the scan
  arrives. A falling tree with no near child keeps its impostor.
- *(Precedent: `plants.rs` already extends `StandardMaterial` with a custom vertex shader and the visibility-range
  dither.)*
- It sways and falls with its parent.
- The 48–55 m shadow toggle moves from the root to this child.
- *(My call: the tiers show by camera distance. The 3×3 scenery tiles follow the anchors, which sit about 90 m
  from the camera at the tactical zoom, so tile streaming would load near tiers that never show. This amends the
  vegetation spec's "streamed with the tiles" for trees.)*

**The UHD child** is unchanged.

**Removed.** Trees stop using `detail_range` and `STRATEGY_REACH`. Phase 2's `--detail` later scales the tree
bands with the rest.

### Cost

**VRAM.** Always resident, outside `GpuScans`. These are estimates under the resolution rule:
- impostor atlases, two per species with mips, ~56 MB for three:
  - birch and dead 256 px, ~25 MB each;
  - small 128 px, ~6 MB;
- near tiers: ~16 MB of maps and ~4 MB of geometry per species, ~60 MB;
- so ~116 MB, against the ~8 MB the three crown-card models free.

Phase 1's Task 11 already frees the two dropped species' pinned UHD scans.

On the 4 GB T1200 that shrinks the scan budget by about the same, through the live VRAM reading. If that's too
much: first drop the near maps to 1024 px, then use 10 × 10 frames. Smaller frames would break the resolution rule;
start the impostors farther out instead.

**Repository.** The new KTX2 files add roughly 70–120 MB to Git LFS (an estimate). Report the total.

## Left to the builder

- Exact frame sizes and start distances within the resolution rule.
- The frame count: 12 × 12 to start; 16 × 16 if orbiting shows frame steps, within the VRAM note above.
- How many near tiers spawn per frame, and the load and drop distances.
- Whether the dead tree needs a near tier, or its impostor holds up from 12 m.
- The atlas packing details, the dilation, and the depth encoding.
- `CLAUDE.md` and `arch-readme.md` updates (the Scenery bullet, asset tiers, the bake tools), and moving the crown
  cards into the archive zip with their `MANIFEST.txt` sums.

## Verification

**Tests.**
- The tiers hand over without a gap (UHD out = near in, near out = impostor in).
- Near tiers spawn within the load distance, drop past the drop distance, stay under the per-frame cap and come
  nearest first.
- An impostor carries its sphere `Aabb` and `NoAutoAabb`.
- A falling tree keeps its impostor and its near child.
- The hemi-octahedral mapping table, on the bake side and on the shader side.

**Traced runs.** Before and after, on the release trace build, with `XDG_DATA_HOME` in a scratch folder: the fpv
walk through the densest wood, a tactical roam, and a zoom out to the overview. Record frame max/p99/average and
peak VRAM.

**The goal 5 gate checks both CPU and GPU.** Xephyr caps the frame rate, so frame times alone gate only the CPU.
- **CPU:** the trace spans (`bevy-trace-summary`).
- **GPU:** the summed opaque, alpha-mask and shadow pass times from a temporary `RenderDiagnosticsPlugin`
  (`elapsed_gpu`), removed afterwards.
- **Baseline:** taken right before the first change.
- **Passing (my call):**
  - in the fpv wood walk, CPU and GPU time each within the baseline + 5% (run-to-run noise), and no frame over
    33 ms;
  - at the overview, GPU time within the baseline + 3 ms.

**Screenshots** for the user to judge, before and after:
- fpv with trees at about 5, 15, 30, 45 and 60 m;
- the tactical zoom (90 m), including a short orbit (right drag) recorded as a frame sequence;
- the overview.
