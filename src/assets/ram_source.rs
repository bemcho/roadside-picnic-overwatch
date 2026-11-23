//! The `ram://` asset source: the UHD scans the game can show, read from
//! `assets/` once behind the splash while the machine has RAM to spare, so
//! later loads decode from memory and no file is opened during play. A scan
//! left out loads from `assets/` through the same source. Native only; the web
//! build loads from `assets/`.

mod reader;

use std::collections::VecDeque;
use std::error::Error;
use std::io::SeekFrom;
use std::path::Path;

use bevy::asset::io::AssetSourceId;
use bevy::asset::io::memory::Dir;
use bevy::asset::{AsyncReadExt, AsyncSeekExt};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};

use crate::states::AppState;

use super::UhdFiles;

pub use reader::{RamDir, register_ram_source};

/// The source's name, as in `ram://models/trees/uhd/tree_big.glb`.
pub const RAM_SOURCE: &str = "ram";

/// Files read at once: each open file holds an 8 MiB read pipe.
const READS_IN_FLIGHT: usize = 4;

/// RAM the fill leaves free for the rest of the game and the machine: 2 GiB.
/// The game's RAM grows about 1.7 GiB after the fill while scans stream, so
/// less would push a tight host into swap.
const RAM_HEADROOM: u64 = 2 << 30;

/// Bytes per GB in the log; GB here are GiB, as in the VRAM budget.
const GIB: f64 = (1u64 << 30) as f64;

/// A file that could not be opened or read: its path and why.
type FileError = (&'static str, String);

/// Every listed file's length in list order, or the first that would not open.
type Measured = Result<Vec<(&'static str, u64)>, FileError>;

/// Reads the UHD files into RAM behind the splash, as many as fit.
pub struct RamSourcePlugin;

impl Plugin for RamSourcePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start_ram_fill).add_systems(
            Update,
            collect_ram_files.run_if(in_state(AppState::Loading)),
        );
    }
}

/// How far the RAM fill has come: the files being measured, their lengths,
/// the files still to read, the reads running, the files left on disk and the
/// first file that could not be read.
#[derive(Resource)]
pub struct RamFill {
    measuring: Option<Task<Measured>>,
    waiting: VecDeque<(&'static str, u64)>,
    /// Each read running, with its file's length.
    reads: Vec<(u64, Task<Result<(), FileError>>)>,
    file_lens: Vec<(&'static str, u64)>,
    skipped: Vec<&'static str>,
    failed: Option<&'static str>,
    started: Instant,
}

impl RamFill {
    fn new(measuring: Option<Task<Measured>>) -> Self {
        Self {
            measuring,
            waiting: VecDeque::new(),
            reads: Vec::new(),
            file_lens: Vec::new(),
            skipped: Vec::new(),
            failed: None,
            started: Instant::now(),
        }
    }

    /// Measures the listed files on the IO pool; reads start once every
    /// length is known.
    fn start(server: &AssetServer, paths: Vec<&'static str>) -> Self {
        let server = server.clone();
        let measuring = IoTaskPool::get().spawn(async move { measure_files(&server, paths).await });
        Self::new(Some(measuring))
    }

    /// Every listed file is in RAM or left on disk.
    pub fn is_full(&self) -> bool {
        self.measuring.is_none()
            && self.waiting.is_empty()
            && self.reads.is_empty()
            && self.failed.is_none()
    }

    /// Takes the measured lengths and the finished reads.
    fn collect(&mut self) {
        if let Some(measured) = self.measuring.as_mut().and_then(check_ready) {
            self.measuring = None;
            match measured {
                Ok(lens) => {
                    self.waiting = lens.iter().copied().collect();
                    self.file_lens = lens;
                }
                Err(err) => fail(&mut self.failed, err),
            }
        }
        let failed = &mut self.failed;
        self.reads.retain_mut(|(_, read)| match check_ready(read) {
            None => true,
            Some(Ok(())) => false,
            Some(Err(err)) => {
                fail(failed, err);
                false
            }
        });
    }

    /// Starts the next files in list order, up to `READS_IN_FLIGHT` at once,
    /// while each fits in the `available` RAM (`None`: no limit); none after
    /// a failure. The first file that does not fit ends the fill.
    fn start_reads(&mut self, server: &AssetServer, ram: &Dir, available: Option<u64>) {
        let pool = IoTaskPool::get();
        while self.can_start() {
            let Some(&(path, len)) = self.waiting.front() else {
                break;
            };
            if let Some(available) = available
                && !fits_in_ram(available, self.in_flight(), len)
            {
                self.leave_rest_on_disk(available);
                break;
            }
            self.waiting.pop_front();
            let server = server.clone();
            let ram = ram.clone();
            let read = pool.spawn(async move { read_into_ram(&server, &ram, path).await });
            self.reads.push((len, read));
        }
    }

    /// A file waits and a read slot is free, and nothing has failed.
    fn can_start(&self) -> bool {
        !self.waiting.is_empty() && self.reads.len() < READS_IN_FLIGHT && self.failed.is_none()
    }

    /// Bytes of the reads running: a read's buffer is not committed until it
    /// is filled, so the free RAM does not count them yet.
    fn in_flight(&self) -> u64 {
        self.reads.iter().map(|(len, _)| len).sum()
    }

    /// Leaves every file not yet started on disk.
    fn leave_rest_on_disk(&mut self, available: u64) {
        self.skipped
            .extend(self.waiting.drain(..).map(|(path, _)| path));
        let (n, bytes) = self.in_ram();
        let total = self.file_lens.len();
        let gb = bytes as f64 / GIB;
        let free = available as f64 / GIB;
        info!(
            "RAM fill stopped at {n} of {total} UHD files ({gb:.1} GB): {free:.1} GB free; the rest load from disk"
        );
    }

    /// How many files the fill reads into RAM (read or reading), and their bytes.
    fn in_ram(&self) -> (usize, u64) {
        let skipped = self.skipped();
        let kept = self
            .file_lens()
            .iter()
            .filter(|(path, _)| !skipped.contains(path));
        kept.fold((0, 0), |(n, bytes), (_, len)| (n + 1, bytes + len))
    }

    /// The first file that could not be read.
    pub fn failed(&self) -> Option<&str> {
        self.failed
    }

    /// Every listed file's length in bytes, in RAM or left on disk; all of
    /// them once the fill is full.
    pub fn file_lens(&self) -> &[(&'static str, u64)] {
        &self.file_lens
    }

    /// The files left on disk because RAM ran short, in list order.
    pub fn skipped(&self) -> &[&'static str] {
        &self.skipped
    }

    /// A fill whose one read never finishes.
    #[cfg(test)]
    pub fn test_pending() -> Self {
        let pool = IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        Self {
            reads: vec![(0, pool.spawn(std::future::pending()))],
            ..Self::new(None)
        }
    }

    /// A finished fill, failed on `failed` if given.
    #[cfg(test)]
    pub fn test_done(failed: Option<&'static str>) -> Self {
        Self {
            failed,
            ..Self::new(None)
        }
    }
}

/// Logs why a file could not be read; the first such file fails the fill.
fn fail(failed: &mut Option<&'static str>, (path, err): FileError) {
    error!("Reading {path} failed: {err}");
    failed.get_or_insert(path);
}

/// Starts measuring the listed UHD files for the RAM fill on the IO pool.
fn start_ram_fill(mut commands: Commands, server: Res<AssetServer>, files: Res<UhdFiles>) {
    commands.insert_resource(RamFill::start(&server, files.0.clone()));
}

/// Collects the lengths and the finished reads, starts the next files that
/// fit in RAM and logs the total once the fill is full.
fn collect_ram_files(mut fill: ResMut<RamFill>, server: Res<AssetServer>, ram: Res<RamDir>) {
    if fill.measuring.is_none() && fill.reads.is_empty() {
        return;
    }
    fill.collect();
    // `/proc/meminfo` is read only when a file can start.
    if fill.can_start() {
        fill.start_reads(&server, &ram.0, available_ram());
    }
    if fill.is_full() {
        let (n, bytes) = fill.in_ram();
        let gb = bytes as f64 / GIB;
        let s = fill.started.elapsed().as_secs_f64();
        info!("Read {n} UHD files ({gb:.1} GB) into RAM in {s:.1} s");
    }
}

/// Whether a file of `next` bytes fits in the `available` RAM beside the
/// reads `in_flight`, with `RAM_HEADROOM` to spare.
fn fits_in_ram(available: u64, in_flight: u64, next: u64) -> bool {
    available.saturating_sub(in_flight).saturating_sub(next) >= RAM_HEADROOM
}

/// The RAM the machine has free, in bytes; `None` reads every file. Linux
/// kills a process that runs out of memory, so the fill keeps within
/// `MemAvailable` there.
#[cfg(target_os = "linux")]
fn available_ram() -> Option<u64> {
    // A platform read, allowed despite the "no `std::fs` in game code" rule:
    // that rule keeps the wasm32 build working, and this never builds there.
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
    let available = meminfo.as_deref().and_then(mem_available);
    if available.is_none() {
        warn_once!("No MemAvailable in /proc/meminfo; reading every UHD file into RAM");
    }
    available
}

/// Windows and macOS page memory out instead of killing the game, so the
/// fill reads every file.
#[cfg(not(target_os = "linux"))]
fn available_ram() -> Option<u64> {
    None
}

/// `MemAvailable` in bytes, from the text of `/proc/meminfo`.
#[cfg(target_os = "linux")]
fn mem_available(meminfo: &str) -> Option<u64> {
    let line = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().next()?.parse().ok()?;
    kib.checked_mul(1024)
}

/// Opens each listed file in turn for its length, without reading it.
async fn measure_files(server: &AssetServer, paths: Vec<&'static str>) -> Measured {
    let mut lens = Vec::with_capacity(paths.len());
    for path in paths {
        let len = file_len(server, path)
            .await
            .map_err(|err| (path, err.to_string()))?;
        lens.push((path, len));
    }
    Ok(lens)
}

/// Reads one file from `assets/` into the RAM folder; its bytes move in uncopied.
async fn read_into_ram(
    server: &AssetServer,
    ram: &Dir,
    path: &'static str,
) -> Result<(), FileError> {
    let bytes = read_file(server, path)
        .await
        .map_err(|err| (path, err.to_string()))?;
    ram.insert_asset(Path::new(path), bytes);
    Ok(())
}

/// A file's length through Bevy's default (`assets/`) source, from a seek to
/// its end.
async fn file_len(server: &AssetServer, path: &str) -> Result<u64, Box<dyn Error + Send + Sync>> {
    let source = server.get_source(AssetSourceId::Default)?;
    let mut reader = source.reader().read(Path::new(path)).await?;
    Ok(reader.seekable()?.seek(SeekFrom::End(0)).await?)
}

/// Reads a whole file through Bevy's default (`assets/`) source into a buffer
/// of exactly its size: `read_to_end` grows its buffer while reading and would
/// keep up to twice the file's size in RAM.
async fn read_file(
    server: &AssetServer,
    path: &str,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let source = server.get_source(AssetSourceId::Default)?;
    let mut reader = source.reader().read(Path::new(path)).await?;
    let file = reader.seekable()?;
    let len = file.seek(SeekFrom::End(0)).await?;
    file.seek(SeekFrom::Start(0)).await?;
    let mut bytes = vec![0; usize::try_from(len)?];
    file.read_exact(&mut bytes).await?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use bevy::asset::AssetPlugin;
    use bevy::asset::io::{AssetSourceId, Reader};
    use bevy::state::app::StatesPlugin;
    use bevy::tasks::block_on;

    use super::*;
    use crate::assets::UhdFiles;
    use crate::states::AppState;

    /// A new, empty folder for one test's `assets/`.
    fn fresh_dir() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "roadside-picnic-ram-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test's assets folder");
        dir
    }

    /// An app with the asset server and the `ram` source over a fresh
    /// `assets/` that holds `on_disk`, and no fill.
    pub(super) fn asset_app(on_disk: &[(&str, &[u8])]) -> (App, RamDir) {
        let root = fresh_dir();
        for (path, bytes) in on_disk {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("a file inside a folder"))
                .expect("create the file's folder");
            std::fs::write(file, bytes).expect("write the test file");
        }
        let root = root.to_string_lossy().into_owned();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        register_ram_source(&mut app, &root);
        app.add_plugins((
            AssetPlugin {
                file_path: root,
                ..default()
            },
            StatesPlugin,
        ))
        .init_state::<AppState>();
        let ram = app.world().resource::<RamDir>().clone();
        (app, ram)
    }

    fn filling_app(on_disk: &[(&str, &[u8])], listed: &[&'static str]) -> (App, RamDir) {
        let (mut app, ram) = asset_app(on_disk);
        app.insert_resource(UhdFiles(listed.to_vec()))
            .add_plugins(RamSourcePlugin);
        (app, ram)
    }

    /// Updates until `done`, pausing briefly so the IO pool can read.
    fn update_until(app: &mut App, done: impl Fn(&App) -> bool) {
        for _ in 0..500 {
            app.update();
            if done(app) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("still not done after 500 updates");
    }

    /// Reads a file back through the asset server's `ram://` source.
    pub(super) async fn read_ram(app: &App, path: &str) -> Vec<u8> {
        let server = app.world().resource::<AssetServer>();
        let source = server
            .get_source(RAM_SOURCE)
            .expect("the ram source is registered");
        let mut reader = source
            .reader()
            .read(Path::new(path))
            .await
            .expect("the ram source has the file");
        let mut bytes = Vec::new();
        Reader::read_to_end(&mut reader, &mut bytes)
            .await
            .expect("read the file from RAM");
        bytes
    }

    #[test]
    fn uhd_paths_use_the_ram_source_on_native() {
        let path = crate::assets::uhd("models/trees/uhd/tree_big.glb");
        assert_eq!(path.source(), &AssetSourceId::from(RAM_SOURCE));
        assert_eq!(path.path(), Path::new("models/trees/uhd/tree_big.glb"));
    }

    #[test]
    fn filling_reads_every_listed_file_into_ram() {
        let (mut app, ram) = filling_app(
            &[
                ("models/a/uhd/one.glb", b"one"),
                ("models/b/uhd/two.glb", b"two"),
            ],
            &["models/a/uhd/one.glb", "models/b/uhd/two.glb"],
        );
        update_until(&mut app, |app| app.world().resource::<RamFill>().is_full());
        assert_eq!(
            ram.0
                .get_asset(Path::new("models/a/uhd/one.glb"))
                .map(|d| d.value().to_vec()),
            Some(b"one".to_vec())
        );
        let read = block_on(read_ram(&app, "models/b/uhd/two.glb"));
        assert_eq!(read, b"two");
    }

    #[test]
    fn the_fill_reads_a_few_files_at_a_time_in_list_order() {
        let names: Vec<String> = (0..10).map(|i| format!("models/n/uhd/{i}.glb")).collect();
        let listed: Vec<&'static str> = names
            .into_iter()
            .map(|name| &*Box::leak(name.into_boxed_str()))
            .collect();
        let on_disk: Vec<(&str, &[u8])> = listed.iter().map(|&path| (path, &b"x"[..])).collect();
        let (mut app, _) = filling_app(&on_disk, &listed);
        update_until(&mut app, |app| {
            let fill = app.world().resource::<RamFill>();
            assert!(
                fill.reads.len() <= READS_IN_FLIGHT,
                "too many reads at once"
            );
            assert!(
                fill.waiting
                    .iter()
                    .map(|(path, _)| path)
                    .eq(&listed[listed.len() - fill.waiting.len()..]),
                "files start out of list order"
            );
            fill.is_full()
        });
        assert_eq!(app.world().resource::<RamFill>().file_lens().len(), 10);
    }

    #[test]
    fn a_file_fits_only_with_the_headroom_to_spare() {
        assert!(fits_in_ram(RAM_HEADROOM + 100, 0, 100));
        assert!(!fits_in_ram(RAM_HEADROOM + 99, 0, 100), "one byte short");
        assert!(
            !fits_in_ram(RAM_HEADROOM + 100, 1, 100),
            "reads in flight count"
        );
        assert!(fits_in_ram(RAM_HEADROOM + 150, 50, 100));
        assert!(!fits_in_ram(10, 20, 30), "less free than the reads");
    }

    #[test]
    fn the_fill_stops_at_the_first_file_that_does_not_fit() {
        let (app, ram) = asset_app(&[
            ("models/a/uhd/one.glb", &[1; 10]),
            ("models/b/uhd/two.glb", &[2; 20]),
            ("models/c/uhd/three.glb", &[3; 30]),
        ]);
        let server = app.world().resource::<AssetServer>().clone();
        let listed = vec![
            "models/a/uhd/one.glb",
            "models/b/uhd/two.glb",
            "models/c/uhd/three.glb",
        ];
        let mut fill = RamFill::start(&server, listed);
        // Room for the first two files only, both in flight at once.
        let available = RAM_HEADROOM + 30;
        for _ in 0..500 {
            fill.collect();
            fill.start_reads(&server, &ram.0, Some(available));
            if fill.is_full() || fill.failed().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(fill.is_full());
        assert_eq!(fill.failed(), None);
        assert!(ram.0.get_asset(Path::new("models/a/uhd/one.glb")).is_some());
        assert!(ram.0.get_asset(Path::new("models/b/uhd/two.glb")).is_some());
        assert!(
            ram.0
                .get_asset(Path::new("models/c/uhd/three.glb"))
                .is_none()
        );
        assert_eq!(fill.skipped(), &["models/c/uhd/three.glb"]);
        assert_eq!(
            fill.file_lens(),
            &[
                ("models/a/uhd/one.glb", 10),
                ("models/b/uhd/two.glb", 20),
                ("models/c/uhd/three.glb", 30),
            ]
        );
        // The skipped file still loads through the `ram` source, from disk.
        assert_eq!(block_on(read_ram(&app, "models/c/uhd/three.glb")), [3; 30]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mem_available_is_read_in_bytes() {
        let meminfo = "MemTotal:       31728204 kB\n\
                       MemFree:         9409400 kB\n\
                       MemAvailable:   14940608 kB\n\
                       Buffers:          123456 kB\n";
        assert_eq!(mem_available(meminfo), Some(14_940_608 * 1024));
        assert_eq!(mem_available("MemTotal: 1 kB\n"), None);
        assert_eq!(mem_available("MemAvailable: lots kB\n"), None);
    }

    #[test]
    fn a_missing_file_fails_the_fill() {
        let (mut app, _) = filling_app(&[], &["models/x/uhd/missing.glb"]);
        update_until(&mut app, |app| {
            app.world().resource::<RamFill>().failed().is_some()
        });
        assert_eq!(
            app.world().resource::<RamFill>().failed(),
            Some("models/x/uhd/missing.glb")
        );
        assert!(!app.world().resource::<RamFill>().is_full());
    }
}
