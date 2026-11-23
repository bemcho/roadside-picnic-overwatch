//! Loading the scans `GpuScans` admits: render-only from `ram://`, measured
//! before render extract takes their data, and confirmed resident by the
//! render world.

use std::collections::HashSet;

use bevy::asset::{LoadState, RecursiveDependencyLoadState, RenderAssetUsages};
use bevy::ecs::system::SystemParam;
use bevy::gltf::{Gltf, GltfLoaderSettings, GltfMaterial, GltfMesh};
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};

#[cfg(not(target_arch = "wasm32"))]
use crate::assets::UhdFiles;
#[cfg(not(target_arch = "wasm32"))]
use crate::assets::ram_source::RamFill;
use crate::assets::uhd;
use crate::launch::{DEFAULT_VRAM_GB, LaunchOptions};
use crate::states::AppState;

use super::residency::{ScanParts, ScanResidency, publish_scan_residency, scan_bytes};
use super::vram::track_vram_budget;
use super::{GpuScans, State, gb, scan_budget};

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
            .insert_resource(residency.clone())
            .add_systems(
                First,
                (track_vram_budget, note_resident_scans).run_if(in_state(AppState::InGame)),
            )
            .add_systems(Update, measure_scans)
            .add_systems(Last, admit_scans.run_if(in_state(AppState::InGame)));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(OnEnter(AppState::InGame), estimate_scans)
            .add_systems(
                Last,
                prefill_scans
                    .after(admit_scans)
                    .run_if(in_state(AppState::InGame)),
            );
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(residency).add_systems(
                Render,
                publish_scan_residency.in_set(RenderSystems::PrepareResources),
            );
        }
    }
}

/// The strong handles that keep a scan's GPU copy alive; dropping them frees it.
pub(super) struct ScanHandles {
    pub(super) gltf: Handle<Gltf>,
    /// The file's scene, known once the load finished.
    pub(super) scene: Option<Handle<WorldAsset>>,
}

impl GpuScans {
    /// Starts loading an admitted scan render-only, from `ram://` (or
    /// `assets/` where the fill left it): its meshes and images live on the
    /// GPU only. The file loads once; its scene comes from the loaded glTF.
    fn load(&mut self, path: &'static str, server: &AssetServer) {
        let gltf = server
            .load_builder()
            .with_settings(render_only)
            .load(uhd(path));
        let scan = self.scan_mut(path);
        scan.handles = Some(ScanHandles { gltf, scene: None });
        scan.resident = false;
    }

    /// Drops an evicted scan's handles; Bevy then frees its GPU copy.
    pub(super) fn unload(&mut self, path: &'static str) {
        let scan = self.scan_mut(path);
        scan.handles = None;
        scan.resident = false;
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

    /// The load finished: keeps the scene to show once the scan is resident,
    /// and its size (`None` keeps the estimate).
    pub(super) fn loaded(
        &mut self,
        path: &'static str,
        scene: Handle<WorldAsset>,
        bytes: Option<u64>,
    ) {
        if let Some(handles) = self.scan_mut(path).handles.as_mut() {
            handles.scene = Some(scene);
        }
        self.measured(path, bytes);
    }

    /// The load failed: the scan ends loaded at 0 bytes and confirmed, with
    /// nothing on the GPU, so it holds neither `settled` nor room and callers
    /// keep showing its mid tier. Once it has left, a retry is admitted at
    /// 0 bytes until measured.
    pub(super) fn fail(&mut self, path: &'static str) {
        self.measured(path, Some(0));
        let scan = self.scan_mut(path);
        scan.handles = None;
        scan.resident = true;
    }

    /// Marks a loaded scan resident; its bytes the first time, `None` after.
    pub(super) fn mark_resident(&mut self, path: &'static str) -> Option<u64> {
        let scan = self.scans.get_mut(path)?;
        let newly = scan.state == State::Loaded && scan.handles.is_some() && !scan.resident;
        scan.resident |= newly;
        newly.then_some(scan.bytes)
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

/// How far off a prefilled scan counts as wanted: past any real ask, so the
/// scans callers want come first.
#[cfg(not(target_arch = "wasm32"))]
const PREFILL_DISTANCE: f32 = f32::MAX;

/// Fills the budget's spare room less `PREFILL_HEADROOM_GB`, before anything
/// asks for the scans: wants the next file of `UhdFiles` that fits, which the
/// next `admit_scans` loads.
/// One at a time, as `next_prefill` waits for the last to be measured. The
/// web build has no file lengths to size a scan by, so it cannot tell what
/// fits and prefills nothing.
#[cfg(not(target_arch = "wasm32"))]
fn prefill_scans(mut scans: ResMut<GpuScans>, files: Res<UhdFiles>) {
    if let Some(path) = scans.next_prefill(&files.0) {
        scans.want(path, PREFILL_DISTANCE);
    }
}

/// The asset stores a finished scan is measured from.
#[derive(SystemParam)]
struct ScanAssets<'w> {
    server: Res<'w, AssetServer>,
    gltfs: Res<'w, Assets<Gltf>>,
    gltf_meshes: Res<'w, Assets<GltfMesh>>,
    materials: Res<'w, Assets<GltfMaterial>>,
    meshes: Res<'w, Assets<Mesh>>,
    images: Res<'w, Assets<Image>>,
}

impl ScanAssets<'_> {
    /// The meshes and images of a loaded scan: every primitive's mesh and
    /// every image its materials bind, each once.
    fn parts(&self, gltf: &Gltf) -> ScanParts {
        let meshes = gltf
            .meshes
            .iter()
            .filter_map(|mesh| self.gltf_meshes.get(mesh))
            .flat_map(|mesh| mesh.primitives.iter().map(|primitive| primitive.mesh.id()))
            .collect();
        let images: HashSet<_> = gltf
            .materials
            .iter()
            .filter_map(|material| self.materials.get(material))
            .flat_map(material_images)
            .collect();
        ScanParts {
            meshes,
            images: images.into_iter().collect(),
        }
    }
}

/// The images a glTF material binds: the five core PBR maps, all this build
/// reads (it enables no `pbr_*_textures` feature).
fn material_images(material: &GltfMaterial) -> impl Iterator<Item = AssetId<Image>> + '_ {
    [
        &material.base_color_texture,
        &material.emissive_texture,
        &material.metallic_roughness_texture,
        &material.normal_map_texture,
        &material.occlusion_texture,
    ]
    .into_iter()
    .flatten()
    .map(Handle::id)
}

/// Measures each scan whose load finished this frame, before render extract
/// takes its data, and hands its parts to the render world to confirm. A
/// failed load, or a file without a scene, ends confirmed at 0 bytes.
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
        let Some(scene) = gltf
            .default_scene
            .clone()
            .or_else(|| gltf.scenes.first().cloned())
        else {
            error!("UHD scan {path} has no scene to show");
            scans.fail(path);
            continue;
        };
        let parts = assets.parts(gltf);
        let bytes = scan_bytes(&parts, &assets.meshes, &assets.images);
        if bytes.is_none() {
            warn!(
                "UHD scan {path} left for the GPU before it was measured; it counts at its estimate"
            );
        }
        scans.loaded(path, scene, bytes);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::{PREFILL_HEADROOM_GB, gib};
    use super::*;

    const MB: u64 = 1 << 20;

    #[test]
    fn prefill_loads_one_scan_at_a_time_until_the_budget_is_used() {
        let files = ["a", "b", "c"];
        let mut scans = GpuScans::new(gib(PREFILL_HEADROOM_GB) + 400 * MB);
        for path in files {
            scans.estimate(path, 100 * MB); // 165 MB on the GPU each
        }
        let mut app = App::new();
        app.insert_resource(scans)
            .insert_resource(UhdFiles(files.to_vec()))
            .add_systems(Update, prefill_scans);
        let frame = |app: &mut App| {
            app.update();
            app.world_mut().resource_mut::<GpuScans>().plan(0.0).load
        };

        assert_eq!(frame(&mut app), ["a"]);
        assert!(frame(&mut app).is_empty(), "a is still loading");
        app.world_mut()
            .resource_mut::<GpuScans>()
            .measured("a", Some(165 * MB));
        assert_eq!(frame(&mut app), ["b"]);
        app.world_mut()
            .resource_mut::<GpuScans>()
            .measured("b", Some(165 * MB));
        assert!(frame(&mut app).is_empty(), "c does not fit the 70 MB left");
    }
}
