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

The upper-right in-game HUD shows the smoothed frame rate and, under it, the
stalker's world position (X, Y, Z in meters; Y is up).

## Controls

- **Any key** — enter the Zone from the hangar splash once loading finishes
- **Left click** — move the stalker there (pathfinding around obstacles and deep
  water; long trips run). While driving, click to drive there.
- **Shift + left click** — add a waypoint after the orders already given; a pulsing
  ring marks each one until it is reached and a thin line traces the route. A
  route started with Shift-clicks waits until Shift is released
- **WASD** — steer directly (camera-relative) and cancel a click order; hold **Shift** to run
- Walk into a rock, building or other solid to **climb** onto it; the cordon walls,
  checkpoints, trees and guards can't be climbed
- **Space** — jump
- **Ctrl** (hold) — crawl prone
- **E** — enter or exit the armored vehicle, or the Institute carrier while it stands
  still; **W/S** to accelerate/reverse, **A/D** to steer, **Space** to brake
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
- `cargo run --release -- --vram <GB>` caps the video memory the game uses:
  normally it caps the driver's live budget (by default the game takes what
  the driver's budget allows); where the driver can't report VRAM, it is the
  card size the fallback budget assumes (6 GB without it). See **RAM and
  GPU** in `doc/world-reference.md`.
- `cargo run --release -- --gpu-tier <modest|strong|exceptional>` forces the
  effects tier (normally decided from the card's size, and on 6 GB+ cards
  whether it has hardware ray tracing). See **RAM and GPU**.

## World reference

`doc/world-reference.md` describes every system below in detail: behaviour,
measured numbers (ranges, sizes, speeds) and the asset pipeline commands that
rebuild its models. **Read the system's section there before changing it or its
assets, and update it in the same change.** Sections are bullets named as here.

| Section | Code |
|---|---|
| Tactical view (camera, zoom, cutaway, tooltips, fog of war, map, orders, locator ring) | `player/{camera,orders,path,route}.rs`, `base/cutaway.rs`, `world/{shroud,places}.rs`, `ui/{tooltip,map,locator}.rs` |
| Terrain, roads, ground LODs, terrain textures; Old roads (painted asphalt, the Zone's road kit) | `world/{terrain,roads,terrain_material}.rs`, `shaders/terrain.wesl`, `tools/bake_road_kit.py` |
| Water | `world/{hydrology,water}.rs` |
| Day/night and weather; Wind; Sky & lighting | `world/weather.rs`, `world/mod.rs` |
| Ground fog and clouds | `world/ground_fog.rs`, `ground_fog/`, `shaders/ground_fog.wesl` |
| Scenery (trees, rocks, reeds, tile streaming), tree falls | `world/{scenery,canopy,tree_fall}.rs` |
| Plants (weeds, bushes, three LOD tiers) | `world/plants.rs`, `shaders/plant_wind.wesl` |
| Vegetation (clusters at landmarks) | `world/trees.rs` |
| Cordons (yard, Perimeter, gates, crawl hole, belts) | `world/cordon.rs` |
| Perimeter guns (laser scan, trespass, tracers, downed creatures) | `world/turrets.rs`, `turrets/{scan,fire,beam}.rs`, `wildlife/downed.rs`, `audio/guns.rs` |
| Sites (Harmont map, 32 scan placements, stand-ins, relief, Institute lights) | `world/{sites,map,institute}.rs` |
| Quarry ramp (haul road, `Surface::Ramp`, collision off the cut, scan cut, `tools/plan_quarry_ramp.py`); Golden Sphere's crater road (on through the Quarry rim) | `world/{quarry_ramp,ramp_cut}.rs`, `world/{roads,terrain,sites}.rs`, `shaders/ramp_cut.wesl`, `tools/trace_crater_backdrop.py`, generated `world/pillars/{quarry_ramp,golden_crater_ramp,golden_crater_backdrop}.rs` |
| Golden Sphere (hover, pool, ferrofluid, reflections and UHD scan per GPU tier) | `world/golden_sphere.rs`, `world/golden_sphere/{ferrofluid,mirror,reflection,uhd}.rs`, `shaders/{ferrofluid,golden_sphere}.wesl`, `tools/smooth_golden_sphere.py` |
| New Harmont; Railway; Marsh props; Rocks | `world/{town,railway,marsh,rocks}.rs` |
| Guards; UN patrol; Harmont traffic; Institute carrier; Institute helicopters | `world/{guards,helicopters}.rs`, `player/vehicle/{patrol,traffic,carrier,carrier_track,track_loop}.rs` |
| Townsfolk (New Harmont pedestrians, scientists at the Institute) | `world/townsfolk.rs` |
| Wildlife; Mutants | `world/wildlife.rs`, `wildlife/{rig,mutants}.rs` |
| Artifact showcase; Base (hangar, props, yard vehicles) | `world/base.rs`, `base/{artifacts,props,cutaway}.rs` |
| Armored vehicle | `player/vehicle.rs` |
| Collision, crushing, climbing | `world/{collision,crush}.rs` |
| Audio | `audio/` |
| Stalker model (rig, clips) | `player/animation.rs`, `tools/rig_stalker.py` |
| Precompiled textures (KTX2, baked tangents) | `tools/compress_glb_textures.py` |
| RAM and GPU (UHD scans from RAM, GPU scan budget, `--vram`, `GpuTier` and `--gpu-tier`, CPU light clustering) | `assets/{ram_source,gpu_scans}.rs`, `gpu_scans/` (`tier.rs`), `launch.rs`, `render/mod.rs` |
| Entering the Zone (curtain) | `ui/curtain.rs` |

Rules that hold everywhere:

- **Ground:** sample heights through the `Terrain` resource (`height_at`;
  `mesh_height_at` for the rendered surface, `water_depth_at` for traversal).
  Never hardcode ground heights.
- **Collision:** give every new solid model a `Collider` (upright `Pillar`s);
  mark fences `Unclimbable`. Regenerate `src/world/pillars/*.rs` with
  `tools/trim_tree_base.py` (relief and checkpoints:
  `tools/sample_landmark_ground.py`) whenever the source geometry changes.
- **Imported models only:** no procedural plant or tree geometry.
- **GLB pipeline:** finish every new or re-exported runtime GLB with
  `python3 tools/compress_glb_textures.py <glb>...`, and run it last (Blender
  and glTF Transform cannot read the converted textures). Edit pipeline scripts
  and regenerate; never hand-edit a generated GLB.
- **Stand-ins** (`low/` site models): never ground or recenter them.
- **VRAM:** all scans at once outgrow a 6 GB GPU; dense scans stream in near a
  `StreamAnchor` and show light stand-ins elsewhere. Load streamed UHD scans
  only through `GpuScans` (`assets::uhd(path)`), never straight from the
  `AssetServer`.
- **Hangar:** never save the `parts/` kit over `medium/hangar.glb`; remeasure
  walls and pendant mounts in `base.rs` if its geometry changes; the walkway
  lines are shader constants, move them with the exhibit rows. Do not replace
  the primary showcase artifact with an LOD.
- **Keep clear:** the hangar's central aisle and the parked vehicle's bay route,
  the yard's staging area and driveway, the roads.
- **Lighting:** tune light and emissive values in physical units for the
  daytime exposure (EV 13); steer the look with the clock, `HAZE` and the
  weather rather than adding ambient light.
- **Map:** `map(px, py)` maps `doc/Roadside_map.jpg` to the world at 2.4 m per
  pixel. Harmont lies on +X of the Perimeter, the Zone on -X (`in_zone`).
  Mutants keep to the Zone; Harmont, the hangar yard and the occupied vehicle
  are safe.

## Build & Run Commands

```bash
# Development run (fast, unoptimized)
cargo run

# Development run with fast relinking and asset hot reload (the `dev` feature)
cargo dev

# Release build (playable framerates; the shipping build, ~3.5 min to rebuild the game)
cargo run --release

# Playtest build: release speed within a few percent, rebuilds in ~35 s
# (no cross-crate LTO, 16 codegen units; the binary lands in target/playtest/)
cargo run --profile playtest

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
> LOD seams: visual, shader, asset, animation and audio changes need a `cargo run --profile playtest`
> (or `--release`) check.

### Hot reloading (development only)

Never enable these features in release, playtest or `wasm32` builds. Pass them on the command line rather than
adding them to the default features in `Cargo.toml`. `cargo dev` (an alias in `.cargo/config.toml`
for `cargo run --features dev`) turns on `bevy/dynamic_linking` and `bevy/file_watcher` together;
keep them out of the playtest build, whose FPS must match release and whose binary the playtest
scripts start without Cargo (a dynamically linked one would not find Bevy's library).

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
├── render/          # renderer tuning (CPU light clustering)
├── zone/            # per-run loop: anomalies and artifacts (placeholder geometry so far)
├── world/           # terrain, water, roads, scenery, base, sites, wildlife, weather, fog
│   └── pillars/     # generated collision and relief data (tools/trim_tree_base.py,
│                    #   tools/sample_landmark_ground.py) — regenerate, don't hand-edit
└── ui/              # splash, curtain, HUD, tooltips; ui/text.rs holds display strings
```

The world is rebuilt deterministically from fixed seeds and placements; there is
no physics crate or networking. `save.rs` keeps the player's progress: the
`Explored` survey, the clock and where the stalker, their car and (once taken)
the Institute carrier stand, in a
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
- `arch-readme-detailed.md` — each subsystem's code paths end to end, with
  mermaid diagrams, constants and the why behind each design (cited to comments,
  tests and commits). Keep it in sync with the code paths it describes.
- `doc/world-reference.md` — every system's behaviour, numbers and asset
  pipelines (indexed under World reference above).
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
