//! The old asphalt roads (`roads::old_roads`), paved with the road kit: a
//! stretch of cracked road scanned in slabs (`assets/models/road/uhd`) and put
//! back together in one shared frame by `tools/straighten_road_kit.py`, with
//! +Z along the road and y = 0 on the asphalt. Copies of the whole module lie
//! end to end along each graded leg; the full scan shows near the view and
//! its simplified copy (`medium/`) farther out.

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::{self, PreloadedAssets};

use super::roads::old_roads;
use super::terrain::Terrain;
use super::{WorldRoot, detail_range};

/// Meters per scan unit along the ground (a 17 m wide road), and a flatter
/// scale upward so the heaved slabs stay low enough to drive over.
const SCALE: f32 = 63.0;
const VERTICAL: f32 = 25.0;
/// The module's length along the road, in scan units.
const LENGTH: f32 = 0.9864;
/// The share of a module each copy advances, so neighbors overlap seamlessly.
const OVERLAP: f32 = 0.97;
/// A leg shorter than this share of a module still gets one, overhanging it.
const MIN_STRETCH: f32 = 0.75;
/// The asphalt above the graded road, so the terrain doesn't show through.
const LIFT: f32 = 0.08;
/// The full scan within this distance, the simplified one out to `FAR`.
const NEAR: f32 = 40.0;
const FAR: f32 = 900.0;
const FADE: f32 = 20.0;

macro_rules! pieces {
    ($($n:literal),*) => {
        [$((
            concat!("models/road/uhd/road_", $n, ".glb"),
            concat!("models/road/medium/road_", $n, ".glb"),
        )),*]
    };
}

/// The module's slabs, curbs and edge strips (full scan, simplified). The
/// kit's other pieces lie buried under them.
const PIECES: [(&str, &str); 18] = pieces!(
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "10", "11", "15", "18", "22", "26", "27",
    "29", "33"
);

/// Where every module lies: centered on the graded road, along each leg.
fn lay_roads(terrain: &Terrain) -> Vec<Transform> {
    let period = LENGTH * OVERLAP * SCALE;
    let mut laid = Vec::new();
    for road in old_roads() {
        for leg in road.windows(2) {
            let (a, b) = (leg[0], leg[1]);
            let length = a.distance(b);
            let heading = (b - a) / length;
            // Stretch the copies a little so a whole number fills the leg.
            let count = (length / period).round().max(1.0);
            let step = period * (length / (count * period)).max(MIN_STRETCH);
            let middle = (a + b) / 2.0;
            for i in 0..count as usize {
                let center = middle + heading * (i as f32 - (count - 1.0) / 2.0) * step;
                laid.push(lay(terrain, center, heading, step / OVERLAP));
            }
        }
    }
    laid
}

/// A module `span` meters long centered at `center`, pitched with the road.
fn lay(terrain: &Terrain, center: Vec2, heading: Vec2, span: f32) -> Transform {
    let half = heading * span / 2.0;
    let (from, to) = (center - half, center + half);
    let rise = terrain.height_at(to.x, to.y) - terrain.height_at(from.x, from.y);
    let yaw = heading.x.atan2(heading.y);
    Transform::from_xyz(
        center.x,
        terrain.height_at(center.x, center.y) + LIFT,
        center.y,
    )
    .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-(rise / span).atan()))
    .with_scale(Vec3::new(SCALE, VERTICAL, span / LENGTH))
}

pub(super) fn preload_old_roads(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for (near, far) in PIECES {
        let (mesh, paint) = assets::load_single_primitive(&server, near);
        let (distant, _) = assets::load_single_primitive(&server, far);
        preloaded
            .0
            .extend([mesh.untyped(), paint.untyped(), distant.untyped()]);
    }
}

pub(super) fn spawn_old_roads(
    mut commands: Commands,
    server: Res<AssetServer>,
    terrain: Res<Terrain>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Both tiers of a piece share the full scan's material.
    let tiers: Vec<_> = PIECES
        .iter()
        .map(|&(near, far)| {
            let (mesh, paint) = assets::load_single_primitive(&server, near);
            let (distant, _) = assets::load_single_primitive(&server, far);
            let paint = paints.get(&paint).expect("preloaded road material");
            let paint = MeshMaterial3d(materials.add(standard_material_from_gltf_material(paint)));
            ((Mesh3d(mesh), paint.clone()), (Mesh3d(distant), paint))
        })
        .collect();
    let near = detail_range(NEAR, FADE);
    let far = VisibilityRange {
        start_margin: near.end_margin.clone(),
        ..detail_range(FAR, FADE)
    };
    let root = commands
        .spawn((
            WorldRoot,
            Name::new("Old roads"),
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    for pose in lay_roads(&terrain) {
        for (full, simple) in &tiers {
            commands.spawn((full.clone(), pose, near.clone(), ChildOf(root)));
            commands.spawn((simple.clone(), pose, far.clone(), ChildOf(root)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modules_pave_every_old_road_leg_end_to_end() {
        let terrain = Terrain::new();
        let laid = lay_roads(&terrain);
        for road in old_roads() {
            for leg in road.windows(2) {
                // Every point of the centerline lies under some module.
                let steps = (leg[0].distance(leg[1]) / 4.0).ceil() as usize;
                for step in 0..=steps {
                    let at = leg[0].lerp(leg[1], step as f32 / steps as f32);
                    assert!(
                        laid.iter().any(|pose| {
                            let local = pose.translation.xz() - at;
                            local.length() <= pose.scale.z * LENGTH / 2.0 + 0.1
                        }),
                        "bare road at {at}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_module_lies_on_the_graded_road() {
        let terrain = Terrain::new();
        for pose in lay_roads(&terrain) {
            let at = pose.translation.xz();
            assert!(terrain.road_mask(at.x, at.y) > 0.99, "off the road at {at}");
            let lift = pose.translation.y - terrain.height_at(at.x, at.y);
            assert!((lift - LIFT).abs() < 1e-3);
            let stretch = pose.scale.z / SCALE;
            assert!((MIN_STRETCH..1.5).contains(&stretch), "stretched {stretch}");
        }
    }
}
