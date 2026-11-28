# World reference

Per-system reference for Roadside-Picnic: what each system does, its measured
numbers, and the asset pipeline that rebuilds its models. `CLAUDE.md` indexes
this file and keeps the rules; read the section for a system before changing
it, and update it in the same change.

## Tactical view

The game plays from a strategy-scale camera (`src/player/camera.rs`): zoom runs
6 m to 4.6 km, the far plane, sun shadow cascades and distance fog follow the
zoom, and the low cotton fog thins out above ~140 m so the map overview stays
readable. The camera's `ViewFocus` (the audio listener `Ears`) also becomes a
`StreamAnchor` while zoomed under 250 m, so roaming streams collision, plants,
ground LODs and walls around the view as well as around the stalker.
`detail_range` adds `STRATEGY_REACH` (75 m) to every detail range for the raised
camera. The hangar's roof and upper walls are cut away (`base/cutaway.rs`,
`shaders/cutaway.wesl`) while the camera or its focus is over the hall and the
camera is within 120 m of its focus (`CUT_ZOOM`); zoomed out further, the
hall shows whole.
The hangar's pendant lamps and cords (`RoofFixture`) hide with the roof.
Hover descriptions (`ui/tooltip.rs`) pick, in order, a `Describe`d entity
(creatures, the car, exhibits), a site/building outline, then the named ground
(`world/places.rs`: water from the hydrology profiles, quarry, bald hill, yard).
The stalker surveys an `Explored` grid within 150 m as they travel.
Unsurveyed ground lies under a shroud (fog of war, `world/shroud.rs`): the
ground fog pass, drawn after the sky, covers it and the fog over it with
opaque, slowly drifting black smoke from a mask texture of that grid (one
texel per 32 m cell, its edges softened and wobbled), so nothing of it shows
from any height (the clouds still float over it), and `cull_shrouded`
drops every other entity standing wholly on it from the cameras' and the sun's
visible lists after Bevy's own culling, so it costs no draws or shadow passes.
V lifts the shroud for testing. M opens the Harmont map (`ui/map.rs`):
`doc/Roadside_map.jpg` (as `textures/ui/roadside_map.png`) with the stalker's
marker, opaque near-black smoke over unsurveyed cells and a few birds wheeling over it.
Point-and-click orders (`player/orders.rs`) use grid A* over collider pillars,
deep water and ground too steep (`player/path.rs`); the vehicle follows them with an autopilot
in `drive_vehicle`. Each mover is routed by its own `Mobility`: the stalker (water
to 1.05 m, ground to 45°, a 1.0 grade: `move_player` lets them walk up anything,
so this is the routing's own limit), the car (1.7 m clear of solids, water to
0.6 m, ground to its tire grip, `MAX_GRIP_GRADE` 0.66) and the wider Institute
carrier (`CARRIER_ROUTING`: 2.3 m clear, water to 1.5 m, ground to the 1.0 grade
its tracks climb, `MAX_CLIMB`, over what it knocks down), so the carrier is never
sent through a gap only the car fits. The grade holds both ways (past grip the car
slides down, and a route down a wall is no route back), and for the whole body:
a cell is blocked where the rendered triangle under its centre is steeper than
the mover goes, or where the body's radius reaches such a triangle (measured
exactly from the rims of steep ground, not grown by the half-cell slack pillars
get, which would close the crater's deepest laps to the carrier). So into a pit
orders take the road graded down its walls. Where no such route joins a foot
click's ends (6 of 200 random 20-250 m trips in the Zone), the stalker alone
searches once more over any slope (`route`): `move_player` has no slope limit,
so a click they could always walk must not fail; the car's and the carrier's are
refused. That costs a trip that needs it 0.18 s against 0.13 s, and an
unreachable click twice the searches, 0.36 s against 0.18 s (test build). The
slope comes from the 2 m detail mesh's vertices, sampled once a search (`Terrain::mesh_patch`, about one land
sample per 16 cells): the movers stand on that mesh, and `height_at` between its
vertices misses where a triangle spans a bank's foot (4 m higher than `height_at`
4.3 m off the crater road's centreline). The search keeps to
the trip's box and 28 m around it (0.5 m cells, coarser past 260k cells); when
no way lies in there it searches once more 84 m around it, for a road that winds
out of the box (the crater's spiral). A pillar blocks if `push_out` would stop
the mover on any ground it touches the pillar from (the pillar's foot and eight
points a reach around it), so on a slope a stone just over the ground at its
foot still blocks a mover coming up from below. A mover standing in a solid's
reach (pressed into it, or up on stone it climbed when it stalled) sets off from
the nearest free cell, as a click on a solid ends at one. Shift-clicks append legs pathfound from the last stop
(`MoveOrder::append`); a route started with Shift-clicks while the mover stands
still is held until Shift is released (appending to a moving order never stops it).
Scenery colliders stream in only near the stalker, so a far leg is planned
without them: `replan_order` pathfinds the leg to the next stop again from where
the mover stands each time a stop is reached, and when the mover stalls (no
progress for 0.8 s). After a stall the mover backs away 6 m first (stopping
sooner if it backs into something, and after 4 s at most): the car reverses at
full throttle, swinging its nose toward the new leg, since it cannot turn in
place against a rock. A mover reversing away passes no waypoint
(`MoveOrder::steer`), so a new leg that turns off right beside the obstacle
keeps its turn rather than heading straight back into it; the stalker, who
walks on toward the new leg during their back-off, takes a waypoint beside
them as any other. The attempts share 18 m of back-off travel: three full
6 m ones, or, as a car's heading turns with the distance it travels (about
0.19 rad a meter at full lock), proportionally more shorter ones where less room
is left behind it: 18 with 1 m, at most 60 (each charged at least 0.3 m). The
stalker turns on the spot, so each of their attempts costs the full 6 m. A
stall with the travel spent drops the order. The autopilot also brakes, at the
8 m/s² it plans its stop at the target with, to a bend speed before each bend in
the route (turns adding up to 20° within 8 m of path; `bend_limit`): 8 m/s for
the car, which turns an 8.7 m circle there and no tighter than 24 m at its top
speed, and 5 m/s for the carrier (8.3 m at its 0.6 rad/s). The stop
rings pulse (1.4 s, swelling 25%). A thin line (`player/route.rs`: a gizmo group 2 px wide with a
small depth bias, in the rings' colour) traces the exact route from the stalker, or
the car they drive, through every pathfound waypoint still ahead to the last ring:
points every 2 m (at most 64 steps a leg) laid on the ground, or on the water where
the stalker wades. It shrinks as waypoints are reached and goes with the order (WASD,
arrival, a stall, boarding or leaving the car). The draped line through the waypoints
is rebuilt only when they change; only the leg to the next one is laid each frame.
Routes pass through the yard gate's boom, and a car
with an `InstitutePass` through the Institute gate's; each lifts as it arrives. A yellow ring (`ui/locator.rs`) lies on the ground around
the stalker, or the car they drive, at every zoom: its vertices follow the
rendered terrain (or the water when wading), it never shrinks under 34 px wide,
a relative depth bias keeps it over the road surface and stones, and it pulses while
the stalker stands still.

## Procedural content

- **Terrain** (`src/world/terrain.rs`) is a deterministic 3.44 km square built in
  128 m chunks: rolling hills, the Zone's ridge mountains, the quarry basin,
  marsh hollow and bald hill (`landform_height`). The graded road network
  (`src/world/roads.rs`) follows the Harmont map: Harmont's streets, the highway
  from the Perimeter gate past the factories and the Garage, the railway tracks,
  the Chapter 4 track through the breach, the marsh road and the quarry, where it
  ends: the way on to the Golden Sphere is the quarry's haul ramp and then the
  Sphere's own. Roads
  are graded over the sited land (pads and dioramas), then paved. Each route
  is dirt, old asphalt (`Surface::Asphalt`: the highway, Harmont's main road,
  the Institute and Plague roads), New Harmont's smooth streets
  (`Surface::Street`) or, for the two pits' haul roads alone (the quarry's and
  the Golden Sphere's), a ramp
  (`Surface::Ramp`: dirt with a 4.5 m half-width bed and a 9 m shoulder,
  `RAMP_HALF_WIDTH`, `RAMP_SHOULDER`, narrower than the 8.5 m roads). A ramp
  is graded at one constant grade from the entrance of its drive to
  its last point's level (the drive before the entrance follows the land, and
  a short crest rounds the break there), instead of following the land at
  every corner, and it
  overrides a diorama's own ground, which hides every other road, so the quarry's pit
  and the Golden Sphere's crater follow it inside its corridor (see **Quarry ramp**);
  the road mask texture holds gravel (R), tire ruts (G),
  asphalt (B) and the old, broken share of that asphalt (A, `Surface::Asphalt`
  only: New Harmont's streets and plaza keep A = 0), and `terrain.wesl` paints
  smooth weathered tar from B and the old asphalt (see Old roads) from A;
  inside the Zone that asphalt gives way to the retired road kit.
  The same network keeps vegetation off the lanes and paints a world-space
  gravel and tire-rut mask independent of terrain LOD. Heights are sampled
  through the `Terrain` resource (`Terrain::height_at`) — never hardcode ground heights
  in other plugins. `Terrain::mesh_height_at` tracks the rendered surface near
  the player. Ground tiles swap from 2 m to 8 m sampling at distance, with
  stitched edges across both resolutions; within 2 tiles of an anchor they
  switch to the 2 m mesh one per frame, nearest first, and back at once. Larger 16 m horizon tiles keep the
  coastal water, lake basin and rolling country visible beyond the traversal
  limit without nearby collision or plant detail.
  `tools/prepare_terrain_textures.py` extracts tiling albedo,
  normal, and packed AO/roughness/height maps from `terrain_texture_image_2.webp`
  (moss) and `terrain_texture_1.webp` (bare ground); those source sheets are not
  checked in, only the extracted maps in `assets/textures/terrain`. The shader blends irregular
  patches over the terrain masks, uses the heights for shallow parallax, and adds
  snow on high, gentler slopes. Steep ground (the cut banks and fill faces of
  the haul ramps, pit walls, the Zone's western scarps) is dressed in rock:
  `tools/prepare_rock_textures.py` takes a 320 px fractured patch of the
  `rock-17m` scan's 4K base colour (assets workspace,
  `models/rocks/uhd/rock-17m.glb`), flattens its lighting, makes it tile,
  grades its mean to the cut scans' grey-brown stone (linear 0.057, 0.048, 0.036)
  and writes `textures/terrain/rock_{albedo,normal,surface}.ktx2` (BC7; source
  PNGs in the workspace's `textures/rock/`). The shader blends it in by the
  smooth vertex normal's slope, from 46 to 55 degrees (the banks lean past 55
  degrees to their rims; a facet's own slope flips triangle by triangle on rough
  hillsides, a checkerboard of rock), sampled triplanar and stochastic at 3.5 m
  a repeat. On steep ground the projection follows the triangle's own tilt
  (from screen derivatives) rather than the vertex normal, averaged over 4 m,
  which leans far less than a bank's 2 m faces, so the ground's maps no longer
  streak down the banks; lighting keeps the smooth vertex normal. Below 46
  degrees the shader is as before. A survey of the 2 m mesh outside the cut
  scans finds 0.29% of its facets above 45 degrees, 0.06% above 60.
- **Water** (`src/world/{hydrology,water}.rs`): the Harmont map has no river.
  The Zone's water is the marsh (`hydrology::SWAMP`): shallow pools between
  drier hummocks; the lake and the sea lie beyond the map's edges. Use
  `Terrain::water_depth_at` for traversal. Walk and drive through the
  shallows; deep water stops you.
- **Day/night and weather** (`src/world/weather.rs`): a 24-minute day (hold T to
  time-lapse) moves the sun and a full moon (a second, dim `DirectionalLight` with
  its own disk) by solar geometry; whichever is up casts shadows, and exposure
  eases from EV 13 by day to EV 9 at night. Every week (days 7w to 7w+6 of the
  clock) brings one spell of bad weather (`bad_weather`), overcast or rain
  (45%) each day: two or three days most weeks, four or five now and then, and
  about one week in twenty the whole week. Shorter spells keep off a week's
  first and last days and a whole bad week never follows another, so at least
  two bad days come every week and no spell runs past seven. The other days
  are clear (60%) or humid; the first day is always humid. Temperature follows the sun (coldest at 05:00); relative humidity
  comes from the day's dew point (Magnus), so it peaks late at night and at dawn,
  or stays high on humid and rainy days. The HUD shows time, temperature,
  humidity and sky.
- **Ground fog and clouds** (`src/world/ground_fog.rs`, `ground_fog/{field,render}.rs`,
  `assets/shaders/ground_fog.wesl`): a custom render pass (after the main pass)
  raymarches a thin layer that follows the terrain or open water. Its top sits
  between half the armored car's height and its roof (1.25–2.6 m), and visibility
  inside ranges 1–5 two-metre ground tiles, so a crawling stalker disappears while
  a standing one shows chest-up. The fog field texture, built from the distant
  terrain tiles while the landscape loads, stores the surface height, how
  readily fog gathers and which side of the Perimeter each spot lies on. In the
  Zone it gathers on all low ground and thickest over water and its banks (the
  marsh, the coast and the lake), in the quarry basin and in the three dense
  woods; the hills (over 22-45 m) stay clear, and small clear patches, 10-30 m
  across, drift through it. Saturated air (`Weather::fog`) brings it, and every
  night does whatever the weather (`night_fog`: it gathers from 19:00 to 21:00
  and lifts from 07:30 to 09:30, so dawn is foggy). Harmont, the hangar yard
  and the Institute get a thinner fog of their own: 90-150 m visibility to 2%
  contrast (a house 50 m away still shows at about a quarter of its contrast),
  densest on the ground and thinning upward, its top rising and falling
  between 1.5 and 8 m; the two blend over 60 m across the Perimeter. Drifting
  banks fray its edges, and billows and wind break it up. Under an upscaler
  the pass draws into the main pass's smaller render area
  (`MainPassResolutionOverride`) and maps pixels by that area, so the fog stays
  on the ground it covers (dormant: no upscaler runs today; DLSS was tried and
  dropped).
  The sun and moon light it through the atmosphere with shadows and forward
  scattering. The hangar hall stays clear, and the fog thins out above ~140 m of
  camera height. The same pass marches three layers of cumulus whose bases lie
  near 170 m, 450 m and 1,050 m: noise cells that drift with the wind, lit by
  the sun through the atmosphere and darkened by rain. Their cover follows the
  day's `Weather::cloud`, from a few puffs on clear days to the most on rainy
  ones, and they blend with the ground fog in the order the ray meets them.
  Bevy's distance fog adds the regional 10–16-tile cloud shelf
  over far low ground (shorter in humid air); the terrain shader masks it to low
  ground, and birds disable classic fog.
- **Scenery** (`src/world/scenery.rs`): textured rock outcrops and reeds are drawn
  in chunk batches. Every tree is an imported model: saplings, birches and dead
  trees (`assets/models/trees/medium/`) grow around the hangar, in the named
  landmark groves and in three dense woods
  (`canopy::WOODS`); open country has no trees. Their GLBs use one
  ~500-triangle primitive each: simplified textured wood and, for living
  trees, cutout crown faces baked from the supplied models. Within 8 m of the
  camera a tree swaps to its full scan (`trees/uhd/`, ~1.9M triangles), a
  child entity that crossfades by `VisibilityRange` and sways with it.
  `stream_close_trees` wants a tree's scan from `GpuScans` as the camera nears
  (16 m from the tree's middle) and shows it once it is on the GPU (the three
  are pinned there behind the entry curtain). It drops the instance 5 m
  further out, or if the scan leaves the GPU.
  `src/world/canopy.rs` fits the roots and trunks, limits
  shadows to nearby trees, and sways some living trees without moving their
  colliders. Individual trees can randomly snap and fall while in view
  (`src/world/tree_fall.rs`, `TreeBreak` message); their
  collision goes away immediately and they don't respawn during the run.
  Colliders and undergrowth stream in a 3×3 chunk neighborhood: one new
  tile spawns per frame, nearest an anchor first, and a tile that leaves
  is marked `Unloading` and despawned 300 children per frame
  (`scenery::unload_stale_tiles`), so crossings don't stall a frame.
  Camera frusta and distance ranges cull scenery detail independently of
  collision and leave the terrain skyline visible.
- **Wind** (`src/world/weather.rs`, `Wind`): the day's breeze with gusts of
  4–10 s that come and go. It leans and stirs the trees, bends the imported weeds and
  bushes in `shaders/plant_wind.wesl` (each vertex by its height above the
  instance's root), sways the nearest plant's full scan from the root, drifts and churns the ground fog,
  and swings the coat of a rig built with coat bones (the soldier has none).
- **Plants** (`src/world/plants.rs`): dry weeds (dry grass, dandelion,
  ragweed) favor dry ground, shrubs (shrub, bramble, dog rose, elder, juniper,
  wormwood) favor woodland, thornbushes the uplands, and damp weeds (hogweed,
  burdock, henbane, thistle) and emissive glowweed favor damp ground. There is
  no procedural plant geometry: every plant is an imported model with three
  tiers in one frame (`assets::PlantModel`): a light, texture-free far mesh
  (`plants/low/`, 400–600 triangles, built by `tools/build_far_lod.py`) drawn
  with the near model's material out to 66 m (plus the camera's reach), the
  textured `medium/` model within 30 m of the camera, and the full `uhd/` scan
  for the plant nearest the stalker within 8 m (henbane and ragweed have no
  textured scan). `stream_full_plants` asks `GpuScans` for the scans of plants
  within 40 m (`UPLOAD_LEAD`) and shows one only once it is on the GPU; the
  plant's own model stays until then, and shows again if the scan leaves the
  GPU. The multipart UHD scans differ in shape from their medium
  models, so their swap shows. Plants sharing a model draw as one GPU
  instance batch. Each dry-weed seed grows a patch of up to 5 plants, other
  weeds and bushes up to 15 (a third of that in the belts, full patches in
  the crawl hole's thicket); sites
  are computed on a worker thread as chunks stream in, and at most
  `PLANTS_PER_FRAME` (150) plants spawn per frame, nearest tile first, each
  given its model's precomputed bounds (`Aabb` + `NoAutoAabb`). Plants have
  no collision or shadows. They draw in a camera's depth prepass where one
  runs, bent by this frame's wind and, for their motion vectors, by last
  frame's, so an upscaler does not smear them; the wind keeps updating while
  paused, so their motion settles. Dormant: no camera runs a prepass today
  (DLSS was tried and dropped).
  The rear and sides of the hangar form a dense
  shelterbelt; the central yard and roads stay clear.
- **Vegetation** (`src/world/trees.rs`): four extracted, textured vegetation clusters
  (`assets/models/vegetation/medium/*.glb`) are scattered only around landmarks, on a jittered grid,
  weighted by `Terrain::woodland_at` (off-road, outside the quarry, sparse above
  the treeline and on the bald hill). Rebuild them from the packed source with
  `tools/prepare_vegetation_asset.py <source.glb> assets/models/vegetation/medium assets/textures/fauna`
  (run in headless Blender after `--`). It reuses the saved albedo, normal and
  roughness PNGs (`textures/fauna` is in the asset archive, see Asset tiers); `--boards <image_3> <image_2> <image_1> <image_0>` rebuilds
  512 px maps by combining swatches and close-ups from the reference sheets.
  The forest uses
  separate birch/petrified bark, and plants share the mud material at their bases.
  Texture cards, material balls and miniature LOD previews are excluded, and
  each extracted cluster is centered with its lowest point at ground level.
  Regenerate each collision pillar file with `tools/trim_tree_base.py` whenever
  the source geometry changes.
- **Cordons** (`src/world/cordon.rs`): the enclosed hangar yard has a level
  staging area. Its north gap holds the checkpoint, turned so its own barrier
  lies on the wall line, and a 12 m lane beside it that a striped yard boom
  (`Gate::Yard`) closes; it lifts for the stalker and their car.
  The Perimeter wall (`PERIMETER_X`) runs the map's length between Harmont and
  the Zone at three times the panel's height (`PERIMETER_RISE`); `in_zone`
  tells the sides apart. Its one gate is the Institute's on the highway,
  narrowed to the road and closed by a red-and-white boom (`Boom`) that lifts
  for a body carrying an `InstitutePass` (the stalkers' car, on Institute
  papers, and the Institute's carrier) and lowers once it has passed
  (`Gate::Institute`). It also lets the stalker out of the Zone on foot,
  however they got in (`Boom::letting_out`): it lifts once they come near from
  the Zone's side and stays up until they are 40 m away; coming in on foot it
  stays down. Click orders route a stalker walking out through it. A second
  checkpoint guards it. The
  old breach on the Chapter 4 track is walled up. The only other way through
  is the crawl hole (`CRAWL_HOLE`): the ground is dug a metre under the wall
  (`hole_depth`, applied in `land_height`), and over it the wall's pillars
  start just below ground, so only a crawling stalker fits under. A dense
  bush thicket (`thicket`) lines both sides of the wall 45 m either way of
  it, and its tooltip shows only within 8 m. A belt of weeds and bushes
  (`PERIMETER_BELT`) lines both sides of the Perimeter, clear of the patrol
  track and the gate; along the patrolled stretch it grows thicker, mostly
  bushes (`bushy`), with trees on both sides (`tree_belt`,
  `canopy::belt_trees`). A separate continuous wall marks the map's edge; the panels
  follow the rendered terrain and stay above the water. The wall is drawn out
  to 800 m from the camera (`WALL_SHOWN`, fading over the next 50 m): its
  chunks stream in the terrain tiles within reach of each camera
  (`stream_wall_view`), one chunk per frame, nearest first (a chunk takes up to
  ~4 ms to build); the full drawn wall costs about 1 FPS against the old 400 m
  reach. Its collision posts stream separately, in the tiles around a
  `StreamAnchor` (`stream_wall_solids`). Anything that stands on the wall
  carries `OnWall` and is hidden while its tile's wall chunk is not drawn
  (`show_with_wall`), so nothing hangs in the air past the wall's end.
  Keep the staging area and driveway clear; the tree and bush shelterbelt is
  concentrated behind the hangar and along the sides.
  The wall repeats one baked box thousands of times: an 84-vertex box
  (`site-cordon-wall-box.glb`: scan colour and normal map, baked tangents, no
  bottom), a depth map (`textures/cordon/wall_depth.ktx2`, linear BC4) and a
  razor-wire strip (`site-cordon-wall-wire.glb`, cut-out and double-sided), all
  baked from the UHD scan by `tools/bake_cordon_wall.py` (Blender 4.5, about 3
  minutes; its docstring has the options and the measurements it prints):

  ```bash
  "$BLENDER" -b --factory-startup --python-exit-code 1 --python tools/bake_cordon_wall.py -- \
    ~/Projects/agentic/RoadSide-Picnic/models/sites/uhd/site-cordon-wall.glb \
    assets/models/sites/medium assets/textures/cordon
  ```

  A third GLB, `site-cordon-wall-sandbags.glb` (`bake_cordon_wall.py --only
  sandbags` bakes just it, in about 25 s, leaving the others untouched), lays a row
  of sandbags along the foot of both faces: the scan has only one block of bags
  (two wide, four layers) at its left end, so one module of it is baked (colour
  and tangent normal, 740 x 632 px) and repeated seven times along the panel and
  mirrored on the back, 696 triangles in all (each side's row is closed by a flat
  cap at both ends, so its end seen end-on at a gate or a corner is solid);
  `BAG_REACH` and `BAG_TOP` are its measured reach (0.1989 from the line, 1.47 m)
  and height (0.0532, 0.39 m). `WallModel::batch` shears it onto the ground like
  the others, but it lies on the ground (`BAG_SINK`, 2 cm) instead of standing 9 cm
  in it, and it rises with its panel, so the Perimeter's bags are three times as
  high. A negative corner lap shortens only the box: both runs' rows run on to the
  outer corner (`WallModel::insets`), so no cap is left with a bare foot.

  The box is 0.147 model units either side of its line (`WALL_HALF_THICKNESS`,
  1.09 m at `WALL_HEIGHT_SCALE` 7.4: a wall 2.2 m thick and 3.04 m high, the
  Perimeter three times that high) and its top is `WALL_TOP`; remeasure both
  when the bake changes. `WallModel::batch` stretches each copy along its run,
  shears it onto the ground and shears its tangents with it, so the box's
  relief mapping (`ParallaxMappingMethod::Relief`, `PARALLAX_DEPTH_SCALE`) does
  not swim on sloped ground. Panels in a run meet end to end; where two runs
  meet in a corner (`end_lap`) the earlier one laps `CORNER_LAP` (the half
  thickness) past the other's line and the later one stops that far short, so the
  outer corner closes and no two faces share a plane. A gate's or the checkpoint
  gap's end, and a run ending against the middle of another (the Perimeter on the
  map-edge wall), stops where it is. A post every 0.77 m fences the wall so a body
  stops at its visible face: a disc of 1.15 m radius up to the box's top
  (`WALL_DISC`) and one of 1.53 m up to the sandbags' top, which lies on the ground
  (`BAG_DISC`, 0.39 m, 1.16 m on the Perimeter); both fence at any height, so a
  stalker cannot step onto the bags. The crawl hole's floor reaches 2 m either
  side of the wall's line (`HOLE_FLOOR.x`), wider than the wall, and a crawler
  passes under the bags' discs as under the wall's.
  Scenery keeps `FOOT_REACH` (the sandbags' front) plus its clearance from the
  wall's line. Both checkpoints use
  the UHD scan; re-measure `src/world/pillars/uhd_checkpoint.rs` with
  `tools/sample_landmark_ground.py`. The old 2,875-triangle panel is in the
  assets archive zip.
- **Sites** (`src/world/sites.rs`): the Harmont map from `doc/` (map, layout
  notes and landmark CSV) as one registry of 32 placements of the UHD landmark
  and site scans. `src/world/map.rs` lays `doc/Roadside_map.jpg` over the world
  at 2.4 m per pixel (`map(px, py)`: x = (705 - px) * 2.4, z = (383 - py) * 2.4)
  and holds the map's railway line, so placements can be checked against the
  image. Harmont lies on the hangar's side of the Perimeter (+X): New
  Harmont, the Institute on the Perimeter, the Plague Quarter, the Borscht (the
  stalkers' red-roofed tavern, stretched to twice its scan's width with
  `Model::widened`) and the Old Borscht (Ernest's gutted first bar), the school,
  houses and cemetery. The Zone lies beyond it (-X),
  deepening toward the quarry: abandoned suburbs, the factories, highway,
  Garage, railway with a locomotive and the derailed train, the marsh and
  witches' jelly flats, the ridge, the crater, the quarry rim and the Golden
  Sphere. Seen from above with +Z up, -X lies to the right, so the world reads
  like the reference map (the code's +X "east" shows on the left of that view).
  Buildings stand on level pads at their measured floor datum; dioramas with
  their own landscape carry a `Relief` grid the terrain follows, anchored with
  the middle of their road-facing edge on the land at the entrance. Measure a
  scan's collision and relief with `tools/sample_landmark_ground.py` (see its
  docstring). All scans at once outgrow a 6 GB GPU, so each site shows a light
  stand-in from `low/` and shows its full UHD scene within 150 m of its
  footprint around any `StreamAnchor`: `stream_full_sites` wants the scan from
  `GpuScans` 40 m before that (`UPLOAD_LEAD`) and shows it once it is on the
  GPU, then drops it 60 m past the range, or if the scan leaves the GPU, and
  the stand-in shows again. The Institute is lit like the hangar
  (`src/world/institute.rs`): the hangar's pendant fixture (`base::LampKit`,
  same housing, lens and `PENDANT_POWER` spot) hangs on brackets 0.55 m out
  from the front and back façades at 4.6 m (four each, away from the porches),
  two hang under each porch canopy (3 m), and a fixture-less floodlight 14 m
  over each roof helipad lights the pads. Their positions are in the scan's
  model units, measured from `uhd/institute.glb` and posed with
  `sites::institute_pose`; none casts shadows. The Institute's Tripo scan came without
  UVs or textures: `tools/prepare_institute_asset.py` (headless Blender) paints
  it with one atlas from its concept image in the workspace (window bays on the
  front and back, precast panels on the sides and roof), then builds its
  stand-in as below. Build a stand-in from the JPEG source
  with glTF Transform `simplify --ratio 0.02 --error 0.002` and `resize --width
  1024 --height 1024`, then compress; never ground or recenter it.
- **Quarry ramp** (`src/world/{quarry_ramp,ramp_cut}.rs`, `roads.rs`, `terrain.rs`,
  `sites.rs`, `assets/shaders/ramp_cut.wesl`, `tools/plan_quarry_ramp.py`): a haul
  road winds down the Quarry rim diorama's pit (48 m deep) to its floor, walkable
  and drivable by the stalker, the car and the carrier.
  - *Path:* `python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_quarry_pit_ground.rs
    src/world/pillars/quarry_ramp.rs` (about a second; `python3 -m unittest
    tools/test_plan_quarry_ramp.py`) searches a 32x32 grid over the footprint (8
    headings, turns of at most 45 degrees, at least 11 m between turns) for the
    path from the front edge's middle down to the floor that keeps closest to the
    relief at a constant grade (a fill costs its square over 9 m, a cut twice that;
    each turn costs), rounds the corners into 9.5 m arcs and writes the centerline
    in model units: 83 points, 244 m, grade 0.198, at most 6.8 m of fill and 2.2 m
    of cut against the smoothed relief, runs at least 22.7 m apart. The tool
    refuses a road steeper than `--grade` or turning tighter than 9 m. The file is
    generated: regenerate it, never edit it.
  - *Other pits:* those numbers are the tool's defaults (a `Site`). `--scale 1` with
    `--centre`, `--half`, `--grid`, `--entrance`, `--heading`, `--end` and
    `--avoid` (see the tool's docstring) plans a road in any relief in metres: to
    a point instead of the floor, keeping every baked point 9 m (`--shoulder`)
    clear of the no-go boxes. A node of the grid is put on `--end`, and an entrance
    inside the box snaps to a node, so neither end has a kink. The tool prints the
    closest approach of separate runs and of the road to a box, and a run other
    than the quarry's writes its options and numbers into the file's header.
    `--turn left|right` makes the road turn only that way (a spiral) and
    `--min-separation M` keeps its separate runs M m apart in the search, not
    only in the report; a road with many corners keeps to `--grade` as baked (the
    search allows for the length its arcs remove).
    `test_quarry_ramp_regenerates_unchanged` keeps the quarry's file byte for byte.
  - *Track:* `sites::quarry_ramp()` is the junction, the entrance and the baked
    points under the site's pose, so the quarry's 60 m drive is the ramp track's
    head (`Surface::Ramp` throughout: one nearest-leg choice, so no step in the
    road or the terrain where a drive and a ramp would meet). The drive leaves
    the main dirt road at the ramp's width: at the junction the main road's 8.5 m
    half-width (gravel to 7.5 m) gives way to the ramp's 4.5 m bed (gravel to
    4.2 m) with no taper, a visible step in the road's edge (playtested, kept).
    The drive follows the land to the entrance; from there
    the ramp falls 48 m over 251 m (0.191) to the floor. A parabolic crest, 24 m long and centred on the
    entrance (`graded_track`, points every 1 m), rounds the 0.27 grade change (radius about 89 m, so a car
    stays on the ground up to about 29 m/s; the sharp break launched the armored car at 21 m/s). The terrain
    applies a ramp after the diorama's ground (`lerp(h, ramp.height, ramp.mask())`),
    cutting and filling the pit's relief inside the corridor.
  - *Collision:* both cut scans (`quarry_ramp::CutScan`: the Quarry rim's and the
    Golden Sphere's crater, `landmark-core`) drop the pillars whose centres lie in
    their cut corridor. A scan's corridor is every ramp leg whose box comes within
    two shoulders (18 m: the leg's own and the cut image's) of its footprint box
    (so the quarry's takes in the crater road's first leg, 23 m of it in the box
    around the scan, on the pit's floor).
    `CutScan::pillars()` keeps 1,658 of the
    quarry's 3,186 pillars (the crater road's leg clears none more) and 1,708 of the
    core's 4,034 (2,326 dropped, 58%), the one rule `CutScan::in_cut_corridor` shares
    with the cut's texels (within 9 m of a centerline); filtered once at first use
    from the same points. `Landmark::solids` serves it, so the stalker, the car,
    the carrier and the router meet no invisible wall on either road, its banks or
    the rest of the cut. Tests run per scan: `no_{quarry,core}_pillar_stands_on_the_ramp`,
    `kept_{quarry,core}_pillars_leave_only_a_small_lip_in_the_cut`,
    `{quarry,core}_pillars_off_the_ramp_are_kept` (at most 55% and 60%),
    `the_widest_mover_walks_the_{quarry,crater}_ramp_unpushed` and
    `the_quarry_corridor_covers_the_crater_roads_first_leg`.
    The centre rule leaves a sub-metre lip (pillar radius 0.9 m): dropped pillars
    reach up to 0.9 m past the 9 m edge (quarry 135, core 311), stone there without
    its own pillar, and kept pillars bulge up to 0.9 m into the cut (quarry 133,
    core 269), at least 3.6 m off the 4.5 m bed. The tests above bound that lip
    at 1 m and keep it off the bed by the carrier's 2.3 m reach.
  - *Cut:* the scans are untouched. `RampCutMaterial` (`ramp_cut.rs`,
    `ExtendedMaterial<StandardMaterial, RampCut>`) draws the stand-in and every
    streamed full scan of both cut scans, each through its own image and bounds
    (`RampCuts`, one `RampCut` per `CutScan`; the shader is shared and unchanged;
    `RampCutScan(scan)` marks a full scan, and `cut_scan_stone` swaps the material on each
    stream-in; it is not cached, which would pin the scan's textures in VRAM) and
    discards the fragments above a cut surface over the corridor, in the main
    pass and the shadow maps' depth pass (its `AlphaMode::Mask` makes the
    shadow pass run the shader; the shader's deferred-prepass variant is
    written but never exercised, the camera is forward), so the road is
    neither covered nor shadowed by the stone removed over it. Each image
    (`build_ramp_cut`, built once at entering the Zone over the scan's footprint
    box and a 9 m shoulder) is an Rg32Float lattice of points 0.5 m apart (the
    quarry's 222x254, 441 KiB, 108 ms, the core's 273x253, 540 KiB, 155 ms, in
    the playtest build). Red holds the ground drawn at the point (the 2 m render
    mesh, `Terrain::mesh_height_at`, not `height_at`, which differs by up to
    0.11 m on the bed and by metres, tens of metres in the crater, on the banks)
    less `BURY` (0.1 m), or `1.0e6` more than 1 m past the corridor. Green holds
    how far the point lies past the corridor's edge, 9 m from a centerline
    (`CutScan::past_cut_corridor`, negative inside). The shader interpolates
    both linearly over each half of a lattice cell, split along its
    anti-diagonal as the mesh's triangles are, and cuts a fragment when green
    is negative and the fragment lies above red. The lattice lies on the
    world's 0.5 m grid, so the mesh's 2 m grid lines run along cell edges and its
    diagonals through cell corners: the mesh is linear on each half cell and the
    cut lies exactly `BURY` under the drawn ground (an unaligned image left
    the core's cut 0.098 m above the mesh on a crater bank). The corridor's edge,
    where green crosses 0, runs straight within each half cell, so the cut's
    outline on the stone follows the ground smoothly; nearest-read texels of
    each cell's lowest corner had stepped it in 0.5 m teeth, on the banks by
    metres a tooth. Stone left above the drawn ground, even by centimeters, shows
    through it as dark slivers; `the_{quarry,core}_cut_lies_below_the_ground_that_is_drawn`
    keep it below (on a 0.37 m grid, through a copy of the shader's
    interpolation, the cut lies 0.1 m under the mesh to 5 mm in both scans),
    `the_{quarry,core}_cut_ends_at_the_corridors_edge` that a point is cut
    exactly when it lies in the corridor whose pillars are dropped (to 5 cm of
    the edge), and `the_quarry_cut_follows_its_ramp_and_the_crater_roads_first_leg`
    and `the_core_cut_follows_the_crater_ramp` check the road in each scan is cut.
    Limits: the main and depth-only (shadow) pipelines are exercised, the
    deferred prepass variant is written but never run, and a normal or
    motion-vector prepass is not supported (the camera runs none); the lattice
    follows the 2 m near mesh, so the far 8 m ground tiles may leave slivers at
    a distance.
  - *Driving and orders:* the stalker running, a car at 7-10 m/s and the
    carrier at 4.5 m/s (its turn rate of 0.6 rad/s makes a 9.5 m arc a 5.7 m/s
    limit) drive or walk from the junction to the floor and back by the keys,
    without a block, a sinking or a lift off the mesh; a car at full throttle
    (23-25 m/s) stays on the ground over the crest (0.02-0.07 m above the mesh
    from the junction to 23 m past the entrance; it left the road only at the
    first bend, driven straight on). Orders take the ramp, not the pit's front
    wall (`a_click_order_reaches_the_pit_floor`). The ramp's north loop runs out of
    the trip's box and 28 m for a vehicle's width, so the car's and the carrier's
    orders from the entrance to the floor take the wider search (0.23-0.29 s a
    click, against 0.06 s for the stalker).
  - *Golden Sphere's crater* (`sites::crater_ramp()`, `sites::cut_scan(CutScan::Core)`, `roads.rs`;
    generated `world/pillars/golden_crater_ramp.rs`): the only road to the Golden
    Sphere, a second `Surface::Ramp` track that runs on from the end of the quarry's
    ramp and spirals down inside the Sphere's crater scan (`landmark-core`) to its
    floor. The Chapter 4 route ends at `QUARRY`; no other route or drive comes nearer
    to the Sphere than that junction, 226 m away
    (`the_only_road_to_the_golden_sphere_runs_through_the_quarry_rim`). The Sphere has
    no dirt drive of its own: `tracks()` leaves out the drive of `GOLDEN_SPHERE` as it
    does the quarry's. The Sphere itself is a painted golden glow on the crater's
    lowest wall (a 44 degree incline, no geometry of its own, about 0.6 m across), at
    model (-0.0172, -0.0138) of the core, world (-1229.3, 86.1). The real Sphere
    hovers over a pool on the floor 14.7 m from that glow (see **Golden Sphere**);
    the glow is still painted in the scan, and "the Sphere" in the road's checks
    below means this glow (`sites::SPHERE_SMUDGE`).
    - *Track:* the quarry ramp's last point (-1063.6, 101.7), at -48.6 m on the Quarry
      rim's floor, `GOLDEN_SPHERE` (-1152, 84), the core's `entrance()` (-1161.8, 84)
      and the baked points under the core's pose, the first on the front edge
      (-1168.8, 84). `graded_track` treats the track's first leg as its drive and the
      second point as the crest, so the drive is the 90 m from the quarry ramp's end to
      the junction: one straight grade between the sited land at its two ends (-48.6 m
      and -36.8 m, 0.13 up), with no step at the quarry ramp's end
      (`the_crater_ramp_continues_the_quarry_ramps_level`). The 24 m crest lies at the
      junction (a 0.30 grade change; `the_ramp_crest_is_rounded` checks both ramps), and
      from there the ramp falls at one grade, 0.170, over 398 m to the last point's
      level (-104.4 m). The junction lies on the saddle between the quarry's and the
      core's ground blends: the drive cuts up to 8.7 m and fills up to 4.3 m against the
      land, and the land rises 3.4 m from the junction to the entrance, so the road
      lies 4.9 m below the land at the entrance and 6.8 m at the front edge, a cutting
      before the crater. The drive's leg to the entrance has no corner of its own: the
      kinks are 11.3 degrees at the junction and 3.3 degrees at the first baked point
      (a 7 m leg, then 3 m ones), by the circle through three points 254 m and 86 m;
      `the_crater_ramp_bends_no_tighter_than_9_m` holds every corner of the track
      to 9 m and `the_connector_meets_the_spiral_in_line` the last kink to 10 degrees, so
      none is rounded.
    - *Path:* `python3 tools/plan_quarry_ramp.py src/world/pillars/uhd_core_ground.rs
      src/world/pillars/golden_crater_ramp.rs` with the options in the file's header
      (`--scale 120`, entrance at the front edge's middle, `--turn right`,
      `--min-separation 18`, `--end` on the floor beside the Sphere, grade at most 0.19):
      a right-hand spiral of 128 points and 381 m, laps at least 20.6 m apart, corners of 9.5 m.
      An `--avoid` box 1 m around the Sphere and the road's 9 m shoulder keep every leg 12 m or
      more from the Sphere; the road ends on the floor 13 m from it, at (-1229.3, 73.1). The
      search box (`--centre`, `--half`) keeps the centerline 6.7 m or more off the tile edges.
      The file is generated: regenerate it, never edit it.
    - *Cut and fill:* deep by design, the road makes its way through the crater's walls
      (nothing caps it). Against the land the ramp cuts up to 49 m through the high west
      rim (at world (-1263, 121), 215 m along the track) and fills up to 25 m on the east
      edge (at (-1259, 32), 316 m along), over the scan's hole there; the quarry's
      cut and fill tests do not apply. The crater ramp overrides the core's ground as
      the quarry's does its own (the core's ground blend is not a limit).
    - *Cut and collision:* the core scan is a cut scan like the quarry's (see
      *Collision* and *Cut* above): its stone is discarded above the drawn ground
      along the spiral, and its pillars there are dropped. The terrain draws the
      banks: the ramp's mask falls from 1 at the bed's 4.5 m to 0 at 9 m, so a 49 m
      cut stands as a terrain wall over those 4.5 m, meeting the scan's uncut stone at
      the corridor's edge. In the game that edge is a vertical seam on the deep
      west banks: the scan stone stands to 9 m from the centerline and is cut flush
      there, above the steep terrain wall, which the terrain shader dresses in
      rock (see *Terrain*; it was dirt streaked down the bank). The seam is kept:
      fading the cut out past 9 m would hide stone whose pillars still block
      (only those inside the corridor are dropped) and open the scan's hollow
      shell behind its face. On the floor the cut's edge on the Sphere's wall
      follows the ground in a smooth curve beside the road's end. Click orders reach the crater's
      floor through both scans' cleared collision along the 799 m of road
      (`click_orders_reach_the_crater_floor`; see *Driving and orders*).
    - *Checks:* `the_crater_ramp_starts_at_the_quarry_ramps_end`,
      `the_crater_ramp_ends_beside_the_sphere` (ground below -95 m, 5.1 to 15 m from the
      Sphere's centre, not under the bed), `the_crater_ramp_laps_keep_apart` (18 m),
      `the_crater_ramp_keeps_clear_of_other_tracks` (18 m, after its first 30 m),
      `a_ramp_is_graded_evenly` and `every_track_joins_the_network_back_to_the_gate`
      (both ramps), `the_crater_ramp_keeps_off_the_sphere` (every leg 10 m or more from
      the Sphere's centre) and `the_crater_ramp_grade_stays_drivable` (the render mesh
      along the centerline and 1.8 m either side; where a deep cut crosses a tile edge,
      the 8 m edge stitch pulls the 2 m mesh up to 0.17 m off the road, so a bake must
      keep its centerline off the 128 m tile edges x = -1280 and z = 128, model z =
      -0.437 and x = -0.367).
    - *Driving and orders:* driven from the quarry's junction to the floor (799 m)
      and back by the keys: the stalker running (4.5 m/s, 176 s down) and walking
      (1.8 m/s, no slowdown on the grade), a car at 8 m/s (103 s each way) and the
      carrier at 4.5 m/s (189 s each way) keep within 1.1 m of the centerline,
      without a block, a sinking or a lift off the mesh (the carrier's hull rides up
      to 0.25 m over its tracks' tilt). A car at full throttle (24 m/s) from the
      Quarry rim's floor over its ramp's end (the grade turning from -0.19 to +0.13)
      and the junction's crest lifts 0.05 m for one frame at the sag and nowhere else.
      The bends are 9.5-15 m (the baked corners' tightest arcs are 9.5 m), tighter
      than the 16.7 m the carrier turns at its 10 m/s top speed: driven flat out, it climbs the lowest
      laps' banks and stalls there once; it holds them below 6.8 m/s (driven at 4.5).
      Click orders keep to ground each mover can climb, so the car's and the
      carrier's follow the spiral road down (the stalker's cut across where the
      ground is no steeper than 45°), and all three reach the floor from either
      junction (`click_orders_reach_the_crater_floor` checks all three movers
      through `route` from both junctions, the road in from the hangar and the rim:
      clear of the scans' stone and on ground no steeper than each climbs, where
      slope-blind routes took the walls, up to a 1.64 grade;
      `a_car_ordered_down_the_crater_road_arrives` drives the car's order). The
      trip's box with 28 m around it leaves out the spiral's outer laps, so these
      orders take the wider second search (84 m): a click costs 0.2-0.6 s near
      the crater (0.19-0.23 s from the Golden Sphere's junction and 0.43-0.52 s from
      the Quarry rim's in the playtest build, both scans streamed in). By click
      (release-speed playtest build): the stalker takes 81 s from the Golden Sphere's junction (a 365 m route) and 129 s
      from the Quarry rim's (582 m, planned before the core streams in); the car
      32 s (380 m) and 63 s (770 m), on the ground all the way (at most 0.06 m off
      the mesh, 3.5 m off the centreline at the first bend), slowing for the bends
      to its 8 m/s bend speed and 24 m/s between them; the carrier 49 s and 88 s at
      its 5 m/s bend speed, holding the bends, with one stall on the lowest lap
      at (-1225, 91) from the Quarry rim's junction, where the route's last
      hairpin brings the hull's corners (3.9 m out) against the bank past the
      2.3 m it is routed by; one back-off clears it. The floor's click point is too
      near the Sphere's wall for the carrier, so its route ends at the nearest
      cell it fits, 2-4 m away. Before the autopilot slowed for bends the car took
      the first bend at 21 m/s, ran 6 m wide and flew 30 m down to a wall between
      laps. On the main thread the crater costs at most
      0.7 ms a frame more than the Quarry rim (7.7 against 7.0 ms); the core's cut
      scan reaches the GPU behind the splash, with no hitch in the game.
- **Golden Sphere** (`src/world/golden_sphere.rs`,
  `golden_sphere/{ferrofluid,mirror,reflection,uhd}.rs`,
  `assets/shaders/{ferrofluid,golden_sphere}.wesl`, `sites.rs`, `assets/gpu_scans/tier.rs`):
  the artifact itself, a 1 m mirror-gold ball that hovers and turns over a 3 m pool
  of black ferrofluid on the floor of its crater. How much of the world it mirrors
  follows the GPU's tier (see **RAM and GPU**).
  - *Sphere:* the medium preview model (`ARTIFACT_PREVIEWS[1]`) scaled to 1 m
    (`SPHERE_DIAMETER`) with its paint made a polished mirror (metallic 1, roughness
    0.075, reflectance 1). Its centre hovers 2.25 m over the pool and bobs 0.15 m
    either way (2.1 to 2.4 m, so its underside is 1.6 to 1.9 m up) over a 7 s period,
    lowest at the start. It turns 0.3 rad/s (a turn in 21 s) about an axis that leans
    4 degrees, the lean circling once in 23 s. One clock, `HoverClock`, drives
    Sphere and pool; it counts game seconds only while `InGamePhase::Running`, so
    a pause holds both still and resuming carries on from where they stopped
    (virtual time runs on through the pause).
  - *Field:* the pool's spikes follow the Sphere's pull, a magnet's cube law
    scaled to 1 with the Sphere lowest and 1/3 with it highest (`Hover::field`).
  - *Pool:* at `sites::golden_sphere_spot()`, a model-space constant of the core
    scan (0.074467, 0.067451), 14.7 m from the painted glow: the flattest floor
    within 20 m of it with the pool's rim 3 m clear of the road's bed (the floor
    rises at most 0.26 m across the pool, a 5 degree slope). The test
    `the_golden_sphere_hovers_over_the_flattest_floor_clear_of_the_road` searches
    that area on a 0.5 m grid and holds the constant within 1 cm of the flattest
    spread. There is no search tool: the constant was found once and the test pins
    it. The liquid's level is the highest `Terrain::mesh_height_at` over a 17 x 17
    grid of samples across the 1.5 m radius (`SPHERE_POOL_RADIUS`) plus 5 mm, read
    when the Zone is entered, so the floor never shows through it; the disc's outer
    0.4 m bends down to the floor along a smoothstep. The pool has a `Collider`
    (`POOL_SOLID`, a pillar of the same radius from 0.1 m under the liquid to 3.4 m
    over it) and is `Unclimbable`: the stalker walks around it and cannot pass
    under the Sphere. Black metal as polished as the Sphere: linear 0.15, 0.15,
    0.165, metallic 1, roughness 0.075. The disc's grid is 2 cm on modest GPUs,
    1 cm on strong and 5 mm on exceptional ones. Its normals are area-weighted:
    Bevy's angle-weighted `compute_smooth_normals` skips corners whose edges'
    squared lengths multiply to under `f32::EPSILON`, which zeroed every normal
    on the 1 cm and 5 mm grids, so the liquid mirrored nothing and read flat
    black. It draws in a camera's depth prepass where one runs, raising this
    frame's spikes and, for their motion vectors, last frame's
    (`previous_field`, `previous_rings`), so an upscaler does not smear them;
    while paused both match (`hold_ferrofluid`), so the spikes report no motion.
    Dormant: no camera runs a prepass today (DLSS was tried and dropped). A
    camera given a depth prepass must also get `MotionVectorPrepass`: without
    it the prepass draws every spike while the main pass fades far ones, and
    the pool tears.
  - *Ferrofluid* (`ferrofluid.wesl`, vertex stage; the fragment stage is standard
    PBR): the flat disc is raised in the shader by a crown of spikes on a hexagonal
    lattice 0.45 m apart, concave cones with rounded tips whose feet touch, turning
    with the Sphere. The tallest spike is 1.0 m with the Sphere highest and 1.4 m with
    it lowest, 0.2 m clear of the Sphere; the others fall off with a Gaussian of 0.35
    m sigma about the middle. Spikes stand only where the field times that Gaussian
    passes 0.15 and in full over 0.3 (as in a real ferrofluid), so the crown reaches
    0.44 m out with the Sphere highest and 0.68 m with it lowest. Three rings of 12
    small spikes (feet 9 cm in radius) creep in from 1.2 m to 0.55 m from the middle,
    9 s a ring, staggered by a third; each spike rises from the liquid over the first
    15% of its way and sinks into the crown over the last 30%. Each pulses between 20
    and 40 cm at its own phase and rate, 20 to 39 whole pulses in the 60 s the pulse
    clock takes before it wraps, so the wrap never shows. The shadow pass runs the
    same vertex shader (`prepass_vertex_shader`), so every spike casts its shadow; in
    the main view spikes fade to smooth liquid as their spacing on screen falls from 8
    px to 3 px, judged from the pool's middle. `pull_ferrofluid` writes only the
    field, the rings and the probe's centre into the pool's material each frame.
  - *Modest GPUs* (`mirror.rs`, `golden_sphere.wesl`): no live map. The Sphere's
    fragment shader traces its reflection ray against up to three stand-ins within
    15 m of it (`REACH`, measured from a body's centre less the half diagonal of its
    footprint): the stalker as a capsule 0.3 m in radius and 1.8 m tall (0.5 m while
    crawling or prone), left out while driving, and the car and the Institute carrier
    as boxes of their hulls (`Seat::half_hull`), turned with them. Each is lit by the
    Sun (direction and illuminance read from the `Sun` each frame) plus the sky at
    20% of the Sun's, both fading out as the Sun sinks from 0.1 to -0.05 of
    height in its direction (`stand_in_light`: the `Sun` keeps its illuminance
    after dark, only its shadows go), in the model's measured paint (the mean linear albedo of its
    base-colour texture: stalker 0.0448, 0.0526, 0.0526; car 0.0598, 0.0359, 0.0266;
    carrier 0.0636, 0.0494, 0.0409), tinted by the gold. The pool does not mirror the
    Sphere here.
  - *Strong GPUs* (`reflection.rs`): a live reflection probe. Six cameras at the
    Sphere's centre (90 degrees, near 0.05 m, far 300 m) render the scene into the
    six faces of a cube, 256 x 256 `Rgba16Float` images, only while the player's
    camera is within 300 m of the Sphere (`SEEN`, its `detail_range`): one face a
    frame in turn while the stalker or a vehicle is within 15 m of it, otherwise
    one face every 10 frames (`RESTING_EVERY`; the whole cube in two seconds), so
    the light and a departed stalker never linger in it. Beyond 300 m the cameras
    sleep, so a car parked by the Sphere costs nothing. The Sphere samples
    the face its reflection ray points at, and the liquid traces the Sphere
    analytically (a ray-sphere test against its centre and 0.5 m radius, the
    centre updated each frame) and bounces the ray into the faces, tinted gold, so
    the gold shows in the black metal; elsewhere it samples the faces along its own
    reflection. The cameras are cleared transparent, so where they drew nothing
    (sky) the environment map's sky stays. They copy the player's camera's
    `Exposure` each frame (EV 9 at night to 13 by day, less under cloud), HDR, no tonemapping or deband and no MSAA, no atmosphere or fog, and
    `ClusterConfig::Single` (one cluster for the whole view).
  - *Exceptional GPUs:* the faces are 1024 x 1024, rendered one a frame in turn
    with someone within 15 m and one every 10 frames otherwise, as on Strong
    (each face is a full view with its own sun cascades, about 7 ms on a 2060:
    all six a frame held 17 FPS, one a frame 42), and the Sphere's UHD scan replaces the medium
    model. The scan (`models/artifacts/uhd/artifact-golden-sphere.glb`, from the
    workspace's `models/artifacts/uhd/`, 1.97 M triangles, one 8192 x 8192 BC7 KTX2
    gold texture, 70 MB on disk, 138 MB on the GPU measured) is streamed through
    `GpuScans` (`stream_uhd_sphere`): wanted once the nearest `StreamAnchor` is
    within 80 m (`UHD_RANGE`, 40 m, plus `UPLOAD_LEAD`, 40 m), and swapped in (mesh
    and gold, rescaled to 1 m from its 0.98 m, centred diameter) within 40 m once
    `scene()` is resident. The medium model comes back when the anchors pass 40 m
    or the scan is evicted. Only exceptional GPUs list it, last in `UhdFiles`
    (`list_uhd_sphere`, at `Startup` once the tier is settled and before the RAM
    fill starts), so the RAM fill and the prefill take it after every scan all
    tiers show, and other tiers never read its 70 MB.
  - *Bounds:* the pool's `Aabb` is the disc's raised to the tallest spike (1.4 m,
    `Ferrofluid::bounds`) with `NoAutoAabb`, since only the vertex shader raises
    the spikes: with Bevy's own box the main view, the shadow cascades and the
    probe's side faces (which look level from 2.25 m and never see the disc)
    would cull the spikes. `NoFrustumCulling` would not do: the fog of war culls
    by `Aabb`.
  - *Render layers:* only the Sphere's mesh is on `SPHERE_LAYER` (1). The player's
    camera sees layers 0 and 1; the probe cameras see layer 0 only, so the probe
    never sees the Sphere it renders for. The Sphere's shadow comes from a twin
    of its medium model (4,852 triangles) on `SPHERE_SHADOW_LAYER` (2), which no
    camera draws and only the Sun sees (layers 0 and 2); the shown Sphere is a
    `NotShadowCaster`. Bevy gives every active camera its own sun cascades, and
    the UHD scan's 1.97 M triangles in all of them, the probe faces' included,
    cost a 2060 from 10 to 17 FPS with every face rendering.
    The pool is on layer 0 and shows in the probe. Anything that looks for the
    player's view must filter `PlayerCamera`, not `Camera3d`: the probe adds six
    more cameras. `pose_wildlife` and `stream_wall_view` did take every `Camera3d`.
  - *Measured* on the 6 GB RTX 2060 (playtest build in Xephyr, beside the Sphere):
    modest 34 FPS, strong 27-28 FPS, exceptional (forced, `--gpu-tier exceptional`)
    10 FPS, all with Bevy's GPU light clustering, which cost 30-44 ms of CPU per
    camera view per frame. With CPU clustering (`render::use_cpu_clustering`, see
    **RAM and GPU**) Strong measures 26-28 FPS at full clocks (findings in
    `~/Projects/agentic/roadside-picnic-overwatch/reviews/2026-11-27-gpu-usage-findings.md`).
    On the real display (1440p, at the Sphere, 2026-11-28): Strong 38 FPS;
    Exceptional with the shadow twin and one face a frame 42 FPS; 2-6 FPS less
    in ground fog or at the widest zoom.
  - *Checks:* `the_spikes_stand_tallest_when_the_sphere_sinks_lowest`,
    `the_field_falls_as_the_sphere_rises`, `the_sphere_turns_at_a_steady_rate`,
    `the_sphere_holds_still_while_paused` (`golden_sphere.rs`);
    `the_pool_mesh_is_a_disc_dense_enough_for_its_spikes`,
    `the_pools_liquid_faces_up_on_every_tier`,
    `the_pools_rim_bends_down_to_the_floor`,
    `the_central_spikes_stand_1_4_m_under_the_lowest_sphere_and_1_m_under_the_highest`,
    `the_small_spikes_creep_inward_ring_after_ring`,
    `the_small_spikes_pulse_on_a_clock_that_wraps_without_a_jump` (`ferrofluid.rs`);
    `only_what_is_within_reach_shows_in_the_sphere`,
    `a_driving_stalker_shows_only_as_the_car`, `a_crawling_stalker_is_a_low_capsule`
    (`mirror.rs`); `the_probe_renders_one_face_a_frame_on_strong_cards`,
    `the_probe_renders_every_face_on_exceptional_cards`,
    `the_probe_rests_when_nobody_is_near_or_on_modest_cards`,
    `re_entering_the_zone_keeps_six_probe_cameras` (`reflection.rs`);
    `the_uhd_sphere_shows_only_on_exceptional_cards_near_and_resident`,
    `an_evicted_uhd_sphere_falls_back_to_medium` (`uhd.rs`).
  - *Open:* the golden smudge painted into the core scan's 8K texture (model
    (-0.0172, -0.0138)) still shows 14.7 m from the real Sphere; removing it
    needs the scan's texture edited and recompressed
    (`~/Projects/agentic/roadside-picnic-overwatch/reviews/2026-11-27-golden-crater-ramp-followups.md`).
- **New Harmont** (`src/world/town.rs`): buildings cut from the Harmont town
  scan (`assets/models/harmont/`) line a street grid around a paved downtown
  plaza on one level town pad; each shows its full scan near the view and a
  light version farther out.
- **Railway** (`src/world/railway.rs`): the old line from south to north
  (`map::RAILWAY`) is laid from the railway embankment scan
  (`assets/models/railway/`): sleeper ladders end to end along the graded bed,
  two rails on top and wreckage beside the line. `tools/align_kit_piece.py`
  squares up each kit piece (+Z along the line, grounded at y = 0).
- **Old roads** (`src/world/roads.rs`, `terrain_material.rs`, `assets/shaders/terrain.wesl`):
  every `Surface::Asphalt` leg (the highway, Harmont's main road, the Institute
  and Plague roads) is painted by the terrain shader; there are no road meshes.
  Where the mask's alpha is set, the shader paints four maps baked from one
  photo by `tools/prepare_asphalt_textures.py` (`assets/textures/terrain/asphalt_{albedo,normal,surface,crack}.ktx2`,
  bindings 110-113: 184 px native crops, BC7 with mip chains) over the tar.
  The mips are the distance versions; there is no second set. Rebuild the maps
  from the photo kept in the assets workspace (needs Pillow, numpy and the `ctt`
  CLI; the PNGs they are encoded from go to `--sources`):

  ```bash
  python3 tools/prepare_asphalt_textures.py \
    ~/Projects/agentic/RoadSide-Picnic/textures/asphalt/image_5b974df1.jpg \
    assets/textures/terrain --sources ~/Projects/agentic/RoadSide-Picnic/textures/asphalt
  ```

  - *Grain:* tiled every `ASPHALT_TILE` (1.2 m). Each lattice point of the
    tiling gets a hashed offset into the map and the four around a pixel blend
    bilinearly, rescaled to the map's average (`map_mean`) so that neither a
    lattice nor contrast dips show.
  - *Cracks:* one decal at most per `CRACK_CELL` (8 m) cell, in `CRACK_SHARE`
    (50%) of the cells, hashed to a random turn, size (60-100% of
    `CRACK_SIZE`, 4 m) and offset inside the cell. `crack_decals_fit_inside_their_cells`
    keeps `CRACK_SIZE * sqrt(2) / 2 + CRACK_JITTER / 2` within half a cell, so
    no decal is cut at a straight edge. A crack darkens the grain
    (`CRACK_DARKEST`, 0.2), blends its normal toward the decal's and fades out
    between 0.08 and 0.25 m per pixel and toward the road's edge.
  - *Normals:* the maps' green runs up the texture, against the rows (+z), so
    the shader flips it; a decal's tilt is also turned back by its angle into
    world XZ.
  - *Relief:* under 0.02 m per pixel (`RELIEF_NEAR`: only the closest zoom
    levels) a 12-step ray march through the grain (1 cm) and crack (3 cm) depths shifts
    where the maps are read, so stones and cracks show depth; farther out one
    step on the grain height.
  - *The Zone's old road:* from `OLD_ROAD_FROM` (20 m) inside the Perimeter
    (x < `PERIMETER_X` - 20) the asphalt degrades over 15 m into the retired
    road kit (`OLD_ROAD_FULL`, 35 m), only on `Surface::Asphalt` legs and only
    as a texture. The kit (18 scanned slabs, once `old_road.rs`) is baked top
    down by `tools/bake_road_kit.py` into one road-aligned strip,
    17.87 m across and 60.28 m along (`OLD_ROAD_WIDTH`, `OLD_ROAD_PERIOD`),
    at its native 61 px/m (1092 x 3684 px, the ends blended so copies tile):
    `assets/textures/terrain/old_road_{albedo,normal,height}.ktx2` (BC7 sRGB,
    BC5, BC4; bindings 117-119). Gaps between the slabs (albedo alpha 0, a
    fifth of the strip) are potholes: they show the bare ground, shaded down
    toward their floor.
    - *Frame:* `RoadNetwork::leg_frame` gives the shader an R16Uint image on
      the mask grid naming the nearest asphalt leg (binding 120) and a table
      of each leg's start, direction, length and distance along its route
      (binding 121). The shader takes that leg or the route neighbour on whose
      side of the bend's bisector the point lies, so the strip runs on
      unbroken along a route; within `OLD_ROAD_BEND` (1.5 m) either side of
      the bisector, measured across it rather than from the legs (past a
      bend's vertex both legs are equally far), the two frames blend.
    - *Wear:* in the band a two-octave noise (`OLD_ROAD_PATCH`, 3 m) lets kit
      patches in where it lies under the wear, low spots of the kit (potholes,
      cracks) first, the slabs last; today's crack decals fade out as the kit
      comes in. The band's ends and the strip's size reach the shader as one
      uniform (binding 122).
    - *Close up:* under 16 mm per pixel (`OLD_ROAD_TEXEL`, the kit's own texel)
      today's photo grain modulates the kit's colour and normal, and under
      0.05 m per pixel (`OLD_ROAD_NEAR`) one parallax step through the kit's
      height (`OLD_ROAD_RELIEF`, 0.12 m of the bake's 1.16 m) shifts where it
      is read. Farther out the strip's mips alone.

    Rebake (headless Blender, about 2.5 min with OptiX; the straightened
    pieces from the unused-assets zip, their JPEG originals from the
    workspace; the PNGs go to `--sources`):

    ```bash
    "$BLENDER" -b --factory-startup --python-exit-code 1 --python tools/bake_road_kit.py -- \
      ~/Projects/agentic/RoadSide-Picnic/archived/roadside-picnic-unused-assets-2026-11-22.zip \
      ~/Projects/agentic/RoadSide-Picnic/models/road/uhd assets/textures/terrain \
      --sources ~/Projects/agentic/RoadSide-Picnic/textures/old-road
    ```

    It prints the strip's width and period; copy them into
    `OLD_ROAD_WIDTH` and `OLD_ROAD_PERIOD` in `terrain_material.rs`, which
    hands them to the shader. The printed depth is the height map's range;
    the shader's relief depth (`OLD_ROAD_RELIEF`) is set by hand. Its helpers are tested by
    `python3 -m pytest tools/test_bake_road_kit.py -q`.
- **Marsh props** (`src/world/marsh.rs`): dead trees, stumps, logs, fungus,
  moss, rocks, mounds and debris from the swamp kit (`assets/models/swamp/`)
  are scattered over the marsh; trees, stumps, logs and mounds may stand in
  shallow pools, the rest keep to the hummocks. Props too tall to step over
  carry a collider.
- **Rocks** (`src/world/rocks.rs`): seven big textured rocks
  (`assets/models/rocks/`, 2–17 m) stand on the shoulders of the country roads
  (`roads::country_roads`) and, in open country, as groups of crags with
  boulders around them, mostly on high ground. Each shows its `medium/` model
  near the view and a `low/` copy farther out, to a range that grows with its
  size. Their colliders (`world/pillars/rock_*.rs`) come from
  `tools/trim_tree_base.py ... --cell 0.2`; the stalker can climb them.
- **Guards** (`src/world/guards.rs`): UN soldiers
  (`assets/models/soldiers/medium/`, rigged by `tools/rig_stalker.py`;
  `--walk-arms` sets the walk clip's arm swing, `--armature` keeps a Tripo
  rig's own skeleton): two at each gate (the hangar yard's and the Institute's)
  pace a short beat beside the opening (the yard's whole gap, checkpoint and
  lane), and five sentries walk short beats beside the two checkpoints. Each idles watching at either end of its beat.
- **Townsfolk** (`src/world/townsfolk.rs`): fifteen New Harmont pedestrians
  (`assets/models/townsfolk/medium/`: a scientist, a factory worker and a
  construction worker, rigged like the guards; Idle is clip 1, Walk clip 4).
  Ten workers and a scientist walk the sidewalks round the town's four blocks,
  7 m out from each street's middle line (the lanes keep 2.5 m, the paving
  8.5 m), both ways round; four scientists walk from the plaza along the
  avenue's north sidewalk, across the ring street, up the town side of the
  Institute road and its drive to a door 6 m past the Institute's
  `entrance()`, where they vanish inside (hidden, collider removed) for
  30–90 s and then walk back. Paces are 1.2–1.4 m/s with the walk clip at
  pace / 1.8; at about a third of the corners they stand 2–8 s. They stop
  while the stalker or anything described stands within 1.2 m ahead, and
  wait at the ring street's kerb until no vehicle (anything described wider
  than 1.5 m, on the ground) is within 90 m. Routes and start points are
  fixed tables; meshes fade out 150 m beyond the strategy reach, like the
  guards'. Asset rebuild: `tools/rig_stalker.py`'s docstring (the scans are
  simplified at a sub-millimetre error to ~100k triangles, 4096 px textures
  kept; the scientist's gesturing arms get `--joints` and `--hang-arms`).
  Traffic does not yet stop for pedestrians.
- **UN patrol** (`src/player/vehicle/patrol.rs`): four white armored cars
  (`UN_VEHICLE`, the stalkers' car repainted with a UN marking in its roof
  atlas; the flank plates are separate quads with `textures/decals/un_flank.png`)
  lap a loop beside the Perimeter wall on Harmont's side, a quarter lap apart,
  from 50 m south of the hangar to beyond the Institute
  (`cordon::PATROL_TURNS`), and halt while the stalker, their car or another
  patrol is in their way.
- **Harmont traffic** (`src/player/vehicle/traffic.rs`): a street sweeper up
  and down New Harmont's plaza avenue, two sedans and the fire engine (7 m,
  7 m/s) round the town's ring street, and two sedans, the ambulance (4.4 m,
  8.5 m/s) and a black panel van (4.8 m, 7.5 m/s) between the Institute and the
  Borscht (`roads::traffic_routes`), a quarter or half lap apart. They keep
  right, turn round at dead ends, slow for bends and stop for the stalker,
  their car, a patrol or each other; faster cars queue behind slower ones. The
  models are Tripo scans split into a chassis and four wheel nodes by
  `tools/split_vehicle_wheels.py` (its docstring has the commands); the wheels
  roll and the front pair steers. The ambulance, fire engine and van come out of
  one scan that fused them with a fourth car (a militia sedan, not used):
  `tools/extract_fused_vehicles.py` gives each welded shell to the nearest
  vehicle's footprint box, so where they touched each part stays whole and no
  hole is cut; they keep every source triangle (3,985 / 6,115 / 3,870) and
  the native 4096² atlas. Every vehicle's UVs span the whole atlas, so the
  three share it: it is one external file,
  `models/vehicles/medium/textures/vintage_emergency_vehicles_3d_model_basecolor.ktx2`
  (`TRAFFIC_EMERGENCY_ATLAS`, BC7, ~22 MB of VRAM once), which each GLB names
  by URI (`compress_glb_textures.py --external-images`, the three in one run)
  and the asset server loads once. The ambulance's two roof beacons and
  the fire engine's one flash in turn (emissive spheres, 30,000 nits, 1.5 Hz).
- **Institute carrier** (`src/player/vehicle/carrier.rs`): the International
  Institute's tracked carrier (`INSTITUTE_CARRIER`, 7 m long) waits 150 s in
  its bay on the hangar's east side (hangar-local (3.2, -12), clear of the
  central aisle, the exhibit rows and the stalkers' car), then drives out of
  the door, through the yard's boom and the Institute gate (its
  `InstitutePass` lifts both) and down the highway about 300 m into the Zone
  (`roads::expedition_route`), keeping 3 m right of the road's middle. It
  waits 90 s, pivots on its tracks (0.35 rad/s, the two sides counter-rolling),
  drives home, pivots on the apron and backs into its bay at 1.5 m/s. It
  keeps to 3 m/s within 75 m of the hangar and on bends and cruises at 7 m/s;
  it stops for the stalker, their car, a patrol or traffic, and they stop for
  it. On the hall's floor it stands level; outside it settles on the rendered
  ground by its first and last road wheels. Road wheels, return rollers, sprocket
  and idler are `WheelL*` /
  `WheelR*` nodes cut by `tools/split_moving_parts.py` (its docstring has the
  commands); the split model is slimmed with `tools/slim_dense_model.py`
  (1.8M to 288k triangles with the belts cut out, maps capped at 2048 px; see
  Perimeter guns) and finished with `tools/compress_glb_textures.py`.
  The belts run with the wheels: the script cuts each belt out of `Hull` and keeps
  one scanned link per side (`TrackLinkL`, `TrackLinkR`, 758 and 809 triangles) plus
  each belt's mid-line loop, mid-plane x, link count (77 a side) and the roll
  radii of its sprocket and idler, written to
  `src/player/vehicle/carrier_track.rs` by `--track-out`. At spawn the carrier
  hides those two template nodes and spawns that many copies of each beside
  them (`CarrierLink`, about 121k triangles in all, drawn instanced and casting
  shadows). `run_carrier_tracks` places every copy from its two pins on the loop
  (`track_loop.rs`), shifted by the same `Carrier::tracks` the wheels roll with
  (meters over `SCALE`), after both `drive_carrier` and `steer_carrier`, so belts
  and wheels turn together: backwards when it backs up, against each other as it
  pivots. The road wheels roll at their rim radius (0.049, the return rollers
  0.019), but the belt wraps the sprocket (front) and idler (rear) and turns
  rigidly with them, so those two roll at the belt's wrap radius, the mean
  distance from the hub to the loop over the 120 degrees straight out from it
  (`track_loop.wrap_radius`): 0.0471 and 0.0540 on the left, 0.0451 and 0.0537
  on the right (model units), not the 0.044 both rolled at before, so the links
  do not slide on them. The lower run travels toward -Z going forward; links nod up to
  about 4 degrees where the scanned belt dips under each road wheel. Change the
  loops and radii by regenerating `carrier_track.rs`, never by hand.
  The stalker can take it with E whenever it stands still (in its bay, waiting
  out in the Zone, or where they left it): its expedition stops for the rest of
  the run and it drives on its tracks (W/S up to 10 m/s ahead and 3 m/s astern
  at 3 m/s², braking at 8 m/s²; A/D turn it at 0.6 rad/s, on the spot when it
  stands, the other way backing up), or to a clicked point by the car's
  autopilot, along routes planned for its own size. It weighs 18 t (`Crusher`, footprint 3.6 × 7 m), so it flattens
  plants and knocks down what is lighter. Off road it does more than the car:
  it fords swamp pools up to 1.5 m deep (the car 0.6 m), climbs grades up to
  1.0 (45°), and rides over rocks, logs and other solids up to 1 m high, each
  quarter of its tracks resting on the highest one under it (`Footing`);
  taller ones, more than 0.5 m above its tracks' line under the hull, stop it,
  though it can still turn in place. As with the car, a step whose turn would
  swing a corner into a solid goes straight instead, if that is free, so nosed
  against a rock it still backs out at full lock. Deep water and climbs slow it, to 40% at
  its fording depth or steepest grade. Four lamps (two headlamps on the upper
  glacis, 50 m; two shadow-casting floods on the cab roof, 180 m) burn while
  it is out on an expedition or driven, as the car's do while driven
  (`Lamp`, `mount_spotlights`). Its engine plays the car's engine sound. E puts the stalker down
  beside it (`Seat`, shared with the car). Once taken it is saved where they
  left it (a `carrier x z yaw` line in the save) and stands there again on the
  next start; saved at its wheel, they come back beside it.
- **Institute helicopters** (`src/world/helicopters.rs`): two helicopters
  (`INSTITUTE_HELICOPTER`, `MILITARY_HELICOPTER`) rest on pads on the
  Institute roof's two wings (`sites::institute_helipads`, model units
  (±0.255, 0.70, -0.10)). Each spins its rotor up over 8 s, climbs at 4 m/s to
  two storeys (7 m) over the Institute's top (`sites::institute_top`), and
  flies three passes back and forth across the Zone between 120 m and 950 m
  west of the Perimeter, 160 m apart, at 32 m/s, turning at 0.3 rad/s; each
  sortie picks a different stretch (`lattice_hash` of the sortie number). It
  stays at least 25 m over the ground, looking 10 s ahead, banks up to 0.35 rad
  into turns and dips its nose with speed. Home, it slows over its pad, slides
  in, sets down at up to 2.5 m/s, spins down and rests 120 s; the second starts
  90 s after the first. The `Rotor` node spins about +Y and `TailRotor` about
  +X (`tools/split_moving_parts.py`); both are slimmed with
  `tools/slim_dense_model.py` (the Institute's from 2M to 295k triangles, maps
  capped at 2048 px) and finished with `tools/compress_glb_textures.py`.
- **Perimeter guns** (`src/world/turrets.rs`, `turrets/{scan,fire,beam}.rs`): twelve
  automatic guns stand on top of the Perimeter wall (`cordon::perimeter_top`,
  about 9 m up), alternating a gatling and a naval mount, evenly spread
  (about 78 m apart) from `REACH_PAST` (50 map tiles, 120 m) south of the
  hangar, z ≈ -233, to as far past the Institute, z ≈ 630 (`guarded_span`).
  The one that would land in the Institute gate steps to `GATE_CLEAR` (12 m)
  from its middle. Each has a `Collider`, is `Unclimbable` and describes itself
  (`Place::Turret`). The full model shows within 150 m of the camera, the light
  `low/` one out to 700 m, as do the beam and spot. A gun and its effects
  (beam, spot and muzzle flash, under one holder) are `OnWall`: they hide
  while the wall under them is not drawn.
  *Scan:* the head sweeps ±60° about the Zone's heading (-X) with a 26 s
  period while the beam's reach nods between 60 and 400 m with a 37 s period,
  a Lissajous scan from a per-gun phase. The beam is a military laser's 532 nm
  green (`LASER`), blended with 30% of the scene showing through
  (`LASER_SHOW_THROUGH`): radius 3.5 cm, `BEAM_GLOW` 5,800 (about 0.6 after the
  EV 13 exposure, as bright to the eye as the old red at 18,000), from the
  `Muzzle` down to where it meets the ground, one terrain sample per gun per
  frame, with a 0.28 m spot there (`SPOT_GLOW` 19,000). The beam's material
  (`turrets/beam.rs`, `shaders/laser_beam.wesl`) keeps it at least 1.5 px wide
  on screen (`LEAST_PIXELS`), fading it by the share of that width it really
  covers, and softens its edge, so a far beam reads as a faint smooth line
  rather than broken dashes.
  *Detection is the beam:* a body is caught within 1 m (plus its radius) of the
  beam's ground track, between the muzzle and the end (swept since the last
  frame, so a fast sweep does not skip it), where the beam passes between its
  feet and its height: the stalker 1.8 m standing, 0.5 m crawling or prone, a
  creature 0.6 × its size. From 10 m up the beam clears a standing stalker
  until the last ~17% of its reach and a crawler until the last ~5%.
  *Fog:* a body standing in the ground fog (`ground_fog::in_ground_fog`: the
  fog tendency, the weather's or the night's, at least 0.35, on ground low
  enough for the fog to settle half, below about 32 m) is seen on only 60% of
  the beam's passes over it (`SEEN_IN_FOG`): one roll per pass, a miss
  remembered until the beam leaves the body. A locked target walking into
  the fog is kept. The rule follows the fog pass's inputs, not its drifting
  banks and clear patches, which live only on the GPU.
  *Targets:* ground creatures (wildlife and mutants, not birds) in the Zone,
  and the stalker only as a `Trespasser`: crossing from Harmont into the Zone
  on foot within 10 m of the crawl hole marks them; the Institute gate, the
  car (`Driving`) or a restored save do not; Harmont's side or a `Respawn`
  clears it. Cars, the carrier, patrols and anyone in Harmont are never targets.
  *Engagement:* a locked head turns after its target (gatling 1.4 rad/s, naval
  0.9), the beam ending at its feet; the barrels spin up over 1 s, then, aimed
  within 3°, fire 0.6 s bursts every 0.95 s: pooled tracers (64 in all, 2.6 m
  streaks; gatling 14/s at 450 m/s, spread 0.6°; naval 6/s at 380 m/s, 1.6×
  bigger), a flickering muzzle flash with a 14 m point light, and a positional
  burst sound (`GunSound`, `src/audio/guns.rs`, heard to 650 m). After 0.3 s of
  fire on a creature (0.5 s on the stalker) plus the rounds' flight time, a
  creature is `Downed` (`wildlife/downed.rs`: it stops, tips onto its side away
  from the gun over 0.6 s, lies 8 s, sinks over 2.5 s and returns to the start
  of its route 45 s after the hit) and the stalker respawns. A gun lets go of a
  target that leaves the Zone, falls, goes beyond 450 m or 75° off the Zone's
  heading; the barrels spin down over 1.6 s and the scan resumes from where the
  gun points.
  *Assets:* `assets/models/props/{medium,low}/{gatling,naval}_turret.glb`
  (`GATLING_TURRET`, `NAVAL_TURRET` and the `_FAR` tiers): glTF +Y up,
  barrels along +Z at rest, about one unit long (scaled 3.5, to read from the tactical camera), base at y = 0 and
  centred on the yaw axis; top-level nodes `Base` (static) and `Head` (yaws
  about its local +Y), under it `Barrels` (spin about local +Z) and an empty
  `Muzzle` at the barrel tips, whose rest position the game reads. They are
  split into these nodes by `tools/split_moving_parts.py` (its docstring has
  the commands). The near tier is then slimmed with `tools/slim_dense_model.py`
  (1.9M to about 290k triangles, maps capped at 2048 px): gltfpack simplifies
  within 0.1 % of the extent while weighing the normals, since glTF Transform's
  position-only simplify crumples the shading of these Tripo panels. Each
  Tripo model at full density cost about 150 MB of VRAM, enough with the
  townsfolk to exhaust a 6 GB card at 2560x1440. Finish with
  `tools/compress_glb_textures.py`.
- **Wildlife** (`src/world/wildlife.rs`): the optimized versions of the
  textured sources in `assets/models/{animals,birds}/medium/` share mesh handles
  and preserve the original PBR maps. Rebuild with glTF Transform `simplify
  --ratio 0.0015 --error 0.009`, `resize --width 1024 --height 1024`, and
  `tangents` (all with `--vertex-layout separate`), then ground and repair
  tangent seams with `tools/prepare_plant_asset.py`.
  `src/world/wildlife/rig.rs` skins the static meshes with shared per-species
  rig weights measured in +Z-forward model space. Walking and running feet
  follow the terrain, and flyers flap or glide. Near the hangar, the raven
  lands, walks, and takes off again while the frostwolf occasionally chases
  and lunges at the hexadeer. The chameleon stays on its modeled perch.
  Every named site and landmark receives a deterministic local fauna route:
  ground creatures, mutants, and flying birds rotate by site index in the Zone;
  Harmont's sites only get birds. Walkers
  stay outside the site's pad and road. Each base flight route fans into a
  five-bird flock with staggered phase, radius, and altitude while preserving
  landing routes and terrain clearance. A herd of ground animals
  (`perimeter_herd`: hexadeer, frostwolves and chameleons, two every 150 m)
  grazes 100 to 400 m west of the Perimeter along the guns' stretch, shown
  from 450 m (others 100 m) so they can be seen from the wall; the Perimeter
  guns shoot them, and a `Downed` creature stops moving and posing until it
  returns. Wildlife is otherwise decorative and has no collision.
- **Artifact showcase** (`src/world/base/artifacts.rs`): all 14 supplied
  artifacts sit on individual lit pedestals in two seven-table rows inside the
  hangar. The west row stays outside the aisle and the east row hugs the wall,
  leaving the armored vehicle's diagonal bay route clear. Every artifact keeps
  a stable rest transform and idly spins, bobs, and sways; display-table
  colliders remain static. When the player approaches a table, the HUD resolves
  its `ArtifactDisplay` index through `ui::text::ARTIFACT_COPY` and shows the
  localized name and lore; the readout hides away from range and while paused.
  The golden sphere retains its textured gold albedo
  but receives a fully metallic, low-roughness runtime material so its surface
  reflects the hangar light probe and spotlights. Artifact runtime GLBs keep the
  original geometry, 4K texture maps, and source PBR/normal materials. Centered
  sources are grounded by `tools/ground_full_asset.py`; already-grounded dense
  PBR scans are copied byte-for-byte. Every display has a 1536 px preview in
  `assets/models/artifacts/medium/`; within 8 m only the nearest preview is hidden
  and its `assets::ARTIFACTS` scene is streamed in. `ARTIFACTS` names the same
  `medium/` files for now; the untouched 4K PBR scans are in the asset archive
  (see Asset tiers). Do not replace the primary
  showcase artifact with an LOD. Bevy's glTF loader generates MikkTSpace
  tangents when a normal map needs them.
- **Mutants** (`src/world/wildlife/mutants.rs`): bovines, chimeras, fleshdeer,
  and mutts occupy dry, biome-weighted patrol pockets across the Zone. Bovines
  give the stalker room; fleshdeer investigate up close; mutts and chimeras
  pursue on foot, brace before lunging, and retreat to their patrols. Site-local
  mutant patrols use the same pad, water, road, spacing, and leash checks. A landed
  strike respawns the stalker. Mutants keep to the Zone: Harmont, the cordoned
  hangar yard and the occupied vehicle are safe.
  The original textured models use the shared wildlife rig, with species-specific
  foot and head weights. Rebuild the GLBs in `assets/models/mutants/medium/` with the
  wildlife pipeline above, using `simplify --ratio 0.003 --error 0.009`.
- **Base** (`src/world/base.rs`): the imported steel hangar
  (`assets/models/hangar/medium/hangar.glb`) sits on a levelled pad at `terrain::BASE`.
  The north (+Z) door stays open for the vehicle, and runs and respawns start at
  `SPAWN` inside. Rebuild the runtime GLB with glTF Transform `simplify --ratio
  0.045 --error 0.0014`, then `resize --width 2048 --height 2048` (use
  `--vertex-layout separate` for both), bake `tangents`, then run
  `tools/prepare_hangar_asset.py` to level the slab and apron. If the geometry
  changes, remeasure the walls and pendant mounts in `base.rs`. The runtime
  GLB is the arched hall shell, one primitive about 0.9 × 0.58 × 1.0 units
  (×38 m); the hangar's `parts/` kit is a different model, so never save it
  over `medium/hangar.glb`. The shell's material (`base/cutaway.rs`,
  `shaders/cutaway.wesl`) repaints it as an industrial hall: steel floor
  plates (`textures/hangar/`, cut from the reference board by
  `tools/prepare_hangar_floor.py`) with yellow walkway lines beside the
  exhibit rows, the parked car's bay, a trench grate and hazard band at the
  door, and steel-grey walls with a hazard band at their foot. The walkway
  lines are shader constants: move them with the exhibit rows. A stronger
  local light probe and broad warm pendants keep the interior readable without
  changing outdoor lighting. Prop and artifact spotlights add focused task and
  exhibit light on top of that soft fill.
  The workshop and tool rack, jack, map board, lab bench, table, and toilet
  use the hangar-local poses and collision in `src/world/base/props.rs`.
  Each prop also has a short-range warm spotlight without a shadow map.
  Rebuild their GLBs in `assets/models/props/medium/` with glTF Transform `simplify
  --ratio 0.012 --error 0.0035`, `resize --width 1024 --height 1024`, and
  `tangents` (all with `--vertex-layout separate`), then ground them and repair
  tangent seams with `tools/prepare_plant_asset.py`. Keep the central aisle
  and the parked vehicle's path through the bay clear. The crawler, quarry
  vehicle, sedan, and freight train are grounded in a separated line behind
  the hangar's south wall, inside the level cordon yard and off the front gate
  route. Each yard vehicle has static collision sampled from its prepared GLB.
- **Armored vehicle** (`src/player/vehicle.rs`): parked in the hangar, driveable with
  independent wheel pivots and two lamps that turn on while occupied: a dipped
  beam at the grille and a roof searchlight that throws a longer, wider spot.
  The chassis and suspension sample the rendered terrain under the wheels.
  Forward speed rises from 16 m/s off-road to 24 m/s on the center of a graded
  road, then blends back down near the shoulder and retains the ford water cap.
  Throttle and steering ease in, steering gets gentler at speed, and the chase
  camera damps vehicle movement and zoom without delaying mouse orbit.
  When throttle is held while blocked, the tires spin without moving the hull.
  A step whose turn would swing a corner into a solid or up a bank goes
  straight instead, if that is free, so a car nosed against a rock can still
  back out at full lock.
  Slopes beyond tire grip cause downhill sliding; the car and stalker fall at 9.81 m/s².
  E lets the stalker out on the first clear door spot (left, right, back, front,
  0.9 m from the hull) within 1.2 m of the hull's height. On a bank too steep
  for any (`exit_position`), they bail out onto the roof (`roof_spot`: over the
  middle of the footprint, on the car collider's highest pillar there), so the
  stuck car stays a shelter: E there gets them back in, since entry
  (`Seat::distance`) reaches from 1.2 m below the hull up to 0.6 m above its
  roof, within 1.65 m of its footprint. With the roof blocked too, they step
  out on the clear door spot nearest the hull's height, so a car stuck there
  never traps its driver. Only solids or the map's edge on all four sides and
  the roof keep the stalker in: the roof has no water check, so a car stalled
  in a deep ford lets them out onto its roof. Tests:
  `a_car_stuck_on_a_steep_bank_still_lets_the_stalker_out`,
  `a_stalker_bailing_out_on_a_steep_bank_can_climb_back_in`,
  `a_stalker_on_the_roof_can_get_in`.
  `tools/prepare_vehicle_asset.py` bakes the GLB's 3072 × 2048 PBR atlases from
  four texture sheets, with matte paint, dark trim, louver and lamp details,
  and subtle procedural wear.
  The vehicle uses mipmaps and anisotropic filtering to keep angled armor sharp.
  Regenerate `src/world/pillars/armored_vehicle.rs` with `tools/trim_tree_base.py`
  after mesh changes.
- **Collision** (`src/world/collision.rs`): every solid model carries a `Collider`, a
  set of upright `Pillar`s posed by its `GlobalTransform`; `push_out` moves bodies out
  of them. Pillars under `STEP_HEIGHT` are stepped over, ones above the body are passed
  under (crawling body is 0.5 m). Give every new solid model a `Collider`.
  A collider with a `Weight` (trees by size, the marsh props) gives way to a
  heavier `Crusher` (`world/crush.rs`): the stalker (90 kg), their car and the
  patrol (12 t) run through it and knock it down. Trees fall through
  `TreeBreak`, slender props topple and squat ones sink; none return during the
  run. Crushers also trample the plants under their footprint (`Trampled`
  keeps them gone when tiles re-stream). The stalker climbs a solid they press
  into (`climb_top`; `Gait::Climb` plays the crawl clip upright) and stands on
  its pillars' tops (`support_height`). `Unclimbable` solids (cordon walls and
  checkpoints, trees, guards, vegetation clusters, marsh trees) fence bodies
  out at any height (`fence_out`).
- **Audio** (`src/audio/`) is synthesized at startup: looping Zone ambience,
  gait-dependent footsteps, jump and landing, vehicle doors, and a speed-driven
  engine. Wildlife calls, footfalls, takeoff/landing, and fight cues originate
  from the creature's world position; nearby sounds pan with the camera and fade
  with distance. Footfalls follow accepted travel and attack sounds follow
  chase, lunge, and hit events. Each Perimeter gun burst (`GunSound`) plays a
  synthesized gatling or naval burst at the muzzle (`audio/guns.rs`), full
  within 90 m and fading out by 650 m. N mutes both positional and non-positional audio.
  Playback follows the system's default audio output.
- **Stalker model** (`STALKER_MODEL`: `assets/models/soldier/uhd/soldier_rigged.glb`)
  is the UHD Tripo soldier scan (~2M triangles, 4K normal-mapped PBR) rigged in
  headless Blender by `tools/rig_stalker.py` (its docstring has the soldier command).
  It binds in the scan's own arms-down pose. Its arms hang against the body, where
  distance weights would give the torso's sides to the arms, so bone heat runs on the
  `medium/` scan of the same soldier and `--weights-from` copies those weights onto
  the full mesh. The script also bakes the Crawl/Idle/Jump/Run/Walk clips (glTF
  animation indices 0–4, sorted by name). `src/player/animation.rs` cross-fades them
  by `Gait` and pitches the body face-down while crawling (the Crawl clip is posed
  standing); the splash poses the same rig. The older trench-coat stalker
  (`models/stalker/uhd/stalker_rigged.glb`, T-pose bind with `--coat` bones that
  `swing_coat` drives) is no longer loaded; it and the soldier's unrigged sources
  are in the asset archive (see Asset tiers). Edit motion in the script and regenerate
  rather than hand-editing the GLB. Preview clips with
  `~/Projects/agentic/agentic-tooling/media/glb-preview`.
- **Sky & lighting** (`src/world/mod.rs`, `weather.rs`) use Bevy's physical `Atmosphere` with
  a `SunDisk` lit at `lux::RAW_SUNLIGHT` and a moon at `MOON_LUX`; the camera runs HDR at
  `Exposure { ev100: 13.0 }` by day, eased toward 9.0 at night by `light_the_sky`. Tune light
  and emissive values in physical units for the daytime exposure, and steer the look with the
  clock, `HAZE` and the weather rather than adding ambient light.
- **Precompiled textures**: every GLB the game loads embeds GPU block-compressed
  KTX2 textures with full mip chains (sRGB BC7 color, BC5 normals, linear BC7
  data maps), loaded through Bevy's `ktx2` + `zstd_rust` features. Finish every
  new or re-exported runtime GLB with
  `python3 tools/compress_glb_textures.py <glb>...` (needs `cargo install ctt-cli`).
  GLBs that share one texture (the emergency vehicles' atlas) instead write it
  once beside them with `--external-images <dir>` and refer to it by URI; give
  its path a constant in `assets/mod.rs` and preload it with the settings the
  glTF loader would use (`load_vehicle_atlas`). Run it last: Blender and glTF Transform cannot decode or edit the converted
  textures' pixels, so re-run earlier pipeline steps from the git/source GLB
  (the tangent bake below is the one step allowed after compression). Runtime CPU mipmaps
  (`terrain_material::mipmap_filtered_textures`) only cover procedural images
  and the small tiling terrain PNGs. Normal-mapped runtime GLBs also carry baked
  MikkTSpace tangents, so Bevy does not generate them at load (5–6 s for a
  1M-vertex scan): after a re-export, bake them with
  `python3 ~/Projects/agentic/agentic-tooling/media/glb-bake-tangents/glb_bake_tangents.py <glb>...`
  (it works on the compressed file and keeps its KTX2 images byte-identical).
  The standalone maps that are not in a GLB (the four asphalt maps,
  `assets/textures/terrain/asphalt_*.ktx2`, from `tools/prepare_asphalt_textures.py`,
  and the wall's depth map, `assets/textures/cordon/wall_depth.ktx2`, from
  `tools/bake_cordon_wall.py`) are encoded with the same `ctt` presets: sRGB BC7 for
  the colour, linear BC7 for the data maps and BC4 for the one-channel wall depth
  (`ctt_arguments("depth")`), full mip chains; their PNG sources stay in the
  assets workspace.
- **RAM and GPU** (`src/assets/ram_source.rs`, `src/assets/gpu_scans.rs`,
  `gpu_scans/{vram,tier}.rs`, `src/launch.rs`):
  behind the splash the `ram://` asset source reads every streamed UHD scan
  (the 40 in `world::uhd_files`: the three trees, the plants, then the sites
  nearest the hangar first; on exceptional GPUs the Golden Sphere's scan last)
  into RAM, four files at a time, while 4 GiB of
  RAM stay free (`RAM_HEADROOM`: the game grows up to 3.2 GiB after the fill). A file that does not fit stays on disk and
  loads from `assets/` through the same source; the splash waits for the fill.
  Every streamed UHD scan loads through `assets::uhd(path)` (`ram://`
  natively, `assets/` on the web) and only through `GpuScans`, never straight
  from the `AssetServer`; the always-shown UHD models (the stalker,
  `STALKER_MODEL`, and the checkpoints, `SITE_CHECKPOINT`) are preloaded from
  `assets/` instead. Callers `want` a scan each frame from `UPLOAD_LEAD` (40 m)
  short of where it shows, passing how far the anchor is from where it would
  show (0 or less: it shows now), the three tree scans are `pin`ned, and
  `scene()` returns the scan only once it is resident; a caller despawns its
  instance when the scan leaves, since the VRAM is freed only once nothing
  holds the scene. `GpuScans::plan` loads pinned scans first, then the
  nearest wants, then prefills spare room in fill order while
  `PREFILL_HEADROOM_GB` (0.5) stays free; behind the entry curtain, while the
  world's own uploads still land, every admission leaves that headroom.
  Scans load render-only (`RenderAssetUsages::RENDER_WORLD`), are measured in
  the frame their load finishes, and the render world reports over
  `ScanResidency` once every mesh and image of a scan is on the GPU. The
  budget follows the driver's live reading (`VK_EXT_memory_budget`, each
  second, `gpu_scans/vram.rs`): the game's driver budget, capped by `--vram`,
  less `MARGIN_GB` (0.5) and the most the rest of the game (all but the
  scans) has used this run. A dropped scan's bytes count as scan memory, not
  the rest of the game, until a reading shows them freed or reused (wgpu
  keeps freed memory in blocks until they empty; `main.rs` asks for 8-64 MiB
  blocks, `MemoryHints::MemoryUsage`, as the default 256 MiB ones left ~1.2 GB
  of the 6 GB card reserved but unused, which is what ran it out of memory;
  the same night drive uses ~0.43 GB less VRAM at the same frame rate);
  after `FREE_WAIT_SECONDS` (30) they
  stop holding room for new scans, but a wanted scan that finds no other room
  may still load into them, one at a time within the live budget: the reading
  after it lands frees them if it reused them, or else no scan tries again
  for 30 s (its overrun is one scan, inside the margin). A scan stays two minutes after it last
  showed (`KEEP_SECONDS`); a wanted scan short of room drops scans not shown
  for that long: those nobody wants, least recently shown first, then those
  wanted further from showing than it, furthest first (never one showing
  now, nor a pin). When the resident scans outgrow a shrunken budget the
  least recently shown unwanted one leaves, unpinned before pinned
  (`UHD scan … left the GPU` in the log). Where the driver cannot report
  VRAM, the budget is `--vram` (6 GB without it) less `DESKTOP_GB` (0.6),
  `GAME_VRAM_GB` and the margin. If the GPU runs out of memory anyway (a
  spike between two readings), the game does not quit as Bevy would
  (`gpu_scans/recovery.rs`, the `RenderErrorHandler`): it drops every scan on
  or bound for the GPU, gives up `OOM_SQUEEZE_GB` (0.5) more of the scan
  budget for the rest of the run, drops the render world's per-frame textures
  (`TextureCache`, where the failed shadow map would otherwise be reused) and
  renders on; the scans come back as the smaller budget allows (`The GPU ran
  out of memory` in the log). A validation error within 2 s of one is its
  echo and recovers too. Any other render error, or a fourth recovery within a
  minute, quits as before, so a lasting error cannot make the screen flash. A
  full device recovery is not an option: the terrain, roads and scenery
  meshes are render-only and could not be uploaded again. Measured on the 6 GB RTX 2060 (with five tree species): peak RSS
  9.3–11.7 GiB, the fill 3.3 GB in 10–14 s warm or cold, 1.4–2.1 GB of scans
  on the GPU. All 42 scans of then would take about 4.4 GiB on the GPU, so every one
  would stay from a driver budget of about 7.4 GiB (a 10 GB card); that is an
  estimate (14 of the 24 site scans measured, the rest taken at ~107 MiB,
  plus 2.4 GiB for the rest of the game and the 0.5 GiB margin), not run on
  a larger card.
  *GPU tier* (`GpuTier`, `gpu_scans/tier.rs`): effects that scale with the GPU
  read one level, `Modest`, `Strong` or `Exceptional`, decided once at `Startup`
  (`settle_gpu_tier`, before the Zone spawns them) and fixed for the session.
  It follows the card's size as the driver reports it (the largest device-local
  Vulkan heap, in the same reading as the budget), not the live budget, which
  falls whenever another program holds VRAM and would take a 6 GB card for a
  4 GB one. Under 4.75 GiB is Modest (4 GB cards), 4.75 GiB and up Strong (a
  5 GB card as the driver reports it, 6 GB, and so on; drivers report a little
  under the size sold) and 15 GiB and up Exceptional (16 GB cards), or 5.75 GiB
  and up with hardware ray tracing (the adapter's ray queries: a 6 GB RTX 2060,
  RX 6000 or newer; a 6 GB GTX 1660 stays Strong). The 2060 holds 42 FPS at
  the Golden Sphere on Exceptional (Strong: 38). Ray queries are only a
  heuristic for that class: slower cards report them too (an RTX 3050 6 GB, an
  RX 6500 XT, an Arc A380), so when they decide the tier the log says so and
  names `--gpu-tier strong`. `--vram`
  caps the size judged (`--vram 4` on a 16 GB card gives Modest). Where the
  driver reports no heap (the web, macOS), the fallback is judged from `--vram`,
  or 6 GB without it: Strong. An integrated GPU (or a CPU renderer, as wgpu's
  adapter reports it) is Modest whatever it reports, since its "device-local"
  heap is the system's RAM (15+ GiB on a 32 GB laptop). `--gpu-tier modest|strong|exceptional` forces the
  tier whatever the card, for testing (a missing or unknown value is warned
  about and ignored). The log says `GPU: effects at the Strong tier` at startup
  and `GPU: a N GB card, effects at the Strong tier` with the first reading. Only
  the Golden Sphere reads it so far: its pool's grid, its reflections and its
  UHD scan (see **Golden Sphere**); that scan is the one streamed scan wanted
  only on Exceptional GPUs. Tests: `the_tier_follows_the_cards_size_capped_by_vram`,
  `a_six_gb_rtx_card_runs_exceptional_effects`,
  `the_tier_is_decided_once`,
  `without_a_reading_the_fallback_settles_and_a_forced_tier_wins`,
  `an_integrated_gpu_stays_modest_unless_forced` (`tier.rs`), `a_gpu_tier_can_be_forced` (`launch.rs`).
  *Light clustering* (`src/render/mod.rs`): `RenderTuningPlugin` sets Bevy's
  `GlobalClusterSettings::gpu_clustering` to `None` in its `finish`, after
  `PbrPlugin`'s, so lamps are assigned to clusters on the CPU (0.2 ms a frame).
  Bevy 0.20's GPU clustering rebuilt each view's buffers on the CPU every frame
  at 30-44 ms a view when the GPU sat at low clocks. Measured on the RTX 2060
  in Xephyr: equal at full clocks (34 FPS at the crater junction on Modest,
  26-28 by the Sphere on Strong); when the driver holds the GPU at low clocks
  (P3/P5, about one run in three on main and on this build alike, with no
  thermal or power cap: the driver's own governor), 15-16 FPS instead of 8-11.
  "Prefer Maximum Performance" in the NVIDIA PowerMizer setting is the
  user-side remedy to try. Tests: `bevy_is_switched_to_cpu_light_clustering`,
  `without_a_renderer_the_switch_does_nothing`.
  *Final numbers* (RTX 2060 at 1440p, end of the gpu-optimal-usage branch):
  - The CPU's main app takes 10-13 ms a frame on every tier (p95 under 15 ms).
  - GPU time away from the Sphere is the same on all three tiers: 20-22 ms at
    the crater junction at full clocks and 22-24 ms on a drive (mixed clocks).
    That misses the 16.7 ms target but holds the 30 FPS floor. The main opaque
    pass and the sun's shadow cascades dominate.
  - At the Golden Sphere, on the real display: Strong 38 FPS, Exceptional 42.
  - With 4 GB (`--vram 4` and the card held to 3.7 GiB free) the game uses
    2.0 of a 3.4 GB budget, with no crash.
  - The numbers and their caveats are in the GPU findings
    (`~/Projects/agentic/roadside-picnic-overwatch/reviews/`).
- **Entering the Zone** (`src/ui/curtain.rs`): an "Entering the Zone" curtain
  covers the freshly spawned world until the render pipeline queue has stayed
  empty, every streamed plant has spawned (`world::PlantsGrowing`) and the
  UHD scans have settled on the GPU (`GpuScans::settled`: every pinned tree
  scan that fits resident, nothing loading or uploading) for a moment (at most
  12 s), so shaders compile and spawn and upload hitches happen behind it
  instead of as pop-in. NVIDIA's driver caches compiled pipelines on
  disk; Bevy 0.20 itself has no persistent pipeline cache.
