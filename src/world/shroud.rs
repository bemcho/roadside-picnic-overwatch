//! The shroud over what the stalker has not surveyed yet (fog of war).
//!
//! Black, slowly drifting smoke covers the ground there and the fog over it:
//! the ground fog pass (`ground_fog/render.rs`, after the sky) draws it from a
//! mask texture of the survey grid ([`Explored`]), its linear filtering
//! softening the edges over a cell, so nothing of that ground shows from any
//! height. Everything else standing there is dropped from the views' and the
//! sun's visible lists after Bevy's own culling, so it costs no draw calls or
//! shadow passes. What frustum culling may not cull (`NoFrustumCulling`, such
//! as the locator ring), the terrain and the water always stay.
//!
//! V lifts and restores the shroud, for testing.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{
    CascadesVisibleEntities, NoFrustumCulling, VisibilitySystems, VisibleEntities,
};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::SimulationLightSystems;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::extract_resource::ExtractResource;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::states::{AppState, InGamePhase};

use super::places::{Explored, SURVEY_CELL, SURVEY_CELLS};
use super::terrain::HALF_SIZE;
use super::terrain_material::TerrainMaterial;
use super::water::WaterMaterial;

/// Whether the shroud covers unsurveyed ground.
#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Shroud {
    #[default]
    Covered,
    Lifted,
}

/// The survey grid as a texture: 255 where surveyed, 0 elsewhere. The fog
/// pass (`ground_fog/render.rs`) draws the shroud from it.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(RenderApp)]
pub(super) struct ShroudMask(pub Handle<Image>);

pub(super) struct ShroudPlugin;

impl Plugin for ShroudPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Shroud>()
            .add_systems(
                Update,
                (
                    toggle_shroud.run_if(in_state(InGamePhase::Running)),
                    paint_shroud_mask.run_if(in_state(AppState::InGame)),
                ),
            )
            .add_systems(
                PostUpdate,
                cull_shrouded
                    .after(VisibilitySystems::CheckVisibility)
                    .after(SimulationLightSystems::CheckLightVisibility)
                    .run_if(in_state(AppState::InGame))
                    .run_if(in_state(Shroud::Covered)),
            );
    }
}

/// Where the mask lies in the world, for the shaders: its corner and one over
/// its extent, in x and z.
pub(super) fn mask_bounds() -> Vec4 {
    let extent = SURVEY_CELLS as f32 * SURVEY_CELL;
    Vec4::new(-HALF_SIZE, -HALF_SIZE, 1.0 / extent, 1.0 / extent)
}

/// A mask with nothing surveyed yet.
pub(super) fn new_mask(images: &mut Assets<Image>) -> Handle<Image> {
    let cells = SURVEY_CELLS as u32;
    let mut mask = Image::new_fill(
        Extent3d {
            width: cells,
            height: cells,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0],
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    );
    mask.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    images.add(mask)
}

fn toggle_shroud(
    keys: Res<ButtonInput<KeyCode>>,
    shroud: Res<State<Shroud>>,
    mut next: ResMut<NextState<Shroud>>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        next.set(match shroud.get() {
            Shroud::Covered => Shroud::Lifted,
            Shroud::Lifted => Shroud::Covered,
        });
    }
}

/// Copies the survey into the mask when it grows or the shroud is toggled;
/// a lifted shroud shows everything.
fn paint_shroud_mask(
    explored: Option<Res<Explored>>,
    shroud: Res<State<Shroud>>,
    mask: Option<Res<ShroudMask>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(explored), Some(mask)) = (explored, mask) else {
        return;
    };
    if !explored.is_changed() && !shroud.is_changed() {
        return;
    }
    let Some(mut image) = images.get_mut(&mask.0) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let lifted = *shroud.get() == Shroud::Lifted;
    for (texel, seen) in data.iter_mut().zip(explored.cells()) {
        *texel = if lifted || seen { 255 } else { 0 };
    }
}

/// Whether an entity bounded by a sphere at `center` of `radius` lies wholly
/// under the shroud: its middle and, for a large one, the corners of its
/// footprint.
fn shrouded(explored: &Explored, center: Vec2, radius: f32) -> bool {
    let reach = radius.min(2.0 * SURVEY_CELL) * 0.7;
    [
        Vec2::ZERO,
        Vec2::ONE,
        Vec2::NEG_ONE,
        Vec2::new(1.0, -1.0),
        Vec2::new(-1.0, 1.0),
    ]
    .iter()
    .all(|&corner| explored.hides(center + corner * reach))
}

/// What the shroud may hide: not what frustum culling skips, nor the ground
/// and the water, whose depth tells the fog pass where to draw the smoke.
type Shroudable = (
    Without<NoFrustumCulling>,
    Without<MeshMaterial3d<TerrainMaterial>>,
    Without<MeshMaterial3d<WaterMaterial>>,
);

/// Drops shrouded entities from what the cameras draw and the lights shadow.
fn cull_shrouded(
    explored: Option<Res<Explored>>,
    bounds: Query<(&GlobalTransform, &Aabb), Shroudable>,
    mut views: Query<&mut VisibleEntities>,
    mut cascades: Query<&mut CascadesVisibleEntities>,
) {
    let Some(explored) = explored else {
        return;
    };
    let keep = |entity: &Entity| {
        let Ok((placed, aabb)) = bounds.get(*entity) else {
            return true;
        };
        let center = placed.transform_point(aabb.center.into());
        !shrouded(
            &explored,
            center.xz(),
            placed.radius_vec3a(aabb.half_extents),
        )
    };
    for mut view in &mut views {
        for entities in view.entities.values_mut() {
            entities.retain(keep);
        }
    }
    for mut light in &mut cascades {
        for cascade in light.entities.values_mut().flatten() {
            cascade.entities.retain(keep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shroud_hides_only_what_stands_wholly_on_unsurveyed_ground() {
        let mut explored = Explored::default();
        let seen = Vec2::new(840.0, -113.0);
        explored.survey(seen);
        assert!(!shrouded(&explored, seen, 2.0));
        assert!(shrouded(&explored, Vec2::new(-1200.0, 400.0), 2.0));
        // A big model reaching onto surveyed ground stays.
        let edge = seen + Vec2::X * 170.0;
        assert!(shrouded(&explored, edge, 2.0));
        assert!(!shrouded(&explored, edge, 80.0));
        // Beyond the map's edge nothing is shrouded.
        assert!(!shrouded(&explored, Vec2::splat(5000.0), 2.0));
    }

    #[test]
    fn the_mask_spans_the_survey_grid() {
        let bounds = mask_bounds();
        let uv = |at: Vec2| (at - bounds.xy()) * bounds.zw();
        assert_eq!(uv(Vec2::splat(-HALF_SIZE)), Vec2::ZERO);
        // A cell's middle samples its own texel's middle.
        let cell = Vec2::new(3.0, 5.0);
        let middle = (cell + 0.5) * SURVEY_CELL - HALF_SIZE;
        let texel = uv(middle) * SURVEY_CELLS as f32;
        assert!((texel - (cell + 0.5)).length() < 1e-3);
    }
}
