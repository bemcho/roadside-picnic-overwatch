# Smooth Streaming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the 117-180 ms frames at every 128 m tile crossing while walking and driving.

**Architecture:** Cap the work per frame in the three streaming systems: plants spawn in batches of `PLANTS_PER_FRAME` with their bounding box given up front, scenery tiles spawn one per frame and ground tiles switch to detail one per frame, nearest an anchor first. Unloading stays immediate.

**Tech Stack:** Rust 2024, Bevy 0.20.1 ECS (`bevy::camera::primitives::{Aabb, MeshAabb}`, `bevy::camera::visibility::NoAutoAabb`).

**Spec:** `docs/superpowers/specs/2026-11-23-smooth-streaming-design.md`

## Global Constraints

- Bevy 0.20.1, no new crates.
- `cargo clippy -- -D warnings` passes; no `unwrap()` in game logic; systems are `verb_noun`, at most 7 parameters and about 60 lines.
- WASM-compatible game code: no `std::thread` outside tests.
- Agents do not commit in this repo (user rule): each task ends at a checkpoint; the final step suggests a `main:` commit message.
- Playtests: Xephyr :5, `XDG_DATA_HOME=$PWD/target/playtest/data`, stop only processes you started.
- Target: no frame over about 33 ms at a tile crossing in the traced walk and drive.

## Review Focus

1. The stalker turns back and a tile despawns while its plants are still spawning: no panic, nothing left behind (Task 1 test).
2. A plant trampled after its tile finished growing but before its batch spawned: it never appears (Task 1 test).
3. A plant model without bounds (mesh missing): Bevy computes its bounds as before (Task 1 test).
4. Two anchors (stalker and camera focus) with overlapping neighborhoods: each tile spawns once (Task 2 test).
5. A ground tile that leaves range before its turn: it never switches to detail (Task 3 test).

---

### Task 1: Plants spawn in batches with precomputed bounds

**Files:**
- Modify: `src/world/plants.rs` (`PlantLook`, `prepare_plant_looks`, `spawn_grown_plants`, `spawn_plants`, tests)
- Modify: `src/world/crush.rs` (`Trampled`)
- Modify: `src/world/mod.rs:177` (registration)

**Interfaces:**
- Produces: `const PLANTS_PER_FRAME: usize = 150;`
- Produces: `#[derive(Component)] pub(super) struct GrownPlants(Vec<PlantSite>);`, a ready tile's plants still to spawn.
- Produces: `pub(super) fn finish_growing_plants(mut commands: Commands, mut tiles: Query<(Entity, &mut PendingPlants)>)`: every ready task becomes `GrownPlants` on its tile (`PendingPlants` removed).
- Produces: `pub(super) fn spawn_grown_plants(mut commands: Commands, mut tiles: Query<(Entity, &mut GrownPlants)>, looks: Res<PlantLooks>, trampled: Res<Trampled>)`: spawns at most `PLANTS_PER_FRAME` plants per frame across tiles, skipping trampled sites at spawn time, and removes `GrownPlants` once empty.
- Produces: `PlantLook { far, near, paint, near_bounds: Option<Aabb>, far_bounds: Option<Aabb> }`.
- Produces: `Trampled::trample(&mut self, at: Vec3) -> bool` (true when newly trampled); `trample_plants` uses it.
- Registration: `(stream_scenery, plants::finish_growing_plants, plants::spawn_grown_plants).chain()` in `PreUpdate`.

- [ ] **Step 1: Write the failing tests** in `plants.rs` tests. A helper builds a tile entity with `GrownPlants` of `n` `PlantSite { kind: Kind::DryGrass, variant: 0, pose: Transform::from_xyz(i as f32, 0.0, 0.0) }`, in an app with `MinimalPlugins`, `AssetPlugin`, `PlantLooks`, `Trampled` and `spawn_grown_plants`.
  - `a_grown_tile_spawns_at_most_plants_per_frame_until_done`: with `2 * PLANTS_PER_FRAME + 1` sites, `Plant` children count is `PLANTS_PER_FRAME` after one update, `2 * PLANTS_PER_FRAME + 1` after three, and the tile no longer has `GrownPlants`.
  - `plants_trampled_while_waiting_never_spawn`: 2 sites, `trample` the first site's position, update twice: exactly 1 `Plant`.
  - `a_tile_despawned_mid_spawn_drops_its_plants`: `2 * PLANTS_PER_FRAME` sites, update, despawn the tile, update: no panic and no `Plant` entities.
  - `plants_carry_their_models_bounds`: give the `DryGrass` variant 0 look `near_bounds`/`far_bounds` = `Some(Aabb::from_min_max(Vec3::ZERO, Vec3::ONE))` and leave the `Shrub` variant 0 look at its default `None`; spawn one plant of each. The first plant and its far child have that `Aabb` and `NoAutoAabb`; the second and its child have neither.
  - Update `streamed_tiles_grow_plants_in_the_background_then_spawn_them`: register `(finish_growing_plants, spawn_grown_plants).chain()` and loop until the root has neither `PendingPlants` nor `GrownPlants`; the count assertion stays.

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test world::plants`
Expected: compile errors for `GrownPlants`, `PLANTS_PER_FRAME`, `finish_growing_plants`, `near_bounds`, `trample`.

- [ ] **Step 3: Implement the interfaces above.** `prepare_plant_looks` takes `meshes: Res<Assets<Mesh>>` and fills the bounds with `meshes.get(&handle).and_then(MeshAabb::get_aabb)` once per look. `spawn_plants` inserts `(aabb, NoAutoAabb)` on the near entity and the far child only when the look has bounds. Update the doc comments ("Spawns at most one grown tile per frame" no longer holds).

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -- world::plants world::crush`
Expected: all pass.

- [ ] **Step 5: Checkpoint:** `cargo clippy -- -D warnings` clean. No commit.

### Task 2: Scenery tiles spawn one per frame, nearest first

**Files:**
- Modify: `src/world/mod.rs` (helper next to `near_anchor`)
- Modify: `src/world/scenery.rs:206-262` (`stream_scenery`, tests)

**Interfaces:**
- Produces: `fn anchor_distance(tile: IVec2, anchors: &[IVec2]) -> i32` in `world/mod.rs`: the Chebyshev tile distance to the nearest anchor, `i32::MAX` with no anchors.
- `stream_scenery` keeps its signature. When the anchor tiles change it despawns tiles outside the neighborhood (as now). Every frame it then spawns at most one tile: of the `neighborhoods(&centers, 1)` tiles that have sites and are not active, the one with the smallest `anchor_distance`.

- [ ] **Step 1: Write the failing tests** in `scenery.rs` tests, with the setup of `distant_colliders_unload_and_return_at_the_same_position` and empty `Sites` for every tile around `IVec2::ZERO`; the player stands at `(64, 0, 64)`.
  - `new_tiles_fill_in_one_per_frame_nearest_first`: after one update `active` holds only `IVec2::ZERO`; after nine updates it holds all 9.
  - `distant_tiles_unload_at_once`: after filling, move the player to the middle of tile `(10, 10)`; after one update none of the 9 tiles is in `active` and their entities are gone.
  - `overlapping_anchors_spawn_each_tile_once`: a second `(StreamAnchor, Transform)` in tile `(1, 0)`, sites for the 12 tiles of both neighborhoods; after 20 updates `active.len() == 12` and the root has 12 children.
  - `anchor_distance_counts_tiles_to_the_nearest_anchor`, in the existing `tests` module of `world/mod.rs`: `anchor_distance(IVec2::new(3, -1), &[IVec2::ZERO, IVec2::new(2, 0)]) == 1` and `anchor_distance(IVec2::ZERO, &[]) == i32::MAX`.

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test world::scenery`
Expected: the fill-in test fails (all 9 tiles spawn in one update); `anchor_distance` does not compile.

- [ ] **Step 3: Implement `anchor_distance` and the one-tile-per-frame `stream_scenery`.** Keep the tile spawn itself (children, `PendingPlants::grow`) unchanged; move it into a helper if the system grows past about 60 lines.

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -- world::scenery world::tree_fall world::tests`
Expected: all pass, including the existing `distant_colliders_unload_and_return_at_the_same_position` and the tree-fall streaming test.

- [ ] **Step 5: Checkpoint:** `cargo clippy -- -D warnings` clean. No commit.

### Task 3: Ground tiles switch to detail one per frame, nearest first

**Files:**
- Modify: `src/world/landscape.rs:45-85` (`stream_ground_detail`, tests)

**Interfaces:**
- Consumes: `anchor_distance` from Task 2.
- `stream_ground_detail` keeps its parameters except the `last` local, which goes. Every frame: each tile that is detailed but no longer within 2 tiles of an anchor switches back to its distant mesh and water at once; of the tiles within 2 tiles still distant, the one with the smallest `anchor_distance` switches to its detailed mesh and water. Split the two switches into helpers to stay under about 60 lines.

- [ ] **Step 1: Write the failing tests** in `landscape.rs` tests, with the setup of `ground_detail_follows_the_player_into_the_new_land` and 25 `GroundLod` tiles over `-2..=2` in x and z, all sharing one distant `Cuboid` mesh; the player stands at `(64, 0, 64)`.
  - `ground_detail_fills_in_one_tile_per_frame_nearest_first`: after one update only tile `(0, 0)` has a mesh other than the distant one; after 25 updates all 25 do.
  - `ground_detail_unloads_at_once`: after filling, move the player to the middle of tile `(12, 12)`; after one update all 25 show the distant mesh.
  - `a_tile_that_leaves_range_before_its_turn_stays_distant`: one update, then move the player to the middle of tile `(8, 0)`; after 25 more updates all 25 tiles show the distant mesh.

- [ ] **Step 2: Run the tests and see them fail**

Run: `cargo test world::landscape`
Expected: the fill-in test fails (all 25 switch in one update).

- [ ] **Step 3: Implement the per-frame switch as described above.**

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test world::landscape`
Expected: all pass, including `ground_detail_follows_the_player_into_the_new_land`.

- [ ] **Step 5: Checkpoint:** `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test` all clean. No commit.

### Task 4: Measure, tune and document

**Files:**
- Modify: `src/world/plants.rs` (`PLANTS_PER_FRAME`, only if the trace says so)
- Modify: `CLAUDE.md` (Plants bullet: "one tile is spawned per frame"; Scenery bullet: "Colliders and undergrowth stream in a 3×3 chunk neighborhood")
- Modify: `arch-readme.md` (pitfall 21)

- [ ] **Step 1: Build the traced binary**

Run: `cargo build --release --features bevy/trace_chrome --target-dir target/trace`
Expected: `Finished`.

- [ ] **Step 2: Re-run the walk and the drive** exactly as before (Xephyr :5 at 1280x720; `run-traced.sh -r show_ready -T 300`).
  - Walk: a fresh `target/playtest/data`, `--new`, steps `sleep:1 key:space:0.2 sleep:16 down:Shift_L down:w sleep:45 up:w up:Shift_L sleep:2`.
  - Drive: a save with `stalker 841.4 3.0 -30.0` and `car 839.0 2.0 -30.0 0.0`, steps `sleep:1 key:space:0.2 sleep:16 key:e:0.2 sleep:1.5 down:w sleep:18 up:w key:space:1.5 sleep:2`.
  - Summarize with `summarize.py --window walk:29:73` and `--window drive:28.4:46.5`; check the stalker or car reached about the same place as before (`z ≈ 95` walking, `z ≈ 368` driving).

Expected: the `main app` max under about 33 ms in both windows. If `spawn_grown_plants` frames exceed it, lower `PLANTS_PER_FRAME` and re-run. If another span still spikes, write it down for the next project instead of widening this one.

- [ ] **Step 3: Update the docs.** CLAUDE.md: plants spawn at most `PLANTS_PER_FRAME` per frame with their models' bounds precomputed; scenery tiles and detailed ground tiles fill in one per frame, nearest first, and unload at once. arch-readme pitfall 21: the same budgets, and that GLB meshes keep their CPU copy, so Bevy rescans every vertex per new entity unless it gets an `Aabb` and `NoAutoAabb`.

- [ ] **Step 4: Final checks:** `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`, `git diff --check`. Stop the Xephyr you started, delete the raw `*.json` traces, keep the summaries.

- [ ] **Step 5: Suggest a `main:` commit message** (at most 10 lines) with the before/after numbers. Do not commit.
