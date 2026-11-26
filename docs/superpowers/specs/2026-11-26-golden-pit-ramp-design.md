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
- Goal: the stalker, the car and the carrier go down and back up it when
  driven along it. Click orders and autopilot only have to reach the floor.
  The router (`path.rs`) does not see slopes, so orders may take the wall, as
  they do at the first quarry, a known limit already accepted there.
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
- **Path:** a baked centerline in world xz, from a rim point on the west or
  south-west rim (`GOLDEN_RIM`, reached by the Chapter 4 road from `map(1100, 368)`)
  along the wall to `GOLDEN_SPHERE`. See "Calls made" below.
  - It curves with the oval: a spiral in the oval's normalised radius, going
    inward as it descends.
  - Corners are no tighter than the first ramp's (arcs of at least 9 m radius).
  - The grade is gentler than the first ramp's 0.19, since there is room; aim for
    0.10-0.15 (see "Calls made").
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
  - A click order from the rim reaches the Sphere's junction, no longer than
    1.6 times the ramp.
- **Cut and fill:** report the largest of each against the land as it was.
- **Playtest** (`--profile playtest`):
  - The stalker walks and the car and carrier drive down and back up, with no
    launch at the crest.
  - Look at the cut banks.
  - Close shots from the rim and from the floor.

## Calls made after the builder check (mine, from the user's "GO")
- **Shape over a fixed start.** From `map(1100, 368)` to the Sphere is only
  210 m, and the straight chute's mean grade (0.143) already lies in the
  target band, so a road between those two points cannot visibly curve.
  - The ramp's start may therefore move south along the west and south-west rim.
    The Chapter 4 dirt road runs on along the rim to it (`GOLDEN_RIM`).
  - The grade band widens to 0.10–0.15, so a longer road of about 300 m shows
    the curve.
  - On the bench shelves the road crosses inward, and on the faces it runs along
    them, so cut and fill stay small.
- **Round the south wall.** The Quarry rim scan sits on the north-west wall
  (box x −1094.7..−1002.9, z 97.2..205.2). Its ground blends into the land
  57.5 m beyond that box (`relief_margin`), and the road keeps out of the box
  and its blend margin.
- **Limits:** cut at most 4 m and fill at most 5 m against the land as it was.
  The bank is a 4.5 m blend, so deeper cuts read as walls. The ramp's grade
  is at most 0.15. Its sampled surface passes the same drivability test as the
  first ramp (`the_ramp_grade_stays_drivable`, centerline 0.25, lanes 0.35).
- **The end is exactly the `GOLDEN_SPHERE` junction** (normalised radius 0.395,
  on the last face, about 3 m above the floor). The ramp grades to its level.
  The Sphere's own drive starts there unchanged. `roads.rs`'s test that every
  site junction lies on a route learns that a ramp's end counts.
- **Produced by the first ramp's planner,** generalised to any relief, entrance,
  end point and no-go boxes. It is fed the pit's land (`sited_height`) exported
  once as a relief file in metres. The path is a generated file.
