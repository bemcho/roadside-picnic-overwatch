//! Which UHD scans hold a copy on the GPU: the budget, admission and eviction.
//!
//! Callers `want` and `pin` scans each frame and show a scan's `scene` once it
//! is on the GPU. This module keeps the books: `GpuScans::plan` says what to
//! load and what to drop. `streaming` (`GpuScansPlugin`) loads the admitted
//! scans render-only from `ram://`, measures them and learns from the render
//! world (`residency`) when they are resident, and prefills spare room with
//! the files nobody wants yet; `vram` keeps the budget on the driver's live
//! VRAM reading.

mod residency;
mod streaming;
mod vram;

use std::collections::{BTreeMap, HashSet};

use bevy::log::warn;
use bevy::prelude::*;

pub use streaming::GpuScansPlugin;
use streaming::ScanHandles;

/// Seconds a scan stays on the GPU after it was last wanted, whatever the pressure.
pub const KEEP_SECONDS: f32 = 120.0;
/// Metres short of the range where a scan shows at which callers start wanting
/// it, so it is on the GPU by the time it is needed.
pub const UPLOAD_LEAD: f32 = 40.0;
/// GiB left free of scans, for the driver and for other programs' growth.
pub const MARGIN_GB: f32 = 0.5;
/// GiB the desktop holds, for the fallback only (0.63 GiB measured on the 2060).
pub const DESKTOP_GB: f32 = 0.6;
/// GiB the game holds without any streamed scan, measured at the map overview.
pub const GAME_VRAM_GB: f32 = 2.4;
/// GPU bytes per file byte of a scan (the five trees: about 700 MB from 428 MB).
pub const GPU_BYTES_PER_FILE_BYTE: f32 = 1.65;
/// GiB of the budget that prefill leaves free. The game's VRAM grows in 256 MiB
/// allocator steps, and one such step in everything else lowers the budget by
/// as much (0.4 GB measured on the 2060, just after the entry curtain). A
/// scan nobody wants yet must not sit at the budget's edge, where that dip
/// evicts it and prefill loads it back. Wanted and pinned scans keep the full
/// budget.
pub const PREFILL_HEADROOM_GB: f32 = 0.5;

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
/// margin. `usage - resident_scans` is everything else the game holds; it
/// never counts for less than `GAME_VRAM_GB`, so a reading taken while the
/// world's own uploads are still landing does not over-admit.
pub fn live_scan_budget(reading: VramReading, resident_scans: u64, cap: Option<u64>) -> u64 {
    let ceiling = cap.map_or(reading.budget, |cap| cap.min(reading.budget));
    let others = reading
        .usage
        .saturating_sub(resident_scans)
        .max(gib(GAME_VRAM_GB));
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

#[derive(Default)]
struct Scan {
    state: State,
    /// GPU bytes: an estimate from the file until `measured` reports the real size.
    bytes: u64,
    is_measured: bool,
    pinned: bool,
    /// Seconds on the caller's clock when it was last wanted; `None` for a
    /// scan nobody has wanted, such as a prefilled one.
    last_used: Option<f32>,
    /// The nearest ask this frame, cleared by `plan`.
    wanted: Option<f32>,
    /// Proposed by `prefill` this frame, cleared by `plan`.
    speculative: bool,
    /// The last `plan` found no room for it, though it is wanted.
    waiting: bool,
    /// Held from admission until eviction.
    handles: Option<ScanHandles>,
    /// Loaded, and the render world confirmed every mesh and image of it (a
    /// failed load is confirmed at once, with nothing on the GPU).
    resident: bool,
}

impl Scan {
    /// Where it queues among scans waiting to load: the nearest ask first.
    fn distance(&self) -> f32 {
        self.wanted.unwrap_or(f32::INFINITY)
    }

    /// When it was last wanted, for ranking: a scan nobody wanted is the oldest.
    fn used_at(&self) -> f32 {
        self.last_used.unwrap_or(f32::NEG_INFINITY)
    }
}

/// The scans that may hold a GPU copy, within a byte budget.
///
/// A scan is off the GPU, loading (admitted, size not yet measured) or loaded.
/// Loading and loaded scans both count in `used`. A loaded scan is resident
/// once the render world has prepared all of it; only resident scans count in
/// `resident_bytes`.
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

    /// Takes the budget from a driver reading, and says whether it did. While
    /// a scan is loaded but not yet confirmed on the GPU, the reading may
    /// already hold it, and `live_scan_budget` would count it as everything
    /// else: near a full budget `plan` would then evict a scan it should
    /// keep. So the last budget stays until the confirmation.
    pub fn apply_reading(&mut self, reading: VramReading, cap: Option<u64>) -> bool {
        let uploading = self
            .scans
            .values()
            .any(|scan| scan.state == State::Loaded && !scan.resident);
        if !uploading {
            self.set_budget(live_scan_budget(reading, self.resident_bytes(), cap));
        }
        !uploading
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// Bytes of the scans the render world confirmed on the GPU: what the
    /// driver's usage already holds. Past the budget, `plan` drops scans. A
    /// scan still loading or uploading is not counted, so it never triggers that.
    pub fn resident_bytes(&self) -> u64 {
        self.bytes_where(|scan| scan.state == State::Loaded && scan.resident)
    }

    /// Bytes of the scans loading or loaded.
    pub fn used(&self) -> u64 {
        self.bytes_where(|scan| scan.state != State::Off)
    }

    /// The scan's scene, once every mesh and image of it is on the GPU.
    pub fn scene(&self, path: &'static str) -> Option<Handle<WorldAsset>> {
        let scan = self.scans.get(path).filter(|scan| scan.resident)?;
        scan.handles.as_ref()?.scene.clone()
    }

    /// No scan loading or waiting for the render world to confirm it, and
    /// every pinned scan that fits on the GPU. A pinned scan admission
    /// declines (it does not fit without dropping others, which pins never
    /// do) and a failed load (confirmed with nothing on the GPU) hold nothing up.
    pub fn settled(&self) -> bool {
        self.scans.values().all(|scan| match scan.state {
            State::Off => !scan.pinned || !self.fits(scan.bytes),
            State::Loading => false,
            State::Loaded => scan.resident,
        })
    }

    /// Loads before every unpinned scan and never leaves for one, but only
    /// within the budget.
    pub fn pin(&mut self, path: &'static str) {
        self.scan_mut(path).pinned = true;
    }

    /// Asks for the scan this frame; the smallest distance of several asks stands.
    pub fn want(&mut self, path: &'static str, distance: f32) {
        let scan = self.scan_mut(path);
        scan.wanted = Some(
            scan.wanted
                .map_or(distance, |nearest| nearest.min(distance)),
        );
    }

    /// Proposes the scan to the next `plan` as a speculative load. It queues
    /// after every ask, loads only into room that stays `PREFILL_HEADROOM_GB`
    /// short of the budget, never makes another scan leave, and is not
    /// stamped as used: until someone wants it, it counts as never shown and
    /// is the first to leave.
    pub fn prefill(&mut self, path: &'static str) {
        self.scan_mut(path).speculative = true;
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
    /// loading and drops as off, and clears this frame's wants and prefills.
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
            scan.speculative = false;
        }
        admission
    }

    /// The first file of `order` that is off and that `plan` would admit as a
    /// prefill (`fits_prefill`), while no scan is loading and no wanted scan
    /// waits (as of the last `plan`). A scan whose load failed (measured at 0
    /// bytes) is skipped, or prefill would load it again each time it leaves.
    /// A pinned scan too big for the card is not wanted and never holds it back.
    pub fn next_prefill(&self, order: &[&'static str]) -> Option<&'static str> {
        if self
            .scans
            .values()
            .any(|scan| scan.state == State::Loading || scan.waiting)
        {
            return None;
        }
        order.iter().copied().find(|path| {
            self.scans.get(path).is_some_and(|scan| {
                let failed = scan.is_measured && scan.bytes == 0;
                scan.state == State::Off && !failed && self.fits_prefill(scan.bytes)
            })
        })
    }

    fn scan_mut(&mut self, path: &'static str) -> &mut Scan {
        self.scans.entry(path).or_default()
    }

    fn bytes_where(&self, counted: impl Fn(&Scan) -> bool) -> u64 {
        self.scans
            .values()
            .filter(|scan| counted(scan))
            .map(|scan| scan.bytes)
            .sum()
    }

    fn room(&self) -> u64 {
        self.budget.saturating_sub(self.used())
    }

    /// Whether `bytes` more fit the budget without dropping anything: all a
    /// pinned scan may ask, as it never evicts others.
    fn fits(&self, bytes: u64) -> bool {
        self.deficit(bytes) == 0
    }

    /// Whether `bytes` more fit while `PREFILL_HEADROOM_GB` stays free: the
    /// one rule for what a prefill may take, kept by both the choice of file
    /// and its admission.
    fn fits_prefill(&self, bytes: u64) -> bool {
        self.fits(bytes + gib(PREFILL_HEADROOM_GB))
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
                scan.last_used = Some(now);
            }
            scan.waiting = false;
        }
    }

    /// For when the resident scans alone (`resident_bytes`) exceed the budget:
    /// drops the oldest resident scan not shown this frame (one nobody ever
    /// wanted, such as a prefilled one, is the oldest), unpinned before
    /// pinned. Scans still loading or uploading are neither counted nor dropped.
    fn evict_over_budget(&mut self) -> Option<&'static str> {
        let (&path, _) = self
            .scans
            .iter()
            .filter(|(_, scan)| {
                scan.state == State::Loaded && scan.resident && scan.wanted.is_none()
            })
            .min_by(|(_, a), (_, b)| {
                a.pinned
                    .cmp(&b.pinned)
                    .then(a.used_at().total_cmp(&b.used_at()))
            })?;
        self.scan_mut(path).state = State::Off;
        Some(path)
    }

    /// Scans to load, pinned first, then the nearest ask, then the prefills.
    fn pending(&self) -> Vec<&'static str> {
        let mut pending: Vec<_> = self
            .scans
            .iter()
            .filter(|(_, scan)| {
                scan.state == State::Off
                    && (scan.pinned || scan.wanted.is_some() || scan.speculative)
            })
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
        if !pinned && scan.wanted.is_none() {
            // Only prefilled: no eviction, no stamp, and no waiting, which a
            // real ask does.
            if self.fits_prefill(bytes) {
                self.scan_mut(path).state = State::Loading;
                admission.load.push(path);
            }
            return;
        }
        // A pinned scan never evicts others: it loads only into room already free.
        let fits =
            self.fits(bytes) || (!pinned && self.free_idle(bytes, now, &mut admission.evict));
        if fits {
            let scan = self.scan_mut(path);
            scan.state = State::Loading;
            scan.last_used = Some(now);
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

    /// Loaded, unpinned scans unused for `KEEP_SECONDS` (a scan nobody wanted
    /// is idle at once), oldest first.
    fn idle(&self, now: f32) -> Vec<&'static str> {
        let mut idle: Vec<_> = self
            .scans
            .iter()
            .filter(|(_, scan)| {
                scan.state == State::Loaded && !scan.pinned && now - scan.used_at() >= KEEP_SECONDS
            })
            .collect();
        idle.sort_by(|(_, a), (_, b)| a.used_at().total_cmp(&b.used_at()));
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

#[cfg(test)]
impl GpuScans {
    /// Puts a scan on the GPU as `load` and the render world's confirmation
    /// would leave it, for tests of the callers that wait for it.
    pub fn insert_resident(&mut self, path: &'static str, scene: Handle<WorldAsset>) {
        let scan = self.scan_mut(path);
        scan.state = State::Loaded;
        scan.resident = true;
        scan.handles = Some(ScanHandles {
            gltf: Handle::default(),
            scene: Some(scene),
        });
    }

    /// When the scan was last shown, as `plan` stamped it; `None` if never.
    pub fn last_used(&self, path: &'static str) -> Option<f32> {
        self.scans.get(path)?.last_used
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

    /// The render world confirms the loaded scan, as `note_resident_scans` would.
    fn confirm(scans: &mut GpuScans, path: &'static str) {
        scans.scan_mut(path).resident = true;
    }

    /// Wants the scan, plans it in with room to spare, reports its size, and
    /// confirms it on the GPU.
    fn fake_loaded(scans: &mut GpuScans, path: &'static str, bytes: u64, last_used: f32) {
        scans.want(path, 0.0);
        assert_eq!(scans.plan(last_used).load, [path], "{path} needs room");
        scans.measured(path, Some(bytes));
        confirm(scans, path);
    }

    /// Gives a scan the handles `load` would, without an asset server.
    fn fake_handles(scans: &mut GpuScans, path: &'static str) {
        scans.scan_mut(path).handles = Some(ScanHandles {
            gltf: Handle::default(),
            scene: None,
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

        scans.loaded("a", Handle::default(), Some(12 * MB));
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
        scans.pin("tree");
        assert!(!scans.settled(), "a pinned scan that fits is off the GPU");

        assert_eq!(scans.plan(0.0).load, ["tree"]);
        fake_handles(&mut scans, "tree");
        assert!(!scans.settled(), "loading");
        scans.loaded("tree", Handle::default(), Some(10 * MB));
        assert!(!scans.settled(), "waiting for the render world");
        scans.mark_resident("tree");
        assert!(scans.settled());
    }

    #[test]
    fn a_pinned_scan_admission_declines_does_not_hold_settled() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "site", 100 * MB, 0.0);
        scans.estimate_exact("tree", 250 * MB);
        scans.pin("tree");
        scans.want("site", 0.0);
        assert!(
            scans.plan(1.0).load.is_empty(),
            "a pin never evicts the site"
        );
        assert!(
            scans.settled(),
            "the tree cannot fit, so nothing waits for it"
        );

        scans.set_budget(400 * MB); // room after all: it loads next plan
        assert!(!scans.settled());
        assert_eq!(scans.plan(2.0).load, ["tree"]);
    }

    #[test]
    fn a_failed_load_holds_neither_settled_nor_room() {
        let mut scans = GpuScans::new(100 * MB);
        scans.estimate_exact("site", 60 * MB);
        scans.want("site", 0.0);
        assert_eq!(scans.plan(0.0).load, ["site"]);
        fake_handles(&mut scans, "site");
        assert!(!scans.settled(), "loading");

        scans.fail("site");
        assert!(scans.settled());
        assert_eq!(scans.scene("site"), None);
        assert_eq!((scans.used(), scans.resident_bytes()), (0, 0));
        scans.estimate_exact("other", 60 * MB);
        scans.want("site", 0.0);
        scans.want("other", 1.0);
        assert_eq!(scans.plan(1.0).load, ["other"], "its room is free");
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
        // Early in the run the world's own uploads are still landing: the
        // 1.8 GiB in use count as the game's settled 2.4 GiB.
        let early = VramReading {
            budget: gib(4.9),
            usage: gib(1.8),
        };
        assert_eq!(
            live_scan_budget(early, 0, None),
            gib(4.9) - gib(GAME_VRAM_GB) - gib(MARGIN_GB)
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
    fn a_reading_taken_while_a_scan_uploads_keeps_the_last_budget() {
        // GiB as bytes, independent of the production conversion.
        let gib = |gb: f32| (gb * (1u64 << 30) as f32) as u64;
        let mut scans = GpuScans::new(gib(2.0));
        fake_loaded(&mut scans, "a", 300 * MB, 0.0);
        scans.estimate_exact("b", 400 * MB);
        scans.want("b", 0.0);
        assert_eq!(scans.plan(1.0).load, ["b"]);
        scans.measured("b", Some(400 * MB)); // loaded; the render world has not confirmed it

        // The driver's usage already holds b: it reads as 400 MB of "everything else".
        let reading = VramReading {
            budget: gib(6.0),
            usage: gib(GAME_VRAM_GB) + 700 * MB,
        };
        assert!(!scans.apply_reading(reading, None));
        assert_eq!(scans.budget(), gib(2.0), "the last budget stays");

        confirm(&mut scans, "b");
        assert!(scans.apply_reading(reading, None));
        assert_eq!(scans.budget(), live_scan_budget(reading, 700 * MB, None));
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
        confirm(&mut scans, "b");

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
        let headroom = gib(PREFILL_HEADROOM_GB);
        let mut scans = GpuScans::new(headroom + 300 * MB);
        fake_loaded(&mut scans, "held", 100 * MB, 0.0); // 200 MB are free beyond the headroom
        scans.estimate_exact("big", headroom + 250 * MB);
        scans.estimate_exact("small", 50 * MB);
        scans.estimate_exact("tiny", 10 * MB);
        let order = ["held", "big", "small", "tiny"];
        assert_eq!(scans.next_prefill(&order), Some("small")); // held is on, big does not fit

        scans.want("big", 1.0); // a wanted scan waits for room
        assert!(scans.plan(1.0).load.is_empty());
        assert_eq!(scans.next_prefill(&order), None);

        scans.plan(2.0); // nobody wants it any more
        assert_eq!(scans.next_prefill(&order), Some("small"));

        scans.prefill("small"); // planned in but not yet measured
        assert_eq!(scans.plan(3.0).load, ["small"]);
        assert_eq!(scans.next_prefill(&order), None);
        scans.measured("small", Some(50 * MB));
        assert_eq!(scans.next_prefill(&order), Some("tiny"));
    }

    #[test]
    fn prefill_stops_short_of_the_budget_so_a_dip_in_it_evicts_nothing() {
        let headroom = gib(PREFILL_HEADROOM_GB);
        let files = ["a", "b", "c"];
        let mut scans = GpuScans::new(headroom + 200 * MB);
        for path in files {
            scans.estimate_exact(path, 100 * MB);
        }
        // What prefill_scans and admit_scans do, one scan per round.
        while let Some(path) = scans.next_prefill(&files) {
            scans.prefill(path);
            assert_eq!(scans.plan(0.0).load, [path]);
            scans.measured(path, Some(100 * MB));
            confirm(&mut scans, path);
        }
        assert_eq!(scans.used(), 200 * MB, "c would sit in the headroom");

        // A dip in the budget just under the headroom (the 2060's was 0.4 GB).
        scans.set_budget(scans.budget() - (headroom - MB));
        let admission = scans.plan(1.0);
        assert!(admission.evict.is_empty() && admission.load.is_empty());

        scans.set_budget(scans.budget() - 2 * MB); // a dip past the headroom
        assert_eq!(scans.plan(2.0).evict.len(), 1);
    }

    #[test]
    fn a_real_want_evicts_a_prefilled_scan_that_was_never_shown() {
        let headroom = gib(PREFILL_HEADROOM_GB);
        let mut scans = GpuScans::new(headroom + 250 * MB);
        scans.estimate_exact("prefilled", 200 * MB);
        scans.prefill("prefilled");
        assert_eq!(scans.plan(1.0).load, ["prefilled"]);
        scans.measured("prefilled", Some(200 * MB));
        confirm(&mut scans, "prefilled");

        // Well inside KEEP_SECONDS, and it needs more than the free room.
        scans.estimate_exact("shown", headroom + 100 * MB);
        scans.want("shown", 1.0);
        let admission = scans.plan(10.0);
        assert_eq!(
            (admission.evict, admission.load),
            (vec!["prefilled"], vec!["shown"])
        );
    }

    #[test]
    fn over_budget_a_never_shown_prefilled_scan_leaves_before_one_shown_60_seconds_ago() {
        let mut scans = GpuScans::new(gib(PREFILL_HEADROOM_GB) + 300 * MB);
        fake_loaded(&mut scans, "shown", 100 * MB, 40.0);
        scans.estimate_exact("prefilled", 100 * MB);
        scans.prefill("prefilled");
        assert_eq!(scans.plan(50.0).load, ["prefilled"]);
        scans.measured("prefilled", Some(100 * MB));
        confirm(&mut scans, "prefilled");

        scans.set_budget(150 * MB); // another program took VRAM at t = 100 s
        assert_eq!(scans.plan(100.0).evict, ["prefilled"]);
    }

    #[test]
    fn a_prefill_that_would_sit_in_the_headroom_at_admission_is_not_admitted() {
        let headroom = gib(PREFILL_HEADROOM_GB);
        let mut scans = GpuScans::new(headroom + 200 * MB);
        scans.estimate_exact("a", 100 * MB);
        assert_eq!(scans.next_prefill(&["a"]), Some("a"));
        scans.prefill("a");

        scans.set_budget(headroom + 50 * MB); // the budget fell before the plan
        assert!(scans.plan(1.0).load.is_empty());
        assert_eq!(scans.used(), 0);
        assert_eq!(scans.next_prefill(&["a"]), None);
    }

    #[test]
    fn a_scan_measured_at_zero_bytes_is_not_prefilled_again() {
        // What a failed load leaves once the over-budget step has evicted it.
        let mut scans = GpuScans::new(gib(PREFILL_HEADROOM_GB) + 300 * MB);
        scans.measured("failed", Some(0));
        scans.estimate_exact("ok", 10 * MB);
        assert_eq!(scans.next_prefill(&["failed", "ok"]), Some("ok"));
        assert_eq!(scans.next_prefill(&["failed"]), None);
    }

    #[test]
    fn a_pinned_scan_that_does_not_fit_never_stops_the_prefill() {
        let headroom = gib(PREFILL_HEADROOM_GB);
        let mut scans = GpuScans::new(headroom + 300 * MB);
        scans.estimate_exact("tree", 2 * headroom);
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

        // Measured, b may still be uploading: it does not count yet either.
        scans.measured("b", Some(200 * MB));
        scans.want("b", 1.0);
        assert!(scans.plan(3.0).evict.is_empty());

        // Once the render world confirms b, 300 MB on the GPU exceed the budget.
        confirm(&mut scans, "b");
        scans.want("b", 1.0);
        assert_eq!(scans.plan(4.0).evict, ["a"]);
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
        assert_eq!((scans.used(), scans.resident_bytes()), (90 * MB, 0)); // uploading
        confirm(&mut scans, "a");
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
        confirm(&mut scans, "a");
        assert_eq!((scans.used(), scans.resident_bytes()), (100 * MB, 100 * MB));
    }
}
