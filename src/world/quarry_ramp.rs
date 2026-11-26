//! The quarry's haul ramp runs through the scan's collision: the pillars whose
//! centres lie in its cut corridor are left out, so the stalker, the car and
//! the click router can use the road the terrain was graded for.

use std::sync::LazyLock;

use bevy::prelude::*;

use super::collision::Pillar;
use super::roads::{RAMP_HALF_WIDTH, RAMP_SHOULDER};
use super::sites;

/// The ramp's centerline in world xz, from the junction to the pit floor.
/// Derived from the baked path and the site pose once, at first use.
static RAMP: LazyLock<Vec<Vec2>> = LazyLock::new(sites::quarry_ramp);

/// The quarry scan's collision without the pillars whose centres lie in the
/// cut corridor, filtered once at first use from the same baked path and site
/// pose the terrain and the road follow.
static PILLARS: LazyLock<Vec<Pillar>> = LazyLock::new(|| {
    let (pose, pillars) = sites::quarry_scan();
    pillars
        .iter()
        .filter(|pillar| {
            let at = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            !in_cut_corridor(at.xz())
        })
        .copied()
        .collect()
});

/// The quarry's solids outside the ramp's cut corridor, the same corridor
/// whose stone the cut hides, so what blocks is what is drawn. Computed once.
pub(crate) fn quarry_pillars() -> &'static [Pillar] {
    &PILLARS
}

/// Whether `at` lies in the corridor whose stone the cut hides: within the
/// bed and its shoulder. The collision and the cut share this one rule.
pub(super) fn in_cut_corridor(at: Vec2) -> bool {
    on_ramp_bed(at, RAMP_SHOULDER - RAMP_HALF_WIDTH)
}

/// Whether a circle of `radius` at `at` overlaps the ramp's bed.
pub(super) fn on_ramp_bed(at: Vec2, radius: f32) -> bool {
    distance_to_ramp(at) < RAMP_HALF_WIDTH + radius
}

fn distance_to_ramp(at: Vec2) -> f32 {
    RAMP.windows(2)
        .map(|leg| {
            let along = leg[1] - leg[0];
            let t = ((at - leg[0]).dot(along) / along.length_squared()).clamp(0.0, 1.0);
            at.distance(leg[0] + along * t)
        })
        .fold(f32::INFINITY, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Body, Collider, STEP_HEIGHT, Terrain, push_out};

    #[test]
    fn no_quarry_pillar_stands_on_the_ramp() {
        let (pose, _) = sites::quarry_scan();
        for pillar in quarry_pillars() {
            let center = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let reach = RAMP_HALF_WIDTH + pillar.radius * pose.scale.x;
            let distance = distance_to_ramp(center.xz());
            assert!(
                distance >= reach,
                "a pillar at {} is {distance} m from the ramp, its bed reaches {reach} m",
                center.xz()
            );
        }
    }

    // A guard against narrowing the filter: with the centre rule no kept pillar lies
    // wholly inside the corridor, so today this examines none.
    #[test]
    fn no_kept_pillar_stands_invisible_in_the_cut() {
        let terrain = Terrain::new();
        let (pose, _) = sites::quarry_scan();
        for pillar in quarry_pillars() {
            let center = pose.transform_point(Vec3::new(pillar.x, 0.0, pillar.z));
            let radius = pillar.radius * pose.scale.x;
            if distance_to_ramp(center.xz()) + radius >= RAMP_SHOULDER {
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
    fn pillars_off_the_ramp_are_kept() {
        let (_, scan) = sites::quarry_scan();
        let dropped = scan.len() - quarry_pillars().len();
        assert!(dropped >= 1, "the ramp's cut corridor clears no pillar");
        // The corridor is 18 m wide and winds 250 m through a 100 m scan.
        assert!(
            dropped * 100 <= scan.len() * 55,
            "{dropped} of {} pillars dropped, the pit's walls must stay",
            scan.len()
        );
    }

    #[test]
    fn the_widest_mover_walks_the_ramp_unpushed() {
        let terrain = Terrain::new();
        let (pose, _) = sites::quarry_scan();
        let scan = GlobalTransform::from(pose);
        let collider = Collider(quarry_pillars());
        // The Institute carrier's routing body (`CARRIER_ROUTING`, its radius and
        // `HULL_HEIGHT` rounded up), wider and taller than the car's.
        let carrier = Body {
            radius: 2.3,
            height: 3.0,
        };
        for leg in RAMP.windows(2) {
            let steps = leg[0].distance(leg[1]).ceil() as usize;
            for i in 0..=steps {
                let at = leg[0].lerp(leg[1], i as f32 / steps as f32);
                let feet = terrain.mesh_height_at(at);
                assert_eq!(
                    push_out(at, feet, carrier, [(&collider, &scan)]),
                    at,
                    "the scan's collision stands on the ramp at {at}"
                );
            }
        }
    }

    #[test]
    fn the_bed_reaches_its_half_width_beside_a_leg() {
        let ramp = sites::quarry_ramp();
        let (from, to) = (ramp[0], ramp[1]);
        let middle = from.midpoint(to);
        let side = (to - from).normalize().perp();
        assert!(on_ramp_bed(middle + side * (RAMP_HALF_WIDTH - 0.1), 0.0));
        assert!(!on_ramp_bed(middle + side * (RAMP_HALF_WIDTH + 0.1), 0.0));
        assert!(on_ramp_bed(middle - side * (RAMP_HALF_WIDTH + 0.5), 1.0));
        assert!(!on_ramp_bed(from - (to - from).normalize() * 20.0, 1.0));
    }
}
