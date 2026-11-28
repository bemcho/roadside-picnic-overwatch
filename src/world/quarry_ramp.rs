//! The haul ramps run through the scans they cut: the Quarry rim's pit and the
//! Golden Sphere's crater. Each cut scan's collision leaves out the pillars
//! whose centres lie in its cut corridor, so the stalker, the car and the click
//! router can use the road the terrain was graded for. The crater's corridor
//! also takes in the dark backdrop the scan was generated with, so the terrain
//! shows there instead of a black face with a second, painted Sphere.

use std::sync::LazyLock;

use bevy::prelude::*;

use super::collision::Pillar;
#[cfg(test)]
use super::roads::RAMP_HALF_WIDTH;
use super::roads::RAMP_SHOULDER;
use super::sites::{self, Kind};

/// A scan whose stone a haul ramp cuts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CutScan {
    /// The Quarry rim's pit: its own ramp and the crater road's first leg,
    /// which leaves from the pit's floor.
    Quarry,
    /// The Golden Sphere's crater: the crater road's spiral.
    Core,
}

/// The legs of every ramp that reach each cut scan's footprint, in `CutScan`
/// order: the centerlines its cut and its cleared collision follow. Derived
/// from the baked paths and the site poses once, at first use.
static CORRIDORS: LazyLock<[Vec<[Vec2; 2]>; 2]> = LazyLock::new(|| {
    let ramps = [sites::quarry_ramp(), sites::crater_ramp()];
    CutScan::ALL.map(|scan| {
        let (low, high) = sites::cut_footprint(scan);
        // A leg matters where its shoulder reaches the cut image, which spans
        // the footprint and a shoulder around it.
        let reach = Vec2::splat(2.0 * RAMP_SHOULDER);
        let (low, high) = (low - reach, high + reach);
        ramps
            .iter()
            .flat_map(|ramp| ramp.windows(2).map(|leg| [leg[0], leg[1]]))
            .filter(|[from, to]| from.min(*to).cmple(high).all() && from.max(*to).cmpge(low).all())
            .collect()
    })
});

/// The core scan's dark backdrop (a smooth, near-black face at the crater's
/// foot with the generation's own Sphere painted on it) in model units (x, z).
const CRATER_BACKDROP: &[Vec2] = include!("pillars/golden_crater_backdrop.rs");

/// `CRATER_BACKDROP` in world xz, placed once at first use.
static BACKDROP: LazyLock<Vec<Vec2>> = LazyLock::new(|| {
    let (pose, _) = sites::cut_scan(CutScan::Core);
    CRATER_BACKDROP
        .iter()
        .map(|at| pose.transform_point(Vec3::new(at.x, 0.0, at.y)).xz())
        .collect()
});

/// Each cut scan's collision without the pillars whose centres lie in its cut
/// corridor, in `CutScan` order, filtered once at first use.
static PILLARS: LazyLock<[Vec<Pillar>; 2]> = LazyLock::new(|| {
    CutScan::ALL.map(|scan| {
        let (pose, pillars) = sites::cut_scan(scan);
        pillars
            .iter()
            .filter(|pillar| {
                let at = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
                !scan.in_cut_corridor(at.xz())
            })
            .copied()
            .collect()
    })
});

impl CutScan {
    pub(crate) const ALL: [Self; 2] = [Self::Quarry, Self::Core];

    /// The site this scan is.
    pub(super) fn kind(self) -> Kind {
        match self {
            Self::Quarry => Kind::QuarryRim,
            Self::Core => Kind::GoldenSphere,
        }
    }

    /// The scan's solids outside its cut corridor, the same corridor whose
    /// stone the cut hides, so what blocks is what is drawn. Computed once.
    pub(crate) fn pillars(self) -> &'static [Pillar] {
        &PILLARS[self as usize]
    }

    /// Whether `at` lies in the corridor whose stone the cut hides: within a
    /// ramp's bed and its shoulder, and in the crater on its dark backdrop.
    /// The collision and the cut share this rule.
    pub(super) fn in_cut_corridor(self, at: Vec2) -> bool {
        self.past_cut_corridor(at) < 0.0
    }

    /// How far `at` lies outside the cut corridor, negative inside it.
    pub(super) fn past_cut_corridor(self, at: Vec2) -> f32 {
        let road = self.distance_to_ramp(at) - RAMP_SHOULDER;
        match self {
            Self::Quarry => road,
            Self::Core => road.min(past_polygon(&BACKDROP, at)),
        }
    }

    /// Whether a circle of `radius` at `at` overlaps a ramp's bed in the scan.
    #[cfg(test)]
    pub(super) fn on_ramp_bed(self, at: Vec2, radius: f32) -> bool {
        self.distance_to_ramp(at) < RAMP_HALF_WIDTH + radius
    }

    fn distance_to_ramp(self, at: Vec2) -> f32 {
        CORRIDORS[self as usize]
            .iter()
            .map(|&[from, to]| distance_to_segment(at, from, to))
            .fold(f32::INFINITY, f32::min)
    }
}

fn distance_to_segment(at: Vec2, from: Vec2, to: Vec2) -> f32 {
    let along = to - from;
    let t = ((at - from).dot(along) / along.length_squared()).clamp(0.0, 1.0);
    at.distance(from + along * t)
}

/// How far `at` lies outside the closed `polygon`, negative inside it.
fn past_polygon(polygon: &[Vec2], at: Vec2) -> f32 {
    let mut inside = false;
    let mut nearest = f32::INFINITY;
    for (i, &to) in polygon.iter().enumerate() {
        let from = polygon[(i + polygon.len() - 1) % polygon.len()];
        let crosses = (from.y > at.y) != (to.y > at.y)
            && at.x < from.x + (at.y - from.y) / (to.y - from.y) * (to.x - from.x);
        inside ^= crosses;
        nearest = nearest.min(distance_to_segment(at, from, to));
    }
    if inside { -nearest } else { nearest }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Body, Collider, Terrain, push_out};

    /// The Institute carrier's routing body (`CARRIER_ROUTING`, its radius and
    /// `HULL_HEIGHT` rounded up), the widest and tallest mover.
    const CARRIER: Body = Body {
        radius: 2.3,
        height: 3.0,
    };
    /// The most a kept pillar's body may reach into the cut past its edge.
    const MAX_LIP: f32 = 1.0;

    fn assert_no_pillar_stands_on_the_ramp(scan: CutScan) {
        let (pose, _) = sites::cut_scan(scan);
        for pillar in scan.pillars() {
            let center = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let reach = RAMP_HALF_WIDTH + pillar.radius * pose.scale.x;
            let distance = scan.distance_to_ramp(center.xz());
            assert!(
                distance >= reach,
                "a pillar at {} is {distance} m from the ramp, its bed reaches {reach} m",
                center.xz()
            );
        }
    }

    #[test]
    fn no_quarry_pillar_stands_on_the_ramp() {
        assert_no_pillar_stands_on_the_ramp(CutScan::Quarry);
    }

    #[test]
    fn no_core_pillar_stands_on_the_ramp() {
        assert_no_pillar_stands_on_the_ramp(CutScan::Core);
    }

    // The centre rule keeps a pillar whose body reaches into the cut, where the
    // stone is hidden, so that lip is solid nobody sees: it must stay a sliver of
    // the bank beside the bed, and clear of the widest mover hugging the bed's edge.
    fn assert_kept_pillars_leave_only_a_small_lip_in_the_cut(scan: CutScan) {
        let (pose, _) = sites::cut_scan(scan);
        let bank = RAMP_SHOULDER - RAMP_HALF_WIDTH;
        let mut lips = 0;
        for pillar in scan.pillars() {
            let center = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let radius = pillar.radius * pose.scale.x;
            // From the pillar's body to the bed's edge; the bank is that wide.
            let from_bed = scan.distance_to_ramp(center.xz()) - radius - RAMP_HALF_WIDTH;
            let lip = bank - from_bed;
            if lip <= 0.0 {
                continue;
            }
            lips += 1;
            assert!(
                lip <= MAX_LIP,
                "a pillar at {} reaches {lip} m into the cut, over {MAX_LIP} m",
                center.xz()
            );
            assert!(
                from_bed >= CARRIER.radius,
                "a pillar at {} is {from_bed} m off the bed, the carrier reaches {} m",
                center.xz(),
                CARRIER.radius
            );
        }
        assert!(
            lips > 0,
            "no kept pillar reaches into the cut: nothing examined"
        );
    }

    #[test]
    fn kept_quarry_pillars_leave_only_a_small_lip_in_the_cut() {
        assert_kept_pillars_leave_only_a_small_lip_in_the_cut(CutScan::Quarry);
    }

    #[test]
    fn kept_core_pillars_leave_only_a_small_lip_in_the_cut() {
        assert_kept_pillars_leave_only_a_small_lip_in_the_cut(CutScan::Core);
    }

    /// At least one pillar is dropped and at most `percent` of them.
    fn assert_pillars_off_the_ramp_are_kept(scan: CutScan, percent: usize) {
        let (_, all) = sites::cut_scan(scan);
        let dropped = all.len() - scan.pillars().len();
        assert!(dropped >= 1, "the ramp's cut corridor clears no pillar");
        assert!(
            dropped * 100 <= all.len() * percent,
            "{dropped} of {} pillars dropped, the walls must stay",
            all.len()
        );
    }

    #[test]
    fn quarry_pillars_off_the_ramp_are_kept() {
        // The corridor is 18 m wide and winds 250 m through a 100 m scan.
        assert_pillars_off_the_ramp_are_kept(CutScan::Quarry, 55);
    }

    #[test]
    fn core_pillars_off_the_ramp_are_kept() {
        // The corridor is 18 m wide and spirals 381 m through a 110 m scan.
        assert_pillars_off_the_ramp_are_kept(CutScan::Core, 60);
    }

    fn assert_the_widest_mover_walks_the_ramp_unpushed(scan: CutScan) {
        let terrain = Terrain::new();
        let (pose, _) = sites::cut_scan(scan);
        let transform = GlobalTransform::from(pose);
        let collider = Collider(scan.pillars());
        for [from, to] in &CORRIDORS[scan as usize] {
            let steps = from.distance(*to).ceil() as usize;
            for i in 0..=steps {
                let at = from.lerp(*to, i as f32 / steps as f32);
                let feet = terrain.mesh_height_at(at);
                assert_eq!(
                    push_out(at, feet, CARRIER, [(&collider, &transform)]),
                    at,
                    "the scan's collision stands on the ramp at {at}"
                );
            }
        }
    }

    #[test]
    fn the_widest_mover_walks_the_quarry_ramp_unpushed() {
        assert_the_widest_mover_walks_the_ramp_unpushed(CutScan::Quarry);
    }

    #[test]
    fn the_widest_mover_walks_the_crater_ramp_unpushed() {
        assert_the_widest_mover_walks_the_ramp_unpushed(CutScan::Core);
    }

    #[test]
    fn the_crater_cut_takes_in_the_painted_backdrop() {
        // The generation's own Sphere is painted in the middle of the face, its
        // nearest edge some 9 m off.
        let (pose, _) = sites::cut_scan(CutScan::Core);
        let painted = pose.transform_point(sites::SPHERE_SMUDGE).xz();
        let past = CutScan::Core.past_cut_corridor(painted);
        assert!(past < -5.0, "the painted Sphere lies {past} m past the cut");
        assert!(CutScan::Core.in_cut_corridor(painted));
        assert!(past_polygon(&BACKDROP, painted) < -5.0);
    }

    #[test]
    fn past_polygon_is_a_signed_distance() {
        let square = [
            Vec2::ZERO,
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(0.0, 2.0),
        ];
        assert_eq!(past_polygon(&square, Vec2::new(1.0, 0.5)), -0.5);
        assert_eq!(past_polygon(&square, Vec2::new(3.0, 1.0)), 1.0);
        assert_eq!(past_polygon(&square, Vec2::new(-1.0, 1.0)), 1.0);
    }

    #[test]
    fn the_quarry_corridor_covers_the_crater_roads_first_leg() {
        // The crater road leaves from the quarry ramp's end on the pit's floor
        // and runs on the floor, inside the box around the quarry scan, for
        // its first 23 m.
        let crater = sites::crater_ramp();
        let (from, to) = (crater[0], crater[1]);
        let (low, high) = sites::cut_footprint(CutScan::Quarry);
        let inside: Vec<_> = (0..=from.distance(to).ceil() as usize)
            .map(|metre| from.move_towards(to, metre as f32))
            .filter(|at| at.cmpge(low).all() && at.cmple(high).all())
            .collect();
        assert!(
            inside.len() >= 20,
            "{} m of the leg lie in the quarry",
            inside.len()
        );
        for at in inside {
            assert!(
                CutScan::Quarry.on_ramp_bed(at, 0.0),
                "{at} is off the quarry's corridor"
            );
        }
    }

    #[test]
    fn the_bed_reaches_its_half_width_beside_a_leg() {
        let ramp = sites::quarry_ramp();
        let (from, to) = (ramp[0], ramp[1]);
        let middle = from.midpoint(to);
        let side = (to - from).normalize().perp();
        let quarry = CutScan::Quarry;
        assert!(quarry.on_ramp_bed(middle + side * (RAMP_HALF_WIDTH - 0.1), 0.0));
        assert!(!quarry.on_ramp_bed(middle + side * (RAMP_HALF_WIDTH + 0.1), 0.0));
        assert!(quarry.on_ramp_bed(middle - side * (RAMP_HALF_WIDTH + 0.5), 1.0));
        assert!(!quarry.on_ramp_bed(from - (to - from).normalize() * 20.0, 1.0));
    }
}
