# Tree Impostors and Near Tiers Implementation Plan

> **Superseded on 2026-11-23. Do not execute.** The user chose geometry tiers ("I choose Geometry tiers"). The
> plan to run is `docs/superpowers/plans/2026-11-23-tree-geometry-tiers.md`.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every tree looks like its UHD scan at every zoom, out to the overview:
- the scan within 12 m;
- a near tier decimated from the scan, with the scan's detail baked onto it, out to about 55 m;
- past that, a two-triangle octahedral impostor baked from the scan.

**Architecture:**
- **Bake.** Two generic Blender tools in agentic-tooling bake each species from its workspace scan:
  - `glb-lod-bake` makes the near GLB;
  - `glb-impostor-bake` renders 12 × 12 hemi-octahedral frames into a colour atlas and a normal-and-depth atlas.
  - `tools/build_tree_tiers.py` runs both for the three species, compresses the outputs and generates the bake
    constants as Rust.
- **Runtime.**
  - The map-wide `CanopyVisual` root draws its species' impostor: one shared quad, an `ExtendedMaterial`, per-pixel
    PBR lighting from the atlases.
  - A near child streams in by camera distance; the UHD child is unchanged.
  - The three tiers crossfade through `VisibilityRange` margins, all measured to the tree's middle.

**Tech Stack:**
- Bevy 0.20.1 (`ExtendedMaterial`, `VisibilityRange`, WESL), Rust 1.100;
- headless Blender 4.5 (Cycles), Python 3 with numpy;
- `ctt` (KTX2 BC7), glTF Transform validate.

**Spec:** `docs/superpowers/specs/2026-11-23-tree-impostors-design.md`. It amends the "Tree tiers" of
`docs/superpowers/specs/2026-11-23-vegetation-lod-design.md`; read both.

**Where this plan departs from the spec, and why:**
1. **The scans' normal maps.** The spec says the scans have no normal map. Each workspace scan does carry one,
   `tree_*_normal.png`. The near bake samples the imported scan, whose material the Blender importer wires to that
   map, so the near tier keeps it.
2. **When the small tree is baked.** The spec bakes it only after the user's go. The game cannot load with a species'
   files missing, so Task 4 bakes it with the others. Task 8 re-bakes it with whatever settings the checkpoint
   settles. Assets are still committed only after the go.
3. **The normal-and-depth atlas preset.** It uses `ctt`'s "data" preset (linear BC7). The "normal" preset is BC5,
   which drops the alpha channel that holds the depth.

## Global Constraints

**When it runs.**
- After the asphalt and cordon wall plan (`docs/superpowers/plans/2026-11-23-asphalt-wall-textures.md`), on the
  branch then checked out.
- So it runs after phase 1, including Task 11 and the main merge:
  - `canopy::Species` is `Small`, `Birch`, `Dead`, in that order;
  - the world description lives in `doc/world-reference.md`, indexed from `CLAUDE.md`.
- The "before" numbers include everything before it.

**Dependencies.**
- Bevy 0.20.1 only; `Cargo.toml` is unchanged.
- Game code stays `wasm32-unknown-unknown` compatible.

**Checks and code style.**
- `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test` and `git diff --check` pass at every commit.
- No `unwrap()` in game logic. Systems are named `verb_noun`; split a system past about 60 lines.

**Precalculate once.**
- Everything is baked offline. The runtime only picks and blends frames.
- Atlases ship as KTX2 with full mips, encoded by `ctt` with `tools/compress_glb_textures.py`'s `ctt_arguments`:
  - the colour-and-coverage atlas uses role `"color"` (sRGB BC7, alpha kept);
  - the normal-and-depth atlas uses the data preset (any other role: linear BC7, perceptual off).
- Near GLBs end with:
  1. `python3 tools/compress_glb_textures.py`;
  2. `python3 ~/Projects/agentic/agentic-tooling/media/glb-bake-tangents/glb_bake_tangents.py`;
  3. `npx @gltf-transform/cli validate`.

**Preloading.** Every new runtime path is a constant in `src/assets/mod.rs` and is preloaded in `Startup`
(`preload_canopies`), or the splash turns Ready before it exists.

**Sources and asset locations.**
- The Blender-readable sources are `~/Projects/agentic/RoadSide-Picnic/models/trees/uhd/tree_{small,birch,dead}.glb`.
  The game's own `uhd/` copies hold KTX2 textures, which Blender cannot read.
- Before calling any asset missing, search the assets workspace `~/Projects/agentic/RoadSide-Picnic`.
- Intermediates go to the workspace, never into the game repo:
  - near GLBs before compression: `models/trees/near/`;
  - atlas PNGs and `impostor.json`: `textures/trees/impostors/<name>/`.
- Retired runtime files go into `~/Projects/agentic/RoadSide-Picnic/archived/roadside-picnic-unused-assets-2026-11-22.zip`:
  - add them with `zip -u` from the repo root, keeping their `assets/...` paths;
  - append their `sha256sum` lines to `MANIFEST.txt` under `Added 2026-11-23 at commit <sha>: <what>`;
  - update `MANIFEST.txt` inside the zip, then verify with `unzip -l`.

**agentic-tooling (Tasks 2 and 3).**
- New tools live in `~/Projects/agentic/agentic-tooling/media/<tool>/`, each with a README whose front matter
  (`name`, `description`, `tags`, `usage`) matches `media/glb-bake-tangents/README.md`. List them in the repo's
  README.
- Test a tool before committing it.
- Commit staged by path (never `git add -A`), message `<tool-name>: <summary>`, then `git push origin main` right
  after. Leave other sessions' changes alone.
- The controller reviews these tasks with `git -C ~/Projects/agentic/agentic-tooling diff BASE..HEAD`.

**Blender.** `BLENDER=~/.cache/blender/blender-4.5.16-linux-x64/blender` (the `blender` skill). Run
`"$BLENDER" --version` first, and fail on any Blender Python error.

**Playtests.**
- Set `XDG_DATA_HOME=$PWD/target/playtest/data`.
- Run Xephyr at 1280x720 on a free display: check `ls /tmp/.X11-unix/`, never use `:5`, `:7` if free. Keep its pid,
  and stop only the pids you started.
- Look with `cargo run --profile playtest`. Take numbers only from
  `cargo build --release --features bevy/trace_chrome --target-dir target/trace`.
- Xephyr caps the game at about 27 fps, so the gate judges CPU spans and GPU pass times, not FPS. Use `LC_ALL=C`.

**Commits.**
- One per task, staged by path.
- Subject prefixed with `git branch --show-current`, imperative; body at most 10 lines.
- Last line `Claude-Session: https://claude.ai/code/session_013NVSdF8r182CK5kaKAUHhv`.
- Never push the game repo.

**Docs.** `doc/world-reference.md` (Scenery bullet, Asset tiers), the `CLAUDE.md` index and `arch-readme.md` change
in the task that changes the behaviour they describe.

## Shared definitions

The bake (Python) and the shader (WGSL, mirrored in Rust) must agree on these exactly. Every vector is in the
tree's glTF model frame: +Y up, the trunk's foot at the origin, about one unit tall.

**Hemi-octahedral encode.** A view direction `d` (unit, from the tree toward the camera) with `d.y ≥ 0` maps to
`uv ∈ [0,1]²`:
```
p  = d.xz / (|d.x| + |d.y| + |d.z|)
uv = (p.x + p.y, p.x - p.y) * 0.5 + 0.5
```
- The horizon maps to the square's border and the pole to its centre.
- Decode inverts it: `oct = uv*2-1`, `p = ((oct.x+oct.y)/2, (oct.x-oct.y)/2)`, `d = normalize(p.x, 1-|p.x|-|p.y|, p.y)`.
- A direction below the horizon is first clamped: `d.y = max(d.y, 0)`, renormalized. If it comes out zero
  (straight down), use `+Y`.

**Frames.**
- Frame `(i, j)`, with `i, j` in `0..N`, looks from `decode((i, j) / (N-1))`. The borders are included.
- N is even (12), so no frame sits exactly at the pole.
- A view's grid position is `g = encode(d) * (N-1)`.

**Blend.** The three frames around `g`:
- `c = clamp(floor(g), 0, N-2)`, `f = g - c`.
- If `f.x + f.y < 1`, the frames are `c`, `c+(1,0)`, `c+(0,1)` with weights `1-f.x-f.y`, `f.x`, `f.y`.
- Otherwise they are `c+(1,1)`, `c+(1,0)`, `c+(0,1)` with weights `f.x+f.y-1`, `1-f.y`, `1-f.x`.

**Frame basis.** For a frame direction `D`: `R = normalize(cross(+Y, D))` and `U = cross(D, R)`. R is screen-right
and U screen-up for a camera at `C + D` looking at `C`.

**Atlas layout.**
- Frame `(i, j)` covers atlas uv `[i/N, (i+1)/N] × [j/N, (j+1)/N]`. Atlas uv (0, 0) is the top-left texel.
- Inside a frame, a point `P` sits at `u = 0.5 + 0.5·dot(P-C, R)/w`, `v = 0.5 - 0.5·dot(P-C, U)/h`.
- `C` is the centre and `(w, h)` the half size from the bake.

**Atlas channels.**
- Colour atlas: RGB is albedo, A is coverage.
- Normal-and-depth atlas:
  - RGB is the model-frame normal: `n*0.5+0.5`;
  - A is `0.5 + 0.5·dot(P-C, D)/radius`, positive toward the camera.

**Bake constants** (`impostor.json`, and the generated Rust):
- `center` is the scan's AABB centre;
- `radius` is half the AABB's diagonal;
- `half_size` is `(w, h)`, the largest projected extents over all frames plus 2%;
- `grid` is N;
- `frame_px` is `[W, H]`;
- `roughness` is the mean of the scan's roughness over covered texels.

**The tree's middle** is `pose.transform_point(center)`. Every tier measures its camera distance to it.

**One frame for all tiers.** The game's `uhd/` copies have the same vertices and frame as the workspace sources
(bounds checked on 2026-11-23). The UHD child already sits at the identity transform under the root, so the near
child does too, and the impostor uses the same model frame.

## Review Focus

1. **A tree looks flat, ghosts or pops while the tactical camera orbits.** That means frames are chosen or blended
   wrong. Pinned by:
   - Task 5's `blend_is_continuous_across_cell_edges` and `the_mapping_matches_the_bake_table`;
   - the debug-tint screenshots in Task 5 and the orbit sequence in Task 7.
2. **A view from below the tree's horizon** (a falling tree, or a camera below a hillside tree) gives NaNs or a black
   tree. Pinned by Task 5's `a_view_from_below_uses_the_horizon_frames`, which includes straight down.
3. **A tree vanishes for a frame or more at a handoff.** This happens when the impostor fades out before its near
   child exists, or when the near and impostor margins leave a gap. Pinned by Task 6's
   `tiers_hand_over_without_a_gap` and `an_impostor_stays_until_its_near_child_exists`.
4. **A zoom-in or save restore brings hundreds of trees into near range at once and stutters.** Pinned by Task 6's
   `near_tiers_spawn_at_most_the_cap_per_frame_nearest_first`.
5. **Frustum or shroud culling drops a tree whose crown is in view,** because the quad is flat. Pinned by Task 5's
   `an_impostor_carries_its_sphere_aabb`.

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
  Task 9 re-runs all three unchanged.

- [ ] **Step 1: Copy the phase 1 runners.** Copy the four scripts from `target/playtest/phase1/followup/`:
  - point their output directory at `target/playtest/trees/`;
  - make the X display a parameter (`DISPLAY_NUM`, default `:7`).

- [ ] **Step 2: Write `gpu-timings.patch`.**
  - It adds `bevy::render::diagnostic::RenderDiagnosticsPlugin` in `main.rs`, plus a small `Update` system that,
    once a second, logs `info!("gpu {path} {avg:.3} ms")` for every diagnostic whose path ends in `elapsed_gpu`.
  - Those paths include `render/main_opaque_pass_3d/elapsed_gpu` and `render/per_view_shadow_pass/elapsed_gpu`.
    The alpha-mask phase draws inside `main_opaque_pass_3d`, so these two cover the spec's opaque, alpha-mask and
    shadow passes.
  - Check that the patch applies and builds, then revert it. It is never committed.

- [ ] **Step 3: Define the three scenarios.**
  - **wood:** the phase 1 wood walk. Use the start save `target/playtest/phase1/wood.progress.txt` and the steps
    `"sleep:1 key:space:0.2 sleep:16 scroll:40 sleep:2 down:w sleep:30 up:w sleep:2"`.
  - **tactical:** the same start save, then:
    - zoom out to 90 m with `scroll:-K`. The orbit distance is multiplied by 0.84 per notch (`camera.rs`), so
      `K = round(ln(90 / start) / ln(1/0.84))`, with the start distance read from `camera.rs`;
    - then four `drag:3:300:0:2` orbits with `sleep:2` between them;
    - then a roam: `down:Up sleep:6 up:Up down:Left sleep:4 up:Left sleep:2`.
  - **overview:** `--new`, `"sleep:1 key:space:0.2 sleep:16 scroll:-60 sleep:20"`.
  - Record the steps in `before.txt`.

- [ ] **Step 4: Run each scenario twice.**
  - Once with the trace build: CPU frame max, p99 and mean over its play window, and peak VRAM less the desktop.
  - Once with the trace build plus `gpu-timings.patch`: the mean of opaque plus shadow `elapsed_gpu` over the same
    window.
  - Run `quiet.sh` before every run.
  - Write `before.txt` in the layout of `target/playtest/phase1/after.txt` §1, and delete the traces.

- [ ] **Step 5: Take the before screenshots.** `poses.sh before` with the playtest build:
  - fpv, looking at a birch and at a dead tree at about 5, 15, 30, 45 and 60 m (edit `progress.txt`'s `stalker` line
    to place the stalker);
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
  `"$BLENDER" -b -P glb_lod_bake.py -- SOURCE.glb OUT.glb --triangles N --size PX [--extrusion F] [--gpu]`. It writes
  `OUT.glb`:
  - one mesh, one primitive, one material;
  - baseColor and metallicRoughness textures (G roughness, B metallic) and a tangent-space normal texture, all PNG
    at `PX`;
  - vertices in the source's exact frame: no recentring, no grounding.
  - It exits non-zero, with a message, if the source has more than one material or no UVs.

- [ ] **Step 1: Write the failing test.** `test_glb_lod_bake.py` (unittest) builds its own source with Blender: a
  subdivided sphere with displaced bumps, one checker-textured material, saved as a GLB in a temp dir. It runs the
  tool with `--triangles 500 --size 256` and asserts:
  - `test_triangle_count_meets_the_target`: at most 525 triangles;
  - `test_stays_in_the_source_frame`: every AABB coordinate within 1% of the source's;
  - `test_one_primitive_and_three_maps`;
  - `test_normal_map_carries_the_bumps`: the normal image's red channel has a standard deviation above 0.02 (a flat
    bake has 0).

- [ ] **Step 2: Run it.** `python3 -m unittest test_glb_lod_bake -v` fails because the script is missing.

- [ ] **Step 3: Implement `glb_lod_bake.py`.**
  - Import the source and duplicate it.
  - Decimate the copy with `COLLAPSE` at `ratio = N / source triangles`. This keeps the source's UV layout.
  - Bake with Cycles, selected-to-active from the source onto the copy:
    - passes: `DIFFUSE` colour only, `ROUGHNESS` and tangent-space `NORMAL`;
    - margin 16 px;
    - cage extrusion `F`, by default 1% of the AABB diagonal.
  - Export glTF binary with PNG images.

- [ ] **Step 4: Run the test** and see it pass.

- [ ] **Step 5: Write the README** with front matter. In `usage`, note that a 1.9M-triangle scan takes minutes per
  pass.

- [ ] **Step 6: Commit and push** in agentic-tooling: `glb-lod-bake: decimate a scan and bake its detail onto the copy`.

### Task 3: `media/glb-impostor-bake` (agentic-tooling)

**Files:**
- Create in `~/Projects/agentic/agentic-tooling/media/glb-impostor-bake/`:
  - `octahedral.py` (pure Python and numpy, no `bpy`);
  - `test_octahedral.py`;
  - `glb_impostor_bake.py` (Blender);
  - `test_glb_impostor_bake.py`;
  - `README.md`.
- Modify: agentic-tooling `README.md`.

**Interfaces:**
- Produces:
  - `octahedral.py`:
    - `encode(d) -> (u, v)` and `decode(u, v) -> d`;
    - `frame_direction(i, j, n) -> d` and `frame_basis(d) -> (right, up)`;
    - `blend(d, n) -> [((i, j), w)] * 3`;
    - `check_table() -> list[(d, uv)]`, about 24 directions: the pole, the four horizon axes, the four horizon
      diagonals, and fixed pseudo-random directions with `d.y ≥ 0`;
    - `rust_check_table() -> str`, the Rust expression `&[([f32; 3], [f32; 2])]` of that table.
  - `"$BLENDER" -b -P glb_impostor_bake.py -- SOURCE.glb OUT_DIR --grid N --frame WxH [--samples S] [--gpu]` writes
    `OUT_DIR/color.png`, `normal_depth.png` (16-bit) and `impostor.json` (keys as in **Shared definitions**).

- [ ] **Step 1: Write the failing mapping tests.** `test_octahedral.py`:
  - `test_known_directions`:
    - `encode((0,1,0)) == (0.5, 0.5)`;
    - `encode((1,0,0)) == (1, 1)`;
    - `encode((0,0,1)) == (1, 0)`;
    - `encode((-1,0,0)) == (0, 0)`;
    - `encode((0,0,-1)) == (0, 1)`.
  - `test_round_trip`: `decode(encode(d)) ≈ d` within 1e-6 for 1,000 seeded directions.
  - `test_blend_weights_sum_to_one`, and `test_blend_picks_an_exact_frame` (weight 1 on frame (3, 7) at its own
    direction).
  - `test_blend_is_continuous`: for 2,000 seeded pairs of directions 1e-5 apart, every frame's weight changes by
    under 1e-3.
  - `test_below_horizon_clamps`: `blend((x,-y,z)) == blend((x,0,z))`; and `(0,-1,0)` gives finite weights summing to 1.
  - `test_basis_is_orthonormal_and_up_points_up`: `dot(up, +Y) ≥ 0`.

- [ ] **Step 2: Run them.** `python3 -m unittest test_octahedral -v` fails.

- [ ] **Step 3: Implement `octahedral.py`** from **Shared definitions**, then run the tests and see them pass.

- [ ] **Step 4: Write the failing bake test.** `test_glb_impostor_bake.py` builds a source in Blender: a red cube
  over a green cone, 1 unit tall, saved as a GLB. It bakes `--grid 4 --frame 32x32` and asserts:
  - `test_atlas_size_is_grid_times_frame` (128 × 128);
  - `test_coverage_matches_the_silhouette`: the top-view frame's covered fraction is within 5% of the cube's
    projected area over the frame;
  - `test_normals_face_the_camera`: in every frame, the mean decoded normal has a positive dot with the frame's `D`;
  - `test_json_has_the_shared_keys`;
  - `test_dilation_fills_empty_texels`: no texel with coverage 0 has RGB (0, 0, 0) within 8 px of a covered texel.

- [ ] **Step 5: Implement `glb_impostor_bake.py`.**
  - Import the source.
  - Read every vertex with `foreach_get`, converting Blender Z-up to glTF Y-up `(x, z, -y)`. Compute `center`,
    `radius` and `half_size` over all frames.
  - For each frame, render one orthographic Cycles view:
    - the camera at `C + D·2·radius`, along `-D`, up `U`;
    - `ortho_scale = 2·max(w, h)`, resolution `W × H`, transparent film;
    - passes Diffuse Color, Normal and Z.
  - Convert each frame's normals and depth into the model frame, as above.
  - Pack both atlases with numpy.
  - Dilate each frame's empty texels from the covered ones, 16 passes, without changing coverage.
  - Write `impostor.json`, with `roughness` from the source's roughness texture over covered texels.

- [ ] **Step 6: Run both test files** and see them pass.

- [ ] **Step 7: README and commit.** Write the README, then commit and push in agentic-tooling:
  `glb-impostor-bake: render hemi-octahedral impostor atlases from a scan`.

### Task 4: Bake the three species and generate the bake constants

**Files:**
- Create:
  - `tools/build_tree_tiers.py`;
  - `tools/test_build_tree_tiers.py`;
  - `src/world/pillars/tree_impostors.rs` (generated);
  - `src/world/pillars/impostor_mapping_checks.rs` (generated).
- Replace, uncommitted until Task 8: `assets/models/trees/medium/tree_{small,birch,dead}.glb`.
- Create, uncommitted until Task 8: `assets/textures/trees/tree_{small,birch,dead}_{color,normal_depth}.ktx2`.

**Interfaces:**
- Consumes: both tools from Tasks 2 and 3.
- Produces:
  - `tree_impostors.rs`: an expression `[ImpostorBake; 3]` in `Species` order (Small, Birch, Dead). Each entry is
    `ImpostorBake { center: [f32; 3], radius: f32, half_size: [f32; 2], grid: u32, frame_px: [u32; 2], roughness: f32 }`,
    with f32 literals written with a decimal point. Its first line follows the pillar files' convention:
    `// Generated by tools/build_tree_tiers.py from the UHD tree scans; do not edit.`
  - `impostor_mapping_checks.rs`: `octahedral.rust_check_table()` with the same header.
  - `python3 tools/build_tree_tiers.py [--species small birch dead] [--triangles 60000] [--grid 12]` rebuilds the
    named species. It always regenerates both Rust files from all three workspace `impostor.json` files.

- [ ] **Step 1: Write the failing test.** `test_build_tree_tiers.py`:
  - `test_rust_bakes_keep_species_order_and_float_literals`: the formatter given three dicts emits three
    `ImpostorBake {` entries in order, and every float token matches `-?\d+\.\d+`;
  - `test_frame_sizes_follow_the_spec`: birch and dead `[256, 256]`, small `[128, 128]`.
  Run it with `python3 -m unittest tools/test_build_tree_tiers.py`; it fails.

- [ ] **Step 2: Implement `build_tree_tiers.py`.** For each species:
  1. **Near tier.**
     - `glb_lod_bake.py` from the workspace scan to `models/trees/near/<name>.glb`, at `--size 2048`.
     - Copy it to `assets/models/trees/medium/<name>.glb`.
     - Compress, bake tangents and validate (Global Constraints).
  2. **Impostor.**
     - `glb_impostor_bake.py` to `textures/trees/impostors/<name>/`, frame size from the table in Step 1.
     - `ctt` each PNG to `assets/textures/trees/<name>_{color,normal_depth}.ktx2`, with `ctt_arguments` imported from
       `compress_glb_textures.py`.
  3. Regenerate both Rust files.
  Then run the test and see it pass.

- [ ] **Step 3: Archive the crown cards before the first overwrite.**
  - Add `assets/models/trees/medium/tree_{small,birch,dead}.glb` to the archive zip with their MANIFEST lines,
    under: `Added 2026-11-23 at commit <HEAD>: tree crown cards (medium), replaced by near tiers baked from the scans`.
  - Verify with `unzip -l`.

- [ ] **Step 4: Bake the three species.** `python3 tools/build_tree_tiers.py`, birch and dead first.
  - Record each species' near triangle count, atlas sizes, KTX2 bytes and bake time in the report.
  - Check that `validate` passes on the three GLBs.

- [ ] **Step 5: Commit** the two tools and the two generated Rust files only. The assets stay uncommitted until the
  user's go (Task 8).

### Task 5: The impostor material, and the root draws it

**Files:**
- Create: `src/world/canopy/impostor.rs`, `assets/shaders/impostor.wesl`.
- Modify:
  - `src/world/canopy.rs`: `CanopyModels`, `preload_canopies`, `prepare_canopy_materials`, `spawn_canopies`,
    `stream_close_trees`;
  - `src/world/mod.rs`: `MaterialPlugin::<canopy::ImpostorMaterial>`;
  - `src/assets/mod.rs`: six atlas path constants.
- Docs: the Scenery bullet in `doc/world-reference.md`.

**Interfaces:**
- Consumes: the generated Rust files from Task 4.
- Produces, in `canopy/impostor.rs`:
  - `pub(super) struct ImpostorBake { … }`, the fields from Task 4;
  - `pub(super) const BAKES: [ImpostorBake; 3] = include!("../pillars/tree_impostors.rs");`
  - `pub(super) type ImpostorMaterial = ExtendedMaterial<StandardMaterial, Impostor>;`. `Impostor` binds:
    - `#[texture(100)] #[sampler(101)] color`;
    - `#[texture(102)] normal_depth`;
    - `#[uniform(103)] params: ImpostorParams { center: Vec3, radius: f32, half_size: Vec2, grid: f32, debug: u32 }`.
    Copy the layout from `world/base/cutaway.rs`.
  - `pub(super) fn encode(view: Vec3) -> Vec2` and `pub(super) fn frame_blend(view: Vec3, grid: u32) -> [(UVec2, f32); 3]`:
    the Rust mirror of the shader, including the below-horizon clamp;
  - `pub(super) fn impostor_start(bake: &ImpostorBake, max_scale: f32) -> f32`, the resolution rule:
    `max_scale · 1080 · max(h / frame_px[1], w / frame_px[0]) / (1.5 · tan(22.5°))`;
  - `pub(super) fn tree_middle(pose: &Transform, bake: &ImpostorBake) -> Vec3`;
  - `pub(super) fn impostor_quad() -> Mesh`: `Rectangle::new(2.0, 2.0)` as a mesh (positions `(±1, ±1, 0)`,
    normals and uvs); the vertex shader positions it;
  - `const NO_END: f32 = 1.0e6`: the impostor's end margin, `NO_END..NO_END`. It is finite so the dither math never
    sees an infinity.

- [ ] **Step 1: Write the failing tests** in `canopy/impostor.rs`:
  - `the_mapping_matches_the_bake_table`: for each entry of `include!("../pillars/impostor_mapping_checks.rs")`,
    `encode(d)` is within 1e-5 of `uv`.
  - `blend_weights_sum_to_one_and_pick_an_exact_frame`.
  - `blend_is_continuous_across_cell_edges`: 2,000 seeded pairs 1e-5 apart, each weight changes by under 1e-3.
  - `a_view_from_below_uses_the_horizon_frames`: as Task 3's test, including `(0,-1,0)`.
  - `impostor_start_meets_the_resolution_rule`. For each species at its largest scale (`TreeModel.scale.1`):
    - the frame magnification at the start, `2·h·scale/(2·start·tan(22.5°))·1080/frame_px[1]` and its width twin,
      is at most 1.5 + 1e-3;
    - each start lies within 40–70 m (the spec's "about 55 m").
  - `an_impostor_carries_its_sphere_aabb`: `spawn_canopies` into a test world. The root has `Aabb` centred on
    `bake.center`, half extents `radius`, `NoAutoAabb`, the impostor material, `NotShadowCaster`, and a
    `VisibilityRange` with start `0..0`, end `NO_END..NO_END` and `use_aabb`.

- [ ] **Step 2: Run them.** `cargo test world::canopy::impostor` fails to compile.

- [ ] **Step 3: Implement the Rust side.**
  - **`CanopyModels`** holds the shared quad and one `ImpostorMaterial` per species. The crown-card handles go;
    Task 6 adds the near ones.
  - **Materials.** `prepare_canopy_materials` builds them with:
    - base `StandardMaterial`: `alpha_mode: Mask(0.5)`, `perceptual_roughness: bake.roughness`, `metallic: 0`;
    - the atlases loaded with a linear, clamp-to-edge sampler.
  - **Debug tint.** `params.debug` is 1 when the `ROADSIDE_IMPOSTOR_DEBUG` environment variable is set, read once
    at preparation. A plain uniform branch, no shader def.
  - **Spawning.** `spawn_canopies` gives the root:
    - the quad, its material, the sphere `Aabb` and `NoAutoAabb`, `NotShadowCaster`;
    - `VisibilityRange { start_margin: 0.0..0.0, end_margin: NO_END..NO_END, use_aabb: true }`.
    - Trees no longer use `detail_range` or `STRATEGY_REACH`.
  - **The UHD child.** `stream_close_trees` measures with `tree_middle`. `reveal_close_tree` keeps setting the
    root's start margin to the UHD end margin, as today.
  - **Preloading.** `preload_canopies` preloads the six atlases. `MaterialExtension` returns
    `shaders/impostor.wesl` for both vertex and fragment, with `enable_prepass` and `enable_shadows` false.

- [ ] **Step 4: Implement `impostor.wesl`.**
  - **Vertex.**
    - `X = C + R·pos.x·s + U·pos.y·s`, where `(R, U)` is `frame_basis` of the real, unclamped view direction (camera
      minus C, in the model frame), with model X as the fallback when `|cross(Y, view)| < 1e-4`.
    - The quad only has to cover the sphere's silhouette, so it always faces the real camera. The below-horizon
      clamp applies only to choosing frames, in the fragment.
    - `s = radius · L / sqrt(L² − radius²)`, the sphere's silhouette on the plane through C. L is the camera's
      distance to C, clamped to at least `1.05·radius`.
    - Output world position and clip position through `get_world_from_local`.
    - `world_normal` faces the camera; carry `instance_index`.
    - Include the `@if(VISIBILITY_RANGE_DITHER)` output, as `plant_wind.wesl` does.
  - **Fragment.**
    - Start from `pbr_input_from_standard_material(in, is_front)`, as `terrain.wesl` does.
    - Apply the `VISIBILITY_RANGE_DITHER` discard.
    - Then:
    ```
    let local = get_local_from_world(in.instance_index);
    let cam = (local * vec4(view.world_position, 1)).xyz;
    let x   = (local * in.world_position).xyz;           // the point on the quad
    let r   = normalize(x - cam);                        // the view ray, model frame
    let d   = clamped_view(cam - C);                     // per tree, as in the vertex shader
    for each (frame, weight) in frame_blend(d):
        D = frame_direction(frame); (R, U) = frame_basis(D)
        s0 = dot(C - x, D) / dot(r, D)                   // onto the frame's plane
        t  = depth at frame_uv(x + r*s0) * 2 - 1         // one parallax step
        p  = x + r * (s0 + t * radius / dot(r, D))
        sample both atlases at frame_uv(p): uv outside [0,1] counts as coverage 0
        accumulate weight * (albedo, coverage, model normal)
    ```
    - `frame_uv` follows **Atlas layout**, clamped half a texel inside the frame. Sample with `textureSampleGrad`,
      using the derivatives of the per-frame uv divided by `grid`.
    - Then set:
      - `base_color` to (albedo, coverage);
      - `N` and `world_normal` to `normalize(mesh_normal_local_to_world(n, instance_index))`;
      - `perceptual_roughness` to `bake.roughness`.
    - Apply `alpha_discard`, then `apply_pbr_lighting` and `main_pass_post_lighting_processing`.
    - **Debug.** When `params.debug == 1`, the fragment returns the heaviest frame's `(i/(N−1), j/(N−1), 0)`,
      unlit, with no fog, after the coverage discard.

- [ ] **Step 5: Run the tests.** `cargo test world::canopy` passes. Then `cargo fmt -- --check` and
  `cargo clippy -- -D warnings`.

- [ ] **Step 6: Debug-tint check** (playtest build, Task 1's tactical start).
  - With `ROADSIDE_IMPOSTOR_DEBUG=1`, take a shot at each of the four orbit angles and one from the overview.
  - With the tint off, take shots at fpv 15, 30 and 60 m.
  - Expect, against the Rust `encode` of each view:
    - **known directions:**
      - from fpv eye height, a tree ahead reads a border colour: red or green near 0 or 1, the one `encode`
        gives for that bearing;
      - straight down from the overview, trees read about (0.5, 0.5, 0), the four frames around the pole;
    - the colours change in single steps between neighbouring frames while orbiting;
    - the same tree shows the same colour from the same angle;
    - with the tint off, no tree is black, inside out or clipped at its quad's edge.
  - Save the shots as `target/playtest/trees/task5-*.png` and list them in the report.

- [ ] **Step 7: Docs and commit.**
  - Update `doc/world-reference.md`'s Scenery bullet: trees draw an impostor past the UHD scan, at every zoom.
  - Commit the code, the shader and the docs.

### Task 6: The near tier

**Files:**
- Create: `src/world/canopy/tiers.rs`. It decides which tier of each tree shows.
- Modify:
  - `src/world/canopy.rs`: move `stream_close_trees`, `spawn_close_scan` and `reveal_close_tree` into `tiers.rs`;
    `CanopyModels` gains the near handles; `spawn_canopies` inserts `Tiers`; `update_canopy_motion` loses the
    shadow toggle;
  - `src/world/mod.rs`: system registration.
- Docs: the Scenery bullet in `doc/world-reference.md`, and the tree path in `arch-readme.md`.

**Interfaces:**
- Consumes: `BAKES`, `impostor_start`, `tree_middle` (Task 5). The near GLBs at the existing `TREE_{SMALL,BIRCH,DEAD}`
  paths (Task 4).
- Produces, in `canopy/tiers.rs`:
  - `#[derive(Component, Default)] pub(super) struct Tiers { near: Option<Entity>, close: Option<Entity>, close_shown: bool }`.
    It replaces `CloseScan`.
  - `#[derive(Component)] pub(super) struct NearTree;`
  - Constants:
    - `NEAR_FADE = 6.0`;
    - `NEAR_LEAD = 4.0`;
    - `NEAR_PER_FRAME = 32`;
    - `SHADOW_ENTER = 48.0` and `SHADOW_EXIT = 55.0`, moved from `canopy.rs`.
  - Distances, all measured to `tree_middle`:
    - `fn near_load(species) -> f32 = impostor_start + NEAR_FADE + NEAR_LEAD`;
    - `fn near_drop(species) -> f32 = near_load + 6.0`.
  - `pub(super) fn tier_margins(tiers: &Tiers, start: f32) -> (Range<f32>, Range<f32>)`. It returns the root's start
    margin and the near child's start margin:
    - root: `start..start+NEAR_FADE` if near exists; else `UHD_RANGE..UHD_RANGE+UHD_FADE` if `close_shown`; else
      `0..0`;
    - near: `UHD_RANGE..UHD_RANGE+UHD_FADE` if `close_shown`, else `0..0`.
    - The near child's end margin is always `start..start+NEAR_FADE`.
  - Systems:
    - `stream_near_trees`: at most `NEAR_PER_FRAME` spawns a frame, nearest first;
    - `fade_tiers`: on `Changed<Tiers>`, applies `tier_margins` to the root and the near child;
    - `cast_near_shadows`;
    - `stream_close_trees`: as today, but it records in `Tiers`.

- [ ] **Step 1: Write the failing tests** in `canopy/tiers.rs`, using the `streaming_with` harness moved from
  `canopy.rs`:
  - `tiers_hand_over_without_a_gap`: for every `Tiers` state, the UHD end equals the near start when `close_shown`,
    and the near end equals the root's start when near exists.
  - `an_impostor_stays_until_its_near_child_exists`: the root's start margin is `0..0` before the spawn, and
    `start..start+NEAR_FADE` in the same update that spawns the near child.
  - `near_tiers_spawn_within_the_load_distance_and_drop_past_the_drop_distance`: camera moves give spawn, keep
    (between load and drop) and despawn.
  - `near_tiers_spawn_at_most_the_cap_per_frame_nearest_first`: 40 trees in range spawn 32 in the first update, the
    32 nearest, and the rest in the next.
  - `a_falling_tree_keeps_its_impostor_and_near_child`: after `break_selected_trees`' swap of `CanopyVisual` for
    `FallingTree`, moving the camera past the drop distance despawns neither.
  - `only_near_tiers_cast_shadows_within_range`: the root always has `NotShadowCaster`. The near child loses it under
    48 m and regains it past 55 m.
  - Keep the existing close-scan tests, reading `Tiers` instead of `CloseScan`.

- [ ] **Step 2: Run them.** `cargo test world::canopy::tiers` fails.

- [ ] **Step 3: Implement.**
  - The near child is spawned with:
    - `Mesh3d`, `MeshMaterial3d`, `Transform::IDENTITY`, `ChildOf(tree)`, `NearTree`, `NotShadowCaster`;
    - `VisibilityRange { start_margin: 0.0..0.0, end_margin: start..start+NEAR_FADE, use_aabb: true }`.
  - Its material is the near GLB's `StandardMaterial`, made as the crown cards' were. It loads at `Startup`, so it
    shows on spawn.
  - `stream_near_trees` collects candidates and sorts them by distance. Trees are skipped when:
    - they lack `CanopyVisual` (falling);
    - they are within load range and already have a near child.
  - Drops apply to trees past `near_drop`. They remove the child and set `Tiers.near = None`.
  - `reveal_close_tree` sets `Tiers.close_shown`, and `fade_tiers` does the rest. The despawn branch clears both
    fields.

- [ ] **Step 4: Run all the checks.** `cargo test` passes, then fmt and clippy.

- [ ] **Step 5: Look check** (playtest build).
  - Walk the wood from Task 1. No tree blinks at 12 m or at its impostor start, and no stutter shows while you walk
    or zoom in from the overview.
  - Save `target/playtest/trees/task6-*.png` at 10, 20 and 58 m from a birch.

- [ ] **Step 6: Docs and commit.**
  - `doc/world-reference.md`: three tiers and their ranges, the shadows within 48–55 m from the near tier.
  - `arch-readme.md`: the tree streaming path, near tiers and impostors.
  - Commit.

### Task 7: The look checkpoint (stops for the user)

**Files:**
- Possibly modify: `tools/build_tree_tiers.py` (near triangles, grid) and the tier constants.
- Create, not committed: `target/playtest/trees/checkpoint-*.png` and `checkpoint-orbit.gif`.

- [ ] **Step 1: Tune.**
  - The near triangle count: the lowest of 60k, 40k and 25k at which the 12 m swap shows no change of shape on the
    birch and the dead tree. Re-bake with `--species birch dead --triangles N`.
  - The grid: 16 instead of 12 only if the orbit shows frame steps. This counts against the VRAM note in the spec.
  - Record every try.

- [ ] **Step 2: Shoot the checkpoint set.** `poses.sh checkpoint`, plus:
  - the UHD scan of each species at 5 m;
  - a 4-second right-drag orbit at the tactical zoom, captured as frames and turned into `checkpoint-orbit.gif` with
    agentic-tooling `media/video-to-gif`.
  - Pair each shot with its `before-*` twin.

- [ ] **Step 3: STOP.** The implementer reports DONE with the shot list and the tuned values. The controller then:
  - sends the shots to the user (`SendUserFile`);
  - asks for the go, naming the near triangle count, the grid, and each species' impostor start;
  - waits.
  On a no, the user's notes become the next round of Step 1. There is no commit in this task.

### Task 8: After the go: the small tree, the assets, the docs

**Files:**
- Commit:
  - `assets/models/trees/medium/tree_{small,birch,dead}.glb`;
  - `assets/textures/trees/*.ktx2`;
  - the regenerated `src/world/pillars/*.rs`.
- Docs: `doc/world-reference.md` (Asset tiers, the bake tools), the `CLAUDE.md` index, `arch-readme.md` (a recipe:
  re-baking a tree's tiers).

- [ ] **Step 1: Re-bake the small tree with the settled settings.** `python3 tools/build_tree_tiers.py --species small`,
  plus birch and dead if Task 7 changed their settings after their last bake.
- [ ] **Step 2: Check.** `cargo test` (the mapping and start-rule tests read the regenerated files), then a playtest
  look at a small tree at 10, 30 and 60 m.
- [ ] **Step 3: Check LFS.** `git check-attr filter` on every new asset prints `lfs`. Report the total bytes added.
- [ ] **Step 4: Docs.**
  - Asset tiers: `trees/medium/` now holds the near tiers baked from the scans; `textures/trees/` holds the
    impostor atlases.
  - Describe both agentic-tooling tools and `tools/build_tree_tiers.py`.
  - The crown cards are in the archive zip.
- [ ] **Step 5: Commit** the assets, generated files and docs.

### Task 9: After numbers and the goal 5 gate

**Files:**
- Create, not committed: `target/playtest/trees/after.txt` and `after-*.png`.
- Possibly modify: `src/world/scenery.rs` (`WOOD_DENSITY`).

- [ ] **Step 1: Re-run Task 1's scenarios unchanged,** both builds, and `poses.sh after`.
- [ ] **Step 2: Judge the gate**, writing `after.txt` beside `before.txt`:
  - **wood:**
    - CPU frame mean and p99 within before + 5%;
    - opaque plus shadow `elapsed_gpu` within before + 5%;
    - no main-app frame over 33 ms.
  - **overview:** opaque plus shadow `elapsed_gpu` within before + 3 ms.
  - **tactical:** recorded, not gated.
  - Also record peak VRAM against before, and the bytes the new files hold.
- [ ] **Step 3: If the gate fails,** apply the spec's order.
  1. The starts already sit at the resolution rule's minimum, so step 1 is used up.
  2. Thin `WOOD_DENSITY` in steps of 0.05 from 0.85, re-running the wood scenario each time, down to 0.70.
  3. If 0.70 still fails, stop and report: the next step is the user's call.
- [ ] **Step 4: Commit** any density change, with the gate's numbers in the body. Report the LFS total from Task 8.
