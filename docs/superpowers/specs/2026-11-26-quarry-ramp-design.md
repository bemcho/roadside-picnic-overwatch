# Quarry ramp: a haul road down to the pit floor

## Intent

The Quarry rim diorama (`landmark-quarry-pit`, 110 m, `world/sites.rs`) is a
terraced pit about 48 m deep, from the rim at its drive down to the floor.
Today the drive ends at the rim. The stalker, the armored car and the
Institute carrier should be able to go down to the floor and back up by road,
along a ramp that winds down the bench walls.

## Goals

- A dirt road from the quarry drive's entrance (front edge, rim level) down
  the closed walls of the pit to its floor, continuous with the existing
  track network.
- Walkable, drivable by the car and the carrier, and usable by click-to-move
  orders (the A* router sees it as passable ground).
- It reads as a haul road: gravel and ruts on top, raised earth banks against
  the stone.

## Non-goals

- No rotation of the quarry (decided: it stays as placed).
- No change to the scan GLB, its stand-in, or its relief grid file.
- No change to the big terrain quarry around the Golden Sphere.

## Anti-goals

- A ramp that disappears under the scan's stone while the stalker walks on it
  (traversal height below the visible surface).
- A ramp too steep or too narrow for the carrier (3.6 m wide, 7 m long).
- Two turns of the ramp so close that one's banks sink into the other.
- Invisible walls: scan rubble collision left standing on the ramp.

## Approaches

- **A. Terrain fill ramp (picked).** A road whose levels are given per point
  rather than taken from the land, graded over the scan as fill. The terrain
  sits 0.12 m (`RELIEF_SINK`) under the scan, so wherever the ramp rises above
  the stone it shows as road; every mover already uses terrain height. Code
  only; the earth banks show terrain texture over the scan's stone.
- B. Carve a ledge into the scan in Blender: the most native look, but the
  scan is one fused mesh, its relief, collision and stand-in would all need
  rebuilding, Blender is not installed here, and it risks model fidelity.
- C. Imported ramp segments (timber/steel or earth, via Tripo): built look,
  but billable, fiddly joins, and it still needs A's height override.

## Design

1. **Ramp surface.** A new `Surface::Ramp` in `world/roads.rs`: dirt look
   (gravel + ruts in the mask), narrower than the 8.5 m roads, about 4.5 m
   half-width bed and a 6.5 m shoulder, so turns fit a ~100 m pit.
2. **Explicit levels.** A ramp route carries its own height per point instead
   of `sited_height` at its corners (`RoadNetwork::new` takes levels from the
   route when given). The levels are computed once when the road network is
   built (it already runs once at startup), not per frame.
3. **Path.** Starts at the quarry site's `entrance()` at rim level and runs
   down along the pit's closed walls (the scan is open on one side, down to
   the floor; the ramp does not cross that side), ending on the floor. The
   points live in world space derived from the site's pose, so they follow the
   site if it ever moves. Roughly 3/4 of a turn, with a hairpin if needed to
   keep the grade.
4. **Grade and fill.** At most ~20% grade on every leg (the drivability test
   allows 0.57), and every point of the bed at or above the scan's relief
   (`diorama_ground`) plus a small clearance, so the ramp is always fill and
   always visible. Adjacent runs keep their shoulders apart horizontally.
5. **Collision.** Scan pillars (rubble) whose footprint lies on the ramp's bed
   are dropped, decided once when the site's colliders are built.
6. **Docs.** Update the Terrain/roads and Sites sections of
   `doc/world-reference.md` and the relevant recipe in `arch-readme.md`.

## Verification

- Unit tests: the ramp is continuous with the drive, stays within the grade,
  stays above the relief, and its runs keep apart; `every_track_is_drivable`
  covers it.
- Playtest (`--profile playtest`, scratch `XDG_DATA_HOME`): walk, drive the
  car and drive the carrier from the rim to the floor and back; click-to-move
  to the floor from the rim; screenshot the ramp from above and from the floor.

## Left to the builder

- The exact path points, turning direction and whether a hairpin is needed.
- The exact widths and clearance, within the limits above.
- How the ramp's bed is tested against scan pillars.
