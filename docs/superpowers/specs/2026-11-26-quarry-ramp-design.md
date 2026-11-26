# Quarry ramp: a haul road down to the pit floor

## Intent

The Quarry rim diorama (`landmark-quarry-pit`, 110 m, `world/sites.rs`) is a
terraced pit about 48 m deep, from the rim at its drive down to the floor.
Today the drive ends at the rim. The stalker, the armored car and the
Institute carrier should be able to go down to the floor and back up by road,
along a ramp that winds down the pit.

## Goals

- A dirt haul road from the quarry drive's entrance (front edge, rim level)
  that spirals down the pit to its floor, continuous with the track network.
- Walkable, drivable by the car and the carrier, and usable by click-to-move
  orders (the A* router sees it as passable ground).
- It reads as a haul road: gravel and ruts on a ledge cut into, or built up
  on, the quarry's stone; the rest of the quarry looks as it does now.

## Non-goals

- No rotation of the quarry (decided: it stays as placed).
- No change to the scan GLB, its stand-in GLB, or its relief grid file.
- No change to the big terrain quarry around the Golden Sphere.

## Anti-goals

- The ramp hidden under the scan's stone while the stalker walks on it.
- A ramp too steep or too narrow for the carrier (3.6 m wide, 7 m long).
- Tall earth embankments filling the pit (the fill-only design reached 26 m).
- Two runs of the ramp so close that one's banks sink into the other.
- Invisible walls: scan rubble collision left standing on the ramp.

## Approaches

- **Cut ramp with a scan cutaway (picked).** The terrain carries the ramp at
  a steady grade, cut into or built up on the scan's ground. The quarry's
  scan (full and stand-in) draws through a material extension that discards
  its fragments above the terrain inside the ramp's corridor, as the hangar's
  roof cutaway (`world/base/cutaway.rs`) discards above a height. The scan
  file is untouched.
- Fill-only terrain ramp (the first approved design): prototyping on the
  relief showed the walls are too steep; at 20% the ramp stood up to 26 m
  above the stone and still ended 9 m short of the floor.
- Steeper ramp without a cutaway (~35%): the ramp would run inside the
  scan's stone. Rejected.
- Carving the scan in Blender, or Tripo ramp segments: rejected earlier
  (fidelity, billable, still needs a height override).

## Design

1. **Path, baked once.** A pipeline tool, `tools/plan_quarry_ramp.py`, reads
   the relief grid (`src/world/pillars/uhd_quarry_pit_ground.rs`) and writes
   the ramp's centerline in the quarry's model units to a generated file,
   `src/world/pillars/quarry_ramp.rs`. It searches a 32×32 grid over the
   footprint, with 8 headings, turns of at most 45° per step and at least
   11 m between turns, for the path from the front edge's middle down to the
   floor that keeps closest to the relief (fill and, at twice the cost, cut
   both penalized, turns penalized) at a constant 20% grade. The cell path's
   corners are then rounded into circular arcs of 9.5 m (a 9 m wide bed must
   not pivot) and the path is resampled every 3 m. As built: 244 m, grade
   0.198, at most 6.8 m of fill and 2.2 m of cut against the smoothed relief
   the search follows (7.9 and 3.2 m against the raw relief), runs at least
   22.7 m apart, tightest turn 9.5 m. It runs west along the front, down
   the west wall, east along the back, north through the middle terrace, east
   again and down the east wall to the floor.
2. **Ramp surface.** A new `Surface::Ramp` in `world/roads.rs`: dirt look
   (gravel and ruts in the mask), a 4.5 m half-width bed and a 9 m shoulder
   (the cut banks), narrower than the 8.5 m roads.
3. **Grade.** A ramp is graded at one constant grade along its whole length,
   from the drive's level at the entrance to the land's level at its floor
   end, instead of leg by leg: 0.191 over 251 m, 48 m down. Its points are
   the quarry's junction and entrance followed by the baked path transformed
   by the site's pose, so the 60 m drive is the ramp track's head (it follows
   the land to the entrance) and no second road meets the ramp end to end.
4. **Terrain.** Inside the diorama the scan's ground overrides roads today;
   ramp legs apply after that override, so the terrain follows the ramp in
   its corridor, cut or fill.
5. **Cutaway.** At entering the Zone, a height image of the corridor (where
   the ramp's shoulder reaches, the lowest drawn ground over the texel less
   0.1 m, "uncut" elsewhere, 0.5 m texels, world-aligned over the quarry's
   footprint) is built once from `Terrain`. The drawn ground is the render
   mesh (`mesh_height_at`), not `height_at`: stone left above it, even by
   centimeters, shows through as dark slivers. The quarry's stand-in and its
   streamed full scan draw with `ExtendedMaterial<StandardMaterial, RampCut>`:
   main, prepass and shadow passes discard fragments above that height, so the
   ramp, its banks and its light are not covered.
6. **Collision.** Scan pillars whose footprint reaches the ramp's bed are
   dropped from the quarry's collider, once (cached).
7. **Docs.** Terrain/roads and Sites sections of `doc/world-reference.md`,
   `arch-readme.md` where it lists generated data and material extensions,
   and the CLAUDE.md world-reference table.

## Verification

- Unit tests: the ramp track starts at the quarry junction (its 60 m drive included, graded like any drive up to the entrance) and ends on the floor,
  grade at most 0.2 everywhere on the rendered terrain, fill at most 8 m and
  cut at most 3 m against the scan's ground, runs at least 2 shoulders apart,
  no quarry pillar on the bed, and the cut image is uncut outside the
  corridor; `every_track_is_drivable` covers it too.
- Playtest (`--profile playtest`, scratch `XDG_DATA_HOME`): walk, drive the
  car and drive the carrier from the rim to the floor and back; click-to-move
  from the rim to the floor; screenshots from above and from the floor.

## Left to the builder

- The search's exact costs and smoothing, within the limits above.
- How the corridor test and the cut image are structured in code.
