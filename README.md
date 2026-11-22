# Roadside-Picnic

Roadside-Picnic is a 3D sci-fi survival/exploration prototype built with Rust
and Bevy 0.20. Inspired by the atmosphere of the Strugatsky brothers' *Roadside
Picnic*, it drops a stalker into a dangerous Zone of anomalies, artifacts,
mutants, abandoned industry, and hostile wilderness.

The playable world is a deterministic 3.44 km square with streamed terrain,
physical sky/lighting, rivers and a lake, shallow fords, dirt roads, dense
vegetation, imported sites and landmarks, wildlife, and four mutant species.
The run starts inside a furnished hangar and can continue on foot or in an
armored vehicle.

## Current features

- Smooth procedural terrain, mountains, quarry, bald hill, swamp, river, lake,
  coast, and horizon scenery.
- World-space dirt/gravel roads with tire ruts, graded approaches, and
  water-crossing fords.
- Streamed 128 m chunks with nearby detail and coarse distant/horizon LODs.
- Third-person stalker movement: walk, run, crawl, jump, fall, and collide with
  streamed pillars.
- Driveable armored vehicle with suspension, four sampled wheel contacts,
  steering, braking, a faster graded-road top speed, wheel animation, slides,
  and a forward spotlight.
- A cordoned hangar yard with a checkpoint, workshop props, parked vehicles,
  and fourteen animated, individually lit artifact displays.
- Distant sites, landmarks, rail embankments, substations, and a derailed train.
- Procedurally rigged wildlife with grounded gait animation, flight, calls, and
  positional sound.
- Neutral, curious, and aggressive mutants with patrol, inspect, chase, attack,
  recovery, and respawn behavior.
- Synthesized ambience, footsteps, impacts, doors, vehicle engine, and creature
  cues with global mute.
- Splash loading gate, pause menu, controls HUD, Zone counters, and smoothed
  FPS display.

This is an active prototype rather than a finished campaign.

## Controls

| Input | Action |
|---|---|
| Any key | Enter the Zone after loading finishes |
| `W/A/S/D` | Camera-relative walk |
| `Shift` | Run |
| `Space` | Jump; while driving, brake |
| `Ctrl` | Hold to crawl |
| `E` | Enter or exit the armored vehicle |
| `W/S`, `A/D` (vehicle) | Accelerate/reverse, steer |
| Right mouse drag | Orbit camera |
| Mouse wheel | Zoom camera |
| `Esc` | Pause/resume |
| `M` | Mute/unmute audio |
| `Q` | Quit |

## Requirements

- A Git installation.
- The stable Rust toolchain from [rustup](https://rustup.rs/).
- A GPU and drivers supported by `wgpu` (Vulkan, Metal, or DirectX 12).
- A working audio output device.

The game uses only local/bundled runtime assets. Tripo, Blender, and
`gltf-transform` are optional authoring tools; they are not required to play.

## Install

```bash
git clone https://github.com/bemcho/Roadside-Picnic.git
cd Roadside-Picnic
```

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

1. Install [Git for Windows](https://git-scm.com/download/win).
2. Install [Rust with rustup](https://rustup.rs/) and keep the default
   `stable-x86_64-pc-windows-msvc` toolchain.
3. Install Visual Studio 2022 Build Tools with the **Desktop development with
   C++** workload. The MSVC linker and Windows SDK are required by Rust.
4. In PowerShell:

   ```powershell
   git clone https://github.com/bemcho/Roadside-Picnic.git
   Set-Location Roadside-Picnic
   cargo run --release
   ```

Keep `cargo` and the game process on the same drive/filesystem where the
checkout lives; `cargo` locates runtime files from the repository root.

### macOS

Install the command-line developer tools and Rust:

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Then clone and run:

```bash
git clone https://github.com/bemcho/Roadside-Picnic.git
cd Roadside-Picnic
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
```

Release mode is recommended for the full 3D world. The first build downloads
Rust crates and compiles Bevy, so it takes considerably longer than later
incremental builds.

Do not launch `target/release/roadside-picnic` directly for normal play: Bevy
may resolve `assets/` relative to the executable directory instead of the
repository.

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
- [`CLAUDE.md`](./CLAUDE.md) documents model measurements, content pipelines,
  rendering constraints, and contributor conventions.
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
- **The game reports paths under `target/.../assets`:** start it with
  `cargo run`/`cargo run --release`, not the executable directly.
- **No sound:** confirm the system output device, then press `M` once to avoid
  testing with the game muted. The audio is synthesized at runtime.
- **Long first compile:** Bevy and GPU/audio dependencies are large; subsequent
  debug builds use the existing incremental `target/` cache.
- **Low development FPS:** use `cargo run --release` for visual/performance
  testing.
- **WESL or asset changes do not appear:** restart for procedural content and
  synthesized audio; file watching does not rebuild every runtime path.
