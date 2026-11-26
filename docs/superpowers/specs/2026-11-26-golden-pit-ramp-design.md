# Golden pit haul road — design

## Intent
The terraced pit at the map's east edge (`terrain.rs` `QUARRY_CENTER`/`QUARRY_RADII`:
a 640 x 1060 m oval, 32 m deep in three ~11 m benches, the Golden Sphere on its
floor) is reached today by the Chapter 4 dirt road's last legs, `QUARRY ->
map(1100, 368) -> map(1146, 368) -> GOLDEN_SPHERE` (`roads.rs`). Dirt tracks
follow the land only at their corners and run straight between them, so the road
is a chute through the bench faces at roughly 1 in 5 or worse. The user wants it
reworked like the scanned quarry's haul road next door: a road that curves down
along the pit's wall to the floor, walkable and drivable by the stalker, the car
and the carrier.

## Goals / non-goals / anti-goals
- Goal: a ledge road, chosen by the user (option 1), that leaves the rim on the
  west side where the Chapter 4 road arrives, curves with the oval down the wall
  at one steady grade to the floor, then reaches the Golden Sphere's junction.
- Goal: the stalker, the car and the carrier (driven and on autopilot) go down
  and back up it; click orders to the floor route along it.
- Non-goal: a full lap round the pit. At a drivable grade 32 m of drop needs
  about 200-270 m of road, and a lap is about 2.7 km. "Spiralling" here means
  curving with the wall for part of a lap.
- Non-goal: bench switchbacks, guard rails, changing the pit's shape or the
  Golden Sphere's placement.
- Anti-goals:
  - A step, kink or launch where the road meets the rim. Reuse the first ramp's
    rounded crest.
  - The road crossing the Quarry rim scan's footprint. A ramp there would need
    that scan cut (`ramp_cut.rs`), which only the first quarry's corridor has.
  - Banks or fills that read as walls or pits beside the road.
  - The old chute still visible.
  - Any per-load or per-frame derivation of the path.

## Constraints
- Precalculate once. The centerline is a baked constant (a generated file or a
  short literal in `roads.rs`), never searched at load.
- Reuse `Surface::Ramp` and `graded_track`:
  - The first leg is the drive: it follows the land and must be at least half a
    crest (12 m) long. From the entrance the road falls at one grade to its last
    point's level, rounded by the 24 m crest.
  - This is the only grading that gives a steady descent.
- The ramp's mask already flattens the terrain to it (`land_height`). This pit is
  plain terrain, not a scan, so the road carves its own cut and fill. No cut
  shader and no collision filtering.
- Keep the road out of every diorama footprint. Check the Quarry rim scan
  (`sites::quarry_footprint`) above all.
- Update `doc/world-reference.md` (Terrain/roads, Quarry ramp) and the CLAUDE.md
  table row in the same change.

## Approaches (chosen: 1)
1. **Ledge road along the wall** (chosen by the user). The road curves with the
   oval from the west rim down to the floor, then crosses the floor to the
   sphere. It is closest to "spiralling down" and to the first quarry's ramp.
2. Bench switchbacks. The road runs along each bench shelf and drops through each
   face by a short ramp. It looks more like a mine and uses the terraces, but has
   three tight turns for the carrier and three crests to round.
3. Re-grade the existing straight legs at one grade. Cheapest, but it stays a
   chute through the benches, which is not what was asked.

## Design
- **Path:** a baked centerline in world xz, from a rim point near `map(1100, 368)`
  (just outside the oval's edge, on the Chapter 4 road's line) along the wall to
  the floor near `GOLDEN_SPHERE`. The Chapter 4 route ends at that rim point.
  - It curves with the oval: a spiral in the oval's normalised radius, going
    inward as it descends.
  - Corners are no tighter than the first ramp's (arcs of at least 9 m radius).
  - The grade is gentler than the first ramp's 0.19, since there is room; aim for
    about 0.12-0.15.
  - It hugs the wall so cut and fill stay within a few metres (the first ramp has
    at most 6.8 m of fill and 2.2 m of cut).
- **Tracks:**
  - `tracks()` gains a second `(Surface::Ramp, path)`. Its first leg, rim point to
    entrance, is the drive.
  - Code and tests that assume one ramp need updating: `roads.rs` tests that take
    `ramps.next()`, the `Surface::Ramp` docs, and `sites.rs`'s footprint test that
    skips ramps.
  - The old `map(1146, 368)` leg is removed.
- **Ground:** the terrain takes the ramp's height inside its mask, as for any road.
  Vegetation, scatter and the road paint follow the road network automatically.
  Check that the pit's floor noise and bench shelves do not show steps at the
  shoulder's edge.

## Verification
- **Unit tests:**
  - The road falls steadily from the entrance to the floor at the chosen grade
    and no steeper anywhere.
  - Its corners are no tighter than 9 m.
  - It stays out of every diorama footprint.
  - Its end lies on the pit floor within reach of `GOLDEN_SPHERE`.
  - A path order from the rim to the sphere follows it.
- **Cut and fill:** report the largest of each against the land as it was.
- **Playtest** (`--profile playtest`):
  - The stalker walks and the car and carrier drive down and back up, with no
    launch at the crest.
  - Look at the cut banks.
  - Close shots from the rim and from the floor.

## Left to the builder
- How the centerline is produced: an analytic spiral on the oval, or the first
  ramp's planner (`tools/plan_quarry_ramp.py`) fed a terrain height grid.
- Its exact start, turn direction (round the north or south wall), grade and
  point spacing.
- Whether it lives in a generated file or a literal.
