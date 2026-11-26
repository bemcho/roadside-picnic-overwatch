//! Procedural terrain for the Zone.
//!
//! Topography inspired by the Zone from *Roadside Picnic*: rolling, overgrown
//! hills around Harmont, the old quarry, a mountain range, and a river basin.
//!
//! Everything is deterministic — same seed, same Zone on every run.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::hydrology::Wetlands;
use super::roads::{RoadNetwork, RoadSample};

const PLAYABLE_HALF_METRES: u32 = 1720;
/// The traversal limit; visible terrain extends past it.
pub const HALF_SIZE: f32 = PLAYABLE_HALF_METRES as f32;

pub(super) const TILE_SIZE: f32 = 128.0;
pub(super) const TILE_RADIUS: i32 = PLAYABLE_HALF_METRES.div_ceil(TILE_SIZE as u32) as i32;
pub(super) const CELLS: usize = 64;
pub(super) const SPACING: f32 = TILE_SIZE / CELLS as f32;
pub(super) const SCENIC_TILE_RADIUS: i32 = 16;
pub(super) const HORIZON_TILE_SIZE: f32 = TILE_SIZE * 4.0;
pub(super) const HORIZON_TILE_RADIUS: i32 = 9;
const SCENIC_HALF_SIZE: f32 = TILE_SIZE * SCENIC_TILE_RADIUS as f32;
const DISTANT_SPACING: f32 = 8.0;
const HORIZON_SPACING: f32 = 16.0;

#[derive(Clone, Copy)]
pub(super) struct TerrainGrid {
    pub origin: Vec2,
    pub spacing: f32,
    pub cells: usize,
}

impl TerrainGrid {
    pub fn detail(tile: IVec2) -> Self {
        Self {
            origin: tile.as_vec2() * TILE_SIZE,
            spacing: SPACING,
            cells: CELLS,
        }
    }

    pub fn distant(tile: IVec2) -> Self {
        Self {
            origin: tile.as_vec2() * TILE_SIZE,
            spacing: DISTANT_SPACING,
            cells: (TILE_SIZE / DISTANT_SPACING) as usize,
        }
    }

    pub fn horizon(tile: IVec2) -> Self {
        Self {
            origin: tile.as_vec2() * HORIZON_TILE_SIZE,
            spacing: HORIZON_SPACING,
            cells: (HORIZON_TILE_SIZE / HORIZON_SPACING) as usize,
        }
    }
}

/// Fixed noise seed.
const SEED: u32 = 0x1972_2016;

/// The quarry at the map's east edge (map px 1235, 375): a terraced oval pit
/// holding the Meat Grinder on its west rim and the Golden Sphere below.
pub(super) const QUARRY_CENTER: (f32, f32) = (-1272.0, 19.0);
pub(super) const QUARRY_RADII: (f32, f32) = (320.0, 530.0);
const QUARRY_DEPTH: f32 = 32.0;
const QUARRY_BENCHES: f32 = 3.0;

/// The bald hill: a scorched, bare mound among the industrial ruins.
pub(super) const BALD_CENTER: (f32, f32) = (288.0, 283.0);
const BALD_HEIGHT: f32 = 12.0;
pub(super) const BALD_SIGMA: f32 = 22.0;

/// The marsh: a broad, stagnant lowland between the railway and the hills,
/// south of the Chapter 4 track.
pub(super) const MARSH_CENTER: (f32, f32) = (-330.0, -420.0);
pub(super) const MARSH_RADII: (f32, f32) = (280.0, 250.0);
const MARSH_DEPTH: f32 = 2.5;

/// The Institute's hangar in Harmont, south of the Institute by the Perimeter
/// gate (map px 355, 430), on a levelled pad that fades back into the land.
pub const BASE: (f32, f32) = (840.0, -113.0);

/// The terrain tile holding the hangar, for test fixtures.
#[cfg(test)]
pub(super) fn base_tile() -> IVec2 {
    (Vec2::from(BASE) / TILE_SIZE).floor().as_ivec2()
}

/// Where the stalker starts and respawns: inside the hangar, facing its bay.
pub const SPAWN: (f32, f32) = (840.0, -108.0);

/// Deterministic terrain model. Pure math until [`Terrain::build_grid`] is
/// called — insert as a resource so any system can sample the ground.
#[derive(Resource, Clone)]
pub struct Terrain {
    pub(super) roads: RoadNetwork,
}

impl Terrain {
    pub fn new() -> Self {
        Self {
            roads: RoadNetwork::new(|at| sited_height(at.x, at.y)),
        }
    }

    /// Height of the terrain surface at world position `(x, z)`.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        self.sample(x, z).0
    }

    /// Height on the rendered tile triangles, including between grid vertices.
    pub(crate) fn mesh_height_at(&self, at: Vec2) -> f32 {
        if at.abs().max_element() > TILE_RADIUS as f32 * TILE_SIZE {
            self.scenic_height_at(at)
        } else {
            self.interpolated_height(at, SPACING)
        }
    }

    pub(super) fn scenic_height_at(&self, at: Vec2) -> f32 {
        let spacing = if at.abs().max_element() > SCENIC_HALF_SIZE {
            HORIZON_SPACING
        } else {
            DISTANT_SPACING
        };
        self.interpolated_height(at, spacing)
    }

    fn interpolated_height(&self, at: Vec2, spacing: f32) -> f32 {
        let grid = at / spacing;
        let cell = grid.floor() * spacing;
        let uv = grid - grid.floor();
        let x = cell.x + spacing;
        let z = cell.y + spacing;
        let b = self.vertex_height(Vec2::new(x, cell.y), self.height_at(x, cell.y));
        let c = self.vertex_height(Vec2::new(cell.x, z), self.height_at(cell.x, z));
        if uv.x + uv.y <= 1.0 {
            let a = self.vertex_height(cell, self.height_at(cell.x, cell.y));
            a + (b - a) * uv.x + (c - a) * uv.y
        } else {
            let d = self.vertex_height(Vec2::new(x, z), self.height_at(x, z));
            d + (c - d) * (1.0 - uv.x) + (b - d) * (1.0 - uv.y)
        }
    }

    fn vertex_height(&self, at: Vec2, sampled: f32) -> f32 {
        edge_segment(at).map_or(sampled, |(a, b, t)| {
            lerp(self.height_at(a.x, a.y), self.height_at(b.x, b.y), t)
        })
    }

    fn stitched_sample(&self, at: Vec2, sampled: f32, wet: &Wetlands) -> (f32, WaterVertex) {
        let water_at = |height, wet: &Wetlands| WaterVertex {
            depth: wet.depth(height),
            marsh: wet.marsh(),
            flow: wet.flow(),
        };
        let Some((a, b, t)) = edge_segment(at) else {
            return (sampled, water_at(sampled, wet));
        };
        let (ha, wa) = self.sample(a.x, a.y);
        let (hb, wb) = self.sample(b.x, b.y);
        let wa = water_at(ha, &wa);
        let wb = water_at(hb, &wb);
        (
            lerp(ha, hb, t),
            WaterVertex {
                depth: lerp(wa.depth, wb.depth, t),
                marsh: lerp(wa.marsh, wb.marsh, t),
                flow: wa.flow.lerp(wb.flow, t),
            },
        )
    }

    pub(crate) fn water_depth_at(&self, x: f32, z: f32) -> f32 {
        let wetlands = Wetlands::at(x, z);
        let h = self.land_height(x, z, &wetlands);
        wetlands.depth(h).max(0.0)
    }

    pub(crate) fn can_wade(&self, from: Vec2, to: Vec2, limit: f32) -> bool {
        let depth = self.water_depth_at(to.x, to.y);
        depth <= limit || depth < self.water_depth_at(from.x, from.y)
    }

    fn sample(&self, x: f32, z: f32) -> (f32, Wetlands) {
        let wetlands = Wetlands::at(x, z);
        (self.land_height(x, z, &wetlands), wetlands)
    }

    fn land_height(&self, x: f32, z: f32, wetlands: &Wetlands) -> f32 {
        let mut h = sited_height(x, z);

        // Road: the track is flattened toward a gentle ramp between its ends,
        // over pads and up to the dioramas' fronts alike.
        let road = self.roads.sample(Vec2::new(x, z));
        if let Some(road) = &road {
            h = lerp(h, road.height, road.mask());
        }
        h = wetlands.soil_height(x, z, h);
        if let Some(ground) = super::sites::diorama_ground(Vec2::new(x, z)) {
            h = ground;
            // A diorama's own ground hides every road but a ramp, which is
            // cut into it (or built up on it) after the override.
            if let Some(ramp) = road.filter(RoadSample::is_ramp) {
                h = lerp(h, ramp.height, ramp.mask());
            }
        }

        // Base: levelled flat for the yard.
        let pad = self.base_mask(x, z);
        if pad > 0.0 {
            h = lerp(h, hills(BASE.0, BASE.1), pad);
        }

        h - super::cordon::hole_depth(Vec2::new(x, z))
    }

    /// 1.0 in the level staging yard, fading into the surrounding terrain.
    pub(super) fn base_mask(&self, x: f32, z: f32) -> f32 {
        super::cordon::yard_grade(Vec2::new(x, z))
    }

    /// 1.0 on the road centerline, 0.0 away from it.
    pub(crate) fn road_mask(&self, x: f32, z: f32) -> f32 {
        self.roads
            .sample(Vec2::new(x, z))
            .map_or(0.0, |road| road.mask())
    }

    pub(super) fn normal_at(&self, x: f32, z: f32) -> Vec3 {
        let e = 0.25;
        let hx = (self.height_at(x + e, z) - self.height_at(x - e, z)) / (2.0 * e);
        let hz = (self.height_at(x, z + e) - self.height_at(x, z - e)) / (2.0 * e);
        Vec3::new(-hx, 1.0, -hz).normalize()
    }

    /// How readily trees grow at `(x, z)`, from 0.0 (never) to 1.0: none on
    /// the road, in the quarry, at the base, or on steep ground; few on the
    /// scorched hill.
    pub fn woodland_at(&self, x: f32, z: f32) -> f32 {
        let qdx = (x - QUARRY_CENTER.0) / QUARRY_RADII.0;
        let qdz = (z - QUARRY_CENTER.1) / QUARRY_RADII.1;
        let outside_quarry = smoothstep(1.1, 1.3, (qdx * qdx + qdz * qdz).sqrt());

        let bdx = x - BALD_CENTER.0;
        let bdz = z - BALD_CENTER.1;
        let ash = (-(bdx * bdx + bdz * bdz) / (2.0 * BALD_SIGMA * BALD_SIGMA)).exp();

        let off_road = 1.0 - smoothstep(0.05, 0.3, self.road_mask(x, z));
        let off_base = 1.0 - self.base_mask(x, z);
        if outside_quarry * off_road * off_base == 0.0
            || super::sites::clear_footprint(Vec2::new(x, z), 0.0)
        {
            return 0.0;
        }
        let (h, wetlands) = self.sample(x, z);
        let dry = smoothstep(0.05, -0.5, wetlands.depth(h));
        let normal = self.normal_at(x, z);
        let soil = smoothstep(1.05, 0.54, normal.xz().length() / normal.y);
        let alpine = smoothstep(119.0, 75.0, h);

        outside_quarry * soil * dry * alpine * off_road * off_base * (1.0 - 0.85 * ash)
    }

    /// Shader data, not a tint: exposed earth, wetness, ash, macro variation.
    fn surface_weights_at(
        &self,
        x: f32,
        z: f32,
        h: f32,
        slope: f32,
        wetlands: &Wetlands,
    ) -> [f32; 4] {
        let qdx = (x - QUARRY_CENTER.0) / QUARRY_RADII.0;
        let qdz = (z - QUARRY_CENTER.1) / QUARRY_RADII.1;
        let quarry = smoothstep(1.12, 0.85, (qdx * qdx + qdz * qdz).sqrt());

        let marsh = smoothstep(1.0, 0.6, marsh_distance(x, z));

        let bdx = x - BALD_CENTER.0;
        let bdz = z - BALD_CENTER.1;
        let ash = (-(bdx * bdx + bdz * bdz) / (2.0 * BALD_SIGMA * BALD_SIGMA)).exp();

        let exposed = smoothstep(0.35, 0.85, slope)
            .max(self.road_mask(x, z))
            .max(quarry)
            .max(ash * 0.95)
            .max(smoothstep(42.0, 110.0, h) * 0.92)
            .max(self.base_mask(x, z));
        let wetness = wetlands.wetness().max(marsh * 0.72);
        let variation = lerp(0.78, 1.12, value_noise(x * 0.04, z * 0.04, SEED + 5));
        [exposed, wetness, ash, variation]
    }

    /// A shared lattice and normal halo make neighboring chunks meet exactly.
    pub(super) fn build_grid(&self, grid: TerrainGrid) -> (Mesh, Vec<WaterVertex>) {
        let TerrainGrid {
            origin,
            spacing,
            cells,
        } = grid;
        let n = cells + 1;
        let halo = n + 2;
        let samples: Vec<_> = (0..halo)
            .flat_map(|z| {
                (0..halo).map(move |x| {
                    self.sample(
                        origin.x + (x as f32 - 1.0) * spacing,
                        origin.y + (z as f32 - 1.0) * spacing,
                    )
                })
            })
            .collect();
        let mut positions = Vec::with_capacity(n * n);
        let mut normals = Vec::with_capacity(n * n);
        let mut colors = Vec::with_capacity(n * n);
        let mut water = Vec::with_capacity(n * n);

        for iz in 0..n {
            let z = origin.y + iz as f32 * spacing;
            for ix in 0..n {
                let x = origin.x + ix as f32 * spacing;
                let i = (iz + 1) * halo + ix + 1;
                let (sampled, wet) = &samples[i];
                let (h, wet_vertex) = self.stitched_sample(Vec2::new(x, z), *sampled, wet);
                let normal =
                    if spacing != SPACING && (ix == 0 || iz == 0 || ix == cells || iz == cells) {
                        Vec3::new(
                            self.height_at(x - SPACING, z) - self.height_at(x + SPACING, z),
                            2.0 * SPACING,
                            self.height_at(x, z - SPACING) - self.height_at(x, z + SPACING),
                        )
                        .normalize()
                    } else {
                        Vec3::new(
                            samples[i - 1].0 - samples[i + 1].0,
                            2.0 * spacing,
                            samples[i - halo].0 - samples[i + halo].0,
                        )
                        .normalize()
                    };
                let slope = normal.xz().length() / normal.y;
                positions.push([x, h, z]);
                normals.push(normal.to_array());
                colors.push(self.surface_weights_at(x, z, h, slope, wet));
                water.push(wet_vertex);
            }
        }

        let mut indices = Vec::with_capacity(cells * cells * 6);
        for iz in 0..cells {
            for ix in 0..cells {
                let a = (iz * n + ix) as u16;
                let b = a + 1;
                let c = a + n as u16;
                let d = c + 1;
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_indices(Indices::U16(indices));
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        (mesh, water)
    }
}

#[derive(Clone, Copy)]
pub(super) struct WaterVertex {
    pub depth: f32,
    pub marsh: f32,
    pub flow: Vec2,
}

// Intermediate land and water vertices share the straight segments of the coarser tile.
fn edge_segment(at: Vec2) -> Option<(Vec2, Vec2, f32)> {
    let (axis, coordinate) = if at.x % TILE_SIZE == 0.0 {
        (Vec2::Y, at.y)
    } else if at.y % TILE_SIZE == 0.0 {
        (Vec2::X, at.x)
    } else {
        return None;
    };
    let step = if at.abs().max_element() >= SCENIC_HALF_SIZE {
        HORIZON_SPACING
    } else {
        DISTANT_SPACING
    };
    let offset = coordinate.rem_euclid(step);
    if offset == 0.0 {
        return None;
    }
    let a = at - axis * offset;
    Some((a, a + axis * step, offset / step))
}

fn regional_height(x: f32, z: f32) -> f32 {
    let lowland = 1.0 - 0.85 * smoothstep(1.3, 0.6, marsh_distance(x, z));
    hills(x, z) * lowland + mountains(x, z) + frontier_relief(x, z)
}

/// Distance from the marsh's middle, in marsh radii.
pub(super) fn marsh_distance(x: f32, z: f32) -> f32 {
    Vec2::new(
        (x - MARSH_CENTER.0) / MARSH_RADII.0,
        (z - MARSH_CENTER.1) / MARSH_RADII.1,
    )
    .length()
}

/// The land with the sites' level pads and the dioramas' own ground on it,
/// before roads and water: what the roads are graded over.
fn sited_height(x: f32, z: f32) -> f32 {
    let h = landform_height(x, z);
    match super::sites::foundation(Vec2::new(x, z)) {
        Some((height, weight)) => lerp(h, height, weight),
        None => h,
    }
}

/// The land before roads, water and sites shape it: the hills and mountains
/// with the bald hill, the marsh hollow and the quarry.
pub(super) fn landform_height(x: f32, z: f32) -> f32 {
    let mut h = regional_height(x, z);

    // Bald hill: a smooth scorched mound.
    let bdx = x - BALD_CENTER.0;
    let bdz = z - BALD_CENTER.1;
    let bd2 = bdx * bdx + bdz * bdz;
    h += BALD_HEIGHT * (-bd2 / (2.0 * BALD_SIGMA * BALD_SIGMA)).exp();

    // Marsh: a broad lowland, flattened and sunk.
    let marsh_d = marsh_distance(x, z);
    h -= MARSH_DEPTH * smoothstep(1.0, 0.35, marsh_d);

    // Quarry: benches stepping down to the floor, a raised lip and
    // micro-relief on the floor.
    let qdx = (x - QUARRY_CENTER.0) / QUARRY_RADII.0;
    let qdz = (z - QUARRY_CENTER.1) / QUARRY_RADII.1;
    let quarry_d = (qdx * qdx + qdz * qdz).sqrt();
    let benches = smoothstep(1.06, 0.3, quarry_d) * QUARRY_BENCHES;
    let cut =
        QUARRY_DEPTH * (benches.floor() + smoothstep(0.62, 1.0, benches.fract())) / QUARRY_BENCHES;
    let lip = 0.9 * (-((quarry_d - 1.08) / 0.13).powi(2)).exp();
    let floor_noise = if quarry_d < 0.9 {
        (value_noise(x * 0.3, z * 0.3, SEED + 11) - 0.5) * 0.9 * smoothstep(0.9, 0.2, quarry_d)
    } else {
        0.0
    };
    h - cut + lip + floor_noise
}

/// Rolling country: flat-ish town land in Harmont, the suburbs' low hills in
/// the Zone, and higher ground north and south of the map's frame.
fn hills(x: f32, z: f32) -> f32 {
    let wild = smoothstep(730.0, 640.0, x).max(smoothstep(820.0, 980.0, z.abs()));
    let local = lerp(3.0, 6.5, wild) * fbm(x, z, SEED, 3, 1.0 / 48.0);
    let rolling = lerp(0.15, 0.6, wild).max(smoothstep(820.0, 980.0, z.abs()));
    local + rolling * 17.0 * fbm(x, z, SEED + 4, 3, 1.0 / 225.0)
}

/// Hills beyond the map's frame, north and south, clear of the railway and
/// the Perimeter.
fn frontier_relief(x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    if z.abs() < 1000.0 {
        return 0.0;
    }
    [
        (Vec2::new(-1500.0, 1350.0), Vec2::new(360.0, 330.0), 118.0),
        (Vec2::new(250.0, 1450.0), Vec2::new(420.0, 290.0), 99.0),
        (Vec2::new(1350.0, 1330.0), Vec2::new(380.0, 320.0), 107.0),
        (Vec2::new(-1150.0, -1350.0), Vec2::new(420.0, 330.0), 104.0),
        (Vec2::new(-250.0, -1450.0), Vec2::new(330.0, 270.0), 86.0),
        (Vec2::new(1300.0, -1350.0), Vec2::new(380.0, 330.0), 96.0),
    ]
    .into_iter()
    .map(|(center, radii, height)| {
        height * smoothstep(1.0, 0.0, ((p - center) / radii).length_squared()).powi(2)
    })
    .sum()
}

/// The Hills / Ridge between the railway and the quarry, north and south of
/// the pass the Chapter 4 track climbs: center, radii and summit height.
const MOUNTAINS: [(Vec2, Vec2, f32); 2] = [
    (Vec2::new(-744.0, 400.0), Vec2::new(180.0, 270.0), 130.0),
    (Vec2::new(-805.0, -440.0), Vec2::new(140.0, 290.0), 120.0),
];

fn mountains(x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    let mut summit: f32 = 0.0;
    for (center, radii, height) in MOUNTAINS {
        let d2 = ((p - center) / radii).length_squared();
        let profile = smoothstep(1.0, 0.0, d2).powf(1.85);
        summit = smooth_max(summit, profile * height);
    }
    if summit < 0.001 {
        return 0.0;
    }
    let ridges = 1.0 - fbm(x + z * 0.4, z, SEED + 23, 4, 1.0 / 92.0).abs();
    summit * (0.67 + 0.32 * ridges.powi(3))
        + smoothstep(0.0, 60.0, summit) * 8.5 * fbm(x, z, SEED + 24, 3, 1.0 / 33.0)
}

fn smooth_max(a: f32, b: f32) -> f32 {
    let overlap = (1.0 - (a - b).abs() / 12.0).max(0.0);
    a.max(b) + 3.0 * overlap * overlap * smoothstep(0.0, 10.0, a.min(b))
}

impl Default for Terrain {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Deterministic integer-lattice hash in `[0, 1)`.
pub(crate) fn lattice_hash(ix: i32, iz: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x8da6_b343)
        ^ (iz as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h as f32 / u32::MAX as f32
}

/// Bilinearly interpolated value noise in `[0, 1)`.
pub(super) fn value_noise(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor();
    let iz = z.floor();
    let fx = x - ix;
    let fz = z - iz;
    let (ix, iz) = (ix as i32, iz as i32);

    let a = lattice_hash(ix, iz, seed);
    let b = lattice_hash(ix + 1, iz, seed);
    let c = lattice_hash(ix, iz + 1, seed);
    let d = lattice_hash(ix + 1, iz + 1, seed);

    // Quintic interpolation keeps curvature continuous across lattice boundaries.
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let ux = fade(fx);
    let uz = fade(fz);
    let ab = a + (b - a) * ux;
    let cd = c + (d - c) * ux;
    ab + (cd - ab) * uz
}

/// Fractal Brownian motion: octaves of value noise, normalized to `[-1, 1]`.
pub(super) fn fbm(x: f32, z: f32, seed: u32, octaves: u32, base_freq: f32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut freq = base_freq;

    for octave in 0..octaves {
        // Per-octave offsets decorrelate the lattice sampling.
        let ox = octave as f32 * 7.13;
        let oz = octave as f32 * 3.17;
        sum += amp * (value_noise(x * freq + ox, z * freq + oz, seed) * 2.0 - 1.0);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }

    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{roads, sites};

    #[test]
    fn the_playable_area_is_fivefold_with_landscape_past_the_far_plane() {
        let area_ratio = (2.0 * HALF_SIZE / 1536.0).powi(2);
        assert!((4.95..=5.1).contains(&area_ratio));
        let horizon = HORIZON_TILE_SIZE * HORIZON_TILE_RADIUS as f32;
        assert!(horizon - HALF_SIZE > 2700.0 + 20.0);
    }

    #[test]
    fn terrain_is_deterministic() {
        let terrain = Terrain::new();
        let a = terrain.height_at(12.3, -45.6);
        let b = terrain.height_at(12.3, -45.6);
        assert_eq!(a, b);
    }

    #[test]
    fn value_noise_is_in_unit_range() {
        for i in 0..100 {
            let v = value_noise(i as f32 * 0.7, -i as f32 * 0.3, SEED);
            assert!((0.0..1.0).contains(&v), "value {v} out of range");
        }
    }

    #[test]
    fn quarry_floor_is_deeper_than_rim() {
        // Just inside the quarry wall vs. just outside it, before the dioramas
        // in it lay their own ground.
        let floor = landform_height(QUARRY_CENTER.0, QUARRY_CENTER.1 - 0.64 * QUARRY_RADII.1);
        let rim = landform_height(QUARRY_CENTER.0, QUARRY_CENTER.1 - 1.05 * QUARRY_RADII.1);
        assert!(floor < rim - 5.0, "floor {floor} vs rim {rim}");
    }

    #[test]
    fn road_is_flatter_than_its_surroundings() {
        let terrain = Terrain::new();
        let mid = (577.2, 78.0); // on the highway's centerline
        let off = (577.2, 40.0); // well off the road
        let normal_on = terrain.normal_at(mid.0, mid.1);
        let normal_off = terrain.normal_at(off.0, off.1);
        let slope_on = normal_on.xz().length() / normal_on.y;
        let slope_off = normal_off.xz().length() / normal_off.y;
        assert!(
            slope_on < slope_off * 0.5,
            "road slope {slope_on} vs {slope_off}"
        );
    }

    #[test]
    fn road_entrance_meets_the_surrounding_hills() {
        let terrain = Terrain::new();
        let road_height = terrain.height_at(39.0, -38.0);
        let hill_height = landform_height(39.0, -38.0);
        assert!(
            (road_height - hill_height).abs() < 0.1,
            "road entrance should not form a raised platform"
        );
    }

    #[test]
    fn adjacent_chunks_share_positions_normals_and_surface_weights() {
        let terrain = Terrain::new();
        let (mesh, _) = terrain.build_grid(TerrainGrid::detail(IVec2::new(-1, 2)));
        let (neighbor, _) = terrain.build_grid(TerrainGrid::detail(IVec2::new(0, 2)));
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("positions present");
        assert_eq!(positions.len(), (CELLS + 1) * (CELLS + 1));
        assert_eq!(
            mesh.indices().expect("shared grid indices").len(),
            CELLS * CELLS * 6
        );
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(normals)) =
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("smooth normals present");
        };
        for normal in normals {
            let normal = Vec3::from_array(*normal);
            assert!((normal.length() - 1.0).abs() < 1e-5);
            assert!(normal.y > 0.0);
        }

        let n = CELLS + 1;
        use bevy::mesh::VertexAttributeValues::{Float32x3, Float32x4};
        for attribute in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_COLOR,
        ] {
            match (mesh.attribute(attribute), neighbor.attribute(attribute)) {
                (Some(Float32x3(left)), Some(Float32x3(right))) => {
                    for z in 0..n {
                        assert_eq!(left[z * n + CELLS], right[z * n]);
                    }
                }
                (Some(Float32x4(left)), Some(Float32x4(right))) => {
                    for z in 0..n {
                        assert_eq!(left[z * n + CELLS], right[z * n]);
                    }
                }
                _ => panic!("missing terrain seam attributes"),
            }
        }
    }

    #[test]
    fn mixed_resolution_terrain_and_water_share_their_edge() {
        use bevy::mesh::VertexAttributeValues::Float32x3;

        let terrain = Terrain::new();
        for (finer, coarser) in [
            (
                TerrainGrid::detail(IVec2::new(-14, 2)),
                TerrainGrid::distant(IVec2::new(-15, 2)),
            ),
            (
                TerrainGrid::distant(IVec2::new(-16, 2)),
                TerrainGrid::horizon(IVec2::new(-5, 0)),
            ),
        ] {
            let (finer_mesh, finer_water) = terrain.build_grid(finer);
            let (coarser_mesh, coarser_water) = terrain.build_grid(coarser);
            let (Some(Float32x3(finer_pos)), Some(Float32x3(coarser_pos))) = (
                finer_mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                coarser_mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            ) else {
                panic!("both grids have positions");
            };
            for row in 0..=finer.cells {
                let i = row * (finer.cells + 1);
                let [x, y, z] = finer_pos[i];
                let skip = (z - coarser.origin.y) / coarser.spacing;
                let lower = skip.floor() as usize;
                let upper = skip.ceil() as usize;
                let at_edge = |j| j * (coarser.cells + 1) + coarser.cells;
                let a = at_edge(lower);
                let b = at_edge(upper);
                let t = skip.fract();
                assert_eq!(x, coarser_pos[a][0]);
                assert!((y - lerp(coarser_pos[a][1], coarser_pos[b][1], t)).abs() < 1e-5);
                let depth = lerp(coarser_water[a].depth, coarser_water[b].depth, t);
                assert!((finer_water[i].depth - depth).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn mountain_range_rises_above_the_traversable_lowlands() {
        let terrain = Terrain::new();
        for (summit, _, height) in MOUNTAINS {
            assert!(terrain.height_at(summit.x, summit.y) > height * 0.6);
        }
        assert!(terrain.height_at(SPAWN.0, SPAWN.1).abs() < 5.0);
        assert!(terrain.height_at(35.0, 340.0) < 10.0);
    }

    #[test]
    fn terrain_normals_are_continuous_across_noise_and_quarry_boundaries() {
        let terrain = Terrain::new();
        for (x, z) in [
            (48.0, -48.0),
            (0.0, 0.0),
            (BALD_CENTER.0, BALD_CENTER.1),
            (QUARRY_CENTER.0 + QUARRY_RADII.0 * 1.08, QUARRY_CENTER.1),
            (QUARRY_CENTER.0 + QUARRY_RADII.0 * 0.64, QUARRY_CENTER.1),
        ] {
            for offset in [Vec2::X, Vec2::Y] {
                let before = terrain.normal_at(x - offset.x * 0.01, z - offset.y * 0.01);
                let after = terrain.normal_at(x + offset.x * 0.01, z + offset.y * 0.01);
                assert!(before.dot(after) > 0.999, "normal seam near ({x}, {z})");
            }
        }
    }

    #[test]
    fn surface_layers_blend_continuously_with_slope_and_elevation() {
        let terrain = Terrain::new();
        for slope in [0.35, 0.55, 0.85] {
            let wet = Wetlands::at(100.0, -100.0);
            let before = terrain.surface_weights_at(100.0, -100.0, 1.499, slope - 0.001, &wet);
            let after = terrain.surface_weights_at(100.0, -100.0, 1.501, slope + 0.001, &wet);
            for channel in 0..4 {
                assert!((before[channel] - after[channel]).abs() < 0.01);
            }
        }
    }

    #[test]
    fn base_stands_on_a_level_pad_clear_of_trees() {
        let terrain = Terrain::new();
        let level = terrain.height_at(BASE.0, BASE.1);
        for (dx, dz) in [(-17.5, -19.5), (17.5, 19.5), (0.0, 22.0), (-10.0, 12.0)] {
            let h = terrain.height_at(BASE.0 + dx, BASE.1 + dz);
            assert!((h - level).abs() < 1e-4, "pad not level at ({dx}, {dz})");
            assert_eq!(terrain.woodland_at(BASE.0 + dx, BASE.1 + dz), 0.0);
        }
    }

    /// The quarry's haul road in the pit's own ground, with its graded level.
    fn ramp_in_the_diorama(terrain: &Terrain) -> Vec<(Vec2, f32, f32)> {
        sites::quarry_ramp()
            .into_iter()
            .filter_map(|at| {
                let ground = sites::diorama_ground(at)?;
                let level = terrain.roads.sample(at).expect("the ramp is a road").height;
                Some((at, ground, level))
            })
            .collect()
    }

    #[test]
    fn the_ramp_cuts_into_the_quarry() {
        let terrain = Terrain::new();
        let inside = ramp_in_the_diorama(&terrain);
        assert!(inside.len() > 50, "the ramp lies in the diorama");
        for (at, _, level) in inside {
            let height = terrain.mesh_height_at(at);
            assert!(
                (height - level).abs() < 0.3,
                "ground {height} vs ramp {level} at {at}"
            );
        }
    }

    #[test]
    fn the_ramp_grade_stays_drivable() {
        // The centerline holds the ramp's grade; on a bend the inner lane is
        // shorter by R / (R - lane), and the 2 m mesh adds to it.
        // The quarry's road level is exactly 0.191, the Golden Sphere's 0.118;
        // the render mesh strays up to 0.11 m from it in a bend.
        const CENTERLINE: f32 = 0.25;
        const LANES: f32 = 0.35;
        let terrain = Terrain::new();
        let mut worst = (0.0, 0.0, Vec2::ZERO);
        for path in [sites::quarry_ramp(), roads::golden_pit_ramp()] {
            for &[a, b] in path.array_windows() {
                let steps = a.distance(b).ceil() as usize;
                let step_length = a.distance(b) / steps as f32;
                let across = (b - a).normalize().perp();
                for lane in [-1.8, 0.0, 1.8] {
                    let limit = if lane == 0.0 { CENTERLINE } else { LANES };
                    let mut last = terrain.mesh_height_at(a + across * lane);
                    for step in 1..=steps {
                        let at = a.lerp(b, step as f32 / steps as f32) + across * lane;
                        let height = terrain.mesh_height_at(at);
                        let excess = (height - last).abs() / step_length - limit;
                        if excess > worst.0 {
                            worst = (excess, limit, at);
                        }
                        last = height;
                    }
                }
            }
        }
        let (excess, limit, at) = worst;
        assert!(
            excess <= 0.0,
            "grade over its limit of {limit} by {excess} at {at}"
        );
    }

    #[test]
    fn the_golden_ramp_cuts_and_fills_within_its_limits() {
        // The planner measured 4 m of cut and 5 m of fill (at most) against its
        // 96 x 96 grid of the land; against the land itself, a little more
        // than the grid's resolution allows.
        const CUT: f32 = 4.3;
        const FILL: f32 = 5.3;
        let terrain = Terrain::new();
        let (mut cut, mut fill) = ((0.0, Vec2::ZERO), (0.0, Vec2::ZERO));
        // The entrance ends the drive, which follows the land.
        for &at in &roads::golden_pit_ramp()[1..] {
            let level = terrain.roads.sample(at).expect("the ramp is a road").height;
            let rise = level - sited_height(at.x, at.y);
            if -rise > cut.0 {
                cut = (-rise, at);
            }
            if rise > fill.0 {
                fill = (rise, at);
            }
        }
        assert!(cut.0 <= CUT, "{} m of cut at {}", cut.0, cut.1);
        assert!(fill.0 <= FILL, "{} m of fill at {}", fill.0, fill.1);
    }

    #[test]
    fn the_ramp_crest_is_rounded() {
        // A 24 m vertical curve over the 0.27 grade change at the entrance;
        // a sharper break launches a car at full throttle.
        const CURVATURE: f32 = 0.27 / 24.0 * 1.1;
        let terrain = Terrain::new();
        let path = sites::quarry_ramp();
        let entrance = path[0].distance(path[1]);
        // The road's level at each metre along the track, 20 m either side of the entrance.
        let mut levels = Vec::new();
        let mut start = 0.0;
        for &[a, b] in path.array_windows() {
            let leg = a.distance(b);
            let mut run = (entrance - 20.0).max(start).ceil();
            while run < start + leg && run <= entrance + 20.0 {
                let at = a.lerp(b, (run - start) / leg);
                levels.push(terrain.roads.sample(at).expect("a road").height);
                run += 1.0;
            }
            start += leg;
        }
        assert!(levels.len() >= 40, "{} samples", levels.len());
        for (i, window) in levels.array_windows::<3>().enumerate() {
            let change = (window[2] - 2.0 * window[1] + window[0]).abs();
            assert!(
                change <= CURVATURE,
                "grade changes by {change} per metre at sample {}",
                i + 1
            );
        }
    }

    #[test]
    fn the_ramp_ends_on_the_floor() {
        let terrain = Terrain::new();
        let (end, floor, _) = *ramp_in_the_diorama(&terrain)
            .last()
            .expect("the ramp lies in the diorama");
        let height = terrain.mesh_height_at(end);
        assert!(
            (height - floor).abs() < 1.0,
            "ramp end {height} vs floor {floor} at {end}"
        );
    }

    #[test]
    fn the_ramp_stays_near_the_scan() {
        let terrain = Terrain::new();
        for (at, ground, level) in ramp_in_the_diorama(&terrain) {
            // Raw relief has clefts a ramp may span or cut, which the scan's
            // cutaway hides. The drive's 7 m before the front edge already
            // lower the ramp by 1.4 m there, deepening its cuts.
            assert!(
                (-5.0..=8.0).contains(&(level - ground)),
                "ramp {level} vs scan {ground} at {at}"
            );
        }
    }

    /// Lines across the quarry's gate, a metre apart from 5 m before its
    /// entrance to 6 m past it, where the drive and the ramp's first leg run
    /// straight; on each, points every 0.25 m out to `half_width` either side.
    fn lines_across_the_gate(half_width: f32) -> Vec<Vec<Vec2>> {
        let site = sites::LANDMARKS
            .iter()
            .find(|site| site.junction == roads::QUARRY)
            .expect("the quarry is a landmark");
        let entrance = site.entrance();
        let axis = (entrance - site.junction).normalize();
        let steps = (half_width / 0.25) as i32;
        (-5..=6)
            .map(|metre| {
                let line = entrance + axis * metre as f32;
                (-steps..=steps)
                    .map(|step| line + axis.perp() * step as f32 * 0.25)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_quarry_gate_road_has_no_step_across_it() {
        let terrain = Terrain::new();
        for line in lines_across_the_gate(roads::RAMP_HALF_WIDTH) {
            let heights: Vec<f32> = line
                .iter()
                .map(|&at| {
                    terrain
                        .roads
                        .sample(at)
                        .expect("the gate is on the road")
                        .height
                })
                .collect();
            for (&[a, b], at) in heights.array_windows().zip(&line[1..]) {
                assert!((a - b).abs() <= 0.1, "road height steps {a} to {b} at {at}");
            }
        }
    }

    #[test]
    fn the_quarry_gate_ground_is_level_across_it() {
        // The axis is straight there, so each line across it lies at one level,
        // give or take the 2 m mesh.
        let terrain = Terrain::new();
        for line in lines_across_the_gate(3.0) {
            let (low, high) = line
                .iter()
                .map(|&at| terrain.mesh_height_at(at))
                .fold((f32::MAX, f32::MIN), |(low, high), h| {
                    (low.min(h), high.max(h))
                });
            assert!(
                high - low <= 0.05,
                "ground spans {low} to {high} across the line at {}",
                line[0]
            );
        }
    }

    /// Exports the Golden Sphere's pit, before roads and water, as the relief
    /// `tools/plan_quarry_ramp.py` plans its haul road over (`--scale 1`), and
    /// prints the boxes the road keeps clear of (`--avoid`).
    #[test]
    #[ignore = "writes src/world/pillars/golden_pit_ground.rs"]
    fn write_golden_pit_relief() {
        use std::fmt::Write;

        const SIZE: usize = 96;
        const MIN: Vec2 = Vec2::new(-1260.0, -330.0);
        const MAX: Vec2 = Vec2::new(-920.0, 130.0);
        let cell = (MAX - MIN) / SIZE as f32;

        let mut text = String::from(
            "// Generated by `cargo test write_golden_pit_relief -- --ignored`; do not edit.\n",
        );
        text += "Relief {\n";
        text += &format!("    min: Vec2::new({:.4}, {:.4}),\n", MIN.x, MIN.y);
        text += &format!("    cell: Vec2::new({:.6}, {:.6}),\n", cell.x, cell.y);
        text += "    heights: &[\n";
        for row in 0..SIZE {
            text += "        ";
            for col in 0..SIZE {
                let at = MIN + (Vec2::new(col as f32, row as f32) + 0.5) * cell;
                write!(text, "{:.3}, ", sited_height(at.x, at.y)).expect("writing to a String");
            }
            text = text.trim_end().to_owned() + "\n";
        }
        text += "    ],\n}\n";
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/world/pillars/golden_pit_ground.rs"
        );
        std::fs::write(path, text).expect("the pillars folder is writable");

        let meets = |(low, high): (Vec2, Vec2)| low.cmple(MAX).all() && high.cmpge(MIN).all();
        let avoid = |(low, high): (Vec2, Vec2)| {
            format!(
                "--avoid={:.1},{:.1},{:.1},{:.1}",
                low.x, low.y, high.x, high.y
            )
        };
        for (name, (low, high), margin) in sites::diorama_footprints() {
            let grown = (low - margin, high + margin);
            if meets(grown) {
                println!("{name}: footprint {}", avoid((low, high)));
                println!("{name}: grown by {margin} m {}", avoid(grown));
            }
        }
    }
}
