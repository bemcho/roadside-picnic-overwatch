//! The named places of the Harmont map (`doc/Roadside_map.jpg`): the town
//! outside the Perimeter and the Zone's landmarks inside it, in the order the
//! novel walks them, from the Borscht and the Institute to the Golden Sphere.
//!
//! Every place is a dense UHD model loaded as is. Buildings stand on a level
//! pad; dioramas with ground of their own (a hillside town, the quarry, the
//! ridge) carry a sampled relief that the terrain follows just below.

use std::f32::consts::FRAC_PI_2;

use bevy::gltf::GltfMaterial;
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use crate::assets::gpu_scans::{GpuScans, UPLOAD_LEAD};
use crate::assets::{self, PreloadedAssets};

use super::collision::{Collider, Pillar};
use super::cordon;
use super::map::map;
use super::places::Place;
use super::roads;
use super::terrain::{BASE, TILE_SIZE, landform_height, lerp, smoothstep};
use super::town;
use super::{StreamAnchor, WorldRoot, detail_range};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Institute,
    Borscht,
    OldBorscht,
    PlagueQuarter,
    SuburbanHouse,
    Schoolhouse,
    Cemetery,
    AbandonedSuburbs,
    OldFactory,
    BrickFactory,
    Silos,
    Substation,
    AbandonedSubstation,
    Highway,
    Garage,
    MotorPool,
    Railway,
    Locomotive,
    Marsh,
    SwampFlats,
    Ridge,
    Crater,
    QuarryRim,
    GoldenSphere,
}

impl Kind {
    /// A place named on the Harmont map, as opposed to the sites around them.
    pub(crate) fn is_landmark(self) -> bool {
        MODELS[self as usize].landmark
    }
}

/// How a model meets the ground.
#[derive(Clone, Copy)]
enum Ground {
    /// A building: its datum (model units) sits on a level pad.
    Level(f32),
    /// A diorama with its own ground, which the terrain follows.
    Relief(&'static Relief),
}

/// A diorama's walkable ground, sampled by `tools/sample_landmark_ground.py`:
/// heights in model units on a `RELIEF_SIZE` grid of cells over its footprint.
pub(super) struct Relief {
    min: Vec2,
    cell: Vec2,
    heights: &'static [f32],
}

const RELIEF_SIZE: usize = 64;

impl Relief {
    /// Bilinear height at a model-space point, clamped to the footprint.
    fn height(&self, at: Vec2) -> f32 {
        let last = (RELIEF_SIZE - 1) as f32;
        let grid = ((at - self.min) / self.cell - 0.5).clamp(Vec2::ZERO, Vec2::splat(last));
        let cell = grid.floor().min(Vec2::splat(last - 1.0));
        let f = grid - cell;
        let (x, z) = (cell.x as usize, cell.y as usize);
        let h = |dx: usize, dz: usize| self.heights[(z + dz) * RELIEF_SIZE + x + dx];
        lerp(
            lerp(h(0, 0), h(1, 0), f.x),
            lerp(h(0, 1), h(1, 1), f.x),
            f.y,
        )
    }

    /// The mean height over a square of half-width `radius` around `at`, so
    /// the land around a diorama meets its edge exactly and relaxes into a
    /// smoothed version of it farther out, without streaks from clefts.
    fn blurred(&self, at: Vec2, radius: f32) -> f32 {
        const TAPS: usize = 5;
        let step = 2.0 * radius / (TAPS - 1) as f32;
        let mut sum = 0.0;
        for j in 0..TAPS {
            for i in 0..TAPS {
                sum += self.height(at - radius + Vec2::new(i as f32, j as f32) * step);
            }
        }
        sum / (TAPS * TAPS) as f32
    }
}

/// Which model axis faces the road.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Front {
    X,
    Z,
}

struct Model {
    name: &'static str,
    /// The full UHD scan, streamed in near the stalker or the view.
    path: &'static str,
    /// A light stand-in built from the scan, shown from farther away.
    far: &'static str,
    scale: f32,
    /// Stretch along model x on top of `scale` (1 keeps the scan's
    /// proportions). Buildings only: a relief keeps its true scale.
    widen: f32,
    ground: Ground,
    /// Half the footprint along model x and z, in model units.
    half: Vec2,
    /// The highest point, in model units.
    top: f32,
    front: Front,
    landmark: bool,
    pillars: &'static [Pillar],
}

impl Model {
    /// The same model stretched `widen` times along its x axis.
    const fn widened(self, widen: f32) -> Self {
        Self { widen, ..self }
    }

    /// Meters per model unit along model x, y and z.
    fn size(&self) -> Vec3 {
        Vec3::new(self.scale * self.widen, self.scale, self.scale)
    }

    /// Half the footprint along model x and z, in meters.
    fn half_meters(&self) -> Vec2 {
        self.half * self.size().xz()
    }

    /// The model height (model units) laid on the land around it: a
    /// building's floor, or the middle of a diorama's front edge.
    fn datum(&self) -> f32 {
        match self.ground {
            Ground::Level(datum) => datum,
            Ground::Relief(relief) => relief.height(match self.front {
                Front::X => Vec2::new(self.half.x, 0.0),
                Front::Z => Vec2::new(0.0, self.half.y),
            }),
        }
    }

    /// How far the front reaches from the middle, in meters.
    fn front_extent(&self) -> f32 {
        match self.front {
            Front::X => self.half_meters().x,
            Front::Z => self.half_meters().y,
        }
    }

    /// Radius of the circle around the footprint, in meters.
    fn radius(&self) -> f32 {
        self.half_meters().length()
    }

    /// Width of the slope that blends a relief into the land, in meters.
    fn relief_margin(&self) -> f32 {
        30.0 + 0.25 * self.scale
    }
}

macro_rules! model {
    ($name:literal, $dir:literal, $file:literal, $scale:literal, $ground:expr,
     ($hx:literal, $hz:literal), $top:literal, $front:ident, $landmark:literal,
     $pillars:literal) => {
        Model {
            name: $name,
            path: concat!("models/", $dir, "/uhd/", $file, ".glb"),
            far: concat!("models/", $dir, "/low/", $file, ".glb"),
            scale: $scale,
            widen: 1.0,
            ground: $ground,
            half: Vec2::new($hx, $hz),
            top: $top,
            front: Front::$front,
            landmark: $landmark,
            pillars: include!(concat!("pillars/uhd_", $pillars, ".rs")),
        }
    };
}

/// The dioramas' own ground, measured by `tools/sample_landmark_ground.py`.
#[allow(clippy::approx_constant, reason = "Measured model heights")]
mod grounds {
    use bevy::math::Vec2;

    use super::Relief;

    pub(super) const PLAGUE: Relief = include!("pillars/uhd_plague_quarter_ground.rs");
    pub(super) const SUBURBS: Relief = include!("pillars/uhd_abandoned_suburbs_ground.rs");
    pub(super) const HIGHWAY: Relief = include!("pillars/uhd_highway_ground.rs");
    pub(super) const EMBANKMENT: Relief = include!("pillars/uhd_railway_embankment_ground.rs");
    pub(super) const MARSH: Relief = include!("pillars/uhd_marsh_ground.rs");
    pub(super) const RIDGE: Relief = include!("pillars/uhd_ridge_ground.rs");
    pub(super) const CRATER: Relief = include!("pillars/uhd_crater_amphitheater_ground.rs");
    pub(super) const QUARRY_RIM: Relief = include!("pillars/uhd_quarry_pit_ground.rs");
    pub(super) const GOLDEN_SPHERE: Relief = include!("pillars/uhd_core_ground.rs");
    pub(super) const SWAMP_FLATS: Relief = include!("pillars/uhd_swamp_ground.rs");
}
use grounds::*;

const KIND_COUNT: usize = Kind::GoldenSphere as usize + 1;

#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
const MODELS: [Model; KIND_COUNT] = [
    model!(
        "International Institute",
        "landmarks",
        "institute",
        46.0,
        Ground::Level(0.0),
        (0.321, 0.233),
        0.98,
        Z,
        true,
        "institute"
    ),
    // The stalkers' tavern, twice the scan's width along its front.
    model!(
        "The Borscht",
        "landmarks",
        "tavern-harmont",
        22.0,
        Ground::Level(-0.205),
        (0.447, 0.49),
        0.406,
        Z,
        true,
        "tavern_harmont"
    )
    .widened(2.0),
    model!(
        "The Old Borscht",
        "landmarks",
        "bar-borscht",
        32.0,
        Ground::Level(-0.19),
        (0.132, 0.492),
        0.209,
        X,
        false,
        "bar_borscht"
    ),
    model!(
        "Plague Quarter",
        "landmarks",
        "landmark-plague-quarter",
        36.0,
        Ground::Relief(&PLAGUE),
        (0.489, 0.261),
        0.225,
        Z,
        true,
        "plague_quarter"
    ),
    model!(
        "Evacuated house",
        "sites",
        "site-plague-quarter",
        15.0,
        Ground::Level(-0.28),
        (0.309, 0.493),
        0.296,
        Z,
        false,
        "suburban_house"
    ),
    model!(
        "Schoolhouse",
        "sites",
        "site-schoolhouse",
        18.0,
        Ground::Level(-0.25),
        (0.491, 0.211),
        0.263,
        Z,
        false,
        "schoolhouse"
    ),
    // The cross stands on its own; a loose stone below it stays buried.
    model!(
        "Cemetery",
        "sites",
        "site-cemetery",
        8.0,
        Ground::Level(0.07),
        (0.194, 0.187),
        0.49,
        Z,
        false,
        "cemetery"
    ),
    model!(
        "Abandoned Suburbs",
        "landmarks",
        "landmark-abandoned-suburbs",
        140.0,
        Ground::Relief(&SUBURBS),
        (0.491, 0.271),
        0.066,
        Z,
        true,
        "abandoned_suburbs"
    ),
    model!(
        "Old Factory",
        "landmarks",
        "landmark-old-factory",
        50.0,
        Ground::Level(-0.25),
        (0.49, 0.295),
        0.256,
        Z,
        true,
        "old_factory"
    ),
    model!(
        "Brick factory",
        "sites",
        "site-brick-factory",
        40.0,
        Ground::Level(-0.37),
        (0.49, 0.478),
        0.385,
        Z,
        false,
        "brick_factory"
    ),
    model!(
        "Grain silos",
        "sites",
        "site-silos",
        21.0,
        Ground::Level(-0.485),
        (0.36, 0.219),
        0.489,
        Z,
        false,
        "silos"
    ),
    model!(
        "Substation",
        "sites",
        "site-substation",
        15.5,
        Ground::Level(-0.475),
        (0.342, 0.463),
        0.49,
        Z,
        false,
        "substation"
    ),
    model!(
        "Abandoned substation",
        "landmarks",
        "landmark-substation",
        20.0,
        Ground::Level(-0.445),
        (0.49, 0.318),
        0.469,
        Z,
        false,
        "landmark_substation"
    ),
    model!(
        "Broken Asphalt Highway",
        "landmarks",
        "landmark-highway",
        60.0,
        Ground::Relief(&HIGHWAY),
        (0.491, 0.176),
        0.049,
        Z,
        true,
        "highway"
    ),
    model!(
        "The Garage",
        "landmarks",
        "landmark-garage",
        30.0,
        Ground::Level(-0.24),
        (0.491, 0.473),
        0.272,
        Z,
        true,
        "landmark_garage"
    ),
    model!(
        "Motor pool",
        "sites",
        "site-garage",
        17.0,
        Ground::Level(-0.16),
        (0.489, 0.395),
        0.396,
        Z,
        false,
        "garage"
    ),
    model!(
        "Railway embankment",
        "sites",
        "site-railway-embankment",
        30.0,
        Ground::Relief(&EMBANKMENT),
        (0.489, 0.229),
        0.116,
        Z,
        true,
        "railway_embankment"
    ),
    model!(
        "Locomotive",
        "landmarks",
        "landmark-roundhouse",
        18.0,
        Ground::Level(-0.18),
        (0.492, 0.126),
        0.185,
        Z,
        false,
        "roundhouse"
    ),
    model!(
        "The Marsh",
        "landmarks",
        "landmark-marsh",
        45.0,
        Ground::Relief(&MARSH),
        (0.493, 0.481),
        0.309,
        Z,
        true,
        "marsh"
    ),
    // The swamp board's pieces regrouped on one ground (`tools/swamp_pieces.json`):
    // the mud bog and plank puddle face the road, the trees stand behind.
    model!(
        "Witches' jelly flats",
        "landmarks",
        "swamp",
        25.0,
        Ground::Relief(&SWAMP_FLATS),
        (0.946, 0.860),
        0.346,
        X,
        false,
        "swamp"
    ),
    model!(
        "The Ridge",
        "landmarks",
        "landmark-ridge",
        140.0,
        Ground::Relief(&RIDGE),
        (0.492, 0.395),
        0.235,
        Z,
        true,
        "ridge"
    ),
    model!(
        "Crater amphitheater",
        "landmarks",
        "landmark-crater-amphitheater",
        90.0,
        Ground::Relief(&CRATER),
        (0.485, 0.492),
        0.094,
        Z,
        false,
        "crater_amphitheater"
    ),
    model!(
        "Quarry rim",
        "landmarks",
        "landmark-quarry-pit",
        110.0,
        Ground::Relief(&QUARRY_RIM),
        (0.491, 0.417),
        0.288,
        Z,
        true,
        "quarry_pit"
    ),
    model!(
        "The Golden Sphere",
        "landmarks",
        "landmark-core",
        120.0,
        Ground::Relief(&GOLDEN_SPHERE),
        (0.446, 0.49),
        0.425,
        Z,
        true,
        "core"
    ),
];

const TRAIN_SCALE: f32 = 40.0;
const TRAIN_PAD: f32 = 32.0;
/// The embankment the freight train came off, and where the train lies.
const DERAILED_RAIL: usize = 25;
const DERAILED_TRAIN_CENTER: Vec2 = Vec2::new(114.3, -641.5);
const TRAIN_YAW_OFFSET: f32 = 0.18;
const TRAIN_ROLL: f32 = 0.10;
const TRAIN_LIFT: f32 = 0.45;
/// How far below a relief's surface the terrain lies, so it never shows through.
const RELIEF_SINK: f32 = 0.12;
/// A site's full scan streams in within this distance of its footprint, and
/// out again once the stalker and the view are this much farther.
const FULL_SCAN_RANGE: f32 = 150.0;
const FULL_SCAN_KEEP: f32 = 60.0;
/// Width of the slope from a level pad back to the land.
const PAD_BLEND: f32 = 26.0;

#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
const TRAIN_PILLARS: &[Pillar] = include!("pillars/vehicle_train.rs");

/// The quarry's haul road in the model's units, from the front edge's middle.
#[allow(clippy::approx_constant, reason = "Measured model coordinates")]
const QUARRY_RAMP: &[Vec2] = include!("pillars/quarry_ramp.rs");

#[derive(Clone, Copy)]
pub(super) struct Landmark {
    kind: Kind,
    pub center: Vec2,
    pub junction: Vec2,
}

impl Landmark {
    const fn new(kind: Kind, center: Vec2, junction: Vec2) -> Self {
        Self {
            kind,
            center,
            junction,
        }
    }

    fn model(self) -> &'static Model {
        &MODELS[self.kind as usize]
    }

    /// Inside the Zone, beyond the Perimeter (rather than in Harmont).
    pub fn in_zone(self) -> bool {
        cordon::in_zone(self.center)
    }

    fn toward_road(self) -> Vec2 {
        (self.junction - self.center).normalize()
    }

    fn rotation(self) -> Quat {
        let front_angle = match self.model().front {
            Front::X => 0.0,
            Front::Z => FRAC_PI_2,
        };
        let facing = self.toward_road();
        Quat::from_rotation_y(front_angle - facing.y.atan2(facing.x))
    }

    pub fn entrance(self) -> Vec2 {
        let m = self.model();
        self.center + self.toward_road() * (m.front_extent() + 7.0)
    }

    /// Radius of the level pad around a building.
    fn pad(self) -> f32 {
        self.model().radius() + 9.0
    }

    /// Radius kept clear of trees, plants and creatures.
    fn footprint(self) -> f32 {
        match self.model().ground {
            Ground::Level(_) => self.pad(),
            Ground::Relief(_) => self.model().radius(),
        }
    }

    /// Height of the model's origin: a building's datum on the land at its
    /// center, a diorama's front edge level with the land at its entrance.
    fn base_height(self) -> f32 {
        let m = self.model();
        let anchor = match m.ground {
            Ground::Level(_) => self.center,
            Ground::Relief(_) => self.entrance(),
        };
        landform_height(anchor.x, anchor.y) - m.datum() * m.scale
    }

    fn pose(self) -> Transform {
        Transform::from_xyz(self.center.x, self.base_height(), self.center.y)
            .with_rotation(self.rotation())
            .with_scale(self.model().size())
    }

    /// A world point in model units.
    fn to_local(self, at: Vec2) -> Vec2 {
        let offset = (at - self.center).extend(0.0).xzy();
        (self.rotation().inverse() * offset).xz() / self.model().size().xz()
    }

    fn contains(self, at: Vec2) -> bool {
        match self.model().ground {
            Ground::Level(_) => at.distance_squared(self.center) < self.pad().powi(2),
            Ground::Relief(_) => self.to_local(at).abs().cmple(self.model().half).all(),
        }
    }

    /// The diorama's own ground under `at`, if it stands on it.
    fn diorama_ground(self, at: Vec2) -> Option<f32> {
        let m = self.model();
        let Ground::Relief(relief) = m.ground else {
            return None;
        };
        if at.distance_squared(self.center) > m.radius().powi(2) {
            return None;
        }
        let local = self.to_local(at);
        local
            .abs()
            .cmple(m.half)
            .all()
            .then(|| self.base_height() + relief.height(local) * m.scale - RELIEF_SINK)
    }

    /// The terrain height this place asks for at `at`, and how strongly.
    fn foundation(self, at: Vec2) -> Option<(f32, f32)> {
        let m = self.model();
        match m.ground {
            Ground::Level(_) => {
                let pad = self.pad();
                let distance = at.distance(self.center);
                (distance < pad + PAD_BLEND).then(|| {
                    (
                        landform_height(self.center.x, self.center.y),
                        smoothstep(pad + PAD_BLEND, pad, distance),
                    )
                })
            }
            Ground::Relief(relief) => {
                let reach = m.radius() + m.relief_margin();
                if at.distance_squared(self.center) >= reach * reach {
                    return None;
                }
                let local = self.to_local(at);
                let outside = (local.abs() - m.half).max(Vec2::ZERO).length() * m.scale;
                (outside < m.relief_margin()).then(|| {
                    let ground = relief.blurred(local.clamp(-m.half, m.half), outside / m.scale);
                    (
                        self.base_height() + ground * m.scale - RELIEF_SINK,
                        smoothstep(m.relief_margin(), 0.0, outside),
                    )
                })
            }
        }
    }
}

use Kind::*;

/// Every site where the Harmont map shows it (`map` pixels), facing the road
/// it is reached from. New Harmont itself is the town (`town.rs`).
pub(super) const LANDMARKS: [Landmark; 32] = [
    // Harmont, on the hangar's side of the Perimeter.
    Landmark::new(Institute, map(349.5, 170.5), roads::INSTITUTE),
    Landmark::new(OldBorscht, map(264.0, 561.5), roads::OLD_BORSCHT),
    Landmark::new(Borscht, map(223.5, 608.0), roads::BORSCHT),
    Landmark::new(PlagueQuarter, map(195.0, 425.5), roads::PLAGUE),
    Landmark::new(SuburbanHouse, map(150.0, 429.0), roads::HOUSES),
    Landmark::new(SuburbanHouse, map(150.0, 451.0), roads::HOUSES),
    Landmark::new(Schoolhouse, map(250.0, 450.0), roads::SCHOOL),
    Landmark::new(Cemetery, map(200.0, 636.0), roads::CEMETERY),
    Landmark::new(MotorPool, map(344.5, 300.0), roads::MOTOR_POOL),
    // The Zone, from the Perimeter to the quarry.
    Landmark::new(AbandonedSuburbs, map(555.0, 561.0), roads::SUBURBS),
    Landmark::new(Cemetery, map(560.0, 481.5), roads::SUBURBS_CEMETERY),
    Landmark::new(AbandonedSuburbs, map(535.0, 229.0), roads::NORTH_SUBURBS),
    Landmark::new(SuburbanHouse, map(520.0, 329.0), roads::NORTH_HOUSES),
    Landmark::new(SuburbanHouse, map(532.0, 352.0), roads::NORTH_HOUSES),
    Landmark::new(Highway, map(607.0, 367.0), roads::HIGHWAY),
    Landmark::new(OldFactory, map(650.5, 282.0), roads::FACTORY),
    Landmark::new(BrickFactory, map(698.0, 257.5), roads::BRICKWORKS),
    Landmark::new(Silos, map(634.5, 318.0), roads::SILOS),
    Landmark::new(Substation, map(739.0, 290.0), roads::SUBSTATION),
    Landmark::new(Garage, map(791.0, 307.0), roads::GARAGE),
    Landmark::new(AbandonedSubstation, map(803.0, 341.5), roads::RAIL_NORTH),
    Landmark::new(Railway, map(821.0, 351.5), roads::RAIL_NORTH),
    Landmark::new(Locomotive, map(927.5, 153.0), roads::LOCOMOTIVE),
    Landmark::new(AbandonedSubstation, map(910.5, 145.5), roads::LOCOMOTIVE),
    Landmark::new(Marsh, map(877.0, 451.0), roads::MARSH),
    Landmark::new(Railway, map(665.0, 652.5), roads::RAIL_SOUTH),
    Landmark::new(Railway, map(907.0, 435.0), roads::EMBANKMENT),
    Landmark::new(SwampFlats, map(775.0, 673.0), roads::SWAMP),
    Landmark::new(Crater, map(880.0, 724.0), roads::CRATER),
    Landmark::new(Ridge, map(1039.5, 395.0), roads::RIDGE),
    // Back from the track: the pit reaches the scan's front edge.
    Landmark::new(QuarryRim, map(1142.0, 320.0), roads::QUARRY),
    Landmark::new(GoldenSphere, map(1216.5, 348.0), roads::GOLDEN_SPHERE),
];

/// The two helipads on the International Institute's roof, in world meters:
/// on its two wings (flat at 0.70 model units, `pillars/uhd_institute.rs`),
/// where a rotor clears the central towers.
pub(crate) fn institute_helipads() -> [Vec3; 2] {
    let pose = institute().pose();
    [-1.0, 1.0].map(|side| pose.transform_point(Vec3::new(0.255 * side, 0.70, -0.10)))
}

/// The height of the Institute's highest point, in world meters.
pub(crate) fn institute_top() -> f32 {
    let site = institute();
    site.pose().transform_point(Vec3::Y * site.model().top).y
}

/// The Institute's model-to-world pose, scale included.
pub(super) fn institute_pose() -> Transform {
    institute().pose()
}

fn institute() -> Landmark {
    landmark(Institute)
}

fn landmark(kind: Kind) -> Landmark {
    LANDMARKS
        .into_iter()
        .find(|site| site.kind == kind)
        .expect("every kind of site is a landmark")
}

/// The quarry's haul road in world xz: its drive from the junction to the
/// entrance, then the baked path (`tools/plan_quarry_ramp.py`) down into the pit.
pub(super) fn quarry_ramp() -> Vec<Vec2> {
    let site = landmark(QuarryRim);
    let pose = site.pose();
    let path = QUARRY_RAMP
        .iter()
        .map(|at| pose.transform_point(Vec3::new(at.x, 0.0, at.y)).xz());
    [site.junction, site.entrance()]
        .into_iter()
        .chain(path)
        .collect()
}

/// Every site and the derailed train as a rough upright cylinder around the
/// model: center, radius, height and what it is.
pub(super) fn outlines() -> impl Iterator<Item = (Vec2, f32, f32, Place)> {
    LANDMARKS
        .into_iter()
        .map(|site| {
            let m = site.model();
            (
                site.center,
                m.half_meters().max_element(),
                (m.top - m.datum()).max(0.05) * m.scale,
                Place::Site(site.kind),
            )
        })
        .chain([(DERAILED_TRAIN_CENTER, 21.0, 5.0, Place::DerailedTrain)])
}

/// The site or train whose ground holds `at`.
pub(super) fn place_at(at: Vec2) -> Option<Place> {
    if at.distance_squared(DERAILED_TRAIN_CENTER) < TRAIN_PAD.powi(2) {
        return Some(Place::DerailedTrain);
    }
    LANDMARKS
        .into_iter()
        .find(|site| site.contains(at))
        .map(|site| Place::Site(site.kind))
}

/// The terrain height the strongest foundation asks for at `at`, and its weight.
/// The ground of the diorama standing at `at`, which roads and water leave as
/// it is.
pub(super) fn diorama_ground(at: Vec2) -> Option<f32> {
    LANDMARKS
        .into_iter()
        .find_map(|site| site.diorama_ground(at))
}

pub(super) fn foundation(at: Vec2) -> Option<(f32, f32)> {
    let rail = LANDMARKS[DERAILED_RAIL].center;
    let outer = TRAIN_PAD + PAD_BLEND;
    let distance = at.distance(DERAILED_TRAIN_CENTER);
    let train = (distance < outer).then(|| {
        (
            landform_height(rail.x, rail.y),
            smoothstep(outer, TRAIN_PAD, distance),
        )
    });
    LANDMARKS
        .into_iter()
        .filter_map(|site| site.foundation(at))
        .chain(train)
        .chain(town::foundation(at))
        .reduce(|best, next| if next.1 > best.1 { next } else { best })
}

pub(super) fn clear_footprint(at: Vec2, clearance: f32) -> bool {
    town::clear_footprint(at, clearance)
        || at.distance_squared(DERAILED_TRAIN_CENTER) < (TRAIN_PAD + clearance).powi(2)
        || LANDMARKS
            .into_iter()
            .any(|site| at.distance_squared(site.center) < (site.footprint() + clearance).powi(2))
}

pub(super) fn grove(at: Vec2) -> f32 {
    LANDMARKS.into_iter().fold(0.0_f32, |density, site| {
        let radius = site.footprint() + 52.0;
        let distance = at.distance_squared(site.center);
        density.max(smoothstep(
            radius * radius,
            (radius * 0.65).powi(2),
            distance,
        ))
    })
}

/// The sites' full scans, for the RAM fill: nearest the hangar first, so a
/// scan several sites share comes up at its nearest.
pub(super) fn uhd_files() -> impl Iterator<Item = &'static str> {
    let hangar = Vec2::from(BASE);
    let mut sites = LANDMARKS;
    sites.sort_by(|a, b| {
        let near = |site: &Landmark| site.center.distance_squared(hangar);
        near(a).total_cmp(&near(b))
    });
    sites.into_iter().map(|site| site.model().path)
}

/// How far from the hangar the nearest site showing the scan at `path` stands.
#[cfg(test)]
pub(super) fn distance_from_base(path: &str) -> Option<f32> {
    let hangar = Vec2::from(BASE);
    LANDMARKS
        .iter()
        .filter(|site| site.model().path == path)
        .map(|site| site.center.distance(hangar))
        .min_by(f32::total_cmp)
}

/// Preloads the sites' far stand-ins; their full scans stream in later.
pub(super) fn preload_sites(server: Res<AssetServer>, mut preloaded: ResMut<PreloadedAssets>) {
    for model in &MODELS {
        let (mesh, paint) = assets::load_single_primitive(&server, model.far);
        preloaded.0.extend([mesh.untyped(), paint.untyped()]);
    }
    let (mesh, paint) = assets::load_single_primitive(&server, assets::VEHICLE_TRAIN);
    preloaded.0.extend([mesh.untyped(), paint.untyped()]);
}

pub(super) fn spawn_sites(
    mut commands: Commands,
    server: Res<AssetServer>,
    paints: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let models = MODELS.each_ref().map(|model| {
        let (mesh, paint) = assets::load_single_primitive(&server, model.far);
        let paint = paints.get(&paint).expect("preloaded landmark material");
        (
            mesh,
            materials.add(standard_material_from_gltf_material(paint)),
        )
    });
    for (index, site) in LANDMARKS.into_iter().enumerate() {
        let (mesh, paint) = &models[site.kind as usize];
        commands
            .spawn((
                WorldRoot,
                Name::new(site.model().name),
                SiteDisplay { index },
                site.pose(),
                Visibility::default(),
            ))
            .with_child((FarSite, Mesh3d(mesh.clone()), MeshMaterial3d(paint.clone())));
    }
    let (mesh, paint) = assets::load_single_primitive(&server, assets::VEHICLE_TRAIN);
    let paint = paints.get(&paint).expect("preloaded train material");
    commands.spawn((
        WorldRoot,
        Name::new("Derailed train"),
        Mesh3d(mesh),
        MeshMaterial3d(materials.add(standard_material_from_gltf_material(paint))),
        train_pose(),
        detail_range(385.0, 45.0),
    ));
}

/// A site, showing its far stand-in until its full scan has streamed in.
#[derive(Component)]
pub(super) struct SiteDisplay {
    index: usize,
}

/// A site's light stand-in.
#[derive(Component)]
pub(super) struct FarSite;

/// A site's full UHD scan, while a stream anchor is near.
#[derive(Component)]
pub(super) struct FullSite {
    display: Entity,
}

/// Shows each site's full scan while the stalker or the view is near it and
/// frees it once they leave: the scans together outgrow video memory.
///
/// A scan is wanted from `UPLOAD_LEAD` before it shows, so it is on the GPU by
/// the time an anchor is within reach, and it shows only once it is. Until
/// then the stand-in stays. A scan that leaves the GPU takes its instance with
/// it, which is when the GPU frees it.
pub(super) fn stream_full_sites(
    mut commands: Commands,
    mut scans: ResMut<GpuScans>,
    anchors: Query<&Transform, With<StreamAnchor>>,
    displays: Query<(Entity, &SiteDisplay)>,
    fulls: Query<(Entity, &FullSite)>,
    mut fars: Query<(&ChildOf, &mut Visibility), With<FarSite>>,
) {
    for (display, site) in &displays {
        let landmark = LANDMARKS[site.index];
        let path = landmark.model().path;
        let reach = landmark.model().radius() + FULL_SCAN_RANGE;
        let nearest = anchors
            .iter()
            .map(|anchor| anchor.translation.xz().distance(landmark.center))
            .fold(f32::INFINITY, f32::min);
        let full = fulls.iter().find(|(_, full)| full.display == display);
        let distance = nearest - reach;
        if full.is_some() {
            // Shown, and kept up to `FULL_SCAN_KEEP` past the reach.
            scans.want(path, distance.min(0.0));
        } else if distance < UPLOAD_LEAD {
            scans.want(path, distance);
        }
        match full {
            None if nearest < reach => {
                if let Some(scene) = scans.scene(path) {
                    commands
                        .spawn((
                            FullSite { display },
                            ChildOf(display),
                            WorldAssetRoot(scene),
                            Visibility::Hidden,
                        ))
                        .observe(reveal_full_site);
                }
            }
            // Dropping the instance releases its hold on the scan's GPU copy,
            // which an evicted scan frees only then.
            Some((entity, _))
                if nearest > reach + FULL_SCAN_KEEP || scans.scene(path).is_none() =>
            {
                commands.entity(entity).despawn();
                for (parent, mut visibility) in &mut fars {
                    if parent.parent() == display {
                        *visibility = Visibility::Inherited;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Swaps the stand-in for the full scan once it has spawned.
fn reveal_full_site(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    fulls: Query<&FullSite>,
    mut fars: Query<(&ChildOf, &mut Visibility), With<FarSite>>,
) {
    let Ok(full) = fulls.get(ready.entity) else {
        return;
    };
    commands.entity(ready.entity).insert(Visibility::Inherited);
    for (parent, mut visibility) in &mut fars {
        if parent.parent() == full.display {
            *visibility = Visibility::Hidden;
        }
    }
}

pub(super) fn spawn_solids(parent: &mut ChildSpawnerCommands, tile: IVec2) {
    for site in LANDMARKS {
        if (site.center / TILE_SIZE).floor().as_ivec2() == tile {
            parent.spawn((site.pose(), Collider(site.model().pillars)));
        }
    }
    if (DERAILED_TRAIN_CENTER / TILE_SIZE).floor().as_ivec2() == tile {
        parent.spawn((train_pose(), Collider(TRAIN_PILLARS)));
    }
}

/// The train lies along the embankment it left, listing toward it.
fn train_pose() -> Transform {
    let rail = LANDMARKS[DERAILED_RAIL];
    let track = rail.toward_road().perp();
    let yaw = -track.y.atan2(track.x) + TRAIN_YAW_OFFSET;
    Transform::from_xyz(
        DERAILED_TRAIN_CENTER.x,
        landform_height(rail.center.x, rail.center.y) + TRAIN_LIFT,
        DERAILED_TRAIN_CENTER.y,
    )
    .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(TRAIN_ROLL))
    .with_scale(Vec3::splat(TRAIN_SCALE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Body, STEP_HEIGHT, TERRAIN_HALF_SIZE, Terrain, push_out};

    /// Points across a site's footprint, in world space.
    fn footprint_samples(site: Landmark) -> impl Iterator<Item = Vec2> {
        let pose = site.pose();
        let half = site.model().half;
        [-0.9, 0.0, 0.9].into_iter().flat_map(move |x| {
            [-0.9, 0.0, 0.9].into_iter().map(move |z| {
                pose.transform_point(Vec3::new(x * half.x, 0.0, z * half.y))
                    .xz()
            })
        })
    }

    #[test]
    fn every_place_stands_dry_on_its_own_ground_by_a_road() {
        let terrain = Terrain::new();
        for (i, site) in LANDMARKS.into_iter().enumerate() {
            let kind = site.kind;
            assert!(site.center.abs().max_element() < TERRAIN_HALF_SIZE - 110.0);
            for other in &LANDMARKS[..i] {
                let gap = site.center.distance(other.center)
                    - site.model().radius()
                    - other.model().radius();
                assert!(
                    gap > 8.0,
                    "{kind:?} at {} crowds {:?}",
                    site.center,
                    other.kind
                );
            }
            let pose = site.pose();
            for at in footprint_samples(site) {
                assert_eq!(
                    terrain.water_depth_at(at.x, at.y),
                    0.0,
                    "{kind:?} at {} flooded at {at}",
                    site.center
                );
                // A ramp is cut into the ground it crosses.
                if terrain.roads.sample(at).is_some_and(|road| road.is_ramp()) {
                    continue;
                }
                let ground = terrain.height_at(at.x, at.y);
                let expected = match site.model().ground {
                    Ground::Level(datum) => pose.translation.y + datum * site.model().scale,
                    Ground::Relief(relief) => {
                        pose.translation.y + relief.height(site.to_local(at)) * site.model().scale
                            - RELIEF_SINK
                    }
                };
                assert!(
                    (ground - expected).abs() < 0.05,
                    "{kind:?} at {}: ground {ground} vs model {expected} at {at}",
                    site.center
                );
            }
            let entrance = site.entrance();
            assert!(
                terrain.road_mask(entrance.x, entrance.y) > 0.99,
                "{kind:?} entrance {entrance} is off the road"
            );
        }
    }

    #[test]
    fn harmont_and_the_zone_lie_on_their_own_sides_of_the_perimeter() {
        for site in LANDMARKS {
            let harmont = matches!(
                site.kind,
                Institute | Borscht | OldBorscht | PlagueQuarter | Schoolhouse | MotorPool
            );
            let margin = site.footprint() + 20.0;
            let clear = (site.center.x - cordon::PERIMETER_X).abs() > margin;
            assert!(clear, "{:?} straddles the Perimeter", site.kind);
            assert!(!(harmont && site.in_zone()), "{:?}", site.kind);
        }
    }

    #[test]
    fn landmarks_keep_the_order_of_the_harmont_map() {
        let at = |kind| {
            LANDMARKS
                .iter()
                .find(|site| site.kind == kind)
                .map(|site| site.center)
                .expect("every map landmark is placed")
        };
        // From Harmont into the Zone (toward -X), as the stalkers walk it.
        let walk = [
            Borscht,
            Institute,
            AbandonedSuburbs,
            OldFactory,
            Garage,
            Railway,
            Ridge,
            QuarryRim,
            GoldenSphere,
        ];
        for pair in walk.windows(2) {
            assert!(
                at(pair[0]).x > at(pair[1]).x,
                "{:?} before {:?}",
                pair[0],
                pair[1]
            );
        }
        // Harmont from north to south, and the Marsh south of the highway.
        assert!(at(Institute).y > at(PlagueQuarter).y);
        assert!(at(PlagueQuarter).y > at(Borscht).y);
        assert!(at(Marsh).y < at(Highway).y - 150.0);
        assert!(at(OldFactory).y > at(Highway).y);
        // The Institute stands north of the hangar, on Harmont's side.
        assert!(at(Institute).x > cordon::PERIMETER_X && at(Institute).y > 400.0);
        for kind in [Institute, Borscht, PlagueQuarter, Garage, Marsh, Ridge] {
            assert!(kind.is_landmark());
        }
    }

    #[test]
    fn buildings_stand_solid_above_their_pad() {
        let terrain = Terrain::new();
        let body = Body {
            radius: 0.35,
            height: 1.8,
        };
        for site in LANDMARKS {
            let m = site.model();
            let Ground::Level(datum) = m.ground else {
                continue;
            };
            assert!(
                m.pillars
                    .iter()
                    .any(|pillar| (pillar.top - datum) * m.scale > STEP_HEIGHT + body.height),
                "{:?} has no solid wall",
                site.kind
            );
            let pose = site.pose();
            let global = GlobalTransform::from(pose);
            let collider = Collider(m.pillars);
            let feet = terrain.mesh_height_at(site.center);
            let blocked = m.pillars.iter().any(|pillar| {
                let at = pose
                    .transform_point(Vec3::new(pillar.x, 0.0, pillar.z))
                    .xz();
                push_out(at, feet, body, [(&collider, &global)]) != at
            });
            assert!(blocked, "{:?} lets the stalker walk through", site.kind);
        }
    }

    #[test]
    fn the_train_lies_on_a_level_pad_beside_its_embankment() {
        let terrain = Terrain::new();
        let rail = LANDMARKS[DERAILED_RAIL];
        assert_eq!(rail.kind, Railway);
        let gap = DERAILED_TRAIN_CENTER.distance(rail.center);
        assert!((15.0..25.0).contains(&gap), "train {gap} m from its rail");
        let (_, weight) = foundation(DERAILED_TRAIN_CENTER).expect("train pad");
        assert_eq!(weight, 1.0);
        let pose = train_pose();
        assert!(pose.translation.y > terrain.mesh_height_at(DERAILED_TRAIN_CENTER));
        assert!(TRAIN_PILLARS.len() > 100);
        assert!(clear_footprint(DERAILED_TRAIN_CENTER, 0.0));
    }

    #[test]
    fn relief_sampling_matches_the_grid() {
        let relief = &PLAGUE;
        let at = relief.min + relief.cell * Vec2::new(10.5, 20.5);
        assert!((relief.height(at) - relief.heights[20 * RELIEF_SIZE + 10]).abs() < 1e-6);
        let far = relief.min - Vec2::splat(5.0);
        assert_eq!(relief.height(far), relief.heights[0]);
    }

    /// The scan of the first site, which `streaming_site` follows.
    fn first_scan() -> &'static str {
        LANDMARKS[0].model().path
    }

    /// An app streaming the scans of the sites at `indices` on a GPU holding
    /// `scans`, with each site's stand-in and a stream anchor far from them.
    fn streaming_sites(scans: GpuScans, indices: &[usize]) -> (App, Entity, Vec<Entity>) {
        let mut app = App::new();
        app.insert_resource(scans)
            .add_systems(Update, stream_full_sites);
        let anchor = app
            .world_mut()
            .spawn((StreamAnchor, Transform::from_xyz(1e6, 0.0, 1e6)))
            .id();
        let fars = indices
            .iter()
            .map(|&index| {
                let site = app.world_mut().spawn(SiteDisplay { index }).id();
                app.world_mut()
                    .spawn((FarSite, Visibility::Inherited, ChildOf(site)))
                    .id()
            })
            .collect();
        (app, anchor, fars)
    }

    /// An app streaming the first site's scan, with the site's stand-in.
    fn streaming_site(scans: GpuScans) -> (App, Entity, Entity) {
        let (app, anchor, fars) = streaming_sites(scans, &[0]);
        (app, anchor, fars[0])
    }

    /// The two evacuated houses by the Plague Quarter, which show one scan.
    fn house_pair() -> [usize; 2] {
        let mut houses = LANDMARKS
            .iter()
            .enumerate()
            .filter(|(_, site)| site.kind == SuburbanHouse)
            .map(|(index, _)| index);
        let mut next = || houses.next().expect("the Plague Quarter has two houses");
        let pair = [next(), next()];
        assert_eq!(
            LANDMARKS[pair[0]].model().path,
            LANDMARKS[pair[1]].model().path
        );
        pair
    }

    /// Stands the anchor `metres` from the first house of `pair`, on the line
    /// from the second house through it: both houses are within reach.
    fn stand_by_houses(app: &mut App, anchor: Entity, pair: [usize; 2], metres: f32) {
        let [near, far] = pair.map(|index| LANDMARKS[index].center);
        let at = near + (near - far).normalize() * metres;
        app.world_mut()
            .entity_mut(anchor)
            .insert(Transform::from_xyz(at.x, 0.0, at.y));
    }

    /// Stands the anchor `offset` metres from the reach of the first site's
    /// scan: outside it when positive, inside it when negative.
    fn stand_from_reach(app: &mut App, anchor: Entity, offset: f32) {
        let landmark = LANDMARKS[0];
        let reach = landmark.model().radius() + FULL_SCAN_RANGE;
        let at = landmark.center + Vec2::X * (reach + offset);
        app.world_mut()
            .entity_mut(anchor)
            .insert(Transform::from_xyz(at.x, 0.0, at.y));
    }

    fn full_sites(app: &mut App) -> Vec<Entity> {
        let world = app.world_mut();
        let mut fulls = world.query_filtered::<Entity, With<FullSite>>();
        fulls.iter(world).collect()
    }

    fn stand_in_shows(app: &App, far: Entity) -> bool {
        let visibility = app.world().entity(far).get::<Visibility>();
        visibility == Some(&Visibility::Inherited)
    }

    #[test]
    fn a_site_asks_for_its_scan_40_m_before_it_shows() {
        let (mut app, anchor, _) = streaming_site(GpuScans::new(1 << 30));
        stand_from_reach(&mut app, anchor, 30.0);
        app.update();
        assert!(full_sites(&mut app).is_empty(), "out of reach");
        let plan = app.world_mut().resource_mut::<GpuScans>().plan(0.0);
        assert_eq!(plan.load, [first_scan()]);
    }

    #[test]
    fn a_site_does_not_ask_for_its_scan_50_m_before_it_shows() {
        let (mut app, anchor, _) = streaming_site(GpuScans::new(1 << 30));
        stand_from_reach(&mut app, anchor, 50.0);
        app.update();
        let plan = app.world_mut().resource_mut::<GpuScans>().plan(0.0);
        assert!(plan.load.is_empty(), "{:?}", plan.load);
    }

    #[test]
    fn a_resident_scan_waits_for_the_anchor_to_come_within_reach() {
        let mut scans = GpuScans::new(1 << 30);
        scans.insert_resident(first_scan(), Handle::default());
        let (mut app, anchor, far) = streaming_site(scans);
        stand_from_reach(&mut app, anchor, 30.0);
        app.update();
        assert!(full_sites(&mut app).is_empty(), "out of reach");
        assert!(stand_in_shows(&app, far));
        stand_from_reach(&mut app, anchor, -10.0);
        app.update();
        assert_eq!(full_sites(&mut app).len(), 1);
    }

    #[test]
    fn a_site_shows_its_scan_once_on_the_gpu_and_keeps_wanting_it() {
        let (mut app, anchor, far) = streaming_site(GpuScans::new(1 << 30));
        stand_from_reach(&mut app, anchor, -10.0);
        app.update();
        assert!(
            full_sites(&mut app).is_empty(),
            "the scan is not on the GPU"
        );
        assert!(stand_in_shows(&app, far));

        let mut scans = app.world_mut().resource_mut::<GpuScans>();
        scans.insert_resident(first_scan(), Handle::default());
        app.update();
        assert_eq!(full_sites(&mut app).len(), 1);
        // The scan has loaded and stands in for the site.
        app.world_mut().entity_mut(far).insert(Visibility::Hidden);

        // A plan forgets the asks so far, so the next stamp shows whether the
        // site asks again past its reach, where the margin keeps its instance.
        let mut scans = app.world_mut().resource_mut::<GpuScans>();
        scans.plan(5.0);
        assert_eq!(scans.last_shown(first_scan()), Some(5.0));
        stand_from_reach(&mut app, anchor, 55.0);
        app.update();
        assert_eq!(full_sites(&mut app).len(), 1, "kept inside the margin");
        let mut scans = app.world_mut().resource_mut::<GpuScans>();
        assert!(scans.plan(10.0).load.is_empty(), "it is loaded already");
        assert_eq!(scans.last_shown(first_scan()), Some(10.0), "still shown");

        stand_from_reach(&mut app, anchor, 61.0);
        app.update();
        assert!(full_sites(&mut app).is_empty(), "dropped past the margin");
        assert!(stand_in_shows(&app, far));
    }

    #[test]
    fn a_scan_that_leaves_the_gpu_takes_its_instance_and_shows_the_stand_in_again() {
        let mut scans = GpuScans::new(1 << 30);
        scans.insert_resident(first_scan(), Handle::default());
        let (mut app, anchor, far) = streaming_site(scans);
        stand_from_reach(&mut app, anchor, -10.0);
        app.update();
        assert_eq!(full_sites(&mut app).len(), 1);
        // The scan has loaded and stands in for the site.
        app.world_mut().entity_mut(far).insert(Visibility::Hidden);

        // Eviction: nothing is on the GPU any more.
        app.insert_resource(GpuScans::new(1 << 30));
        app.update();

        assert!(full_sites(&mut app).is_empty(), "frees its VRAM");
        assert!(stand_in_shows(&app, far));
        let plan = app.world_mut().resource_mut::<GpuScans>().plan(0.0);
        assert_eq!(plan.load, [first_scan()], "the scan is wanted back");
    }

    #[test]
    fn sites_sharing_a_scan_ask_for_it_at_the_nearest_ones_distance() {
        let pair @ [near, far] = house_pair();
        let shared = LANDMARKS[near].model().path;
        // The card holds one of the two scans: the nearer ask loads.
        let mut scans = GpuScans::new(250 << 20);
        scans.estimate(shared, 100 << 20);
        scans.estimate("rival", 100 << 20);
        let (mut app, anchor, _) = streaming_sites(scans, &pair);
        stand_by_houses(&mut app, anchor, pair, 50.0);
        // The rival asks from between the two houses' distances to their reach.
        let reach = |index: usize| LANDMARKS[index].model().radius() + FULL_SCAN_RANGE;
        let gap = LANDMARKS[near].center.distance(LANDMARKS[far].center);
        assert!(50.0 + gap < reach(far), "both houses are within reach");
        let rival = 50.0 + gap / 2.0 - reach(near);
        let mut scans = app.world_mut().resource_mut::<GpuScans>();
        scans.want("rival", rival);
        app.update();
        let plan = app.world_mut().resource_mut::<GpuScans>().plan(0.0);
        assert_eq!(plan.load, [shared], "the nearest house's ask stands");
    }

    #[test]
    fn a_scan_that_leaves_the_gpu_takes_the_instances_of_all_sites_sharing_it() {
        let pair = house_pair();
        let mut scans = GpuScans::new(1 << 30);
        scans.insert_resident(LANDMARKS[pair[0]].model().path, Handle::default());
        let (mut app, anchor, fars) = streaming_sites(scans, &pair);
        stand_by_houses(&mut app, anchor, pair, 50.0);
        app.update();
        assert_eq!(full_sites(&mut app).len(), 2, "both houses show it");
        // The scan has loaded and stands in for both.
        for &far in &fars {
            app.world_mut().entity_mut(far).insert(Visibility::Hidden);
        }

        // Eviction: nothing is on the GPU any more.
        app.insert_resource(GpuScans::new(1 << 30));
        app.update();

        assert!(full_sites(&mut app).is_empty(), "frees its VRAM");
        assert!(fars.iter().all(|&far| stand_in_shows(&app, far)));
    }
}
