# Roadside-Picnic repository instructions

Read `CLAUDE.md` for project-specific asset recipes and contributor rules, and
`arch-readme.md` for the cross-module runtime graph. This repository uses Rust
2024, Bevy 0.20.1, and Bevy's WESL material shaders.

The repository's relevant agent skills are pinned in `agent-skills.json`.
Install them into the local Copilot/Claude/agent skill locations with:

```bash
python3 tools/setup_agent_skills.py
```

Pass `--source-dir /path/to/agentic-tooling` or set
`ROADSIDE_PICNIC_AGENTIC_TOOLING` when the canonical private skill repository
is already checked out locally.

Use `bevy-game-dev` for project development, `blender` for local mesh
preparation, and the `tripo-*` skills for paid asset generation, texturing,
rigging, conversion, or Studio fallback. The Tripo skills require credit-plan
confirmation before any billable operation.

## Build, test, and lint

Run commands from the repository root so Bevy resolves `assets/` correctly.

```bash
# Fast iterative type-check
cargo check

# Development run
cargo run

# Playable visual/performance run
cargo run --release

# Full suite
cargo test

# One module or one exact test
cargo test world::base::artifacts -- --nocapture
cargo test world::base::artifacts::tests::idle_motion_bobs_and_turns_from_a_stable_rest_pose -- --exact --nocapture

# Formatting and strict lint
cargo fmt -- --check
cargo clippy -- -D warnings
```

Visual, shader, asset, animation, and audio changes require a release run;
`cargo check` cannot prove runtime WESL compilation, GLB loading, HUD layout,
audio output, or terrain LOD seams. Validate changed GLBs with:

```bash
npx @gltf-transform/cli validate path/to/model.glb
```

Development-only hot reload commands are:

```bash
cargo run --features bevy/file_watcher
cargo run --features bevy/dynamic_linking,bevy/file_watcher
dx serve --hotpatch --features bevy/hotpatching
```

Do not enable these features in the default dependency set, release builds, or
WASM builds. File watching covers file-backed assets only; Rust-procedural
terrain/scenery and synthesized audio still need a restart. Code hotpatching
cannot add or reshape components, resources, messages, or system signatures.

## Runtime architecture

`src/main.rs` composes Bevy with `GameAudioPlugin`, `WorldPlugin`,
`PlayerPlugin`, `ZonePlugin`, and `UiPlugin`.

- `AppState::Loading -> AppState::InGame` is the world lifecycle.
- `InGamePhase::Running/Paused` is an overlay substate. Pausing must keep the
  Zone, player, and HUD spawned.
- Startup preloads `src/assets/mod.rs` handles and computes the coarse
  `Landscape` asynchronously. `ui/splash.rs` watches recursive asset load
  state and only becomes Ready after the preload contract is complete.
- `OnEnter(AppState::InGame)` spawns the world, sky, hangar, props, artifact
  showcase, cordon, sites, wildlife, player, zone objects, and HUD.
- `WorldPlugin` runs terrain/cordon/scenery streaming in `PreUpdate`, gameplay
  pose updates in `PostUpdate` before `TransformSystems::Propagate`, and
  despawns world roots on `OnExit`.
- Audio is synthesized and played from `src/audio/`; no runtime audio files are
  required.

`Terrain` is the cross-module ground source of truth:

```text
Terrain::height_at / mesh_height_at / normal_at / water_depth_at
  -> player and vehicle contact
  -> wildlife feet and patrol validation
  -> site foundations
  -> vegetation clearances
  -> terrain/water LOD meshes
```

Site placement is deliberately coupled. Changing `sites::LANDMARKS` also
affects `RoadNetwork`, graded terrain, the visual road mask, vegetation masks,
rendered GLBs, and streamed pillar collision. Treat those as one change and run
the site/road/terrain tests together.

The hangar display artifacts in `src/world/base/artifacts.rs` are visual
showpieces; gameplay `Artifact` components in `src/zone/` are separate
placeholder collectibles. Do not conflate them.

Wildlife shares the `Kind -> source GLB -> Rig -> Wildlife motion` pipeline.
Mutant decision state lives in `src/world/wildlife/mutants.rs`, movement in
`movement.rs`, procedural posing in `rig.rs`, and positional audio in
`sound.rs`.

## Repository-specific conventions

### ECS lifecycle and ordering

- Own components, resources, messages, and systems in the feature's plugin
  module. Prefer focused system functions named `verb_noun`.
- Spawn state-owned content in `OnEnter`, despawn it in `OnExit`, and gate
  gameplay with `run_if(in_state(InGamePhase::Running))`.
- Do not model lifecycle state with an ad hoc boolean resource.
- Use Bevy messages for one-shot events (`Respawn`, `CreatureSound`,
  `TreeBreak`); components/resources hold persistent state.
- Use `.chain()` or explicit ordering when a system consumes another system's
  changes. Systems that update local transforms must run before
  `TransformSystems::Propagate` when children must render correctly in the same
  frame.
- Keep `WorldRoot`, `ChildOf`, and `DespawnOnExit` relationships intact so pause
  and state transitions do not leak entities.

### Grounding, streaming, and collision

- Never hardcode a world ground height. Ground objects from
  `Terrain::mesh_height_at` when matching rendered triangles; use
  `height_at` for analytical sampling.
- Preserve deterministic placement. Terrain, scenery, sites, and patrols use
  fixed seeds and candidates rather than runtime randomness.
- Distant terrain/forest visuals and the nearby 3x3 collision/undergrowth stream
  are separate budgets. Do not create an entity per distant plant or tree.
- Static collision is the custom `Collider(&[Pillar])` model in
  `src/world/collision.rs`. Pillars are upright and `push_out` handles step-over
  and crawl-under behavior. Large pitch/roll is not represented by this model.
- Do not hand-edit `src/world/pillars/*.rs`. Regenerate a model's pillars from
  the installed runtime GLB with `tools/trim_tree_base.py`; use
  `--split-columns` for roofs and open bays. Spawn the visual and collider with
  the same transform/scale.

### Assets and asset preparation

- Canonical runtime paths belong in `src/assets/mod.rs`; systems load through
  `AssetServer`, not filesystem calls.
- `load_single_primitive` means exactly mesh 0, primitive 0, material 0. Use a
  `WorldAssetRoot` scene (`#Scene0`) for multipart scenes.
- Preload every asset required by an `OnEnter` path, otherwise the splash can
  become Ready before the dependent model/material exists.
- Preserve glTF Y-up and measure model forward orientation. The armored
  vehicle's child node names (`WheelFrontRight`, `WheelFrontLeft`,
  `WheelRearRight`, `WheelRearLeft`) and stalker animation order (`Crawl`,
  `Idle`, `Jump`, `Run`, `Walk`) are runtime contracts.
- Modify source preparation tools instead of manually patching generated
  runtime assets. Reuse `tools/prepare_plant_asset.py`,
  `tools/prepare_*_asset.py`, `tools/rig_stalker.py`, and
  `tools/trim_tree_base.py`.
- The camera uses HDR `Exposure { ev100: 13.0 }` with a physical atmosphere and
  sunlight; evaluate outdoor material/light changes under that setup.

### Player, UI, and audio

- The player movement chain is respawn -> vehicle interaction -> on-foot
  movement -> vehicle movement -> wheels/spotlight -> camera -> animation.
  Preserve that ordering when inserting dependent systems.
- Vehicle motion is substepped and uses accepted travel, four tire contacts,
  limited suspension, slope/water limits, and `push_out`; wheel roll must use
  accepted signed distance, not elapsed held input.
- Put every player-visible string in `src/ui/text.rs`.
- Footsteps follow accepted distance and gait. Vehicle door/engine cues and
  creature cues follow actual state transitions, not held keys.
- `M` must mute live audio sinks and pending playback settings; `GlobalVolume`
  alone is insufficient.

### Code shape

- Bevy is the only engine; avoid adding another engine or physics framework.
- No `unwrap()` in game logic. Use `Option`/`Result`, or `expect()` only where
  failure is impossible by construction.
- Keep `use` order as `std`, external crates, `crate`, then `super`.
- Generated files and raw/local asset dumps must not replace checked-in runtime
  assets or `target/`.
- For pure logic, add focused unit tests. For ECS behavior, build a minimal
  `App`, insert required resources, and step the relevant schedule. Do not add
  graphics snapshot/golden tests.
- Keep the crate compatible with `wasm32-unknown-unknown`: avoid
  `std::thread`/`std::net` in game code and gate platform-specific code.
- When a change alters the plugin graph, schedule, source-of-truth ownership,
  or an important integration pitfall, update `arch-readme.md` in the same
  change.
