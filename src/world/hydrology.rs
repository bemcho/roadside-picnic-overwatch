//! Shared marsh, lake and coast profiles for terrain, water and movement. The
//! Harmont map has no river: the Zone's water is the marsh's pools, and the
//! lake and the sea lie beyond the map's edges.

use bevy::prelude::*;

use super::terrain::{HALF_SIZE, MARSH_CENTER, MARSH_RADII, fbm, lerp, smoothstep, value_noise};

pub(super) const WATER_LEVEL: f32 = -3.0;
/// The marsh's pools and hummocks, between the railway and the hills.
pub(crate) const SWAMP: Vec2 = Vec2::new(MARSH_CENTER.0, MARSH_CENTER.1);
/// The pools fill the inner part of the marsh hollow.
const POOLS: Vec2 = Vec2::new(MARSH_RADII.0 * 0.88, MARSH_RADII.1 * 0.9);
/// The lake north-east of the quarry, reaching in across the map's east edge.
const EAST_BASIN: Vec2 = Vec2::new(-1930.0, 760.0);

/// The named body of water a point lies in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Water {
    Lake,
    Swamp,
    Coast,
}

pub(super) struct Wetlands {
    lake: f32,
    marsh: f32,
    coast: f32,
}

impl Wetlands {
    pub(super) fn at(x: f32, z: f32) -> Self {
        let basin = Vec2::new(x, z) - EAST_BASIN;
        let lake = if basin.abs().cmple(Vec2::new(400.0, 335.0)).all() {
            (basin / Vec2::new(263.0, 198.0)).length()
                + (value_noise(x / 88.0, z / 88.0, 0x7CE) - 0.5) * 0.24
        } else {
            f32::INFINITY
        };
        let coast = if z < -HALF_SIZE + 90.0 {
            let shore = -HALF_SIZE - 118.0 + 34.0 * (x / 230.0).sin() + 16.0 * (x / 91.0).sin();
            shore - z
        } else {
            f32::NEG_INFINITY
        };
        let swamp_p = Vec2::new(x, z) - SWAMP;
        let marsh = if swamp_p.abs().cmple(POOLS * 1.35).all() {
            let irregular = (value_noise(x / 47.0, z / 47.0, 0xB06) - 0.5) * 0.35;
            smoothstep(1.16, 0.57, (swamp_p / POOLS).length() + irregular)
        } else {
            0.0
        };
        Self { lake, marsh, coast }
    }

    pub(super) fn soil_height(&self, x: f32, z: f32, ground: f32) -> f32 {
        let mut height = ground;
        if self.lake < 1.65 {
            let offset = if self.lake < 1.0 {
                (self.lake * self.lake - 1.0) * 12.0
            } else {
                (self.lake - 1.0) * 4.8
            };
            let bed = WATER_LEVEL + offset;
            height = height.min(lerp(bed, ground, smoothstep(1.06, 1.65, self.lake)));
        }
        if self.marsh > 0.0 {
            let hummocks = fbm(x, z, 0x022B_0905, 3, 1.0 / 16.0);
            let mud = WATER_LEVEL + 0.07 + 2.15 * hummocks;
            height = height.min(lerp(ground, mud, self.marsh));
        }
        if self.coast > -105.0 {
            let seabed = WATER_LEVEL - (self.coast * 0.115).clamp(-8.0, 31.0);
            height = height.min(lerp(ground, seabed, smoothstep(-105.0, -10.0, self.coast)));
        }
        height
    }

    /// Signed water depth, restricted to the lake, the coast and the pools.
    pub(super) fn depth(&self, ground: f32) -> f32 {
        let boundary = ((1.22 - self.lake) * 48.0)
            .max((self.marsh - 0.62) * 6.0)
            .max(self.coast * 0.115);
        (WATER_LEVEL - ground).min(boundary)
    }

    /// Which water `(x, z)` lies in or on the bank of, if any.
    pub(super) fn water(&self) -> Option<Water> {
        if self.marsh > 0.35 {
            Some(Water::Swamp)
        } else if self.lake < 1.25 {
            Some(Water::Lake)
        } else if self.coast > -12.0 {
            Some(Water::Coast)
        } else {
            None
        }
    }

    pub(super) fn marsh(&self) -> f32 {
        self.marsh
    }

    pub(super) fn wetness(&self) -> f32 {
        self.marsh
            .max(smoothstep(-35.0, 4.0, self.coast))
            .max(smoothstep(1.42, 0.84, self.lake))
    }

    pub(super) fn flow(&self) -> Vec2 {
        if self.coast >= 0.0 {
            return Vec2::new(-0.18, 0.32);
        }
        Vec2::NEG_X * 0.08 * (1.0 - self.marsh * 0.92)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Terrain;

    #[test]
    fn swamp_contains_both_standing_water_and_dry_hummocks() {
        let terrain = Terrain::new();
        let samples = (-12..=12)
            .flat_map(|x| (-12..=12).map(move |z| SWAMP + Vec2::new(x as f32, z as f32) * 8.0));
        let (wet, dry) = samples.fold((0, 0), |(wet, dry), p| {
            if terrain.water_depth_at(p.x, p.y) > 0.0 {
                (wet + 1, dry)
            } else {
                (wet, dry + 1)
            }
        });
        assert!(wet > 100 && dry > 100, "{wet} wet, {dry} dry");
    }

    #[test]
    fn the_map_itself_holds_no_open_water_outside_the_marsh() {
        let terrain = Terrain::new();
        for x in (-15..=15).map(|i| i as f32 * 100.0) {
            for z in (-8..=8).map(|i| i as f32 * 100.0) {
                if Vec2::new(x, z).distance(SWAMP) > 380.0 {
                    assert_eq!(terrain.water_depth_at(x, z), 0.0, "water at ({x}, {z})");
                }
            }
        }
    }

    #[test]
    fn coastal_water_and_the_east_basin_extend_past_the_playable_edge() {
        let terrain = Terrain::new();
        for x in [-1400.0, -250.0, 0.0, 1150.0] {
            assert_eq!(terrain.water_depth_at(x, -HALF_SIZE + 12.0), 0.0);
            assert!(terrain.water_depth_at(x, -HALF_SIZE - 240.0) > 5.0);
        }
        assert!(terrain.water_depth_at(EAST_BASIN.x, EAST_BASIN.y) > 10.0);
    }
}
