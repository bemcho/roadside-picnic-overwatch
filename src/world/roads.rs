//! Graded landmark tracks and their shared visual and scenery masks.

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::cordon::{BREACH, GATE, PERIMETER_GATE};
use super::map::{RAILWAY, map};
use super::sites::{self, LANDMARKS};
use super::terrain::{BASE, TILE_SIZE, lerp, smoothstep};
use super::town;

const HALF_WIDTH: f32 = 8.5;
/// Where a road's graded shoulder meets the land, from its middle line.
pub(super) const SHOULDER: f32 = 11.0;
/// The railway's gravel bed, narrower than a road.
const RAIL_HALF_WIDTH: f32 = 3.2;
const RAIL_SHOULDER: f32 = 6.0;
/// The quarry's haul road, a dirt track cut into the pit's wall.
pub(super) const RAMP_HALF_WIDTH: f32 = 4.5;
pub(super) const RAMP_SHOULDER: f32 = 9.0;
/// The longest graded run of the railway bed between sampled levels.
const RAIL_RUN: f32 = 48.0;

// Junctions shared by several routes, in map pixels (`doc/Roadside_map.jpg`).
const YARD_TURN: Vec2 = Vec2::new(GATE.x, 78.0);
const AVENUE: Vec2 = Vec2::new(GATE.x, town::PLAZA.y);
const HARMONT_FORK: Vec2 = map(322.0, 371.0);
const PLAGUE_FORK: Vec2 = map(300.0, 440.0);
const BORSCHT_LANE: Vec2 = map(262.0, 592.0);
const RED_LINK: Vec2 = map(727.0, 350.0);
const RED_TURN: Vec2 = map(748.0, 432.0);
/// Where the Chapter 4 track crosses the railway (a point of both).
pub(super) const RAIL_CROSSING: Vec2 = RAILWAY[5];
const NORTH_SUBURBS_FORK: Vec2 = map(532.0, 341.0);
const SOUTH_SUBURBS_FORK: Vec2 = map(560.0, 490.0);
const INDUSTRY_FORK: Vec2 = map(637.0, 359.0);

// Where each place's drive leaves the road.
pub(super) const INSTITUTE: Vec2 = Vec2::new(GATE.x, 470.0);
pub(super) const MOTOR_POOL: Vec2 = Vec2::new(GATE.x, 199.0);
pub(super) const OLD_BORSCHT: Vec2 = map(277.0, 566.0);
pub(super) const BORSCHT: Vec2 = map(232.0, 618.0);
pub(super) const CEMETERY: Vec2 = map(205.0, 642.0);
pub(super) const SCHOOL: Vec2 = map(250.0, 440.0);
pub(super) const PLAGUE: Vec2 = map(195.0, 440.0);
pub(super) const HOUSES: Vec2 = map(150.0, 440.0);
pub(super) const HIGHWAY: Vec2 = map(607.0, 348.0);
pub(super) const GARAGE: Vec2 = map(787.0, 321.0);
pub(super) const NORTH_SUBURBS: Vec2 = map(535.0, 250.0);
pub(super) const NORTH_HOUSES: Vec2 = NORTH_SUBURBS_FORK;
pub(super) const SUBURBS: Vec2 = map(555.0, 540.0);
pub(super) const SUBURBS_CEMETERY: Vec2 = SOUTH_SUBURBS_FORK;
pub(super) const SILOS: Vec2 = map(645.0, 318.0);
pub(super) const FACTORY: Vec2 = map(658.0, 298.0);
pub(super) const BRICKWORKS: Vec2 = map(698.0, 275.0);
pub(super) const SUBSTATION: Vec2 = map(728.0, 290.0);
pub(super) const MARSH: Vec2 = map(877.0, 433.0);
pub(super) const EMBANKMENT: Vec2 = map(907.0, 449.0);
pub(super) const RIDGE: Vec2 = map(997.0, 395.0);
pub(super) const QUARRY: Vec2 = map(1095.0, 320.0);
pub(super) const GOLDEN_SPHERE: Vec2 = map(1185.0, 348.0);
/// Where the Chapter 4 road meets the Golden Sphere's pit ramp, on the rim
/// (world metres, not map pixels): the ramp's drive starts here.
pub(super) const GOLDEN_RIM: Vec2 = Vec2::new(-980.2, -77.8);
pub(super) const SWAMP: Vec2 = map(775.0, 690.0);
pub(super) const CRATER: Vec2 = map(880.0, 700.0);
// On the railway itself.
pub(super) const RAIL_SOUTH: Vec2 = RAILWAY[2];
pub(super) const RAIL_NORTH: Vec2 = RAILWAY[7];
pub(super) const LOCOMOTIVE: Vec2 = RAILWAY[10];

/// How a track is surfaced: its look and the width of its bed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
    /// Graded earth with tire ruts.
    Dirt,
    /// The old, broken asphalt of Harmont's roads and the highway to the
    /// Garage, painted by the terrain shader from the mask's old-asphalt share.
    Asphalt,
    /// New Harmont's own paved streets.
    Street,
    /// The railway's gravel bed.
    Ballast,
    /// A pit's dirt haul road, the quarry's or the Golden Sphere's. The drive
    /// follows the land; the ramp is graded evenly from the entrance, with a
    /// rounded crest there.
    Ramp,
}

impl Surface {
    /// Half-widths of the full-strength bed and of its shoulder.
    fn widths(self) -> (f32, f32) {
        match self {
            Surface::Ballast => (RAIL_HALF_WIDTH, RAIL_SHOULDER),
            Surface::Ramp => (RAMP_HALF_WIDTH, RAMP_SHOULDER),
            Surface::Dirt | Surface::Asphalt | Surface::Street => (HALF_WIDTH, SHOULDER),
        }
    }
}

struct Route {
    surface: Surface,
    points: &'static [Vec2],
}

const fn asphalt(points: &'static [Vec2]) -> Route {
    Route {
        surface: Surface::Asphalt,
        points,
    }
}

const fn street(points: &'static [Vec2]) -> Route {
    Route {
        surface: Surface::Street,
        points,
    }
}

const fn dirt(points: &'static [Vec2]) -> Route {
    Route {
        surface: Surface::Dirt,
        points,
    }
}

/// Harmont's main road, south along the Perimeter to the old Borscht.
const MAIN_ROAD: &[Vec2] = &[
    YARD_TURN,
    map(337.0, 362.0),
    HARMONT_FORK,
    map(307.0, 408.0),
    PLAGUE_FORK,
    map(292.0, 502.0),
    OLD_BORSCHT,
];

/// The tracks of the Harmont map (`doc/Roadside_map.jpg`). Each route after
/// the first starts on an earlier one.
const ROUTES: &[Route] = &[
    // Chapter 1: out of the hangar yard, through the Institute gate in the
    // Perimeter and along the broken asphalt highway to the Garage.
    asphalt(&[
        Vec2::new(BASE.0, BASE.1 + 18.0),
        GATE,
        YARD_TURN,
        map(397.0, 348.0),
        PERIMETER_GATE,
        map(442.0, 353.0),
        map(487.0, 348.0),
        NORTH_SUBURBS_FORK,
        map(562.0, 342.0),
        HIGHWAY,
        INDUSTRY_FORK,
        map(667.0, 373.0),
        map(697.0, 362.0),
        RED_LINK,
        map(757.0, 333.0),
        GARAGE,
    ]),
    asphalt(MAIN_ROAD),
    // North past the motor pool to the Institute, and the avenue through
    // New Harmont's plaza.
    asphalt(&[YARD_TURN, MOTOR_POOL, AVENUE, INSTITUTE]),
    street(&[AVENUE, town::AVENUE[0], town::PLAZA, town::AVENUE[1]]),
    // The boulevard and the ring street around the town's blocks.
    street(town::STREETS[0]),
    street(town::STREETS[1]),
    street(town::STREETS[2]),
    street(town::STREETS[3]),
    asphalt(&[PLAGUE_FORK, SCHOOL, PLAGUE, HOUSES]),
    dirt(&[OLD_BORSCHT, BORSCHT_LANE, BORSCHT, CEMETERY]),
    // Chapter 4: from the Borscht through the illegal breach, over the
    // railway, along the marsh and through the pass to the quarry.
    dirt(&[
        BORSCHT_LANE,
        map(300.0, 585.0),
        map(330.0, 560.0),
        map(365.0, 535.0),
        map(398.0, 510.0),
        BREACH,
        map(425.0, 509.0),
        map(480.0, 505.0),
        SOUTH_SUBURBS_FORK,
        map(640.0, 470.0),
        map(700.0, 455.0),
        RED_TURN,
        RAIL_CROSSING,
        map(787.0, 449.0),
        map(802.0, 450.0),
        map(832.0, 431.0),
        map(847.0, 426.0),
        MARSH,
        EMBANKMENT,
        map(937.0, 476.0),
        map(967.0, 478.0),
        map(982.0, 439.0),
        RIDGE,
        map(1012.0, 356.0),
        map(1027.0, 334.0),
        map(1057.0, 318.0),
        QUARRY,
        map(1100.0, 368.0),
        // Along the pit's rim, on the low ridge, to where its ramp starts.
        Vec2::new(-962.0, 10.0),
        Vec2::new(-966.0, -20.0),
        Vec2::new(-972.0, -48.0),
        GOLDEN_RIM,
    ]),
    // The short track between the highway's end and the Chapter 4 route.
    dirt(&[RED_LINK, map(737.0, 395.0), RED_TURN]),
    dirt(&[SOUTH_SUBURBS_FORK, SUBURBS]),
    dirt(&[NORTH_SUBURBS_FORK, NORTH_SUBURBS]),
    // The industrial ruins north of the highway.
    dirt(&[INDUSTRY_FORK, SILOS, FACTORY, BRICKWORKS, SUBSTATION]),
    // From the railway along the marsh's south shore to the crater.
    dirt(&[
        RAIL_SOUTH,
        map(720.0, 672.0),
        SWAMP,
        map(830.0, 702.0),
        CRATER,
    ]),
];

/// Where Harmont's traffic drives (`player/vehicle/traffic.rs`), each route
/// with whether it closes on itself: the ring street round New Harmont's
/// blocks, the plaza avenue short of the ring, and the road from the
/// Institute down the main road to the Borscht.
pub(crate) fn traffic_routes() -> [(Vec<Vec2>, bool); 3] {
    let [west, east] = town::AVENUE;
    let (west_side, east_side) = (town::STREETS[2], town::STREETS[3]);
    // North, round the west side to south, then back up the east side.
    let ring = west_side
        .iter()
        .chain(east_side[1..east_side.len() - 1].iter().rev())
        .copied()
        .collect();
    let short_of_ring = Vec2::new(20.0, 0.0);
    let avenue = vec![west + short_of_ring, town::PLAZA, east - short_of_ring];
    let main = [INSTITUTE, AVENUE, MOTOR_POOL]
        .into_iter()
        .chain(MAIN_ROAD[1..].iter().copied())
        .chain([BORSCHT_LANE, BORSCHT])
        .collect();
    [(ring, true), (avenue, false), (main, false)]
}

/// The Institute carrier's expedition (`player/vehicle/carrier.rs`): from the
/// yard's gate through the Institute gate and along the highway to the north
/// suburbs' fork, about 300 m into the Zone.
pub(crate) fn expedition_route() -> [Vec2; 7] {
    [
        GATE,
        YARD_TURN,
        map(397.0, 348.0),
        PERIMETER_GATE,
        map(442.0, 353.0),
        map(487.0, 348.0),
        NORTH_SUBURBS_FORK,
    ]
}

/// The dirt tracks and old asphalt roads outside the town's streets.
pub(super) fn country_roads() -> impl Iterator<Item = &'static [Vec2]> {
    ROUTES
        .iter()
        .filter(|route| route.surface != Surface::Street)
        .map(|route| route.points)
}

/// The Golden Sphere's haul road in world xz (`tools/plan_quarry_ramp.py`),
/// from the entrance in the pit's rim down to the Sphere's junction.
#[allow(
    clippy::excessive_precision,
    reason = "The planner writes 5 decimals; the file is generated, not edited"
)]
const GOLDEN_PIT_RAMP: &[Vec2] = include!("pillars/golden_pit_ramp.rs");

/// The Golden Sphere's haul road: its drive from the Chapter 4 road's end on
/// the rim to the baked entrance, then the baked path down the pit to the Sphere.
pub(crate) fn golden_pit_ramp() -> Vec<Vec2> {
    [GOLDEN_RIM]
        .into_iter()
        .chain(GOLDEN_PIT_RAMP.iter().copied())
        .collect()
}

/// The railway's bed: the map's line, split into graded runs.
fn railway_bed() -> Vec<Vec2> {
    let mut bed = vec![RAILWAY[0]];
    for &[a, b] in RAILWAY.array_windows() {
        let runs = (a.distance(b) / RAIL_RUN).ceil();
        bed.extend((1..=runs as usize).map(|i| a.lerp(b, i as f32 / runs)));
    }
    bed
}

/// Every graded path: the routes, each site's drive, the railway bed and the
/// two ramps: the quarry's, which carries the quarry's drive as its head, and
/// the Golden Sphere's, which starts at the end of the Chapter 4 road.
fn tracks() -> Vec<(Surface, Vec<Vec2>)> {
    let routes = ROUTES
        .iter()
        .map(|route| (route.surface, route.points.to_vec()));
    let drives = LANDMARKS
        .iter()
        .filter(|site| site.junction != QUARRY)
        .map(|site| (Surface::Dirt, vec![site.junction, site.entrance()]));
    routes
        .chain(drives)
        .chain([
            (Surface::Ballast, railway_bed()),
            (Surface::Ramp, sites::quarry_ramp()),
            (Surface::Ramp, golden_pit_ramp()),
        ])
        .collect()
}

#[derive(Clone)]
struct Leg {
    start: Vec2,
    along: Vec2,
    length: f32,
    levels: [f32; 2],
    surface: Surface,
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
    surface: Surface,
    pub height: f32,
}

impl RoadSample {
    pub fn mask(&self) -> f32 {
        let (half_width, shoulder) = self.surface.widths();
        smoothstep(shoulder, half_width, self.distance)
    }

    pub fn is_ramp(&self) -> bool {
        self.surface == Surface::Ramp
    }

    /// The material mask: gravel, tire ruts, asphalt, and the share of that asphalt
    /// that is old and broken (New Harmont's streets are smooth tar).
    fn surface(&self) -> [u8; 4] {
        let d = self.distance;
        let [gravel, rut, asphalt, old] = match self.surface {
            Surface::Dirt => [smoothstep(7.5, 4.6, d), ruts(d), 0.0, 0.0],
            Surface::Ramp => [smoothstep(4.2, 2.8, d), ruts(d), 0.0, 0.0],
            Surface::Asphalt | Surface::Street => {
                let tar = smoothstep(6.4, 5.4, d);
                let old = if self.surface == Surface::Asphalt {
                    tar
                } else {
                    0.0
                };
                [smoothstep(8.2, 6.2, d), 0.0, tar, old]
            }
            Surface::Ballast => [smoothstep(4.4, 2.6, d), 0.0, 0.0, 0.0],
        };
        [gravel, rut, asphalt, old].map(|v| (v * 255.0).round() as u8)
    }
}

/// How much a tire rut stands out at `d` from the track's middle.
fn ruts(d: f32) -> f32 {
    smoothstep(0.83, 0.32, (d - 1.51).abs())
}

/// A ramp's vertical curve at its entrance spans this many meters, half on
/// the drive and half down the ramp.
const CREST_LENGTH: f32 = 24.0;
/// How far apart the crest's points lie; at 1 m a chord's grade breaks stay under the curve's own bend.
const CREST_STEP: f32 = 1.0;

/// A track's points with the graded level at each. A ramp's first leg is its
/// drive, following the land like any other; from the drive's end the ramp
/// falls evenly with its length to its last point's level, however the land
/// between lies. A parabola rounds the break between the two grades: left
/// sharp, it launches a car at full throttle off the entrance. Other tracks
/// follow the land at every corner.
fn graded_track(
    surface: Surface,
    path: &[Vec2],
    level: impl Fn(Vec2) -> f32,
) -> (Vec<Vec2>, Vec<f32>) {
    if surface != Surface::Ramp {
        return (path.to_vec(), path.iter().map(|&at| level(at)).collect());
    }
    let (junction, entrance) = (path[0], path[1]);
    let half = CREST_LENGTH / 2.0;
    debug_assert!(
        junction.distance(entrance) >= half,
        "the ramp's drive is shorter than half the crest"
    );
    // The crest's points: the drive's last half and the ramp's first half, in
    // steps, each with its signed run from the entrance (negative on the drive).
    let toward_junction = (junction - entrance).normalize();
    let mut points: Vec<(Vec2, f32)> = (0..=(half / CREST_STEP) as usize)
        .rev()
        .map(|i| i as f32 * CREST_STEP)
        .map(|back| (entrance + toward_junction * back, -back))
        .collect();
    points.insert(0, (junction, -junction.distance(entrance)));
    for &[a, b] in path[1..].array_windows() {
        let (_, start) = points[points.len() - 1];
        let pieces = if start < half {
            (a.distance(b) / CREST_STEP).ceil()
        } else {
            1.0
        };
        for i in 1..=pieces as usize {
            let t = i as f32 / pieces;
            points.push((a.lerp(b, t), start + a.distance(b) * t));
        }
    }

    let (drive, total) = (-points[0].1, points[points.len() - 1].1);
    let (start, top, bottom) = (
        level(junction),
        level(entrance),
        level(path[path.len() - 1]),
    );
    let (drive_grade, ramp_grade) = ((top - start) / drive, (bottom - top) / total);
    points
        .into_iter()
        .map(|(at, run)| {
            let line = top + run * if run < 0.0 { drive_grade } else { ramp_grade };
            let beyond = (half - run.abs()).max(0.0);
            let round = (ramp_grade - drive_grade) * beyond * beyond / (2.0 * CREST_LENGTH);
            (at, line + round)
        })
        .unzip()
}

impl RoadNetwork {
    pub fn new(level: impl Fn(Vec2) -> f32) -> Self {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        let mut tiles: HashMap<_, Vec<_>> = HashMap::default();
        for (surface, path) in tracks() {
            let (path, levels) = graded_track(surface, &path, &level);
            for (&[a, b], &levels) in path.array_windows().zip(levels.array_windows()) {
                let delta = b - a;
                let length = delta.length();
                let reach = Vec2::splat(surface.widths().1);
                let leg = Leg {
                    start: a,
                    along: delta / length,
                    length,
                    levels,
                    surface,
                    min: a.min(b) - reach,
                    max: a.max(b) + reach,
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

    /// The nearest leg whose shoulder reaches `at`.
    fn sample_legs(legs: &[Leg], at: Vec2) -> Option<RoadSample> {
        let mut nearest = None;
        let mut best = f32::INFINITY;
        for leg in legs {
            if at.cmplt(leg.min).any() || at.cmpgt(leg.max).any() {
                continue;
            }
            let run = (at - leg.start).dot(leg.along).clamp(0.0, leg.length);
            let distance = at.distance(leg.start + leg.along * run);
            // Compare by how far into its shoulder the point lies.
            let depth = distance / leg.surface.widths().1;
            if depth < 1.0 && depth < best {
                nearest = Some((leg, run / leg.length, distance));
                best = depth;
            }
        }
        nearest.map(|(leg, t, distance)| RoadSample {
            distance,
            surface: leg.surface,
            height: lerp(leg.levels[0], leg.levels[1], t),
        })
    }

    pub fn surface_texture(&self) -> (Image, Vec4) {
        // A world-space mask keeps the same road width across coarse and fine terrain tiles.
        const TEXELS_PER_METRE: f32 = 1.5;
        let origin = self.min.floor();
        let size = ((self.max.ceil() - origin) * TEXELS_PER_METRE).as_uvec2();
        let mut pixels = vec![0; (size.x * size.y * 4) as usize];
        for (&tile, legs) in &self.tiles {
            let first = tile.as_vec2() * TILE_SIZE;
            let pixel = |at: Vec2| ((at - origin) * TEXELS_PER_METRE - 0.5).ceil().as_ivec2();
            let low = pixel(first).max(IVec2::ZERO);
            let high = pixel(first + TILE_SIZE).min(size.as_ivec2());
            for z in low.y..high.y {
                for x in low.x..high.x {
                    let at = origin + (Vec2::new(x as f32, z as f32) + 0.5) / TEXELS_PER_METRE;
                    let i = (z as usize * size.x as usize + x as usize) * 4;
                    if let Some(road) = Self::sample_legs(legs, at) {
                        pixels[i..i + 4].copy_from_slice(&road.surface());
                    }
                    if town::paved(at) {
                        pixels[i + 2] = 255;
                        pixels[i + 3] = 0;
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
            TextureFormat::Rgba8Unorm,
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

    fn on_routes(at: Vec2) -> bool {
        ROUTES.iter().any(|route| route.points.contains(&at))
    }

    #[test]
    fn every_track_joins_the_network_back_to_the_gate() {
        assert!(ROUTES[0].points.contains(&GATE));
        // The rail bed is drivable and meets the Chapter 4 track at its crossing.
        let bed = railway_bed();
        for (i, route) in ROUTES.iter().enumerate().skip(1) {
            let start = route.points[0];
            assert!(
                ROUTES[..i].iter().any(|road| road.points.contains(&start)) || bed.contains(&start),
                "route {i} starts off the network"
            );
        }
        assert!(bed.contains(&RAIL_CROSSING) && on_routes(RAIL_CROSSING));
        // A ramp track is the road's end for a site it leads to.
        let ramp_ends: Vec<Vec2> = ramp_tracks()
            .iter()
            .map(|path| path[path.len() - 1])
            .collect();
        for site in LANDMARKS {
            assert!(
                on_routes(site.junction)
                    || bed.contains(&site.junction)
                    || ramp_ends.contains(&site.junction),
                "{:?} has no road",
                site.junction
            );
        }
    }

    #[test]
    fn every_track_is_drivable() {
        let terrain = Terrain::new();
        let mut bad = Vec::new();
        for (i, (surface, path)) in tracks().into_iter().enumerate() {
            let lane = surface.widths().0 * 0.4;
            for &[a, b] in path.array_windows() {
                let mut worst_grade = (0.0, a);
                let mut deepest = (0.0, a);
                let steps = a.distance(b).ceil() as usize;
                let across = (b - a).normalize().perp();
                for offset in [-lane, 0.0, lane].map(|lane| across * lane) {
                    let mut last = terrain.mesh_height_at(a + offset);
                    for step in 0..=steps {
                        let at = a.lerp(b, step as f32 / steps as f32) + offset;
                        assert!(terrain.road_mask(at.x, at.y) > 0.99, "track {i} at {at}");
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
            "track, grade and position, depth and position: {bad:#?}"
        );
    }

    /// The quarry's haul road and the Golden Sphere's.
    fn ramp_tracks() -> Vec<Vec<Vec2>> {
        let ramps: Vec<_> = tracks()
            .into_iter()
            .filter(|(surface, _)| *surface == Surface::Ramp)
            .map(|(_, path)| path)
            .collect();
        assert_eq!(ramps.len(), 2, "the quarry's and the Golden Sphere's ramps");
        ramps
    }

    fn quarry_ramp_track() -> Vec<Vec2> {
        ramp_tracks()
            .into_iter()
            .find(|path| path[0] == QUARRY)
            .expect("the quarry has a ramp track")
    }

    /// The point halfway along `path`, by length.
    fn midway(path: &[Vec2]) -> Vec2 {
        let length: f32 = path.array_windows().map(|&[a, b]| a.distance(b)).sum();
        let mut left = length / 2.0;
        for &[a, b] in path.array_windows() {
            let leg = a.distance(b);
            if left <= leg {
                return a.lerp(b, left / leg);
            }
            left -= leg;
        }
        path[path.len() - 1]
    }

    #[test]
    fn the_ramp_starts_at_the_quarry_drive() {
        let quarry = LANDMARKS
            .iter()
            .find(|site| site.junction == QUARRY)
            .expect("the quarry is a landmark");
        let track = quarry_ramp_track();
        assert_eq!(track[..2], [quarry.junction, quarry.entrance()]);
        // The drive is the ramp's head, not a track of its own beside it.
        assert!(!tracks().iter().any(|(_, path)| path[..] == track[..2]));
    }

    #[test]
    fn the_golden_ramp_starts_on_the_chapter_4_road() {
        let chapter_4 = ROUTES
            .iter()
            .find(|route| route.points.contains(&QUARRY))
            .expect("the Chapter 4 route passes the quarry");
        assert_eq!(chapter_4.points.last(), Some(&GOLDEN_RIM));
        assert_eq!(golden_pit_ramp()[0], GOLDEN_RIM);
        assert!(ramp_tracks().contains(&golden_pit_ramp()));
        // The old chute along the pit's floor is gone.
        let chute = map(1146.0, 368.0);
        assert!(!ROUTES.iter().any(|route| route.points.contains(&chute)));
    }

    #[test]
    fn the_golden_ramp_ends_at_the_golden_sphere() {
        assert_eq!(golden_pit_ramp().last(), Some(&GOLDEN_SPHERE));
    }

    /// A point on a path, its run from the path's start and the direction across it.
    struct Station {
        run: f32,
        at: Vec2,
        across: Vec2,
    }

    /// Points along `path` every `step` meters, and its last point.
    fn stations(path: &[Vec2], step: f32) -> Vec<Station> {
        let mut found = Vec::new();
        let (mut start, mut next) = (0.0, 0.0);
        for &[a, b] in path.array_windows() {
            let leg = a.distance(b);
            let across = (b - a).normalize().perp();
            while next < start + leg {
                let at = a.lerp(b, (next - start) / leg);
                found.push(Station {
                    run: next,
                    at,
                    across,
                });
                next += step;
            }
            start += leg;
        }
        let last = path.len() - 1;
        let across = (path[last] - path[last - 1]).normalize().perp();
        found.push(Station {
            run: start,
            at: path[last],
            across,
        });
        found
    }

    /// How far `at` lies from the segment from `a` to `b`.
    fn distance_to_leg(at: Vec2, a: Vec2, b: Vec2) -> f32 {
        let leg = b - a;
        let t = ((at - a).dot(leg) / leg.length_squared()).clamp(0.0, 1.0);
        at.distance(a + leg * t)
    }

    #[test]
    fn the_golden_ramp_keeps_clear_of_other_tracks() {
        // Its first 30 m leave the Chapter 4 road at the rim and its last 30 m
        // join the Sphere's own drive; between them no other track's nearest
        // leg may take the road's level.
        const JOINS: f32 = 30.0;
        let ramp = golden_pit_ramp();
        let length = stations(&ramp, 1.0).last().map_or(0.0, |end| end.run);
        let others: Vec<_> = tracks()
            .into_iter()
            .filter(|(_, path)| *path != ramp)
            .collect();
        let mut worst = (f32::INFINITY, Vec2::ZERO);
        for station in stations(&ramp, 1.0) {
            if station.run < JOINS || station.run > length - JOINS {
                continue;
            }
            for (_, path) in &others {
                for &[a, b] in path.array_windows() {
                    let gap = distance_to_leg(station.at, a, b);
                    if gap < worst.0 {
                        worst = (gap, station.at);
                    }
                }
            }
        }
        assert!(
            worst.0 > 2.0 * RAMP_SHOULDER,
            "another track comes within {} m of the ramp at {}",
            worst.0,
            worst.1
        );
    }

    #[test]
    fn the_golden_ramp_meets_the_chapter_4_road_level() {
        let terrain = Terrain::new();
        let ramp = golden_pit_ramp();
        let chapter_4 = ROUTES
            .iter()
            .find(|route| route.points.contains(&QUARRY))
            .expect("the Chapter 4 route passes the quarry");
        let before = chapter_4.points[chapter_4.points.len() - 2];
        let level = |toward: Vec2| {
            let at = GOLDEN_RIM + (toward - GOLDEN_RIM).normalize();
            terrain
                .roads
                .sample(at)
                .expect("the rim is on the road")
                .height
        };
        let step = (level(before) - level(ramp[1])).abs();
        assert!(step < 0.2, "the road steps {step} m at the rim");
    }

    #[test]
    fn the_golden_ramp_keeps_off_the_dioramas() {
        // The ramp may pass a scan's ground blend, but not its footprint: that
        // is the Golden Sphere's own pad and the Quarry rim's scan.
        let footprints: Vec<_> = sites::diorama_footprints().collect();
        for name in ["The Golden Sphere", "Quarry rim"] {
            assert!(footprints.iter().any(|(found, ..)| *found == name));
        }
        for station in stations(&golden_pit_ramp(), 3.0) {
            for side in [-RAMP_SHOULDER, 0.0, RAMP_SHOULDER] {
                let at = station.at + station.across * side;
                assert!(
                    sites::diorama_ground(at).is_none(),
                    "a diorama's ground lies {side} m from the ramp's {} m at {at}",
                    station.run
                );
                for (name, (low, high), _) in &footprints {
                    assert!(
                        at.cmplt(*low).any() || at.cmpgt(*high).any(),
                        "{name}'s footprint holds the point {side} m from the ramp's {} m at {at}",
                        station.run
                    );
                }
            }
        }
    }

    #[test]
    fn the_golden_ramp_is_dry() {
        let terrain = Terrain::new();
        for station in stations(&golden_pit_ramp(), 1.0) {
            for side in [-RAMP_HALF_WIDTH, 0.0, RAMP_HALF_WIDTH] {
                let at = station.at + station.across * side;
                let depth = terrain.water_depth_at(at.x, at.y);
                assert_eq!(
                    depth, 0.0,
                    "{depth} m of water {side} m from the ramp's {} m at {at}",
                    station.run
                );
            }
        }
    }

    #[test]
    fn a_ramp_is_graded_evenly() {
        for track in ramp_tracks() {
            let (drive, ramp) = track.split_at(1);
            let roads = RoadNetwork::new(|at| at.x);
            let ends = (ramp[0].x, ramp[ramp.len() - 1].x);
            assert!((ends.0 - ends.1).abs() > 10.0, "the ramp must change level");
            let sample = |at| {
                roads
                    .sample(at)
                    .expect("the ramp's middle is on the network")
            };
            let road = sample(midway(ramp));
            assert_eq!(road.surface, Surface::Ramp);
            let mean = (ends.0 + ends.1) / 2.0;
            assert!(
                (road.height - mean).abs() < 0.01,
                "height {} vs mean {mean}",
                road.height
            );
            // Its drive follows the land from the junction to the entrance.
            let middle = sample(midway(&[drive[0], ramp[0]]));
            let mean = (drive[0].x + ramp[0].x) / 2.0;
            assert!((middle.height - mean).abs() < 0.01);
        }
    }

    #[test]
    fn ramp_mask_paints_a_narrow_dirt_bed() {
        let at = |distance| RoadSample {
            distance,
            surface: Surface::Ramp,
            height: 0.0,
        };
        assert_eq!(at(0.0).surface()[0], 255);
        assert_eq!(at(5.0).surface()[0], 0);
        assert_eq!(at(RAMP_SHOULDER + 0.1).mask(), 0.0);
        assert_eq!(at(RAMP_HALF_WIDTH).mask(), 1.0);
        // Two tire ruts, as on a dirt track.
        let dirt = RoadSample {
            distance: 1.51,
            surface: Surface::Dirt,
            height: 0.0,
        };
        assert_eq!(at(1.51).surface()[1], dirt.surface()[1]);
        assert!(at(1.51).surface()[1] >= 190);
    }

    /// The mask's four bytes (gravel, ruts, asphalt, old asphalt) at a world point.
    fn read4(image: &Image, bounds: Vec4, at: Vec2) -> [u8; 4] {
        let uv = (at - bounds.xy()) * bounds.zw();
        let texel = (uv * Vec2::new(image.width() as f32, image.height() as f32))
            .floor()
            .as_uvec2();
        let i = ((texel.y * image.width() + texel.x) * 4) as usize;
        let bytes = image.data.as_ref().expect("mask pixels");
        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
    }

    #[test]
    fn old_asphalt_is_marked_apart_from_new_harmont_streets() {
        let terrain = Terrain::new();
        let (image, bounds) = terrain.roads.surface_texture();
        let read = |at: Vec2| read4(&image, bounds, at);
        // The highway and the yard gate's road are old asphalt.
        assert_eq!(
            read(map(667.0, 373.0).midpoint(map(697.0, 362.0))),
            [255, 0, 255, 255]
        );
        assert_eq!(read(GATE), [255, 0, 255, 255]);
        // New Harmont stays smooth tar: its plaza and a street's centerline.
        assert_eq!(read(town::PLAZA + Vec2::new(20.0, 15.0))[2..], [255, 0]);
        let boulevard = town::STREETS[0];
        assert_eq!(read(boulevard[0].midpoint(boulevard[1]))[2..], [255, 0]);
    }

    #[test]
    fn the_material_mask_paints_asphalt_dirt_ruts_and_the_rail_bed() {
        let terrain = Terrain::new();
        let (image, bounds) = terrain.roads.surface_texture();
        let read = |at: Vec2| {
            let [gravel, ruts, asphalt, _] = read4(&image, bounds, at);
            [gravel, ruts, asphalt]
        };
        // The asphalt leaves the yard gate; the yard itself stays unpainted.
        assert_eq!(read(GATE), [255, 0, 255]);
        assert_eq!(read(GATE + Vec2::new(-10.0, -4.0)), [0, 0, 0]);
        assert_eq!(read(town::PLAZA + Vec2::new(20.0, 15.0))[2], 255);
        // A dirt track between the breach and the suburbs has two ruts.
        let (a, b) = (map(480.0, 505.0), SOUTH_SUBURBS_FORK);
        let middle = a.midpoint(b);
        let across = (b - a).normalize().perp();
        assert_eq!(read(middle), [255, 0, 0]);
        for side in [-1.0, 1.0] {
            assert!(read(middle + across * side * 1.5)[1] >= 190);
            assert_eq!(read(middle + across * side * 8.0), [0, 0, 0]);
        }
        // The railway's narrow gravel bed.
        let rail = RAILWAY[3].midpoint(RAILWAY[4]);
        let across = (RAILWAY[4] - RAILWAY[3]).normalize().perp();
        assert_eq!(read(rail), [255, 0, 0]);
        assert_eq!(read(rail + across * 5.0), [0, 0, 0]);
        for route in ROUTES {
            assert!(route.points.iter().all(|&at| read(at)[0] == 255));
        }
    }
}
