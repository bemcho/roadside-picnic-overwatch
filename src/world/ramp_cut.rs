//! The cut scans' stone (the Quarry rim's pit and the Golden Sphere's crater,
//! `CutScan`) is cut away above the haul ramps, so the road the terrain was
//! graded for shows through it instead of lying buried in the scan. The scans'
//! files stay as they are: their materials discard the fragments higher than
//! the ground inside the ramps' corridor (`shaders/ramp_cut.wesl`), in the main
//! pass and in the shadow maps alike. Each scan has its own image of its corridor.

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets;

use super::quarry_ramp::CutScan;
use super::roads::RAMP_SHOULDER;
use super::sites;
use super::terrain::{SPACING, Terrain};

pub(super) type RampCutMaterial = ExtendedMaterial<StandardMaterial, RampCut>;

/// Height meaning "not cut": above any fragment.
const UNCUT: f32 = 1.0e6;
/// Meters between the cut image's lattice points. They lie on the world's
/// 0.5 m lattice, so each 2 m cell of the near ground mesh holds whole lattice
/// cells, and its diagonal (x + z a multiple of 2 m) runs across them corner to
/// corner: split along that same diagonal, as the shader splits every lattice
/// cell, the heights at the corners interpolate to the mesh itself.
const TEXEL: f32 = 0.5;
const _: () = assert!(
    SPACING % TEXEL == 0.0,
    "the mesh's grid lines lie on the lattice"
);
/// How far under the drawn ground the stone is cut, so that stone lying on the
/// ground, which would show through it as dark slivers, goes too.
const BURY: f32 = 0.1;
/// How far past the corridor's edge a lattice point keeps its cut height: a
/// lattice cell (0.71 m across) any part of which lies inside the corridor has
/// every corner within it, so the shader never interpolates from `UNCUT` there.
const EDGE_REACH: f32 = 2.0 * TEXEL;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct RampCut {
    /// Where the image lies in the world: its corner (x, z), in meters, and
    /// its lattice points per meter (x, z).
    #[uniform(100)]
    bounds: Vec4,
    /// At each lattice point: the height above which the stone is cut (red)
    /// and how far the point lies past the corridor's edge, negative inside
    /// (green). Read point by point (`textureLoad`) and interpolated in the
    /// shader, so it needs no filtering.
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

/// Each cut scan's cut (its image and where it lies), in `CutScan` order.
#[derive(Resource)]
pub(super) struct RampCuts([RampCut; 2]);

impl RampCuts {
    /// `base` drawn through `scan`'s cut.
    pub(super) fn material(&self, scan: CutScan, base: StandardMaterial) -> RampCutMaterial {
        RampCutMaterial {
            base,
            extension: self.0[scan as usize].clone(),
        }
    }
}

/// Marks a cut scan's full scan, whose stone its ramp cuts.
#[derive(Component)]
pub(super) struct RampCutScan(pub CutScan);

/// Builds the cut images once, at entering the Zone. They follow `Terrain`,
/// which derives from the runtime terrain (roads, relief, overrides), so baking
/// them would need a re-run on every terrain change.
pub(super) fn prepare_ramp_cut(
    mut commands: Commands,
    terrain: Res<Terrain>,
    mut images: ResMut<Assets<Image>>,
) {
    let cuts = CutScan::ALL.map(|scan| {
        let (image, bounds) = build_ramp_cut(&terrain, scan);
        RampCut {
            bounds,
            heights: images.add(image),
        }
    });
    commands.insert_resource(RampCuts(cuts));
}

/// The cut over `scan`'s footprint and a shoulder around it, and where it
/// lies: at each lattice point the ground drawn there (the 2 m render mesh,
/// which is not `height_at` and falls steeply on the banks, by tens of metres
/// in the crater) less `BURY`, and how far the point lies past the corridor.
/// Interpolated as the mesh is (see `TEXEL`), the cut lies exactly `BURY` under
/// the drawn ground, and its edge on the stone is the corridor's own, smooth
/// rather than stepped texel by texel.
fn build_ramp_cut(terrain: &Terrain, scan: CutScan) -> (Image, Vec4) {
    let (low, high) = sites::cut_footprint(scan);
    let corner = ((low - Vec2::splat(RAMP_SHOULDER)) / TEXEL).floor() * TEXEL;
    let points = ((high + Vec2::splat(RAMP_SHOULDER) - corner) / TEXEL)
        .ceil()
        .as_uvec2()
        + UVec2::ONE;
    let texels = (0..points.y)
        .flat_map(|row| (0..points.x).map(move |column| UVec2::new(column, row)))
        .flat_map(|point| {
            let at = corner + point.as_vec2() * TEXEL;
            let past = scan.past_cut_corridor(at);
            let height = if past < EDGE_REACH {
                terrain.mesh_height_at(at) - BURY
            } else {
                UNCUT
            };
            [height.to_le_bytes(), past.to_le_bytes()]
        })
        .flatten()
        .collect();
    let image = Image::new(
        Extent3d {
            width: points.x,
            height: points.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texels,
        TextureFormat::Rg32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    (
        image,
        Vec4::new(corner.x, corner.y, 1.0 / TEXEL, 1.0 / TEXEL),
    )
}

/// The stores a stone's material is copied from and cut into.
#[derive(SystemParam)]
pub(super) struct CutMaterials<'w> {
    standard: Res<'w, Assets<StandardMaterial>>,
    cuts: Res<'w, RampCuts>,
    materials: ResMut<'w, Assets<RampCutMaterial>>,
}

impl CutMaterials<'_> {
    /// A new material drawing `stone`'s through `scan`'s cut.
    fn of(
        &mut self,
        scan: CutScan,
        stone: &MeshMaterial3d<StandardMaterial>,
    ) -> Option<RampCutMaterial> {
        let base = self.standard.get(&stone.0)?;
        Some(self.cuts.material(scan, base.clone()))
    }
}

/// Draws a cut scan through its cut once it has spawned.
///
/// The materials are made at each stream-in and go with the instance: handles
/// cached elsewhere would keep the scan's textures in video memory after the
/// scan streams out.
pub(super) fn cut_scan_stone(
    ready: On<WorldInstanceReady>,
    scans: Query<&RampCutScan>,
    children: Query<&Children>,
    stones: Query<&MeshMaterial3d<StandardMaterial>>,
    mut cut: CutMaterials,
    mut commands: Commands,
) {
    let Ok(&RampCutScan(scan)) = scans.get(ready.entity) else {
        return;
    };
    for child in children.iter_descendants(ready.entity) {
        let Ok(stone) = stones.get(child) else {
            continue;
        };
        let Some(material) = cut.of(scan, stone) else {
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

    use super::super::roads::RAMP_HALF_WIDTH;
    use super::*;

    /// The cut's height over `at`, read as the shader reads it: interpolated
    /// over the lattice cell's half that holds `at`, and nothing is cut past
    /// the corridor's edge or outside the image.
    fn cut_height(image: &Image, bounds: Vec4, at: Vec2) -> f32 {
        let grid = (at - bounds.xy()) * bounds.zw();
        let last = (image.size() - UVec2::ONE).as_vec2();
        if grid.cmplt(Vec2::ZERO).any() || grid.cmpge(last).any() {
            return UNCUT;
        }
        let data = image.data.as_ref().expect("the image keeps its texels");
        let load = |point: UVec2| {
            let start = (point.y * image.width() + point.x) as usize * 8;
            let value =
                |at: usize| f32::from_le_bytes(data[at..at + 4].try_into().expect("four bytes"));
            Vec2::new(value(start), value(start + 4))
        };
        let cell = grid.floor().as_uvec2();
        let t = grid - grid.floor();
        let (b, c) = (load(cell + UVec2::X), load(cell + UVec2::Y));
        let cut = if t.x + t.y <= 1.0 {
            let a = load(cell);
            a + (b - a) * t.x + (c - a) * t.y
        } else {
            let d = load(cell + UVec2::ONE);
            d + (c - d) * (1.0 - t.x) + (b - d) * (1.0 - t.y)
        };
        if cut.y < 0.0 { cut.x } else { UNCUT }
    }

    fn inside(at: Vec2, (low, high): (Vec2, Vec2)) -> bool {
        at.cmpge(low).all() && at.cmple(high).all()
    }

    /// Every metre of `ramp` inside the footprint of `scan`.
    fn ramp_in_scan(scan: CutScan, ramp: &[Vec2]) -> Vec<Vec2> {
        let footprint = sites::cut_footprint(scan);
        ramp.windows(2)
            .flat_map(|leg| {
                let steps = leg[0].distance(leg[1]).ceil() as usize;
                (0..steps).map(move |i| leg[0].lerp(leg[1], i as f32 / steps as f32))
            })
            .filter(|&at| inside(at, footprint))
            .collect()
    }

    /// The road through `scan` lies in its cut, a little under the ground (a
    /// texel of the 19% grade spans 0.1 m of height), and the stone 15 m from
    /// it is not cut.
    fn assert_the_cut_follows_the_road(scan: CutScan, road: &[Vec2]) {
        let terrain = Terrain::new();
        let (image, bounds) = build_ramp_cut(&terrain, scan);

        // The first point of the footprint's box at least 15 m from the ramp
        // (the hairpin runs through the quarry's middle terrace, so the middle
        // may not do).
        let (low, high) = sites::cut_footprint(scan);
        let far = (0..)
            .map(|step| low + Vec2::splat(step as f32))
            .take_while(|at| at.cmple(high).all())
            .find(|&at| !scan.on_ramp_bed(at, 15.0 - RAMP_HALF_WIDTH))
            .expect("a point of the footprint is far from the ramp");
        assert_eq!(cut_height(&image, bounds, far), UNCUT);

        assert!(
            road.len() > 25,
            "only {} m of the road lie in the scan",
            road.len()
        );
        for &at in road {
            let ground = terrain.height_at(at.x, at.y);
            let cut = cut_height(&image, bounds, at);
            assert!(
                (-0.1..0.5).contains(&(ground - cut)),
                "the cut at {at} is {cut}, the terrain {ground}"
            );
        }
    }

    #[test]
    fn the_quarry_cut_follows_its_ramp_and_the_crater_roads_first_leg() {
        let ramp = sites::quarry_ramp();
        let crater = sites::crater_ramp();
        let connector = ramp_in_scan(CutScan::Quarry, &crater[..2]);
        // The crater road's first 23 m run in the box around the quarry.
        assert!(
            connector.len() >= 20,
            "{} m of the leg lie in the quarry",
            connector.len()
        );
        let road = [ramp_in_scan(CutScan::Quarry, &ramp), connector].concat();
        assert_the_cut_follows_the_road(CutScan::Quarry, &road);
    }

    #[test]
    fn the_core_cut_follows_the_crater_ramp() {
        let road = ramp_in_scan(CutScan::Core, &sites::crater_ramp());
        assert_the_cut_follows_the_road(CutScan::Core, &road);
    }

    /// Every point of `scan`'s footprint on a 0.37 m grid, off the lattice on
    /// purpose, with its cut height.
    fn cut_over_the_footprint(scan: CutScan) -> Vec<(Vec2, f32)> {
        let terrain = Terrain::new();
        let (image, bounds) = build_ramp_cut(&terrain, scan);
        let (low, high) = sites::cut_footprint(scan);
        let steps = ((high - low) / 0.37).as_uvec2();
        (0..steps.x)
            .flat_map(|column| (0..steps.y).map(move |row| UVec2::new(column, row)))
            .map(|step| {
                let at = low + step.as_vec2() * 0.37;
                (at, cut_height(&image, bounds, at))
            })
            .collect()
    }

    // Stone left above the drawn ground, even by centimeters, shows through it
    // as dark slivers, and a cut that steps under it shows its teeth on the
    // stone: the cut follows the drawn ground `BURY` under it everywhere.
    fn assert_the_cut_lies_below_the_ground_that_is_drawn(scan: CutScan) {
        let terrain = Terrain::new();
        let cut: Vec<_> = cut_over_the_footprint(scan)
            .into_iter()
            .filter(|&(_, height)| height < UNCUT)
            .collect();
        assert!(
            cut.len() > 10_000,
            "only {} points lie in the cut",
            cut.len()
        );
        for (at, height) in cut {
            let under = terrain.mesh_height_at(at) - height;
            assert!(
                (under - BURY).abs() < 0.005,
                "the cut at {at} lies {under} m under the drawn ground"
            );
        }
    }

    /// The cut's edge is the corridor's edge, the one the collision is cleared
    /// to, and runs smoothly with it (within 5 cm) instead of in texel steps.
    fn assert_the_cut_ends_at_the_corridors_edge(scan: CutScan) {
        for (at, height) in cut_over_the_footprint(scan) {
            if scan.past_cut_corridor(at).abs() < 0.05 {
                continue;
            }
            assert_eq!(
                height < UNCUT,
                scan.in_cut_corridor(at),
                "the cut's edge strays from the corridor's at {at}"
            );
        }
    }

    #[test]
    fn the_quarry_cut_ends_at_the_corridors_edge() {
        assert_the_cut_ends_at_the_corridors_edge(CutScan::Quarry);
    }

    #[test]
    fn the_core_cut_ends_at_the_corridors_edge() {
        assert_the_cut_ends_at_the_corridors_edge(CutScan::Core);
    }

    #[test]
    fn the_quarry_cut_lies_below_the_ground_that_is_drawn() {
        assert_the_cut_lies_below_the_ground_that_is_drawn(CutScan::Quarry);
    }

    #[test]
    fn the_core_cut_lies_below_the_ground_that_is_drawn() {
        assert_the_cut_lies_below_the_ground_that_is_drawn(CutScan::Core);
    }

    /// Each cut's bounds in the test app, which tell the cuts apart.
    fn test_bounds(scan: CutScan) -> Vec4 {
        Vec4::splat(scan as usize as f32 + 1.0)
    }

    /// An app that spawns world instances, with a root for each cut scan and
    /// for another site, each holding a stone. Returns the cut scans' stones,
    /// in `CutScan` order, and the other's.
    fn app_with_scans() -> (App, [Entity; 2], Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            WorldSerializationPlugin,
        ))
        .init_asset::<StandardMaterial>()
        .init_asset::<RampCutMaterial>()
        .insert_resource(RampCuts(CutScan::ALL.map(|scan| RampCut {
            bounds: test_bounds(scan),
            heights: Handle::default(),
        })));
        let scene = app
            .world_mut()
            .resource_mut::<Assets<WorldAsset>>()
            .add(WorldAsset::new(World::new()));
        let stone = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let mut scan = |cut: Option<CutScan>| {
            let mut root = app.world_mut().spawn(WorldAssetRoot(scene.clone()));
            root.observe(cut_scan_stone);
            if let Some(cut) = cut {
                root.insert(RampCutScan(cut));
            }
            let root = root.id();
            app.world_mut()
                .spawn((MeshMaterial3d(stone.clone()), ChildOf(root)))
                .id()
        };
        let cuts = CutScan::ALL.map(|cut| scan(Some(cut)));
        let other = scan(None);
        (app, cuts, other)
    }

    /// `scan`'s stone is drawn through its own cut, and the other site's is not cut.
    fn assert_the_scan_draws_through_its_cut(scan: CutScan) {
        let (mut app, cuts, other) = app_with_scans();
        app.update();

        let world = app.world();
        let stone = cuts[scan as usize];
        let material = world
            .get::<MeshMaterial3d<RampCutMaterial>>(stone)
            .and_then(|material| world.resource::<Assets<RampCutMaterial>>().get(&material.0))
            .expect("the stone is drawn through a cut");
        assert_eq!(material.extension.bounds, test_bounds(scan));
        assert!(
            world
                .get::<MeshMaterial3d<StandardMaterial>>(stone)
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

    #[test]
    fn the_quarry_scan_draws_through_the_ramp_cut() {
        assert_the_scan_draws_through_its_cut(CutScan::Quarry);
    }

    #[test]
    fn the_core_scan_draws_through_the_ramp_cut() {
        assert_the_scan_draws_through_its_cut(CutScan::Core);
    }
}
