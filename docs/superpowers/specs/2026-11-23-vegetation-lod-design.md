# UHD everywhere from RAM; vegetation and rocks at every zoom

Date: 2026-11-23. Status: approved by the user on 2026-11-23.

The file name keeps its first topic (vegetation LOD); the scope has grown to
the UHD scans of every kind and how they are kept in memory.

## Intent

Trees, bushes, weeds and rocks should look better and cost fewer frames, so
walking and driving stay smooth on the target card (RTX 2060, 6 GB), with room to
look richer on stronger GPUs.

The user's own words:

- "all vegetation disappears after certain zoom out": today it only shows near
  **fpv**, the user's name for the closest zoom (6 m, over the shoulder).
- "i don't like low quality trees, better to have 3 uhd than 1000 bad":
  quality beats quantity.
- "fix what see doable": the user handed over the how.
- "i want to see best quality close up": close up, every kind shows its native
  UHD scan. The lighter tiers exist only for distance.
- "its ok to use as much ram as possible".
- "add uhd where missing keep everything in ram, copy to gpu, do not move
  forth and back. we aim minimal disk io for assets especially": every kind
  with a UHD source gets a close-up UHD tier. Each asset file is read from
  disk once per run and stays in RAM. The GPU gets copies. Nothing goes back
  to disk or is read from it again.

## What is there today (measured from the code and assets)

| Kind | Tiers drawn | Problems |
|---|---|---|
| Trees (5 species, `canopy.rs`) | 500-tri alpha-cutout crown cards to 200–410 m; the 1.9M-tri UHD scan swapped in within 16 m of the camera, loaded from disk then (70–98 MB) and dropped 5 m further out | Cards look flat and poor, especially from above. Nothing past 410 m, so the map shows bare at zoom out. Every close swap reloads 70–98 MB; the 2026-11-23 trace did not catch it as a hitch, but the load repeats each time. |
| Plants (15 models, `plants.rs`) | `medium/` 2.6k–21.6k tris to 105 m (30 m + 75 m reach); `low/` 400–600 tris, no texture of its own, to 141 m; the nearest plant within 8 m of the stalker swaps to a 70–98 MB UHD scan | 10 of 15 have only a colour map, so they light flat. The UHD swap pops (different shape) and reloads its scan each time. Plants exist only in the 3×3 tiles around the stalker and the view focus. |
| Rocks (7 sizes, `rocks.rs`) | `medium/` 10k tris, colour map only, to 136–256 m; `low/` 600 tris after that, to 625–2125 m | Flat lighting: no normal map, though the workspace holds 1.45M-tri UHD sources. |
| Everything else | Vehicles, animals, birds, mutants, guards, hangar shell and props, artifact previews, vegetation clusters: one `medium/` model at every distance, preloaded at startup and kept | No close-up detail, though the workspace holds UHD sources for all of them except the guards and the two Harmont traffic cars (see "UHD for the other kinds"). |

Disk reads during play today: the tree close-ups (`stream_close_trees`), the
nearest plant's scan (`stream_full_plants`), the site and landmark scans
(`sites.rs`, within 150 m of an anchor) and the artifact scenes within 8 m are
loaded from disk when needed and dropped afterwards, so each comes back from
disk next time. Everything else loads once at startup into `PreloadedAssets`
with the default usages (`MAIN_WORLD | RENDER_WORLD`), so it already keeps a
RAM copy and a GPU copy for the whole run.

Since `374be19` (smooth streaming, 2026-11-23):
- Plants carry precomputed bounds (`Aabb` + `NoAutoAabb`) and at most 150
  spawn per frame.
- Scenery and ground tiles fill in one per frame, nearest first, and leaving
  tiles unload 300 children per frame.

On the RTX 2060 trace (under Xephyr, so CPU only), the worst walking frame
fell from 180 to 23 ms and the worst driving frame from 160 to 41 ms. The
41 ms frame is a UHD site scan arriving (12 ms of transform propagation and
31 ms of render extract), not vegetation. The driving frame averages 10.6 ms
of main-app CPU, p95 15 ms. Visibility checks (ranges, culling, light
visibility) take a few ms of that.

So the tile-crossing stutter is solved. This design is about the look, the
steady cost and the zoomed-out view, and must not bring hitches back.

Shared cause: `detail_range` adds `STRATEGY_REACH` (75 m) to every range. The
ranges are measured from the camera, so in fpv full detail draws 75 m farther
than needed. Once zoomed out, the ranges still run out and things vanish.

## Goals

1. Trees and rocks show at every zoom, out to the whole-map overview. Bushes
   show until about 250 m of zoom (while their tiles are streamed); small weeds
   until about 150 m.
2. Best quality close up (the user's call). In fpv, the trees, the nearest
   plants and the rocks around the stalker show their UHD scans with native
   textures. Every lighter tier is baked from the same scan, so a tier change
   crossfades between two versions of one shape and never looks like a
   different plant. Never the old crown cards. The user judges this by eye,
   from before/after screenshots at fpv with trees at about 5, 15, 30 and
   45 m. If the near tier shows, raise its budget (up to ~150k triangles) or
   the UHD range (up to 15 m). *(Assistant's call.)*
3. Rocks and plants get the scan's surface detail (normal maps) on their light
   tiers.
4. Smooth walking and driving: vegetation and UHD swaps cause no frame over
   33 ms in traced fpv walks and drives through the densest wood. A scan that
   goes to the GPU during play never holds up a frame and never pops in late
   at close range.
5. Minimal disk reads (the user's call). On native builds, no asset file is
   read from disk after the entry curtain lifts. Each file is read once per
   run, at load. Checked with `strace -f -e trace=openat` during the traced
   runs: no path under `assets/` after the curtain.
6. UHD where missing (the user's call): every kind with a UHD source shows it
   close up (see "UHD for the other kinds").
7. Average frame time at fpv in the densest wood is no worse than today, and
   ideally better. The full-map overview draws trees it does not draw today, so
   it may cost at most 3 ms more per frame than the measured baseline. To meet
   that, thin the distant trees first (e.g. the speck tier draws every other
   tree), then fall back to C. *(Assistant's call.)*
8. Two best-quality flags (the user's call), independent of each other:
   - `--vram <GB>` (optional): a cap on the VRAM the game may use. Without
     it, the game uses as much as the driver says this process may (see "RAM
     and GPU").
   - `--detail <x>` (default 1.0): the scale on every vegetation and rock tier
     distance, so stronger GPUs draw the higher tiers farther out.

   The README documents both flags (see "README").

   *Amended 2026-11-23 after the game ran out of memory on the user's T1200
   laptop (4 GB). The user: "still try to use as much as possible vram without
   out of memory on other hosts".* The first version said wgpu cannot report
   VRAM. That was wrong. wgpu 30 enables Vulkan's `VK_EXT_memory_budget`
   whenever the driver offers it, and the game can read the live budget and
   usage through `as_hal` and `ash`. So the budget is detected on every host,
   and `--vram` is only a cap. Hosts that cannot report it (other backends,
   the web) use 6 GB, or `--vram`.

## Non-goals

- The marsh props, reeds and the ground stone batches stay as they are.
- No new models from Tripo (no credits spent). Every new tier comes from scans
  already in the assets workspace or the archive zip. So the kinds with no UHD
  source keep their `medium/` model close up: the guards (`soldiers/`), the
  Harmont traffic sedan and street sweeper, and henbane and ragweed (their
  workspace scans have no textures).
- The Perimeter and edge wall panel keeps its medium model: it repeats
  thousands of times (see `CLAUDE.md`, Cordons).
- No graphics settings menu: the detail setting is chosen at startup.
- Placement rules (where things grow) stay as they are, except that the dense
  woods may be thinned (see Constraints).
- The world keeps its size (the user's call). The strips beyond the Harmont
  map stay as they are.
- The site spawn spike: a site's UHD scene still spawns its entities in one
  frame (12 ms of transform propagation in the garage trace). RAM does not
  fix that; it is a separate change.

## Anti-goals (a failure even if it runs)

- Trees that read as flat cards or billboards from the fpv or tactical camera.
- A visible shape jump when a tree or plant changes tier at a normal walking
  distance. A soft crossfade is fine; a swap that looks like a different plant
  is not.
- New stutter: mid-game loads, or many meshes arriving in one frame.
- Thrashing: a scan leaving the GPU and coming back again and again while the
  stalker moves about one area.
- The overview losing readability: the fog of war, sites and roads must stay
  clear.

## Constraints

- Bevy 0.20, no new crates. Keep `wasm32` compatibility: no threads in game code.
- Every runtime GLB is finished with `tools/compress_glb_textures.py` (KTX2,
  BC7/BC5). Blender cannot read KTX2, so bakes start from the workspace sources
  (`~/Projects/agentic/RoadSide-Picnic/models/<kind>/uhd/`) or from the
  archive zip.
- VRAM: the UHD site scans already strain 6 GB. Each UHD tree is ~75 MB of
  geometry (~1.1M vertices with tangents, ~1.95M triangles of u32 indices)
  plus three native 4K BC7 maps with mips (~64 MB), so the five come to about
  700 MB. All UHD scans together (~7.5 GB of files) need roughly 9–10 GB of
  VRAM, so on the 6 GB card only part of them can be on the GPU at once (see
  "RAM and GPU"). Close-up quality wins (the user's call): if VRAM runs short
  in a traced run (watch `nvidia-smi`), cut from the distance tiers and the
  site stand-ins first, and the UHD scans last. *(Assistant's call.)*
- Repo size: the assets are on Git LFS. The new UHD tiers (vehicles,
  creatures, props, hangar shell, vegetation clusters, rocks, artifacts) are
  ~3 GB of workspace sources and grow to about 4 GB as KTX2 files (the UHD
  trees grew 1.36×, from 314 to 428 MB). The vegetation tiers add about
  120 MB more. Every fresh clone
  downloads them, and they count against the LFS storage and bandwidth quota.
  Report the total, and check the LFS plan before pushing. The retired
  crown-card mediums leave `assets/` for the archive zip, per the asset-tier
  convention.
- RAM: the RAM level holds ~7.5 GB of UHD files, and the preloaded mid tiers
  keep their decoded copies (~1 GB), so the game uses about 9 GB of RAM more
  than the files it shows. Here 14 of 30 GB are free. *(Estimate; measure.)*
- Anything that reads a scan's vertices on the CPU must do it before the scan
  goes to the GPU, because a render-only asset keeps no vertex data in the
  main world. Today that is the creatures' skin weights (`wildlife/rig.rs`).
  Bounds are safe: `Mesh::take_gpu_data` keeps the mesh's final `Aabb`
  (`bevy_mesh` `mesh.rs`), so `calculate_bounds` and the plants'
  precomputed bounds still work.
- Collision is unchanged: trunk pillars, rock pillars and the tree weights stay
  as they are. Every tier of a model stays in the same frame as its scan, so the
  pillars still fit.
- Tree sway, tree falling (`tree_fall.rs`), crushing, the shadow limit (48 m)
  and fog-of-war culling (`cull_shrouded`) must keep working on every tier.
- Quality beats quantity (the user's call). If a tree budget can't be met in
  the dense woods, thin `WOOD_DENSITY` rather than cheapen the near tiers.
- Entity count. Trees are spawned map-wide (~10k `CanopyVisual` entities),
  and visibility checks run over every entity each frame, for the camera and
  each shadow view. Map-wide, a tree keeps at most two visual entities (far
  and speck). The UHD, near and mid tiers stream with the 3×3 scenery tiles
  around the anchors: inside the one-tile-per-frame spawn and through
  `unload_stale_tiles`. Sway, falling and crushing must act on the streamed
  tiers too. *(Assistant's call.)*
- GPU buffers. `mesh_allocator_settings` (`world/mod.rs`) gives a mesh's
  vertex data, and separately its index data, its own buffer from 4 MiB up
  (32 MiB when this spec was written; lowered in phase 1, Task 4b) and keeps
  shared slabs at 128 MiB or less, because
  slab growth once ran the 6 GB card out of memory. Close-up scans must stay
  in their own buffers: never split them into pieces that land in the shared
  slabs.
- Keep the smooth-streaming budgets (`PLANTS_PER_FRAME`, one tile per frame,
  `UNLOADS_PER_FRAME`) and precomputed plant bounds for every new plant tier.

## Approaches considered

- **A. Tier chains baked from the UHD scans (chosen).** For each tree species,
  decimate the scan into several tiers and bake the scan's normals into each one,
  then draw each tier in its own distance band with `VisibilityRange`
  crossfades. The farthest tier reaches the map edge. Rocks and plants get baked
  normal maps on their mid meshes the same way. Close up, every kind shows its
  UHD scan. Remove `STRATEGY_REACH`. Keep as many scans in VRAM as a budget
  allows, the UHD trees always.
  *Trade-offs:* one new bake tool and more asset files, but no new rendering
  technique. Bevy's automatic instancing keeps each tier of a species one batch.
- **B. Bevy meshlets for the UHD scans.** Automatic LOD on the full scans. It is
  experimental in 0.20, opaque-only, has no tangent normal maps, needs MSAA off,
  and is unproven with the fog-of-war culling and visibility ranges. Rejected:
  too risky for the gain.
- **C. Billboard impostors for the farthest trees.** These are the cheapest
  distant trees, but need a new shader and an atlas bake. Kept as the fallback
  for the farthest tier, only if measuring shows A's overview is too heavy.

Chosen: A. The user handed over the how; this is the assistant's call.

How the RAM level holds the scans (the user chose "keep everything in RAM,
copy to GPU"; how is the assistant's call):

- **R1. The files' bytes in an in-memory asset source (chosen).** At load,
  read every UHD file once into a Bevy `Dir` and register it as a second asset
  source (`MemoryAssetReader`, `register_asset_source`, both in
  `bevy_asset` 0.20.1). UHD loads use that source instead of `assets/`.
  Showing a scan is an ordinary `AssetServer` load, decoded from RAM on Bevy's
  worker pools and uploaded. Hiding it drops its handles, which frees the VRAM;
  the bytes stay. *Trade-offs:* little new code, because Bevy's own loader,
  scene spawning and handle-based freeing stay. RAM equals the files' size
  (BC7 textures stay compressed). Each return to the GPU decodes the file
  again: about 0.1–0.5 s of worker time, never on the frame.
- **R2. Decoded CPU-only assets, cloned for the GPU.** Keep every scan decoded
  (`MAIN_WORLD`) and build render-only copies on a worker thread. *Trade-offs:*
  closer to the user's words and faster to show again. But it needs a custom
  deep copy of multipart scenes (meshes, images and materials, up to 78
  materials in one plant scan) and holds more RAM (decoded meshes, generated
  tangents). Rejected as more code for no visible gain.

## Design

### Tree tiers

Each species gets a chain built from its UHD scan (`trees/uhd/<species>.glb`
in the workspace, in the same frame as today's runtime models):

| Tier | Triangles (target) | Textures | Draws (at detail 1.0) |
|---|---|---|---|
| UHD | the scan (~1.9M), unchanged | the scan's native 4K maps | 0 – 12 m |
| near | ~60k | 2K colour, baked normal, roughness | 10 – 45 m |
| mid | ~8k | 1K, baked normal | 45 – 220 m |
| far | ~1.5k | 512 px colour, baked normal | 220 – 900 m |
| speck | ~200 | 256 px colour | 900 m – map edge (~5 km) |

The numbers are starting points. The builder tunes them by measurement in the
densest wood, where one tree stands per ~75 m², so about 900 trees stand within
150 m. The far and speck tiers are the map-wide entities, spawned at load as
the crown cards are today. The UHD, near and mid tiers stream with the
scenery tiles (see Constraints, entity count). Sway, falling, crushing and
fog-of-war culling must act on every tier a tree shows. The shadow toggle
applies to the near and UHD tiers only; mid and farther never cast shadows. The
UHD tier is a child within its range like the others. The five UHD tree scans
go to the GPU behind the entry curtain and stay there (see "RAM and GPU"); all
trees of a species share one mesh. Dead trees have no crown and can
skip the near tier if mid holds up.

The old 500-tri crown cards (`trees/medium/`) are retired from runtime.

### Plants

- Keep the close-up swap to the UHD scan, but fix why it hurts today. It shows
  only one plant, loads 70–98 MB as you arrive, and pops into a different
  shape.
  - The nearest few plants (start with 4) within 8 m of the stalker show their
    scans.
  - The scans come from RAM as described in "RAM and GPU" below.
- Rebuild each `medium/` model from its UHD scan, so the swap only adds
  detail: decimate to at most 6k triangles (the shrub is 21.6k today), and
  bake both its colour and its normals. Many plant scans are multipart, with
  up to 78 materials, so the colour bakes into one atlas. Henbane and ragweed
  have no textured UHD scan (their workspace scans have geometry only): they
  keep their medium model close up, with a normal map baked from that
  geometry.
- Rebuild each `low/` model so it carries its own small colour texture, sharing
  the near material as today if its UVs fit, and cut the weeds to about 150
  triangles.
- Ranges (detail 1.0): near to 30 m. Far to 150 m for weeds and about 300 m for
  the bush kinds (shrub, bramble, dog rose, elder, juniper, thornbush,
  wormwood), so bushes still show around the focus up to `DETAIL_ZOOM`.

### Rocks

- Add the UHD tier: each workspace UHD source (1.45M triangles, native 4K
  colour), unchanged, as for the trees and plants (the user: "add uhd where
  missing"). It shows within 12 m plus the rock's size, through the RAM and
  GPU levels below.
- Bake a normal map from the same source onto the existing 10k `medium/` mesh,
  keeping its UVs and colour map.
- Extend the `low/` tier to the map edge for rocks of 7 m and up.
- Ranges otherwise stay as they are, minus `STRATEGY_REACH`.

### RAM and GPU

Two levels (the user's call: "keep everything in ram, copy to gpu, do not
move forth and back", "minimal disk io"). Native builds only: `wasm32` has a
4 GB address space, so the web build keeps loading from `assets/` as today
(`#[cfg(target_arch = "wasm32")]`).

**RAM: every asset, read once.**

- At load, read every UHD file the game can show into the in-memory source
  (R1 above). That is ~7.5 GB: trees 0.43, plants 1.0, sites 0.7,
  landmarks 1.2, and the new UHD tiers (~4 GB, rocks included). Read them
  through Bevy's own asset reader on its IO pool, so there is no `std::thread`
  and no `std::fs` in game code.
- Asset sources must be registered before `AssetPlugin` is built
  (`bevy_asset` `lib.rs`, `register_asset_source`). So `main.rs` registers
  the in-memory source with an empty `Dir` before `DefaultPlugins`, and keeps
  a clone of the `Dir` (it is shared) as a resource that the load step fills.
- The splash waits until the RAM source is full. Reading ~7.5 GB costs a few
  seconds on NVMe, ~15 s on a SATA SSD and much longer on a hard disk. It is
  less on later runs while the files sit in the OS page cache. *(Estimate;
  measure the splash time before and after.)*
- Everything else (mid and low tiers, terrain maps, shaders, UI images) is
  already read once at startup and kept in `PreloadedAssets`. It stays that
  way, and the new vegetation tiers join it.
- After the curtain lifts, nothing is read from `assets/` (goal 5). Each UHD
  path goes through one helper in `src/assets/mod.rs` that picks the
  in-memory source on native builds and `assets/` on the web, so no call site
  names a source itself.

**GPU: copies, kept as long as they fit.**

- Showing a scan loads it from the RAM source with render-only usages
  (`RenderAssetUsages::RENDER_WORLD` for meshes, materials and images). The
  upload then moves the data instead of cloning it during render extract.
  With both usages, Bevy 0.20 clones the whole asset in
  `extract_render_asset` on every upload (`bevy_render` `render_asset.rs`). A
  ~100 MB clone during extract is a likely part of the 31 ms extract frame in
  the garage trace.
- **One owner keeps the copies alive.** In Bevy a GPU copy lives exactly as
  long as some strong handle to its asset; when the last one drops
  (`AssetEvent::Unused`), the render world frees it. So one resource (e.g.
  `GpuScans`) holds a strong handle to every scan that is on or going to the
  GPU, along with its byte count and when it was last shown. Spawned entities
  hold handles too, but only `GpuScans` decides when a copy leaves: it drops
  its handles once no entity shows the scan any more.
- **Knowing a copy is on the GPU.** Bevy 0.20 has no event for "uploaded", and
  `RenderAssets` lives only in the render world. So a small render-world
  system, after asset preparation, checks the tracked meshes and images in
  `RenderAssets` and writes the ids that are ready into a shared set (an `Arc`
  resource present in both worlds). `GpuScans` reads that set to decide when
  a UHD tier may show. *(Assistant's call.)*
- **A VRAM budget, not a count, read live from the driver.** About once a
  second the game reads the device-local heap's budget and its own usage
  (`VK_EXT_memory_budget`). Usage minus the resident scans is everything else
  the game holds. So scan budget = min(driver budget, `--vram`) minus that,
  minus a 0.5 GB margin. The desktop, other programs and the game's own base
  are all inside the driver's numbers, so the budget is whatever is really
  left on this host, with no constants to tune. Where the driver cannot report
  it, scan budget = (`--vram` or 6) minus the game's measured peak VRAM
  without UHD scans, minus the desktop's ~0.6 GB, minus the margin. Count
  each scan's bytes as it reaches the GPU (meshes plus images).
- **Never past the budget.** If the budget shrinks below what the scans use
  (another program takes VRAM, or a small card), scans not shown this frame
  leave, unpinned before pinned, least recently shown first, one per reading,
  until the scans fit. Staying over the driver's budget is the crash, so this
  ignores the two-minute rule below.
- **Always on the GPU when they fit:** the five UHD trees, the stalker (as
  today), the armored car and the hangar shell. They are seen at every zoom
  near the stalker, so they go up first behind the curtain and never leave for
  other scans. A pin that does not fit the budget waits (warn once), and that
  close-up keeps its medium model. *(Amended: pins used to load even over the
  budget.)*
- **Fill behind the curtain.** Then upload the other scans nearest the spawn
  first (hangar props, artifacts, yard vehicles, nearby plants and rocks),
  until the budget is full.
- **Do not go back and forth.** A GPU copy stays after its object goes out of
  range. It leaves only when a new scan needs room, and only if it has not
  been shown for at least two minutes and is not near any anchor. The least
  recently shown one goes first. If nothing may leave, the new scan waits and
  its mid tier stays. A copy that leaves is only dropped from the GPU: the RAM
  copy stays and nothing is read from disk. Every scan should fit once the
  driver offers the game about 7.4 GiB, which takes a 10 GB card, so nothing
  ever leaves there (estimated in phase 1, Task 9: the 42 scans take about
  4.4 GiB on the GPU, with 14 of the 24 site scans measured and the rest taken
  at ~107 MiB, plus 2.4 GiB for the rest of the game and the 0.5 GiB margin;
  not yet run on a larger card. This first said 12 GB). *(Assistant's call: two minutes is a starting point.)*
- **Upload early.** Start a scan's upload once an anchor comes within about
  40 m of an object that may show it (the car covers that in under 2 s at
  24 m/s). Show it only once it is on the GPU; until then the mid or near tier
  stays. Never wait, and never pop in late at close range.
- **Spread uploads.** *(Amended in phase 1: Ruling 30 dropped the limit this
  bullet asked for.)* It asked for Bevy's `RenderAssetBytesPerFrame` at about
  32 MB per frame. That limit also spread the fog-of-war mask and the
  stalker's ring over several frames, so they flashed and blinked, which is
  worse than a one-frame hitch; Bevy's default, no limit, stays. Task 9's
  traces measured the cost: each scan upload after the curtain holds one
  frame for 103-147 ms (`allocate_and_free_meshes` 65-105 ms, plus up to
  45 ms of `prepare_assets<GpuImage>`). The walk had five (plant scans of
  76-135 MB), the drive one (the Institute, 65 MB), and the wood run, with
  every tree and plant scan on the GPU before the curtain, none. Do not split
  scans to get around that (see GPU buffers under Constraints). No upload
  limit is added in phase 1; whether one returns (with the ring and the
  shroud made not to re-upload each change) is decided after the
  eviction-cascade fix is measured, since these gaps came while the budget
  cascaded.
- Sites and landmarks keep their own rule for when to show a scan
  (`sites.rs`: within 150 m of a `StreamAnchor`), under the same budget. In
  the trace the garage took ~1.1 s to load from disk; from RAM it arrives in
  step with the anchor.

### UHD for the other kinds

Every kind with a UHD source in the workspace gets it as a close-up tier (the
user: "add uhd where missing"). The `medium/` model stays as the distance
tier and keeps the collision, so each UHD file must sit in exactly the same
frame as its medium model: same scale, origin and forward axis, grounded the
same way. Each gets the usual finish (`tools/compress_glb_textures.py`), and
its tier is drawn with a `VisibilityRange` crossfade like the vegetation.

| Kind | Workspace UHD | In-game models | Close-up range (start) | Per-kind work |
|---|---|---|---|---|
| Artifacts | 842 MB, 14 | 14 showcase pieces | 8 m (as today) | Point `ARTIFACTS` at the `uhd/` scans (the archived untouched 4K PBR scans); the 8 m stream already exists. Easiest; do first. |
| Hangar shell | 66 MB, 1 | `hangar.glb` | while the stalker or view is at the base (always on the GPU) | Must be the same arched shell (not the `parts/` kit) and one primitive for the cutaway material. Level it with `tools/prepare_hangar_asset.py`, then re-check the wall and pendant measurements in `base.rs`. |
| Hangar props | 540 MB, 9 (7 used) | lab, map board, repair stand, table, toilet, tools, workshop | 15 m | Ground with `tools/prepare_plant_asset.py` like the medium; the hangar-local poses and colliders stay. The two Roadside map boards are not used. |
| Vehicles | 315 MB, 5 | armored car (and its UN repaint), crawler, quarry vehicle, sedan, freight train | 20 m (the car always on the GPU) | Armored car: keep its wheel pivots and node layout, and bake the same atlases as `tools/prepare_vehicle_asset.py`, or the swap shows a different paint. The UN car is its repaint, made by the same step. Yard vehicles and the train are static. |
| Animals | 268 MB, 4 | frostwolf, hexadeer, skyglider, chameleon | 12 m | Skin with the same `rig.rs` weights. They are computed from vertex positions when the model loads, so the UHD needs only the medium's frame. Do it while the scan is decoded, off the main thread (or bake the weights into the GLB if that measures slow). |
| Birds | 315 MB, 5 | hawk, heron, owl, raven, sparrow | 12 m | As animals. Flocks have five birds; a UHD skin per bird is affordable only for the few nearby. |
| Mutants | 258 MB, 4 | bovine, chimera, fleshdeer, mutt | 12 m | As animals, with their species foot and head weights. |
| Vegetation clusters | 246 MB, 4 | dead forest, overgrowth, mud bog, slime ditch | 12 m | Same frame as the medium, so the collision pillars stay; check with `tools/trim_tree_base.py` that they still match. |
| Rocks | 299 MB, 7 | 7 sizes | 12 m + size | See Rocks. |

Skinned UHD creatures cost GPU skinning for ~1–2M vertices each, and again in
every shadow pass. So at most the nearest few show their UHD (start with 3),
and the builder decides by measurement whether they cast shadows from the UHD
or the medium.

### Ranges and the detail setting

- Remove `STRATEGY_REACH` from `detail_range` for vegetation and rocks. The tiers
  replace it: from the tactical zoom the mid and far tiers show, which is
  enough. Other users of `detail_range` (walls, ground stones, sites) keep their
  current reach unless the builder finds the same problem there; check each.
- Add a `DetailScale` resource (default 1.0) that multiplies every vegetation
  and rock tier distance, set from `--detail <x>`. Parse `--detail` and `--vram`
  next to `--new`, in one place.

### README

In the step that adds the flags, and not before, add a "Graphics quality"
section to `README.md`, next to the run commands, along these lines:

> The game is tuned for a 6 GB GPU (RTX 2060). Two flags raise the quality on
> stronger cards:
>
> - `--vram <GB>` (optional): the most video memory the game may use. The
>   game reads every model once at start and keeps it in RAM, as much as the
>   machine has free (the loading screen takes longer the first time). On the
>   GPU it keeps as many close-up (UHD) models as the driver says fit, and
>   frees the least recently seen ones when memory runs short. From about
>   10 GB up (estimated), all of them stay. Use the flag to leave video memory for other
>   programs, or where the driver cannot report it (then the game assumes
>   6 GB).
> - `--detail <x>` (default 1.0): how far the higher-detail versions of trees,
>   plants and rocks draw, as a multiple of the default distances.
>
> For the best quality on a 12 GB card:
>
>     cargo run --release -- --vram 12 --detail 2
>
> Both combine with `--new`.

Also list both flags in `CLAUDE.md` under Controls, next to `--new`.

### The bake tool

There is no high-to-low bake tool yet. Add one Blender tool that, given a source
scan and a triangle target, writes one tier:

1. Collapse-decimate a copy to the target, keeping the scan's UVs.
2. Bake a tangent-space normal map from the scan onto the decimated copy
   ("selected to active", a small cage extrusion).
3. Resize the colour and roughness maps to the tier's size.
4. Export with tangents, in the scan's exact frame. Grounding and recentering
   stay off: the frame must match the trunk pillars.

It is generic, so it lives in `~/Projects/agentic/agentic-tooling/media/`
(name it e.g. `glb-lod-bake`) per the user's reusable-tooling rule. A small
project script in `tools/` lists every tier to build (source, target, triangle
budget, texture size), so a rebuild is one command. Finish every output with
`tools/compress_glb_textures.py`.

### Measuring

Build on `374be19` (smooth streaming). Its traces in `target/playtest/`
(`drive2_*`, walk) are the hitch baseline. Xephyr caps the frame rate (~27
FPS there), so measure GPU cost per pass with a temporary
`RenderDiagnosticsPlugin` + `LogDiagnosticsPlugin` (`elapsed_gpu`), and
remove both afterwards.

Before any change, record a baseline with the traced build and
`bevy-trace-summary`, using fixed input and `XDG_DATA_HOME` set to a scratch
folder: an fpv walk through the densest wood, an fpv drive along a forest road,
and a zoom out to the overview. In the same baseline, record the peak VRAM
(`nvidia-smi` sampled during the run, minus the desktop's share): it sets the
scan budget. Rerun the same scenarios after each kind (trees,
then rocks, then plants) and compare frame times and the worst frames. Capture
a screenshot at fpv, at the tactical zoom (90 m) and at the overview, before and
after, for the user to judge the look.

Also record, before and after each phase:
- The splash's load time, cold (after dropping the page cache) and warm.
- The game's peak RAM (resident set size).
- Disk reads during play: run the traced walk and drive under
  `strace -f -e trace=openat`, and list any `assets/` path opened after the
  curtain. The target is none.

## Order of work

Each phase is measured and playable on its own:

1. **RAM and GPU levels.** Add the in-memory source and the `--vram` budget,
   and route today's disk loads through it: tree close-ups, the plant scan,
   sites and landmarks, artifact scenes. Then confirm no `assets/` reads after
   the curtain. This also removes the garage's 1.1 s load.
2. **Vegetation and rock tiers.** Add the bake tool, the tree, plant and rock
   tiers, the rocks' UHD, `--detail`, and remove `STRATEGY_REACH`.
3. **UHD for the other kinds,** in the order of the table: artifacts, hangar
   shell and props, vehicles, vegetation clusters, then the creatures (rig and
   skinning cost last).
## Left to the builder

- Exact triangle budgets, texture sizes and range bands, tuned by measurement.
- Whether a species or plant needs every tier (e.g. the dead tree's near tier).
- How the streamed tiers hang together with today's split between the
  map-wide `CanopyVisual` (sways, no collider) and the per-tile
  `StandingTree` (collider, falls), so a tree sways and falls as one, whichever
  tier shows.
- Cage distance and bake settings per kind.
- Whether `rocks/medium` keeps the UV layout of its UHD source (it was
  simplified from it). If not, bake the colour as well as the normal map.
- Whether `WOOD_DENSITY` needs thinning to meet the frame budget.
- Fallback C (billboard impostors) for the farthest tier, only if the
  overview measures worse than today.
- Updating `CLAUDE.md` and `arch-readme.md` (asset tiers, ranges, the bake
  tool, the RAM source), and moving retired files into the archive zip with
  their `MANIFEST.txt` sums.
- The exact keep time and upload-early radius for GPU copies, and the always
  on the GPU list, tuned against `nvidia-smi` on the 6 GB card.
- How the UHD tier of each other kind is spawned and crossfaded (a child with
  a `VisibilityRange`, or a swap as the trees do today), per kind.
- Whether the creatures' UHD skin weights are computed at load or baked into
  the GLB by a tool.
