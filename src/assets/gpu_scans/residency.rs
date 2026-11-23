//! Which loaded UHD scans the render world has prepared: the back-channel from
//! the GPU to `GpuScans`, and the size of a scan before it gets there.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bevy::prelude::*;
use bevy::render::mesh::RenderMesh;
use bevy::render::render_asset::{RenderAsset, RenderAssets};
use bevy::render::texture::GpuImage;

/// The meshes and images one scan uploads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanParts {
    pub meshes: Vec<AssetId<Mesh>>,
    pub images: Vec<AssetId<Image>>,
}

/// Scans whose parts are on their way to the GPU, and those that arrived.
#[derive(Default)]
pub struct Residency {
    pub waiting: HashMap<&'static str, ScanParts>,
    pub ready: HashSet<&'static str>,
}

impl Residency {
    /// Forgets an evicted scan, wherever it was.
    pub fn forget(&mut self, path: &'static str) {
        self.waiting.remove(path);
        self.ready.remove(path);
    }
}

/// `Residency`, shared by the main and render apps.
#[derive(Resource, Clone, Default)]
pub struct ScanResidency(pub Arc<Mutex<Residency>>);

impl ScanResidency {
    /// The shared state; a panic elsewhere while it was held leaves it usable.
    pub fn lock(&self) -> MutexGuard<'_, Residency> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The GPU bytes of a scan's meshes (vertices and indices) and images
/// (texels, mips included), or `None` once any of them has moved to the GPU.
pub fn scan_bytes(parts: &ScanParts, meshes: &Assets<Mesh>, images: &Assets<Image>) -> Option<u64> {
    let mut bytes = 0;
    for &id in &parts.meshes {
        let mesh = meshes.get(id)?;
        // `byte_len` panics on a mesh whose data went to the render world.
        if mesh.try_attributes().is_err() {
            return None;
        }
        bytes += RenderMesh::byte_len(mesh)? as u64;
    }
    for &id in &parts.images {
        bytes += GpuImage::byte_len(images.get(id)?)? as u64;
    }
    Some(bytes)
}

/// Moves each waiting scan whose meshes and images are all ready into `ready`.
pub fn settle(
    residency: &mut Residency,
    mesh_ready: impl Fn(AssetId<Mesh>) -> bool,
    image_ready: impl Fn(AssetId<Image>) -> bool,
) {
    let Residency { waiting, ready } = residency;
    waiting.retain(|&path, parts| {
        let on_gpu = parts.meshes.iter().all(|&id| mesh_ready(id))
            && parts.images.iter().all(|&id| image_ready(id));
        if on_gpu {
            ready.insert(path);
        }
        !on_gpu
    });
}

/// Render world: marks the scans whose every part the render world has prepared.
pub fn publish_scan_residency(
    residency: Res<ScanResidency>,
    meshes: Res<RenderAssets<RenderMesh>>,
    images: Res<RenderAssets<GpuImage>>,
) {
    let mut residency = residency.lock();
    if residency.waiting.is_empty() {
        return;
    }
    settle(
        &mut residency,
        |id| meshes.get(id).is_some(),
        |id| images.get(id).is_some(),
    );
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    use super::*;

    fn parts(meshes: &[AssetId<Mesh>], images: &[AssetId<Image>]) -> ScanParts {
        ScanParts {
            meshes: meshes.to_vec(),
            images: images.to_vec(),
        }
    }

    #[test]
    fn settle_moves_scans_whose_parts_are_all_on_the_gpu() {
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let m1 = meshes.add(Mesh::from(Cuboid::default())).id();
        let m2 = meshes.add(Mesh::from(Cuboid::default())).id();
        let i1 = images.add(Image::default()).id();
        let i2 = images.add(Image::default()).id();
        let mut residency = Residency::default();
        residency.waiting.insert("a", parts(&[m1], &[i1, i2]));
        residency.waiting.insert("b", parts(&[m2], &[]));

        settle(&mut residency, |m| [m1, m2].contains(&m), |i| i == i1);

        assert_eq!(residency.ready, HashSet::from(["b"]));
        assert!(residency.waiting.contains_key("a"));
        assert!(!residency.waiting.contains_key("b"));
    }

    #[test]
    fn scan_bytes_counts_vertices_indices_and_texels() {
        let mesh = Mesh::from(Cuboid::default());
        let image = Image::new_fill(
            Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0; 4],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        let expected = RenderMesh::byte_len(&mesh).unwrap_or(0) + 64;
        assert!(expected > 64, "the cube has vertices");
        let mut meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let parts = parts(&[meshes.add(mesh).id()], &[images.add(image).id()]);

        assert_eq!(scan_bytes(&parts, &meshes, &images), Some(expected as u64));
    }

    #[test]
    fn scan_bytes_is_unknown_once_a_mesh_has_moved_to_the_gpu() {
        let mut mesh = Mesh::from(Cuboid::default());
        let _ = mesh.take_gpu_data();
        let mut meshes = Assets::<Mesh>::default();
        let images = Assets::<Image>::default();
        let parts = parts(&[meshes.add(mesh).id()], &[]);

        assert_eq!(scan_bytes(&parts, &meshes, &images), None);
    }

    #[test]
    fn scan_bytes_is_unknown_once_an_image_has_moved_to_the_gpu() {
        let meshes = Assets::<Mesh>::default();
        let mut images = Assets::<Image>::default();
        let uploaded = Image {
            data: None,
            ..default()
        };
        let parts = parts(&[], &[images.add(uploaded).id()]);

        assert_eq!(scan_bytes(&parts, &meshes, &images), None);
    }
}
