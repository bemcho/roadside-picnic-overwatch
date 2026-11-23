# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Load the Bevy skill first

For any game development, debugging, or playtesting request in this checkout, load the
`bevy-game-dev` skill as the first action if it is not already loaded. The checkout
itself is the trigger: a request like "the ground still looks flat" need not mention
Bevy, Roadside-Picnic, or the skill by name.

If the skill is missing from discovery, read
`~/Projects/agentic/agentic-tooling/skills/bevy-game-dev/SKILL.md` if available.
If neither is available, continue with the project guidance below.

## Project Overview

**Roadside-Picnic** is a sci-fi game built in Rust with the [Bevy](https://bevyengine.org) game engine.
The setting is inspired by the Strugatsky brothers' novel *Roadside Picnic*: a mysterious, dangerous
area called **the Zone**, full of deadly **anomalies**, coveted **artifacts**, and the **stalkers**
who risk their lives to enter it and smuggle artifacts out.

Themes: exploration, survival, anomaly physics, artifact hunting, atmospheric dread.

The upper-right in-game HUD shows the smoothed frame rate.

## Controls

- **Any key** — enter the Zone from the hangar splash once loading finishes
- **Left click** — move the stalker there (pathfinding around obstacles and deep
  water; long trips run). While driving, click to drive there.
- **Shift + left click** — add a waypoint after the orders already given; a ring
  marks each one until it is reached
- **WASD** — steer directly (camera-relative) and cancel a click order; hold **Shift** to run
- Walk into a rock, building or other solid to **climb** onto it; the cordon walls,
  checkpoints, trees and guards can't be climbed
- **Space** — jump
- **Ctrl** (hold) — crawl prone
- **E** — enter or exit the armored vehicle; **W/S** to accelerate/reverse, **A/D** to steer, **Space** to brake
- **Mouse wheel** — zoom from over the shoulder out to the whole map; pitch steepens with zoom
- **Right mouse drag** — rotate the camera
- **Arrow keys / middle drag / screen edges** — roam the camera freely
- **F** — toggle the free camera (hold the view where it is) and following the stalker
- **C** — find the stalker: the camera returns to following them from wherever it roams
- **Mouse pointer** — hover over a site, landmark, building, water, creature,
  the car or an exhibit for its description (unsurveyed places show only their kind)
- **T** (hold) — time-lapse the day/night cycle (40× speed)
- **Esc** — pause / resume
- **M** — open / close the Harmont map
- **N** — mute / unmute game audio
- **V** — lift / restore the fog of war (for testing)
- **F5** — save progress (it also saves every minute and on quitting)
- **Q** — quit
- Start with `cargo run --release -- --new` to ignore the save and begin afresh
- `cargo run --release -- --vram <GB>` caps the video memory the game uses
  (by default, what the driver reports free)

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
Point-and-click orders (`player/orders.rs`) use grid A* over collider pillars
and deep water (`player/path.rs`); the vehicle follows them with an autopilot
in `drive_vehicle`. Shift-clicks append legs pathfound from the last stop
(`MoveOrder::append`). Routes pass through the yard gate's boom, and a car
with an `InstitutePass` through the Institute gate's; each lifts as it arrives. A yellow ring (`ui/locator.rs`) lies on the ground around
the stalker, or the car they drive, at every zoom: its vertices follow the
rendered terrain (or the water when wading), it never shrinks under 34 px wide,
a relative depth bias keeps it over road slabs and stones, and it pulses while
the stalker stands still.

## Procedural content

- **Terrain** (`src/world/terrain.rs`) is a deterministic 3.44 km square built in
  128 m chunks: rolling hills, the Zone's ridge mountains, the quarry basin,
  marsh hollow and bald hill (`landform_height`). The graded road network
  (`src/world/roads.rs`) follows the Harmont map: Harmont's streets, the highway
  from the Perimeter gate past the factories and the Garage, the railway tracks,
  the Chapter 4 track through the breach, the marsh road and the quarry. Roads
  are graded over the sited land (pads and dioramas), then paved. Each route
  is dirt, old asphalt (`Surface::Asphalt`: the highway, Harmont's main road,
  the Institute and Plague roads) or New Harmont's smooth streets
  (`Surface::Street`); the road mask texture holds gravel (R), tire ruts (G)
  and asphalt (B), and `terrain.wesl` paints weathered asphalt from B.
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
  snow on high, gentler slopes.
- **Water** (`src/world/{hydrology,water}.rs`): the Harmont map has no river.
  The Zone's water is the marsh (`hydrology::SWAMP`): shallow pools between
  drier hummocks; the lake and the sea lie beyond the map's edges. Use
  `Terrain::water_depth_at` for traversal. Walk and drive through the
  shallows; deep water stops you.
- **Day/night and weather** (`src/world/weather.rs`): a 24-minute day (hold T to
  time-lapse) moves the sun and a full moon (a second, dim `DirectionalLight` with
  its own disk) by solar geometry; whichever is up casts shadows, and exposure
  eases from EV 13 by day to EV 9 at night. Each day rolls clear, humid, overcast
  or rain. Temperature follows the sun (coldest at 05:00); relative humidity
  comes from the day's dew point (Magnus), so it peaks late at night and at dawn,
  or stays high on humid and rainy days. The HUD shows time, temperature,
  humidity and sky.
- **Ground fog and clouds** (`src/world/ground_fog.rs`, `ground_fog/{field,render}.rs`,
  `assets/shaders/ground_fog.wesl`): a custom render pass (after the main pass)
  raymarches a thin layer that follows the terrain or open water. Its top sits
  between half the armored car's height and its roof (1.25–2.6 m), and visibility
  inside ranges 1–5 two-metre ground tiles, so a crawling stalker disappears while
  a standing one shows chest-up. The fog field texture, built from the distant
  terrain tiles while the landscape loads, stores the surface height and where
  fog may gather: over water and its banks (the marsh, the coast and the lake),
  in the quarry basin and in the three dense woods. Saturated air
  (`Weather::fog`) spreads it there; the rest of the map stays clear even in
  saturated air. Drifting banks fray its edges, and billows and wind break it up.
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
  in chunk batches. Every tree is an imported model: saplings, broadleaves, tall
  conifers, birches, and dead trees (`assets/models/trees/medium/`) grow around
  the hangar, in the named landmark groves and in three dense woods
  (`canopy::WOODS`); open country has no trees. Their GLBs use one
  ~500-triangle primitive each: simplified textured wood and, for living
  trees, cutout crown faces baked from the supplied models. Within 8 m of the
  camera a tree swaps to its full scan (`trees/uhd/`, ~1.9M triangles), a
  child entity that crossfades by `VisibilityRange` and sways with it.
  `stream_close_trees` wants a tree's scan from `GpuScans` as the camera nears
  (16 m from the tree's middle) and shows it once it is on the GPU (the five
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
  no collision or shadows. The rear and sides of the hangar form a dense
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
  only for a body carrying an `InstitutePass` (the stalkers' car, on Institute
  papers) and lowers once it has passed (`Gate::Institute`); a second
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
  follow the rendered terrain and stay above the water. Wall meshes and solid collision stream in nearby terrain tiles.
  Keep the staging area and driveway clear; the tree and bush shelterbelt is
  concentrated behind the hangar and along the sides.
  The wall repeats one panel thousands of times, so it keeps its medium model:
  simplify with glTF Transform `simplify --ratio 0.24 --error 0.016`, resize to
  1536 px, bake tangents with `--vertex-layout separate`, then ground with
  `tools/prepare_plant_asset.py`. Both checkpoints use the UHD scan; re-measure
  `src/world/pillars/uhd_checkpoint.rs` with `tools/sample_landmark_ground.py`.
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
  stand-in from `low/` and streams its full UHD scene in within 150 m of its
  footprint around any `StreamAnchor`. The Institute's Tripo scan came without
  UVs or textures: `tools/prepare_institute_asset.py` (headless Blender) paints
  it with one atlas from its concept image in the workspace (window bays on the
  front and back, precast panels on the sides and roof), then builds its
  stand-in as below. Build a stand-in from the JPEG source
  with glTF Transform `simplify --ratio 0.02 --error 0.002` and `resize --width
  1024 --height 1024`, then compress; never ground or recenter it.
- **New Harmont** (`src/world/town.rs`): buildings cut from the Harmont town
  scan (`assets/models/harmont/`) line a street grid around a paved downtown
  plaza on one level town pad; each shows its full scan near the view and a
  light version farther out.
- **Railway** (`src/world/railway.rs`): the old line from south to north
  (`map::RAILWAY`) is laid from the railway embankment scan
  (`assets/models/railway/`): sleeper ladders end to end along the graded bed,
  two rails on top and wreckage beside the line. `tools/align_kit_piece.py`
  squares up each kit piece (+Z along the line, grounded at y = 0).
- **Old roads** (`src/world/old_road.rs`): copies of the cracked-asphalt road
  module (`assets/models/road/`, reassembled into one straight frame by
  `tools/straighten_road_kit.py`) pave every `Surface::Asphalt` leg end to end,
  pitched with the graded road; the full scan shows within 40 m, the
  simplified `medium/` copy out to 900 m.
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
- **UN patrol** (`src/player/vehicle/patrol.rs`): four white armored cars
  (`UN_VEHICLE`, the stalkers' car repainted with a UN marking in its roof
  atlas; the flank plates are separate quads with `textures/decals/un_flank.png`)
  lap a loop beside the Perimeter wall on Harmont's side, a quarter lap apart,
  from 50 m south of the hangar to beyond the Institute
  (`cordon::PATROL_TURNS`), and halt while the stalker, their car or another
  patrol is in their way.
- **Harmont traffic** (`src/player/vehicle/traffic.rs`): a street sweeper up
  and down New Harmont's plaza avenue, two sedans round the town's ring street
  and two between the Institute and the Borscht (`roads::traffic_routes`). They
  keep right, turn round at dead ends, slow for bends and stop for the stalker,
  their car, a patrol or each other. Both models are Tripo scans split into a
  chassis and four wheel nodes by `tools/split_vehicle_wheels.py` (its
  docstring has the commands); the wheels roll and the front pair steers.
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
  landing routes and terrain clearance. Wildlife is otherwise decorative and
  has no collision.
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
  Slopes beyond tire grip cause downhill sliding; the car and stalker fall at 9.81 m/s².
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
  chase, lunge, and hit events. N mutes both positional and non-positional audio.
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
  Run it last: Blender and glTF Transform cannot read the converted textures, so
  re-run earlier pipeline steps from the git/source GLB. Runtime CPU mipmaps
  (`terrain_material::mipmap_filtered_textures`) only cover procedural images
  and the small tiling terrain PNGs. Normal-mapped runtime GLBs also carry baked
  MikkTSpace tangents, so Bevy does not generate them at load (5–6 s for a
  1M-vertex scan): after a re-export, bake them with
  `python3 ~/Projects/agentic/agentic-tooling/media/glb-bake-tangents/glb_bake_tangents.py <glb>...`
  (it works on the compressed file and keeps its KTX2 images byte-identical).
- **Entering the Zone** (`src/ui/curtain.rs`): an "Entering the Zone" curtain
  covers the freshly spawned world until the render pipeline queue has stayed
  empty and every streamed plant has spawned (`world::PlantsGrowing`) for a
  moment (at most 12 s), so shaders compile and spawn hitches happen behind
  it instead of as pop-in. NVIDIA's driver caches compiled pipelines on
  disk; Bevy 0.20 itself has no persistent pipeline cache.

## Build & Run Commands

```bash
# Development run (fast, unoptimized)
cargo run

# Release build (playable framerates — Bevy games should use release for real testing)
cargo run --release

# Type-check only (faster iteration than full builds)
cargo check

# Lint with project-wide defaults
cargo clippy -- -D warnings

# Format
cargo fmt

# Run tests
cargo test

# One module, or one exact test
cargo test world::base::artifacts -- --nocapture
cargo test world::base::artifacts::tests::idle_motion_bobs_and_turns_from_a_stable_rest_pose -- --exact --nocapture

# Format check (CI-style)
cargo fmt -- --check

# Validate a changed GLB
npx @gltf-transform/cli validate path/to/model.glb

# Offline meshlet preprocessing of dense UHD scans (slow, opt-in feature)
cargo run --release --example meshletize --features meshlet_processor -- in.glb out.meshlet_mesh

# Update Bevy + plugins to latest compatible versions
cargo update
```

Run everything from the repository root so Bevy resolves `assets/`.

> **Always prefer `cargo check` during iterative edits.** It is several times faster than a full build.
> It cannot prove runtime WESL compilation, GLB loading, HUD layout, audio output or terrain
> LOD seams: visual, shader, asset, animation and audio changes need a `cargo run --release` check.

### Hot reloading (development only)

Never enable these features in release or `wasm32` builds. Pass them on the command line rather than
adding them to the default features in `Cargo.toml`.

- **Assets:** the `file_watcher` feature makes the `AssetServer` watch `assets/` and reload textures,
  shaders, meshes, audio and scenes on save, with no restart. Run
  `cargo run --features bevy/file_watcher`. Only assets loaded from file paths reload; procedural
  terrain and synthesized audio are rebuilt only on restart.
- **Rust code:** the `hotpatching` feature patches edits made inside existing system bodies (logic,
  math, constants) into the running game. It uses Dioxus Subsecond via the Dioxus CLI:
  `dx serve --hotpatch --features bevy/hotpatching`. New types, component or resource changes, system
  signature changes and newly registered systems still need a normal rebuild.
- **Fast rebuilds:** add `bevy/dynamic_linking` (for example,
  `cargo run --features bevy/dynamic_linking,bevy/file_watcher`) to cut incremental compile times.

## Architecture Conventions

This project follows idiomatic Bevy ECS. Read these before adding code:

- **Systems are functions, not structs.** Prefer plain functions over implementing `System` trait.
- **Group systems into plugins** via `Plugin` impls (`WorldPlugin`, `ZonePlugin`, `PlayerPlugin`, etc.).
  Each plugin owns its related components, resources, and systems.
- **States:** use Bevy `States` for top-level flow (`AppState::Loading` → `AppState::InGame`)
  and `SubStates` for gameplay overlays (`InGamePhase::Running`/`Paused`, toggled with Esc).
  Never use boolean resources for state. Pausing must never despawn the Zone.
- **Spawn objects in `OnEnter` schedules** rather than `Startup`, and despawn them in `OnExit`.
  `Startup` only loads handles (`preload_game_assets`, the plugins' `preload_*`/`load_*`
  systems). Preload every asset an `OnEnter(AppState::InGame)` path needs, or the splash
  turns Ready before that model or material exists.
- **Ordering:** systems that write local transforms of rendered children run in `PostUpdate`
  before `TransformSystems::Propagate`; use `.chain()` or explicit ordering when one system
  consumes another's changes.
- **Define components in the plugin's module** as `#[derive(Component)]` structs, with `#[require(...)]`
  for required components instead of bundling.
- **Queries:** keep systems lean and precise — filter with `With`/`Without`, use `Query` data access
  over `Commands` mutations when possible.
- **Messages** for one-shot communication (Bevy 0.20: `#[derive(Message)]`, `add_message`,
  `MessageWriter`/`MessageReader`; e.g. `Respawn`, `CreatureSound`, `TreeBreak`). Avoid
  sending messages via resources. Scene-ready hooks are `On<WorldInstanceReady>` observers.
- **Assets:** runtime paths are constants in `src/assets/mod.rs`, loaded via `AssetServer`;
  startup handles are held in `PreloadedAssets`. `load_single_primitive` takes exactly mesh 0,
  primitive 0, material 0; use a `#Scene0` scene for multipart models.
- **Precalculate once.** Anything derivable from the assets or the fixed seeds is computed once — baked into the
  runtime files by the asset pipeline, or computed once and cached — never at every load, re-admission or frame
  (e.g. tangents are baked into normal-mapped GLBs). Where something must stay at runtime, say why in a comment.
- **Two kinds of artifact:** the hangar showcase (`world/base/artifacts.rs`) is decorative;
  the gameplay `Artifact` collectibles live in `src/zone/` as placeholders. Don't conflate them.
- **Physics/collision:** if a physics plugin is added, declare it in Cargo.toml and keep its systems
  isolated from game logic where practical.
- **WASM support is a goal.** Keep the crate compatible with `wasm32-unknown-unknown` builds:
  no `std::thread`/`std::net` in game code, gate platform-specific code with `#[cfg(target_arch = "wasm32")]`.

### Module layout

`src/main.rs` composes Bevy with six project plugins: `GameAudioPlugin`,
`WorldPlugin`, `PlayerPlugin`, `ZonePlugin`, `UiPlugin` and `SavePlugin`.

```
src/
├── main.rs          # App entry point: default plugins + state setup
├── states.rs        # AppState + InGamePhase, state transitions, pause toggle
├── save.rs          # saved progress: survey, clock, stalker and car positions
├── assets/          # asset path constants + startup preload handles
├── audio/           # synthesized ambience, effects, positional creature cues
├── player/          # stalker, animation, camera, orders/pathfinding, vehicle
├── zone/            # per-run loop: anomalies and artifacts (placeholder geometry so far)
├── world/           # terrain, water, roads, scenery, base, sites, wildlife, weather, fog
│   └── pillars/     # generated collision and relief data (tools/trim_tree_base.py,
│                    #   tools/sample_landmark_ground.py) — regenerate, don't hand-edit
└── ui/              # splash, curtain, HUD, tooltips; ui/text.rs holds display strings
```

The world is rebuilt deterministically from fixed seeds and placements; there is
no physics crate or networking. `save.rs` keeps the player's progress: the
`Explored` survey, the clock and where the stalker and their car stand, in a
plain text file (`progress.txt` in `$XDG_DATA_HOME/roadside-picnic`,
`~/.local/share/roadside-picnic`, `%APPDATA%` or `~/Library/Application
Support`), written atomically on F5, every minute and on quitting, and restored
on the first in-game frame. The web build keeps none. Playtests should set
`XDG_DATA_HOME` to a scratch folder so they neither load nor overwrite the
player's save.

### Asset tiers

`assets/models/<kind>/` is split into `low/`, `medium/` and `uhd/`. Landmarks,
sites, both checkpoints and the stalker use their `uhd/` scans; the sites stream
theirs in near the view and show `low/` stand-ins built from the same scans
elsewhere. Other runtime paths in `src/assets/mod.rs` load `medium/`, with `uhd/`
used for close-up swaps (e.g. the 4K shrub GLBs in `plants.rs`, the full
artifact scenes within 8 m).

`assets/` holds only files the game loads (every runtime path, `concat!`-built
ones included, is a string in the release binary). Unused tiers and pipeline
sources (reference boards, hangar parts, the old stalker, the soldier's
unrigged scans, the 4K artifact scans) are in
`~/Projects/agentic/RoadSide-Picnic/archived/roadside-picnic-unused-assets-2026-11-22.zip`, with sha256 sums in its `MANIFEST.txt`, and in the Git LFS
history. Restore them from the repo root before re-running a pipeline that
needs them: `unzip -n <zip> 'assets/*' -d .`.

### Assets workspace

`~/Projects/agentic/RoadSide-Picnic` is the **assets workspace**, or just **the
workspace**: whenever a request says "workspace", it means this folder. It holds
the Tripo generations (task records in `models/tripo-out/`, their concept and
preview images in `images/<name>-<id8>/`), every source model by category and
tier under `models/<kind>/{low,medium,uhd}/`, the reference boards and the
`archived/` zip above. Look there first for a source to build or texture an
asset from, and follow its `AGENTS.md` for where generated files go.

### Further reading

- `arch-readme.md` — runtime graph, per-frame schedule, coupling pitfalls, and
  step-by-step recipes ("How to add a static site model / hangar prop / artifact /
  creature / terrain feature / shader / gameplay event"). Read the relevant recipe
  before adding such content, and keep it in sync with architectural changes.
- `README.md` — per-OS system dependencies (ALSA etc.) and troubleshooting.
- `doc/` — lore, map layout and landmark references (`harmont_zone_landmarks.csv`,
  `Roadside_map.jpg`) and `to-do.txt`.
- `agent-skills.json` pins the agent skills for this repo; install them with
  `python3 tools/setup_agent_skills.py` (`--source-dir` or
  `ROADSIDE_PICNIC_AGENTIC_TOOLING` for a local agentic-tooling checkout).
  The `tripo-*` skills are billable: confirm the credit plan before generating.

## Code Style

- **No `unwrap()` in game logic.** Use `Option`/`Result` handling, or `expect()` with a message
  only where failure is impossible by construction.
- `cargo clippy -- -D warnings` must pass before declaring a change done.
- Name ECS systems `verb_noun` (e.g. `move_player`, `detect_anomaly_collisions`).
- Prefer small focused systems over monolithic ones; split when a system exceeds ~60 lines.
- Keep systems generic-free unless truly needed; Bevy's typed queries are enough.
- `use` statements: `std` first, then external crates, then `crate` modules, then `super` modules.
- Comments: `//` for explanations of non-obvious logic; avoid commenting obvious code.
- All text shown to players must go through localization-ready constants/resources, never hardcoded strings.

## Dependencies

- **Bevy is the only engine.** Do not add another engine or framework.
- Plugin choices should be minimal and justified — each added crate increases compile time.
- Keep Cargo.toml organized with comments grouping `[dependencies]`.

## Testing

- Unit-test pure logic (artifact pricing, inventory weight, anomaly damage formulas) with `#[cfg(test)]` mods.
- Integration smoke tests: spawn the `App` with plugins and step the schedule manually.
- No snapshot/golden tests for graphics.

## Git Workflow

- Commit messages in imperative mood, e.g. `add stalker movement system`.
- One logical change per commit.
- Do not commit `target/` or local asset dumps (see `.gitignore`).
