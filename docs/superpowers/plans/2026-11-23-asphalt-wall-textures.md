# Asphalt and Cordon Wall from Textures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the scanned road kit and the 2,875-triangle wall panel with relief-mapped textures baked from the photo and the UHD scan, so the roads and walls look 3D for a fraction of the draw cost.

**Architecture:**
- **Asphalt.** The terrain shader paints the old asphalt from a tiling grain set and hashed crack decals. A new alpha channel in the road mask marks it apart from New Harmont's streets. The road kit and `old_road.rs` go.
- **Wall.** Blender bakes the UHD wall scan onto a pre-split box and two razor-wire strips. `cordon.rs` builds its chunk meshes from them, as today, but carries tangents. A depth map gives Bevy's relief parallax.

**Tech Stack:**
- Bevy 0.20.1 (`StandardMaterial` relief parallax), WESL;
- Python 3 with Pillow and numpy;
- headless Blender 4.5 (Cycles bake);
- `ctt` (KTX2 BC7), glTF Transform validate.

**Spec:** `docs/superpowers/specs/2026-11-23-asphalt-texture-design.md`

## Global Constraints

**When it runs.**
- After phase 1 (`docs/superpowers/plans/2026-11-23-uhd-ram-gpu.md`), including its Task 10 and the final review, on the branch then checked out.
- The "before" numbers include phase 1.

**Dependencies.**
- Bevy 0.20.1 only; `Cargo.toml` is unchanged.
- Game code stays `wasm32-unknown-unknown` compatible.

**Checks and code style.**
- `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test` and `git diff --check` pass at every commit.
- No `unwrap()` in game logic.
- Systems are named `verb_noun`.

**Precalculate once.**
- Every map is baked by a tool and shipped as KTX2 with a full mip chain, encoded with `ctt` using the presets in `tools/compress_glb_textures.py` (`ctt_arguments`):
  - colour: sRGB BC7;
  - normals and data maps: linear BC7 (the "data" preset), not BC5, so the shaders read `.rgb` as today.
  - The spec names the asphalt maps `.png`. The user's precalculate-once rule makes them `.ktx2` with the same packing, and their PNGs stay in the workspace.
- Runtime GLBs end with `python3 tools/compress_glb_textures.py`, then `python3 ~/Projects/agentic/agentic-tooling/media/glb-bake-tangents/glb_bake_tangents.py`, then `npx @gltf-transform/cli validate`.
- The only runtime derivation is the wall's tangent shear, which depends on the ground under each panel. Comment it.

**Preloading.** Every new runtime path is a constant in `src/assets/mod.rs` and is preloaded in `Startup`, or the splash turns Ready before it exists.

**Asset locations.**
- `assets/` holds only runtime files.
- Retired runtime files go into `~/Projects/agentic/RoadSide-Picnic/archived/roadside-picnic-unused-assets-2026-11-22.zip` with `zip -u` from the repo root, keeping their `assets/...` paths. Their `sha256sum` lines are appended to its `MANIFEST.txt` under the line `Added 2026-11-23 at commit <sha>: <what>`. Update `MANIFEST.txt` inside the zip, then list the new entries with `unzip -l` to verify.
- Before calling any asset missing, search the assets workspace `~/Projects/agentic/RoadSide-Picnic` (`models/<kind>/{low,medium,uhd}/`, `models/tripo-out/`, `images/`, the `archived/` zip).
- Texture sources and intermediate PNGs go to `~/Projects/agentic/RoadSide-Picnic/textures/asphalt/` and `.../textures/cordon-wall/`. These are new folders. They are not checked into the game repo.

**Blender.** `BLENDER=~/.cache/blender/blender-4.5.16-linux-x64/blender` (the `blender` skill); run `"$BLENDER" --version` first.

**Playtests.**
- Set `XDG_DATA_HOME=$PWD/target/playtest/data`.
- Use a free X display (`ls /tmp/.X11-unix/`, never `:5`) at 1280x720.
- Stop only the pids you started.
- Use `cargo run --profile playtest` to look. Take numbers only from `cargo build --release --features bevy/trace_chrome --target-dir target/trace`.

**Commits.**
- One per task, staged by path.
- Subject prefixed with `git branch --show-current`, imperative.
- Body at most 10 lines.
- Last line `Claude-Session: https://claude.ai/code/session_013NVSdF8r182CK5kaKAUHhv`.
- Never push.

**Docs.** `CLAUDE.md` and `arch-readme.md` change in the task that changes the behaviour they describe.

## Review Focus

1. **Wall boxes z-fighting or corner notches.**
   - Panels in a straight run must meet end to end without overlap: coplanar faces flicker.
   - Where two runs meet, the outer corner must close with no square notch.
   - Pinned by Task 5's `straight_runs_meet_end_to_end_and_corners_lap` and the box-bounds check in Task 4.
2. **Old asphalt marking leaking into New Harmont, or missing on the highway.**
   - Streets and the plaza must stay smooth tar.
   - Pinned by Task 3's `old_asphalt_is_marked_apart_from_new_harmont_streets`.
3. **Crack decals cut off at their cell's edge, or wrapping into a second copy.**
   - Pinned by Task 2's `test_crack_border_is_transparent`.
   - Also pinned by Task 3's `crack_decals_fit_inside_their_cells`, which reads the shader's constants.
4. **Relief swimming on sheared or sloped wall faces.** This happens when tangents do not follow the panel's shear. Pinned by Task 5's `wall_chunk_tangents_run_along_the_panel_and_square_to_the_normals`.
5. **New textures loading after the curtain and popping in.** Pinned by Task 6's strace check: no `asphalt_` or `cordon` path is opened after `Entered the Zone`.

---

### Task 1: Before numbers and screenshots

**Files:**
- Create: `target/playtest/asphalt-wall/run_scenario.sh`, `poses.sh` and `before.txt`, plus `before-*.png` (not committed).

**Interfaces:**
- Produces:
  - `run_scenario.sh NAME "xinput steps" WRAPPER [game args]`, the same contract as `target/playtest/phase1/run_scenario.sh`, writing into `target/playtest/asphalt-wall/`;
  - `poses.sh PREFIX`, which takes the nine screenshots named `PREFIX-{highway,street,perimeter}-{6,40,250}m.png`.
  Task 6 re-runs both unchanged.

- [ ] **Step 1: Copy the phase 1 runner.**
  - Change `out=` to `target/playtest/asphalt-wall`.
  - Change the display to a free one.
  - Build the trace binary.

- [ ] **Step 2: Record two traced scenarios** under Ruling 41's quiet-machine rules (log `uptime`, `pgrep -a roadside-picnic` and the nvidia-smi compute apps before each).
  - **`highway`:**
    - Seed `$XDG_DATA_HOME/roadside-picnic/progress.txt` like `target/playtest/phase1/drive.progress.txt`, with the stalker and car beside the highway leg `map(667,373)`–`map(697,362)`.
    - Enter the car, then click-order it at least 20 s along the old asphalt toward the Garage.
  - **`perimeter`:**
    - Put the stalker 30 m east of `cordon::PERIMETER_X`, 150 m south of `PERIMETER_GATE`.
    - At about 40 m zoom, toggle the free camera (F).
    - Roam north along the wall with the arrow keys for 20 s, past the gate.
  - Run both with the `time` wrapper. `strace` slows the frames, and the file-open check is after-only (Task 6).

- [ ] **Step 3: Script `poses.sh`.** It takes 6 m, 40 m and 250 m zoom screenshots at three places:
  - the highway leg above;
  - a New Harmont street near `town::PLAZA`;
  - the Perimeter 100 m south of the gate, with the 6 m and 40 m views from the side (rotated with a right drag) and the 250 m view top-down.

  Use agentic-tooling `media/x11-app-screenshot`. Record the wheel counts and drags in the script, so the poses repeat exactly. Run it as `poses.sh before`.

- [ ] **Step 4: Write `before.txt`.** For each scenario include:
  - frame max and p99 in the moving window (`summarize.py`);
  - the `stream_cordon` span max and total;
  - peak VRAM over the desktop;
  - the static counts: road-kit entities (`2 × lay_roads().count()`) and wall triangles per panel (2,875).

  Delete the raw `*.json` traces. Nothing is committed in this task.

### Task 2: Asphalt maps from the photo

**Files:**
- Create: `tools/prepare_asphalt_textures.py`, `tools/test_prepare_asphalt_textures.py`.
- Output (committed): `assets/textures/terrain/asphalt_{albedo,normal,surface,crack}.ktx2`.
- Source: copy `~/Downloads/image_5b974df1.jpg` to `~/Projects/agentic/RoadSide-Picnic/textures/asphalt/image_5b974df1.jpg`.

**Interfaces:**
- Produces:
  - the four KTX2 maps, each 512×512 with full mips;
  - `asphalt_surface` packs R = AO, G = roughness, B = height, with 1 at the top;
  - `asphalt_crack` packs RG = tangent normal xy (0.5 is flat; x along the decal's u, y along its v), B = crack depth (0 at the surface, 1 deepest) and A = crack mask;
  - grain maps tile seamlessly.
- CLI: `python3 tools/prepare_asphalt_textures.py PHOTO OUT_DIR --sources DIR`. It writes the PNGs to `--sources` and the KTX2s to `OUT_DIR`.
- It imports `tileable` from `prepare_terrain_textures.py` and `ctt_arguments` from `compress_glb_textures.py` (same `tools/` directory).

- [ ] **Step 1: Write the failing tests** in `tools/test_prepare_asphalt_textures.py` (unittest, run on synthetic images):
  - `test_normal_from_height_points_downhill`: a height ramp rising toward +x gives a normal whose decoded x is < 0 and whose decoded z is > 0. The convention is `n = normalize(-dh/dx, -dh/dy, 1)`, OpenGL +Y, as the other terrain normals. Here y points up the texture, against image rows, so the green channel is `+dh/drow`.
  - `test_grain_tiles_without_a_seam`: the mean absolute difference between the left and right edge columns, and between the top and bottom rows, of the tileable albedo is ≤ 1.5 times that of two adjacent interior columns.
  - `test_crack_border_is_transparent`: every crack decal pixel within 8 px of the border has alpha 0.
  - `test_albedo_mean_is_dark_asphalt`: the mean linear luminance of the output albedo is between 0.06 and 0.12. The photo's grey must be graded down to weathered asphalt.

- [ ] **Step 2: Run them to see them fail.** Run: `python3 -m unittest tools/test_prepare_asphalt_textures.py -v`. Expected: an ImportError or AttributeError (no functions yet).

- [ ] **Step 3: Implement the tool.**
  - Functions: `rectify(photo, corners) -> Image` (a four-point perspective warp of the sharp lower half), `flatten_lighting(image) -> Image` (divide by a wide blur), `normal_from_height(height) -> Image`, `crack_decal(image, box) -> Image` and `main()`.
  - The corner, crop and threshold constants are yours to choose by eye. Name them at the top with the photo's pixel coordinates.
  - Derive the height from high-passed luminance, AO from cavity and roughness from luminance.
  - The crack decal's mask comes from dark, thin luminance after a threshold and a small dilation. Its depth is the mask blurred. It is cut from the photo's longest crack.

- [ ] **Step 4: Run the tests to see them pass.** Then run the tool for real and check:
  - four `.ktx2` files of about 512 px with mips (`ctt` presets as in Global Constraints);
  - the PNGs in the workspace folder.

- [ ] **Step 5: Commit** the tool, its test and the four KTX2 files.

### Task 3: Paint the old asphalt and retire the road kit

**Files:**
- Modify:
  - `src/world/roads.rs`: `RoadSample::surface`, `surface_texture`, tests, the `old_road.rs` comment at :74, and `old_roads()` if nothing else calls it;
  - `src/world/terrain_material.rs`: `GroundLayers` bindings 110–113 and the test;
  - `src/assets/mod.rs`: four constants and the preload arrays;
  - `assets/shaders/terrain.wesl`;
  - `src/world/mod.rs` (:14, :134, :156), `CLAUDE.md` ("Old roads", "Terrain"), `arch-readme.md` (:109, :295).
- Delete: `src/world/old_road.rs`, `tools/straighten_road_kit.py` (git history keeps it), `assets/models/road/` (moved to the archive zip).

**Interfaces:**
- Consumes: Task 2's maps.
- Produces:
  - the mask's alpha is the old-asphalt share: the same `smoothstep(6.4, 5.4, d)` as blue, for `Surface::Asphalt` only;
  - `town::paved` texels get B = 255 and A = 0;
  - constants `TERRAIN_ASPHALT_ALBEDO`, `TERRAIN_ASPHALT_NORMAL`, `TERRAIN_ASPHALT_SURFACE` and `TERRAIN_ASPHALT_CRACK` (`textures/terrain/asphalt_*.ktx2`);
  - shader bindings 110 `asphalt_albedo`, 111 `asphalt_normal`, 112 `asphalt_surface` and 113 `asphalt_crack`.

- [ ] **Step 1: Write the failing tests.**
  - In `roads.rs`, `old_asphalt_is_marked_apart_from_new_harmont_streets`. It reads RGBA with `read4`, a 4-byte twin of the existing test's `read`; the existing test keeps its 3-byte asserts:
    ```rust
    assert_eq!(read(map(667.0, 373.0).midpoint(map(697.0, 362.0))), [255, 0, 255, 255]);
    assert_eq!(read(GATE), [255, 0, 255, 255]);
    assert_eq!(read(town::PLAZA + Vec2::new(20.0, 15.0))[2..], [255, 0]);
    ```
    It also asserts B = 255 and A = 0 at a point on the centerline of a `Surface::Street` leg. Pick one where the blue reads 255.
  - In `terrain_material.rs`, `crack_decals_fit_inside_their_cells`. It parses `const CRACK_CELL`, `CRACK_SIZE` and `CRACK_JITTER` from `include_str!("../../assets/shaders/terrain.wesl")` and asserts `CRACK_SIZE * SQRT_2 / 2 + CRACK_JITTER / 2 <= CRACK_CELL / 2`.

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -- old_asphalt_is_marked crack_decals_fit`. Expected: the alpha reads 0, and the constants are not found.

- [ ] **Step 3: Write the mask.** In `RoadSample::surface` the fourth value becomes the asphalt share for `Surface::Asphalt` only. In `surface_texture` the paved override also zeroes alpha.

- [ ] **Step 4: Bind and preload the maps.**
  - Add the four `GroundLayers` fields (`#[texture(110)]`…`#[texture(113)]`), loaded with `load_repeating`. The albedo is sRGB and the rest linear.
  - Add the constants to the `albedo` and `data_maps` preload arrays.
  - KTX2 images carry mips, so `needs_mipmaps` skips them already.

- [ ] **Step 5: Paint the asphalt in `terrain.wesl`.**
  - **Constants.** Start with these, then tune them by eye: `ASPHALT_TILE` 3.0 m, `CRACK_CELL` 9.0, `CRACK_SHARE` 0.4, `CRACK_SIZE` 5.0, `CRACK_JITTER` 1.8, `GRAIN_DEPTH` 0.01 m, `CRACK_DEPTH` 0.03 m, `RELIEF_STEPS` 12, `RELIEF_NEAR` 0.02 (`pixel_size`, m/px), and the crack fade over `pixel_size` 0.08–0.25.
  - **Sampling.**
    - Sample the mask as `rgba`; `old = road.a`.
    - Compute every UV and its `dpdx`/`dpdy` before any branch.
    - Inside branches and loops, sample only with `textureSampleGrad`: `textureSample` needs uniform control flow.
  - **Crack cell.** In the cell `floor(p.xz / CRACK_CELL)`:
    - `patch_hash` decides whether the cell has a crack (`< CRACK_SHARE`);
    - two more hashes give the offset `(h - 0.5) * CRACK_JITTER` per axis;
    - one more gives the angle;
    - `local = rotation(angle) * (p.xz - centre - offset) / CRACK_SIZE + 0.5`;
    - outside [0,1]², the cell has no crack;
    - the gradients are `rotation(angle) * dpdx(p.xz) / CRACK_SIZE`, which are analytic and do not jump at cell borders.
    - The crack's weight is `mask * smoothstep(0.6, 1.0, old) * (1 - fade)`.
  - **Relief.** Where `old > 0` and `pixel_size < RELIEF_NEAR`, march the view ray from the surface down to `GRAIN_DEPTH + CRACK_DEPTH`:
    - each step moves along `-tangent_view / max(facing, 0.32)`, built as the existing shallow parallax does;
    - stop at the first step below `(1 - grain.b) * GRAIN_DEPTH + crack.b * crack_weight * CRACK_DEPTH`;
    - refine once by linear interpolation.
    - Farther out, take one step: the existing `slide` with the grain height.
  - **Shading.**
    - Albedo is the grain albedo, darkened toward 0.35 in cracks.
    - Roughness is `mix(0.34, 0.98, surface.g)`, and AO is `surface.r`.
    - The normal is the grain normal (decoded like `ground_normal`'s top projection). In cracks it is blended toward the decal's RG, rotated back by `-angle` into world XZ.
    - All of this mixes over today's tar and street result by `old`. Streets (`old = 0`) keep the current tar and the flattened normal.

- [ ] **Step 6: Retire the road kit.**
  - Delete `old_road.rs` and its three registrations, and `tools/straighten_road_kit.py`.
  - Delete `roads::old_roads` if clippy reports it dead.
  - Archive `assets/models/road/` (Global Constraints), then `git rm -r assets/models/road`.
  - Rewrite the "Old roads" bullet as asphalt painted by the terrain shader, update the "Terrain" mask sentence (alpha marks old asphalt) and fix the `arch-readme.md` rows.

- [ ] **Step 7: Run the tests and checks.** Run: `cargo test` and `cargo clippy -- -D warnings`. Expected: all pass, including the two new tests.

- [ ] **Step 8: Look.**
  - Run `cargo run --profile playtest`, then `target/playtest/asphalt-wall/poses.sh mid` for the highway and street poses.
  - Expected: grain and scattered cracks on the highway at 6 and 40 m; no visible tiling or shimmer at 250 m; smooth tar on the New Harmont street; no crack cut at a straight edge.
  - Tune the constants if needed. Keep the test's inequality.

- [ ] **Step 9: Commit**, including the shader, the Rust changes, the docs, the deletions and the asphalt KTX2 files if they were retuned.

### Task 4: Bake the cordon wall

**Files:**
- Create: `tools/bake_cordon_wall.py` (headless Blender; its docstring holds the full command).
- Output (committed):
  - `assets/models/sites/medium/site-cordon-wall-box.glb`;
  - `assets/models/sites/medium/site-cordon-wall-wire.glb`;
  - `assets/textures/cordon/wall_depth.ktx2`.
- Source: the UHD scan `~/Projects/agentic/RoadSide-Picnic/models/sites/uhd/site-cordon-wall.glb`. If it is missing, it is in the archive zip.

**Interfaces:**
- Produces:
  - **Box GLB.** One mesh with one primitive and one material (base colour plus normal map), with baked tangents. It is in the panel frame:
    - x is exactly ±0.43 (`PANEL_SPAN / 2`);
    - z is ± the measured half thickness;
    - y runs from 0 to the measured concrete top, under the wire.
    It is pre-split into 8 equal columns along x and rows at y = 0, 0.15 and the top, so the runtime shear follows the ground and the water-foundation row stretches alone. It has front (+z), back (−z), top and two end faces, and no bottom. On the front, back and top faces, UV u increases with +x.
  - **Wire GLB.** One primitive, one material with `alphaMode: MASK`, `alphaCutoff: 0.5` and `doubleSided: true`. It holds two strips spanning x ±0.43, both 8 columns:
    - an upright one at z = 0;
    - a flat one at the coils' mid-height.
  - **Depth map.** `wall_depth.ktx2` is linear BC7 with mips, in the box's UV layout. White is deepest, as Bevy's `depth_map` expects.
  - The tool prints the measured half thickness and top (model units) for Task 5's constants.

- [ ] **Step 1: Write the tool.**
  - **Align.** Import the UHD scan and the current medium panel (`assets/models/sites/medium/site-cordon-wall.glb`). Align the scan to the panel frame by matching the bounding-box minimum y and the x/z centres.
  - **Measure.** Take the half thickness as the median |z| of scan vertices with y in 0.10–0.30, and the concrete top as the height where that band's vertex count drops.
  - **Build the box and strips** as above. Fail, with a message, if the box's x extent is not ±0.43 within 1e-4.
  - **Bake** with Cycles, selected-to-active, the cage extrusion and max ray distance short enough to skip the sandbags, the tower and the wire:
    - Diffuse colour only;
    - tangent Normal (OpenGL);
    - height via an emission shader on the scan whose strength is the depth below the box's face;
    - for the wire strips, colour plus coverage (emission white on the wire, black elsewhere), from a tower-free stretch.
  - **Atlas.** About 2048 px along the panel.
  - **Export.** Write the GLBs, then run `compress_glb_textures.py`, `glb_bake_tangents.py` and `validate` (Global Constraints). Write the depth PNG and encode it to KTX2 with the data preset.
  - Save every intermediate PNG to the workspace `textures/cordon-wall/`.

- [ ] **Step 2: Run it.** Run: `"$BLENDER" -b --factory-startup --python-exit-code 1 --python tools/bake_cordon_wall.py -- <uhd> assets/models/sites/medium/site-cordon-wall.glb assets/models/sites/medium assets/textures/cordon`. Expected: exit 0, the printed measurements, and validate reporting 0 errors for both GLBs.

- [ ] **Step 3: Check the outputs.**
  - Use `python3 /home/bemcho/.claude/jobs/3974ca0f/tmp/glbinfo.py` (or `npx @gltf-transform/cli inspect`). The box has one primitive, `TANGENT` and KTX2 images, with x bounds ±0.43. The wire material is MASK and double-sided.
  - Look at the albedo, normal and depth PNGs: blocks and joints are visible, and there are no sandbags, tower or wire on the box.

- [ ] **Step 4: Commit** the tool and the three outputs.

### Task 5: Build the wall from the box

**Files:**
- Modify:
  - `src/world/cordon.rs`: `WallModel`, `Panel`, `wall_chunks`, `spawn_wall_chunk`, `preload_cordon`, `prepare_cordon`, the `Cordon` resource and tests;
  - `src/assets/mod.rs`: `CORDON_WALL_BOX`, `CORDON_WALL_WIRE` and `CORDON_WALL_DEPTH` replace `SITE_CORDON_WALL`;
  - `CLAUDE.md` ("Cordons": the panel-pipeline paragraph becomes the bake command) and `arch-readme.md`.
- Retire: `assets/models/sites/medium/site-cordon-wall.glb` (to the archive zip, then `git rm`).

**Interfaces:**
- Consumes: Task 4's files and its printed half thickness.
- Produces:
  - `WallModel` also carries `tangents: Vec<[f32; 4]>`. `WallModel::new` requires them (`unreachable!("the wall GLBs have baked tangents")`).
  - `Panel` gains `laps: [f32; 2]`: render-only metres added at its start and end; a negative lap insets that end. Colliders still use `center` and `length`.
  - `const CORNER_LAP: f32 = WALL_HALF_THICKNESS * WALL_HEIGHT_SCALE;`, where `WALL_HALF_THICKNESS` is Task 4's measured value.
  - `Cordon` holds `wall` and `wire` `WallModel`s and two materials.

- [ ] **Step 1: Write the failing tests** in `cordon.rs`. Use a test box, Bevy's `Cuboid::new(PANEL_SPAN, 0.4, 0.08)` mesh moved up by 0.2, with `with_generated_tangents()`, as the `WallModel`.
  - `wall_chunk_tangents_run_along_the_panel_and_square_to_the_normals`:
    - batch panels on sloped ground (a stretch of the west edge, `-EDGE`, where `floor_point` varies);
    - for every vertex, `|dot(t.xyz, n)| < 1e-3`;
    - for every vertex whose normal is within 45° of `across` or up, `|dot(t.xz, along)| > 0.9 * |t.xz|`;
    - `t.w` is ±1.
  - `straight_runs_meet_end_to_end_and_corners_lap`:
    - batch two consecutive panels of one run; the largest along-run coordinate of the first's vertices equals the smallest of the second's within 1e-3;
    - from `wall_chunks()`, at each corner `a`, `b`, `c` and `d`, two panels end. The one whose run comes first in the run order has `+CORNER_LAP` on that end, the other `-CORNER_LAP`. Batched, their boxes touch: the largest coordinate of one along the other's run axis equals the other's smallest within 1e-3, and they do not overlap;
    - the Perimeter panels ending on the map-edge wall have 0 on that end. This is a T-junction: the end stops at the edge wall's centerline, inside it;
    - the panels beside `PERIMETER_GATE` and the yard checkpoint gap have 0 on their gap-side end.

- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -- wall_chunk_tangents straight_runs_meet`. Expected: fail to compile (no tangents or `laps` yet).

- [ ] **Step 3: Carry the tangents through `batch`.**
  - Map each model tangent with the same Jacobian that maps positions:
    - horizontal = `along * tx * scale_x + across * tz * WALL_HEIGHT_SCALE`;
    - vertical = `ty * dheight_dy + slope · horizontal`, where `dheight_dy` is `rise` plus the foundation stretch term the normals already use.
  - Gram–Schmidt it against the final normal, normalize, and keep `w`.
  - Comment why this stays at runtime: the shear depends on each panel's ground.

- [ ] **Step 4: Apply the laps.**
  - In `batch`, model x ∈ [−0.43, 0.43] maps to the along-run offset `−length/2 − laps[0] + t × (length + laps[0] + laps[1])`, with `t = x / PANEL_SPAN + 0.5`. Widen the `GroundPatch` extents by the laps.
  - In `wall_chunks`, laps depend on where a run end meets another run:
    - **An L corner** (two run ends meet within 0.5 m): the run that comes first in the run order laps that end by `+CORNER_LAP`, and the other insets by `-CORNER_LAP`. The first box's end cap then closes the outer corner, and no two faces overlap. Overlapping faces in one plane flicker.
    - **A T-junction** (a run end within 0.5 m of another run's middle, e.g. the Perimeter at the map-edge wall): 0. The end stops inside the other wall.
    - **Gate and lane ends:** 0.

- [ ] **Step 5: Load the box, the wire and the depth map.**
  - `preload_cordon` preloads the box and wire primitives and the depth map. Load the depth map with linear `ImageLoaderSettings`.
  - `prepare_cordon` builds both `WallModel`s.
  - The box material is the GLB's material plus:
    - `depth_map: Some(depth)`;
    - `parallax_mapping_method: ParallaxMappingMethod::Relief { max_steps: 4 }`;
    - `max_parallax_layer_count: 16.0`;
    - `parallax_depth_scale` starting at 0.02. Tune it by eye; it is in UV units, and the atlas spans about 1 panel per u.
  - The wire uses its GLB material as is.
  - `spawn_wall_chunk` spawns the wire mesh as a child of the chunk with the chunk's `detail_range` and, at the map edge, `NotShadowCaster`.

- [ ] **Step 6: Run the tests and checks.** Run: `cargo test` and `cargo clippy -- -D warnings`. Expected: all pass, the existing cordon tests unchanged.

- [ ] **Step 7: Retire the old panel and update the docs.** Archive and `git rm` the old panel. Replace the "Cordons" pipeline sentences with the `bake_cordon_wall.py` command. Update `arch-readme.md`.

- [ ] **Step 8: Look.**
  - Run `cargo run --profile playtest`, then `poses.sh mid` for the perimeter poses, plus one view of a yard corner and the crawl hole.
  - Expected: no flicker along straight runs; corners closed; blocks and joints that shift with the view at 6–40 m; wire outline on top; the crawl hole still passable while crawling; the Perimeter at three times the height, as today.

- [ ] **Step 9: Commit.**

### Task 6: After numbers and comparison

**Files:**
- Create: `target/playtest/asphalt-wall/after.txt` and `after-*.png` (not committed).

- [ ] **Step 1: Rebuild and re-run.** Rebuild the trace binary. Re-run `highway` and `perimeter` exactly as in Task 1 (the `time` wrapper). Then run `highway-strace`, the same steps under the `strace` wrapper, used for the file-open check only. Then run `poses.sh after`.

- [ ] **Step 2: Compare and record.** Write `after.txt` with the before and after tables. Expected:
  - no frame over 33 ms in the moving windows;
  - the `stream_cordon` max no higher than before;
  - zero road-kit entities;
  - the wall at about 50 triangles per panel instead of 2,875;
  - no `assets/textures/terrain/asphalt_`, `assets/textures/cordon/` or `site-cordon-wall` path opened after the `Entered the Zone` log time in the `highway-strace` run (Review Focus 5).

- [ ] **Step 3: Pair the screenshots.** Put each before/after pair side by side (agentic-tooling `media/` montage, or Pillow) as `compare-*.png` for the report.

- [ ] **Step 4: Final checks.** Run: `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test` and `git diff --check`. Expected: all pass. Delete the raw traces and stop only your own processes.

- [ ] **Step 5: Report.** Give the tables and the comparison images' paths, and suggest the branch's final commit message (Task 6 itself commits nothing).
