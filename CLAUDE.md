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
  water; long trips or **Shift**-click run). While driving, click to drive there.
- **WASD** — steer directly (camera-relative) and cancel a click order; hold **Shift** to run
- **Space** — jump
- **Ctrl** (hold) — crawl prone
- **E** — enter or exit the armored vehicle; **W/S** to accelerate/reverse, **A/D** to steer, **Space** to brake
- **Mouse wheel** — zoom from over the shoulder out to the whole map; pitch steepens with zoom
- **Right mouse drag** — rotate the camera
- **Arrow keys / middle drag / screen edges** — roam the camera freely
- **F** — toggle the free camera (hold the view where it is) and following the stalker
- **Mouse pointer** — hover over a site, landmark, building, water, creature,
  the car or an exhibit for its description (unsurveyed places show only their kind)
- **T** (hold) — time-lapse the day/night cycle (40× speed)
- **Esc** — pause / resume
- **M** — mute / unmute game audio
- **Q** — quit

## Tactical view

The game plays from a strategy-scale camera (`src/player/camera.rs`): zoom runs
6 m to 4.6 km, the far plane, sun shadow cascades and distance fog follow the
zoom, and the low cotton fog thins out above ~140 m so the map overview stays
readable. The camera's `ViewFocus` (the audio listener `Ears`) also becomes a
`StreamAnchor` while zoomed under 250 m, so roaming streams collision, plants,
ground LODs and walls around the view as well as around the stalker.
`detail_range` adds `STRATEGY_REACH` (75 m) to every detail range for the raised
camera. The hangar's roof and upper walls are cut away (`base/cutaway.rs`,
`shaders/cutaway.wesl`) while the camera or its focus is over the hall.
The hangar's pendant lamps and cords (`RoofFixture`) hide with the roof.
Hover descriptions (`ui/tooltip.rs`) pick, in order, a `Describe`d entity
(creatures, the car, exhibits), a site/building outline, then the named ground
(`world/places.rs`: water from the hydrology profiles, quarry, bald hill, yard).
The stalker surveys an `Explored` grid within 150 m as they travel.
Point-and-click orders (`player/orders.rs`) use grid A* over collider pillars
and deep water (`player/path.rs`); the vehicle follows them with an autopilot
in `drive_vehicle`.

## Procedural content

- **Terrain** (`src/world/terrain.rs`) is a deterministic 3.44 km square built in
  128 m chunks: rolling hills, the Zone's ridge mountains, the quarry basin,
  marsh hollow and bald hill (`landform_height`). The graded road network
  (`src/world/roads.rs`) follows the Harmont map: Harmont's streets, the highway
  from the Perimeter gate past the factories and the Garage, the railway tracks,
  the Chapter 4 track through the breach, the marsh road and the quarry. Roads
  are graded over the sited land (pads and dioramas), then paved.
  The same network keeps vegetation off the lanes and paints a world-space
  gravel and tire-rut mask independent of terrain LOD. Heights are sampled
  through the `Terrain` resource (`Terrain::height_at`) — never hardcode ground heights
  in other plugins. `Terrain::mesh_height_at` tracks the rendered surface near
  the player. Ground tiles swap from 2 m to 8 m sampling at distance, with
  stitched edges across both resolutions. Larger 16 m horizon tiles keep the
  coastal water, lake basin and rolling country visible beyond the traversal
  limit without nearby collision or plant detail.
  `tools/prepare_terrain_textures.py` extracts tiling albedo,
  normal, and packed AO/roughness/height maps from `terrain_texture_image_2.webp`
  (moss) and `terrain_texture_1.webp` (bare ground); those source sheets are not
  checked in, only the extracted maps in `assets/textures/terrain`. The shader blends irregular
  patches over the terrain masks, uses the heights for shallow parallax, and adds
  snow on high, gentler slopes.
- **Water** (`src/world/{hydrology,water}.rs`): the main river crosses the map
  into a lake north of Harmont and is joined by a tributary in the Zone; the
  marsh in the Zone's south has shallow pools between drier hummocks. Use `Terrain::water_depth_at` for traversal. Walk and
  drive through the shallows; deep water stops you. The main dirt track crosses
  at a vehicle-depth ford.
- **Day/night and weather** (`src/world/weather.rs`): a 24-minute day (hold T to
  time-lapse) moves the sun and a full moon (a second, dim `DirectionalLight` with
  its own disk) by solar geometry; whichever is up casts shadows, and exposure
  eases from EV 13 by day to EV 9 at night. Each day rolls clear, humid, overcast
  or rain. Temperature follows the sun (coldest at 05:00); relative humidity
  comes from the day's dew point (Magnus), so it peaks late at night and at dawn,
  or stays high on humid and rainy days. The HUD shows time, temperature,
  humidity and sky.
- **Ground fog** (`src/world/ground_fog.rs`, `ground_fog/{field,render}.rs`,
  `assets/shaders/ground_fog.wesl`): a custom render pass (after the main pass)
  raymarches a thin layer that follows the terrain or open water. Its top sits
  between half the armored car's height and its roof (1.25–2.6 m), and visibility
  inside ranges 1–5 two-metre ground tiles, so a crawling stalker disappears while
  a standing one shows chest-up. The fog field texture, built from the distant
  terrain tiles while the landscape loads, stores the surface height and a fog
  affinity: water and its banks, vegetation, hollows and low ground. Saturated
  air (`Weather::fog`) spreads it; drifting banks, billows and wind break it up.
  The sun and moon light it through the atmosphere with shadows and forward
  scattering. The hangar hall stays clear, and the fog thins out above ~140 m of
  camera height. Bevy's distance fog adds the regional 10–16-tile cloud shelf
  over far low ground (shorter in humid air); the terrain shader masks it to low
  ground, and birds disable classic fog.
- **Scenery** (`src/world/scenery.rs`): textured rock outcrops and reeds are drawn
  in chunk batches. Every tree is an imported model: saplings, broadleaves, tall
  conifers, birches, and dead trees (`assets/models/trees/medium/`) grow around
  the hangar and in the named landmark groves; open country has no trees. Their GLBs use one ~500-triangle primitive each: simplified
  textured wood and, for living trees, cutout crown faces baked from the
  supplied models.
  `src/world/canopy.rs` fits the roots and trunks, limits
  shadows to nearby trees, and sways some living trees without moving their
  colliders. Individual trees can randomly snap and fall while in view
  (`src/world/tree_fall.rs`, `TreeBreak` message); their
  collision goes away immediately and they don't respawn during the run.
  Colliders and undergrowth stream in a 3×3 chunk neighborhood.
  Camera frusta and distance ranges cull scenery detail independently of
  collision and leave the terrain skyline visible.
- **Wind** (`src/world/weather.rs`, `Wind`): the day's breeze with gusts of
  4–10 s that come and go. It leans and stirs the trees, sways imported bushes
  from the root, bends the batched ground cover in `shaders/plant_wind.wesl`
  (vertex color alpha = height above the root), drifts and churns the ground fog,
  and swings the stalker's coat.
- **Plants** (`src/world/plants.rs`): dry grass and thornbushes favor dry ground,
  shrubs favor woodland, and hogweed and emissive glowweed favor damp ground.
  Each dry-grass placement seeds a patch of up to ten procedural plants, while
  bushes and weeds seed up to 100 and spread across a proportional radius. The
  root of every shrub or thorn patch uses an imported PBR model: byte-identical
  4K UHD shrub/thornbush GLBs render within 8 m, then crossfade to 1536 px
  medium variants through about 70 m. The remaining patch plants use small,
  frustum-culled mesh batches, grown on a worker thread as chunks stream in
  (one tile is spawned per frame), not an
  entity per plant. Ground cover has no collision or shadows. The rear and
  sides of the hangar form a dense shelterbelt; the central yard and roads stay
  clear.
- **Vegetation** (`src/world/trees.rs`): four extracted, textured vegetation clusters
  (`assets/models/vegetation/medium/*.glb`) are scattered only around landmarks, on a jittered grid,
  weighted by `Terrain::woodland_at` (off-road, outside the quarry, sparse above
  the treeline and on the bald hill). Rebuild them from the packed source with
  `tools/prepare_vegetation_asset.py <source.glb> assets/models/vegetation/medium assets/textures/fauna`
  (run in headless Blender after `--`). It reuses the saved albedo, normal and
  roughness PNGs; `--boards <image_3> <image_2> <image_1> <image_0>` rebuilds
  512 px maps by combining swatches and close-ups from the reference sheets.
  The forest uses
  separate birch/petrified bark, and plants share the mud material at their bases.
  Texture cards, material balls and miniature LOD previews are excluded, and
  each extracted cluster is centered with its lowest point at ground level.
  Regenerate each collision pillar file with `tools/trim_tree_base.py` whenever
  the source geometry changes.
- **Cordons** (`src/world/cordon.rs`): the enclosed hangar yard has a level
  staging area, with a checkpoint beside the 16 m opening on the north road.
  The Perimeter wall (`PERIMETER_X`) runs the map's length between Harmont and
  the Zone, with the Institute gate on the highway (guarded by a second
  checkpoint) and the stalkers' breach on the Chapter 4 track; `in_zone` tells
  the sides apart. A separate continuous wall marks the map's edge; the panels
  follow the rendered terrain and stay above the water at river crossings. Wall meshes and solid collision stream in nearby terrain tiles.
  Keep the staging area and driveway clear; the tree and bush shelterbelt is
  concentrated behind the hangar and along the sides.
  The wall repeats one panel thousands of times, so it keeps its medium model:
  simplify with glTF Transform `simplify --ratio 0.24 --error 0.016`, resize to
  1536 px, bake tangents with `--vertex-layout separate`, then ground with
  `tools/prepare_plant_asset.py`. Both checkpoints use the UHD scan; re-measure
  `src/world/pillars/uhd_checkpoint.rs` with `tools/sample_landmark_ground.py`.
- **Sites** (`src/world/sites.rs`): the Harmont map from `doc/` (map, layout
  notes and landmark CSV) as one registry of 33 placements of the UHD landmark
  and site scans. Harmont lies on the hangar's side of the Perimeter (+X): New
  Harmont, the Institute on the Perimeter, the Plague Quarter, the Borscht and
  the roadhouse, the school, houses and cemetery. The Zone lies beyond it (-X),
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
  footprint around any `StreamAnchor`. Build a stand-in from the JPEG source
  with glTF Transform `simplify --ratio 0.02 --error 0.002` and `resize --width
  1024 --height 1024`, then compress; never ground or recenter it.
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
  and its untouched full 4K PBR scene is streamed in. Do not replace the primary
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
  changes, remeasure the walls and pendant mounts in `base.rs`. A stronger
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
  independent wheel pivots and a forward spotlight that turns on while occupied.
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
- **Audio** (`src/audio/`) is synthesized at startup: looping Zone ambience,
  gait-dependent footsteps, jump and landing, vehicle doors, and a speed-driven
  engine. Wildlife calls, footfalls, takeoff/landing, and fight cues originate
  from the creature's world position; nearby sounds pan with the camera and fade
  with distance. Footfalls follow accepted travel and attack sounds follow
  chase, lunge, and hit events. M mutes both positional and non-positional audio.
  Playback follows the system's default audio output.
- **Stalker model** (`assets/models/stalker/uhd/stalker_rigged.glb`) is the UHD Tripo
  scan rigged in headless Blender by `tools/rig_stalker.py` (its docstring has the UHD
  command): it binds in a T-pose that the clips hang from the shoulders, and adds eight
  coat bones (`CoatFront/Back/Left/Right.1/.2`) that no clip keys. The script also bakes
  the Crawl/Idle/Jump/Run/Walk clips (glTF animation indices 0–4, sorted by name).
  `src/player/animation.rs` cross-fades them by `Gait`, pitches the body face-down while
  crawling (the Crawl clip is posed standing) and swings the coat against the air and in
  gusts; the splash hangs the arms the same way. Edit motion in the script and
  regenerate rather than hand-editing the GLB.
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
  and the small tiling terrain PNGs.
- **Entering the Zone** (`src/ui/curtain.rs`): an "Entering the Zone" curtain
  covers the freshly spawned world until the render pipeline queue has stayed
  empty briefly (at most 12 s), so shaders compile and spawn hitches happen
  behind it instead of as pop-in. NVIDIA's driver caches compiled pipelines on
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
- **Two kinds of artifact:** the hangar showcase (`world/base/artifacts.rs`) is decorative;
  the gameplay `Artifact` collectibles live in `src/zone/` as placeholders. Don't conflate them.
- **Physics/collision:** if a physics plugin is added, declare it in Cargo.toml and keep its systems
  isolated from game logic where practical.
- **WASM support is a goal.** Keep the crate compatible with `wasm32-unknown-unknown` builds:
  no `std::thread`/`std::net` in game code, gate platform-specific code with `#[cfg(target_arch = "wasm32")]`.

### Module layout

`src/main.rs` composes Bevy with five project plugins: `GameAudioPlugin`,
`WorldPlugin`, `PlayerPlugin`, `ZonePlugin` and `UiPlugin`.

```
src/
├── main.rs          # App entry point: default plugins + state setup
├── states.rs        # AppState + InGamePhase, state transitions, pause toggle
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
no physics crate, networking or save system.

### Asset tiers

`assets/models/<kind>/` is split into `low/`, `medium/` and `uhd/`. Landmarks,
sites, both checkpoints and the stalker use their `uhd/` scans; the sites stream
theirs in near the view and show `low/` stand-ins built from the same scans
elsewhere. Other runtime paths in `src/assets/mod.rs` load `medium/`, with `uhd/`
used for close-up swaps (e.g. the 4K shrub GLBs in `plants.rs`, the full
artifact scenes within 8 m).

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
