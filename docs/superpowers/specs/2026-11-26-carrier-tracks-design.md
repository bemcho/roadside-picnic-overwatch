# Carrier tracks that run — design

## Intent
The Institute carrier's road wheels, sprocket and idler already roll with the
distance each track has run (`Carrier::tracks`, counter-rolling on a pivot), but
its two track belts are fused into the hull and stand still. The belts should
run with the wheels: forward and back with speed, at different rates on a
turn, against each other on a pivot.

## Goals / non-goals / anti-goals
- Goal: the scan's own links visibly travel round each track's loop, locked to
  `tracks[side]` (the same value the wheels use), for expeditions, driving and
  autopilot alike.
- Non-goal: suspension, track sag that changes with load, links reacting to
  rocks; the loop is fixed in the hull's frame, as the wheels are.
- Anti-goals: links sliding against the wheels (a speed mismatch you can see),
  a gap or double link where the loop closes, a visible seam where the belt
  was cut from the hull, degrading the scan (no flattened belts, no lower-res
  link), or a frame-time cost noticeable while the carrier is in view.

## Approaches (chosen: 1)
1. **Moving links** — cut the belts out of the hull, cut one link from the scan,
   run copies of it round each loop, shifted by how far that track has run.
   Real 3D links, correct on turns and pivots for free; ~70 shared-mesh
   entities per side. *Chosen by the user.*
2. Scrolling texture — re-bake the belt to a strip and slide its UVs. Cheap, but
   the links' relief stays still under moving paint; flattening it breaks
   "keep model fidelity".
3. Sliding mesh — one belt mesh whose vertices slide round the loop in a vertex
   shader. Same look as 1, but needs per-vertex baked data and its own shader for
   main, prepass and shadow passes: more machinery than 140 cheap entities.

## Design
- **Pipeline (offline, precalculated once).** `tools/split_moving_parts.py`'s
  carrier model gains: a belt region (faces within the belt's band around the
  loop, |x| over the track width) that is removed from `Hull`; one clean link
  from the lower run exported as a `TrackLink` node, pivot at its centre on the
  belt's mid-line, identity rotation. The same run measures each side's loop
  (the belt mid-line in the side plane: lower run, arcs round sprocket and
  idler, upper run over the return rollers) and the scan's link pitch, and
  writes them to a generated `src/player/vehicle/carrier_track.rs` (loop
  polyline per side, link count; regenerate, don't hand-edit). The link count
  per side is the integer nearest loop length / scanned pitch, so the loop
  closes without a gap. Then `slim_dense_model.py` and
  `compress_glb_textures.py`, as today.
- **Runtime.** When the carrier's scene is ready, spawn `N` copies of
  `TrackLink`'s mesh and material per side as children of the carrier (model
  units) and hide the template. A system places each link `i` at arc length
  `i * pitch + tracks[side] / SCALE` (mod loop length) on its side's loop,
  turned so it lies along the loop's tangent with its tread facing out. Moving
  forward, the lower run travels rearward; links only move when `tracks`
  changes.
- **Docs.** The Institute carrier bullet in `doc/world-reference.md`, the tool's
  docstring (commands, measurements), arch-readme if the recipe changes.

## Verification
- Unit tests on the loop math: closure (positions at offset 0 and at one pitch
  are the same set), lower-run links move to −Z as `tracks` grows, the two
  sides independent, link spacing equal round the arcs.
- Pipeline: `--pose-test`-style render of the split model (belt gone from the
  hull, no holes in the side skirts) and of the link; glTF validation.
- Playtest (`--profile playtest`): close shots driving, turning and pivoting;
  links stay on the wheels; frame time with the carrier in view no worse than
  before.

## Left to the builder
The exact belt region and which link to cut (pick a clean one between road
wheels), how the loop is sampled (points every half pitch is plenty), and
whether link placement lives in `drive_carrier` or its own system.
