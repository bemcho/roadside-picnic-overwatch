# Golden pit haul road Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Chapter 4 road's straight chute into the Golden Sphere's terraced pit with one steady-grade
ledge road that curves down the pit's wall to the floor.

**Architecture:**
- The first quarry's planner (`tools/plan_quarry_ramp.py`) is generalised to any relief, entrance, target and
  no-go boxes.
- The pit's land, before roads, is exported once from the Rust terrain as a relief file in metres.
- The planner bakes the centerline into a generated file. `roads.rs` runs it as a second `Surface::Ramp` track,
  so the existing ramp grading (one grade from the entrance, rounded crest) and the terrain's road flattening
  carve it.

**Tech Stack:** Rust, Bevy 0.20, Python 3 (stdlib only) for the offline planner.

**Spec:** `docs/superpowers/specs/2026-11-26-golden-pit-ramp-design.md`

## Global Constraints

- Project rules (`CLAUDE.md`):
  - Precalculate once: the centerline and the relief are baked files, regenerated, never hand-edited, never
    searched at load.
  - No `unwrap()` in game logic.
  - `use` order: std, external, crate, super.
  - `cargo clippy -- -D warnings`, `cargo fmt -- --check` and `cargo test` must be clean.
  - Read and update `doc/world-reference.md` in the same change.
- Reuse `Surface::Ramp` and `graded_track`. The first leg is the drive: it follows the land and is at least
  12 m long (half `CREST_LENGTH`). From the entrance the road falls at one grade to its last point's level.
- The ramp's grade is at most 0.15 (aim 0.12–0.15). Corners are no tighter than 9 m radius.
- The road, shoulders included (`RAMP_SHOULDER` = 9 m), stays out of every diorama footprint (the Quarry rim
  scan above all) and off the Golden Sphere's pad.
- The Chapter 4 route ends at the rim point `GOLDEN_RIM = map(1100.0, 368.0)`. The ramp starts there and ends
  exactly at `GOLDEN_SPHERE` (the existing constant, `map(1185.0, 348.0)`), so the Sphere's drive still starts
  at its junction.
- The first quarry's ramp, `src/world/pillars/quarry_ramp.rs`, must regenerate byte-identical from the
  generalised planner.

## Review Focus

1. **The rim junction.** Where the Chapter 4 dirt road ends at `GOLDEN_RIM` and the ramp's drive begins, the
   road level and the terrain have no step. Test: Task 3 `the_golden_ramp_meets_the_chapter_4_road_level`.
2. **Other tracks nearby.** The ramp passing near another track (the Chapter 4 leg from `QUARRY`, the Quarry
   rim's drive) would let the nearest-leg choice in `RoadNetwork::sample` flip the level between them. Test:
   Task 3 `the_golden_ramp_keeps_clear_of_other_tracks`.
3. **The spiral meeting itself.** Runs of the ramp passing within a shoulder of each other merge their cuts.
   Test: Task 2's acceptance numbers (runs at least 2 × `RAMP_SHOULDER` apart), and Task 1
   `test_runs_keep_apart_is_reported`.
4. **Water on the pit floor.** A flooded road end would strand the car. Test: Task 3
   `the_golden_ramp_is_dry`.
5. **Launching at the crest.** A car or carrier at speed would leave the ground at the entrance if the grade
   change were sharp. The crest rounding is reused. Test: Task 3 `the_golden_ramp_is_no_steeper_than_its_grade`
   checks the sampled level everywhere, and the playtest drives it at full throttle.

---

### Task 1: Generalise the ramp planner

**Files:**
- Modify: `tools/plan_quarry_ramp.py`
- Test: `tools/test_plan_quarry_ramp.py`

**Interfaces:**
- Produces this CLI. The defaults reproduce today's quarry run exactly.
  ```
  python3 tools/plan_quarry_ramp.py RELIEF OUTPUT [--grade G] [--scale S]
      [--entrance X,Z] [--heading H] [--half X,Z] [--grid N]
      [--end X,Z] [--avoid XMIN,ZMIN,XMAX,ZMAX]... [--max-length M]
  ```
- `--entrance`, `--half`, `--end` and `--avoid` take model units (metres when `--scale 1`).
- `--heading` is an index into `HEADINGS`.
- `--end`: the search ends at the cell nearest that point instead of on the floor. The baked path's last
  point is exactly that point.
- `--avoid`: a box that no point of the road, shoulders included, may enter. Every baked point keeps at
  least `--shoulder` metres (default 9.0) from it. The option repeats.
- The module constants `ENTRANCE`, `HALF_X`, `HALF_Z`, `GRID`, `START_HEADING` and `MAX_LENGTH_M` become the
  defaults of a `Site` dataclass passed through `find_cells` / `plan_path` / `measure`.

- [ ] **Step 1: Write the failing tests** in `tools/test_plan_quarry_ramp.py`:
  - `test_quarry_ramp_regenerates_unchanged`: run `main()` with the quarry's arguments into a temp file and
    assert it equals `src/world/pillars/quarry_ramp.rs`.
  - `test_ends_at_the_given_point`: a synthetic cone pit (a 64×64 relief in metres, `--scale 1`, 40 m deep,
    300 m across) with `--end` set to a point on its floor. The last baked point equals the end point.
  - `test_keeps_out_of_avoid_boxes`: the same cone with a box across the straight line from the entrance to
    the end. Every baked point is at least 9 m outside the box.
  - `test_runs_keep_apart_is_reported`: the closest approach of separate runs is reported, and the stats
    expose it.
  - `test_refuses_steeper_than_grade` still holds with a `Site`.
- [ ] **Step 2:** Run `python3 -m unittest tools/test_plan_quarry_ramp.py -v`. The new tests fail.
- [ ] **Step 3: Implement.**
  - Add `Site`.
  - Thread it through the search.
  - Cells whose point lies within shoulder of an avoid box are impassable.
  - The `--end` stop condition replaces the floor test when given. It keeps the `END_BELOW_GROUND_M` level
    rule measured at the end cell. The target point is appended after the arcs.
  - The arcs are checked against avoid boxes too. Refuse to write (non-zero exit) if any baked point
    violates one.
  - Update the docstring (commands for both sites, what each option means).
- [ ] **Step 4:** Run the unittest. All pass.
- [ ] **Step 5:** Run `python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_quarry_pit_ground.rs
  /tmp/q.rs && cmp /tmp/q.rs src/world/pillars/quarry_ramp.rs`. They are identical.
- [ ] **Step 6: Commit** `golden-pit-ramp: let the ramp planner take any relief, entrance, end and no-go box`.

### Task 2: Bake the pit's relief and its ramp

**Files:**
- Modify: `src/world/terrain.rs` (an ignored test at the end of its tests module)
- Create (generated): `src/world/pillars/golden_pit_ground.rs`, `src/world/pillars/golden_pit_ramp.rs`

**Interfaces:**
- Consumes: Task 1's CLI.
- Produces:
  - `golden_pit_ground.rs`: a `Relief { min, cell, heights }` literal in world metres, 96×96 heights of
    `sited_height` (the land with the sites' pads and dioramas, before roads and water). It covers a box
    holding `GOLDEN_RIM`, `GOLDEN_SPHERE` and the pit's west half: x from −1250 to −925, z from −230 to 170.
    Widen the box if the path needs it. The header reads
    `// Generated by \`cargo test write_golden_pit_relief -- --ignored\`; do not edit.`
  - `golden_pit_ramp.rs`: a `&[Vec2]` literal in world xz metres. The first point is the entrance, 15–40 m from
    `GOLDEN_RIM` along the line into the pit. The last point is exactly `GOLDEN_SPHERE`.

- [ ] **Step 1: Write the ignored test** `write_golden_pit_relief` in `src/world/terrain.rs`'s tests.
  - It writes the file above.
  - It prints the world-space avoid boxes for the run: every diorama's footprint
    (`sites::quarry_footprint()` and any other diorama whose box meets the export box) and the Golden Sphere's
    pad. Add a `pub(super)` helper in `sites.rs` only if one is missing.
- [ ] **Step 2:** Run `cargo test write_golden_pit_relief -- --ignored --exact` (use the full test path).
  The file is written.
- [ ] **Step 3: Run the planner.**
  ```
  python3 tools/plan_quarry_ramp.py src/world/pillars/golden_pit_ground.rs \
      src/world/pillars/golden_pit_ramp.rs --scale 1 --grade 0.15 \
      --entrance <E> --heading <toward the pit> --half <box> --end <GOLDEN_SPHERE> --avoid <each box>
  ```
  Raise `--grid` (64 or more) so that cells are under about 8 m, and `--max-length` as the pit needs.
- [ ] **Step 4: Accept the path only if all of these hold.** Otherwise adjust the entrance or heading, then
  report:
  - grade at most 0.15;
  - largest fill at most 7 m and largest cut at most 6 m, against the relief as it is;
  - turn radius at least 9 m;
  - separate runs at least 18 m apart.

  Record the command and the printed numbers in the task report and in the generated file's header comment
  (the tool writes it).
- [ ] **Step 5: Commit** `golden-pit-ramp: bake the pit's land and a ledge road down it`.

### Task 3: Run the ramp, test it, document it, playtest it

**Files:**
- Modify: `src/world/roads.rs`, `src/world/mod.rs` (re-export if the router test needs it),
  `src/player/path.rs` (test), `doc/world-reference.md`, `CLAUDE.md` (table row), `arch-readme.md` if it names
  the quarry ramp.

**Interfaces:**
- Consumes: `golden_pit_ramp.rs` (`&[Vec2]`, world xz).
- Produces:
  - `pub(super) const GOLDEN_RIM: Vec2 = map(1100.0, 368.0);`
  - `pub(crate) fn golden_pit_ramp() -> Vec<Vec2>` (in `roads.rs`): `GOLDEN_RIM`, then the baked points.

- [ ] **Step 1: Write the failing tests.**

  In `roads.rs`:
  - Replace `quarry_ramp_track()`'s "one ramp track only" with `ramp_tracks()`, which returns both. The quarry
    tests take the one starting at `QUARRY`.
  - `the_golden_ramp_starts_on_the_chapter_4_road`: the ramp's first point equals the Chapter 4 route's last
    point, `GOLDEN_RIM`, and no route still passes `map(1146.0, 368.0)`.
  - `the_golden_ramp_ends_at_the_golden_sphere`: its last point equals `GOLDEN_SPHERE`.
  - `a_ramp_is_graded_evenly`: run for both ramps.
  - `the_golden_ramp_keeps_clear_of_other_tracks`: every ramp point past the first 30 m lies more than
    `2.0 * RAMP_SHOULDER` from every non-ramp track's legs.

  In a test with the built `Terrain` (where the existing terrain tests build one):
  - `the_golden_ramp_meets_the_chapter_4_road_level`: the road heights 1 m either side of `GOLDEN_RIM` differ
    by less than 0.2 m.
  - `the_golden_ramp_is_no_steeper_than_its_grade`: sample `roads.sample(p).height` every 2 m along the
    ramp from its entrance. Every chord grade is at most 0.155.
  - `the_golden_ramp_keeps_off_the_dioramas`: for points every 3 m along it and 9 m either side,
    `sites::diorama_ground(p)` is `None`.
  - `the_golden_ramp_is_dry`: `water_depth_at` is 0 along it.

  In `path.rs`:
  - `a_click_order_reaches_the_golden_sphere`, alongside `a_click_order_reaches_the_pit_floor`: a route from
    `GOLDEN_RIM` to the ramp's end exists and is no longer than 1.6 × the ramp's length.
- [ ] **Step 2:** Run `cargo test world::roads player::path`. The new tests fail.
- [ ] **Step 3: Implement.**
  - The Chapter 4 route ends `…, QUARRY, GOLDEN_RIM`.
  - `GOLDEN_PIT_RAMP` is `include!("pillars/golden_pit_ramp.rs")`.
  - `tracks()` chains `(Surface::Ramp, golden_pit_ramp())`.
  - Update the `Surface::Ramp` doc and `tracks()`' doc: two ramps.
- [ ] **Step 4:** Run `cargo test`, `cargo clippy -- -D warnings` and `cargo fmt -- --check`. All are clean.
- [ ] **Step 5: Docs.**
  - `doc/world-reference.md`:
    - The Terrain bullet ("for the quarry's haul road alone" becomes both pits).
    - A *Golden pit* sub-bullet under **Quarry ramp**, with the export and planner commands, the length,
      grade, fill and cut.
  - `CLAUDE.md`: the table row lists `golden_pit_ramp.rs` and `golden_pit_ground.rs` as generated.
- [ ] **Step 6: Playtest** (`cargo run --profile playtest`, Xephyr, `XDG_DATA_HOME` scratch, as the
  bevy-game-dev skill's gameplay-verification reference says).
  - The stalker walks from `GOLDEN_RIM` to the Sphere by click order.
  - The car and the carrier drive down and back up at full throttle, with no launch at the crest.
  - Close shots: from the rim down the road, from the floor up, and of the cut banks and fills, including any
    step at the 9 m shoulder edge on the bench shelves.
  - Frame time unchanged.
  - Remove temporary debug systems.
- [ ] **Step 7: Commit** `golden-pit-ramp: run a ledge road down to the Golden Sphere`.
