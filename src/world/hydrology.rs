//! Shared river, lake and swamp profiles for terrain, water and movement.

use bevy::prelude::*;

use super::terrain::{HALF_SIZE, fbm, lerp, smoothstep, value_noise};

pub(super) const WATER_LEVEL: f32 = -3.0;
pub(super) const LAKE: Vec2 = Vec2::new(445.0, 327.0);
/// The marsh's pools and hummocks, south of the highway.
pub(super) const SWAMP: Vec2 = Vec2::new(-720.0, -860.0);
const WEST_BASIN: Vec2 = Vec2::new(-2020.0, 333.0);
/// Fords: the factory road north, Harmont's road to New Harmont, the railway track.
pub(crate) const FORD_X: f32 = -520.0;
pub(super) const WEST_FORD_X: f32 = 695.0;
pub(super) const EAST_FORD_X: f32 = -765.0;

/// The named body of water a point lies in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Water {
    River,
    Ford,
    Tributary,
    Lake,
    WestLake,
    Swamp,
    Coast,
}

pub(super) struct Wetlands {
    river: f32,
    tributary: f32,
    lake: f32,
    marsh: f32,
    coast: f32,
}

/// The main river's course, meandering away from the lake toward the Zone.
fn river_z(x: f32) -> f32 {
    let s = LAKE.x - x;
    LAKE.y + 58.0 * (s / 134.0).sin() + 23.0 * (s / 62.0).sin()
}

/// The tributary's course, from the north down into the main river.
fn tributary_x(z: f32) -> f32 {
    -235.0 - 74.0 * ((z - 360.0) / 113.0).sin()
}

impl Wetlands {
    pub(super) fn at(x: f32, z: f32) -> Self {
        let river = if (z - LAKE.y).abs() < 150.0 {
            let width = 12.5 + 2.7 * (x / 81.0).sin();
            (z - river_z(x)).abs() / width
        } else {
            f32::INFINITY
        };
        let tributary = if x > -395.0 && x < -90.0 && z > 215.0 {
            let d = (x - tributary_x(z)).abs() / (7.5 + 1.2 * (z / 49.0).sin());
            d.max((river_z(x) - z - 1.0) / 5.0)
        } else {
            f32::INFINITY
        };
        let lake_p = Vec2::new(x, z) - LAKE;
        let inner_lake = if lake_p.abs().cmple(Vec2::new(226.0, 166.0)).all() {
            (lake_p / Vec2::new(124.0, 90.0)).length()
                + (value_noise(x / 47.0, z / 47.0, 0x1A5E) - 0.5) * 0.26
        } else {
            f32::INFINITY
        };
        let basin = Vec2::new(x, z) - WEST_BASIN;
        let west_lake = if basin.abs().cmple(Vec2::new(400.0, 335.0)).all() {
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
        let marsh = if swamp_p.abs().cmple(Vec2::new(220.0, 175.0)).all() {
            let irregular = (value_noise(x / 37.0, z / 37.0, 0xB06) - 0.5) * 0.35;
            smoothstep(
                1.16,
                0.57,
                (swamp_p / Vec2::new(166.0, 126.0)).length() + irregular,
            )
        } else {
            0.0
        };
        Self {
            river,
            tributary,
            lake: inner_lake.min(west_lake),
            marsh,
            coast,
        }
    }

    pub(super) fn soil_height(&self, x: f32, z: f32, ground: f32) -> f32 {
        let mut height = f32::INFINITY;
        for (d, falloff, depth) in [
            (
                self.river,
                4.35 + ford(x, WEST_FORD_X, 12.0),
                lerp(
                    4.3,
                    0.32,
                    ford(x, FORD_X, 14.0)
                        .max(ford(x, WEST_FORD_X, 12.0))
                        .max(ford(x, EAST_FORD_X, 14.0)),
                ),
            ),
            (self.tributary, 4.75, lerp(3.1, 0.36, ford(z, 563.0, 11.0))),
            (self.lake, 1.65, 12.0),
        ] {
            if d >= falloff {
                continue;
            }
            let offset = if d < 1.0 {
                (d * d - 1.0) * depth
            } else {
                (d - 1.0) * 4.8
            };
            let channel = WATER_LEVEL + offset;
            height = height.min(lerp(channel, ground, smoothstep(1.06, falloff, d)));
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
        if height.is_finite() { height } else { ground }
    }

    /// Signed water depth, restricted to the watercourses and swamp pools.
    pub(super) fn depth(&self, ground: f32) -> f32 {
        let boundary = ((1.24 - self.river).max(1.24 - self.tributary) * 7.0)
            .max((1.22 - self.lake) * 48.0)
            .max((self.marsh - 0.62) * 6.0)
            .max(self.coast * 0.115);
        (WATER_LEVEL - ground).min(boundary)
    }

    /// Which water `(x, z)` lies in or on the bank of, if any.
    pub(super) fn water(&self, x: f32, z: f32) -> Option<Water> {
        let at = Vec2::new(x, z);
        let crossing = [FORD_X, WEST_FORD_X, EAST_FORD_X]
            .into_iter()
            .any(|ford| (x - ford).abs() < 14.0);
        if self.marsh > 0.35 {
            Some(Water::Swamp)
        } else if self.lake < 1.25 {
            Some(if at.distance(LAKE) < at.distance(WEST_BASIN) {
                Water::Lake
            } else {
                Water::WestLake
            })
        } else if self.tributary < 1.3 {
            Some(Water::Tributary)
        } else if self.river < 1.3 {
            Some(if crossing { Water::Ford } else { Water::River })
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
            .max(smoothstep(
                1.42,
                0.84,
                self.river.min(self.tributary).min(self.lake),
            ))
    }

    pub(super) fn flow(&self) -> Vec2 {
        if self.coast >= 0.0 {
            return Vec2::new(-0.18, 0.32);
        }
        let current = 1.0 - smoothstep(0.82, 1.18, self.lake);
        let stream = if self.tributary < self.river {
            Vec2::NEG_Y
        } else {
            Vec2::NEG_X
        };
        stream * (1.0 - current).max(0.08) * (1.0 - self.marsh * 0.92)
    }
}

fn ford(at: f32, center: f32, width: f32) -> f32 {
    smoothstep(width * 2.0, width * 0.52, (at - center).abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Terrain;

    #[test]
    fn lake_and_both_river_branches_are_flooded_but_the_ford_is_driveable() {
        let terrain = Terrain::new();
        assert!(terrain.water_depth_at(LAKE.x, LAKE.y) > 10.0);
        for x in [830.0, 660.0, 290.0, 110.0, -20.0, -170.0, -500.0, -670.0] {
            assert!(
                terrain.water_depth_at(x, river_z(x)) > 2.0,
                "main river dry at {x}"
            );
        }
        for z in [410.0, 480.0, 650.0, 720.0] {
            let x = tributary_x(z);
            assert!(terrain.water_depth_at(x, z) > 2.0, "tributary dry at {z}");
        }
        for z in (310..=365).map(|z| z as f32) {
            assert!(terrain.water_depth_at(FORD_X, z) <= 0.55);
        }
        let depth = terrain.water_depth_at(FORD_X, river_z(FORD_X));
        assert!((0.2..0.45).contains(&depth), "ford depth {depth}");
    }

    #[test]
    fn swamp_contains_both_standing_water_and_dry_hummocks() {
        let terrain = Terrain::new();
        let samples = (-12..=12)
            .flat_map(|x| (-12..=12).map(move |z| SWAMP + Vec2::new(x as f32, z as f32) * 4.0));
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
    fn coastal_water_and_the_western_basin_extend_past_the_playable_edge() {
        let terrain = Terrain::new();
        for x in [-1400.0, -250.0, 0.0, 1150.0] {
            assert_eq!(terrain.water_depth_at(x, -HALF_SIZE + 12.0), 0.0);
            assert!(terrain.water_depth_at(x, -HALF_SIZE - 240.0) > 5.0);
        }
        assert!(terrain.water_depth_at(WEST_BASIN.x, WEST_BASIN.y) > 10.0);
        assert!(terrain.water_depth_at(-2250.0, river_z(-2250.0)) > 2.0);
    }
}
