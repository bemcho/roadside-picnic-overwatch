# Roadside-Picnic

Roadside-Picnic is a 3D sci-fi survival/exploration prototype built with Rust
and Bevy 0.20. Inspired by the atmosphere of the Strugatsky brothers' *Roadside
Picnic*, it drops a stalker into a dangerous Zone of anomalies, artifacts,
mutants, abandoned industry, and hostile wilderness.

The playable world is a deterministic 3.44 km square with streamed terrain,
physical sky/lighting, rivers and a lake, shallow fords, dirt and old asphalt
roads, dense vegetation, imported sites and landmarks, wildlife, and four
mutant species. A new run starts inside a furnished hangar and can continue on
foot or in an armored vehicle; progress is saved and restored.

## Current features

- Smooth procedural terrain, mountains, quarry, bald hill, swamp, river, lake,
  coast, and horizon scenery.
- World-space dirt/gravel roads with tire ruts, graded approaches, and
  water-crossing fords; old asphalt highways that break up into cracked,
  potholed slabs inside the Zone.
- Streamed 128 m chunks with nearby detail and coarse distant/horizon LODs.
- Third-person stalker movement: walk, run, crawl, jump, fall, climb onto
  rocks and buildings, and collide with streamed pillars.
- Tactical camera from over the shoulder to the whole map, point-and-click
  orders with pathfinding and Shift-click waypoints, hover descriptions, a fog
  of war, and the Harmont map.
- Driveable armored vehicle with suspension, four sampled wheel contacts,
  steering, braking, a faster graded-road top speed, wheel animation, slides,
  and a forward spotlight.
- A cordoned hangar yard with a checkpoint, workshop props, parked vehicles,
  and fourteen animated, individually lit artifact displays.
- Distant sites, landmarks, rail embankments, substations, and a derailed train.
- New Harmont with townsfolk and traffic, the Institute with its carrier and
  helicopters, and the Perimeter: cordon walls, guards, a UN patrol and
  scanning guns.
- Day/night cycle and weather with wind, ground fog and clouds.
- Procedurally rigged wildlife with grounded gait animation, flight, calls, and
  positional sound.
- Neutral, curious, and aggressive mutants with patrol, inspect, chase, attack,
  recovery, and respawn behavior.
- Synthesized ambience, footsteps, impacts, doors, vehicle engine, and creature
  cues with global mute.
- Splash loading gate, pause menu, controls HUD, Zone counters, smoothed FPS
  and position display, and saved progress (F5, every minute and on quitting).

This is an active prototype rather than a finished campaign.

## Controls

| Input | Action |
|---|---|
| Any key | Enter the Zone after loading finishes |
| Left click | Move there (pathfinding; long trips run); while driving, drive there |
| `Shift` + left click | Add a waypoint after the orders already given |
| `W/A/S/D` | Camera-relative walk; cancels a click order |
| `Shift` | Run |
| `Space` | Jump; while driving, brake |
| `Ctrl` | Hold to crawl |
| Walk into a solid | Climb onto a rock, building or other solid |
| `E` | Enter or exit the armored vehicle, or the Institute carrier while it stands still |
| `W/S`, `A/D` (vehicle) | Accelerate/reverse, steer |
| Right mouse drag | Rotate the camera |
| Mouse wheel | Zoom from over the shoulder out to the whole map |
| Arrow keys / middle drag / screen edges | Roam the camera |
| `F` | Toggle the free camera |
| `C` | Find the stalker (the camera follows them again) |
| Mouse pointer | Hover over a place, creature or vehicle for its description |
| `T` | Hold to time-lapse the day/night cycle |
| `Esc` | Pause/resume |
| `M` | Open/close the map |
| `N` | Mute/unmute audio |
| `V` | Lift/restore the fog of war (for testing) |
| `F5` | Save progress |
| `Q` | Quit |

## Requirements

- A Git installation with [Git LFS](https://git-lfs.com/): the models and
  images under `assets/` are LFS objects.
- The stable Rust toolchain from [rustup](https://rustup.rs/).
- A GPU and drivers supported by `wgpu` (Vulkan, Metal, or DirectX 12).
- A working audio output device.

The game uses only local/bundled runtime assets. Tripo, Blender, and
`gltf-transform` are optional authoring tools; they are not required to play.

## Install

```bash
sudo apt install git-lfs    # macOS: brew install git-lfs; Git for Windows ships it
git lfs install             # once per user, before cloning
git clone https://github.com/bemcho/roadside-picnic-overwatch.git
cd roadside-picnic-overwatch
git lfs pull                # fetches the assets if the clone left pointer files
```

Without Git LFS the clone holds small text pointers in place of the GLB, PNG
and model KTX2 assets, and the game fails to load them (see Troubleshooting).

Install the platform dependencies below, then run `cargo run --release`.

### Linux

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install \
  build-essential \
  pkg-config \
  libasound2-dev \
  libudev-dev \
  libx11-dev \
  libxcursor-dev \
  libxi-dev \
  libxinerama-dev \
  libxrandr-dev \
  libwayland-dev \
  libxkbcommon-dev
```

Fedora:

```bash
sudo dnf install \
  gcc \
  pkgconf-pkg-config \
  alsa-lib-devel \
  systemd-devel \
  libX11-devel \
  libXcursor-devel \
  libXi-devel \
  libXinerama-devel \
  libXrandr-devel \
  wayland-devel \
  libxkbcommon-devel
```

Arch:

```bash
sudo pacman -S \
  base-devel \
  pkgconf \
  alsa-lib \
  systemd-libs \
  libx11 \
  libxcursor \
  libxi \
  libxinerama \
  libxrandr \
  wayland \
  libxkbcommon
```

Install Rust if needed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### Windows

1. Install [Git for Windows](https://git-scm.com/download/win). It ships Git
   LFS; keep its **Git LFS** component selected.
2. Install [Rust with rustup](https://rustup.rs/) and keep the default
   `stable-x86_64-pc-windows-msvc` toolchain.
3. Install [Build Tools for Visual Studio 2026](https://visualstudio.microsoft.com/downloads/)
   with the **Desktop development with C++** workload. The MSVC linker and
   Windows SDK are required by Rust. Rust finds the 2026 tools from 1.93 on
   (`rustup update stable`); the 2022 Build Tools still work.
4. In PowerShell:

   ```powershell
   git lfs install   # once per user, before cloning
   git clone https://github.com/bemcho/roadside-picnic-overwatch.git
   Set-Location roadside-picnic-overwatch
   git lfs pull      # fetches the assets if the clone left pointer files
   cargo run --release
   ```

Keep `cargo` and the game process on the same drive/filesystem where the
checkout lives; `cargo` locates runtime files from the repository root.

### macOS

Install the command-line developer tools, Git LFS (with
[Homebrew](https://brew.sh/); Apple's Git does not include it) and Rust:

```bash
xcode-select --install
brew install git-lfs
git lfs install   # once per user, before cloning
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Then clone and run:

```bash
git clone https://github.com/bemcho/roadside-picnic-overwatch.git
cd roadside-picnic-overwatch
git lfs pull      # fetches the assets if the clone left pointer files
cargo run --release
```

Apple Silicon and Intel Macs use the host Rust target selected by `rustup`.

## Run

Run from the repository root. The crate root is also Bevy's asset root.

```bash
# Fastest playable visual result
cargo run --release

# Development build
cargo run

# Playtest build: release speed within a few percent, much faster to rebuild
cargo run --profile playtest

# Ignore the saved progress and start afresh
cargo run --release -- --new
```

Progress (the surveyed map, the clock, where the stalker and the vehicles
stand) is saved to `roadside-picnic/progress.txt` under `$XDG_DATA_HOME`
(`~/.local/share` when unset) on Linux, `%APPDATA%` on Windows or
`~/Library/Application Support` on macOS.

Release mode is recommended for the full 3D world. The first build downloads
Rust crates and compiles Bevy, so it takes considerably longer than later
incremental builds.

Do not launch `target/release/roadside-picnic` directly for normal play: Bevy
may resolve `assets/` relative to the executable directory instead of the
repository.

### Graphics quality

The game reads every close-up (UHD) model once at start and keeps it in RAM,
so the loading screen takes a few seconds longer. That copy takes about
3.3 GB, and the game as a whole peaked at 9.3–11.7 GB of RAM in our tests
(about 7 GB before; both measured with five tree species, two since
dropped). It always leaves 4 GB of RAM free for the rest of the game: on a
machine short of RAM, the models that do not fit stay on disk and load from
there when needed.

It keeps as many close-up models on the GPU as the driver says fit, and frees
the least recently seen ones when video memory runs short. By our estimate
(not yet tried on a larger card, and made with five tree species), all of
them take about 4.4 GB of video memory, so every one would stay on the GPU
once the driver offers the game about 7.4 GB, which takes a 10 GB card; a
6 GB card has room for about 1.9 GB of them. A model that leaves the GPU
does not shrink the room the others have, so they do not leave one after
another.

- `--vram <GB>` caps the video memory the game uses. Use it to leave room for
  other programs, or where the driver cannot report VRAM (the game then
  assumes 6 GB). It combines with `--new`:

  ```bash
  cargo run --release -- --vram 3
  ```

- `--gpu-tier <modest|strong|exceptional>` forces the level that effects
  scaled to the GPU use, whatever the card (the Golden Sphere's reflections
  and its close-up model are the first). Without it the game decides once, at
  start, from the card's size: under 5 GB is modest, 5 GB and up strong, 16 GB
  and up exceptional (`--vram` caps the size it judges). The log says which
  tier it took:

  ```bash
  cargo run --release -- --gpu-tier modest
  ```

## Development

```bash
# Fast iterative type-check
cargo check

# Unit/integration tests
cargo test

# One test module or exact test
cargo test world::base::artifacts -- --nocapture
cargo test world::base::artifacts::tests::idle_motion_bobs_and_turns_from_a_stable_rest_pose -- --exact --nocapture

# Format and lint
cargo fmt -- --check
cargo clippy -- -D warnings
```

Development-only hot reload:

```bash
# File assets only
cargo run --features bevy/file_watcher

# Faster linking plus file watching
cargo run --features bevy/dynamic_linking,bevy/file_watcher

# Rust system-body hotpatching through Dioxus Subsecond
dx serve --hotpatch --features bevy/hotpatching
```

File watching does not rebuild procedural terrain/scenery or synthesized
audio. Hotpatching cannot introduce new components, resources, messages, or
changed system signatures. Keep these development features out of release and
WASM builds.

## Architecture and assets

- [`arch-readme.md`](./arch-readme.md) gives the bird's-eye ECS/plugin graph,
  runtime schedules, terrain/streaming/collision flow, and troubleshooting map.
- [`CLAUDE.md`](./CLAUDE.md) holds the contributor conventions and project-wide
  rules, and indexes [`doc/world-reference.md`](./doc/world-reference.md): each
  system's behaviour, model measurements and content pipelines.
- [`.github/copilot-instructions.md`](./.github/copilot-instructions.md)
  contains the concise guidance used by Copilot coding sessions.

Runtime GLBs, textures, and WESL shaders live under `assets/`. Preparation and
rigging tools live under `tools/`. Generated collision arrays live under
`src/world/pillars/` and are derived from the installed runtime model rather
than edited by hand.

## Agent skills for this repository

The relevant Bevy and Tripo workflows are published as agent skills:

- `bevy-game-dev`
- `blender`
- `tripo-3d`
- `tripo-assets`
- `tripo-game-asset`
- `tripo-studio-mcp`

Install the complete set into the local Copilot/Claude agent skill locations:

```bash
python3 tools/setup_agent_skills.py
```

On Windows, use:

```powershell
py tools/setup_agent_skills.py
```

The installer reads [`agent-skills.json`](./agent-skills.json), obtains the
canonical skill definitions, and installs links or copies where symlinks are
unavailable. It does not contain or request Tripo credentials.

`agentic-tooling` is the canonical skill source. Maintainers can avoid a network
clone by setting `ROADSIDE_PICNIC_AGENTIC_TOOLING` or passing
`--source-dir /path/to/agentic-tooling`. Collaborators need read access to that
repository, or a local copy of it, before they can install the optional skills.

## Troubleshooting

- **Splash never finishes loading:** run from the repository root and inspect
  the first `Could not preload ...` log entry.
- **`invalid glTF file: expected value at line 1 column 1` or `Invalid PNG
  signature`:** the assets are Git LFS pointer files, not the real files
  (`head -c 60 assets/textures/terrain/grass_surface.png` prints
  `version https://git-lfs...`). Install Git LFS, then run `git lfs install`
  and `git lfs pull` in the checkout.
- **The game reports paths under `target/.../assets`:** start it with
  `cargo run`/`cargo run --release`, not the executable directly.
- **No sound:** confirm the system output device, then press `N` once to avoid
  testing with the game muted. The audio is synthesized at runtime.
- **Long first compile:** Bevy and GPU/audio dependencies are large; subsequent
  debug builds use the existing incremental `target/` cache.
- **Low development FPS:** use `cargo run --release` for visual/performance
  testing.
- **WESL or asset changes do not appear:** restart for procedural content and
  synthesized audio; file watching does not rebuild every runtime path.
