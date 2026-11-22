//! The ground fog's map of the Zone: the surface the fog rests on (land or
//! open water) and how readily fog gathers there. Fog favors water and its
//! banks, vegetated ground, hollows where cold air pools, and low elevations.
//! Sampled from the distant terrain tiles while the landscape is prepared.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::super::terrain::{SCENIC_TILE_RADIUS, TILE_SIZE, TerrainGrid, WaterVertex, smoothstep};

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

pub(super) fn origin() -> Vec2 {
    Vec2::splat(-EXTENT * 0.5)
}

pub(in crate::world) struct FogField {
    surface: Vec<f32>,
    wet: Vec<f32>,
    exposed: Vec<f32>,
}

impl Default for FogField {
    fn default() -> Self {
        Self {
            surface: vec![UNMAPPED; SIZE * SIZE],
            wet: vec![0.0; SIZE * SIZE],
            exposed: vec![1.0; SIZE * SIZE],
        }
    }
}

impl FogField {
    /// Records one distant tile's vertices: surface height (water level where
    /// flooded), wetness, and exposed earth from the terrain's shader weights.
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
            self.exposed[index] = weight[0];
        }
    }

    /// Per texel: surface height (coarse, remainder) and fog affinity.
    fn texels(&self) -> Vec<[f32; 4]> {
        let pooled = box_blur(&self.surface, 12);
        let damp = box_blur(&self.wet, 4);
        (0..SIZE * SIZE)
            .map(|index| {
                let surface = self.surface[index];
                let water = self.wet[index].max(damp[index] * 1.6).min(1.0);
                let plants = 1.0 - self.exposed[index];
                let hollow = smoothstep(0.5, 7.0, pooled[index] - surface);
                let lowland = 1.0 - smoothstep(4.0, 40.0, surface);
                let affinity =
                    (0.45 * water + 0.2 * plants + 0.25 * hollow + 0.2 * lowland).clamp(0.0, 1.0);
                let coarse = (surface / HEIGHT_STEP).round() * HEIGHT_STEP;
                [coarse, surface - coarse, affinity, 1.0]
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
    use super::super::super::hydrology::{LAKE, WATER_LEVEL};
    use super::super::super::terrain::Terrain;
    use super::*;

    fn texel(texels: &[[f32; 4]], at: Vec2) -> [f32; 4] {
        let cell = ((at - origin()) / SPACING).round().as_uvec2();
        texels[cell.y as usize * SIZE + cell.x as usize]
    }

    #[test]
    fn fog_rests_on_open_water_and_favors_it_over_mountain_tops() {
        let terrain = Terrain::new();
        // The summit of the ridge north of the railway.
        let summit = Vec2::new(-1070.0, 420.0);
        let mut field = FogField::default();
        for at in [LAKE, summit] {
            let grid = TerrainGrid::distant((at / TILE_SIZE).floor().as_ivec2());
            let (ground, water) = terrain.build_grid(grid);
            field.add_tile(grid, &ground, &water);
        }
        let texels = field.texels();
        let lake = texel(&texels, LAKE);
        let peak = texel(&texels, summit);
        assert!((lake[0] + lake[1] - WATER_LEVEL).abs() < 0.05, "{lake:?}");
        assert!(lake[1].abs() <= HEIGHT_STEP * 0.5);
        assert!(
            lake[2] > 0.7 && peak[2] < 0.3,
            "lake {lake:?} peak {peak:?}"
        );
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
