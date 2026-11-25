//! Which UHD scans hold a copy on the GPU: the budget, admission and eviction.
//!
//! Callers `want` and `pin` scans each frame and show a scan's `scene` once it
//! is on the GPU. This module keeps the books: `GpuScans::plan` says what to
//! load and what to drop. `streaming` (`GpuScansPlugin`) loads the admitted
//! scans render-only from `ram://`, measures them and learns from the render
//! world (`residency`) when they are resident, and prefills spare room with
//! the files nobody wants yet; `vram` keeps the budget on the driver's live
//! VRAM reading.

mod recovery;
mod residency;
mod streaming;
mod vram;

use std::collections::{BTreeMap, HashSet};

use bevy::log::warn;
use bevy::prelude::*;

pub use streaming::GpuScansPlugin;
use streaming::ScanHandles;

/// Seconds a scan stays on the GPU after it last showed, whatever other scans
/// want (only a budget shrunk below the resident scans drops it sooner).
pub const KEEP_SECONDS: f32 = 120.0;
/// Metres short of the range where a scan shows at which callers start wanting
/// it, so it is on the GPU by the time it is needed.
pub const UPLOAD_LEAD: f32 = 40.0;
/// GiB left free of scans, for the driver and for other programs' growth.
pub const MARGIN_GB: f32 = 0.5;
/// GiB the desktop holds, for the fallback only (0.63 GiB measured on the 2060).
pub const DESKTOP_GB: f32 = 0.6;
/// GiB the game holds without any streamed scan, measured at the map overview:
/// for the fallback only, as a live reading measures the game itself.
pub const GAME_VRAM_GB: f32 = 2.4;
/// GPU bytes per file byte of a scan (the five trees: about 700 MB from 428 MB).
pub const GPU_BYTES_PER_FILE_BYTE: f32 = 1.65;
/// GiB of the budget that prefill leaves free. The game's VRAM grows in
/// allocator steps (256 MiB before `main` asked wgpu for 8-64 MiB blocks), and
/// one such step in everything else lowers the budget by as much (0.4 GB
/// measured on the 2060 with 256 MiB blocks, just after the entry curtain). A
/// scan nobody wants yet must not sit at the budget's edge, where that dip
/// evicts it and prefill loads it back. Behind the entry curtain, while the
/// world's own uploads still land, every admission leaves it free
/// (`GpuScans::entering`); past it, wanted and pinned scans keep the full budget.
pub const PREFILL_HEADROOM_GB: f32 = 0.5;
/// Seconds a dropped scan's bytes may take to leave the driver's usage before
/// they stop holding room for other scans. Rendering lets go of a scan within
/// a few frames, but wgpu's allocator keeps freed memory in its blocks (8-64
/// MiB) until a block empties, so a free may not show; a scan loaded later
/// usually reuses it, which the reading then shows.
pub const FREE_WAIT_SECONDS: f32 = 30.0;
/// GiB the scan budget gives up for the rest of the run each time the GPU runs
/// out of memory (`GpuScans::out_of_memory`).
pub const OOM_SQUEEZE_GB: f32 = 0.5;

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
/// player's `cap`) leaves once the rest of the game is counted, less the
/// margin. `GpuScans::apply_reading` measures the rest of the game.
pub fn live_scan_budget(reading: VramReading, rest_of_game: u64, cap: Option<u64>) -> u64 {
    let ceiling = cap.map_or(reading.budget, |cap| cap.min(reading.budget));
    ceiling
        .saturating_sub(rest_of_game)
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
    /// Seconds on the caller's clock when it last showed (was wanted at a
    /// distance of 0 or less); `None` for a scan never shown, such as a
    /// prefilled one.
    last_shown: Option<f32>,
    /// The nearest ask this frame (see `GpuScans::want`), cleared by `plan`.
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

    /// When it last showed, for ranking: a scan never shown is the oldest.
    fn shown_at(&self) -> f32 {
        self.last_shown.unwrap_or(f32::NEG_INFINITY)
    }

    /// Where it queues to make way for another scan, first to leave first:
    /// those nobody wants, least recently shown first, then those wanted,
    /// furthest from showing first.
    fn leave_order(&self) -> (bool, f32) {
        match self.wanted {
            None => (false, self.shown_at()),
            Some(distance) => (true, -distance),
        }
    }
}

/// Bytes of a scan dropped at `at` seconds, which the driver's usage may still hold.
#[derive(Clone, Copy, Debug)]
struct Free {
    bytes: u64,
    at: f32,
}

/// The scans that may hold a GPU copy, within a byte budget.
///
/// A scan is off the GPU, loading (admitted, size not yet measured) or loaded.
/// Loading and loaded scans both count in `used`. A loaded scan is resident
/// once the render world has prepared all of it; only resident scans count in
/// `resident_bytes`.
#[derive(Resource, Default)]
pub struct GpuScans {
    /// What the resident scans may hold before one must leave. Admission
    /// fills less: see `budget()`.
    budget: u64,
    /// The most the rest of the game (all but the scans) has held, by the
    /// live readings so far; `None` until one comes, and for good where the
    /// driver cannot report VRAM.
    rest_peak: Option<u64>,
    /// Scans dropped whose bytes the last reading may still hold, oldest first.
    frees: Vec<Free>,
    /// Bytes of `frees` that the reading had held for `FREE_WAIT_SECONDS`.
    stuck: u64,
    /// A wanted scan loaded into the room of stuck frees (`try_stuck_room`),
    /// and the stuck bytes then: the first reading once it lands says
    /// whether it reused them.
    trying: Option<(&'static str, u64)>,
    /// No scan tries stuck room before this time: the last one did not reuse it.
    next_try: f32,
    /// Bytes every budget leaves free on top of the margin, grown by each
    /// time the GPU ran out of memory.
    squeeze: u64,
    /// The world still uploads behind the entry curtain: every admission
    /// leaves `PREFILL_HEADROOM_GB` free.
    entering: bool,
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

    /// Books for a run whose world still uploads behind the entry curtain:
    /// until `entered`, every admission leaves `PREFILL_HEADROOM_GB` free, as
    /// a prefill always does. Those uploads raise the rest of the game by
    /// about as much before the curtain lifts (from 1.8-2.0 GiB at the first
    /// reading to 2.4 GiB on the 2060), and the budget shrinks with them.
    pub fn entering(budget: u64) -> Self {
        Self {
            entering: true,
            ..Self::new(budget)
        }
    }

    /// The entry curtain lifted: wanted and pinned scans may fill the whole budget.
    pub fn entered(&mut self) {
        self.entering = false;
    }

    /// A new live budget; the next `plan` evicts down to it if needed.
    pub fn set_budget(&mut self, budget: u64) {
        self.budget = budget;
    }

    /// Takes the budget from a driver reading at `now` seconds, and says
    /// whether it did.
    ///
    /// The reading's usage beyond the resident scans is the rest of the game
    /// plus the scans dropped that the driver has not freed yet (`frees`).
    /// The budget leaves room for the most the rest of the game has held
    /// (`rest_peak`), so a scan leaving never shrinks it. Frees the reading
    /// no longer holds, freed or reused by a scan loaded since, are forgotten,
    /// oldest first; one it still holds after `FREE_WAIT_SECONDS` no longer
    /// holds room for new scans (`budget()`).
    ///
    /// While a scan uploads, the reading may already hold part of it, which
    /// would count as the rest of the game, and for good: so the last budget
    /// stays until the render world confirms it.
    pub fn apply_reading(&mut self, reading: VramReading, cap: Option<u64>, now: f32) -> bool {
        if self.uploading() {
            return false;
        }
        let rest = reading.usage.saturating_sub(self.resident_bytes());
        let peak = self.rest_peak.unwrap_or(0);
        self.settle_frees(rest.saturating_sub(peak));
        let peak = peak.max(rest.saturating_sub(self.freeing()));
        self.rest_peak = Some(peak);
        self.note_stuck_frees(now);
        self.judge_try(now);
        self.set_budget(live_scan_budget(reading, peak, cap).saturating_sub(self.squeeze));
        debug!(
            "VRAM: usage {} MB, scans {} MB, rest of the game {} MB at most, freeing {} MB ({} MB stuck); budget {} MB",
            reading.usage >> 20,
            self.resident_bytes() >> 20,
            peak >> 20,
            self.freeing() >> 20,
            self.stuck >> 20,
            self.budget >> 20,
        );
        true
    }

    /// The GPU ran out of memory at `now`: drops every scan on the GPU or on
    /// its way there, as the failed allocation may be one of theirs, and
    /// leaves `OOM_SQUEEZE_GB` more free from now on. Returns the dropped
    /// scans; the caller frees them (`unload`). They come back as the
    /// smaller budget allows, their stand-ins showing meanwhile.
    pub fn out_of_memory(&mut self, now: f32) -> Vec<&'static str> {
        let dropped: Vec<_> = self
            .scans
            .iter()
            .filter(|(_, scan)| scan.state != State::Off)
            .map(|(&path, _)| path)
            .collect();
        for &path in &dropped {
            self.drop_scan(path, now);
        }
        self.squeeze += gib(OOM_SQUEEZE_GB);
        self.budget = self.budget.saturating_sub(gib(OOM_SQUEEZE_GB));
        dropped
    }

    /// What scans may fill: the live budget less the bytes of dropped scans
    /// the driver has held for `FREE_WAIT_SECONDS`, which may never come free.
    /// Those still count as scan memory for the resident scans, though, as
    /// dropping another scan for them would free nothing either, and a wanted
    /// scan may still try that room (`try_stuck_room`).
    pub fn budget(&self) -> u64 {
        self.budget.saturating_sub(self.stuck)
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
    /// declines (it does not fit without dropping others, which a pin
    /// nobody wants never does) and a failed load (confirmed with nothing on
    /// the GPU) hold nothing up.
    pub fn settled(&self) -> bool {
        self.scans.values().all(|scan| match scan.state {
            State::Off => !scan.pinned || !self.fits(scan.bytes),
            State::Loading => false,
            State::Loaded => scan.resident,
        })
    }

    /// Loads before every unpinned scan and never leaves for one, but only
    /// within the budget. It does not evict others to load unless it is
    /// wanted (a tree near the camera): then it may drop unpinned scans that
    /// make way (`make_room`).
    pub fn pin(&mut self, path: &'static str) {
        self.scan_mut(path).pinned = true;
    }

    /// Asks for the scan this frame. `distance` is how far, in metres, the
    /// asking anchor is from where the scan would show: 0 or less means it
    /// shows now (and counts as shown), more means it is wanted ahead of
    /// showing, from `UPLOAD_LEAD` out. Every caller measures alike, as scans
    /// queue to load and make way for each other by it: a wanted scan may
    /// drop one wanted further from showing that does not show now and has
    /// not shown for `KEEP_SECONDS`. The smallest distance of several asks stands.
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
    /// stamped as shown: until it shows, it is the first to leave.
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
    /// room. The over-budget step counts only the resident scans, the ones the
    /// reading holds apart from the rest of the game, against the live budget
    /// without `budget()`'s cut for frees that never showed: a scan still
    /// uploading must not make a healthy budget look overcommitted, nor must
    /// memory that dropping a scan does not give back make more scans leave.
    pub fn plan(&mut self, now: f32) -> Admission {
        let mut admission = Admission::default();
        self.begin_plan(now);
        if self.resident_bytes() > self.budget {
            // The budget shrank. Drop one scan per plan: the reading's numbers lag the GPU's frees.
            admission.evict.extend(self.evict_over_budget(now));
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
        self.budget().saturating_sub(self.used())
    }

    /// Whether `bytes` more fit the budget without dropping anything, less
    /// the headroom behind the entry curtain: all a pinned scan nobody wants
    /// may ask, as it never evicts others.
    fn fits(&self, bytes: u64) -> bool {
        self.deficit(bytes + self.reserve()) == 0
    }

    /// Bytes admission leaves free: `PREFILL_HEADROOM_GB` behind the entry
    /// curtain (`entering`), none past it.
    fn reserve(&self) -> u64 {
        if self.entering {
            gib(PREFILL_HEADROOM_GB)
        } else {
            0
        }
    }

    /// Whether `bytes` more fit while `PREFILL_HEADROOM_GB` stays free: the
    /// one rule for what a prefill may take, kept by both the choice of file
    /// and its admission. The same headroom behind the curtain as past it.
    fn fits_prefill(&self, bytes: u64) -> bool {
        self.deficit(bytes + gib(PREFILL_HEADROOM_GB)) == 0
    }

    /// Bytes that must leave before `bytes` more fit the budget. Unlike
    /// `room()`, exact when the uploads in flight already put `used()` past the
    /// budget, where `room()` reads 0 and hides how far.
    fn deficit(&self, bytes: u64) -> u64 {
        (self.used() + bytes).saturating_sub(self.budget())
    }

    /// Stamps every scan showing this frame (wanted at a distance of 0 or
    /// less) as shown now, and forgets who waited.
    fn begin_plan(&mut self, now: f32) {
        for scan in self.scans.values_mut() {
            if scan.wanted.is_some_and(|distance| distance <= 0.0) {
                scan.last_shown = Some(now);
            }
            scan.waiting = false;
        }
    }

    /// Takes the scan off the GPU at `now`. Once a live reading has come,
    /// its bytes count as freeing until a reading shows them gone.
    fn drop_scan(&mut self, path: &'static str, now: f32) {
        let scan = self.scan_mut(path);
        scan.state = State::Off;
        let bytes = scan.bytes;
        if self.rest_peak.is_some() {
            self.frees.push(Free { bytes, at: now });
        }
    }

    /// A scan is loaded and not yet confirmed on the GPU. One still loading
    /// has nothing there: its parts reach the render world in the frame it
    /// is measured.
    fn uploading(&self) -> bool {
        self.scans
            .values()
            .any(|scan| scan.state == State::Loaded && !scan.resident)
    }

    /// Bytes of the dropped scans the last reading may still hold.
    fn freeing(&self) -> u64 {
        self.frees.iter().map(|free| free.bytes).sum()
    }

    /// Forgets frees, oldest first, until they total no more than `held`,
    /// what the reading holds beyond the rest of the game's peak.
    fn settle_frees(&mut self, held: u64) {
        let mut gone = self.freeing().saturating_sub(held);
        self.frees.retain_mut(|free| {
            let settled = free.bytes.min(gone);
            gone -= settled;
            free.bytes -= settled;
            free.bytes > 0
        });
    }

    /// Counts the frees held for `FREE_WAIT_SECONDS` at `now`, and logs when there are more.
    fn note_stuck_frees(&mut self, now: f32) {
        let stuck = self
            .frees
            .iter()
            .filter(|free| now - free.at >= FREE_WAIT_SECONDS)
            .map(|free| free.bytes)
            .sum();
        if stuck > self.stuck {
            info!(
                "UHD scans dropped {FREE_WAIT_SECONDS} s ago or more still hold {} MB of the driver's usage; new scans no longer count on it",
                stuck >> 20
            );
        }
        self.stuck = stuck;
    }

    /// For when the resident scans alone (`resident_bytes`) exceed the budget:
    /// drops the resident scan nobody wants this frame that showed least
    /// recently (one never shown, such as a prefilled one, first), unpinned
    /// before pinned. Scans still loading or uploading are neither counted nor
    /// dropped.
    fn evict_over_budget(&mut self, now: f32) -> Option<&'static str> {
        let (&path, _) = self
            .scans
            .iter()
            .filter(|(_, scan)| {
                scan.state == State::Loaded && scan.resident && scan.wanted.is_none()
            })
            .min_by(|(_, a), (_, b)| {
                a.pinned
                    .cmp(&b.pinned)
                    .then(a.shown_at().total_cmp(&b.shown_at()))
            })?;
        self.drop_scan(path, now);
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

    /// Loads the scan if it fits, after dropping scans that make way for it
    /// when it must. A scan asked for this frame may drop them, pinned or
    /// not; a pinned scan nobody asks for (the startup admission) loads only
    /// into room already free. Pinned scans never make way, so none drops
    /// another. Loading is not showing: the scan is stamped once it shows.
    fn admit(&mut self, path: &'static str, now: f32, admission: &mut Admission) {
        let Some(scan) = self.scans.get(path) else {
            return;
        };
        let (bytes, pinned, wanted) = (scan.bytes, scan.pinned, scan.wanted);
        if !pinned && wanted.is_none() {
            // Only prefilled: no eviction, no stamp, and no waiting, which a
            // real ask does.
            if self.fits_prefill(bytes) {
                self.scan_mut(path).state = State::Loading;
                admission.load.push(path);
            }
            return;
        }
        let fits = self.fits(bytes)
            || wanted.is_some_and(|distance| {
                self.make_room(bytes, distance, now, &mut admission.evict)
                    || self.try_stuck_room(path, bytes, now)
            });
        if fits {
            self.scan_mut(path).state = State::Loading;
            admission.load.push(path);
        } else if pinned {
            self.warn_pinned_waits(path, bytes);
        } else {
            self.scan_mut(path).waiting = true;
        }
    }

    /// Drops the scans that make way for one wanted at `distance`, in their
    /// order, until `bytes` more fit, and says whether they do. Drops
    /// nothing when even all of them would not make room.
    fn make_room(
        &mut self,
        bytes: u64,
        distance: f32,
        now: f32,
        evicted: &mut Vec<&'static str>,
    ) -> bool {
        let needed = bytes + self.reserve();
        let ways = self.making_way(distance, now);
        let freed: u64 = ways.iter().map(|path| self.scans[path].bytes).sum();
        if self.deficit(needed) > freed {
            return false;
        }
        for path in ways {
            if self.deficit(needed) == 0 {
                break;
            }
            self.drop_scan(path, now);
            evicted.push(path);
        }
        true
    }

    /// The scans that make way for one wanted at `distance`, first to leave
    /// first (`Scan::leave_order`): loaded, unpinned, not shown for
    /// `KEEP_SECONDS` (a scan never shown, at once), and either wanted by
    /// nobody or wanted further from showing than it and not showing now.
    fn making_way(&self, distance: f32, now: f32) -> Vec<&'static str> {
        let mut ways: Vec<_> = self
            .scans
            .iter()
            .filter(|(_, scan)| {
                scan.state == State::Loaded
                    && !scan.pinned
                    && now - scan.shown_at() >= KEEP_SECONDS
                    && scan.wanted.is_none_or(|wanted| wanted > distance.max(0.0))
            })
            .collect();
        ways.sort_by(|(_, a), (_, b)| {
            let (a, b) = (a.leave_order(), b.leave_order());
            a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
        });
        ways.into_iter().map(|(&path, _)| path).collect()
    }

    /// Whether the wanted scan may load into the room of stuck frees, and
    /// notes it if so. wgpu usually places a new scan in the blocks a dropped
    /// one left, which only a reading after the new scan lands can show, so
    /// without a try that room would stay lost for the run. One scan tries at
    /// a time, within the live budget; if it went to a new block instead, the
    /// driver's usage rises by at most that scan, inside `MARGIN_GB`.
    fn try_stuck_room(&mut self, path: &'static str, bytes: u64, now: f32) -> bool {
        let fits_live = self.used() + bytes + self.reserve() <= self.budget;
        let may = self.trying.is_none() && now >= self.next_try && bytes <= self.stuck;
        if may && fits_live {
            self.trying = Some((path, self.stuck));
        }
        may && fits_live
    }

    /// Once the scan trying stuck room has landed (or failed to load): if the
    /// stuck bytes did not fall, it took a new block, and the next try waits
    /// `FREE_WAIT_SECONDS`. Frees turning stuck meanwhile read as a miss too,
    /// which only delays the next try.
    fn judge_try(&mut self, now: f32) {
        let Some((path, stuck_before)) = self.trying else {
            return;
        };
        let scan = &self.scans[path];
        if scan.state != State::Off && !scan.resident {
            return;
        }
        self.trying = None;
        if self.stuck >= stuck_before {
            self.next_try = now + FREE_WAIT_SECONDS;
        }
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
                self.budget() >> 20,
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

    /// When the scan last showed, as `plan` stamped it; `None` if never.
    pub fn last_shown(&self, path: &'static str) -> Option<f32> {
        self.scans.get(path)?.last_shown
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
    fn fake_loaded(scans: &mut GpuScans, path: &'static str, bytes: u64, shown: f32) {
        scans.want(path, 0.0);
        assert_eq!(scans.plan(shown).load, [path], "{path} needs room");
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
        // T1200: driver budget 3.9 GiB; the rest of the game uses 2.5 GiB
        let reading = VramReading {
            budget: gib(3.9),
            usage: gib(3.0),
        };
        assert_eq!(
            live_scan_budget(reading, gib(2.5), None),
            gib(3.9 - 2.5 - MARGIN_GB)
        );
        assert_eq!(
            live_scan_budget(reading, gib(2.5), Some(gib(3.5))),
            gib(3.5 - 2.5 - MARGIN_GB)
        );
        assert_eq!(live_scan_budget(reading, gib(3.6), None), 0);
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

        // None of b is on the GPU while it loads (its parts reach the render
        // world in the frame it is measured), so the reading is taken in:
        // behind the curtain, where scans load one after another, the budget
        // follows the driver.
        let loading = VramReading {
            budget: gib(6.0),
            usage: gib(2.0) + 300 * MB,
        };
        assert!(scans.apply_reading(loading, None, 2.0), "loading");
        let budget = live_scan_budget(loading, gib(2.0), None);
        assert_eq!(scans.budget(), budget);

        // Once measured, the driver's usage may hold some or all of b, which
        // would read as the rest of the game.
        let uploading = VramReading {
            usage: gib(2.0) + 700 * MB,
            ..loading
        };
        scans.measured("b", Some(400 * MB)); // the render world has not confirmed it
        assert!(!scans.apply_reading(uploading, None, 3.0), "uploading");
        assert_eq!(scans.budget(), budget, "the last budget stays");

        confirm(&mut scans, "b");
        assert!(scans.apply_reading(uploading, None, 4.0));
        assert_eq!(scans.budget(), budget);
    }

    /// Books with "a" (shown at 0 s) and "b" (shown at 10 s), 100 MB each,
    /// and the reading taken in at 20 s that leaves them exactly the budget.
    fn two_scans_at_the_budget() -> (GpuScans, VramReading) {
        let mut scans = GpuScans::new(200 * MB);
        fake_loaded(&mut scans, "a", 100 * MB, 0.0);
        fake_loaded(&mut scans, "b", 100 * MB, 10.0);
        let rest = gib(2.5); // the rest of the game, as on the 2060
        let reading = VramReading {
            budget: rest + 200 * MB + gib(MARGIN_GB),
            usage: rest + 200 * MB,
        };
        assert!(scans.apply_reading(reading, None, 20.0));
        assert_eq!(scans.budget(), 200 * MB);
        (scans, reading)
    }

    #[test]
    fn running_out_of_memory_drops_every_scan_and_squeezes_the_budget() {
        let (mut scans, full) = two_scans_at_the_budget();
        scans.estimate_exact("c", 10 * MB);
        scans.want("c", 0.0);
        scans.set_budget(gib(1.0));
        assert_eq!(scans.plan(20.0).load, ["c"]);

        assert_eq!(scans.out_of_memory(21.0), ["a", "b", "c"], "loading too");
        assert_eq!(scans.resident_bytes(), 0);
        assert_eq!(scans.budget(), gib(1.0) - gib(OOM_SQUEEZE_GB));

        // Every reading from now on leaves the squeeze free as well.
        let roomy = VramReading {
            budget: full.budget + gib(1.0),
            usage: full.usage - 200 * MB,
        };
        assert!(scans.apply_reading(roomy, None, 60.0));
        assert_eq!(scans.budget(), 200 * MB + gib(1.0) - gib(OOM_SQUEEZE_GB));
    }

    #[test]
    fn an_eviction_does_not_shrink_the_budget_before_the_driver_frees_it() {
        let (mut scans, full) = two_scans_at_the_budget();
        // Another program takes 50 MB: "a" leaves.
        let tighter = VramReading {
            budget: full.budget - 50 * MB,
            ..full
        };
        assert!(scans.apply_reading(tighter, None, 21.0));
        assert_eq!(scans.plan(21.0).evict, ["a"]);

        // The driver has not freed "a" yet: its bytes are not the rest of the game.
        assert!(scans.apply_reading(tighter, None, 22.0));
        assert_eq!(scans.budget(), 150 * MB);
        assert!(scans.plan(22.0).evict.is_empty(), "b stays");

        // Once the driver frees it, the rest of the game growing back by as
        // much is the game's own.
        let freed = VramReading {
            usage: tighter.usage - 100 * MB,
            ..tighter
        };
        assert!(scans.apply_reading(freed, None, 23.0));
        assert_eq!(scans.budget(), 150 * MB);
        assert!(scans.apply_reading(tighter, None, 24.0));
        assert_eq!(scans.budget(), 50 * MB);

        // A free the driver never shows holds room only for a while.
        let (mut scans, _) = two_scans_at_the_budget();
        assert!(scans.apply_reading(tighter, None, 21.0));
        assert_eq!(scans.plan(21.0).evict, ["a"]);
        let waited = 21.0 + FREE_WAIT_SECONDS;
        assert!(scans.apply_reading(tighter, None, waited - 1.0));
        assert_eq!(scans.budget(), 150 * MB, "it still holds its room");
        assert!(scans.apply_reading(tighter, None, waited));
        assert_eq!(scans.budget(), 50 * MB, "it no longer does");
        assert!(
            scans.plan(waited).evict.is_empty(),
            "nor does b leave for it: that would free nothing either"
        );
    }

    /// `two_scans_at_the_budget` after another program took 50 MB: "a" left
    /// at 21 s and the driver still holds its 100 MB `FREE_WAIT_SECONDS`
    /// later, so they are stuck. Returns the books, that reading and the time.
    fn a_stuck_free() -> (GpuScans, VramReading, f32) {
        let (mut scans, full) = two_scans_at_the_budget();
        let tighter = VramReading {
            budget: full.budget - 50 * MB,
            ..full
        };
        assert!(scans.apply_reading(tighter, None, 21.0));
        assert_eq!(scans.plan(21.0).evict, ["a"]);
        let waited = 21.0 + FREE_WAIT_SECONDS;
        assert!(scans.apply_reading(tighter, None, waited));
        assert_eq!(scans.budget(), 50 * MB, "b fills it");
        (scans, tighter, waited)
    }

    #[test]
    fn a_wanted_scan_loads_into_stuck_room_and_frees_it_when_it_reuses_it() {
        let (mut scans, tighter, now) = a_stuck_free();
        scans.estimate_exact("c", 30 * MB);
        scans.want("c", 0.0);
        assert_eq!(scans.plan(now).load, ["c"], "the live budget has room");
        // Before it lands, a reading tells nothing.
        assert!(scans.apply_reading(tighter, None, now + 0.5));
        scans.measured("c", Some(30 * MB));
        confirm(&mut scans, "c");

        // The driver's usage did not rise: "c" took over 30 MB of a's bytes.
        assert!(scans.apply_reading(tighter, None, now + 1.0));
        assert_eq!(scans.budget(), 80 * MB, "70 MB still stuck");
        scans.estimate_exact("d", 20 * MB);
        scans.want("d", 0.0);
        assert_eq!(scans.plan(now + 1.0).load, ["d"], "the next may try too");
    }

    #[test]
    fn a_scan_that_does_not_reuse_stuck_room_stops_the_next_trying_for_a_while() {
        let (mut scans, tighter, now) = a_stuck_free();
        scans.estimate_exact("c", 30 * MB);
        scans.want("c", 0.0);
        assert_eq!(scans.plan(now).load, ["c"]);
        scans.measured("c", Some(30 * MB));
        confirm(&mut scans, "c");

        // The driver's usage rose by all of it: it went to a new block.
        let fresh = VramReading {
            usage: tighter.usage + 30 * MB,
            ..tighter
        };
        assert!(scans.apply_reading(fresh, None, now + 1.0));
        scans.estimate_exact("d", 10 * MB);
        scans.want("d", 0.0);
        assert!(scans.plan(now + 1.0).load.is_empty(), "d waits");
        let later = now + 1.0 + FREE_WAIT_SECONDS;
        assert!(scans.apply_reading(fresh, None, later));
        scans.want("d", 0.0);
        assert_eq!(scans.plan(later).load, ["d"]);
    }

    #[test]
    fn the_reserve_is_the_high_water_mark_of_the_rest_of_the_game_plus_the_margin() {
        let driver = gib(4.0);
        // Takes in a reading where the rest of the game uses `rest`.
        let read = |scans: &mut GpuScans, rest: u64, at: f32| {
            let usage = rest + scans.resident_bytes();
            let reading = VramReading {
                budget: driver,
                usage,
            };
            assert!(scans.apply_reading(reading, None, at));
            scans.budget()
        };
        let mut scans = GpuScans::new(0);
        assert_eq!(
            read(&mut scans, gib(1.0), 0.0),
            driver - gib(1.0) - gib(MARGIN_GB),
            "not the 2.4 GiB of a fixed reserve"
        );
        fake_loaded(&mut scans, "a", 300 * MB, 0.0);
        let peak = driver - gib(1.5) - gib(MARGIN_GB);
        assert_eq!(read(&mut scans, gib(1.5), 1.0), peak, "the rise counts");
        assert_eq!(read(&mut scans, gib(1.2), 2.0), peak, "the dip does not");
    }

    /// Plans a 150 MB site scan wanted at `site` metres from showing, at 1 s,
    /// into a 300 MB budget full of 100 MB plant scans, each loaded at 0 s
    /// while wanted at its distance from showing, and wanted there still.
    fn plan_a_site_among_plants(plants: [(&'static str, f32); 3], site: f32) -> Admission {
        let mut scans = GpuScans::new(300 * MB);
        let want_plants = |scans: &mut GpuScans| {
            for (path, distance) in plants {
                scans.estimate_exact(path, 100 * MB);
                scans.want(path, distance);
            }
        };
        want_plants(&mut scans);
        assert_eq!(scans.plan(0.0).load.len(), 3);
        for (path, _) in plants {
            scans.measured(path, Some(100 * MB));
            confirm(&mut scans, path);
        }
        want_plants(&mut scans);
        scans.estimate_exact("site", 150 * MB);
        scans.want("site", site);
        scans.plan(1.0)
    }

    #[test]
    fn a_site_about_to_show_displaces_plant_scans_not_yet_showing() {
        let plants = [("plant 10", 10.0), ("plant 20", 20.0), ("plant 30", 30.0)];
        let admission = plan_a_site_among_plants(plants, 0.0);
        assert_eq!(
            admission.evict,
            ["plant 30", "plant 20"],
            "the furthest first"
        );
        assert_eq!(admission.load, ["site"]);
    }

    #[test]
    fn a_scan_showing_now_is_never_displaced() {
        // The site is well inside its range, but every plant shows already.
        let plants = [("plant -8", -8.0), ("plant -3", -3.0), ("plant 0", 0.0)];
        let admission = plan_a_site_among_plants(plants, -10.0);
        assert!(admission.evict.is_empty(), "{:?}", admission.evict);
        assert!(admission.load.is_empty(), "the site waits");
    }

    #[test]
    fn behind_the_curtain_every_admission_keeps_the_headroom_free() {
        let headroom = gib(PREFILL_HEADROOM_GB);
        let mut scans = GpuScans::entering(headroom + 200 * MB);
        scans.estimate_exact("tree", 250 * MB);
        scans.pin("tree");
        scans.estimate_exact("plant", 100 * MB);
        scans.want("plant", 0.0);
        let load = scans.plan(0.0).load;
        assert_eq!(load, ["plant"], "the tree would sit in the headroom");
        scans.measured("plant", Some(100 * MB));
        confirm(&mut scans, "plant");
        assert!(scans.settled(), "nor does the curtain wait for it");

        scans.entered();
        assert_eq!(scans.plan(1.0).load, ["tree"], "the curtain is up");
    }

    #[test]
    fn behind_the_curtain_a_prefill_leaves_the_headroom_once() {
        let mut scans = GpuScans::entering(gib(PREFILL_HEADROOM_GB) + 100 * MB);
        scans.estimate_exact("site", 100 * MB);
        assert_eq!(scans.next_prefill(&["site"]), Some("site"));
        scans.prefill("site");
        assert_eq!(scans.plan(0.0).load, ["site"]);
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
    fn a_scan_shown_within_two_minutes_never_leaves() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "a", 250 * MB, 0.0);
        scans.want("a", 0.0);
        scans.plan(100.0); // still shown at t = 100 s
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
    fn a_wanted_pin_drops_idle_scans_for_room_but_an_unwanted_one_waits() {
        let headroom = gib(PREFILL_HEADROOM_GB);
        let mut scans = GpuScans::new(headroom + 250 * MB);
        scans.estimate_exact("prefilled", 200 * MB);
        scans.prefill("prefilled");
        assert_eq!(scans.plan(1.0).load, ["prefilled"]);
        scans.measured("prefilled", Some(200 * MB));
        confirm(&mut scans, "prefilled");
        // It does not fit beside the never-shown prefill.
        scans.estimate_exact("tree", headroom + 100 * MB);
        scans.pin("tree");

        // Startup admission: nobody is near, and a pin loads only into free room.
        let admission = scans.plan(10.0);
        assert!(admission.evict.is_empty() && admission.load.is_empty());

        scans.want("tree", 3.0);
        let admission = scans.plan(11.0);
        assert_eq!(
            (admission.evict, admission.load),
            (vec!["prefilled"], vec!["tree"])
        );
    }

    #[test]
    fn a_wanted_pin_never_drops_another_pin() {
        let mut scans = GpuScans::new(300 * MB);
        fake_loaded(&mut scans, "other", 200 * MB, 0.0);
        scans.pin("other");
        scans.estimate_exact("tree", 200 * MB);
        scans.pin("tree");
        scans.want("tree", 1.0);
        // "other" has been idle far longer than KEEP_SECONDS, but it is pinned.
        let admission = scans.plan(1_000.0);
        assert!(admission.evict.is_empty() && admission.load.is_empty());
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
