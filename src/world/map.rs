//! The Harmont map (`doc/Roadside_map.jpg`, 1408 × 768 px) laid over the
//! world at one uniform scale, so every placement can be checked against the
//! image. North is +Z; the map's west (Harmont, left) is +X and its east (the
//! Zone, right) is -X, which reads the right way round from above.

use bevy::math::Vec2;

/// Meters per map pixel: the map's frame (1346 × 710 px) spans 3.2 × 1.7 km.
pub(crate) const METERS_PER_PIXEL: f32 = 2.4;
/// The map pixel at the world origin, the middle of the map's frame.
const CENTER: Vec2 = Vec2::new(705.0, 383.0);

/// The world position of a map pixel.
pub(crate) const fn map(px: f32, py: f32) -> Vec2 {
    Vec2::new(
        (CENTER.x - px) * METERS_PER_PIXEL,
        (CENTER.y - py) * METERS_PER_PIXEL,
    )
}

/// The old railway, south to north across the whole map (the double line on
/// the map), out to the map's edges. Sites stand beside some of its points.
pub(crate) const RAILWAY: [Vec2; 14] = [
    Vec2::new(378.0, -1700.0),
    map(650.0, 738.0),
    map(674.0, 655.0),
    map(690.0, 600.0),
    map(735.0, 500.0),
    map(765.0, 440.0),
    map(785.0, 400.0),
    map(813.0, 347.0),
    map(838.0, 300.0),
    map(900.0, 200.0),
    map(921.0, 150.0),
    map(942.0, 100.0),
    map(972.0, 30.0),
    Vec2::new(-1006.0, 1700.0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_reads_west_to_east_as_harmont_to_the_zone() {
        // The Borscht (left) lies on +X, the quarry (right) on -X; north is +Z.
        assert!(map(195.0, 593.0).x > 0.0 && map(1235.0, 375.0).x < 0.0);
        assert!(map(174.0, 213.0).y > map(195.0, 593.0).y);
        assert_eq!(map(705.0, 383.0), Vec2::ZERO);
    }
}
