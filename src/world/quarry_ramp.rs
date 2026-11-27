//! The haul ramps run through the scans they cut: the Quarry rim's pit and the
//! Golden Sphere's crater. Each cut scan's collision leaves out the pillars
//! whose centres lie in its cut corridor, so the stalker, the car and the click
//! router can use the road the terrain was graded for.

use std::sync::LazyLock;

use bevy::prelude::*;

use super::collision::Pillar;
use super::roads::{RAMP_HALF_WIDTH, RAMP_SHOULDER};
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
    /// ramp's bed and its shoulder. The collision and the cut share this rule.
    pub(super) fn in_cut_corridor(self, at: Vec2) -> bool {
        self.on_ramp_bed(at, RAMP_SHOULDER - RAMP_HALF_WIDTH)
    }

    /// How far `at` lies outside the cut corridor, negative inside it.
    pub(super) fn past_cut_corridor(self, at: Vec2) -> f32 {
        self.distance_to_ramp(at) - RAMP_SHOULDER
    }

    /// Whether a circle of `radius` at `at` overlaps a ramp's bed in the scan.
    pub(super) fn on_ramp_bed(self, at: Vec2, radius: f32) -> bool {
        self.distance_to_ramp(at) < RAMP_HALF_WIDTH + radius
    }

    fn distance_to_ramp(self, at: Vec2) -> f32 {
        CORRIDORS[self as usize]
            .iter()
            .map(|&[from, to]| {
                let along = to - from;
                let t = ((at - from).dot(along) / along.length_squared()).clamp(0.0, 1.0);
                at.distance(from + along * t)
            })
            .fold(f32::INFINITY, f32::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Body, Collider, STEP_HEIGHT, Terrain, push_out};

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

    // A guard against narrowing the filter: with the centre rule no kept pillar lies
    // wholly inside the corridor, so today this examines none.
    fn assert_no_kept_pillar_stands_invisible_in_the_cut(scan: CutScan) {
        let terrain = Terrain::new();
        let (pose, _) = sites::cut_scan(scan);
        for pillar in scan.pillars() {
            let center = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let radius = pillar.radius * pose.scale.x;
            if scan.distance_to_ramp(center.xz()) + radius >= RAMP_SHOULDER {
                continue;
            }
            // The cut hides the stone here, so a solid taller than a step
            // would be a climbable column nobody can see.
            let top = center.y + pillar.top * pose.scale.y;
            let ground = terrain.mesh_height_at(center.xz());
            assert!(
                top <= ground + STEP_HEIGHT,
                "a pillar at {} rises {} m above the drawn ground inside the cut",
                center.xz(),
                top - ground
            );
        }
    }

    #[test]
    fn no_kept_quarry_pillar_stands_invisible_in_the_cut() {
        assert_no_kept_pillar_stands_invisible_in_the_cut(CutScan::Quarry);
    }

    #[test]
    fn no_kept_core_pillar_stands_invisible_in_the_cut() {
        assert_no_kept_pillar_stands_invisible_in_the_cut(CutScan::Core);
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
        // The Institute carrier's routing body (`CARRIER_ROUTING`, its radius and
        // `HULL_HEIGHT` rounded up), wider and taller than the car's.
        let carrier = Body {
            radius: 2.3,
            height: 3.0,
        };
        for [from, to] in &CORRIDORS[scan as usize] {
            let steps = from.distance(*to).ceil() as usize;
            for i in 0..=steps {
                let at = from.lerp(*to, i as f32 / steps as f32);
                let feet = terrain.mesh_height_at(at);
                assert_eq!(
                    push_out(at, feet, carrier, [(&collider, &transform)]),
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
