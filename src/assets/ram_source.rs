//! The `ram://` asset source: every UHD scan the game can show, read from
//! `assets/` once behind the splash, so later loads decode from memory and no
//! file is opened during play. Native only; the web build loads from `assets/`.

use std::collections::VecDeque;
use std::error::Error;
use std::io::SeekFrom;
use std::path::Path;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::asset::{AsyncReadExt, AsyncSeekExt};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};

use crate::states::AppState;

use super::UhdFiles;

/// The source's name, as in `ram://models/trees/uhd/tree_big.glb`.
pub const RAM_SOURCE: &str = "ram";

/// Files read at once: each open file holds an 8 MiB read pipe.
const READS_IN_FLIGHT: usize = 4;

/// Bytes per GB in the log; GB here are GiB, as in the VRAM budget.
const GIB: f64 = (1u64 << 30) as f64;

/// The in-memory folder behind the source, shared with its reader.
#[derive(Resource, Clone)]
pub struct RamDir(pub Dir);

/// One file read into RAM: its path and length, or its path and why it failed.
type FileRead = Result<(&'static str, u64), (&'static str, String)>;

/// Registers the `ram://` source. Asset sources are built with `AssetPlugin`,
/// so this runs before `DefaultPlugins`.
pub fn register_ram_source(app: &mut App) {
    let dir = Dir::default();
    let root = dir.clone();
    app.register_asset_source(
        RAM_SOURCE,
        AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: root.clone() })),
    )
    .insert_resource(RamDir(dir));
}

/// Reads every UHD file into RAM behind the splash.
pub struct RamSourcePlugin;

impl Plugin for RamSourcePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, start_ram_fill).add_systems(
            Update,
            collect_ram_files.run_if(in_state(AppState::Loading)),
        );
    }
}

/// How far the RAM fill has come: the files still to read, the reads
/// running, the files read and the first file that could not be read.
#[derive(Resource)]
pub struct RamFill {
    waiting: VecDeque<&'static str>,
    reads: Vec<Task<FileRead>>,
    file_lens: Vec<(&'static str, u64)>,
    failed: Option<&'static str>,
    started: Instant,
}

impl RamFill {
    fn new(waiting: VecDeque<&'static str>) -> Self {
        Self {
            waiting,
            reads: Vec::new(),
            file_lens: Vec::new(),
            failed: None,
            started: Instant::now(),
        }
    }

    /// Every listed file is in RAM.
    pub fn is_full(&self) -> bool {
        self.waiting.is_empty() && self.reads.is_empty() && self.failed.is_none()
    }

    /// Starts the next files in list order, up to `READS_IN_FLIGHT` at once;
    /// none after a failure.
    fn start_reads(&mut self, server: &AssetServer, ram: &Dir) {
        let pool = IoTaskPool::get();
        while self.reads.len() < READS_IN_FLIGHT && self.failed.is_none() {
            let Some(path) = self.waiting.pop_front() else {
                break;
            };
            let server = server.clone();
            let ram = ram.clone();
            let read = pool.spawn(async move { read_into_ram(&server, &ram, path).await });
            self.reads.push(read);
        }
    }

    /// The first file that could not be read.
    pub fn failed(&self) -> Option<&str> {
        self.failed
    }

    /// Each file read so far, with its length in bytes.
    pub fn file_lens(&self) -> &[(&'static str, u64)] {
        &self.file_lens
    }

    /// A fill whose one read never finishes.
    #[cfg(test)]
    pub fn test_pending() -> Self {
        let pool = IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        Self {
            reads: vec![pool.spawn(std::future::pending())],
            ..Self::new(VecDeque::new())
        }
    }

    /// A finished fill, failed on `failed` if given.
    #[cfg(test)]
    pub fn test_done(failed: Option<&'static str>) -> Self {
        Self {
            failed,
            ..Self::new(VecDeque::new())
        }
    }
}

/// Starts reading the listed UHD files into RAM on the IO pool.
fn start_ram_fill(
    mut commands: Commands,
    server: Res<AssetServer>,
    files: Res<UhdFiles>,
    ram: Res<RamDir>,
) {
    let mut fill = RamFill::new(files.0.iter().copied().collect());
    fill.start_reads(&server, &ram.0);
    commands.insert_resource(fill);
}

/// Collects the finished reads, starts the next files and logs the total once
/// every file is in RAM.
fn collect_ram_files(mut fill: ResMut<RamFill>, server: Res<AssetServer>, ram: Res<RamDir>) {
    if fill.reads.is_empty() {
        return;
    }
    let RamFill {
        reads,
        file_lens,
        failed,
        ..
    } = &mut *fill;
    reads.retain_mut(|read| match check_ready(read) {
        None => true,
        Some(Ok(file)) => {
            file_lens.push(file);
            false
        }
        Some(Err((path, err))) => {
            error!("Reading {path} failed: {err}");
            failed.get_or_insert(path);
            false
        }
    });
    fill.start_reads(&server, &ram.0);
    if fill.is_full() {
        let n = fill.file_lens().len();
        let gb = fill.file_lens().iter().map(|(_, len)| len).sum::<u64>() as f64 / GIB;
        let s = fill.started.elapsed().as_secs_f64();
        info!("Read {n} UHD files ({gb:.1} GB) into RAM in {s:.1} s");
    }
}

/// Reads one file from `assets/` into the RAM folder; its bytes move in uncopied.
async fn read_into_ram(server: &AssetServer, ram: &Dir, path: &'static str) -> FileRead {
    let bytes = read_file(server, path)
        .await
        .map_err(|err| (path, err.to_string()))?;
    let len = bytes.len() as u64;
    ram.insert_asset(Path::new(path), bytes);
    Ok((path, len))
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
    use bevy::asset::io::AssetSourceId;
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

    fn filling_app(on_disk: &[(&str, &[u8])], listed: &[&'static str]) -> (App, RamDir) {
        let root = fresh_dir();
        for (path, bytes) in on_disk {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().expect("a file inside a folder"))
                .expect("create the file's folder");
            std::fs::write(file, bytes).expect("write the test file");
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        register_ram_source(&mut app);
        app.add_plugins((
            AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..default()
            },
            StatesPlugin,
        ))
        .init_state::<AppState>()
        .insert_resource(UhdFiles(listed.to_vec()))
        .add_plugins(RamSourcePlugin);
        let ram = app.world().resource::<RamDir>().clone();
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
    async fn read_ram(app: &App, path: &str) -> Vec<u8> {
        let server = app.world().resource::<AssetServer>();
        let source = server
            .get_source(RAM_SOURCE)
            .expect("the ram source is registered");
        let mut reader = source
            .reader()
            .read(Path::new(path))
            .await
            .expect("the file is in RAM");
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
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
                    .eq(&listed[listed.len() - fill.waiting.len()..]),
                "files start out of list order"
            );
            fill.is_full()
        });
        assert_eq!(app.world().resource::<RamFill>().file_lens().len(), 10);
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
