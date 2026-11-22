//! What lies at a point of the Zone, for hover descriptions: sites and
//! landmarks, buildings, water and landforms, and the things described where
//! they stand. The stalker surveys the map as they travel; places further out
//! stay unexplored.

use bevy::prelude::*;

use crate::player::Player;

use super::cordon;
use super::hydrology::{Water, Wetlands};
use super::sites::{self, Kind as SiteKind};
use super::terrain::{self, BASE, HALF_SIZE, Terrain};
use super::town;
use super::wildlife::Kind as CreatureKind;

/// A named place or thing the map can describe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Site(SiteKind),
    DerailedTrain,
    Hangar,
    Checkpoint,
    Yard,
    Town,
    Quarry,
    BaldHill,
    StagnantFlats,
    Swamp,
    Lake,
    Coast,
    Creature(CreatureKind),
    Vehicle,
    /// A UN soldier guarding a way through a cordon.
    Guard,
    /// The UN armored cars patrolling the Perimeter.
    Patrol,
    /// Harmont's street sweeper.
    Sweeper,
    /// A Harmont sedan in town traffic.
    Sedan,
    /// The hole dug under the Perimeter, described only up close.
    CrawlHole,
    Artifact(usize),
}

/// Describes its entity where it stands, within an upright span of `height`
/// meters above its origin and `radius` meters around it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Describe {
    pub place: Place,
    pub radius: f32,
    pub height: f32,
}

const HALL_HALF_EXTENTS: Vec2 = Vec2::new(19.0, 21.0);
/// The hall's walls as the tactical view shows them, with the roof cut away.
const HALL_HEIGHT: f32 = 7.0;
const CHECKPOINT_RADIUS: f32 = 7.0;
const CHECKPOINT_HEIGHT: f32 = 9.0;
const CHECKPOINTS: [Vec2; 2] = [cordon::CHECKPOINT, cordon::PERIMETER_CHECKPOINT];

/// How far around them the stalker surveys the land, in meters.
pub const SURVEY_RANGE: f32 = 150.0;
pub const SURVEY_CELL: f32 = 32.0;
pub(super) const SURVEY_CELLS: usize = (2.0 * HALF_SIZE / SURVEY_CELL) as usize + 1;

/// The named place on the ground at `at`.
pub fn place_at(at: Vec2) -> Option<Place> {
    if (at - Vec2::from(BASE)).abs().cmple(HALL_HALF_EXTENTS).all() {
        return Some(Place::Hangar);
    }
    if CHECKPOINTS
        .into_iter()
        .any(|checkpoint| at.distance(checkpoint) < CHECKPOINT_RADIUS)
    {
        return Some(Place::Checkpoint);
    }
    if let Some(place) = sites::place_at(at) {
        return Some(place);
    }
    if let Some(water) = Wetlands::at(at.x, at.y).water() {
        return Some(match water {
            Water::Lake => Place::Lake,
            Water::Swamp => Place::Swamp,
            Water::Coast => Place::Coast,
        });
    }
    if cordon::inside_yard(at) {
        return Some(Place::Yard);
    }
    if town::contains(at) {
        return Some(Place::Town);
    }
    landform(at)
}

fn landform(at: Vec2) -> Option<Place> {
    let quarry = (at - Vec2::from(terrain::QUARRY_CENTER)) / Vec2::from(terrain::QUARRY_RADII);
    if quarry.length() < 1.1 {
        Some(Place::Quarry)
    } else if at.distance(Vec2::from(terrain::BALD_CENTER)) < terrain::BALD_SIGMA * 2.0 {
        Some(Place::BaldHill)
    } else if terrain::marsh_distance(at.x, at.y) < 1.0 {
        Some(Place::StagnantFlats)
    } else {
        None
    }
}

/// The nearest building or site the ray enters within `reach`: distance along
/// the ray and the place.
pub fn structure_on_ray(terrain: &Terrain, ray: Ray3d, reach: f32) -> Option<(f32, Place)> {
    let checkpoints = CHECKPOINTS.map(|checkpoint| {
        (
            checkpoint,
            CHECKPOINT_RADIUS,
            CHECKPOINT_HEIGHT,
            Place::Checkpoint,
        )
    });
    let floor = Vec2::from(BASE)
        .extend(terrain.mesh_height_at(Vec2::from(BASE)))
        .xzy();
    let corner = HALL_HALF_EXTENTS.extend(0.0).xzy();
    let hall = enter_box(ray, floor - corner, floor + corner + Vec3::Y * HALL_HEIGHT)
        .map(|t| (t, Place::Hangar));
    sites::outlines()
        .chain(town::outlines())
        .chain(checkpoints)
        .filter_map(|(center, radius, height, place)| {
            let base = center.extend(terrain.mesh_height_at(center)).xzy();
            enter_cylinder(ray, base, radius, height).map(|t| (t, place))
        })
        .chain(hall)
        .filter(|&(t, _)| t < reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Where the ray first enters an axis-aligned box (0 if it starts inside).
fn enter_box(ray: Ray3d, min: Vec3, max: Vec3) -> Option<f32> {
    let inverse = ray.direction.recip();
    let a = (min - ray.origin) * inverse;
    let b = (max - ray.origin) * inverse;
    let near = a.min(b).max_element().max(0.0);
    let far = a.max(b).min_element();
    (far >= near).then_some(near)
}

/// Where the ray first enters an upright cylinder standing on `base`.
fn enter_cylinder(ray: Ray3d, base: Vec3, radius: f32, height: f32) -> Option<f32> {
    let from = ray.origin - base;
    let toward = *ray.direction;
    let mut nearest = None::<f32>;
    let mut keep = |t: f32| {
        if t >= 0.0 && nearest.is_none_or(|best| t < best) {
            nearest = Some(t);
        }
    };
    let a = toward.xz().length_squared();
    if a > 1.0e-8 {
        let b = from.xz().dot(toward.xz());
        let c = from.xz().length_squared() - radius * radius;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let t = (-b - disc.sqrt()) / a;
            if (0.0..=height).contains(&(from.y + toward.y * t)) {
                keep(t);
            }
        }
    }
    if toward.y < 0.0 {
        let t = (height - from.y) / toward.y;
        if (from + toward * t).xz().length_squared() <= radius * radius {
            keep(t);
        }
    }
    nearest
}

/// The parts of the Zone the stalker has seen up close, on a coarse grid.
#[derive(Resource)]
pub struct Explored {
    seen: Vec<bool>,
}

impl Default for Explored {
    fn default() -> Self {
        Self {
            seen: vec![false; SURVEY_CELLS * SURVEY_CELLS],
        }
    }
}

impl Explored {
    fn cell(at: Vec2) -> Option<usize> {
        let cell = ((at + HALF_SIZE) / SURVEY_CELL).floor();
        (cell.min_element() >= 0.0 && cell.max_element() < SURVEY_CELLS as f32)
            .then(|| cell.y as usize * SURVEY_CELLS + cell.x as usize)
    }

    pub fn contains(&self, at: Vec2) -> bool {
        Self::cell(at).is_some_and(|index| self.seen[index])
    }

    /// Whether `at` lies on the map but has not been surveyed.
    pub(super) fn hides(&self, at: Vec2) -> bool {
        Self::cell(at).is_some_and(|index| !self.seen[index])
    }

    /// Every cell, row by row from the map's -X, -Z corner: surveyed or not.
    pub(crate) fn cells(&self) -> impl Iterator<Item = bool> + '_ {
        self.seen.iter().copied()
    }

    /// The survey from its cells, as [`Self::cells`] lists them; `None` if
    /// they don't cover the grid.
    pub(crate) fn from_cells(seen: Vec<bool>) -> Option<Self> {
        (seen.len() == SURVEY_CELLS * SURVEY_CELLS).then_some(Self { seen })
    }

    pub(crate) fn survey(&mut self, at: Vec2) {
        let reach = (SURVEY_RANGE / SURVEY_CELL).ceil() as i32;
        let center = ((at + HALF_SIZE) / SURVEY_CELL).floor().as_ivec2();
        for z in -reach..=reach {
            for x in -reach..=reach {
                let middle = (center + IVec2::new(x, z)).as_vec2() * SURVEY_CELL
                    + SURVEY_CELL * 0.5
                    - HALF_SIZE;
                if middle.distance(at) <= SURVEY_RANGE
                    && let Some(index) = Self::cell(middle)
                {
                    self.seen[index] = true;
                }
            }
        }
    }
}

pub(super) fn start_survey(mut commands: Commands) {
    commands.insert_resource(Explored::default());
}

pub(super) fn survey_around_stalker(
    players: Query<&Transform, With<Player>>,
    mut explored: ResMut<Explored>,
    mut last: Local<Option<IVec2>>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    let at = player.translation.xz();
    let cell = (at / SURVEY_CELL).floor().as_ivec2();
    if *last == Some(cell) && !explored.is_added() {
        return;
    }
    *last = Some(cell);
    explored.survey(at);
}

#[cfg(test)]
mod tests {
    use super::super::hydrology::SWAMP;
    use super::super::sites::LANDMARKS;
    use super::*;

    #[test]
    fn ground_points_name_the_hangar_water_and_sites() {
        assert_eq!(place_at(Vec2::from(BASE)), Some(Place::Hangar));
        let swamp = (-20..20)
            .map(|x| SWAMP + Vec2::new(x as f32 * 6.0, 0.0))
            .find_map(place_at);
        assert_eq!(swamp, Some(Place::Swamp));
        let site = LANDMARKS[0];
        assert!(matches!(place_at(site.center), Some(Place::Site(_))));
        assert_eq!(
            place_at(Vec2::from(terrain::BALD_CENTER)),
            Some(Place::BaldHill)
        );
    }

    #[test]
    fn a_ray_from_above_enters_the_site_below_it() {
        let terrain = Terrain::new();
        let site = LANDMARKS[0].center;
        let ray = Ray3d::new(site.extend(300.0).xzy(), Dir3::NEG_Y);
        let (distance, place) = structure_on_ray(&terrain, ray, 1.0e4).expect("site hit");
        assert!(matches!(place, Place::Site(_)));
        assert!(distance < 300.0 - terrain.mesh_height_at(site));
        assert_eq!(structure_on_ray(&terrain, ray, 10.0), None);
    }

    #[test]
    fn the_stalker_surveys_only_the_land_around_them() {
        let mut explored = Explored::default();
        let start = Vec2::from(terrain::SPAWN);
        explored.survey(start);
        assert!(explored.contains(Vec2::from(BASE)));
        assert!(explored.contains(start + Vec2::new(SURVEY_RANGE * 0.8, 0.0)));
        assert!(!explored.contains(start + Vec2::new(SURVEY_RANGE * 1.5, 0.0)));
        assert!(!explored.contains(SWAMP));
    }
}
