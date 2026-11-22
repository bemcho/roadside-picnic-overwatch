//! Graded landmark tracks and their shared visual and scenery masks.

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::cordon::{BREACH, GATE, PERIMETER_GATE};
use super::hydrology::{EAST_FORD_X, FORD_X, WEST_FORD_X};
use super::sites::LANDMARKS;
use super::terrain::{BASE, TILE_SIZE, lerp, smoothstep};

const HALF_WIDTH: f32 = 8.5;
const SHOULDER: f32 = 11.0;

// Junctions shared by several routes.
const YARD_TURN: Vec2 = Vec2::new(39.0, -38.0);
const FACTORY_FORK: Vec2 = Vec2::new(-320.0, -72.0);
const NORTH_FORK: Vec2 = Vec2::new(-350.0, 215.0);
const RAIL_CROSSING: Vec2 = Vec2::new(-700.0, 40.0);
const MARSH_FORK: Vec2 = Vec2::new(-860.0, -150.0);
const HARMONT_FORK: Vec2 = Vec2::new(380.0, -60.0);
const SUBURBS_FORK: Vec2 = Vec2::new(-220.0, -480.0);
const RAIL_DITCH: Vec2 = Vec2::new(-640.0, -450.0);

// Where each place's drive leaves the road.
pub(super) const INSTITUTE: Vec2 = Vec2::new(-10.0, -38.0);
pub(super) const SCHOOL: Vec2 = Vec2::new(260.0, -40.0);
pub(super) const PLAGUE: Vec2 = Vec2::new(480.0, -80.0);
pub(super) const HOUSES: Vec2 = Vec2::new(600.0, -80.0);
pub(super) const HARMONT_WEST: Vec2 = Vec2::new(680.0, -140.0);
pub(super) const CEMETERY: Vec2 = Vec2::new(700.0, -240.0);
pub(super) const BORSCHT: Vec2 = Vec2::new(510.0, -900.0);
pub(super) const TAVERN: Vec2 = Vec2::new(510.0, -950.0);
pub(super) const TOWN: Vec2 = Vec2::new(710.0, 470.0);
pub(super) const MOTOR_POOL: Vec2 = Vec2::new(-185.0, -50.0);
pub(super) const HIGHWAY: Vec2 = Vec2::new(-390.0, -82.0);
pub(super) const SUBSTATION: Vec2 = Vec2::new(-345.0, 100.0);
pub(super) const BRICKWORKS: Vec2 = Vec2::new(-350.0, 150.0);
pub(super) const FACTORY: Vec2 = Vec2::new(-413.0, 215.0);
pub(super) const SILOS: Vec2 = Vec2::new(-510.0, 225.0);
pub(super) const NORTH_SUBURBS: Vec2 = Vec2::new(-350.0, 690.0);
pub(super) const GARAGE: Vec2 = Vec2::new(-606.0, 85.0);
pub(super) const SUBURBS: Vec2 = Vec2::new(-240.0, -640.0);
pub(super) const DITCHES: Vec2 = Vec2::new(-520.0, -430.0);
pub(super) const RAIL_NORTH: Vec2 = Vec2::new(-735.0, 250.0);
pub(super) const LOCOMOTIVE: Vec2 = Vec2::new(-780.0, 560.0);
pub(super) const RAIL_FAR_NORTH: Vec2 = Vec2::new(-820.0, 792.0);
pub(super) const RAIL_DERAILMENT: Vec2 = Vec2::new(-660.0, -330.0);
pub(super) const MARSH: Vec2 = Vec2::new(-610.0, -660.0);
pub(super) const RAIL_SOUTH: Vec2 = Vec2::new(-545.0, -1100.0);
pub(super) const SWAMP: Vec2 = Vec2::new(-880.0, -590.0);
pub(super) const CRATER: Vec2 = Vec2::new(-980.0, -660.0);
pub(super) const RIDGE: Vec2 = Vec2::new(-990.0, -130.0);
pub(super) const QUARRY: Vec2 = Vec2::new(-1105.0, -245.0);
pub(super) const GOLDEN_SPHERE: Vec2 = Vec2::new(-1280.0, -300.0);

/// The tracks of the Harmont map. Each route after the first starts on an
/// earlier one.
const ROUTES: &[&[Vec2]] = &[
    // Chapter 1: out of the Institute yard, through the official gate in the
    // Perimeter, along the broken asphalt highway to the Garage; then over the
    // railway, around the marsh and over the ridge to the quarry.
    &[
        Vec2::new(BASE.0, BASE.1 + 18.0),
        GATE,
        YARD_TURN,
        INSTITUTE,
        PERIMETER_GATE,
        Vec2::new(-130.0, -42.0),
        MOTOR_POOL,
        Vec2::new(-250.0, -62.0),
        FACTORY_FORK,
        HIGHWAY,
        Vec2::new(-480.0, -60.0),
        Vec2::new(-550.0, 0.0),
        GARAGE,
        RAIL_CROSSING,
        Vec2::new(-780.0, -40.0),
        MARSH_FORK,
        Vec2::new(-930.0, -170.0),
        RIDGE,
        Vec2::new(-1040.0, -200.0),
        QUARRY,
        Vec2::new(-1190.0, -300.0),
        GOLDEN_SPHERE,
    ],
    // Harmont's main street, away from the Perimeter.
    &[
        YARD_TURN,
        Vec2::new(140.0, -38.0),
        SCHOOL,
        HARMONT_FORK,
        PLAGUE,
        HOUSES,
        HARMONT_WEST,
        CEMETERY,
    ],
    // South past the Plague Quarter to the Borscht at the edge of town.
    &[
        PLAGUE,
        Vec2::new(440.0, -250.0),
        Vec2::new(450.0, -450.0),
        Vec2::new(480.0, -650.0),
        Vec2::new(510.0, -800.0),
        BORSCHT,
        TAVERN,
    ],
    // Chapter 4: from the Borscht through the illegal breach, along the
    // drainage ditches to the railway.
    &[
        BORSCHT,
        Vec2::new(380.0, -830.0),
        Vec2::new(210.0, -700.0),
        Vec2::new(50.0, -580.0),
        Vec2::new(-30.0, -515.0),
        BREACH,
        Vec2::new(-120.0, -490.0),
        SUBURBS_FORK,
        Vec2::new(-370.0, -430.0),
        DITCHES,
        RAIL_DITCH,
    ],
    &[SUBURBS_FORK, Vec2::new(-230.0, -560.0), SUBURBS],
    // North over the river ford to New Harmont.
    &[
        HARMONT_FORK,
        Vec2::new(460.0, 60.0),
        Vec2::new(580.0, 140.0),
        Vec2::new(WEST_FORD_X, 190.0),
        Vec2::new(WEST_FORD_X, 410.0),
        TOWN,
    ],
    // The industrial ruins north of the highway.
    &[
        FACTORY_FORK,
        Vec2::new(-335.0, 30.0),
        SUBSTATION,
        BRICKWORKS,
        NORTH_FORK,
        FACTORY,
        SILOS,
    ],
    // From the silos over the river ford to the northern suburbs.
    &[
        SILOS,
        Vec2::new(FORD_X, 265.0),
        Vec2::new(FORD_X, 470.0),
        Vec2::new(-440.0, 610.0),
        NORTH_SUBURBS,
    ],
    // The service track along the railway, north over its own ford.
    &[
        RAIL_CROSSING,
        Vec2::new(-725.0, 150.0),
        RAIL_NORTH,
        Vec2::new(-755.0, 320.0),
        Vec2::new(EAST_FORD_X, 365.0),
        Vec2::new(-772.0, 430.0),
        LOCOMOTIVE,
        Vec2::new(-800.0, 690.0),
        RAIL_FAR_NORTH,
    ],
    // ...and south past the derailment, around the swamp's shore.
    &[
        RAIL_CROSSING,
        Vec2::new(-685.0, -100.0),
        Vec2::new(-675.0, -230.0),
        RAIL_DERAILMENT,
        RAIL_DITCH,
        Vec2::new(-610.0, -560.0),
        MARSH,
        Vec2::new(-520.0, -760.0),
        Vec2::new(-470.0, -900.0),
        Vec2::new(-500.0, -1000.0),
        Vec2::new(-550.0, -1050.0),
        RAIL_SOUTH,
    ],
    // Down the marsh's north shore to the witches' jelly and the crater.
    &[
        MARSH_FORK,
        Vec2::new(-870.0, -300.0),
        Vec2::new(-880.0, -450.0),
        SWAMP,
        Vec2::new(-930.0, -620.0),
        CRATER,
    ],
];

#[derive(Clone)]
struct Leg {
    start: Vec2,
    along: Vec2,
    length: f32,
    levels: [f32; 2],
    min: Vec2,
    max: Vec2,
}

#[derive(Clone)]
pub(super) struct RoadNetwork {
    tiles: HashMap<IVec2, Vec<Leg>>,
    min: Vec2,
    max: Vec2,
}

pub(super) struct RoadSample {
    distance: f32,
    pub height: f32,
}

impl RoadSample {
    pub fn mask(&self) -> f32 {
        smoothstep(SHOULDER, HALF_WIDTH, self.distance)
    }

    fn surface(&self) -> [u8; 2] {
        let road = smoothstep(7.5, 4.6, self.distance);
        let rut = smoothstep(0.83, 0.32, (self.distance - 1.51).abs());
        [(road * 255.0).round() as u8, (rut * 255.0).round() as u8]
    }
}

impl RoadNetwork {
    pub fn new(level: impl Fn(Vec2) -> f32) -> Self {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        let mut tiles: HashMap<_, Vec<_>> = HashMap::default();
        let drives = LANDMARKS.map(|site| [site.junction, site.entrance()]);
        for path in ROUTES
            .iter()
            .copied()
            .chain(drives.iter().map(|path| path.as_slice()))
        {
            for &[a, b] in path.array_windows() {
                let delta = b - a;
                let length = delta.length();
                let leg = Leg {
                    start: a,
                    along: delta / length,
                    length,
                    levels: [level(a), level(b)],
                    min: a.min(b) - Vec2::splat(SHOULDER),
                    max: a.max(b) + Vec2::splat(SHOULDER),
                };
                min = min.min(leg.min);
                max = max.max(leg.max);
                let first = tile_at(leg.min);
                let last = tile_at(leg.max);
                for z in first.y..=last.y {
                    for x in first.x..=last.x {
                        tiles.entry(IVec2::new(x, z)).or_default().push(leg.clone());
                    }
                }
            }
        }
        Self { tiles, min, max }
    }

    pub fn sample(&self, at: Vec2) -> Option<RoadSample> {
        if at.cmplt(self.min).any() || at.cmpgt(self.max).any() {
            return None;
        }
        Self::sample_legs(self.tiles.get(&tile_at(at))?, at)
    }

    fn sample_legs(legs: &[Leg], at: Vec2) -> Option<RoadSample> {
        let mut nearest = None;
        let mut best = SHOULDER * SHOULDER;
        for leg in legs {
            if at.cmplt(leg.min).any() || at.cmpgt(leg.max).any() {
                continue;
            }
            let run = (at - leg.start).dot(leg.along).clamp(0.0, leg.length);
            let distance_squared = at.distance_squared(leg.start + leg.along * run);
            if distance_squared < best {
                nearest = Some((leg, run / leg.length));
                best = distance_squared;
            }
        }
        nearest.map(|(leg, t)| RoadSample {
            distance: best.sqrt(),
            height: lerp(leg.levels[0], leg.levels[1], t),
        })
    }

    pub fn surface_texture(&self) -> (Image, Vec4) {
        // A world-space mask keeps the same road width across coarse and fine terrain tiles.
        const TEXELS_PER_METRE: f32 = 1.5;
        let origin = self.min.floor();
        let size = ((self.max.ceil() - origin) * TEXELS_PER_METRE).as_uvec2();
        let mut pixels = vec![0; (size.x * size.y * 2) as usize];
        for (&tile, legs) in &self.tiles {
            let first = tile.as_vec2() * TILE_SIZE;
            let pixel = |at: Vec2| ((at - origin) * TEXELS_PER_METRE - 0.5).ceil().as_ivec2();
            let low = pixel(first).max(IVec2::ZERO);
            let high = pixel(first + TILE_SIZE).min(size.as_ivec2());
            for z in low.y..high.y {
                for x in low.x..high.x {
                    let at = origin + (Vec2::new(x as f32, z as f32) + 0.5) / TEXELS_PER_METRE;
                    if let Some(road) = Self::sample_legs(legs, at) {
                        let i = (z as usize * size.x as usize + x as usize) * 2;
                        pixels[i..i + 2].copy_from_slice(&road.surface());
                    }
                }
            }
        }
        let image = Image::new(
            Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rg8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        let uv_scale = Vec2::splat(TEXELS_PER_METRE) / size.as_vec2();
        (image, Vec4::new(origin.x, origin.y, uv_scale.x, uv_scale.y))
    }
}

fn tile_at(point: Vec2) -> IVec2 {
    (point / TILE_SIZE).floor().as_ivec2()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Terrain;

    #[test]
    fn every_landmark_track_has_a_continuous_drivable_route_back_to_the_gate() {
        let terrain = Terrain::new();
        assert!(ROUTES[0].contains(&GATE));
        let drives = LANDMARKS.map(|site| [site.junction, site.entrance()]);
        let mut bad = Vec::new();
        for (i, path) in ROUTES
            .iter()
            .copied()
            .chain(drives.iter().map(|path| path.as_slice()))
            .enumerate()
        {
            if i > 0 {
                assert!(
                    ROUTES[..i.min(ROUTES.len())]
                        .iter()
                        .any(|road| road.contains(&path[0]))
                );
            }
            for &[a, b] in path.array_windows() {
                let mut worst_grade = (0.0, a);
                let mut deepest = (0.0, a);
                let steps = a.distance(b).ceil() as usize;
                let across = (b - a).normalize().perp();
                for lane in [-2.4, 0.0, 2.4] {
                    let offset = across * lane;
                    let mut last = terrain.mesh_height_at(a + offset);
                    for step in 0..=steps {
                        let at = a.lerp(b, step as f32 / steps as f32) + offset;
                        assert!(terrain.road_mask(at.x, at.y) > 0.99);
                        let height = terrain.mesh_height_at(at);
                        let grade = (height - last).abs() * steps as f32 / a.distance(b);
                        if grade > worst_grade.0 {
                            worst_grade = (grade, at);
                        }
                        let depth = terrain.water_depth_at(at.x, at.y);
                        if depth > deepest.0 {
                            deepest = (depth, at);
                        }
                        last = height;
                    }
                }
                if worst_grade.0 > 0.57 || deepest.0 > 0.53 {
                    bad.push((i, worst_grade, deepest));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "route, grade and position, depth and position: {bad:#?}"
        );
    }

    #[test]
    fn the_material_mask_draws_a_lane_and_two_ruts_without_painting_the_yard() {
        let terrain = Terrain::new();
        let (image, bounds) = terrain.roads.surface_texture();
        let read = |at: Vec2| {
            let uv = (at - bounds.xy()) * bounds.zw();
            let texel = (uv * Vec2::new(image.width() as f32, image.height() as f32))
                .floor()
                .as_uvec2();
            let i = ((texel.y * image.width() + texel.x) * 2) as usize;
            let bytes = image.data.as_ref().unwrap();
            (bytes[i], bytes[i + 1])
        };
        assert_eq!(read(GATE), (255, 0));
        assert_eq!(read(GATE + Vec2::new(-10.0, -4.0)), (0, 0));
        assert_eq!(read(Vec2::new(FORD_X, 300.0)), (255, 0));
        for side in [-1.0, 1.0] {
            assert!(read(Vec2::new(FORD_X + side * 1.5, 300.0)).1 >= 190);
            assert_eq!(read(Vec2::new(FORD_X + side * 8.0, 300.0)), (0, 0));
        }
        for &path in ROUTES {
            assert!(path.iter().all(|&at| read(at).0 == 255));
        }
    }
}
