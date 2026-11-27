//! The ground fog's map of the Zone: the surface the fog rests on (land or
//! open water), how readily fog gathers there and which side of the Perimeter
//! it lies on. In the Zone fog spreads over all low ground and thickens over
//! water and its banks (the marsh, the coast and the lake), in the quarry
//! basin and in the three dense woods; the high ground stays clear. Harmont,
//! the hangar and the Institute get a thinner, taller fog of their own.
//! Sampled from the distant terrain tiles while the landscape is prepared.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::super::canopy::wood;
use super::super::cordon::PERIMETER_X;
use super::super::terrain::{
    QUARRY_CENTER, QUARRY_RADII, SCENIC_TILE_RADIUS, TILE_SIZE, TerrainGrid, WaterVertex,
    smoothstep,
};

/// Matches the distant tiles' vertex spacing, in meters.
const SPACING: f32 = 8.0;
const SIZE: usize = 2 * SCENIC_TILE_RADIUS as usize * (TILE_SIZE / SPACING) as usize + 1;
/// Width of the mapped square, in meters.
pub(super) const EXTENT: f32 = (SIZE - 1) as f32 * SPACING;
/// Unmapped texels sit far below any fog.
const UNMAPPED: f32 = -1.0e3;
/// Heights are split into a coarse step and a remainder so half floats keep
/// centimeter precision; both halves still interpolate linearly.
const HEIGHT_STEP: f32 = 16.0;
/// Affinity of the Zone's low ground and of Harmont's: enough for fog to
/// gather in moist air, below the water's, the quarry's and the woods'.
const LOWLAND_AFFINITY: f32 = 0.6;
/// Surface heights over which the lowland fog thins out to clear hills.
const HILLS: (f32, f32) = (22.0, 45.0);
/// Half the width of the blend from the Zone's fog to Harmont's, across the
/// Perimeter.
const PERIMETER_BLEND: f32 = 30.0;

/// How readily fog gathers on open land `surface` m high: the low ground's
/// affinity, thinning out over the hills.
pub(super) fn lowland(surface: f32) -> f32 {
    LOWLAND_AFFINITY * (1.0 - smoothstep(HILLS.0, HILLS.1, surface))
}

pub(super) fn origin() -> Vec2 {
    Vec2::splat(-EXTENT * 0.5)
}

pub(in crate::world) struct FogField {
    surface: Vec<f32>,
    wet: Vec<f32>,
}

impl Default for FogField {
    fn default() -> Self {
        Self {
            surface: vec![UNMAPPED; SIZE * SIZE],
            wet: vec![0.0; SIZE * SIZE],
        }
    }
}

impl FogField {
    /// Records one distant tile's vertices: surface height (water level where
    /// flooded) and wetness from the terrain's shader weights.
    pub(in crate::world) fn add_tile(
        &mut self,
        grid: TerrainGrid,
        ground: &Mesh,
        water: &[WaterVertex],
    ) {
        debug_assert_eq!(grid.spacing, SPACING);
        let (
            Some(VertexAttributeValues::Float32x3(positions)),
            Some(VertexAttributeValues::Float32x4(weights)),
        ) = (
            ground.attribute(Mesh::ATTRIBUTE_POSITION),
            ground.attribute(Mesh::ATTRIBUTE_COLOR),
        )
        else {
            return;
        };
        for ((position, weight), water) in positions.iter().zip(weights).zip(water) {
            let cell = ((Vec2::new(position[0], position[2]) - origin()) / SPACING)
                .round()
                .as_ivec2();
            if cell.cmplt(IVec2::ZERO).any() || cell.cmpge(IVec2::splat(SIZE as i32)).any() {
                continue;
            }
            let index = cell.y as usize * SIZE + cell.x as usize;
            self.surface[index] = position[1] + water.depth.max(0.0);
            self.wet[index] = weight[1].max(f32::from(water.depth > 0.0));
        }
    }

    /// Per texel: surface height (coarse, remainder), fog affinity (0 wherever
    /// fog must not form) and how far it lies on Harmont's side (0 in the Zone).
    fn texels(&self) -> Vec<[f32; 4]> {
        let damp = box_blur(&self.wet, 4);
        (0..SIZE * SIZE)
            .map(|index| {
                let surface = self.surface[index];
                let cell = Vec2::new((index % SIZE) as f32, (index / SIZE) as f32);
                let at = origin() + cell * SPACING;
                let water = self.wet[index].max(damp[index] * 1.6).min(1.0);
                let affinity = water
                    .max(quarry_basin(at))
                    .max(0.8 * wood(at))
                    .max(lowland(surface));
                let harmont = smoothstep(
                    PERIMETER_X - PERIMETER_BLEND,
                    PERIMETER_X + PERIMETER_BLEND,
                    at.x,
                );
                let coarse = (surface / HEIGHT_STEP).round() * HEIGHT_STEP;
                [coarse, surface - coarse, affinity, harmont]
            })
            .collect()
    }

    pub(in crate::world) fn into_image(self) -> Image {
        let data = self
            .texels()
            .into_iter()
            .flatten()
            .flat_map(|value| half_float(value).to_le_bytes())
            .collect();
        Image::new(
            Extent3d {
                width: SIZE as u32,
                height: SIZE as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba16Float,
            RenderAssetUsages::RENDER_WORLD,
        )
    }
}

/// 1 on the quarry's floor and benches, easing to 0 at its rim.
fn quarry_basin(at: Vec2) -> f32 {
    let from_center = (at - Vec2::from(QUARRY_CENTER)) / Vec2::from(QUARRY_RADII);
    1.0 - smoothstep(0.75, 1.0, from_center.length())
}

/// Mean over a `(2 * radius + 1)²` square, clamped at the edges.
fn box_blur(values: &[f32], radius: usize) -> Vec<f32> {
    let pass = |values: &[f32], stride: usize, step: usize| {
        let mut out = vec![0.0; values.len()];
        for line in 0..SIZE {
            let at = |i: usize| values[line * stride + i * step];
            let mut sum: f32 =
                (0..=radius).map(|i| at(i.min(SIZE - 1))).sum::<f32>() + at(0) * radius as f32;
            for i in 0..SIZE {
                out[line * stride + i * step] = sum / (2 * radius + 1) as f32;
                sum += at((i + radius + 1).min(SIZE - 1)) - at(i.saturating_sub(radius));
            }
        }
        out
    };
    pass(&pass(values, SIZE, 1), 1, SIZE)
}

/// IEEE half-precision bits, rounded to nearest; tiny values flush to zero.
fn half_float(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exponent <= 0 {
        return sign;
    }
    if exponent >= 31 {
        return sign | 0x7c00;
    }
    let mantissa = (bits & 0x7f_ffff) + 0x1000;
    if mantissa & 0x80_0000 != 0 {
        sign | (((exponent + 1) as u16) << 10)
    } else {
        sign | ((exponent as u16) << 10) | (mantissa >> 13) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::canopy::WOODS;
    use super::super::super::hydrology::{SWAMP, WATER_LEVEL};
    use super::super::super::terrain::{BASE, Terrain};
    use super::super::super::town::PLAZA;
    use super::*;

    /// The texels at `points`, from a field holding just their tiles.
    fn texels_at(terrain: &Terrain, points: &[Vec2]) -> Vec<[f32; 4]> {
        let mut field = FogField::default();
        for at in points {
            let grid = TerrainGrid::distant((*at / TILE_SIZE).floor().as_ivec2());
            let (ground, water) = terrain.build_grid(grid);
            field.add_tile(grid, &ground, &water);
        }
        let texels = field.texels();
        points
            .iter()
            .map(|at| {
                let cell = ((*at - origin()) / SPACING).round().as_uvec2();
                texels[cell.y as usize * SIZE + cell.x as usize]
            })
            .collect()
    }

    /// Dry, open low ground in the Zone, between the Perimeter and the marsh.
    const ZONE_LOWLAND: Vec2 = Vec2::new(520.0, -300.0);

    /// The deepest of the marsh's pools.
    fn marsh_pool(terrain: &Terrain) -> Vec2 {
        let depth = |at: Vec2| terrain.water_depth_at(at.x, at.y);
        (-30..=30)
            .flat_map(|i| (-30..=30).map(move |j| SWAMP + Vec2::new(i as f32, j as f32) * 6.0))
            .max_by(|a, b| depth(*a).total_cmp(&depth(*b)))
            .expect("the marsh has pools")
    }

    #[test]
    fn fog_rests_on_open_water() {
        let terrain = Terrain::new();
        let [pool] = texels_at(&terrain, &[marsh_pool(&terrain)])[..] else {
            unreachable!("one point in, one texel out");
        };
        assert!((pool[0] + pool[1] - WATER_LEVEL).abs() < 0.05, "{pool:?}");
        assert!(pool[1].abs() <= HEIGHT_STEP * 0.5);
    }

    #[test]
    fn fog_gathers_on_low_ground_most_over_water_and_never_on_the_hills() {
        let terrain = Terrain::new();
        let places = [
            ("marsh pool", marsh_pool(&terrain)),
            ("quarry floor", Vec2::from(QUARRY_CENTER)),
            ("north wood", WOODS[2].0),
            ("Zone lowland", ZONE_LOWLAND),
            ("mountain summit", Vec2::new(-744.0, 400.0)),
            ("hangar", Vec2::from(BASE)),
            ("town plaza", PLAZA),
        ];
        let points: Vec<_> = places.iter().map(|(_, at)| *at).collect();
        let texels = texels_at(&terrain, &points);
        let [pool, quarry, wood, lowland, summit, hangar, plaza] = texels[..] else {
            unreachable!("seven points in, seven texels out");
        };
        for (name, texel) in [("pool", pool), ("quarry", quarry), ("wood", wood)] {
            assert!(texel[2] > lowland[2], "{name} gathers more fog: {texel:?}");
        }
        assert!((lowland[2] - LOWLAND_AFFINITY).abs() < 0.05, "{lowland:?}");
        assert!(summit[2] < 0.05, "the hills stay clear: {summit:?}");
        for texel in [hangar, plaza] {
            assert!(texel[2] > 0.5, "Harmont gathers fog too: {texel:?}");
            assert_eq!(texel[3], 1.0, "on Harmont's side: {texel:?}");
        }
        assert_eq!(lowland[3], 0.0, "in the Zone: {lowland:?}");
    }

    #[test]
    fn half_floats_round_trip_the_encoded_ranges() {
        let decode = |bits: u16| {
            let exponent = i32::from((bits >> 10) & 0x1f) - 15;
            let mantissa = 1.0 + f32::from(bits & 0x3ff) / 1024.0;
            let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
            if bits & 0x7fff == 0 {
                0.0
            } else {
                sign * mantissa * 2f32.powi(exponent)
            }
        };
        for value in [-1024.0, -16.0, -3.0, 0.37, 1.0, 7.99, 208.0] {
            let decoded = decode(half_float(value));
            assert!(
                (decoded - value).abs() <= value.abs() / 1024.0,
                "{value} -> {decoded}"
            );
        }
        assert_eq!(half_float(0.0), 0);
    }
}
