//! The Sphere's UHD scan on exceptional GPUs: streamed through `GpuScans`
//! near the Sphere, swapped in once resident, and the medium model back when
//! it leaves or is evicted.

use bevy::gltf::{GltfAssetLabel, GltfMaterial};
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::assets::gpu_scans::{GpuScans, GpuTier, TierLevel, UPLOAD_LEAD};
use crate::assets::{GOLDEN_SPHERE_UHD, uhd};
use crate::world::StreamAnchor;

use super::mirror::MirrorMaterial;
use super::{GoldenSphere, SOURCE_DIAMETER, gild};

/// How near a stream anchor brings the UHD Sphere in, in metres.
const UHD_RANGE: f32 = 40.0;
/// The UHD scan's diameter; it is centred, unlike the medium one.
const UHD_DIAMETER: f32 = 0.98;

/// Which of its models the Sphere shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Detail {
    Medium,
    Uhd,
}

/// The model to show `distance` metres from the nearest anchor, on a card at
/// `level`, with the UHD scan `resident` on the GPU or not.
pub(super) fn sphere_detail(level: TierLevel, distance: f32, resident: bool) -> Detail {
    if level == TierLevel::Exceptional && distance <= UHD_RANGE && resident {
        Detail::Uhd
    } else {
        Detail::Medium
    }
}

/// The Sphere's skin: its medium model, kept to swap back to.
#[derive(Component)]
pub(super) struct SphereSkin {
    medium: Handle<Mesh>,
    medium_paint: (Color, Option<Handle<Image>>),
    medium_at: Transform,
    shown: Detail,
}

impl SphereSkin {
    pub fn medium(mesh: Handle<Mesh>, gold: &StandardMaterial, at: Transform) -> Self {
        Self {
            medium: mesh,
            medium_paint: (gold.base_color, gold.base_color_texture.clone()),
            medium_at: at,
            shown: Detail::Medium,
        }
    }
}

pub(super) fn on_exceptional_gpu(tier: Option<Res<GpuTier>>) -> bool {
    tier.is_some_and(|tier| tier.level() == TierLevel::Exceptional)
}

/// Asks for the UHD scan near the Sphere and swaps it in once resident; the
/// medium model comes back when the anchors leave or the scan is evicted,
/// dropping the hold on the scan's GPU copy.
pub(super) fn stream_uhd_sphere(
    mut scans: ResMut<GpuScans>,
    server: Res<AssetServer>,
    paints: Res<Assets<GltfMaterial>>,
    mut mirrors: ResMut<Assets<MirrorMaterial>>,
    anchors: Query<&Transform, (With<StreamAnchor>, Without<SphereSkin>)>,
    spheres: Query<&Transform, (With<GoldenSphere>, Without<SphereSkin>)>,
    mut skins: Query<(
        &ChildOf,
        &mut SphereSkin,
        &mut Mesh3d,
        &mut Transform,
        &MeshMaterial3d<MirrorMaterial>,
    )>,
) {
    for (parent, mut skin, mut mesh, mut at, mirror) in &mut skins {
        let Ok(sphere) = spheres.get(parent.parent()) else {
            continue;
        };
        let distance = anchors
            .iter()
            .map(|anchor| anchor.translation.distance(sphere.translation))
            .fold(f32::INFINITY, f32::min);
        let ahead = distance - UHD_RANGE;
        if skin.shown == Detail::Uhd {
            scans.want(GOLDEN_SPHERE_UHD, ahead.min(0.0));
        } else if ahead < UPLOAD_LEAD {
            scans.want(GOLDEN_SPHERE_UHD, ahead);
        }
        let parts = scans
            .scene(GOLDEN_SPHERE_UHD)
            .and_then(|_| uhd_parts(&server, &paints));
        let detail = sphere_detail(TierLevel::Exceptional, distance, parts.is_some());
        if detail == skin.shown {
            continue;
        }
        let Some(mut material) = mirrors.get_mut(&mirror.0) else {
            continue;
        };
        let base = &mut material.base;
        match parts {
            Some((uhd_mesh, uhd_gold)) if detail == Detail::Uhd => {
                mesh.0 = uhd_mesh;
                *at = Transform::from_scale(Vec3::splat(SOURCE_DIAMETER / UHD_DIAMETER));
                base.base_color = uhd_gold.base_color;
                base.base_color_texture = uhd_gold.base_color_texture;
            }
            _ => {
                mesh.0 = skin.medium.clone();
                *at = skin.medium_at;
                (base.base_color, base.base_color_texture) = skin.medium_paint.clone();
            }
        }
        skin.shown = detail;
    }
}

/// The resident UHD scan's mesh and gold, read from the loaded glTF without
/// loading anything (`GpuScans` alone loads scans).
fn uhd_parts(
    server: &AssetServer,
    paints: &Assets<GltfMaterial>,
) -> Option<(Handle<Mesh>, StandardMaterial)> {
    let primitive = GltfAssetLabel::Primitive {
        mesh: 0,
        primitive: 0,
    };
    let material = GltfAssetLabel::Material {
        index: 0,
        is_scale_inverted: false,
    };
    let mesh = server.get_handle(primitive.from_asset(uhd(GOLDEN_SPHERE_UHD)))?;
    let paint = server.get_handle(material.from_asset(uhd(GOLDEN_SPHERE_UHD)))?;
    let gold = gild(standard_material_from_gltf_material(paints.get(&paint)?));
    Some((mesh, gold))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uhd_sphere_shows_only_on_exceptional_cards_near_and_resident() {
        assert_eq!(
            sphere_detail(TierLevel::Exceptional, 10.0, true),
            Detail::Uhd
        );
        assert_eq!(
            sphere_detail(TierLevel::Exceptional, 50.0, true),
            Detail::Medium,
            "too far"
        );
        assert_eq!(
            sphere_detail(TierLevel::Strong, 10.0, true),
            Detail::Medium,
            "strong card"
        );
        assert_eq!(
            sphere_detail(TierLevel::Modest, 10.0, true),
            Detail::Medium,
            "modest card"
        );
    }

    #[test]
    fn an_evicted_uhd_sphere_falls_back_to_medium() {
        assert_eq!(
            sphere_detail(TierLevel::Exceptional, 10.0, false),
            Detail::Medium
        );
    }
}
