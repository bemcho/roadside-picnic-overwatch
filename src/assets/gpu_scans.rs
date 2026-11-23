//! Which UHD scans hold a copy on the GPU: the budget, admission and eviction.
//!
//! Callers `want` and `pin` scans each frame and show a scan's `scene` once it
//! is on the GPU. `GpuScans::plan` says what to load and what to drop;
//! `GpuScansPlugin` loads the admitted scans render-only from `ram://`,
//! measures them, learns from the render world when they are resident, and
//! keeps the budget on the driver's live VRAM reading (`vram`).

mod residency;
mod vram;

use std::collections::{BTreeMap, HashSet};

use bevy::asset::{LoadState, RecursiveDependencyLoadState, RenderAssetUsages};
use bevy::ecs::system::SystemParam;
use bevy::gltf::{Gltf, GltfLoaderSettings, GltfMesh};
use bevy::log::warn;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetBytesPerFrame;
use bevy::render::{Render, RenderApp, RenderSystems};

#[cfg(not(target_arch = "wasm32"))]
use crate::assets::ram_source::RamFill;
use crate::assets::uhd;
use crate::launch::{DEFAULT_VRAM_GB, LaunchOptions};
use crate::states::AppState;

use residency::{ScanParts, ScanResidency, publish_scan_residency, scan_bytes};
use vram::track_vram_budget;

/// Seconds a scan stays on the GPU after it was last wanted, whatever the pressure.
pub const KEEP_SECONDS: f32 = 120.0;
/// Metres short of the range where a scan shows at which callers start wanting
/// it, so it is on the GPU by the time it is needed.
// The tree, plant and site streams of Tasks 6 and 7 use it.
#[expect(dead_code)]
pub const UPLOAD_LEAD: f32 = 40.0;
/// GiB left free of scans, for the driver and for other programs' growth.
pub const MARGIN_GB: f32 = 0.5;
/// GiB the desktop holds, for the fallback only (0.63 GiB measured on the 2060).
pub const DESKTOP_GB: f32 = 0.6;
/// GiB the game holds without any streamed scan, measured at the map overview.
pub const GAME_VRAM_GB: f32 = 2.4;
/// GPU bytes per file byte of a scan (the five trees: about 700 MB from 428 MB).
pub const GPU_BYTES_PER_FILE_BYTE: f32 = 1.65;
/// Bytes the render world uploads per frame. A soft limit: an asset larger
/// than it still goes up whole, then the rest wait for the next frame.
const UPLOAD_BYTES_PER_FRAME: usize = 32 * 1024 * 1024;

/// The driver's device-local heap budget for this process and this process's
/// usage of it, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramReading {
    pub budget: u64,
    pub usage: u64,
}

/// What `GpuScans::plan` decided: scans to start loading and scans to drop.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Admission {
    pub load: Vec<&'static str>,
    pub evict: Vec<&'static str>,
}

/// GiB as bytes; negative sizes are none.
fn gib(gb: f32) -> u64 {
    (gb.max(0.0) * (1u64 << 30) as f32) as u64
}

/// Bytes as GiB, for the log.
fn gb(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}

/// The scan budget from a live driver reading: what the driver (or the
/// player's `cap`) leaves once everything but the scans is counted, less a
/// margin. `usage - resident_scans` is everything else the game holds.
pub fn live_scan_budget(reading: VramReading, resident_scans: u64, cap: Option<u64>) -> u64 {
    let ceiling = cap.map_or(reading.budget, |cap| cap.min(reading.budget));
    let others = reading.usage.saturating_sub(resident_scans);
    ceiling
        .saturating_sub(others)
        .saturating_sub(gib(MARGIN_GB))
}

/// The scan budget when the driver cannot report VRAM: the card less what the
/// game, the desktop and the margin hold.
pub fn scan_budget(vram_gb: f32) -> u64 {
    gib(vram_gb - (GAME_VRAM_GB + DESKTOP_GB + MARGIN_GB))
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Off,
    Loading,
    Loaded,
}

/// The strong handles that keep a scan's GPU copy alive; dropping them frees it.
struct ScanHandles {
    gltf: Handle<Gltf>,
    scene: Handle<WorldAsset>,
}

#[derive(Default)]
struct Scan {
    state: State,
    /// GPU bytes: an estimate from the file until `measured` reports the real size.
    bytes: u64,
    is_measured: bool,
    pinned: bool,
    /// Seconds on the caller's clock when it was last wanted.
    last_used: f32,
    /// The nearest ask this frame, cleared by `plan`.
    wanted: Option<f32>,
    /// The last `plan` found no room for it, though it is wanted.
    waiting: bool,
    /// Held from admission until eviction.
    handles: Option<ScanHandles>,
    /// Loaded, and every mesh and image of it is prepared in the render world.
    resident: bool,
    /// Its last load failed: it counts at its estimate until it leaves.
    failed: bool,
}

impl Scan {
    /// Where it queues among scans waiting to load: the nearest ask first.
    fn distance(&self) -> f32 {
        self.wanted.unwrap_or(f32::INFINITY)
    }
}

/// The scans that may hold a GPU copy, within a byte budget.
///
/// A scan is off the GPU, loading (admitted, size not yet measured) or loaded.
/// Loading and loaded scans both count in `used`. A loaded scan is resident
/// once the render world has prepared all of it.
#[derive(Resource, Default)]
pub struct GpuScans {
    budget: u64,
    // Ordered, so equal distances and ages resolve the same way every run.
    scans: BTreeMap<&'static str, Scan>,
    warned: HashSet<&'static str>,
    /// Warnings `warn_pinned_waits` logged.
    #[cfg(test)]
    pinned_warnings: usize,
}

impl GpuScans {
    pub fn new(budget: u64) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    /// A new live budget; the next `plan` evicts down to it if needed.
    pub fn set_budget(&mut self, budget: u64) {
        self.budget = budget;
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// Bytes of the scans that are loaded, not those still loading. Past the
    /// budget, `plan` drops scans: it is what the driver's usage already holds.
    pub fn resident_bytes(&self) -> u64 {
        self.bytes_where(|state| state == State::Loaded)
    }

    /// Bytes of the scans loading or loaded.
    pub fn used(&self) -> u64 {
        self.bytes_where(|state| state != State::Off)
    }

    /// The scan's scene, once every mesh and image of it is on the GPU.
    // The tree, plant, site and artifact streams of Tasks 7 and 8 call it.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn scene(&self, path: &'static str) -> Option<Handle<WorldAsset>> {
        let scan = self.scans.get(path)?;
        let handles = scan.handles.as_ref().filter(|_| scan.resident)?;
        Some(handles.scene.clone())
    }

    /// No scan loading or waiting for the render world to confirm it, and
    /// every pinned scan on the GPU. A scan whose load failed never holds it up.
    // The entry curtain waits for it from Task 6.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn settled(&self) -> bool {
        self.scans.values().all(|scan| {
            scan.failed
                || match scan.state {
                    State::Off => !scan.pinned,
                    State::Loading => false,
                    State::Loaded => scan.resident,
                }
        })
    }

    /// Loads before every unpinned scan and never leaves for one, but only
    /// within the budget.
    // The tree pins of Task 6 call it.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn pin(&mut self, path: &'static str) {
        self.scan_mut(path).pinned = true;
    }

    /// Asks for the scan this frame; the smallest distance of several asks stands.
    // The plant, site and artifact streams of Tasks 7 and 8 call it.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn want(&mut self, path: &'static str, distance: f32) {
        let scan = self.scan_mut(path);
        scan.wanted = Some(
            scan.wanted
                .map_or(distance, |nearest| nearest.min(distance)),
        );
    }

    /// Sizes the scan from its file until it is measured.
    pub fn estimate(&mut self, path: &'static str, file_len: u64) {
        let scan = self.scan_mut(path);
        if !scan.is_measured {
            scan.bytes = (file_len as f32 * GPU_BYTES_PER_FILE_BYTE) as u64;
        }
    }

    /// The load finished. `bytes` replaces the estimate and stays the size for
    /// any later reload; `None` keeps the estimate.
    pub fn measured(&mut self, path: &'static str, bytes: Option<u64>) {
        let scan = self.scan_mut(path);
        if scan.state == State::Loading {
            scan.state = State::Loaded;
        }
        if let Some(bytes) = bytes {
            scan.bytes = bytes;
            scan.is_measured = true;
        }
    }

    /// Decides what to load and what to drop at `now` seconds, marks loads as
    /// loading and drops as off, and clears this frame's wants.
    ///
    /// Admission counts every scan in `used`, so uploads in flight keep their
    /// room. The over-budget step counts only the loaded scans, which are the
    /// ones the driver's usage holds (and `live_scan_budget` subtracted from the
    /// budget): a scan still uploading, in `usage` but not yet `measured`, must
    /// not make a healthy budget look overcommitted.
    pub fn plan(&mut self, now: f32) -> Admission {
        let mut admission = Admission::default();
        self.begin_plan(now);
        if self.resident_bytes() > self.budget {
            // The budget shrank. Drop one scan per plan: the reading's numbers lag the GPU's frees.
            admission.evict.extend(self.evict_over_budget());
        } else {
            for path in self.pending() {
                self.admit(path, now, &mut admission);
            }
        }
        for scan in self.scans.values_mut() {
            scan.wanted = None;
        }
        admission
    }

    /// The first file of `order` that is off and fits the free room, while no
    /// scan is loading and no wanted scan waits (as of the last `plan`). A
    /// pinned scan too big for the card is not wanted and never holds it back.
    // The prefill of Task 6 calls it.
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn next_prefill(&self, order: &[&'static str]) -> Option<&'static str> {
        if self
            .scans
            .values()
            .any(|scan| scan.state == State::Loading || scan.waiting)
        {
            return None;
        }
        let room = self.room();
        order.iter().copied().find(|path| {
            self.scans
                .get(path)
                .is_some_and(|scan| scan.state == State::Off && scan.bytes <= room)
        })
    }

    fn scan_mut(&mut self, path: &'static str) -> &mut Scan {
        self.scans.entry(path).or_default()
    }

    /// Starts loading an admitted scan render-only, from `ram://` (or
    /// `assets/` where the fill left it): its meshes and images live on the GPU only.
    fn load(&mut self, path: &'static str, server: &AssetServer) {
        // Both loads take the same settings, so the file loads once.
        let gltf = server
            .load_builder()
            .with_settings(render_only)
            .load(uhd(path));
        let scene = server
            .load_builder()
            .with_settings(render_only)
            .load(GltfAssetLabel::Scene(0).from_asset(uhd(path)));
        let scan = self.scan_mut(path);
        scan.handles = Some(ScanHandles { gltf, scene });
        scan.resident = false;
        scan.failed = false;
    }

    /// Drops an evicted scan's handles; Bevy then frees its GPU copy.
    fn unload(&mut self, path: &'static str) {
        let scan = self.scan_mut(path);
        scan.handles = None;
        scan.resident = false;
        scan.failed = false;
    }

    /// The loading scans and their glTF assets.
    fn loading(&self) -> Vec<(&'static str, AssetId<Gltf>)> {
        self.scans
            .iter()
            .filter(|(_, scan)| scan.state == State::Loading)
            .filter_map(|(&path, scan)| Some((path, scan.handles.as_ref()?.gltf.id())))
            .collect()
    }

    /// The loading scan whose glTF asset this is.
    fn loading_path(&self, id: AssetId<Gltf>) -> Option<&'static str> {
        self.loading()
            .into_iter()
            .find_map(|(path, gltf)| (gltf == id).then_some(path))
    }

    /// The load failed: the scan ends loading at its estimate and shows its
    /// mid tier; it is not retried until it has left the GPU.
    fn fail(&mut self, path: &'static str) {
        self.measured(path, None);
        let scan = self.scan_mut(path);
        scan.handles = None;
        scan.failed = true;
    }

    /// Marks a loaded scan resident; its bytes the first time, `None` after.
    fn mark_resident(&mut self, path: &'static str) -> Option<u64> {
        let scan = self.scans.get_mut(path)?;
        let newly = scan.state == State::Loaded && scan.handles.is_some() && !scan.resident;
        scan.resident |= newly;
        newly.then_some(scan.bytes)
    }

    fn bytes_where(&self, state: impl Fn(State) -> bool) -> u64 {
        self.scans
            .values()
            .filter(|scan| state(scan.state))
            .map(|scan| scan.bytes)
            .sum()
    }

    fn room(&self) -> u64 {
        self.budget.saturating_sub(self.used())
    }

    /// Bytes that must leave before `bytes` more fit the budget. Unlike
    /// `room()`, exact when the uploads in flight already put `used()` past the
    /// budget, where `room()` reads 0 and hides how far.
    fn deficit(&self, bytes: u64) -> u64 {
        (self.used() + bytes).saturating_sub(self.budget)
    }

    /// Stamps every scan wanted this frame as used now and forgets who waited.
    fn begin_plan(&mut self, now: f32) {
        for scan in self.scans.values_mut() {
            if scan.wanted.is_some() {
                scan.last_used = now;
            }
            scan.waiting = false;
        }
    }

    /// For when the loaded scans alone (`resident_bytes`) exceed the budget:
    /// drops the oldest loaded scan not shown this frame, unpinned before
    /// pinned. Scans still loading are neither counted nor dropped.
    fn evict_over_budget(&mut self) -> Option<&'static str> {
        let (&path, _) = self
            .scans
            .iter()
            .filter(|(_, scan)| scan.state == State::Loaded && scan.wanted.is_none())
            .min_by(|(_, a), (_, b)| {
                a.pinned
                    .cmp(&b.pinned)
                    .then(a.last_used.total_cmp(&b.last_used))
            })?;
        self.scan_mut(path).state = State::Off;
        Some(path)
    }

    /// Scans to load, pinned first, then the nearest.
    fn pending(&self) -> Vec<&'static str> {
        let mut pending: Vec<_> = self
            .scans
            .iter()
            .filter(|(_, scan)| scan.state == State::Off && (scan.pinned || scan.wanted.is_some()))
            .collect();
        pending.sort_by(|(_, a), (_, b)| {
            b.pinned
                .cmp(&a.pinned)
                .then(a.distance().total_cmp(&b.distance()))
        });
        pending.into_iter().map(|(&path, _)| path).collect()
    }

    /// Loads the scan if it fits, after dropping idle scans when it must.
    fn admit(&mut self, path: &'static str, now: f32, admission: &mut Admission) {
        let Some(scan) = self.scans.get(path) else {
            return;
        };
        let (bytes, pinned) = (scan.bytes, scan.pinned);
        // A pinned scan never evicts others: it loads only into room already free.
        let fits = self.deficit(bytes) == 0
            || (!pinned && self.free_idle(bytes, now, &mut admission.evict));
        if fits {
            let scan = self.scan_mut(path);
            scan.state = State::Loading;
            scan.last_used = now;
            admission.load.push(path);
        } else if pinned {
            self.warn_pinned_waits(path, bytes);
        } else {
            self.scan_mut(path).waiting = true;
        }
    }

    /// Drops idle scans, oldest first, until `bytes` fit, and says whether they
    /// do. Drops nothing when even all of them would not make room.
    fn free_idle(&mut self, bytes: u64, now: f32, evicted: &mut Vec<&'static str>) -> bool {
        let idle = self.idle(now);
        let freed: u64 = idle.iter().map(|path| self.scans[path].bytes).sum();
        if self.deficit(bytes) > freed {
            return false;
        }
        for path in idle {
            if self.deficit(bytes) == 0 {
                break;
            }
            self.scan_mut(path).state = State::Off;
            evicted.push(path);
        }
        true
    }

    /// Loaded, unpinned scans unused for `KEEP_SECONDS`, oldest first.
    fn idle(&self, now: f32) -> Vec<&'static str> {
        let mut idle: Vec<_> = self
            .scans
            .iter()
            .filter(|(_, scan)| {
                scan.state == State::Loaded && !scan.pinned && now - scan.last_used >= KEEP_SECONDS
            })
            .collect();
        idle.sort_by(|(_, a), (_, b)| a.last_used.total_cmp(&b.last_used));
        idle.into_iter().map(|(&path, _)| path).collect()
    }

    fn warn_pinned_waits(&mut self, path: &'static str, bytes: u64) {
        if self.warned.insert(path) {
            #[cfg(test)]
            {
                self.pinned_warnings += 1;
            }
            warn!(
                "pinned UHD scan {path} needs {} MB but {} of {} MB are free; it waits for room",
                bytes >> 20,
                self.room() >> 20,
                self.budget >> 20,
            );
        }
    }
}

/// Loads the scans `GpuScans` admits, learns when they are on the GPU, and
/// keeps the budget on the driver's live VRAM reading.
pub struct GpuScansPlugin;

impl Plugin for GpuScansPlugin {
    fn build(&self, app: &mut App) {
        // The budget until the first live reading, and for good where the
        // driver cannot report VRAM.
        let vram_gb = app
            .world()
            .get_resource::<LaunchOptions>()
            .and_then(|launch| launch.vram_gb)
            .unwrap_or(DEFAULT_VRAM_GB);
        let residency = ScanResidency::default();
        app.insert_resource(GpuScans::new(scan_budget(vram_gb)))
            .insert_resource(RenderAssetBytesPerFrame::new(UPLOAD_BYTES_PER_FRAME))
            .insert_resource(residency.clone())
            .add_systems(
                First,
                (track_vram_budget, note_resident_scans).run_if(in_state(AppState::InGame)),
            )
            .add_systems(Update, measure_scans)
            .add_systems(Last, admit_scans.run_if(in_state(AppState::InGame)));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(OnEnter(AppState::InGame), estimate_scans);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(residency).add_systems(
                Render,
                publish_scan_residency.in_set(RenderSystems::PrepareResources),
            );
        }
    }
}

/// Keeps a scan's meshes and images in the render world only.
fn render_only(settings: &mut GltfLoaderSettings) {
    settings.load_meshes = RenderAssetUsages::RENDER_WORLD;
    settings.load_materials = RenderAssetUsages::RENDER_WORLD;
}

/// Sizes every UHD scan from its file before the first `plan`: a scan never
/// estimated would count as 0 bytes and fit anywhere. The splash turns ready
/// only once the fill is full, so every file's length is known by now. The
/// web build has no fill: there each scan counts 0 bytes until `measured`
/// sizes it after its first load.
#[cfg(not(target_arch = "wasm32"))]
fn estimate_scans(fill: Res<RamFill>, mut scans: ResMut<GpuScans>) {
    for &(path, len) in fill.file_lens() {
        scans.estimate(path, len);
    }
}

/// Applies this frame's plan: evicted scans drop their handles, which frees
/// their GPU copy, and admitted ones start loading. It runs only in game, so
/// no `ram://` load starts before the fill is full.
fn admit_scans(
    mut scans: ResMut<GpuScans>,
    server: Res<AssetServer>,
    time: Res<Time<Real>>,
    residency: Res<ScanResidency>,
) {
    let admission = scans.plan(time.elapsed_secs());
    for path in admission.evict {
        scans.unload(path);
        residency.lock().forget(path);
        info!("UHD scan {path} left the GPU");
    }
    for path in admission.load {
        scans.load(path, &server);
    }
}

/// The asset stores a finished scan is measured from.
#[derive(SystemParam)]
struct ScanAssets<'w> {
    server: Res<'w, AssetServer>,
    gltfs: Res<'w, Assets<Gltf>>,
    gltf_meshes: Res<'w, Assets<GltfMesh>>,
    meshes: Res<'w, Assets<Mesh>>,
    images: Res<'w, Assets<Image>>,
}

impl ScanAssets<'_> {
    /// The meshes and images of a loaded scan: every primitive's mesh and
    /// every texture the file embeds.
    fn parts(&self, path: &'static str, gltf: &Gltf) -> ScanParts {
        let meshes = gltf
            .meshes
            .iter()
            .filter_map(|mesh| self.gltf_meshes.get(mesh))
            .flat_map(|mesh| mesh.primitives.iter().map(|primitive| primitive.mesh.id()))
            .collect();
        let images = (0..)
            .map_while(|index| {
                let texture = GltfAssetLabel::Texture(index).from_asset(uhd(path));
                self.server.get_path_id(texture)
            })
            .filter_map(|id| id.try_typed::<Image>().ok())
            .collect();
        ScanParts { meshes, images }
    }
}

/// Measures each scan whose load finished this frame, before render extract
/// takes its data, and hands its parts to the render world to confirm. A
/// failed load ends at its estimate.
fn measure_scans(
    mut loads: MessageReader<AssetEvent<Gltf>>,
    assets: ScanAssets,
    mut scans: ResMut<GpuScans>,
    residency: Res<ScanResidency>,
) {
    for event in loads.read() {
        let &AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };
        let (Some(path), Some(gltf)) = (scans.loading_path(id), assets.gltfs.get(id)) else {
            continue;
        };
        let parts = assets.parts(path, gltf);
        let bytes = scan_bytes(&parts, &assets.meshes, &assets.images);
        if bytes.is_none() {
            warn!(
                "UHD scan {path} left for the GPU before it was measured; it counts at its estimate"
            );
        }
        scans.measured(path, bytes);
        residency.lock().waiting.insert(path, parts);
    }
    for (path, id) in scans.loading() {
        if let Some(
            (LoadState::Failed(err), _, _) | (_, _, RecursiveDependencyLoadState::Failed(err)),
        ) = assets.server.get_load_states(id)
        {
            error!("UHD scan {path} failed to load: {err}");
            scans.fail(path);
        }
    }
}

/// Copies the render world's confirmations into `GpuScans` and logs each
/// scan's first frame on the GPU.
fn note_resident_scans(mut scans: ResMut<GpuScans>, residency: Res<ScanResidency>) {
    let residency = residency.lock();
    for &path in &residency.ready {
        if let Some(bytes) = scans.mark_resident(path) {
            let (used, budget) = (gb(scans.used()), gb(scans.budget()));
            info!(
                "UHD scan {path} on the GPU ({} MB); {used:.1} of {budget:.1} GB",
                bytes >> 20
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::launch::DEFAULT_VRAM_GB;

    use super::*;

    const MB: u64 = 1 << 20;

    impl GpuScans {
        /// Sets a scan's size directly, as if it were measured.
        fn estimate_exact(&mut self, path: &'static str, bytes: u64) {
            self.scan_mut(path).bytes = bytes;
        }
    }

    /// Wants the scan, plans it in with room to spare, and reports its size.
    fn fake_loaded(scans: &mut GpuScans, path: &'static str, bytes: u64, last_used: f32) {
        scans.want(path, 0.0);
        assert_eq!(scans.plan(last_used).load, [path], "{path} needs room");
        scans.measured(path, Some(bytes));
    }

    /// Gives a scan the handles `load` would, without an asset server.
    fn fake_handles(scans: &mut GpuScans, path: &'static str) {
        scans.scan_mut(path).handles = Some(ScanHandles {
            gltf: Handle::default(),
            scene: Handle::default(),
        });
    }

    #[test]
    fn a_scene_shows_only_once_all_of_it_is_on_the_gpu() {
        let mut scans = GpuScans::new(100 * MB);
        scans.estimate_exact("a", 10 * MB);
        scans.want("a", 0.0);
        assert_eq!(scans.plan(0.0).load, ["a"]);
        fake_handles(&mut scans, "a");
        assert_eq!(scans.scene("a"), None, "loading");
        assert_eq!(scans.mark_resident("a"), None, "not loaded yet");

        scans.measured("a", Some(12 * MB));
        assert_eq!(scans.scene("a"), None, "loaded, still uploading");
        assert_eq!(scans.mark_resident("a"), Some(12 * MB));
        assert_eq!(scans.scene("a"), Some(Handle::default()));
        assert_eq!(scans.mark_resident("a"), None, "reported once");

        scans.unload("a");
        assert_eq!(scans.scene("a"), None, "evicted");
    }

    #[test]
    fn settled_waits_for_loads_confirmations_and_pinned_scans() {
        let mut scans = GpuScans::new(100 * MB);
        assert!(scans.settled(), "nothing asked for");
        scans.estimate_exact("tree", 10 * MB);
        scans.estimate_exact("site", 10 * MB);
        scans.pin("tree");
        assert!(!scans.settled(), "a pinned scan is off the GPU");

        assert_eq!(scans.plan(0.0).load, ["tree"]);
        fake_handles(&mut scans, "tree");
        assert!(!scans.settled(), "loading");
        scans.measured("tree", Some(10 * MB));
        assert!(!scans.settled(), "waiting for the render world");
        scans.mark_resident("tree");
        assert!(scans.settled());

        scans.want("site", 0.0);
        assert_eq!(scans.plan(1.0).load, ["site"]);
        fake_handles(&mut scans, "site");
        assert!(!scans.settled(), "loading");
        scans.fail("site");
        assert!(scans.settled(), "a failed load holds nothing up");
        assert_eq!(scans.scene("site"), None);
    }

    #[test]
    fn the_fallback_budget_is_vram_minus_the_game_the_desktop_and_a_margin() {
        // GiB as bytes, independent of the production conversion.
        let gib = |gb: f32| (gb * (1u64 << 30) as f32) as u64;
        assert_eq!(
            scan_budget(GAME_VRAM_GB + DESKTOP_GB + MARGIN_GB + 2.0),
            gib(2.0)
        );
        assert_eq!(scan_budget(1.0), 0);
    }

    #[test]
    fn the_default_card_leaves_room_for_scans() {
        assert!(scan_budget(DEFAULT_VRAM_GB) > 0);
    }

    #[test]
    fn a_default_resource_has_no_budget_and_admits_nothing() {
        let mut scans = GpuScans::default();
        assert_eq!(scans.budget(), 0);
        scans.estimate_exact("a", MB);
        scans.want("a", 1.0);
        assert!(scans.plan(0.0).load.is_empty());
    }

    #[test]
    fn the_live_budget_is_what_the_driver_leaves_minus_a_margin() {
        // GiB as bytes, independent of the production conversion.
        let gib = |gb: f32| (gb * (1u64 << 30) as f32) as u64;
        // T1200: driver budget 3.9 GiB, game uses 3.0 GiB of which 0.5 GiB are scans
        let reading = VramReading {
            budget: gib(3.9),
            usage: gib(3.0),
        };
        assert_eq!(
            live_scan_budget(reading, gib(0.5), None),
            gib(3.9 - 2.5 - MARGIN_GB)
        );
        assert_eq!(
            live_scan_budget(reading, gib(0.5), Some(gib(3.5))),
            gib(3.5 - 2.5 - MARGIN_GB)
        );
        assert_eq!(
            live_scan_budget(
                VramReading {
                    budget: gib(2.0),
                    usage: gib(3.0)
                },
                0,
                None
            ),
            0
        );
    }

    #[test]
    fn a_shrunk_budget_evicts_scans_not_shown_least_recently_used_first() {
        let mut scans = GpuScans::new(400 * MB);
        fake_loaded(&mut scans, "old", 100 * MB, 0.0);
        fake_loaded(&mut scans, "newer", 100 * MB, 10.0);
        fake_loaded(&mut scans, "shown", 100 * MB, 0.0);
        fake_loaded(&mut scans, "pinned", 100 * MB, 0.0);
        scans.pin("pinned");
        scans.set_budget(250 * MB); // another program took VRAM at t = 20 s
        scans.want("shown", 5.0);
        assert_eq!(scans.plan(20.0).evict, ["old"]); // one per reading, ignores KEEP_SECONDS
        scans.want("shown", 5.0);
        assert_eq!(scans.plan(21.0).evict, ["newer"]); // now 200 MB <= 250 MB
        scans.want("shown", 5.0);
        assert!(scans.plan(22.0).evict.is_empty());
    }

    #[test]
    fn wanted_scans_load_nearest_first_within_the_budget() {
        let mut scans = GpuScans::new(300 * MB);
        for (path, bytes) in [("a", 200 * MB), ("b", 200 * MB), ("c", 50 * MB)] {
            scans.estimate_exact(path, bytes);
        }
        scans.want("a", 30.0);
        scans.want("b", 10.0);
        scans.want("c", 20.0);
        assert_eq!(scans.plan(0.0).load, ["b", "c"]); // a waits
    }

    #[test]
    fn pinned_scans_load_first_but_only_within_the_budget() {
        let mut scans = GpuScans::new(750 * MB);
        scans.estimate_exact("tree", 700 * MB);
        scans.estimate_exact("site", 100 * MB);
        scans.pin("tree");
        scans.want("site", 5.0);
        assert_eq!(scans.plan(0.0).load, ["tree"]); // the site waits for room

        let mut small = GpuScans::new(300 * MB); // a card too small for the pin
        small.estimate_exact("tree", 700 * MB);
        small.estimate_exact("site", 100 * MB);
        small.pin("tree");
        small.want("site", 5.0);
        assert_eq!(small.plan(0.0).load, ["site"]); // the tree waits (warn once)
    }

    #[test]
    fn a_scan_used_within_two_minutes_never_leaves() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "a", 250 * MB, 0.0);
        scans.want("a", 50.0);
        scans.plan(100.0); // still near an anchor at t = 100 s
        scans.estimate_exact("b", 100 * MB);
        scans.want("b", 10.0); // b needs a's room at t = 150 s
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
        scans.estimate_exact("b", 100 * MB);
        scans.want("b", 10.0);
        let admission = scans.plan(200.0);
        assert_eq!((admission.evict, admission.load), (vec!["old"], vec!["b"]));
    }

    #[test]
    fn nothing_leaves_unless_the_new_scan_then_fits() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "idle", 100 * MB, 0.0);
        scans.estimate_exact("huge", 400 * MB);
        scans.want("huge", 1.0);
        let admission = scans.plan(500.0);
        assert!(admission.evict.is_empty() && admission.load.is_empty());
    }

    #[test]
    fn an_evicted_scan_comes_back_when_wanted_again() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "a", 100 * MB, 0.0);
        scans.estimate_exact("b", 300 * MB);
        scans.want("b", 1.0);
        let admission = scans.plan(200.0); // b needs all the room: idle a leaves
        assert_eq!((admission.evict, admission.load), (vec!["a"], vec!["b"]));
        scans.measured("b", Some(300 * MB));

        scans.set_budget(0); // another program takes the VRAM: b leaves too
        assert_eq!(scans.plan(400.0).evict, ["b"]);
        scans.set_budget(300 * MB);
        assert_eq!(scans.used(), 0);

        scans.want("a", 1.0);
        assert_eq!(scans.plan(401.0).load, ["a"]);
        assert_eq!(scans.used(), 100 * MB); // the size measured on its first load
    }

    #[test]
    fn prefill_takes_the_next_file_that_fits_only_while_nothing_waits() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "held", 100 * MB, 0.0); // 200 MB are free
        scans.estimate_exact("big", 250 * MB);
        scans.estimate_exact("small", 50 * MB);
        scans.estimate_exact("tiny", 10 * MB);
        let order = ["held", "big", "small", "tiny"];
        assert_eq!(scans.next_prefill(&order), Some("small")); // held is on, big does not fit

        scans.want("big", 1.0); // a wanted scan waits for room
        assert!(scans.plan(1.0).load.is_empty());
        assert_eq!(scans.next_prefill(&order), None);

        scans.plan(2.0); // nobody wants it any more
        assert_eq!(scans.next_prefill(&order), Some("small"));

        scans.want("small", 1.0); // planned in but not yet measured
        assert_eq!(scans.plan(3.0).load, ["small"]);
        assert_eq!(scans.next_prefill(&order), None);
        scans.measured("small", Some(50 * MB));
        assert_eq!(scans.next_prefill(&order), Some("tiny"));
    }

    #[test]
    fn a_pinned_scan_that_does_not_fit_never_stops_the_prefill() {
        let mut scans = GpuScans::new(300 * MB);
        scans.estimate_exact("tree", 700 * MB);
        scans.estimate_exact("site", 100 * MB);
        scans.pin("tree");
        scans.want("tree", 1.0); // pinned, wanted and too big
        assert!(scans.plan(0.0).load.is_empty());
        assert_eq!(scans.next_prefill(&["tree", "site"]), Some("site"));
    }

    #[test]
    fn a_pinned_scan_that_does_not_fit_warns_once() {
        let mut scans = GpuScans::new(300 * MB);
        scans.estimate_exact("tree", 700 * MB);
        scans.pin("tree");
        scans.plan(0.0);
        scans.plan(1.0);
        assert_eq!(scans.pinned_warnings, 1);
    }

    #[test]
    fn the_nearest_ask_wins_when_several_callers_want_a_scan() {
        let mut scans = GpuScans::new(100 * MB);
        scans.estimate_exact("a", 100 * MB);
        scans.estimate_exact("b", 100 * MB);
        scans.want("a", 30.0);
        scans.want("b", 20.0);
        scans.want("a", 10.0);
        assert_eq!(scans.plan(0.0).load, ["a"]);
    }

    #[test]
    fn a_shrunk_budget_loads_nothing_that_plan() {
        let mut scans = GpuScans::new(200 * MB);
        fake_loaded(&mut scans, "a", 100 * MB, 0.0);
        fake_loaded(&mut scans, "b", 100 * MB, 0.0);
        scans.set_budget(150 * MB);
        scans.estimate_exact("c", 10 * MB);
        scans.want("b", 5.0);
        scans.want("c", 1.0);
        let admission = scans.plan(1.0);
        assert_eq!((admission.evict, admission.load), (vec!["a"], vec![]));
        scans.want("b", 5.0);
        scans.want("c", 1.0);
        assert_eq!(scans.plan(2.0).load, ["c"]); // 100 MB used, 50 MB free
    }

    #[test]
    fn a_scan_still_uploading_does_not_trigger_the_over_budget_step() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "a", 100 * MB, 0.0);
        scans.estimate_exact("b", 200 * MB);
        scans.want("b", 1.0);
        assert_eq!(scans.plan(1.0).load, ["b"]); // planned, not yet measured

        // The driver's usage already holds b, which `live_scan_budget` then
        // counts as "other": the budget reads 250 MB while 300 MB are in use.
        scans.set_budget(250 * MB);
        scans.want("b", 1.0);
        let admission = scans.plan(2.0);
        assert!(admission.evict.is_empty() && admission.load.is_empty());

        // Once b is measured the 300 MB on the GPU really exceed the budget.
        scans.measured("b", Some(200 * MB));
        scans.want("b", 1.0);
        assert_eq!(scans.plan(3.0).evict, ["a"]);
    }

    #[test]
    fn uploads_in_flight_keep_their_room_from_idle_admission() {
        let mut scans = GpuScans::new(400 * MB);
        fake_loaded(&mut scans, "a", 100 * MB, 0.0); // idle by t = 200 s
        fake_loaded(&mut scans, "b", 100 * MB, 190.0);
        scans.estimate_exact("c", 200 * MB);
        scans.want("c", 1.0);
        assert_eq!(scans.plan(191.0).load, ["c"]); // uploading, not yet measured

        // The driver's usage holds c, so the budget reads 250 MB while the
        // loaded scans are 200 MB: not over it, but 400 MB are in use.
        scans.set_budget(250 * MB);
        scans.estimate_exact("d", 100 * MB);
        scans.want("d", 1.0);
        // Dropping idle a leaves 300 MB in use, still past 250 MB: d must wait.
        let admission = scans.plan(200.0);
        assert!(admission.evict.is_empty() && admission.load.is_empty());
    }

    #[test]
    fn a_pinned_scan_leaves_when_nothing_else_can() {
        let mut scans = GpuScans::new(100 * MB);
        fake_loaded(&mut scans, "tree", 100 * MB, 0.0);
        scans.pin("tree");
        scans.set_budget(50 * MB);
        assert_eq!(scans.plan(1.0).evict, ["tree"]);
        assert!(scans.plan(2.0).load.is_empty()); // it no longer fits, so it waits
    }

    #[test]
    fn an_estimate_scales_the_file_and_gives_way_to_the_measured_size() {
        let mut scans = GpuScans::new(1000 * MB);
        scans.estimate("a", 100 * MB);
        let estimated = (100.0 * MB as f32 * GPU_BYTES_PER_FILE_BYTE) as u64;
        scans.want("a", 1.0);
        scans.plan(0.0);
        assert_eq!((scans.used(), scans.resident_bytes()), (estimated, 0)); // still loading

        scans.measured("a", Some(90 * MB));
        assert_eq!((scans.used(), scans.resident_bytes()), (90 * MB, 90 * MB));
        scans.estimate("a", 500 * MB); // a later estimate must not undo the measure
        assert_eq!(scans.used(), 90 * MB);
    }

    #[test]
    fn a_load_that_ends_unmeasured_keeps_its_estimate() {
        let mut scans = GpuScans::new(1000 * MB);
        scans.estimate_exact("a", 100 * MB);
        scans.want("a", 1.0);
        scans.plan(0.0);
        scans.measured("a", None);
        assert_eq!((scans.used(), scans.resident_bytes()), (100 * MB, 100 * MB));
    }
}
