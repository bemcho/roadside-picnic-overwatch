# Golden crater ramp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The only road to the Golden Sphere runs through the Quarry rim: from the first haul ramp's end on the Quarry rim's floor, on to the Golden Sphere's entrance, then spiralling down through the crater scan's walls to the very bottom, beside the Sphere.

**Architecture:** One more `Surface::Ramp` track, built like `sites::quarry_ramp()`: `[quarry ramp's last point, GOLDEN_SPHERE, core entrance, baked spiral under the core's pose]`. The spiral is baked once by `tools/plan_quarry_ramp.py` from the core scan's relief (`uhd_core_ground.rs`, model units). The scan cut (`ramp_cut.rs`) and the collision clearing (`quarry_ramp.rs`) are made per scan, so the core scan is cut along the spiral as the Quarry rim is along its ramp.

**Tech Stack:** Rust, Bevy 0.20, WESL; Python 3 stdlib for the planner.

**Spec:** `docs/superpowers/specs/2026-11-26-golden-pit-ramp-design.md`, section "Revision 2" (it supersedes the earlier "Calls made" and the south-west rim road). Branch `golden-pit-ramp`, base `c737993`.

## Global Constraints

- The only way in is through the Quarry rim. The Chapter 4 route ends at `QUARRY`; nothing else reaches the Golden Sphere's pit.
- Precalculate once: the spiral is a generated file (regenerate, never hand-edit), never searched at load. The cut image and the cleared collision are built once, as the Quarry rim's are.
- The spiral's grade is at most 0.19; `the_ramp_grade_stays_drivable` (centerline 0.25, lanes 0.35) holds for every ramp; corners ≥ 9 m radius; separate runs ≥ 18 m apart; it turns one way only.
- Cut depth inside the crater is not limited (the user: "the road will make its path through the walls"). Report the largest cut and fill.
- The road ends at the very bottom (crater floor, ≈ −100 m) beside the Golden Sphere, never over the Sphere itself.
- `tools/plan_quarry_ramp.py` regenerates `src/world/pillars/quarry_ramp.rs` byte-identical (`test_quarry_ramp_regenerates_unchanged`).
- The Quarry rim's first ramp, its grade and its cut stay as they are.
- No `unwrap()` in game logic; `use` order std, external, crate, super; `cargo clippy -- -D warnings`, `cargo fmt -- --check`, `cargo test` clean.
- Update `doc/world-reference.md` (Terrain bullet, Quarry ramp bullet) and the CLAUDE.md table row in the task that changes the behaviour.
- Commit messages start `golden-pit-ramp: ` and end with `Claude-Session: https://claude.ai/code/session_013NVSdB36MzEA4ix3Z4qrfa`.

## Review Focus

1. **Stone over the road.** Any scan geometry left above the drawn ground in the spiral's corridor shows as slivers or walls on the road. Test: Task 3 `the_core_cut_lies_below_the_ground_that_is_drawn`.
2. **Invisible walls.** A kept core pillar standing in the cut stops the stalker or a car on the spiral. Test: Task 3 `no_core_pillar_stands_on_the_ramp`.
3. **The connector inside the Quarry rim.** Its first ~31 m lie on the Quarry rim's floor inside that scan's footprint; stone there must be cut too. Test: Task 3 extends the Quarry rim's cut tests to the connector's leg.
4. **Overlapping laps.** Two laps of the spiral closer than the shoulders make one road's bank cut the other. Test: Task 2 `the_crater_ramp_laps_keep_apart` (≥ 18 m) and Task 1's planner check.
5. **The bottom.** The last point is on the crater floor, beside the Sphere and not over it. Test: Task 2 `the_crater_ramp_ends_beside_the_sphere`.

---

### Task 1: Plan the spiral

**Files:** Modify `tools/plan_quarry_ramp.py`, `tools/test_plan_quarry_ramp.py`. Create `src/world/pillars/golden_crater_ramp.rs` (generated).

**Interfaces:** Produces `golden_crater_ramp.rs`: a `&[Vec2]` literal in the core scan's model units (x, z), first point the core's entrance on its front edge (+z model, ≈ (0, 0.49)), last point on the crater floor beside the Sphere. Report the Sphere's position in model units.

- [ ] Step 1 (tests first): `test_turns_one_way_only` (with `--turn left` or `right`, every corner of the baked road turns that way), `test_keeps_laps_apart` (with `--min-separation 18`, the printed closest approach of separate runs is ≥ 18 m, and a road that cannot meet it is refused, exit non-zero, nothing written), `test_baked_grade_meets_the_target` (on a road with many corners the baked grade is ≤ `--grade`: today the arcs shorten the path and the grade overshoots, e.g. 0.181 for 0.18).
- [ ] Step 2: Implement `--turn {left,right}`, `--min-separation M` (enforced in the search, not only reported) and the grade fix (plan for the baked length, or lengthen the level window by what the arcs remove). The quarry stays byte-identical.
- [ ] Step 3: Find where the Sphere stands in `landmark-core` (inspect the GLB, e.g. with Blender or glTF Transform; the relief's lowest region is near model (0.075, 0)). Choose an end on the floor 6–12 m from the Sphere's edge.
- [ ] Step 4: Run, e.g. `python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_core_ground.rs src/world/pillars/golden_crater_ramp.rs --scale 120 --grade 0.19 --centre=0,0 --half=0.43,0.47 --grid N --entrance=0,0.47 --heading 6 --end=X,Z --turn left|right --min-separation 18 --max-length 700`. Try both turn directions and a few grids; keep the road with the least total cut and fill that meets: grade ≤ 0.19, radius ≥ 9 m, laps ≥ 18 m apart. The whole road may leave the footprint only within the core's 60 m ground blend.
- [ ] Step 5: Report: the options, length, grade, largest cut and fill, laps' separation, end and its ground, a top-down plot over the relief (scratchpad). Commit "golden-pit-ramp: plan a spiral down the Golden Sphere's crater".

### Task 2: Wire the crater ramp

**Files:** Modify `src/world/sites.rs`, `src/world/roads.rs`, `src/world/terrain.rs`, `src/player/path.rs`, `src/world/mod.rs`, `doc/world-reference.md`, `CLAUDE.md`, `arch-readme.md`. Delete `src/world/pillars/golden_pit_ramp.rs`, `src/world/pillars/golden_pit_ground.rs`.

The south-west approach goes in this task, so every commit keeps the Golden Sphere's junction on the network.

**Interfaces:** Consumes `golden_crater_ramp.rs`. Produces `pub(crate) fn sites::crater_ramp() -> Vec<Vec2>`: `[quarry_ramp()'s last point, GOLDEN_SPHERE junction, core entrance]` then the baked points under the core's pose; and `pub(crate) fn sites::core_scan() -> (Transform, &'static [Pillar])` like `quarry_scan()`.

- [ ] Step 0: Remove the south-west approach: the Chapter 4 route ends `…, QUARRY` (drop `map(1100.0, 368.0)` and the four rim points), and `GOLDEN_RIM`, `GOLDEN_PIT_RAMP`, `golden_pit_ramp()`, its `tracks()` entry, its tests in roads.rs, terrain.rs and path.rs, the ignored `write_golden_pit_relief` export test, the two generated files, and `sites::diorama_footprints()` if nothing else uses it go. Keep the generic two-ramp test machinery (`ramp_tracks()`, the ramp-end allowance in `every_track_joins_the_network_back_to_the_gate`, the ramp loop in `the_ramp_grade_stays_drivable`).
- [ ] Step 1 (tests first, roads.rs unless named):
  - `the_only_road_to_the_golden_sphere_runs_through_the_quarry_rim`: no route or dirt drive comes within 400 m of `GOLDEN_SPHERE`; only the two ramp tracks do.
  - `the_crater_ramp_starts_at_the_quarry_ramps_end`: its first point equals `sites::quarry_ramp()`'s last, and it passes `GOLDEN_SPHERE` and the core's entrance.
  - `the_crater_ramp_ends_beside_the_sphere`: the last point lies on the crater floor (ground below −95 m) within 15 m of the Sphere and not inside it.
  - `a_ramp_is_graded_evenly` and `the_ramp_grade_stays_drivable` (terrain.rs) run over all three: quarry, crater.
  - `the_crater_ramp_laps_keep_apart`: points more than 60 m apart along the road are ≥ 18 m apart.
  - `the_crater_ramp_keeps_clear_of_other_tracks` (except its first 30 m, where it leaves the Quarry ramp).
  - The Golden Sphere has no separate dirt drive (`tracks()` filters `GOLDEN_SPHERE` like `QUARRY`); `every_place_stands_dry_on_its_own_ground_by_a_road` still holds at its entrance.
  - `a_click_order_reaches_the_crater_floor` (path.rs, beside `a_click_order_reaches_the_pit_floor`): a route from `QUARRY` to the crater ramp's last point exists.
- [ ] Step 2: Implement; the connector is the track's drive (first leg, follows the land).
- [ ] Step 3: Docs: remove the *Golden pit* sub-bullet and the south-west road; add to the Terrain bullet and a *Golden Sphere's crater* sub-bullet under **Quarry ramp** (path command and numbers, track, the connector); CLAUDE.md row.
- [ ] Step 4: `cargo test`, clippy, fmt; commit "golden-pit-ramp: run the Quarry rim's road on down into the Golden Sphere's crater".

### Task 3: Cut the core scan and clear its collision

**Files:** Modify `src/world/quarry_ramp.rs` (rename only if it reads better; keep `quarry_pillars()` callers working), `src/world/ramp_cut.rs`, `src/world/sites.rs`, `assets/shaders/ramp_cut.wesl` only if the image layout must change.

**Interfaces:** A per-scan cut: each cut scan (Quarry rim, Golden Sphere) has its own corridor (the ramp points within 9 m, `RAMP_SHOULDER`), its own cut image and bounds, and its own cleared pillars. The Quarry rim's corridor also covers the crater ramp's leg inside its footprint.

- [ ] Step 1 (tests first): the Quarry rim's existing cut and collision tests run per scan: `no_core_pillar_stands_on_the_ramp`, `no_kept_core_pillar_stands_invisible_in_the_cut`, `the_core_cut_lies_below_the_ground_that_is_drawn`, `the_core_scan_draws_through_the_ramp_cut` (rename `only_the_quarry_scan_is_cut_by_the_ramp` to say which scans are cut), and the Quarry rim's versions cover the connector's leg.
- [ ] Step 2: Implement; one code path for both scans, no copy of the quarry's code.
- [ ] Step 3: Report build time and size of each cut image.
- [ ] Step 4: Docs (*Cut*, *Collision* sub-bullets say both scans). `cargo test`, clippy, fmt; commit "golden-pit-ramp: cut the crater scan along its road".

### Task 4: Playtest

- [ ] `cargo run --profile playtest`, scratch `XDG_DATA_HOME`, a temporary debug system removed before committing. From the Quarry rim's junction to the bottom and back: the stalker running, the car at 7–10 m/s and full throttle, the carrier. Pass: no block, sinking, lift off, launch at a crest, or invisible wall; no stone over the road; shots from the crater's rim and floor and of the cut walls (scratchpad). Frame time near the crater against the Quarry rim's.
- [ ] Fix what fails in the owning file; record the result in `doc/world-reference.md` (*Driving and orders*). Commit "golden-pit-ramp: drive the crater road".
