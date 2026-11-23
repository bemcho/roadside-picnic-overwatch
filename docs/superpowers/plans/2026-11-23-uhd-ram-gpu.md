# UHD from RAM, GPU Budget Implementation Plan (phase 1 of 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read every UHD scan the game can show into RAM once at load, keep GPU copies of them under the driver's live VRAM budget (`--vram` caps it), and route today's four streamed UHD kinds (tree close-ups, the nearest plant's scan, sites and landmarks, artifact scenes) through that, so nothing is read from `assets/` after the entry curtain.

**Architecture:** `main.rs` registers an in-memory asset source (`ram://`) before `DefaultPlugins`; at startup every listed UHD file is read through Bevy's own reader on the IO pool into its shared `Dir`, and the splash waits for that. A `GpuScans` resource is the only owner of UHD GPU copies: callers `want` a scan when an anchor comes within 40 m, `GpuScans` loads it from `ram://` with render-only usages under a byte budget (`--vram` − the game's own VRAM − 0.5 GB), evicts the least recently used scan only after two idle minutes, and hands out the scene only once a render-world system has seen all its meshes and images prepared.

**Tech Stack:** Rust 2024, Bevy 0.20.1 (`bevy::asset::io::memory::{Dir, MemoryAssetReader}`, `AssetSourceBuilder`, `GltfLoaderSettings`, `RenderAssetUsages::RENDER_WORLD`, `RenderAssets<RenderMesh>`, `RenderAssets<GpuImage>`, `IoTaskPool`, `bevy::tasks::futures::check_ready`).

**Spec:** `docs/superpowers/specs/2026-11-23-vegetation-lod-design.md` (sections "RAM and GPU", "Measuring", "README", "Order of work" step 1). Phases 2 (vegetation and rock tiers, `--detail`) and 3 (UHD for the other kinds) get their own plans once this phase is measured: their budgets depend on the numbers Task 9 records.

## Global Constraints

- Bevy 0.20, no new crates. Task 5 adds `ash` 0.38 as a direct dependency (and `wgpu` 30 if Bevy does not re-export `wgpu::hal`), for native targets only. Both are already in `Cargo.lock` at those versions, so nothing new is compiled. Keep `wasm32` compatibility: no threads in game code; no `std::fs` in game code (tests may use it).
- Native builds only for the RAM level: `wasm32` keeps loading from `assets/` as today (`#[cfg(target_arch = "wasm32")]`).
- `cargo clippy -- -D warnings` passes; no `unwrap()` in game logic; systems are `verb_noun`, at most 7 parameters and about 60 lines; `use` order std, external, `crate`, `super`.
- Player-facing text goes through `src/ui/text.rs`; log lines may be plain strings.
- Close-up scans stay in their own GPU buffers: never split a scan to fit the upload limit.
- Keep the smooth-streaming budgets: `PLANTS_PER_FRAME`, one tile per frame, `UNLOADS_PER_FRAME`.
- Sites and landmarks keep their 150 m show rule (`FULL_SCAN_RANGE`) and 60 m keep margin (`FULL_SCAN_KEEP`).
- Targets: no frame over 33 ms in the traced walk and drive; no path under `assets/` opened after the curtain lifts.
- Git (user rule): agents do not commit, push or merge. Each task ends at a checkpoint: run the checks, then suggest a commit message prefixed `worktree-vegetation-lod-design:` (at most 10 lines, ending with `Claude-Session: https://claude.ai/code/session_013NVSdF8r182CK5kaKAUHhv`). Commit only after the human confirms.
- Playtests: Xephyr :5 at 1280x720, `XDG_DATA_HOME=$PWD/target/playtest/data`, `LC_ALL=C` for `bc`/`printf`; stop only the processes you started; delete raw `*.json` traces afterwards and keep the summaries.

## Review Focus

1. **A listed UHD file is missing or unreadable at load.** The splash must show its failed state and log the path, as a failed preload does, not wait forever. (Task 3 test `a_missing_file_fails_the_fill`.)
2. **`--vram` too small or malformed** (`--vram 2`, `--vram abc`, `--vram` as the last argument, `--vram -3`). A malformed value warns and is ignored (Task 5 makes "no flag" mean the driver's budget). A budget below the pinned trees loads no pin that does not fit and warns once, and those close-ups keep their medium models; nothing crashes. (Task 2 test `a_bad_vram_value_keeps_the_default`; Task 4 test `pinned_scans_load_first_but_only_within_the_budget`.)
3. **The stalker paces back and forth across the 40 m line or a site's reach.** No scan leaves the GPU and comes back again and again (anti-goal "thrashing"). (Task 4 test `a_scan_used_within_two_minutes_never_leaves`.)
4. **A scan's mesh has already moved to the render world when it is measured.** `Mesh::attributes()` panics on an extracted mesh, so measuring must not call it; the scan keeps its estimate and a warning is logged. (Task 5 test `scan_bytes_is_unknown_once_a_mesh_has_moved_to_the_gpu`.)
5. **A scan is shown while its GPU copy is evicted.** An entity still holding the handles would keep the copy alive while the budget counts it free. Every caller wants each scan it shows on every frame, and only idle scans leave. (Task 7 test `a_shown_scan_is_wanted_every_frame`; Task 8 tests that a shown site and a shown artifact keep wanting.)
6. **The host has less VRAM than the game would like, or loses some while playing** (the user's T1200 laptop, 4 GB, ran out of memory after zooming out, in and out with free roam; another program grabs VRAM). The budget follows the driver's live numbers. Scans not shown this frame leave until the rest fit, and nothing allocates past the budget. (Task 4 tests `a_shrunk_budget_evicts_scans_not_shown_least_recently_used_first` and `the_live_budget_is_what_the_driver_leaves_minus_a_margin`; Task 9 replays the user's steps with `vram-hog` holding the 2060 to 4 GiB.)

---

### Task 1: Baseline measurements

No game code. Records the numbers that phase 1 is judged against and that set `GAME_VRAM_GB`.

**Files:**
- Create: `~/Projects/agentic/agentic-tooling/testing/drop-file-cache/` (only if no such tool exists there)
- Output: `target/playtest/phase1/baseline.txt` (not committed)

**Interfaces:**
- Produces: `GAME_VRAM_GB`, the game's VRAM with no UHD scan streamed (Task 4 uses it), and the baseline rows Task 9 compares against.

- [ ] **Step 1: Get a page-cache eviction tool.** Search `~/Projects/agentic/agentic-tooling` first (none found on 2026-11-23). If still missing, create `testing/drop-file-cache/drop-file-cache.py`: for every file under the given paths, `os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)` (no root needed), printing the total bytes. Add a README with the usual metadata, following that repo's README. Test it with `fincore` on one GLB before and after. Commit and push there as `drop-file-cache: evict files from the page cache for cold-load timing` (that repo's own rule allows it).

- [ ] **Step 2: Build the traced binary on the current HEAD**

Run: `cargo build --release --features bevy/trace_chrome --target-dir target/trace`
Expected: `Finished`.

- [ ] **Step 3: Run four scenarios** with `run-traced.sh -d :5 -t roadside -r show_ready -T 300` (see the `bevy-trace-playtest` memory). For each, sample VRAM with `nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits -lms 500` into a file, after recording the desktop's value before launch. Wrap the binary in `/usr/bin/time -v -o <scenario>.time` for peak RSS.
  - **walk**: a fresh `target/playtest/data`, `--new`, steps `sleep:1 key:space:0.2 sleep:16 down:Shift_L down:w sleep:45 up:w up:Shift_L sleep:2`.
  - **drive**: save with `stalker 841.4 3.0 -30.0` and `car 839.0 2.0 -30.0 0.0`, steps `sleep:1 key:space:0.2 sleep:16 key:e:0.2 sleep:1.5 down:w sleep:18 up:w key:space:1.5 sleep:2` (passes the garage site).
  - **overview**: `--new`, steps `sleep:1 key:space:0.2 sleep:16 scroll:-60 sleep:20`.
  - **cold splash**: evict `assets/` with the Step 1 tool, then the overview run again. Its `ready` mark is the cold load time; the earlier overview's mark is the warm one.

- [ ] **Step 4: Record disk reads after the curtain.** Re-run the walk with the binary wrapped as `strace -f --seccomp-bpf -ttt -e trace=openat -o target/playtest/phase1/walk.strace`. The curtain lifts at most `MAX_SECONDS` (12 s) after the `space` key. Count as "after the curtain" every `assets/` open later than the first strace timestamp plus the `ready` mark plus 13 s. Today, expect the tree, plant and site scans. Task 9 applies the same rule, and can also use its `Entered the Zone` log line.

- [ ] **Step 5: Write `baseline.txt`:** for each scenario the `main app` max and p99 frame time from `summarize.py --window`, the peak VRAM minus the desktop, the peak RSS, the warm and cold `ready` time, and the walk's `assets/` opens after the curtain. Set `GAME_VRAM_GB` to the overview's peak VRAM minus the desktop, rounded up to 0.1 GB: zoomed out over the hangar, no tree or plant scan streams. Note any `FullSite` within 150 m of the hangar if one shows.

---

### Task 2: Launch options (`--new`, `--vram`)

**Files:**
- Create: `src/launch.rs`
- Modify: `src/main.rs` (parse once, insert the resource), `src/save.rs:113-116` (`read_progress` reads the resource)

**Interfaces:**
- Produces: `crate::launch::LaunchOptions` — `#[derive(Resource, Clone, Debug, PartialEq)] pub struct LaunchOptions { pub new: bool, pub vram_gb: f32 }`, `impl Default` (`new: false`, `vram_gb: 6.0`), `pub fn parse(args: impl IntoIterator<Item = String>) -> Self`, `pub fn from_env() -> Self` (= `parse(std::env::args().skip(1))`). Phase 2 adds `detail` here.

- [ ] **Step 1: Write the failing tests** in `src/launch.rs`:

```rust
fn parse(args: &[&str]) -> LaunchOptions {
    LaunchOptions::parse(args.iter().map(|arg| arg.to_string()))
}

#[test]
fn defaults_are_a_saved_run_on_a_6_gb_card() {
    assert_eq!(parse(&[]), LaunchOptions { new: false, vram_gb: 6.0 });
}

#[test]
fn flags_parse_in_any_order() {
    assert_eq!(parse(&["--vram", "12", "--new"]), LaunchOptions { new: true, vram_gb: 12.0 });
    assert_eq!(parse(&["--new", "--vram", "7.5"]), LaunchOptions { new: true, vram_gb: 7.5 });
}

#[test]
fn a_bad_vram_value_keeps_the_default() {
    for args in [&["--vram", "abc"][..], &["--vram"], &["--vram", "-3"], &["--vram", "0"], &["--vram", "NaN"]] {
        assert_eq!(parse(args).vram_gb, 6.0, "{args:?}");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test launch::`
Expected: FAIL (module or `LaunchOptions` not found).

- [ ] **Step 3: Implement `LaunchOptions`.** Unknown arguments are ignored. A missing, unparsable, non-finite or non-positive `--vram` value logs `warn!` and keeps 6.0.

- [ ] **Step 4: Wire it.** In `main.rs`, `let launch = LaunchOptions::from_env();` and `.insert_resource(launch)` before the project plugins. In `save.rs`, `read_progress(mut commands: Commands, launch: Res<LaunchOptions>)` returns early on `launch.new` in place of reading `std::env::args()`.

- [ ] **Step 5: Run the tests and checks**

Run: `cargo test launch:: save:: && cargo clippy -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 6: Checkpoint:** suggest a commit message (`… add launch options with --vram next to --new`); commit only after the human confirms.

---

### Task 3: The RAM source

**Files:**
- Create: `src/assets/ram_source.rs` (native only: `#[cfg(not(target_arch = "wasm32"))] pub mod ram_source;` in `src/assets/mod.rs`)
- Modify: `src/assets/mod.rs` (`uhd`, `UhdFiles`), `src/main.rs` (register before `DefaultPlugins`, add `RamSourcePlugin`), `src/world/mod.rs` (insert `UhdFiles`), `src/world/{canopy,plants,sites}.rs` and `src/world/base/artifacts.rs` (each lists its UHD files), `src/ui/splash.rs` (`finish_loading` waits for the fill)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `crate::assets::uhd(path: &'static str) -> AssetPath<'static>`: `AssetPath::from(path).with_source(RAM_SOURCE)` on native, the plain path on `wasm32`. The only place that names the source.
  - `crate::assets::UhdFiles(pub Vec<&'static str>)`, `#[derive(Resource, Default)]`: every UHD file the game can show, in fill order (nearest the spawn first). `WorldPlugin` inserts it from `world::uhd_files()`.
  - `ram_source::RAM_SOURCE: &str = "ram"`; `#[derive(Resource, Clone)] RamDir(pub Dir)`; `pub fn register_ram_source(app: &mut App)` (registers `AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: dir.clone() }))` under `RAM_SOURCE` and inserts `RamDir`); `RamSourcePlugin` (Startup `start_ram_fill`, Update `collect_ram_files` while `AppState::Loading`); `#[derive(Resource)] RamFill` with `is_full(&self) -> bool`, `failed(&self) -> Option<&str>` and `file_lens(&self) -> &[(&'static str, u64)]`.
  - `world::uhd_files() -> Vec<&'static str>`: the five `TREE_*_UHD`, then the 14 artifact scene paths (`ARTIFACT_MODELS[..].path`), then every `PlantModel::full` of every kind, then each landmark's `model().path` sorted by distance from `terrain::BASE`; no duplicates. Each module exposes its own list (`pub(super) fn uhd_files() -> impl Iterator<Item = &'static str>`) and `world/mod.rs` chains them.

- [ ] **Step 1: Write the failing tests** in `src/assets/ram_source.rs`. The helper `filling_app(on_disk: &[(&str, &[u8])], listed: &[&'static str]) -> (App, RamDir)` builds the app:
  - `MinimalPlugins`;
  - `register_ram_source`, before `AssetPlugin { file_path: <a fresh temp dir>, ..default() }`;
  - `RamSourcePlugin` and `UhdFiles(listed)`.

  It writes `on_disk` into the temp dir with `std::fs`.

```rust
#[test]
fn uhd_paths_use_the_ram_source_on_native() {
    let path = crate::assets::uhd("models/trees/uhd/tree_big.glb");
    assert_eq!(path.source(), &AssetSourceId::from(RAM_SOURCE));
    assert_eq!(path.path(), Path::new("models/trees/uhd/tree_big.glb"));
}

#[test]
fn filling_reads_every_listed_file_into_ram() {
    let (mut app, ram) = filling_app(
        &[("models/a/uhd/one.glb", b"one"), ("models/b/uhd/two.glb", b"two")],
        &["models/a/uhd/one.glb", "models/b/uhd/two.glb"],
    );
    update_until(&mut app, |app| app.world().resource::<RamFill>().is_full());
    assert_eq!(ram.0.get_asset(Path::new("models/a/uhd/one.glb")).map(|d| d.value().to_vec()), Some(b"one".to_vec()));
    let read = block_on(read_ram(&app, "models/b/uhd/two.glb")); // through AssetServer::get_source(RAM_SOURCE).reader()
    assert_eq!(read, b"two");
}

#[test]
fn a_missing_file_fails_the_fill() {
    let (mut app, _) = filling_app(&[], &["models/x/uhd/missing.glb"]);
    update_until(&mut app, |app| app.world().resource::<RamFill>().failed().is_some());
    assert_eq!(app.world().resource::<RamFill>().failed(), Some("models/x/uhd/missing.glb"));
    assert!(!app.world().resource::<RamFill>().is_full());
}
```

`update_until` gives up after about 500 updates and fails the test.

In `src/world/mod.rs` tests, add `uhd_files_list_each_scan_once_trees_first`. It checks two things:
- the list has no duplicates;
- its first five entries are the `TREE_*_UHD` paths.

In `src/ui/splash.rs` tests, next to `ready_waits_until_the_asset_server_finishes_loading`, add `ready_waits_for_the_ram_fill`. Two cases:
- A `RamFill::test_pending()` keeps `SplashPhase::Loading`, even when every preloaded handle is loaded.
- A `RamFill::test_done(Some("models/x/uhd/missing.glb"))` sets `SplashPhase::Failed`.

Both constructors are `#[cfg(test)]`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test ram_source:: splash::`
Expected: FAIL (`register_ram_source`, `RamFill` not found).

- [ ] **Step 3: Implement the source and the fill.**
  - `start_ram_fill(mut commands, server: Res<AssetServer>, files: Res<UhdFiles>, ram: Res<RamDir>)` spawns one `IoTaskPool` task per file. Each task reads the file with `server.get_source(AssetSourceId::Default)?.reader().read(path)` and `read_to_end`, inserts it with `ram.0.insert_asset(path, bytes)` (the `Vec` moves in, so there is no copy), and returns `Result<(&'static str, u64), (&'static str, String)>`.
  - `collect_ram_files` polls the tasks with `check_ready`. It records each length and keeps the first failure. When the last task finishes, it logs `info!("Read {n} UHD files ({gb:.1} GB) into RAM in {s:.1} s")`.
  - Clone the `AssetServer` into each task; it is an `Arc`.

- [ ] **Step 4: Wire it.** In `main.rs`, on native, call `register_ram_source(&mut app)` before `.add_plugins(DefaultPlugins)`, then add `RamSourcePlugin`. `WorldPlugin::build` inserts `UhdFiles(uhd_files())`. `finish_loading` takes `fill: Option<Res<RamFill>>`: `None` (on the web) changes nothing; a failure logs `error!("Could not read {path} into RAM")` and sets `Failed`; until the fill is full, the splash stays pending.

- [ ] **Step 5: Run the tests and checks**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 6: Release check.** Run `XDG_DATA_HOME=$PWD/target/playtest/data cargo run --release -- --new`. Expected: the log shows `Read N UHD files (… GB) into RAM`, the splash turns ready after that, and the game plays exactly as before (nothing loads from `ram://` yet).

- [ ] **Step 7: Checkpoint:** suggest a commit message (`… read every UHD scan into an in-memory asset source at load`); commit only after the human confirms.

---

### Task 4: `GpuScans` budget, admission and eviction (pure logic)

**Files:**
- Create: `src/assets/gpu_scans.rs`

**Interfaces:**
- Consumes: `LaunchOptions::vram_gb` (Task 2); `GAME_VRAM_GB` (Task 1).
- Produces (all in `crate::assets::gpu_scans`):
  - Constants: `KEEP_SECONDS: f32 = 120.0`, `UPLOAD_LEAD: f32 = 40.0`, `MARGIN_GB: f32 = 0.5`, `DESKTOP_GB: f32 = 0.6` (fallback only; 0.63 GiB measured on the 2060), `GAME_VRAM_GB: f32 = <Task 1>`, `DEFAULT_VRAM_GB` (6.0, from `crate::launch`), `GPU_BYTES_PER_FILE_BYTE: f32 = 1.65` (the five trees: about 700 MB on the GPU from 428 MB of files).
  - `pub struct VramReading { pub budget: u64, pub usage: u64 }`: the driver's device-local heap budget for this process and this process's usage, in bytes. Task 5 reads it.
  - `pub fn live_scan_budget(reading: VramReading, resident_scans: u64, cap: Option<u64>) -> u64`: `max(0, min(reading.budget, cap) − (reading.usage − resident_scans) − MARGIN_GB)`. `usage − resident_scans` (saturating) is everything else the game holds.
  - `pub fn scan_budget(vram_gb: f32) -> u64`: the fallback when the driver cannot report VRAM: `max(0, vram_gb − GAME_VRAM_GB − DESKTOP_GB − MARGIN_GB)` GiB, in bytes.
  - `#[derive(Resource, Default)] pub struct GpuScans` (the default has a budget of 0) with `new(budget: u64) -> Self` and:
    - `set_budget(&mut self, budget: u64)`: a new live budget; the next `plan` evicts down to it if needed.
    - `resident_bytes(&self) -> u64`: the bytes of scans loaded and measured (not those still loading).
    - `pin(&mut self, path: &'static str)`: loads before every unpinned scan and never leaves for one, but only within the budget.
    - `want(&mut self, path: &'static str, distance: f32)`: this frame, keeping the smallest distance when several callers ask.
    - `estimate(&mut self, path: &'static str, file_len: u64)`: sets the size to `file_len × GPU_BYTES_PER_FILE_BYTE` until it is measured.
    - `plan(&mut self, now: f32) -> Admission`: stamps every scan wanted this frame as used at `now` and clears the wants. It decides what to load and what to evict, marks loads as *loading* and evictions as *off*, and counts both in `used` at once.
    - `measured(&mut self, path: &'static str, bytes: Option<u64>)`: the load finished. It replaces the estimate when `bytes` is `Some` and keeps that size for any later reload.
    - `next_prefill(&self, order: &[&'static str]) -> Option<&'static str>`: the first file in `order` that is off and fits the free room. It returns `None` while any scan is loading or any wanted scan waits.
    - `used(&self) -> u64`, `budget(&self) -> u64`.
    - Task 5 adds the handles, `scene` and `settled`.
  - `pub struct Admission { pub load: Vec<&'static str>, pub evict: Vec<&'static str> }`.

- [ ] **Step 1: Write the failing tests** in `src/assets/gpu_scans.rs`. Two `#[cfg(test)]` helpers:
  - `estimate_exact(path, bytes)` sets the size directly.
  - `fake_loaded(scans, path, bytes, last_used)` wants the scan, runs `plan(last_used)` with room, then calls `measured(path, Some(bytes))`.

```rust
const MB: u64 = 1 << 20;

#[test]
fn the_fallback_budget_is_vram_minus_the_game_the_desktop_and_a_margin() {
    let gib = |gb: f32| (gb * (1u64 << 30) as f32) as u64;
    assert_eq!(scan_budget(GAME_VRAM_GB + DESKTOP_GB + MARGIN_GB + 2.0), gib(2.0));
    assert_eq!(scan_budget(1.0), 0);
}

#[test]
fn the_live_budget_is_what_the_driver_leaves_minus_a_margin() {
    let gib = |gb: f32| (gb * (1u64 << 30) as f32) as u64;
    // T1200: driver budget 3.9 GiB, game uses 3.0 GiB of which 0.5 GiB are scans
    let reading = VramReading { budget: gib(3.9), usage: gib(3.0) };
    assert_eq!(live_scan_budget(reading, gib(0.5), None), gib(3.9 - 2.5 - MARGIN_GB));
    assert_eq!(live_scan_budget(reading, gib(0.5), Some(gib(3.5))), gib(3.5 - 2.5 - MARGIN_GB));
    assert_eq!(live_scan_budget(VramReading { budget: gib(2.0), usage: gib(3.0) }, 0, None), 0);
}

#[test]
fn a_shrunk_budget_evicts_scans_not_shown_least_recently_used_first() {
    let mut scans = GpuScans::new(400 * MB);
    fake_loaded(&mut scans, "old", 100 * MB, 0.0);
    fake_loaded(&mut scans, "newer", 100 * MB, 10.0);
    fake_loaded(&mut scans, "shown", 100 * MB, 0.0);
    fake_loaded(&mut scans, "pinned", 100 * MB, 0.0);
    scans.pin("pinned");
    scans.set_budget(250 * MB);              // another program took VRAM at t = 20 s
    scans.want("shown", 5.0);
    assert_eq!(scans.plan(20.0).evict, ["old"]);   // one per reading, ignores KEEP_SECONDS
    scans.want("shown", 5.0);
    assert_eq!(scans.plan(21.0).evict, ["newer"]); // now 200 MB <= 250 MB
    scans.want("shown", 5.0);
    assert!(scans.plan(22.0).evict.is_empty());
}

#[test]
fn wanted_scans_load_nearest_first_within_the_budget() {
    let mut scans = GpuScans::new(300 * MB);
    for (path, bytes) in [("a", 200 * MB), ("b", 200 * MB), ("c", 50 * MB)] { scans.estimate_exact(path, bytes); }
    scans.want("a", 30.0); scans.want("b", 10.0); scans.want("c", 20.0);
    assert_eq!(scans.plan(0.0).load, ["b", "c"]); // a waits
}

#[test]
fn pinned_scans_load_first_but_only_within_the_budget() {
    let mut scans = GpuScans::new(750 * MB);
    scans.estimate_exact("tree", 700 * MB);
    scans.estimate_exact("site", 100 * MB);
    scans.pin("tree");
    scans.want("site", 5.0);
    assert_eq!(scans.plan(0.0).load, ["tree"]);   // the site waits for room

    let mut small = GpuScans::new(300 * MB);      // a card too small for the pin
    small.estimate_exact("tree", 700 * MB);
    small.estimate_exact("site", 100 * MB);
    small.pin("tree");
    small.want("site", 5.0);
    assert_eq!(small.plan(0.0).load, ["site"]);   // the tree waits (warn once)
}

#[test]
fn a_scan_used_within_two_minutes_never_leaves() {
    let mut scans = GpuScans::new(300 * MB);
    fake_loaded(&mut scans, "a", 250 * MB, 0.0);
    scans.want("a", 50.0);
    scans.plan(100.0);                       // still near an anchor at t = 100 s
    scans.estimate_exact("b", 100 * MB);
    scans.want("b", 10.0);                   // b needs a's room at t = 150 s
    let admission = scans.plan(150.0);
    assert!(admission.evict.is_empty() && admission.load.is_empty());
}

#[test]
fn the_least_recently_used_scan_leaves_first_once_idle_for_two_minutes() {
    let mut scans = GpuScans::new(300 * MB);
    fake_loaded(&mut scans, "old", 100 * MB, 0.0);
    fake_loaded(&mut scans, "newer", 100 * MB, 10.0);
    fake_loaded(&mut scans, "pinned", 100 * MB, 0.0);
    scans.pin("pinned");
    scans.estimate_exact("b", 100 * MB); scans.want("b", 10.0);
    let admission = scans.plan(200.0);
    assert_eq!((admission.evict, admission.load), (vec!["old"], vec!["b"]));
}

#[test]
fn nothing_leaves_unless_the_new_scan_then_fits() {
    let mut scans = GpuScans::new(300 * MB);
    fake_loaded(&mut scans, "idle", 100 * MB, 0.0);
    scans.estimate_exact("huge", 400 * MB); scans.want("huge", 1.0);
    let admission = scans.plan(500.0);
    assert!(admission.evict.is_empty() && admission.load.is_empty());
}

#[test]
fn an_evicted_scan_comes_back_when_wanted_again() {
    // fake_loaded "a" (100 MB, measured) at 0 s; at 200 s a wanted 300 MB "b"
    // evicts it (budget 300 MB); after "b" is idle and freed, want "a" again:
    // plan(..).load == ["a"] and used() grows by exactly its measured 100 MB
}

#[test]
fn prefill_takes_the_next_file_that_fits_only_while_nothing_waits() {
    // order ["big", "small"]; free room fits only "small" -> Some("small");
    // with a wanted scan waiting for room -> None; with a scan loading
    // (planned but not yet `measured`) -> None
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test gpu_scans::`
Expected: FAIL.

- [ ] **Step 3: Implement `plan`.** It is the one algorithm the tests leave open:

```text
if used > budget:   (the live budget shrank)
    evict the first loaded scan not wanted this frame, unpinned before pinned,
    oldest last_used first, ignoring KEEP_SECONDS; one per plan
    (a reading's numbers lag the GPU frees), and load nothing this plan
pending = scans not loaded that are pinned or wanted this frame;
          pinned first, then by distance
for each pending scan:
    fits in budget - used -> load
    pinned, does not fit  -> waits (warn once per scan); it never evicts others
    otherwise: idle = loaded, not pinned, now - last_used >= KEEP_SECONDS,
                      oldest last_used first
               if freeing all of idle would make it fit:
                   evict idle scans in order until it fits, then load
               else: it waits (evict nothing for it)
```

Evicted and loaded scans update `used` as the loop goes, so a later pending scan sees the room left.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test gpu_scans::`
Expected: PASS.

- [ ] **Step 5: Checkpoint:** suggest a commit message (`… add the GPU scan budget with idle-only LRU eviction`); commit only after the human confirms.

---

### Task 5: Loading, residency and the `--vram` wiring

**Files:**
- Create: `src/assets/gpu_scans/residency.rs`, `src/assets/gpu_scans/vram.rs`
- Modify: `src/assets/gpu_scans.rs` (handles, `scene`, `settled`, systems, `GpuScansPlugin`), `src/launch.rs` (`vram_gb` becomes optional), `Cargo.toml` (`ash`, native only), `src/main.rs` (add `GpuScansPlugin` after the `LaunchOptions` insert), `README.md` ("Graphics quality"), `CLAUDE.md` (Controls)

**Interfaces:**
- Consumes: `GpuScans` (with `set_budget`, `resident_bytes`), `Admission`, `scan_budget`, `live_scan_budget`, `VramReading` (Task 4); `uhd`, `UhdFiles`, `RamFill::file_lens` (Task 3); `LaunchOptions` (Task 2).
- Produces:
  - `GpuScans::scene(&self, path: &'static str) -> Option<Handle<WorldAsset>>`: `Some` only once every mesh and image of the scan is prepared in the render world.
  - `GpuScans::settled(&self) -> bool`: no scan loading or waiting to be confirmed, and every pinned scan on the GPU.
  - `residency::ScanParts { pub meshes: Vec<AssetId<Mesh>>, pub images: Vec<AssetId<Image>> }`.
  - `residency::scan_bytes(parts: &ScanParts, meshes: &Assets<Mesh>, images: &Assets<Image>) -> Option<u64>`: `RenderMesh::byte_len` plus `GpuImage::byte_len` (both from `bevy::render::render_asset::RenderAsset`). It returns `None` if any mesh fails `try_attributes()` or any image has `data == None`.
  - `#[derive(Resource, Clone, Default)] residency::ScanResidency(pub Arc<Mutex<Residency>>)`, where `Residency { pub waiting: HashMap<&'static str, ScanParts>, pub ready: HashSet<&'static str> }`. It is inserted into the main and render apps, like the curtain's `PipelineBacklog`.
  - `residency::settle(residency: &mut Residency, mesh_ready: impl Fn(AssetId<Mesh>) -> bool, image_ready: impl Fn(AssetId<Image>) -> bool)`: moves each waiting scan whose parts are all ready into `ready`.
  - `GpuScansPlugin`: inserts `GpuScans::new(scan_budget(launch.vram_gb.unwrap_or(DEFAULT_VRAM_GB)))` (the fallback until the first live reading) and adds the systems below. *(Amended in Task 9: Ruling 30 dropped the `RenderAssetBytesPerFrame::new(32 * 1024 * 1024)` insert this step first had, so Bevy's default, no limit, stays. The limit spread the fog-of-war mask and the stalker's ring over several frames too, so they flashed and blinked. Task 9's traces measured what it would have smoothed: each scan upload after the curtain holds one frame for 103-147 ms, five in the walk and one in the drive, none in the wood run.)*
  - `LaunchOptions::vram_gb: Option<f32>`: `None` without the flag; a malformed value warns and gives `None`. Update Task 2's tests to match.
  - `vram::heap_reading(heaps: &[(bool, u64)], budgets: &[u64], usages: &[u64]) -> Option<VramReading>`: pure. `heaps` holds each heap's (device-local, size). It reads the largest device-local heap's budget and usage. It returns `None` when there is no such heap or its budget is 0 (the extension is off).
  - `vram::read_vram(device: &RenderDevice) -> Option<VramReading>` (native only; `None` on wasm32 and on every backend but Vulkan):
    - get the hal device with `unsafe { device.wgpu_device().as_hal::<wgpu::hal::api::Vulkan>() }`;
    - call `shared_instance().raw_instance().get_physical_device_memory_properties2(raw_physical_device(), ..)` with `vk::PhysicalDeviceMemoryBudgetPropertiesEXT` chained (the same call wgpu-hal 30 makes in `vulkan/device.rs` `get_memory_properties`);
    - pass the result to `heap_reading`.

    Each `unsafe` block gets a `// SAFETY:` comment.
  - `track_vram_budget(scans: ResMut<GpuScans>, device: Res<RenderDevice>, launch: Res<LaunchOptions>, time: Res<Time<Real>>, next: Local<f32>)` in `First`, run while `AppState::InGame`:
    - About once a second it calls `read_vram`.
    - On `Some`, it calls `set_budget(live_scan_budget(reading, scans.resident_bytes(), cap))`, where `cap` is `launch.vram_gb` in bytes.
    - On `None`, it keeps the fallback.
    - The first time it logs `info!("VRAM: driver budget {b:.1} GB, game using {u:.1} GB; UHD scans may use {s:.1} GB")`, or `warn!` once that the driver cannot report VRAM and the game assumes `--vram` or 6 GB.

- [ ] **Step 1: Write the failing tests** in `residency.rs`:

```rust
#[test]
fn settle_moves_scans_whose_parts_are_all_on_the_gpu() {
    // scan "a": meshes [m1], images [i1, i2]; scan "b": meshes [m2]
    // mesh_ready: m1, m2; image_ready: i1 only
    settle(&mut residency, |m| [m1, m2].contains(&m), |i| i == i1);
    assert_eq!(residency.ready, HashSet::from(["b"]));
    assert!(residency.waiting.contains_key("a"));
}

#[test]
fn scan_bytes_counts_vertices_indices_and_texels() {
    let mesh = Mesh::from(Cuboid::default());
    let image = Image::new_fill(Extent3d { width: 4, height: 4, depth_or_array_layers: 1 }, TextureDimension::D2, &[0; 4], TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    let expected = RenderMesh::byte_len(&mesh).unwrap_or(0) + 64;
    // add both to Assets, build ScanParts with their ids
    assert_eq!(scan_bytes(&parts, &meshes, &images), Some(expected as u64));
}

#[test]
fn the_largest_device_local_heap_is_read() {
    // in vram.rs: heaps [(false, 16 GiB host), (true, 4 GiB vram), (true, 256 MiB bar)]
    // budgets [15, 3.9 GiB, 0.2 GiB], usages [1, 3.0 GiB, 0.1 GiB]
    // -> Some(VramReading { budget: 3.9 GiB, usage: 3.0 GiB });
    // the same with the device-local budget 0 -> None; no device-local heap -> None
}

#[test]
fn scan_bytes_is_unknown_once_a_mesh_has_moved_to_the_gpu() {
    let mut mesh = Mesh::from(Cuboid::default());
    let _ = mesh.take_gpu_data();
    // add it to Assets<Mesh>, parts with its id
    assert_eq!(scan_bytes(&parts, &meshes, &images), None);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test gpu_scans::`
Expected: FAIL.

- [ ] **Step 3: Implement loading and the systems.**
  - Load each admitted scan twice, with the same settings both times: `server.load_builder().with_settings(render_only).load::<Gltf>(uhd(path))` and `….load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(uhd(path)))`. Here `render_only` sets `load_meshes` and `load_materials` to `RenderAssetUsages::RENDER_WORLD`, so images follow `load_materials`. `GpuScans` keeps both strong handles. Evicting drops them and removes the scan from `ScanResidency`; Bevy then frees the GPU copy when it sends `AssetEvent::Unused`.
  - `admit_scans(scans, server, time: Res<Time<Real>>, residency: Res<ScanResidency>)` in `Last`, run while `AppState::InGame`: calls `plan(time.elapsed_secs())`, applies it and logs `info!("UHD scan {path} left the GPU")` for each eviction. It only runs in game, so no `ram://` load starts before the fill is full.
  - `measure_scans(MessageReader<AssetEvent<Gltf>>, …)` in `Update`. On `LoadedWithDependencies` for a tracked scan, it collects the parts: meshes through `gltf.meshes` → `GltfMesh::primitives` → `mesh`; images through `server.get_path_id(GltfAssetLabel::Texture(i).from_asset(uhd(path)))` for `i = 0..` until `None`. It calls `scans.measured(path, scan_bytes(..))`, warning when the bytes are `None`, and puts the parts into `residency.waiting`. It runs in the frame the scan is inserted, before render extract moves the data. On `LoadState::Failed` it logs an error and calls `measured(path, None)`, so the scan does not stay loading forever; its mid tier keeps showing.
  - `publish_scan_residency(residency: Res<ScanResidency>, meshes: Res<RenderAssets<RenderMesh>>, images: Res<RenderAssets<GpuImage>>)` in `Render`, `.in_set(RenderSystems::PrepareResources)`: calls `settle` with `meshes.get(id).is_some()` and `images.get(id).is_some()`.
  - `note_resident_scans` in `First` copies `ready` into each scan's resident flag. On the first frame resident, it logs `info!("UHD scan {path} on the GPU ({mb} MB); {used:.1} of {budget:.1} GB")`.
  - `GpuScansPlugin` also records `RamFill::file_lens` into `estimate` once the fill is full (native only).

- [ ] **Step 4: Run the tests and checks**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Document `--vram`.** In `README.md`, next to the run commands, add the spec's "Graphics quality" section with the `--vram` item only:
  - It says the game reads every UHD model once at start and keeps it in RAM, so the first loading screen takes longer.
  - It gives the RAM in GB that Task 9 measures.
  - It says the game keeps as many close-up models on the GPU as the driver says fit, and frees the least recently seen ones when memory runs short.
  - It says `--vram <GB>` caps that: use it to leave video memory for other programs, or where the driver cannot report it (then the game assumes 6 GB).
  - Example: `cargo run --release -- --vram 3`, which combines with `--new`.

  Phase 2 adds `--detail` and the combined example. In `CLAUDE.md` Controls, next to `--new`, add one line: `cargo run --release -- --vram <GB>` caps the video memory the game uses (by default, what the driver reports free).

- [ ] **Step 6: Checkpoint:** suggest a commit message (`… load UHD scans render-only from RAM and track their GPU residency`); commit only after the human confirms.

---

### Task 6: Pins, prefill and the curtain

**Files:**
- Modify: `src/world/canopy.rs` (pin the five trees), `src/assets/gpu_scans.rs` (`prefill_scans`), `src/ui/curtain.rs` (wait for `settled`, log the lift)

**Interfaces:**
- Consumes: `GpuScans::{pin, next_prefill, settled}`, `UhdFiles` (Tasks 3–5).
- Produces:
  - `canopy::pin_tree_scans(mut scans: ResMut<GpuScans>)` (Startup) pins `TREE_SMALL_UHD`, `TREE_BIG_UHD`, `TREE_VERY_TALL_UHD`, `TREE_BIRCH_UHD` and `TREE_DEAD_UHD`. The stalker stays preloaded as today. The armored car and the hangar shell join the pins in phase 3, when they get UHD scans.
  - `prefill_scans` runs in `Last`, after `admit_scans`.
  - `info!("Entered the Zone")` is logged when the curtain lifts; Task 9's `strace` check keys on it.

- [ ] **Step 1: Write the failing test** in `src/ui/curtain.rs`, next to `curtain_waits_for_plants_to_spawn_then_lifts`:

```rust
#[test]
fn curtain_waits_for_pinned_scans_then_lifts() {
    // entered_app() plus a GpuScans with one pinned scan not yet resident
    // step past SETTLE_SECONDS: the curtain is still up
    // mark the scan resident (#[cfg(test)] GpuScans::insert_resident(path, Handle::default()))
    // step past SETTLE_SECONDS again: the curtain is gone
}
```

`entered_app()` inserts a default `GpuScans` so the existing curtain tests keep passing.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test curtain::`
Expected: FAIL (the curtain lifts without waiting).

- [ ] **Step 3: Implement.**
  - `lift_curtain` adds `scans: Res<GpuScans>` and counts `!scans.settled()` as busy. `MAX_SECONDS` still caps the wait: a tree still loading after 12 s shows its crown until it is resident.
  - `prefill_scans(mut scans: ResMut<GpuScans>, files: Res<UhdFiles>, server: Res<AssetServer>)` loads `next_prefill(&files.0)`, at most one per frame.
  - Register `pin_tree_scans` in `CanopyPlugin`.

- [ ] **Step 4: Run the tests and checks**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Release check.** Run `cargo run --release -- --new`. Expected:
  - The five `UHD scan … on the GPU` lines appear before `Entered the Zone`.
  - Then prefill lines follow (artifacts, then plants), stopping at the budget.
  - The curtain lifts no later than before (compare with the baseline's warm `ready`, plus at most a few seconds).

- [ ] **Step 6: Checkpoint:** suggest a commit message (`… pin the UHD trees behind the curtain and prefill the rest`); commit only after the human confirms.

---

### Task 7: Route tree close-ups and the plant scan

**Files:**
- Modify: `src/world/canopy.rs` (`stream_close_trees` and its tests), `src/world/plants.rs` (`stream_full_plants`, new tests)

**Interfaces:**
- Consumes: `GpuScans::{want, scene}`, `UPLOAD_LEAD` (Tasks 4–5); `#[cfg(test)] GpuScans::insert_resident` (Task 6).
- Produces: no new public items. `stream_close_trees` and `stream_full_plants` take `Res<GpuScans>` / `ResMut<GpuScans>` in place of `Res<AssetServer>`.

- [ ] **Step 1: Write the failing tests.**
  - In `canopy.rs`, the `streaming()` helper inserts a `GpuScans` with `TREE_SMALL_UHD` resident, so the existing tests keep their meaning. Add:

```rust
#[test]
fn a_tree_waits_for_its_scan_on_the_gpu() {
    // streaming() but with an empty GpuScans; camera 3 m from a 12 m tree
    app.update();
    assert!(app.world().entity(tree).get::<CloseScan>().is_none());
    assert_eq!(start_margin(&app, tree), 0.0..0.0, "the crown stays");
}
```

  - In `plants.rs` tests, build an app with `stream_full_plants`, a `Player` with a `Transform`, a tile entity, and a `Plant` whose model has a `full` scan (a shrub). Then add:

```rust
#[test]
fn plants_within_40_m_ask_for_their_scans() {
    // plant 30 m from the player: no FullPlant spawned; after the update,
    // GpuScans::plan(0.0) lists the shrub's full path in `load` (budget large)
}

#[test]
fn the_nearest_plant_shows_its_scan_only_once_on_the_gpu() {
    // plant 5 m away, scan not resident: no FullPlant
    // insert_resident(full, handle): next update spawns one FullPlant for it
}

#[test]
fn a_shown_scan_is_wanted_every_frame() {
    // after the FullPlant spawns, each update still wants the full path
    // (plan(now).load is empty because it is loaded, but last_used == now)
}
```

  For the last test, give `GpuScans` a `#[cfg(test)] fn last_used(&self, path) -> Option<f32>`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test canopy:: plants::`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement.**
  - `stream_close_trees` spawns a `CloseScan` only when `scans.scene(visual.close)` is `Some` and uses that handle in `WorldAssetRoot`. The trees are pinned, so it does not call `want`.
  - `stream_full_plants` wants `plant.model.full` for every plant within `UPLOAD_LEAD` of the player (distance from the player), and still picks the nearest within `FULL_RANGE`. It spawns a `FullPlant` only with `scans.scene(full)`. Until then, the plant's own model stays visible.

- [ ] **Step 4: Run the tests and checks**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Release check.** Walk the stalker out of the hangar to the shelterbelt and stand next to a tree and a shrub. Expected:
  - The tree's full scan crossfades in within 8 m as before.
  - The shrub swaps to its scan within 8 m.
  - The log shows no `UHD scan … left the GPU` while walking near the hangar.

- [ ] **Step 6: Checkpoint:** suggest a commit message (`… show tree and plant scans from the GPU budget`); commit only after the human confirms.

---

### Task 8: Route sites, landmarks and artifact scenes

**Files:**
- Modify: `src/world/sites.rs` (`stream_full_sites`, new tests), `src/world/base/artifacts.rs` (`stream_full_artifacts`, new tests)

**Interfaces:**
- Consumes: `GpuScans::{want, scene}`, `UPLOAD_LEAD` (Tasks 4–5); `insert_resident`, `last_used` (test helpers).
- Produces: no new public items.

- [ ] **Step 1: Write the failing tests.**
  - `sites.rs`: build an app with `stream_full_sites`, one `SiteDisplay` for `LANDMARKS[0]` with a `FarSite` child, and a `StreamAnchor`. Let `reach = model().radius() + FULL_SCAN_RANGE`.

```rust
#[test]
fn a_site_asks_for_its_scan_40_m_before_it_shows() {
    // anchor at reach + 30 m: no FullSite, but the scan is wanted
    // anchor at reach + 50 m: not wanted
}

#[test]
fn a_site_shows_its_scan_once_on_the_gpu_and_keeps_wanting_it() {
    // anchor at reach - 10 m, not resident: no FullSite, the stand-in visible
    // insert_resident: a FullSite spawns
    // anchor at reach + 55 m (inside FULL_SCAN_KEEP): still shown and still wanted
    // anchor at reach + 61 m: FullSite despawned, the stand-in visible again
}
```

  - `artifacts.rs`: build an app with `stream_full_artifacts`, two `ArtifactDisplay` tables 5 m and 30 m from the `Player`, each with its `ArtifactPreview`:

```rust
#[test]
fn artifacts_within_40_m_ask_for_their_scenes_and_the_nearest_shows_once_on_the_gpu() {
    // both scenes wanted; nothing shown until the near one is resident
    // insert_resident(near path): FullArtifact spawns for the near table only,
    // and its scene stays wanted on the next update
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test sites:: base::artifacts::`
Expected: the new tests FAIL.

- [ ] **Step 3: Implement.**
  - `stream_full_sites` wants `landmark.model().path` while a `FullSite` exists or the nearest anchor is within `reach + UPLOAD_LEAD`, with distance `nearest − reach`. It spawns the `FullSite` with `scans.scene(path)` only when `nearest < reach`, and despawns it past `reach + FULL_SCAN_KEEP` as today.
  - `stream_full_artifacts` wants `ARTIFACT_MODELS[index].path` for every table within `UPLOAD_LEAD` of the player, and spawns the nearest within `FULL_ARTIFACT_RANGE` only with `scans.scene(path)`.
  - `reveal_full_artifact`'s golden-sphere material edit still works: `StandardMaterial` stays in the main world under render-only usages. Only meshes and images move to the render world.
  - Until phase 3 points `ARTIFACTS` at the `uhd/` scans, the scene paths are the same `medium/` files as the previews. So a shown artifact gets a second, render-only copy from `ram://`. That copy is small, and it keeps post-curtain disk reads at zero.

- [ ] **Step 4: Run the tests and checks**

Run: `cargo test && cargo clippy -- -D warnings && cargo fmt -- --check`
Expected: PASS.

- [ ] **Step 5: Release check.** Walk to the hangar's exhibit rows, then drive to the garage site. Expected:
  - Each exhibit swaps to its scene within 8 m, and the golden sphere is still mirror-like.
  - The garage swaps from its stand-in to the full scan before the car is within 150 m of it, with no visible wait.

- [ ] **Step 6: Checkpoint:** suggest a commit message (`… show sites and artifact scenes from the GPU budget`); commit only after the human confirms.

---

### Task 9: Measure, tune `GAME_VRAM_GB`, document

**Files:**
- Modify: `src/assets/gpu_scans.rs` (`GAME_VRAM_GB`, only if the measurement says so), `CLAUDE.md` (a "RAM and GPU" bullet under Procedural content; the Tree and Plants bullets' "not preloaded … loads a tree's scan" wording), `arch-readme.md` (runtime graph and the asset-loading recipe), `README.md` (the RAM figure)
- Output: `target/playtest/phase1/after.txt`

**Interfaces:**
- Consumes: everything above; `baseline.txt` (Task 1).

- [ ] **Step 1: Rebuild the traced binary and re-run Task 1's four scenarios** with the same steps, VRAM sampling, `/usr/bin/time -v` and `strace` (on the walk and the drive).

- [ ] **Step 2: Check the targets.**

Expected:
  - The `main app` max is under 33 ms in the walk and drive windows. The garage frame (41 ms at baseline) is lower, with no 1.1 s load before it.
  - No `assets/` path is opened in `walk.strace` or `drive.strace` after the `Entered the Zone` log time. Convert the log's UTC stamp with `date -d … +%s.%N`.
  - Peak VRAM minus the desktop stays under the 6 GB card, and the log's `VRAM: driver budget …` line shows the live budget was read (not the fallback).
  - There is no `left the GPU` line followed by the same scan coming back within the run.
  - **The T1200 replay (Review Focus 6).** Run agentic-tooling `testing/vram-hog`, holding enough of the 2060 that the game's driver budget is about 4 GiB. Replay the user's steps: zoom fully out, in, out again, then free roam over at least three sites under 250 m of zoom. Record nvidia-smi every 500 ms. Expected:
    - no crash;
    - the game's usage never rises above its driver budget;
    - after each zoom-out, the game's VRAM is no higher than after the previous one (no leak);
    - `left the GPU` lines appear when room runs short.

    Repeat once without the hog, to show the 2060 then keeps more scans.

  If a frame is over 33 ms, find its span with `summarize.py --match`. If it is a single upload, note its size: the spec allows splitting only above the 4 MiB own-buffer threshold (32 MiB when this plan was written; Task 4b lowered it), so record it for phase 2 and do not widen this phase.

- [ ] **Step 3: Tune the fallback budget.** `GAME_VRAM_GB` now only feeds the fallback for hosts whose driver cannot report VRAM. Find the overview run's `on the GPU … {used} of {budget} GB` line at the VRAM peak. Set `GAME_VRAM_GB` to the peak VRAM minus `used`, rounded up to 0.1 GB, if it differs from Task 1's value by more than 0.2 GB. No re-run is needed: the live budget does not use it.

- [ ] **Step 4: Document.**
  - README: the measured RAM figure (peak RSS) and the VRAM size from which every scan stays on the GPU.
  - CLAUDE.md: the "RAM and GPU" bullet. It covers the `ram://` source filled at load, `GpuScans` (want, pin, budget, two-minute idle eviction, 40 m lead, residency back-channel) and the rule that every UHD load goes through `assets::uhd`. Correct the tree and plant wording.
  - arch-readme: the same in the runtime graph, plus a pitfall: a render-only mesh panics on `attributes()` after extract, so read vertices before it reaches the GPU.

- [ ] **Step 5: Final checks**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings && cargo test && git diff --check`
Expected: all pass. If the `wasm32-unknown-unknown` target is installed (`rustup target list --installed`), also run `cargo check --target wasm32-unknown-unknown`; otherwise report that it was not checked. Stop the Xephyr you started, delete the raw `*.json` traces and keep `baseline.txt`, `after.txt` and the summaries.

- [ ] **Step 6: Report and suggest the commit message.** Give before and after tables for:
  - frame max and p99 per scenario;
  - warm and cold `ready` time;
  - peak RSS and peak VRAM;
  - `assets/` opens after the curtain.

  Suggest a commit message prefixed `worktree-vegetation-lod-design:`, at most 10 lines, with the main numbers. Do not commit.
