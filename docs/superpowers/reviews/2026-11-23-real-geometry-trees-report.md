# Real-looking trees: many-triangle tiers cut from the UHD scans

Date: 2026-11-23. Status: proposal, nothing started. Looked at: branch `worktree-vegetation-lod-design`
at `e34c6b8` plus the uncommitted work in the tree.

Request: "solution to have many triangles trees (for me real tree looking like); current ones are nerfed
to 500 triangles from thousands and look cheese". The un-nerfed trees are the scans in
`~/Projects/agentic/RoadSide-Picnic/models/trees/uhd`.

## How this report was made

- Read: `src/world/{canopy,tree_fall,shroud,landscape,mod}.rs`, `src/player/camera.rs`,
  `src/assets/mod.rs`, the tree specs and plans in `docs/superpowers/`.
- Measured: every tree GLB in the game and in the workspace, with `glb-inventory` and `glb-components`
  (agentic-tooling, `media/`).
- Read the Bevy 0.20.1 sources for meshlets and visibility ranges (`~/.cargo/registry/src/index.crates.io-*/`).
- Nothing was built, baked or run. Triangle counts and sizes of existing files are measured. Every
  number for a new tier (triangles in view, memory, disk, bake time) is an estimate until the
  checkpoints in section 8 measure it.

## 1. The answer in short

- **Why the trees look like that.** A tree has only two looks: the 1.9M-triangle scan within 8–12 m
  of the camera, and a ~500-triangle crown-card model everywhere else. Nothing sits between them.
  From the start zoom (28 m) and the tactical zoom (90 m) every tree is the card. Past 200–435 m a
  tree is not drawn at all, and at the tactical zoom no tree casts a shadow.
- **The solution.** Cut four lighter versions of each tree from its own scan (about 120,000, 15,000,
  2,000 and 250 triangles), each with colour, normal and roughness maps baked from the scan. Show the
  one that fits the tree's size on screen and crossfade between them. A tree at the tactical zoom then
  has about 15,000 real triangles in place of 500.
- **Why this works here.** Each scan is one connected, opaque surface (measured), so it can be reduced
  in steps and keep its shape. No new shader is needed, and nothing changes in the engine setup.
- **Cost (estimates).** About 27 MB of GPU memory and 16 MB on disk per species; 5–6M more triangles
  in the first-person view of the densest wood, about 4.5M at the tactical zoom, 2.5M in the overview.
- **Your decision.** This replaces the impostor half of the tree design you approved today ("A for
  trees"), where trees past ~55 m become two-triangle pictures of the scan. Section 11 lists this and
  three smaller decisions.

## 2. What the game draws today

| Distance from the camera | What a tree draws | Triangles |
|---|---|---|
| 0–8 m, fading out to 12 m | the UHD scan, a streamed child (`UHD_RANGE`, `UHD_FADE`) | 1.90–1.97M |
| 8 m to `draw_distance` + 75 m, fading over 25 m more | the crown-card model (`detail_range`, `STRATEGY_REACH`) | 492–504 |
| beyond that | nothing | 0 |

The crown card's reach by species (`MODELS` in `canopy.rs`): small tree 200 m, dead tree 230 m, birch
250 m, broadleaf 270 m, tall conifer 410 m.

- **The views.** The camera zooms from 6 m to 4,600 m (`DISTANCE_LIMITS`). Play starts at 28 m and the
  tactical view is 90 m. So only the one or two trees right beside the camera are ever scans.
- **Shadows.** A tree casts a shadow only within 48 m of the camera (`SHADOW_ENTER`, exit at 55 m;
  `update_canopy_motion`). At the tactical zoom the camera is 90 m from what it looks at, so no tree
  casts one. The shadow cascades do reach there (`fit_view_to_zoom` sets them to 2.5 × the zoom
  distance); only this tree rule stops them.
- **About 10,000 trees** stand on the map, one entity each, spawned at load (`spawn_canopies`).

The files (measured):

| File | Triangles | Maps | Size |
|---|---|---|---|
| game `trees/medium/*.glb` (crown cards) | 492–504 | 3 × 2048×1024 KTX2 (dead tree 1024²) | 2.8–3.1 MB |
| game `trees/uhd/*.glb` | 1,898,428–1,970,744 | 3 × 4096² KTX2 | 102–112 MB |
| workspace `trees/uhd/*.glb` (Blender can read these) | the same geometry and frame | 4096² jpg/png | 62–69 MB |
| workspace `trees/low/*.glb` | 162–250 | none | 7–10 KB |
| workspace `trees/medium/{roadside_tree,tall_tree}.glb` | 18,570 and 29,567 | yes | 1.9–2.4 MB |

## 3. Why the scan cannot simply be drawn further out

- 10,000 trees × 1.9M triangles is 19 billion. One wood (600–800 trees) is over a billion.
- Raising the scan's range from 12 m to 20 m alone puts 3–8 scans in view in a wood: 6–15M triangles
  for a handful of trees, and every other tree is still a card.
- My working budget for an RTX 2060 is about 10M tree triangles a frame. That is an estimate; the
  traced runs in section 9 replace it with a measurement.

## 4. What makes real geometry possible: the scans are single surfaces

Connected pieces of each UHD scan (`glb-components`):

| Scan | Pieces | Triangles per piece |
|---|---|---|
| birch | 1 | 1.9M |
| dead tree | 3 | 1,769,644 + 81,442 + 47,342 |
| small tree | 4 | 1,736,391 + 93,770 + 44,610 + 33,244 |

The material is opaque and single-sided, with no alpha cutout. So a scan is a sculpted solid, not
thousands of loose leaf cards. Edge-collapse decimation reduces a mesh like that step by step and keeps
its silhouette. Loose leaf cards would fall apart under the same treatment, and that is the usual
reason games need crown cards or impostors. These scans do not have that problem.

## 5. Options compared

| Option | A tree at the tactical zoom | Needs | Verdict |
|---|---|---|---|
| A. Geometry tiers cut from the scans | ~15,000 real triangles, casts a shadow | a bake tool, ~300 lines of Rust, no shader | **recommended** |
| B. Bevy meshlets (virtual geometry) | the full scan, automatic level of detail | MSAA off for the whole game, Vulkan or Metal with 64-bit atomics, no web build | rejected |
| C. Impostors past ~55 m (the queued plan) | a two-triangle picture of the scan | an atlas bake tool and a new shader | does not meet "many triangles" |
| D. Show the scan further out | 1.9M per tree | nothing | not affordable (section 3) |
| E. The two 18–30k models in the workspace | a different tree from the scan | nothing | the tree would change shape at 12 m |
| F. New Tripo generations | new models | credits | the vegetation spec rules out new models |

**B, meshlets, in detail** (Bevy 0.20.1, `bevy_pbr-0.20.1/src/meshlet/`):
- `configure_meshlet_views` ends the process unless the camera runs with `Msaa::Off`.
- `check_meshlet_features` ends the process on a GPU without `TEXTURE_INT64_ATOMIC`, `SHADER_INT64`,
  `SUBGROUP` and the rest. Vulkan and Metal only, so no web build.
- `extract_meshlet_mesh_entities` takes every meshlet instance each frame. It ignores `Visibility`,
  visibility ranges and so the fog-of-war culling (`cull_shrouded`) and the crossfades.
- Opaque materials only; the feature is marked experimental.
- A mesh fallback would still be needed for the GPUs it refuses, which is option A anyway.

**C, impostors, fairly.** An impostor is a picture of the real scan, lit with its normals, so from far
away it does look like the tree, and it is the cheapest option by a wide margin. Its limits are that it
is flat (two triangles), it casts no proper shadow, and the queued design starts it at ~55 m, inside the
tactical view. If your complaint is only about the crown cards and a faithful picture would do, the
queued plan already covers it. If you want real geometry at the views you play in, it does not.

## 6. The recommended design

### 6.1 Tiers

Starting numbers, tuned at the checkpoints:

| Tier | Triangles | Maps | Takes over when the tree is |
|---|---|---|---|
| UHD scan | 1.9M | native 4K | closer than 12 m (unchanged) |
| near | 120,000 | 2K colour, normal, roughness | taller than the screen down to ~400 px |
| mid | 15,000 | 1K colour, normal, roughness | under ~400 px tall |
| far | 2,000 | 512 px colour, normal, roughness | under ~90 px tall |
| speck | 250 | 256 px colour | under ~25 px tall |

Bands follow the tree's height: `distance = 1304 × height / pixels` (45° vertical view at 1080p, so one
metre covers 1304 / distance pixels). One set of bands per species, from its mean height:

| Species | Height (`scale`) | near | mid | far | speck |
|---|---|---|---|---|---|
| small tree | 4.8–7.5 m | 12–20 m | 20–89 m | 89–321 m | 321 m on |
| dead tree | 9.5–14.5 m | 12–39 m | 39–174 m | 174–626 m | 626 m on |
| birch | 10–15 m | 12–41 m | 41–181 m | 181–652 m | 652 m on |
| broadleaf (leaves in Task 11) | 13.5–19.5 m | 12–54 m | 54–239 m | 239–861 m | 861 m on |
| tall conifer (leaves in Task 11) | 25.5–33.5 m | 12–96 m | 96–427 m | 427–1,539 m | 1,539 m on |

- Each tier has 8–16 times fewer triangles than the one before and takes over 3.6–4.5 times further
  out, so a triangle is about the same size on screen at every handover. With a rough 300 m² of
  surface for a birch, an edge is ~7 px long when a tier takes over and ~2 px when it hands over.
- Tiers crossfade with the `VisibilityRange` dither, as the scan and the crown card do today.
- All tiers are opaque `StandardMaterial`: no alpha cutout, no new shader.
- Bands are tuned for 1080p. Scaling them with the window height is left out; it can follow if a
  1440p screen shows the handovers.

### 6.2 Triangles in view (estimates, densest wood, one tree per 75 m²)

| View | Tree triangles today | With tiers | How |
|---|---|---|---|
| first person, in a wood | ~2M (one scan) + ~0.1M | ~7.5M | 1 scan + 13 near × 120k + up to 263 mid × 15k |
| tactical, 90 m | under 0.2M | 2.6–4.5M | 175–300 mid × 15k |
| 200 m zoom over a wood | under 0.4M | ~1.6M | 800 far × 2k |
| overview | 0 (trees are not drawn) | ~2.5M | 10,000 speck × 250 |

The shadow pass is on top of this and is the largest unknown:
- Trees within 55 m of the camera or of the view focus cast shadows.
- Around the focus at the tactical zoom that is up to 127 mid trees: about 1.9M per shadow cascade
  that holds them.
- In first person it is the near tier: between 2.5M and 8.4M, by sun direction (the upper bound is all
  70 near trees within 41 m).

### 6.3 Memory and disk (estimates)

| Tier | Maps on the GPU (BC7/BC5 with mips) | Mesh on the GPU | On disk |
|---|---|---|---|
| near | 3 × 5.6 MB = 16.8 MB | ~4.6 MB | ~13 MB |
| mid | 3 × 1.4 MB = 4.2 MB | ~0.5 MB | ~2.6 MB |
| far | 3 × 0.35 MB = 1.05 MB | ~0.1 MB | ~0.6 MB |
| speck | 0.09 MB | negligible | ~0.1 MB |
| **per species** | | **~27 MB** | **~16 MB** |

- Three species: ~82 MB on the GPU, against ~21 MB for their crown cards today. About 60 MB more.
- Three species: ~50 MB in Git LFS, against 8.5 MB today.
- The tiers are preloaded at startup like the crown cards, so they never stream and never wait.
- If the 4 GB laptop target needs it: drop the near roughness map for a constant (5.6 MB per species)
  or bake the near maps at 1K (12.6 MB per species).

### 6.4 Runtime

**Entities.**
- One map-wide root per tree, as today. It draws the far tier.
- Past the far band the root swaps its mesh and material to the speck (10% hysteresis). A 25 px tree
  needs no fade.
- The mid, near and UHD tiers are children. One system spawns them by camera distance and drops them a
  few metres further out, at most 150 spawns per frame (the `PLANTS_PER_FRAME` figure).
- Mid and near are preloaded plain meshes, so a child shows the frame it spawns.
- About 1,000 children exist at once (an estimate: one wood within the mid band plus ~70 near).

**Why streamed children and not three permanent ones per tree.** Bevy rebuilds its whole
visibility-range table whenever any range is added or changed (`extract_visibility_ranges`,
`bevy_render-0.20.1/src/view/visibility/range.rs:203`). Plants stream on most walking frames, so the
table is rebuilt often. Permanent children would add 30,000 entries to every rebuild; streamed ones add
about 1,000. Identical ranges share one GPU slot (`range_to_index`), so per-species bands stay far
below the 65,535 limit.

**Handover rule.** A tier starts where the next finer tier ends only while that finer child exists (for
the UHD scan: once it is revealed, as `reveal_close_tree` does today). Otherwise it starts at 0. So a
tree never vanishes while a tier is missing or still spawning. One function, `tier_margins`.

**Shadows.** Near and mid children cast within 48–55 m of the camera or of the `ViewFocus`. Trees
around the focus then cast and shade each other at the tactical zoom. Far and speck never cast.

**Sway, falling, fog of war.**
- Sway stays on the root's transform; children follow.
- A felled tree loses `CanopyVisual` (`break_selected_trees`) and keeps its children. The new `Tiers`
  component stays on it, so its tiers keep streaming as the camera moves.
- `cull_shrouded` filters every mesh entity by its bounds, so the children are covered as they are.
- Trunk collision is unchanged: every tier is exported in the scan's exact frame.

**Files.**
- New `src/world/canopy/tiers.rs`: the bands, the `Tiers` component (replaces `CloseScan`),
  `stream_tree_tiers` (replaces `stream_close_trees`), `tier_margins`, the shadow toggle.
- `src/world/canopy.rs`: `TreeModel` gets `tiers: [&str; 4]` and loses `draw_distance`; `CanopyModels`
  holds four looks per species; `spawn_canopies` gives the root the far tier; `update_canopy_motion`
  keeps only the sway. Trees stop using `detail_range` and `STRATEGY_REACH`.
- `src/assets/mod.rs` (tier paths) and `src/world/mod.rs` (system registration).
- Reused unchanged: `load_single_primitive`, `standard_material_from_gltf_material`,
  `range_scene_meshes`, and the `GpuScans` pin and want calls for the UHD child.

### 6.5 Bake

**A generic tool, `media/glb-lod-bake`, in agentic-tooling** (headless Blender 4.5,
`~/.cache/blender/blender-4.5.16-linux-x64/blender`):
1. Import the scan and weld its vertices. The scan's UV seams split them, and unwelded, decimation
   tears the surface along every seam.
2. Collapse-decimate a copy to the triangle target and join it into one mesh with one material
   (`load_single_primitive` reads exactly mesh 0, primitive 0, material 0).
3. Unwrap the copy anew (Smart UV Project). The scan's own UV layout has too many islands to survive a
   16× to 7,600× reduction.
4. Bake colour, roughness and a tangent-space normal map from the scan onto the copy (Cycles, selected
   to active). The normal map carries the bark and leaf relief the triangles no longer do.
5. Export in the scan's exact frame: no grounding, no recentring.

It gets unit tests on a synthetic bumpy sphere: triangle count, unchanged frame, three maps present,
the bumps visible in the normal map.

**A project script, `tools/build_tree_tiers.py`**, lists species × tier (triangles, map size, output
path) and runs the tool from the workspace sources. Each output is finished with
`tools/compress_glb_textures.py` (last, as the project rule says), `glb-bake-tangents` and
`npx @gltf-transform/cli validate`.

**Outputs.**
- `trees/medium/tree_<s>.glb` (near, replacing the crown card) and `trees/medium/tree_<s>_mid.glb`;
- `trees/low/tree_<s>.glb` (far) and `trees/low/tree_<s>_speck.glb`.
- The crown cards go into the archive zip with their `MANIFEST.txt` sums before they are overwritten.

**If the far and speck tiers come out tattered** (collapse can leave shards on a leafy crown at 250
triangles): voxel-remesh the scan into a closed hull first, then decimate that. The hull keeps the
crown's volume.

Bake time is not known; I expect minutes per tier. Step 3 measures it.

## 7. How this relates to what is already approved

- **`2026-11-23-vegetation-lod-design.md`** set the first tree chain (UHD, near ~60k, mid ~8k, far
  ~1.5k, speck ~200), rejected meshlets, and required that map-wide a tree keeps at most two visual
  entities. This proposal returns to that chain with higher counts and keeps the rule: one entity per
  tree map-wide, children only near the camera.
- **`2026-11-23-tree-impostors-design.md`** replaced the mid, far and speck tiers with impostors from
  ~55 m. This proposal reverses that part.
- **`docs/superpowers/plans/2026-11-23-tree-impostors.md`** (9 tasks, not started, queued after the
  asphalt and wall plan):

| Its task | Here |
|---|---|
| 1 before numbers | kept (step 4) |
| 2 `glb-lod-bake` | kept, changed: unwraps anew and bakes colour too (step 2) |
| 3 `glb-impostor-bake` | dropped |
| 4 `tools/build_tree_tiers.py` | kept, four tiers per species (step 3) |
| 5 impostor material and `impostor.wesl` | dropped |
| 6 near tier in `canopy/tiers.rs` | kept, extended to mid and to the far/speck swap (step 5) |
| 7 look checkpoint | kept (checkpoint B) |
| 8 commit assets, 9 after numbers | kept (step 7) |

## 8. Steps and checkpoints

1. Record the decision: save the plan under `docs/superpowers/plans/` and add a status line to the
   impostor spec and plan.
2. Build `glb-lod-bake` with its tests (agentic-tooling).
3. Write `tools/build_tree_tiers.py`; bake the birch (the leafiest) and the dead tree (the thinnest).
   **Checkpoint A:** a contact sheet from `glb-preview`, each tier beside the scan at the pixel size
   where it takes over, with today's crown card. It shows shape only, without normal maps.
4. Take the before numbers and screenshots with the runners in `target/playtest/phase1/followup/`: the
   wood walk, a tactical orbit, the overview, and shots of a birch and a dead tree from 5, 15, 30, 45
   and 60 m.
5. Write the runtime tiers and their tests. This waits until `src/world/canopy.rs` is free: another
   session has an uncommitted edit in it.
6. **Checkpoint B, your go:** the same shots in the game, before and after. Tuning happens here.
7. Bake the remaining species, archive the crown cards, take the after numbers, update
   `doc/world-reference.md` and `arch-readme.md`.

Steps 1–4 only add files. Nothing is committed in the game repo without your say.

## 9. Verification

**Tests** (`cargo test world::canopy`):
- tiers hand over without a gap in every combination of present children;
- children spawn inside the load distance, drop past the drop distance, at most 150 per frame;
- the root swaps far and speck with hysteresis;
- a felled tree keeps its children;
- only near and mid children cast shadows, within range of the camera or the focus;
- the existing close-scan tests, reading `Tiers`.

**Checks:** `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`, `validate` on each GLB.

**Look:** `cargo run --profile playtest` with `XDG_DATA_HOME` in a scratch folder, and the step 4
shots again.

**Frames** (release trace build, CPU spans and GPU pass times):
- no frame over 33 ms in the wood walk;
- the overview within 3 ms of before;
- first-person and tactical means reported against before.

**If the wood walk misses, in this order:**
1. near trees cast shadows only within 30 m;
2. the near tier drops from 120k to 80k;
3. ask you. Thinning the woods (`WOOD_DENSITY`) changes the map and is your call.

## 10. Risks the checkpoints are there to catch

- **The 12–20 m handover.** A 120k crown is smoother than the scan and its outline is faceted at
  roughly 5–8 px there. Levers: near tier up to 200k, or the scan's range up to 15 m.
- **The dead tree's twigs** thin out or vanish in the mid tier. Lever: a higher mid count for that
  species alone.
- **The normal bake** can pick the wrong surface inside a dense crown and leave dark blotches. Lever:
  a shorter ray distance, or the voxel hull as the bake target.
- **The shadow pass** (section 6.2) is the largest frame cost and the least certain estimate.
- **A solid crown** is what the scans are. Tiers keep that look; they do not add see-through foliage.
  If the scans themselves look too solid from 15 m, no decimation fixes it.

## 11. Decisions that are yours

1. **Geometry tiers at every distance, in place of impostors past ~55 m.** This changes "A for trees".
   I recommend it: it is the only option that gives many-triangle trees at the views you play in, and
   it needs less new code than the impostor plan (no atlas tool, no shader).
2. **Species.** Three (birch, dead tree, small tree) if phase 1's Task 11 lands first; otherwise all
   five are baked so the game still loads, at ~27 MB of GPU memory each.
3. **Tree shadows at the tactical zoom.** Included. It is a visible gain and a real cost; say if you
   would rather keep it out of this change.
4. **When.** After the asphalt and wall plan, as the impostor plan was queued, or ahead of it.

## Appendix: where each fact comes from

- Tree ranges, shadows, species: `src/world/canopy.rs` (`UHD_RANGE`, `UHD_FADE`, `SHADOW_ENTER`,
  `SHADOW_EXIT`, `MODELS`, `spawn_canopies`, `reveal_close_tree`, `update_canopy_motion`);
  `src/world/mod.rs` (`STRATEGY_REACH`, `detail_range`).
- Zoom limits and shadow cascades: `src/player/camera.rs` (`DISTANCE_LIMITS`, `TACTICAL_DISTANCE`,
  `START_DISTANCE`, `fit_view_to_zoom`).
- Tree falls: `src/world/tree_fall.rs` (`break_selected_trees`). Fog of war: `src/world/shroud.rs`
  (`cull_shrouded`). Wood density: `src/world/scenery.rs` (`WOOD_DENSITY`), `WOODS` in `canopy.rs`.
- Meshlets: `bevy_pbr-0.20.1/src/meshlet/` (`configure_meshlet_views`, `check_meshlet_features`,
  `extract_meshlet_mesh_entities`).
- Visibility ranges: `bevy_camera-0.20.1/src/visibility/range.rs` (`check_visibility_ranges`),
  `bevy_render-0.20.1/src/view/visibility/range.rs` (`extract_visibility_ranges`, `range_to_index`).
- Triangle counts, maps and pieces: `glb-inventory` and `glb-components` on the files in section 2.
