//! The `ram` source's reader: a file the fill read comes from RAM, any other
//! path from `assets/`, so a `ram://` path loads whether or not its file was
//! filled.

use std::path::Path;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, ErasedAssetReader, PathStream,
    Reader,
};
use bevy::prelude::*;

use super::RAM_SOURCE;

/// The in-memory folder behind the source, shared with its reader.
#[derive(Resource, Clone)]
pub struct RamDir(pub Dir);

/// Registers the `ram://` source over `root`, the folder Bevy's default source
/// reads (`assets`). Asset sources are built with `AssetPlugin`, so this runs
/// before `DefaultPlugins`.
pub fn register_ram_source(app: &mut App, root: &str) {
    let dir = Dir::default();
    let ram = dir.clone();
    let root = root.to_owned();
    app.register_asset_source(
        RAM_SOURCE,
        AssetSourceBuilder::new(move || Box::new(RamOrDisk::new(ram.clone(), &root))),
    )
    .insert_resource(RamDir(dir));
}

/// The `ram` source's reader: RAM for a file the fill read, `root` on disk
/// for any other path.
struct RamOrDisk {
    ram: MemoryAssetReader,
    disk: Box<dyn ErasedAssetReader>,
}

impl RamOrDisk {
    fn new(ram: Dir, root: &str) -> Self {
        Self {
            ram: MemoryAssetReader { root: ram },
            // The reader Bevy builds for its default source over `root`.
            disk: AssetSource::get_default_reader(root.to_owned())(),
        }
    }

    /// RAM for a file in it, the disk for any other path. A file in RAM takes
    /// its (absent) meta from RAM too, so loading it during play never probes
    /// `assets/` for a `.meta` file. Folders come from the disk, which holds
    /// every file RAM does.
    fn reader_for(&self, path: &Path) -> &dyn ErasedAssetReader {
        if self.ram.root.get_asset(path).is_some() {
            &self.ram
        } else {
            &*self.disk
        }
    }
}

impl AssetReader for RamOrDisk {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.reader_for(path).read(path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.reader_for(path).read_meta(path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.reader_for(path).read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.reader_for(path).is_directory(path).await
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::tasks::block_on;

    use super::*;
    use crate::assets::ram_source::tests::{asset_app, read_ram};

    /// The path in the `NotFound` error of a meta read through the `ram` source.
    fn meta_not_found(app: &App, path: &str) -> PathBuf {
        let server = app.world().resource::<AssetServer>();
        let source = server
            .get_source(RAM_SOURCE)
            .expect("the ram source is registered");
        match block_on(source.reader().read_meta(Path::new(path))) {
            Err(AssetReaderError::NotFound(path)) => path,
            Err(err) => panic!("expected NotFound, got {err}"),
            Ok(_) => panic!("expected no meta for {path}"),
        }
    }

    #[test]
    fn a_ram_path_missing_from_memory_loads_from_disk() {
        let bytes: Vec<u8> = (0..=255).collect();
        let (app, ram) = asset_app(&[("models/d/uhd/disk.glb", &bytes)]);
        assert!(
            ram.0
                .get_asset(Path::new("models/d/uhd/disk.glb"))
                .is_none()
        );
        assert_eq!(block_on(read_ram(&app, "models/d/uhd/disk.glb")), bytes);
    }

    #[test]
    fn a_file_in_ram_takes_its_meta_from_ram_and_others_from_disk() {
        // Both files are on disk; only the first is in RAM.
        let (app, ram) = asset_app(&[
            ("models/r/uhd/ram.glb", b"ram"),
            ("models/d/uhd/disk.glb", b"disk"),
        ]);
        ram.0
            .insert_asset(Path::new("models/r/uhd/ram.glb"), b"ram".to_vec());
        // RAM's NotFound carries the bare asset path; the disk's, `root/….meta`.
        assert_eq!(
            meta_not_found(&app, "models/r/uhd/ram.glb"),
            Path::new("models/r/uhd/ram.glb")
        );
        let on_disk = meta_not_found(&app, "models/d/uhd/disk.glb");
        assert!(on_disk.is_absolute(), "{}", on_disk.display());
        assert!(on_disk.ends_with("models/d/uhd/disk.glb.meta"));
    }
}
