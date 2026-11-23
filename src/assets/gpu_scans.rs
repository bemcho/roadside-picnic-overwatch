//! Which UHD scans hold a copy on the GPU: the budget, admission and eviction.
//!
//! Pure bookkeeping, no Bevy loading: callers `want` and `pin` scans each
//! frame, `plan` says what to load and what to drop, and `measured` reports
//! the size a finished load took.

// Wired into the app by Task 5 (GpuScansPlugin); remove this then.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, HashSet};

use bevy::log::warn;
use bevy::prelude::*;

/// Seconds a scan stays on the GPU after it was last wanted, whatever the pressure.
pub const KEEP_SECONDS: f32 = 120.0;
/// Metres short of the range where a scan shows at which callers start wanting
/// it, so it is on the GPU by the time it is needed.
// Used by the stream systems of Tasks 6 and 7; remove the attribute then.
#[cfg_attr(test, allow(dead_code))]
pub const UPLOAD_LEAD: f32 = 40.0;
/// GiB left free of scans, for the driver and for other programs' growth.
pub const MARGIN_GB: f32 = 0.5;
/// GiB the desktop holds, for the fallback only (0.63 GiB measured on the 2060).
pub const DESKTOP_GB: f32 = 0.6;
/// GiB the game holds without any streamed scan, measured at the map overview.
pub const GAME_VRAM_GB: f32 = 2.4;
/// GPU bytes per file byte of a scan (the five trees: about 700 MB from 428 MB).
pub const GPU_BYTES_PER_FILE_BYTE: f32 = 1.65;

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
/// Loading and loaded scans both count in `used`.
#[derive(Resource, Default)]
pub struct GpuScans {
    budget: u64,
    // Ordered, so equal distances and ages resolve the same way every run.
    scans: BTreeMap<&'static str, Scan>,
    warned: HashSet<&'static str>,
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

    #[test]
    fn the_fallback_budget_is_vram_minus_the_game_the_desktop_and_a_margin() {
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
        assert_eq!(scans.warned.len(), 1);
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
