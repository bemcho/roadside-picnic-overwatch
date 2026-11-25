# Tree Geometry Tiers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every tree draws real triangles cut from its own UHD scan at every zoom:
- the scan within 12 m;
- then a 120k, 15k, 2k and 250-triangle tier, each with colour, normal and roughness maps baked from the scan;
- chosen by the tree's height on screen, and crossfaded.

**Architecture:**
- **Bake.** A generic agentic-tooling tool, `media/glb-lod-bake` (headless Blender), welds a scan, decimates a copy,
  unwraps it anew and bakes the scan's colour, roughness and normals onto it. `tools/build_tree_tiers.py` runs it
  for three species × four tiers and finishes each GLB.
- **Runtime.**
  - The map-wide `CanopyVisual` root draws the far tier and swaps to the speck past its band.
  - Mid and near children stream in by camera distance, at most 150 spawns a frame. The UHD child is unchanged.
  - Tiers crossfade through `VisibilityRange` margins.
  - Near and mid children cast shadows within 48–55 m of the camera or the view focus.
  - There is no new shader: every tier is an opaque `StandardMaterial`.

**Tech Stack:**
- Bevy 0.20.1, Rust 1.100;
- headless Blender 4.5 (Cycles), Python 3;
- `ctt` (KTX2), glTF Transform validate.

**Spec:** `docs/superpowers/specs/2026-11-23-tree-geometry-tiers-design.md`.
- Read its status block first: the user's four decisions and the corrected fallback order.
- It supersedes `2026-11-23-tree-impostors-design.md`, and amends "Tree tiers" in
  `2026-11-23-vegetation-lod-design.md`, whose goals, anti-goals and constraints still bind.

**Where this plan departs from the spec, and why:**
1. **When the small tree is baked.** Task 3 bakes it with the birch and the dead tree, because the game cannot load
   with a species' tier files missing. Assets are still committed only after checkpoint B (Task 7).
2. **The UHD child keeps its own system.** `stream_close_trees` stays, reading `Tiers`, instead of being folded
   into `stream_tree_tiers` as section 6.4 says. The scan goes through `GpuScans` and a scene, while the other
   tiers are preloaded meshes, and one system for both would pass 60 lines.
3. **Speck swaps are capped at 2,000 a frame** (my call). Zooming out to the overview swaps every tree on the map,
   and each swap re-extracts its entity.
4. **The frame gate.**
   - Pass/fail uses section 9's limits: no frame over 33 ms in the wood walk, and the overview within 3 ms.
   - The wood walk's and the tactical view's mean frame times are reported against before, not gated. The user
     chose 5–6M more triangles knowing the cost.

## Global Constraints

**When it runs.**
- After the asphalt and cordon wall plan (`docs/superpowers/plans/2026-11-23-asphalt-wall-textures.md`), on the
  branch then checked out, which is the user's decision 4.
- So it runs after phase 1, including Task 11 and the main merge:
  - `canopy::Species` is `Small`, `Birch`, `Dead`, in that order;
  - the world description lives in `doc/world-reference.md`, indexed from `CLAUDE.md`.

**Dependencies.**
- Bevy 0.20.1 only; `Cargo.toml` is unchanged.
- Game code stays `wasm32-unknown-unknown` compatible.

**Checks and code style.**
- `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test` and `git diff --check` pass at every commit.
- No `unwrap()` in game logic. Systems are named `verb_noun`; split a system past about 60 lines.

**Precalculate once.**
- Every tier is baked offline.
- Each runtime GLB ends with:
  1. `python3 tools/compress_glb_textures.py`;
  2. `python3 ~/Projects/agentic/agentic-tooling/media/glb-bake-tangents/glb_bake_tangents.py` (tiers with a normal
     map);
  3. `npx @gltf-transform/cli validate`.

**Preloading.** Every tier path is a constant in `src/assets/mod.rs` and is preloaded by `preload_canopies`, or the
splash turns Ready before it exists.

**Sources and asset locations.**
- The Blender-readable sources are `~/Projects/agentic/RoadSide-Picnic/models/trees/uhd/tree_{small,birch,dead}.glb`.
  They have the same vertices and frame as the game's `uhd/` copies (bounds checked on 2026-11-23), so every tier
  sits at the identity transform under its tree, as the UHD child does.
- Before calling an asset missing, search the assets workspace `~/Projects/agentic/RoadSide-Picnic`.
- Uncompressed intermediates go to the workspace, `models/trees/tiers/`, never into the game repo.
- The workspace's existing `models/trees/low/` files (162–250 untextured triangles) are unrelated. Leave them alone.
- Retired runtime files go into `~/Projects/agentic/RoadSide-Picnic/archived/roadside-picnic-unused-assets-2026-11-22.zip`:
  - add them with `zip -u` from the repo root, keeping their `assets/...` paths;
  - append their `sha256sum` lines to `MANIFEST.txt` under `Added 2026-11-23 at commit <sha>: <what>`;
  - update `MANIFEST.txt` inside the zip, then verify with `unzip -l`.

**agentic-tooling (Task 2).**
- The tool lives in `~/Projects/agentic/agentic-tooling/media/glb-lod-bake/`, with a README whose front matter
  (`name`, `description`, `tags`, `usage`) matches `media/glb-preview/README.md`. List it in the repo README.
- Test it before committing.
- Commit staged by path (never `git add -A`), message `glb-lod-bake: <summary>`, then `git push origin main` right
  after. Leave other sessions' changes alone.
- The controller reviews it with `git -C ~/Projects/agentic/agentic-tooling diff BASE..HEAD`.

**Blender.** `BLENDER=~/.cache/blender/blender-4.5.16-linux-x64/blender`. Run
`"$BLENDER" -b --factory-startup --python-exit-code 1 --python <script> -- <args>`, and fail on any Blender Python
error.

**Playtests.**
- Set `XDG_DATA_HOME=$PWD/target/playtest/data`.
- Run Xephyr at 1280x720 on a free display: check `ls /tmp/.X11-unix/`, never use `:5`, `:7` if free. Keep its pid,
  and stop only the pids you started.
- Look with `cargo run --profile playtest`. Take numbers only from
  `cargo build --release --features bevy/trace_chrome --target-dir target/trace`.
- Xephyr caps the game at about 27 fps, so judge CPU spans and GPU pass times, not FPS. Use `LC_ALL=C`.

**Commits.**
- One per task, staged by path.
- Subject prefixed with `git branch --show-current`, imperative; body at most 10 lines.
- Last line `Claude-Session: https://claude.ai/code/session_013NVSdF8r182CK5kaKAUHhv`.
- Never push the game repo.

**Docs.** `doc/world-reference.md` (Scenery bullet, Asset tiers), the `CLAUDE.md` index and `arch-readme.md` change
in the task that changes the behaviour they describe.

## Shared definitions

**Tiers.** `<s>` is `small`, `birch` or `dead`.

| Tier | Triangles | Maps (px) | Game path under `assets/models/trees/` |
|---|---|---|---|
| near | 120,000 | colour, normal, roughness at 2048 | `medium/tree_<s>.glb` (replaces the crown card) |
| mid | 15,000 | colour, normal, roughness at 1024 | `medium/tree_<s>_mid.glb` |
| far | 2,000 | colour, normal, roughness at 512 | `low/tree_<s>.glb` |
| speck | 250 | colour at 256 | `low/tree_<s>_speck.glb` |

**Bands.**
- A tree's height on screen at distance `d` is `1304 · height / d` pixels: a 45° vertical view at 1080p.
- `height` is the mean of the species' `TreeModel.scale` range.
- Where each tier hands over:
  - near → mid at 400 px;
  - mid → far at 90 px;
  - far → speck at 25 px.
- So `band(height, px) = 1304 · height / px`. In metres:

| Species | near ends | mid ends | far ends |
|---|---|---|---|
| small | 20 | 89 | 321 |
| dead | 39 | 174 | 626 |
| birch | 41 | 181 | 652 |

**Fades.**
- A tier that ends at `b` fades out over `b..1.1·b`, and the next coarser tier fades in over the same range.
- The UHD scan keeps 8..12 m.

**Distance.**
- Every tier measures to the tree's middle, `pose.transform_point(Vec3::Y * 0.5)`, the point `scan_distance` uses
  today.
- Every tier's `VisibilityRange` has `use_aabb: true`. The tiers' AABBs share the scan's centre to within 3% of its
  height.

**Handover rule** (the spec's `tier_margins`).
- A tier's start margin is the end margin of the next finer tier that exists. The UHD scan counts once it has been
  revealed.
- Otherwise it is `0..0`, so a tree never vanishes while a tier is missing or still spawning.

**Child streaming.**
- A near or mid child loads within `1.1·b + TIER_LEAD`, where `TIER_LEAD` is 4 m.
- It drops past that plus `TIER_SLACK`, 6 m.

**Shadows.**
- Bevy applies visibility ranges in the shadow views too (`bevy_light-0.20.1/src/lib.rs:441`), so a tier outside its
  range casts nothing.
- Near and mid children therefore cast whenever they are within reach:
  - they start within `SHADOW_ENTER` (48 m) of the camera or the `ViewFocus`;
  - they stop past `SHADOW_EXIT` (55 m) of both.
- The root never casts.

## Review Focus

1. **A tree vanishes or doubles at a handover** while a child is missing or still spawning. Pinned by Task 4's
   `tiers_hand_over_without_a_gap_in_every_combination` and Task 5's `a_new_child_takes_over_in_the_frame_it_spawns`.
2. **Zooming in from the overview spawns hundreds of children at once and stutters.** Pinned by Task 5's
   `children_spawn_at_most_150_per_frame_nearest_first` and Task 8's overview trace.
3. **Zooming out swaps every root to a speck in one frame, or a tree flickers between far and speck at its band.**
   Pinned by Task 4's `speck_swaps_have_hysteresis` and `speck_swaps_stop_at_the_cap`.
4. **The bake tears a crown along the scan's UV seams, or leaves shards in the far and speck tiers.** Pinned by
   Task 2's `test_welds_seams_before_decimating` and Task 3's checkpoint A sheet.
5. **A felled tree loses its tiers, or keeps children it should drop.** Pinned by Task 5's
   `a_felled_tree_keeps_its_children_and_streams_them`.

---

### Task 1: Before numbers and screenshots

**Files:**
- Create, not committed, under `target/playtest/trees/`:
  - `run_scenario.sh`, `quiet.sh`, `analyze.py`, `slow_frames.py`;
  - `poses.sh`, `gpu-timings.patch`;
  - `before.txt` and `before-*.png`.

**Interfaces:**
- Produces:
  - `run_scenario.sh NAME "xinput steps" WRAPPER [game args]`: the contract of
    `target/playtest/phase1/followup/run_scenario.sh`, writing into `target/playtest/trees/`;
  - `poses.sh PREFIX`: the screenshots below;
  - `gpu-timings.patch`: applied with `git apply` before a measured build and reverted with `git apply -R` after.
  Task 8 re-runs all three unchanged.

- [ ] **Step 1: Copy the phase 1 runners.** Copy the four scripts from `target/playtest/phase1/followup/`:
  - point their output directory at `target/playtest/trees/`;
  - make the X display a parameter (`DISPLAY_NUM`, default `:7`).

- [ ] **Step 2: Write `gpu-timings.patch`.**
  - It adds `bevy::render::diagnostic::RenderDiagnosticsPlugin` in `main.rs`, plus an `Update` system that, once a
    second, logs `info!("gpu {path} {avg:.3} ms")` for every diagnostic whose path ends in `elapsed_gpu`.
  - The gate reads `render/main_opaque_pass_3d/elapsed_gpu` (opaque and alpha-mask draws) and
    `render/per_view_shadow_pass/elapsed_gpu`.
  - Check that the patch applies and builds, then revert it. It is never committed.

- [ ] **Step 3: Define the three scenarios.** Record their steps in `before.txt`.
  - **wood:** the start save `target/playtest/phase1/wood.progress.txt`, then
    `"sleep:1 key:space:0.2 sleep:16 scroll:40 sleep:2 down:w sleep:30 up:w sleep:2"`.
  - **tactical:** the same start save, then:
    - zoom out to 90 m with `scroll:-K`. The orbit distance is multiplied by 0.84 per notch (`camera.rs`), so
      `K = round(ln(90 / start) / ln(1/0.84))`, with the start distance read from `camera.rs`;
    - four `drag:3:300:0:2` orbits with `sleep:2` between them;
    - a roam: `down:Up sleep:6 up:Up down:Left sleep:4 up:Left sleep:2`.
  - **overview:** `--new`, then `"sleep:1 key:space:0.2 sleep:16 scroll:-60 sleep:20 scroll:27 sleep:10"`. This is
    out to the 4,600 m limit, then back in to about 40 m (4600 · 0.84^27), which crosses every band both ways.

- [ ] **Step 4: Run each scenario twice.**
  - With the trace build: main-app frame max, p99 and mean over its play window, and peak VRAM less the desktop.
    For the overview, also over the zoom-out and zoom-in windows on their own.
  - With the trace build plus `gpu-timings.patch`: the mean and the largest one-second value of opaque plus shadow
    `elapsed_gpu` over the same windows.
  - Run `quiet.sh` before every run.
  - Write `before.txt` in the layout of `target/playtest/phase1/after.txt` §1, and delete the traces.

- [ ] **Step 5: Take the before screenshots.** `poses.sh before` with the playtest build:
  - fpv, looking at a birch and at a dead tree at about 5, 15, 30, 45 and 60 m (edit `progress.txt`'s `stalker`
    line to place the stalker);
  - the tactical view at four orbit angles;
  - the overview.
  - Name them `before-{birch,dead}-{5,15,30,45,60}m.png`, `before-tactical-{0,90,180,270}.png` and
    `before-overview.png`.

  No commit: everything here is under `target/`.

### Task 2: `media/glb-lod-bake` (agentic-tooling)

**Files:**
- Create in `~/Projects/agentic/agentic-tooling/media/glb-lod-bake/`:
  - `glb_lod_bake.py` (Blender script);
  - `test_glb_lod_bake.py`;
  - `README.md`.
- Modify: agentic-tooling `README.md` (tool list).

**Interfaces:**
- Produces:
  `"$BLENDER" -b --factory-startup --python-exit-code 1 --python glb_lod_bake.py -- SOURCE.glb OUT.glb --triangles N --size PX [--maps color,roughness,normal] [--extrusion F] [--max-ray F]`.
  - `OUT.glb` has one mesh, one primitive and one material, in the source's exact frame: no recentring, no
    grounding.
  - Its PNG maps are at `PX`:
    - baseColor;
    - metallicRoughness (G roughness, B metallic 0), if `roughness` is listed;
    - a tangent-space normal texture, if `normal` is listed.
  - `--maps` defaults to all three.
  - `--extrusion` (cage) and `--max-ray` default to 1% and 2% of the source's AABB diagonal.
  - It exits non-zero, with a message, if the source has more than one material or no UVs.

- [ ] **Step 1: Write the failing tests.** `test_glb_lod_bake.py` (unittest) has Blender build its own source in a
  temp dir:
  - a UV sphere, subdivided and displaced into bumps;
  - its UVs split along a seam, so the seam's vertices are duplicated as a scan's are;
  - one checker-textured material.
  It runs the tool with `--triangles 500 --size 256` and asserts:
  - `test_triangle_count_meets_the_target`: at most 525 triangles;
  - `test_stays_in_the_source_frame`: every AABB coordinate within 1% of the source's;
  - `test_one_primitive_and_the_requested_maps`: three maps by default; with `--maps color`, only baseColor;
  - `test_normal_map_carries_the_bumps`: the normal image's red channel has a standard deviation above 0.02;
  - `test_welds_seams_before_decimating`: the output's triangles form one connected piece by shared vertex
    positions. Unwelded, the seam tears.

- [ ] **Step 2: Run them.** `python3 -m unittest test_glb_lod_bake -v` fails because the script is missing.

- [ ] **Step 3: Implement `glb_lod_bake.py`** (spec section 6.5):
  1. Import the source. Weld a copy with merge-by-distance at 1e-5 of the AABB diagonal.
  2. Collapse-decimate the copy to `N / its triangles`.
  3. Join it into one mesh with one material.
  4. Unwrap the copy anew: Smart UV Project, island margin 2 px at `PX`.
  5. Bake from the source onto the copy with Cycles, selected-to-active:
     - passes: `DIFFUSE` colour only, `ROUGHNESS` and a tangent-space `NORMAL`, each only if listed;
     - margin 4 px;
     - cage extrusion and max ray distance from the arguments.
  6. Export glTF binary with PNG images, and print the bake time of each pass.

- [ ] **Step 4: Run the tests** and see them pass.

- [ ] **Step 5: Write the README** with front matter. In `usage`, note that a 1.9M-triangle scan takes minutes per
  pass.

- [ ] **Step 6: Commit and push** in agentic-tooling: `glb-lod-bake: cut a textured lighter tier from a dense scan`.

### Task 3: `tools/build_tree_tiers.py`, the bake, and checkpoint A

**Files:**
- Create: `tools/build_tree_tiers.py` and `tools/test_build_tree_tiers.py`.
- Replace, uncommitted until Task 7: `assets/models/trees/medium/tree_{small,birch,dead}.glb`.
- Create, uncommitted until Task 7: `assets/models/trees/medium/tree_<s>_mid.glb` and
  `assets/models/trees/low/tree_<s>{,_speck}.glb`.
- Create, not committed: `target/playtest/trees/checkpoint-a.png`.

**Interfaces:**
- Consumes: `glb_lod_bake.py` (Task 2).
- Produces: `python3 tools/build_tree_tiers.py [--species small birch dead] [--tiers near mid far speck]`. It writes
  the game paths of the **Tiers** table:
  - bakes to the workspace `models/trees/tiers/tree_<s>_<tier>.glb`;
  - copies to the game path, then runs the finishing chain. The tangent bake is skipped for the speck, which has no
    normal map.

- [ ] **Step 1: Write the failing tests.** `test_build_tree_tiers.py`:
  - `test_tier_table_matches_the_spec`: the four tiers' triangles, map size and maps, as in **Tiers**;
  - `test_outputs_land_where_the_game_loads_them`: the twelve game paths;
  - `test_speck_bakes_colour_only_and_skips_tangents`.
  Run `python3 -m unittest tools/test_build_tree_tiers.py`; it fails.

- [ ] **Step 2: Implement `build_tree_tiers.py`,** with the tier table as data and one loop. Then run the tests and
  see them pass.

- [ ] **Step 3: Archive the crown cards before the first overwrite.**
  - Add `assets/models/trees/medium/tree_{small,birch,dead}.glb` to the archive zip with their MANIFEST lines, under:
    `Added 2026-11-23 at commit <HEAD>: tree crown cards (medium), replaced by geometry tiers baked from the scans`.
  - Verify with `unzip -l`.

- [ ] **Step 4: Bake.** `python3 tools/build_tree_tiers.py`: birch and dead first, then small.
  - Record per output: triangles, map sizes, bytes and bake time.
  - Check that `validate` passes on all twelve.
  - Until Task 4 lands, the game draws the 120k near tier where the crown cards were, out to 250 m. Do not run it.

- [ ] **Step 5: Checkpoint A, the shape sheet.**
  - Render each tier of the birch and the dead tree with agentic-tooling `media/glb-preview` (`front` and `top`):
    - at the pixel height where it takes over: near 400, mid 90 and far 25 px. Render the speck at 25 px as well;
    - beside the scan and today's crown card (the archived copy) at the same size.
  - Assemble one `checkpoint-a.png` with Pillow.
  - **If the far or speck tier comes out tattered** (shards, or holes in the crown's outline):
    - add `--voxel SIZE` to `glb_lod_bake.py`: voxel-remesh the welded scan into a closed hull before decimating;
    - add a test that the hull comes out closed;
    - re-bake those tiers.
    - Report which tiers needed it.
  - The controller looks at the sheet and sends it to the user (`SendUserFile`) as information. The run does not
    stop here.

- [ ] **Step 6: Commit** `tools/build_tree_tiers.py`, its test, and any `--voxel` change to agentic-tooling (its own
  commit there). The assets stay uncommitted until the user's go (Task 7).

### Task 4: The root draws the far tier and swaps to the speck

**Files:**
- Create: `src/world/canopy/tiers.rs`.
- Modify:
  - `src/world/canopy.rs`:
    - `TreeModel` gets `tiers: [&'static str; 4]` in place of `path`, and loses `draw_distance`;
    - `CanopyModels`, `preload_canopies`, `prepare_canopy_materials` and `spawn_canopies`;
    - `CloseScan` is replaced by `Tiers`, in `spawn_close_scan`, `reveal_close_tree` and `stream_close_trees`;
  - `src/assets/mod.rs`: the tier path constants;
  - `src/world/mod.rs`: registration.
- Docs: the Scenery bullet in `doc/world-reference.md`.

**Interfaces:**
- Produces, in `canopy/tiers.rs`:
  - `pub(super) enum Tier { Near, Mid, Far, Speck }`;
  - `CanopyModels([[(Handle<Mesh>, Handle<StandardMaterial>); 4]; 3])`, indexed `[species][tier]`. Materials are
    made with `standard_material_from_gltf_material`, as today;
  - `pub(super) struct Bands { near_end: f32, mid_end: f32, far_end: f32 }` and
    `pub(super) fn bands(species: Species) -> Bands`, from **Bands**;
  - `pub(super) fn end_margin(end: f32) -> Range<f32>`, which returns `end..end * 1.1`;
  - `#[derive(Component, Default)] pub(super) struct Tiers { near: Option<Entity>, mid: Option<Entity>, close: Option<Entity>, close_shown: bool, speck: bool }`;
  - `pub(super) struct StartMargins { root: Range<f32>, mid: Range<f32>, near: Range<f32> }` and
    `pub(super) fn tier_margins(tiers: &Tiers, bands: &Bands) -> StartMargins`, from **Handover rule**;
  - constants:
    - `NO_END = 1.0e6`, the root's end margin;
    - `SPECK_HYSTERESIS = 0.1`;
    - `SPECK_SWAPS_PER_FRAME = 2_000`;
  - systems:
    - `fade_tiers`: on `Changed<Tiers>`, applies `tier_margins` to the root and to existing children;
    - `swap_specks`:
      - a root becomes the speck past `far_end · 1.1`, and the far tier again within `far_end`;
      - it swaps both `Mesh3d` and `MeshMaterial3d`, and at most `SPECK_SWAPS_PER_FRAME` roots a frame.

- [ ] **Step 1: Write the failing tests** in `canopy/tiers.rs`. Move the `streaming_with` harness there from
  `canopy.rs`.
  - `bands_follow_the_tree_height_on_screen`: each species' three ends within 1 m of the **Bands** table.
  - `tiers_hand_over_without_a_gap_in_every_combination`. For all 8 combinations of near, mid and `close_shown`:
    - each start margin equals the end margin of the next finer tier that exists;
    - with no finer tier, it is `0..0`.
  - `a_tree_root_draws_the_far_tier_with_no_end`: after `spawn_canopies`, the root has the far mesh and material,
    `VisibilityRange { start 0..0, end NO_END..NO_END, use_aabb: true }`, `NotShadowCaster` and `Tiers`.
  - `speck_swaps_have_hysteresis`, for a birch (far ends at 652 m):
    - a root at 1.05 · 652 m stays far;
    - at 1.15 · 652 m it becomes the speck;
    - back at 1.05 · 652 m it stays the speck;
    - at 0.95 · 652 m it is far again.
  - `speck_swaps_stop_at_the_cap`: 2,500 roots past the band, 2,000 swap in one update and the rest in the next.
  - `a_revealed_scan_sets_the_roots_start`: with no children, revealing the UHD scan sets the root's start margin to
    `8..12`. This keeps today's behaviour, now through `Tiers`.
  - The existing close-scan tests, reading `Tiers` instead of `CloseScan`.

- [ ] **Step 2: Run them.** `cargo test world::canopy` fails to compile.

- [ ] **Step 3: Implement.**
  - `spawn_canopies` gives each root:
    - the far tier, its pose and `CanopyVisual`;
    - `Tiers::default()` and `NotShadowCaster`;
    - the visibility range above.
  - Trees stop using `detail_range` and `STRATEGY_REACH`.
  - `reveal_close_tree` sets `Tiers.close_shown`; the despawn branch of `stream_close_trees` clears `close` and
    `close_shown`; `fade_tiers` does the rest.
  - Order `swap_specks` and `fade_tiers` after `stream_close_trees`, in one `.chain()`.

- [ ] **Step 4: Run all the checks.** `cargo test` passes, then fmt and clippy.

- [ ] **Step 5: Look check** (playtest build).
  - The overview shows every wood as trees.
  - The tactical view shows far tiers (mid comes in Task 5).
  - Zooming out and back in shows no far-to-speck flicker.
  - Save `target/playtest/trees/task4-{overview,tactical}.png`.

- [ ] **Step 6: Docs and commit.**
  - `doc/world-reference.md`'s Scenery bullet: trees show at every zoom; the root draws the far tier, then the speck.
  - Commit.

### Task 5: Near and mid children, and tree shadows

**Files:**
- Modify:
  - `src/world/canopy/tiers.rs`: `stream_tree_tiers` and `cast_tree_shadows`;
  - `src/world/canopy.rs`: `update_canopy_motion` loses its shadow toggle; `SHADOW_ENTER` and `SHADOW_EXIT` move to
    `tiers.rs`;
  - `src/world/mod.rs`: registration.
- Docs: `doc/world-reference.md` Scenery bullet, and the tree streaming path in `arch-readme.md`.

**Interfaces:**
- Consumes: `Tiers`, `bands`, `end_margin`, `tier_margins`, `fade_tiers`, `CanopyModels` (Task 4).
- Produces:
  - `#[derive(Component)] pub(super) struct TreeTier(Tier)`, on each near or mid child;
  - constants `TIER_LEAD = 4.0`, `TIER_SLACK = 6.0` and `TIER_SPAWNS_PER_FRAME = 150`;
  - `stream_tree_tiers`:
    - runs over every entity with `Tiers`, felled trees included;
    - spawns missing near and mid children inside their load distance, nearest first, at most 150 a frame;
    - despawns those past their drop distance, and records both in `Tiers`;
  - `cast_tree_shadows`: the **Shadows** rule, from the camera and the `ViewFocus` positions.

- [ ] **Step 1: Write the failing tests:**
  - `children_spawn_within_the_load_distance_and_drop_past_the_drop_distance`: camera moves give spawn, keep (between
    load and drop) and despawn, for both near and mid.
  - `children_spawn_at_most_150_per_frame_nearest_first`: 200 trees in range spawn 150 in the first update, the 150
    nearest, and the rest in the next.
  - `a_new_child_takes_over_in_the_frame_it_spawns`: after the update that spawns a mid child, the root's start
    margin is the mid's end margin.
  - `a_felled_tree_keeps_its_children_and_streams_them`. After `break_selected_trees`' swap of `CanopyVisual` for
    `FallingTree`:
    - its children remain;
    - moving the camera past the drop distance despawns them;
    - coming back spawns them again.
  - `near_and_mid_cast_shadows_near_the_camera_or_the_focus`:
    - a child 40 m from the camera but 10 m from the focus casts;
    - at 60 m from both it does not;
    - one at 50 m keeps its previous state (48/55 hysteresis);
    - the root never casts.

- [ ] **Step 2: Run them.** `cargo test world::canopy::tiers` fails.

- [ ] **Step 3: Implement.**
  - A child is spawned with:
    - `Mesh3d` and `MeshMaterial3d` from `CanopyModels`;
    - `Transform::IDENTITY`, `ChildOf(tree)`, `TreeTier`, `NotShadowCaster`;
    - `VisibilityRange { start_margin: from tier_margins, end_margin: end_margin(its band end), use_aabb: true }`.
  - It shows in the frame it spawns, because the tiers are preloaded.
  - Chain `stream_tree_tiers` before `fade_tiers`.

- [ ] **Step 4: Run all the checks.** `cargo test` passes, then fmt and clippy.

- [ ] **Step 5: Look check** (playtest build).
  - Walk the wood from Task 1. No tree blinks at 12 m or at its band ends.
  - Zoom from the overview into the tactical view: no stutter you can see.
  - At the tactical zoom, trees around the focus cast shadows.
  - Save `target/playtest/trees/task5-birch-{10,30,60,150}m.png` and `task5-tactical.png`.

- [ ] **Step 6: Docs and commit.**
  - `doc/world-reference.md`: the four tiers with their bands, and the shadow rule.
  - `arch-readme.md`: the tree streaming path.
  - Commit.

### Task 6: Checkpoint B (stops for the user)

**Files:**
- Possibly modify: `tools/build_tree_tiers.py` (triangle counts, per species if needed), `UHD_RANGE`.
- Create, not committed: `target/playtest/trees/checkpoint-*.png` and `checkpoint-orbit.gif`.

- [ ] **Step 1: Tune against the spec's section 10 risks.** Record every try.
  - **The 12–20 m handover** shows a smoother crown or a faceted outline: raise the near tier up to 200k, or
    `UHD_RANGE` to 15 m.
  - **The dead tree's twigs** thin out in the mid tier: a higher mid count for the dead tree alone.
  - **Dark blotches** in a normal bake: a shorter `--max-ray`, or bake onto the voxel hull.

- [ ] **Step 2: Shoot the checkpoint set.** `poses.sh checkpoint`, plus:
  - the UHD scan of each species at 5 m;
  - a 4-second right-drag orbit at the tactical zoom, captured as frames and turned into `checkpoint-orbit.gif` with
    agentic-tooling `media/video-to-gif`.
  - Pair each shot with its `before-*` twin.

- [ ] **Step 3: STOP.** The implementer reports DONE with the shot list and the tuned values. The controller then:
  - sends the shots to the user;
  - asks for the go, naming each tier's triangle count per species and `UHD_RANGE`;
  - waits.
  On a no, the user's notes become the next round of Step 1. There is no commit in this task, unless tuning changed
  code: then commit that alone.

### Task 7: After the go: the assets and the docs

**Files:**
- Commit: the twelve tier GLBs.
- Docs: `doc/world-reference.md` (Asset tiers, the bake), the `CLAUDE.md` index, `arch-readme.md` (a recipe:
  re-baking a tree's tiers).

- [ ] **Step 1: Re-bake** whatever Task 6 changed since the last bake, the small tree included.
- [ ] **Step 2: Check.** `cargo test`, then a playtest look at a small tree at 10, 30, 60 and 150 m.
- [ ] **Step 3: Check LFS.** `git check-attr filter` on every new GLB prints `lfs`. Report the total bytes added.
- [ ] **Step 4: Docs.**
  - Asset tiers: `trees/medium/` holds the near and mid tiers, `trees/low/` the far and speck.
  - Describe `glb-lod-bake` and `tools/build_tree_tiers.py`.
  - The crown cards are in the archive zip.
- [ ] **Step 5: Commit** the assets and docs.

### Task 8: After numbers and the gate

**Files:**
- Create, not committed: `target/playtest/trees/after.txt` and `after-*.png`.
- Possibly modify: `src/world/canopy/tiers.rs` (the near shadow range), `src/world/scenery.rs` (`WOOD_DENSITY`).

- [ ] **Step 1: Re-run Task 1's scenarios unchanged,** both builds, and `poses.sh after`.
- [ ] **Step 2: Judge the gate,** writing `after.txt` beside `before.txt`:
  - **wood:**
    - no main-app frame over 33 ms;
    - no one-second mean of opaque plus shadow `elapsed_gpu` over 33 ms.
  - **overview:** opaque plus shadow `elapsed_gpu` within before + 3 ms. Also no main-app frame over 33 ms in the
    zoom windows.
  - **Reported, not gated:** the wood and tactical means against before, peak VRAM against before, and the tree
    triangles in view against the spec's section 6.2 estimates.
- [ ] **Step 3: If the gate fails,** apply the spec's corrected order:
  1. Near children cast shadows only within 30 m (enter 30, exit 35); re-run.
  2. Thin `WOOD_DENSITY` in steps of 0.05 from 0.85, re-running the wood scenario each time, down to 0.70.
  3. If 0.70 still fails, stop and report. A cheaper near tier is the user's call.
- [ ] **Step 4: Commit** any change, with the gate's numbers in the body.
