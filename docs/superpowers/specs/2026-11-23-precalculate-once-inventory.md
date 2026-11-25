# Precalculate once: inventory of derivable runtime work

The user's rule, 2026-11-23: "As a rule precalculate all if possible once." This is the input for that project's design.

It comes from a read-only code sweep of 2163a10. Costs marked *meas.* come from the phase 1 baseline traces in
`target/playtest/phase1/`; costs marked *est.* are counted from the code (about 1 µs per `Terrain::height_at`).
Nothing below is spanned yet, so step 1 is to add `info_span!`s and measure.

## Headlines

1. Every runtime terrain computation bottoms out in `Terrain::height_at`: startup tiles, near tiles, plant sites,
   wall chunks, the path grid and per-frame ground sampling. It reruns noise, the 32-landmark loops and the town
   loops on every call, with no baked grid.
2. The near ground tile, the wall chunks and the plant sites are rebuilt on every re-entry into an area.
3. The loading screen (about 9.5 s) is probably bound by the KTX2 zstd decode (pure-Rust ruzstd on 3 IO threads)
   plus the single-threaded landscape build.

## Inventory (most costly first)

| # | What | Where | When | Cost | Bake |
|---|---|---|---|---|---|
| 1 | Shader pipeline compile behind the curtain | `ui/curtain.rs:20` | once per run | up to ~12 s (meas., indirect) | **No**: Bevy 0.20 has no on-disk pipeline cache. Warm them during the splash instead. |
| 2 | KTX2 zstd decode of GLB textures (all 378 GLBs, `--zstd=19`) | `Cargo.toml:14`, `tools/compress_glb_textures.py:95` | startup and every UHD re-load | medium GLB loads up to 4–8 s each in parallel (meas.); UHD re-load 0.6–1.1 s (meas.), about half of it decode (est.) | Store without zstd (scheme 0), about +30–40 % size, or use the `zstd_c` feature on native. |
| 3 | Terrain meshes: 1,024 8 m tiles and 260 16 m tiles, with heights, normals, weights, stitched edges, hydrology, road grading and water clipping | `world/landscape.rs:120-184`, `terrain.rs:296` | startup, 1 thread | 1.5–2 s (est.) | Bake height, weight and water grids; tile meshes become array copies. |
| 4 | Scenery site lists for 784 tiles: rocks, trees, belt trees, stands, reeds, batch meshes | `world/scenery.rs:112,184,381`, `canopy.rs` `belt_trees`, `trees.rs` `tree_sites` | startup, same task | 2–3 s (est.) | Bake `Sites` and the batches into `assets/world/tiles.bin`. |
| 5 | Per-frame scans of every collider, plant and tree (push_out/fence_out, crush, mutants, canopy, shroud) | `player/mod.rs:83-94`, `world/crush.rs:39,73`, `wildlife/mutants.rs:346`, `canopy.rs:450,502`, `shroud.rs:163` | per frame | 0.5–1.5 ms (est.); wood PostUpdate mean is 7.8 ms vs 4.9 ms in the overview (meas.) | Store world-space pillars in a spatial hash at spawn; index plants and trees by tile; cache shroud state per survey version. |
| 6 | Per-frame terrain samples: locator ring (~770), wildlife `dry_ground`, traffic/patrol contact, camera and fog (4× the same point), tooltip (site base heights every 80 ms) | `ui/locator.rs:113`, `wildlife/movement.rs:304,366`, `vehicle/contact.rs:38`, `ground_fog.rs:85,115,135`, `places.rs:113` | per frame | 1–2 ms (est.) | Read the baked height grid (20–50 ns); precompute the constants. |
| 7 | Plant sites per streamed tile (~1,300 plants) | `world/plants.rs:312,326,383`, `scenery.rs:254` | every stream-in and re-entry (worker) | 15–20 ms per tile (est.) | Bake per-tile plant sites (~7 MB); `Trampled` stays at spawn. |
| 8 | Near 2 m ground tile (67×67 halo, weights, edges, water clip) | `world/landscape.rs:50-94` | stream-in and re-entry, main thread | 8–10 ms per tile (est.); walk mean 8.8 ms vs p99 13.7 ms (meas.) | Slice from the baked grids, or at least cache it and build it async. |
| 9 | Cordon wall chunks: vertices, ground patch, ~166 colliders each with `mesh_height_at` | `world/cordon.rs:669,700,776,830` | every anchor-tile change, main thread, **uncapped** (up to 5 per frame) | 4–5 ms per chunk, so 20+ ms frames (est.) | Bake per-chunk wall buffers and pillar poses. |
| 10 | The OnEnter(InGame) frame, which includes 11–15 and the spawns | `world/mod.rs:348` | once | 310–330 ms, then 178–193 ms (meas.) | Partly; see 11–15. |
| 11 | Audio synthesis: 150 s ambience, the hangar loop and ~25 clips | `audio/mod.rs:74,102`, `ambience.rs:45`, `effects.rs`, `creatures.rs:23` | startup, main thread | 200–350 ms (est.); first frame 382–456 ms (meas.) | Bake WAV/OGG to `assets/audio/`. |
| 12 | Road mask texture, ~3,900×2,100 RGBA8, no mips | `world/roads.rs:389`, `terrain_material.rs:56` | startup, main thread | 50–150 ms (est.) | Bake `road_mask.ktx2` with mips. |
| 13 | Wildlife route planning | `world/wildlife.rs:198,311`, `wildlife/mutants.rs:449,495` | OnEnter, main thread | 50–100 ms (est.) | Bake the route tables. |
| 14 | Wildlife rig weights and skinned bounds (13 species) | `wildlife/rig.rs:320`, `wildlife.rs:166` | OnEnter, main thread | 10–30 ms (est.) | Write JOINTS_0/WEIGHTS_0 and inverse binds into the GLBs once. |
| 15 | Fog field texture (513², blur, half floats) | `ground_fog/field.rs:82-110`, `landscape.rs:190` | OnEnter, main thread | 25–40 ms (est.) | Bake the RGBA16F (2.1 MB). |
| 16 | Bevy `calculate_bounds` on every mesh without an `Aabb`: canopies, ~760 road modules, marsh, rocks, town, every UHD scan spawn | `bevy_camera visibility/mod.rs:557`; `canopy.rs:407,450`, `old_road.rs`, `sites.rs:865` | OnEnter (60–80 ms) and each UHD spawn (0.5–2 ms) (est.) | wood PostUpdate max 53.6 ms (meas., unattributed) | Read the POSITION min/max once and insert `Aabb` + `NoAutoAabb`, as `plants.rs:178-200` does. |
| 17 | Pathfinding grid: ≤260k cells × `water_depth_at`, plus collider rasterisation | `player/path.rs:61,96,127`, `orders.rs:224` | **every click / order, main thread** | 100–340 ms per click (est.) | Bake a deep-water bitmap and a static obstacle raster. |
| 18 | Constants recomputed in hot paths: landmark foundation, base height, rotation, relief blur; `town::pad_level`; `clear_footprint` over 53 buildings | `world/sites.rs:575-779`, `town.rs:124-231`, `terrain.rs:196` | every `height_at` / `clear_footprint` | about half of each `height_at` (est.) | `LazyLock` per-landmark constants plus a 64 m cell → sites index. |
| 19 | CPU mipmaps for 9 PNGs and the foliage texture; PNG decode of the splash, map and decal | `terrain_material.rs:100,127`, `scenery.rs:345`, `assets/mod.rs` | startup | 30–50 ms (est.) | Ship KTX2 BC7/BC5 with mips. |
| 20 | Skinned bounds for the soldier (1M vertices) and the guards | `bevy_gltf loader/mod.rs:863` | load and re-load | 20–50 ms (est.) | No bake; switch the policy to BindPose. Low priority. |
| 21 | Per-pixel noise in the terrain, fog and shroud shaders | `terrain.wesl:26-38`, `ground_fog.wesl:109-143,446-450` | per frame, GPU | unmeasured (Xephyr is vsync-bound) | Possible noise atlas; low priority until GPU timing exists. |

## Already precalculated

- **GLB data:** every GLB has normals and KTX2 textures with mips. Normal-mapped GLBs have tangents, except the 10 UHD
  tree and plant scans that Task 1b bakes.
- **Collision and relief:** collision pillars are compile-time constants, and so are the relief grids.
- **Plant bounds:** measured once per model.
- **Built once per run:** the distant terrain tiles, the scenery and rock site lists, the fog field and the road mask.

## Not precalculable

- **The shroud mask:** it is the player's progress.
- **Shader pipelines:** Bevy 0.20 has no on-disk pipeline cache.
