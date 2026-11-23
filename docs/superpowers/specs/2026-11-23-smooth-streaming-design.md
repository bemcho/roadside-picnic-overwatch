# Smooth streaming while walking and driving

## Intent

Walking and driving stutter: every time the stalker or the car crosses into a
new 128 m ground tile, two to four frames take 117-180 ms (measured on the RTX
2060 with `trace_chrome`, 2026-11-23; a normal frame is about 9 ms). The goal
is smooth movement on this card first; per-GPU quality levels are a separate,
later project.

## What the trace showed

In the crossing frames:

- `calculate_bounds` 57-106 ms: plant meshes come from GLBs and keep their
  CPU copy, so Bevy never caches their bounding box and rescans every vertex
  for each new plant entity.
- `check_entities_needing_specialization<PlantMaterial>` 57-106 ms: thousands
  of new plant entities (a near model plus a far child per plant) arrive in
  one frame, one whole tile per frame.
- `stream_scenery` commands 35-55 ms: every new scenery tile's colliders,
  standing-tree colliders and undergrowth spawn in the same frame.
- `stream_ground_detail` 14-18 ms: the new detailed ground tiles (about 3 ms
  each) are built on the main thread in the same frame.

Shader compiles and the close-up tree scans played no part.

## Goals, non-goals

- Goal: no frame over about 33 ms at a tile crossing, walking or driving.
- Non-goal: changing how plants, trees or ground look or behave; trampling,
  the nearest-plant UHD swap and near/far fades stay.
- Non-goal: lowering the steady per-frame cost or GPU quality levels (next
  project).

## Approaches

- **A. Spread the work out (chosen).** Cap the work per frame in the three
  streaming systems and give plants their bounding box up front. Small and
  local; a new tile fills in over a fraction of a second, at least 128 m away.
- B. Merge each tile's plants into a few combined meshes. Fixes the stutter
  and the steady cost, but reworks trampling, the UHD swap and fades, and
  duplicates geometry in VRAM (risky on 6 GB).
- C. Load tiles earlier. Only moves the spike.

## Design

1. **Plants** (`plants.rs`): `prepare_plant_looks` computes each look's near
   and far mesh `Aabb` once. `spawn_plants` inserts that `Aabb` with
   `NoAutoAabb`, so `calculate_bounds` skips plants. `spawn_grown_plants`
   spawns at most `PLANTS_PER_FRAME` plants per frame (first value 150; tune
   from the trace), keeping a ready tile's remaining plants on the tile until
   it is done. Trampled plants are still skipped.
2. **Scenery tiles** (`scenery.rs`): `stream_scenery` despawns tiles that left
   the neighborhood as now, but spawns at most one new tile per frame, the one
   nearest an anchor first, so the stalker's own tile always comes first.
3. **Ground detail** (`landscape.rs`): `stream_ground_detail` switches at most
   one tile to its detailed mesh per frame, nearest an anchor first. Switching
   back to the distant mesh stays immediate (it only swaps handles).
   *My call:* this replaces the worker thread floated in approach A, since a
   tile costs about 3 ms and one per frame stays well within budget without
   tasks.

## Verification

- Unit tests: plants spawn over several frames and all arrive; plants carry
  an `Aabb` and `NoAutoAabb`; scenery and ground detail fill in one tile per
  frame, nearest first, and still unload at once.
- `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`.
- Re-run the same traced walk and drive (Xephyr :5, scratch `XDG_DATA_HOME`)
  and compare with `target/playtest/{walk,drive}_hitch.txt`.
- Docs: CLAUDE.md Plants bullet ("one tile is spawned per frame"),
  arch-readme pitfall 21.

## Left to the builder

The exact budget numbers, after measuring.
