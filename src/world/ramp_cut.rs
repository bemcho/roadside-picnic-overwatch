//! The quarry scan's stone is cut away above the haul ramp, so the road the
//! terrain was graded for shows through it instead of lying buried in the scan.
//! The scan's file stays as it is: its material discards the fragments higher
//! than the ground inside the ramp's corridor (`shaders/ramp_cut.wesl`), in the
//! main pass and in the shadow maps alike.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;

use super::quarry_ramp::on_ramp_bed;
use super::roads::{RAMP_HALF_WIDTH, RAMP_SHOULDER};
use super::sites;
use super::terrain::Terrain;

pub(super) type RampCutMaterial = ExtendedMaterial<StandardMaterial, RampCut>;

/// Height meaning "not cut": above any fragment.
const UNCUT: f32 = 1.0e6;
/// Meters per texel of the cut image.
const TEXEL: f32 = 0.5;
/// How far under the drawn ground the stone is cut, so that stone lying on the
/// ground, which would show through it as dark slivers, goes too. The lowest
/// corner of a texel can lie up to about 0.07 m above the mesh inside it, where
/// a triangle's crease crosses the texel (a margin of -0.05 m left stone 0.12 m
/// above the ground in the test), so 0.1 m keeps the cut under the ground.
const BURY: f32 = 0.1;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct RampCut {
    /// Where the image lies in the world: its corner (x, z) and one over its
    /// extent (x, z), in meters.
    #[uniform(100)]
    bounds: Vec4,
    /// The height above which the stone is cut in the corridor, `UNCUT`
    /// elsewhere. Read texel by texel (`textureLoad`), so it needs no filtering.
    #[texture(101, sample_type = "float", filterable = false)]
    heights: Handle<Image>,
}

impl MaterialExtension for RampCut {
    fn fragment_shader() -> ShaderRef {
        assets::RAMP_CUT_SHADER.into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        assets::RAMP_CUT_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        assets::RAMP_CUT_SHADER.into()
    }

    // The shadow maps run a fragment shader only for materials that may
    // discard, and an opaque one does not say so.
    fn alpha_mode() -> Option<AlphaMode> {
        Some(AlphaMode::Mask(0.5))
    }
}

/// The cut's image and where it lies, for the quarry's materials.
#[derive(Resource)]
pub(super) struct RampCutImage(pub Handle<Image>, pub Vec4);

impl RampCutImage {
    pub(super) fn material(&self, base: StandardMaterial) -> RampCutMaterial {
        RampCutMaterial {
            base,
            extension: RampCut {
                bounds: self.1,
                heights: self.0.clone(),
            },
        }
    }
}

/// Marks the quarry's full scan, whose stone the ramp cuts.
#[derive(Component)]
pub(super) struct RampCutScan;

/// Builds the cut image once, at entering the Zone. It follows `Terrain`, which
/// exists only at run time, so no asset pipeline can bake it.
pub(super) fn prepare_ramp_cut(
    mut commands: Commands,
    terrain: Res<Terrain>,
    mut images: ResMut<Assets<Image>>,
) {
    let (image, bounds) = build_ramp_cut(&terrain);
    commands.insert_resource(RampCutImage(images.add(image), bounds));
}

/// The height above which the stone is cut wherever the ramp's shoulder
/// reaches, `UNCUT` elsewhere, over the quarry's footprint and a shoulder around
/// it, and where it lies. A texel is cut from the lowest of the ground drawn at
/// its corners (the render mesh, which is not `height_at` and falls steeply on
/// the banks) less `BURY`. That keeps the cut under the drawn ground at the
/// points `the_cut_lies_below_the_ground_that_is_drawn` samples; it is not a
/// proof for every point, as the mesh may dip below its corners inside a texel.
fn build_ramp_cut(terrain: &Terrain) -> (Image, Vec4) {
    let (low, high) = sites::quarry_footprint();
    let corner = low - Vec2::splat(RAMP_SHOULDER);
    let size = ((high - low + Vec2::splat(2.0 * RAMP_SHOULDER)) / TEXEL)
        .ceil()
        .as_uvec2();
    let extent = size.as_vec2() * TEXEL;
    // The drawn ground at the texels' corners, found once each.
    let mut drawn = HashMap::new();
    let mut ground = |lattice: UVec2| {
        *drawn
            .entry(lattice)
            .or_insert_with(|| terrain.mesh_height_at(corner + lattice.as_vec2() * TEXEL))
    };
    let texels = (0..size.y)
        .flat_map(|row| (0..size.x).map(move |column| UVec2::new(column, row)))
        .flat_map(|texel| {
            let at = corner + (texel.as_vec2() + 0.5) * TEXEL;
            let height = if on_ramp_bed(at, RAMP_SHOULDER - RAMP_HALF_WIDTH) {
                let lowest = [UVec2::ZERO, UVec2::X, UVec2::Y, UVec2::ONE]
                    .map(|offset| ground(texel + offset))
                    .into_iter()
                    .fold(f32::INFINITY, f32::min);
                lowest - BURY
            } else {
                UNCUT
            };
            height.to_le_bytes()
        })
        .collect();
    let image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texels,
        TextureFormat::R32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    (
        image,
        Vec4::new(corner.x, corner.y, 1.0 / extent.x, 1.0 / extent.y),
    )
}

/// The stores a stone's material is copied from and cut into.
#[derive(SystemParam)]
pub(super) struct CutMaterials<'w> {
    standard: Res<'w, Assets<StandardMaterial>>,
    cut: Res<'w, RampCutImage>,
    materials: ResMut<'w, Assets<RampCutMaterial>>,
}

impl CutMaterials<'_> {
    /// A new material drawing `stone`'s through the cut.
    fn of(&mut self, stone: &MeshMaterial3d<StandardMaterial>) -> Option<RampCutMaterial> {
        let base = self.standard.get(&stone.0)?;
        Some(self.cut.material(base.clone()))
    }
}

/// Draws the quarry's scan through the cut once it has spawned.
///
/// The materials are made at each stream-in and go with the instance: handles
/// cached elsewhere would keep the scan's textures in video memory after the
/// scan streams out.
pub(super) fn cut_scan_stone(
    ready: On<WorldInstanceReady>,
    scans: Query<(), With<RampCutScan>>,
    children: Query<&Children>,
    stones: Query<&MeshMaterial3d<StandardMaterial>>,
    mut cut: CutMaterials,
    mut commands: Commands,
) {
    if !scans.contains(ready.entity) {
        return;
    }
    for child in children.iter_descendants(ready.entity) {
        let Ok(stone) = stones.get(child) else {
            continue;
        };
        let Some(material) = cut.of(stone) else {
            continue;
        };
        commands
            .entity(child)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(cut.materials.add(material)));
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldSerializationPlugin};

    use super::*;

    /// The cut image's height under `at`, read as the shader reads it.
    fn cut_height(image: &Image, bounds: Vec4, at: Vec2) -> f32 {
        let uv = (at - bounds.xy()) * bounds.zw();
        assert!(
            uv.cmpge(Vec2::ZERO).all() && uv.cmplt(Vec2::ONE).all(),
            "{at} lies outside the cut image"
        );
        let size = image.size();
        let texel = (uv * size.as_vec2()).as_uvec2();
        let start = (texel.y * size.x + texel.x) as usize * 4;
        let data = image.data.as_ref().expect("the image keeps its texels");
        f32::from_le_bytes(data[start..start + 4].try_into().expect("four bytes"))
    }

    #[test]
    fn the_cut_image_is_uncut_off_the_ramp() {
        let terrain = Terrain::new();
        let (image, bounds) = build_ramp_cut(&terrain);

        // The first point of the footprint's box at least 15 m from the ramp
        // (the hairpin runs through the middle terrace, so the middle may not do).
        let (low, high) = sites::quarry_footprint();
        let far = (0..)
            .map(|step| low + Vec2::splat(step as f32))
            .take_while(|at| at.cmple(high).all())
            .find(|&at| !on_ramp_bed(at, 15.0 - RAMP_HALF_WIDTH))
            .expect("a point of the footprint is far from the ramp");
        assert_eq!(cut_height(&image, bounds, far), UNCUT);

        // The baked points, past the drive's two, lie in the cut, a little
        // under the ground (a texel of the 19% grade spans 0.1 m of height).
        for at in sites::quarry_ramp().into_iter().skip(2) {
            let ground = terrain.height_at(at.x, at.y);
            let cut = cut_height(&image, bounds, at);
            assert!(
                (-0.1..0.5).contains(&(ground - cut)),
                "the cut at {at} is {cut}, the terrain {ground}"
            );
        }
    }

    #[test]
    fn the_cut_lies_below_the_ground_that_is_drawn() {
        let terrain = Terrain::new();
        let (image, bounds) = build_ramp_cut(&terrain);
        let (low, high) = sites::quarry_footprint();
        // Stone left above the drawn ground, even by centimeters, shows through
        // it as dark slivers; the grid is off the texels' lattice on purpose.
        let mut worst = (f32::MIN, Vec2::ZERO);
        let mut cut = 0;
        for column in 0..((high.x - low.x) / 0.37) as u32 {
            for row in 0..((high.y - low.y) / 0.37) as u32 {
                let at = low + UVec2::new(column, row).as_vec2() * 0.37;
                let height = cut_height(&image, bounds, at);
                if height < UNCUT {
                    cut += 1;
                    let above = height - terrain.mesh_height_at(at);
                    if above > worst.0 {
                        worst = (above, at);
                    }
                }
            }
        }
        assert!(cut > 10_000, "only {cut} points lie in the cut");
        assert!(
            worst.0 <= 0.0,
            "the cut is {} m above the drawn ground at {}",
            worst.0,
            worst.1
        );
    }

    /// An app that spawns world instances, with two scans' roots, each holding
    /// a stone: the quarry's (marked) and another site's. Returns the stones.
    fn app_with_scans() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            WorldSerializationPlugin,
        ))
        .init_asset::<StandardMaterial>()
        .init_asset::<RampCutMaterial>()
        .insert_resource(RampCutImage(Handle::default(), Vec4::ZERO));
        let scene = app
            .world_mut()
            .resource_mut::<Assets<WorldAsset>>()
            .add(WorldAsset::new(World::new()));
        let stone = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let mut scan = |quarry: bool| {
            let mut root = app.world_mut().spawn(WorldAssetRoot(scene.clone()));
            root.observe(cut_scan_stone);
            if quarry {
                root.insert(RampCutScan);
            }
            let root = root.id();
            app.world_mut()
                .spawn((MeshMaterial3d(stone.clone()), ChildOf(root)))
                .id()
        };
        let quarry = scan(true);
        let other = scan(false);
        (app, quarry, other)
    }

    #[test]
    fn the_quarry_scan_draws_through_the_ramp_cut() {
        let (mut app, quarry, other) = app_with_scans();
        app.update();

        let world = app.world();
        assert!(
            world
                .get::<MeshMaterial3d<RampCutMaterial>>(quarry)
                .is_some()
        );
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(quarry)
                .is_none()
        );
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(other)
                .is_some()
        );
        assert!(
            world
                .get::<MeshMaterial3d<RampCutMaterial>>(other)
                .is_none()
        );
    }
}
