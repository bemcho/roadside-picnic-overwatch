# Review of phase 1 (UHD from RAM, GPU budget): room for improvement

Date: 2026-11-23. Reviewed: branch `worktree-vegetation-lod-design` at `e34c6b8`, plus the uncommitted
work in the tree at 18:15 (the eviction-cascade fix in `src/assets/gpu_scans.rs` and its callers).

## How this review was done

Read, not run:

- the code the branch adds or changes: `src/assets/{gpu_scans,ram_source}.rs` and their submodules,
  `src/launch.rs`, and the diffs to `canopy.rs`, `plants.rs`, `sites.rs`, `curtain.rs`, `splash.rs`,
  `world/mod.rs`, `assets/mod.rs`, `main.rs`;
- the specs and plans in `docs/superpowers/`;
- the measurements in `target/playtest/phase1/after.txt`;
- the sources of Bevy 0.20.1, wgpu 30.0.2 and gpu-allocator 0.28.0 in `~/.cargo/registry`, to check
  what the engine really does. Paths below that start with a crate name are in
  `~/.cargo/registry/src/index.crates.io-*/`.

Nothing was built, run or measured for this review. Every proposal that changes behaviour is marked as
an experiment to measure before it is adopted. Byte and time savings are estimates unless they cite
`after.txt`.

## What is in good shape

- The budget logic is pure (`GpuScans::plan` takes a clock and returns an `Admission`), so its 40 or so
  rules are unit-tested without an asset server. 277 tests passed at `84186cb`.
- Scans show only once the render world confirms them (`ScanResidency`), so nothing pops in half
  uploaded.
- The work was measured before and after, with the scripts kept in `target/playtest/`.
- Goal 5 is met: no `assets/` path is opened after the curtain (`after.txt` §3: walk 2 → 0, wood 56 → 0).
- The wood run's worst frame fell from 86.6 to 18.7 ms (`after.txt` §1).

## Summary

| # | Area | Finding | Size of the change |
|---|---|---|---|
| 1 | GPU budget | The eviction cascade comes from wgpu's 256 MiB memory blocks; one renderer setting may remove the cause | 5 lines, then an experiment |
| 2 | GPU budget | wgpu can report exact allocated and reserved bytes, if 1 is not enough | about 30 lines |
| 3 | GPU budget | Over budget, a scan that is wanted can never leave | about 10 lines and a test |
| 4 | GPU budget | No VRAM reading is taken while any scan loads; prefill keeps one loading all the time | about 15 lines and a test |
| 5 | GPU budget | On a small card the pinned trees take the whole budget and no site ever shows its scan | about 10 lines and a test |
| 6 | Hitches | Prefill keeps uploading after the curtain lifts; each upload freezes rendering 103–147 ms | about 20 lines |
| 7 | Hitches, VRAM | Bevy can compress scan vertices at load: 20 bytes per vertex instead of 48 | 1 line, then an experiment |
| 8 | Hitches | The upload freeze itself is inside Bevy; only fewer or lighter uploads help | a decision, no code |
| 9 | Load time | Three IO threads decode every GLB with pure-Rust zstd and also run the RAM fill | a Cargo feature and a plugin setting |
| 10 | Per frame | `GpuScans::want` is called once per plant within 40 m | about 10 lines |
| 11–15 | Process | No upstream, five topics on one branch, a merge conflict with main, wasm never compiled, a doc bullet that repeats constants | see below |

Findings 1 and 2 bear on the cascade fix that is being written in this tree now.

---

## GPU budget

### 1. The eviction cascade has a root cause that one renderer setting may remove

**What was measured.** `after.txt` §4: once the game's own VRAM grew by one allocator step, a scan was
evicted, the driver's usage did not fall, the next reading counted the freed bytes as "everything else",
the budget shrank by the scan's size, and the next scan left. Budget 1627 → 1488 → 1354 → 1223 → 1090 MB;
all five pinned trees left; nvidia-smi stayed at 5255 MiB.

**Why the usage does not fall.** wgpu's Vulkan backend allocates through gpu-allocator:

- The block sizes come from `MemoryHints`. The default, `Performance`, uses 128–256 MiB blocks for
  device memory (`wgpu-hal-30.0.2/src/lib.rs:439-450`). The block size is `min << count`, capped at the
  maximum (`gpu-allocator-0.28.0/src/lib.rs:424-436`), so the first block is 128 MiB and every later one
  256 MiB. This is the 256 MiB step the comments in `gpu_scans.rs` describe.
- A block that sub-allocates is freed only when it is empty
  (`gpu-allocator-0.28.0/src/vulkan/mod.rs:658-682`). A scan's buffers and textures share blocks with
  the rest of the game, so a block rarely empties when a scan leaves.
- An allocation larger than the current block size gets a block of its own
  (`requires_personal_block = size > memblock_size`, `vulkan/mod.rs:485-488`), and that block is
  destroyed when the allocation is freed.

**The setting.** Bevy passes `WgpuSettings::memory_hints` to wgpu
(`bevy_render-0.20.1/src/settings.rs:61`, `renderer/mod.rs:358`). The game does not set it:
`src/main.rs:34` adds `DefaultPlugins` unchanged.

```rust
// src/main.rs
const SCAN_BLOCK: u64 = 16 << 20; // 16 MiB

app.add_plugins(DefaultPlugins.set(RenderPlugin {
    render_creation: WgpuSettings {
        // A 4K texture with mips is 21 MiB and a scan's vertex buffer 16-52 MiB:
        // each gets a block of its own, which the driver takes back when the
        // scan leaves the GPU.
        memory_hints: MemoryHints::Manual {
            suballocated_device_memory_block_size: SCAN_BLOCK..SCAN_BLOCK,
        },
        ..default()
    }
    .into(),
    ..default()
}));
```

`MemoryHints::MemoryUsage` (8–64 MiB blocks) is the ready-made alternative, but its blocks grow to
64 MiB, so a 21 MiB texture would again share a block. A fixed 16 MiB keeps every 4K map and every large
mesh buffer on its own.

**What should change, if it works.**

- The driver's usage falls within a reading or two of each `UHD scan … left the GPU` line, so the
  budget computed from it does not shrink and the cascade does not start.
- The game's own usage grows in 16 MiB steps instead of 256 MiB. That step is the reason given for
  `PREFILL_HEADROOM_GB` and for `GpuScans::entering`.
- The uncommitted ledger (`rest_peak`, `frees`, `stuck`, `settle_frees`, `note_stuck_frees`,
  `FREE_WAIT_SECONDS`, about 150 lines with tests) models memory the driver never returns. With the
  memory returned, most or all of it has nothing left to model.

**What may still be retained.** Index buffers under 16 MiB (some sites' are 7–9 MiB) and anything else
smaller than the block still share blocks. That is a small part of a scan (a site scan is about 107 MiB).

**Risks.** More, smaller driver allocations: a few hundred instead of a few dozen. That is far under
any driver's limit, but check that load time and frame time do not move.

**Experiment E1.** Branch from the committed `e34c6b8`, which still has the cascade. Add only the
setting. Replay the walk (`target/playtest/phase1/run_scenario.sh`) and the T1200 runs
(`target/playtest/t1200/run.sh NAME LEAVE`, with agentic-tooling `testing/vram-hog`). It passes if:

- nvidia-smi and the logged driver usage fall after each eviction;
- no budget staircase appears, and no scan leaves and returns;
- frame max and p99 are no worse than `after.txt` §1, and the splash is no slower than §5.

If it passes, decide how much of the ledger to keep before committing it.

### 2. wgpu can report exact numbers, if finding 1 is not enough

`wgpu::Device::generate_allocator_report()` (`wgpu-30.0.2/src/api/device.rs:630`; Vulkan:
`wgpu-hal-30.0.2/src/vulkan/device.rs:2954`) returns `total_allocated_bytes` (live allocations) and
`total_reserved_bytes` (the blocks). `RenderDevice::wgpu_device()` reaches it, as `read_vram` already
does for the driver's budget.

- Rest of the game = allocated − resident scans. It is exact, and it falls as soon as a scan's buffers
  are freed, whether or not a block empties.
- Holes = reserved − allocated: room a new scan can use without the driver's usage growing.

That replaces the guess about which bytes of the driver's usage are "the rest of the game" and which
are freed scans. The report lists every live allocation (it clones their names), so read it once a
second as now, not per frame, and time it once. It counts every memory type, staging buffers included;
the driver's reading counts one heap. Use this only if E1 leaves retention that matters: one mechanism
is easier to maintain than two.

### 3. Over budget, a scan that is wanted can never leave

`evict_over_budget` (`src/assets/gpu_scans.rs:506-519`) picks only among scans with
`wanted.is_none()`:

```rust
.filter(|(_, scan)| {
    scan.state == State::Loaded && scan.resident && scan.wanted.is_none()
})
```

Callers want a scan from 40 m short of where it shows (`UPLOAD_LEAD`), so "wanted" includes scans that
are not showing. Two consequences:

- A plant scan wanted 39 m ahead is protected, while a pinned tree nobody is near is not. The pinned
  tree leaves first.
- If every resident scan is wanted (a dense belt: up to 13 plant scans within 40 m, plus a site within
  its lead), nothing leaves, and the game stays over the driver's budget. The plan's Review Focus 6
  calls that state the crash.

`rest_peak` only rises, so the budget only shrinks while the game runs; this state is reachable.

**Change.** Evict anything that is not showing now (`wanted` is none or more than 0), in
`Scan::leave_order`, pinned last. That is the rule `making_way` already applies when a wanted scan
needs room, so both paths rank scans the same way. Test: "over budget, a scan wanted ahead leaves
before a pinned one, and one showing now stays".

### 4. No VRAM reading is taken while any scan loads

`apply_reading` returns early while `uploading()` (`gpu_scans.rs:229-232`, `461-465`): any scan that is
loading, or loaded and not yet confirmed. `next_prefill` (`375-389`) holds back only while a scan is
`Loading`, so it proposes the next file as soon as the last one is measured, before that one is
confirmed. During prefill there is therefore always a scan in flight, and no reading lands until
prefill has nothing left to load. The same holds on a walk through plants whose scans load back to back.

The first reading comes in the first in-game frame, when the rest of the game holds 1.8–2.0 GiB; by the
time the curtain lifts it holds 2.4 GiB (`after.txt` §4 and §6). The budget in between is the stale one.
`PREFILL_HEADROOM_GB` and the `entering` reserve (0.5 GiB each) cover that gap with constants measured
on the 2060.

**Change.** Let a reading through between scans: prefill proposes the next file only after one reading
has been applied since the last scan turned resident, and `track_vram_budget` reads at once when
uploads end instead of at its next one-second tick. Each prefilled scan then costs one reading more
(under a frame if the read is immediate), and the budget follows the world's own uploads as they land.
With that and finding 1, check whether the two reserves are still needed.

### 5. On a small card the pinned trees take the whole budget

`after.txt` §7, run hog1 (driver budget 3.4 GB, scans may use 0.5 GB): three of the five pinned trees
filled the budget. "No site or plant scan ever loaded: every site stayed a stand-in." A site in front
of the camera kept its stand-in for the whole run while tree scans that no tree was near held the room.

Pins never make way: `making_way` excludes them (`gpu_scans.rs:608`).

**Change.** A scan that is showing now (distance 0 or less) may displace a pinned scan nobody wants.
Pins keep what they are for: they load first, and they do not leave for scans that are only wanted
ahead. This is a product call as much as a code one: on a 4 GB card, is a site in view worth more than
a tree scan nobody stands near? The rule above says yes.

---

## Frame hitches

### 6. Prefill keeps uploading after the curtain lifts

`prefill_scans` runs in `Last` for as long as the game is in `AppState::InGame`
(`src/assets/gpu_scans/streaming.rs:47-54`), one file at a time. Each scan that reaches the GPU stalls
the render world for 103–147 ms (`after.txt` §2). Behind the curtain nobody sees that. After it, the
player takes a freeze for a scan nobody asked for.

On the 2060 this does not show: the scan budget (1.8 GB less the headroom) fills before the curtain's
12 s cap. With a large budget it would. 42 scans at 0.6–1.1 s each, one at a time, take 25–45 s; the
curtain lifts after at most 12 s; so a 10 GB card would take about 30 freezes in its first half minute
of play. That is an estimate from the code and the 2060's timings, not a run on a larger card. It works
against goal 8 (look richer on stronger GPUs).

**Change.**

- Behind the curtain (`GpuScans::entering`), let prefill load several files at once. The freezes are
  hidden there, and the IO pool's threads are otherwise idle.
- After the curtain, prefill only when a freeze does not matter: while the game is paused or the map is
  open. Or stop prefilling once the curtain lifts.

### 7. Bevy can compress scan vertices at load

Bevy 0.20.1 has mesh attribute compression, and the glTF loader takes it per load:
`GltfLoaderSettings::mesh_compression` (`bevy_gltf-0.20.1/src/loader/mod.rs:221-223`, applied at
`:872`, on the loader's worker, not on a frame). `MeshCompressionArgs::regular()`
(`bevy_mesh-0.20.1/src/mesh.rs:271-331`) stores:

| Attribute | Now | Compressed |
|---|---|---|
| Position | 12 bytes (Float32x3) | 8 (Snorm16x4, relative to the mesh's bounds) |
| Normal | 12 (Float32x3) | 4 (Snorm16x2, octahedral) |
| Tangent | 16 (Float32x4) | 4 (Snorm16x2, octahedral, sign kept) |
| UV | 8 (Float32x2) | 4 (Unorm16x2, over the UV range) |
| **Per vertex** | **48** | **20** |

Bevy's own mesh and prepass shaders decode these (`bevy_pbr-0.20.1/src/render/mesh.rs`,
`prepass/mod.rs`). A material with its own vertex shader would need to decode them too, and the game
has one: `assets/shaders/plant_wind.wesl`, used by the medium and low plants. The UHD instances do not
use it. They keep the glTF's `StandardMaterial`; their ready hooks only toggle visibility
(`canopy.rs:521`, `plants.rs:748`, `sites.rs:949`). So the change is one line where scans load, and
nothing else in the game is touched:

```rust
// src/assets/gpu_scans/streaming.rs
fn render_only(settings: &mut GltfLoaderSettings) {
    settings.load_meshes = RenderAssetUsages::RENDER_WORLD;
    settings.load_materials = RenderAssetUsages::RENDER_WORLD;
    settings.mesh_compression = Some(MeshCompressionArgs::regular());
}
```

**Estimated saving.** A tree scan has about 1.1M vertices: 28 bytes less each is about 29 MiB of its
134 MiB. Plant scans are similar. A site scan has about 20 MiB of vertex data, so about 12 MiB less.
Over all 42 scans that is roughly 0.8 of the 4.4 GiB they take (`after.txt` §8). `scan_bytes` measures
the compressed mesh, so the books follow without a change. More scans fit a budget, so fewer upload
during play.

**Precision.** Positions step by the bounds' half-extent over 32,767: about 0.5 mm on a 33 m tree and
1.5 mm on a 100 m site. UVs step by their range over 65,535, about a sixteenth of a texel on a 4K map.

**What it does not do.** The upload freeze shrinks less than the bytes do: Bevy copies attribute by
attribute, vertex by vertex (finding 8), and the vertex and attribute counts stay the same.

**Experiment E2.** Add the line; compare the `on the GPU (N MB)` log sizes, the upload gaps from
`slow_frames.py`, and fpv screenshots of a tree, a plant and a site before and after. It passes if the
sizes drop by about the estimate, no `Failed to compress mesh` debug line appears, and the screenshots
show no difference.

### 8. The upload freeze itself is inside Bevy

`after.txt` §2: each scan upload after the curtain is a 103–147 ms gap, of which
`allocate_and_free_meshes` is 65–105 ms and `prepare_assets<GpuImage>` up to 45 ms.

- For a mesh with its own buffer, Bevy creates the buffer mapped and fills it with
  `write_packed_vertex_buffer_data` (`bevy_render-0.20.1/src/slab_allocator.rs:1042`,
  `bevy_mesh-0.20.1/src/mesh.rs:1375-1398`): one small copy per vertex per attribute, about 4.4 million
  of them for a tree scan, on the render thread, in one frame.
- `RenderAssetBytesPerFrame` does not apply to meshes: `allocate_and_free_meshes`
  (`bevy_render-0.20.1/src/mesh/allocator.rs:249`) takes every extracted mesh of the frame.
- For images it does apply, but Bevy removes the old GPU copy of an updated asset before the new one
  is written. That is why the fog-of-war mask and the locator ring blinked under the limit (Ruling 30).

So no setting spreads a scan's upload. What helps is fewer uploads during play (findings 1, 5, 6, 7) or
lighter scans, which is the open decision at the end.

---

## Load time and per-frame cost

### 9. Three IO threads do all the loading

This machine has 12 cores. Bevy's default IO pool is a quarter of them, at most four
(`bevy_app-0.20.1/src/task_pool_plugin.rs:122-124`): three threads. They read and decode all 378 GLBs,
with the KTX2 levels unpacked by pure-Rust zstd (`Cargo.toml:14`, feature `zstd_rust`), and the RAM
fill's reads run on the same pool (`ram_source.rs:92`, `:145`). `after.txt` §5 shows it: the fill takes
10.0–11.7 s for 3.3 GB whether the files are cached or not, so it waits for a thread, not for the disk.
The splash is 2–3 s longer than before.

**Changes to try, each measured on its own.**

- `zstd_c` on native builds and `zstd_rust` on the web, through target-specific dependency tables.
  It is already item 2 of the precalculate-once inventory. It shortens the splash and every scan
  reload: 0.6–1.1 s each, about half of it decode (the inventory's estimate). It adds the C `zstd`
  crates to the native build.
- A larger IO share in `TaskPoolPlugin`. The compute pool gets what is left, so check the frame time
  in the wood walk afterwards.

### 10. `want` is called once per plant

`want_plant_scans` (`src/world/plants.rs:718-745`) calls `scans.want(full, …)` for every plant within
40 m that has a scan, every frame. Each call is a lookup in a `BTreeMap` keyed by path strings that
share a long prefix. In a dense belt that is hundreds to a few thousand lookups a frame, to feed at
most 13 distinct scans.

**Change.** Keep the nearest distance per model while looping, then call `want` once per model.

---

## Process

### 11. The branch has no upstream

`git branch -vv` shows no remote for `worktree-vegetation-lod-design`, and `git branch -r` lists only
`origin/main`. The 26 commits exist only on this disk. Pushing is the user's call.

### 12. One branch carries five topics

Committed: the RAM and GPU levels (phase 1). In the tree, untracked: the tree-impostor design, the
asphalt and wall texture design and plan, and the precalculate-once inventory. Planned on the same
branch: the vegetation and rock tiers (phase 2) and UHD for the other kinds (phase 3).

Phase 1 is measured and stands on its own. Opening its pull request before the next topic starts keeps
each review small, and gets `main` the `--vram` budget that fixes the T1200 crash.

### 13. Merging main conflicts in `CLAUDE.md`

`main` moved "Tactical view" and "Procedural content" out of `CLAUDE.md` into `doc/world-reference.md`
(`d5e2518`). The branch edited bullets inside those sections. `git merge-tree --write-tree HEAD main`
reports one conflict, in `CLAUDE.md`; `README.md` and `arch-readme.md` merge cleanly.

**Recipe.** Take main's `CLAUDE.md`, then:

- Keep in `CLAUDE.md`: the `--vram` line under Controls, and the "Precalculate once" bullet under
  Architecture Conventions.
- Move into `doc/world-reference.md`: the branch's edits to the Scenery, Plants, Sites, Precompiled
  textures and Entering the Zone bullets, and the new "RAM and GPU" bullet.
- Add a "RAM and GPU" row to the World reference table in `CLAUDE.md` (code: `assets/gpu_scans.rs`,
  `assets/ram_source.rs`), and one line under "Rules that hold everywhere": every streamed UHD scan
  loads through `assets::uhd` and `GpuScans`.

### 14. The web build has never compiled these changes

`rustup target list --installed` shows only `x86_64-unknown-linux-gnu`. The branch adds about ten
`#[cfg(target_arch = "wasm32")]` branches (`assets/mod.rs`, `main.rs`, `splash.rs`, `streaming.rs`,
`ram_source`), and `after.txt` §9 records that the web build was not checked. `CLAUDE.md` names WASM
support as a goal. Run `rustup target add wasm32-unknown-unknown`, then
`cargo check --target wasm32-unknown-unknown` and `cargo clippy --target wasm32-unknown-unknown -- -D warnings`.

### 15. The "RAM and GPU" doc bullet repeats the code

The bullet is 40 lines and restates eight constants with their values and the eviction rules. The
uncommitted code already contradicts it: it says the rest of the game never counts for less than
`GAME_VRAM_GB`, and that floor is gone. Describe the mechanism (read once into RAM, one owner of GPU
copies, a budget from the driver, show only when resident, callers drop their instance when a scan
leaves) and leave constants and their values in the code, where the compiler keeps them true.

---

## Smaller notes

- **The walk still misses its target.** Its worst main-app frame is 39.5 ms against the 33 ms target
  (`after.txt` §1). The spans in it add up to about 15 ms; the rest is unattributed.
- **RAM grew more than the fill.** Peak RSS rose from 6.8–7.7 to 9.5–11.7 GiB; the fill is 3.3 GiB.
  Up to 1.6 GiB more is unexplained. The user allows the RAM, but the cause is not known; decode
  buffers that the allocator keeps after each scan load are the likely one.
- **Tunables.** `gpu_scans.rs` has eight constants (`KEEP_SECONDS`, `UPLOAD_LEAD`, `MARGIN_GB`,
  `DESKTOP_GB`, `GAME_VRAM_GB`, `GPU_BYTES_PER_FILE_BYTE`, `PREFILL_HEADROOM_GB`, `FREE_WAIT_SECONDS`)
  and eleven fields per scan. The spec asked for a budget "with no constants to tune". Findings 1 and 4
  aim at removing three of them (`FREE_WAIT_SECONDS`, `PREFILL_HEADROOM_GB`, and the `entering` state).
- **The budget is logged once.** `track_vram_budget` logs the first reading at info level; later
  budgets show only in the `on the GPU` lines. The uncommitted `debug!` line in `apply_reading` covers
  that for a debug run.

## Open decision (the user's)

The 13 plant scans take 1.3 GiB on the GPU (`after.txt` §8) and cost a 100–150 ms freeze each time one
arrives, to show the one plant within 8 m of the stalker. In the after-walk they filled the budget, and
the garage's scan never showed. After E1 and E2 are measured, choose one:

1. Keep it as it is.
2. Show plant scans only when they fit behind the curtain; never upload one during play.
3. Cap the plant scans' triangle count and keep their native 4K textures, so each is lighter to hold
   and to upload.

## Suggested order

1. E1 (finding 1), before the ledger is finished and committed.
2. Findings 3 and 5: small, and they make tight cards safer.
3. E2 (finding 7), then findings 4 and 6 with E1's result in hand.
4. Findings 9 and 10.
5. The merge (13) and the wasm check (14), then the pull request for phase 1 (12).

Run E1 and E2 with the GPU quiet: no other game running, as `after.txt` requires of its own runs.
