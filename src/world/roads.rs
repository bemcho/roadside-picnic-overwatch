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
/// two ramps. The quarry's carries the quarry's drive as its head; the Golden
/// Sphere's carries the Sphere's, and leaves the end of the quarry's ramp, the
/// only way to it.
fn tracks() -> Vec<(Surface, Vec<Vec2>)> {
    let routes = ROUTES
        .iter()
        .map(|route| (route.surface, route.points.to_vec()));
    let drives = LANDMARKS
        .iter()
        .filter(|site| ![QUARRY, GOLDEN_SPHERE].contains(&site.junction))
        .map(|site| (Surface::Dirt, vec![site.junction, site.entrance()]));
    routes
        .chain(drives)
        .chain([
            (Surface::Ballast, railway_bed()),
            (Surface::Ramp, sites::quarry_ramp()),
            (Surface::Ramp, sites::crater_ramp()),
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
        // A ramp starts on a route or where an earlier ramp ends, and carries the
        // junction of the site it leads to.
        let ramps = ramp_tracks();
        for (i, ramp) in ramps.iter().enumerate() {
            assert!(
                on_routes(ramp[0])
                    || ramps[..i]
                        .iter()
                        .any(|earlier| earlier.last() == Some(&ramp[0])),
                "ramp {i} starts off the network"
            );
        }
        for site in LANDMARKS {
            assert!(
                on_routes(site.junction)
                    || bed.contains(&site.junction)
                    || ramps.iter().any(|ramp| ramp.contains(&site.junction)),
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

    fn crater_ramp_track() -> Vec<Vec2> {
        ramp_tracks()
            .into_iter()
            .find(|path| path.contains(&GOLDEN_SPHERE))
            .expect("the Golden Sphere has a ramp track")
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

    /// The Golden Sphere's centre in the core scan's model units, the centroid
    /// of its painted glow on the crater's lowest inner wall (the scan has no
    /// geometry for it), and its radius in metres.
    const SPHERE: Vec3 = Vec3::new(-0.0172, 0.0, -0.0138);
    const SPHERE_RADIUS: f32 = 0.6;

    fn golden_sphere() -> Vec2 {
        let (pose, _) = sites::core_scan();
        pose.transform_point(SPHERE).xz()
    }

    #[test]
    fn the_only_road_to_the_golden_sphere_runs_through_the_quarry_rim() {
        // The Chapter 4 road ends at the quarry's junction, the nearest any
        // route or dirt drive comes; the way on is the quarry's ramp and then
        // the crater's, which both come nearer.
        let reach = QUARRY.distance(GOLDEN_SPHERE);
        let chapter_4 = ROUTES
            .iter()
            .find(|route| route.points.contains(&QUARRY))
            .expect("the Chapter 4 route passes the quarry");
        assert_eq!(chapter_4.points.last(), Some(&QUARRY));
        let gap = |path: &[Vec2]| {
            path.array_windows()
                .map(|&[a, b]| distance_to_leg(GOLDEN_SPHERE, a, b))
                .fold(f32::INFINITY, f32::min)
        };
        for (surface, path) in tracks() {
            if surface == Surface::Ramp {
                assert!(gap(&path) < reach, "a ramp keeps away from the Sphere");
            } else {
                assert!(
                    gap(&path) >= reach - 0.01,
                    "a {surface:?} track {} m from the Sphere passes the quarry's junction at {reach} m",
                    gap(&path)
                );
            }
        }
    }

    #[test]
    fn the_crater_ramp_starts_at_the_quarry_ramps_end() {
        let quarry = sites::quarry_ramp();
        let crater = crater_ramp_track();
        assert_eq!(crater[0], quarry[quarry.len() - 1]);
        let site = LANDMARKS
            .iter()
            .find(|site| site.junction == GOLDEN_SPHERE)
            .expect("the Golden Sphere is a landmark");
        assert_eq!(crater[1..3], [GOLDEN_SPHERE, site.entrance()]);
        // The Sphere has no dirt drive of its own beside the ramp.
        assert!(!tracks().iter().any(|(_, path)| path[0] == GOLDEN_SPHERE));
    }

    #[test]
    fn the_crater_ramp_continues_the_quarry_ramps_level() {
        // Every half metre from 4 m before the quarry ramp's end to 4 m after
        // it, along the two tracks: no step in the road's level at the join.
        let terrain = Terrain::new();
        let quarry = sites::quarry_ramp();
        let crater = crater_ramp_track();
        let end = crater[0];
        let level = |toward: Vec2, metres: f32| {
            let at = end + (toward - end).normalize() * metres;
            terrain
                .roads
                .sample(at)
                .expect("the join is on the road")
                .height
        };
        let metres = (0..=8).map(|half| half as f32 * 0.5);
        let profile: Vec<f32> = metres
            .clone()
            .rev()
            .map(|back| level(quarry[quarry.len() - 2], back))
            .chain(metres.skip(1).map(|ahead| level(crater[1], ahead)))
            .collect();
        for window in profile.array_windows::<2>() {
            let step = (window[1] - window[0]).abs();
            assert!(step <= 0.5 * 0.25, "the road steps {step} m at the join");
        }
    }

    #[test]
    fn the_crater_ramp_ends_beside_the_sphere() {
        let terrain = Terrain::new();
        let crater = crater_ramp_track();
        let end = crater[crater.len() - 1];
        let floor = terrain.height_at(end.x, end.y);
        assert!(floor < -95.0, "the road ends at {floor} m, above the floor");
        // Its bed leaves the Sphere itself alone, and the Sphere is near.
        let gap = end.distance(golden_sphere());
        assert!(
            gap > RAMP_HALF_WIDTH + SPHERE_RADIUS,
            "the road's end lies {gap} m from the Sphere's centre, over it"
        );
        assert!(gap <= 15.0, "the road ends {gap} m from the Sphere");
    }

    #[test]
    fn the_crater_ramp_keeps_off_the_sphere() {
        // The Sphere stands on the crater's lowest wall; no leg of the road may
        // come nearer to it than its bed and a car's width.
        const CLEAR: f32 = 10.0;
        let crater = crater_ramp_track();
        let sphere = golden_sphere();
        let (gap, at) = crater
            .array_windows()
            .map(|&[a, b]| (distance_to_leg(sphere, a, b), a))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("the ramp has legs");
        assert!(
            gap >= CLEAR,
            "the road passes {gap} m from the Sphere, on the leg from {at}"
        );
    }

    /// A point on a path and its run from the path's start.
    struct Station {
        run: f32,
        at: Vec2,
    }

    /// Points along `path` every `step` meters, and its last point.
    fn stations(path: &[Vec2], step: f32) -> Vec<Station> {
        let mut found = Vec::new();
        let (mut start, mut next) = (0.0, 0.0);
        for &[a, b] in path.array_windows() {
            let leg = a.distance(b);
            while next < start + leg {
                let at = a.lerp(b, (next - start) / leg);
                found.push(Station { run: next, at });
                next += step;
            }
            start += leg;
        }
        found.push(Station {
            run: start,
            at: path[path.len() - 1],
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
    fn the_crater_ramp_keeps_clear_of_other_tracks() {
        // Its first 30 m leave the quarry ramp's end; after them no other
        // track's nearest leg may take the road's level.
        const LEAVES: f32 = 30.0;
        let ramp = crater_ramp_track();
        let others: Vec<_> = tracks()
            .into_iter()
            .filter(|(_, path)| *path != ramp)
            .collect();
        let mut worst = (f32::INFINITY, Vec2::ZERO);
        for station in stations(&ramp, 1.0) {
            if station.run < LEAVES {
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
    fn the_crater_ramp_laps_keep_apart() {
        // Two laps closer than the shoulders would cut one road's bank into
        // the other's bed.
        const APART: f32 = 18.0;
        const SEPARATE: f32 = 60.0;
        let stations = stations(&crater_ramp_track(), 1.0);
        let mut closest = (f32::INFINITY, 0.0, 0.0);
        for (i, a) in stations.iter().enumerate() {
            for b in stations[i + 1..]
                .iter()
                .filter(|b| b.run - a.run > SEPARATE)
            {
                let gap = a.at.distance(b.at);
                if gap < closest.0 {
                    closest = (gap, a.run, b.run);
                }
            }
        }
        assert!(
            closest.0 >= APART,
            "the road's {} m and {} m runs lie {} m apart",
            closest.1,
            closest.2,
            closest.0
        );
    }

    /// The radius of the circle through three points.
    fn circumradius([a, b, c]: [Vec2; 3]) -> f32 {
        let area_twice = (b - a).perp_dot(c - a).abs();
        a.distance(b) * b.distance(c) * c.distance(a) / (2.0 * area_twice)
    }

    #[test]
    fn the_crater_ramp_bends_no_tighter_than_9_m() {
        // Tracks are polylines: every corner, the connector's and the
        // spiral's, is judged by the circle through it and its neighbours.
        let crater = crater_ramp_track();
        let (radius, at) = crater
            .array_windows::<3>()
            .map(|&window| (circumradius(window), window[1]))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("the ramp has corners");
        assert!(radius >= 9.0, "the road bends at {radius} m at {at}");
    }

    #[test]
    fn the_connector_meets_the_spiral_in_line() {
        // The drive runs straight in over the entrance; the baked spiral
        // leaves the front edge about 6 degrees off it. The kink between the
        // two legs is too slight to round (the test above gives its radius).
        const KINK: f32 = 10.0;
        let crater = crater_ramp_track();
        let [entrance, edge, next] = [crater[2], crater[3], crater[4]];
        let turn = (edge - entrance).angle_to(next - edge).to_degrees().abs();
        assert!(
            turn <= KINK,
            "the spiral leaves the drive at {turn} degrees"
        );
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
