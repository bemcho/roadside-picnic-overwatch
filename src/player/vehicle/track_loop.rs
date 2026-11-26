//! The Institute carrier's track belt as a closed loop in the side plane, and where
//! each scanned link sits on it (the measured loops are in `carrier_track`).

use bevy::prelude::*;

/// A closed polyline of (z, y) points in model units, with its arc lengths tabulated
/// once. The closing segment runs from the last point back to the first.
pub(super) struct TrackLoop {
    points: Vec<Vec2>,
    /// Arc length at each point.
    starts: Vec<f32>,
    length: f32,
}

impl TrackLoop {
    pub(super) fn new(points: &[[f32; 2]]) -> TrackLoop {
        assert!(
            points.len() >= 3,
            "a track loop needs at least three points"
        );
        let points: Vec<Vec2> = points.iter().copied().map(Vec2::from).collect();
        let mut starts = Vec::with_capacity(points.len());
        let mut length = 0.0;
        for (i, point) in points.iter().enumerate() {
            starts.push(length);
            length += point.distance(points[(i + 1) % points.len()]);
        }
        TrackLoop {
            points,
            starts,
            length,
        }
    }

    pub(super) fn length(&self) -> f32 {
        self.length
    }

    /// The point `s` along the loop, with `s` taken modulo its length.
    pub(super) fn point(&self, s: f32) -> Vec2 {
        // Rounding can lift a hair below zero to exactly `length`: the last segment's end.
        let s = s.rem_euclid(self.length);
        let i = self
            .starts
            .partition_point(|&start| start <= s)
            .saturating_sub(1);
        let (from, to) = (self.points[i], self.points[(i + 1) % self.points.len()]);
        let along = (s - self.starts[i]) / from.distance(to).max(f32::MIN_POSITIVE);
        from.lerp(to, along.clamp(0.0, 1.0))
    }
}

/// A link's rear and front pins: the loop's points half a pitch either side of the
/// link's own arc length, `run` along from where link `index` of `links` rests.
fn pins(track: &TrackLoop, links: usize, index: usize, run: f32) -> [Vec2; 2] {
    let pitch = track.length() / links as f32;
    // Reduce the run first, so f32 keeps its precision however far the carrier has driven.
    let s = index as f32 * pitch + run.rem_euclid(track.length());
    [track.point(s - pitch / 2.0), track.point(s + pitch / 2.0)]
}

/// Link `index` of `links` on the loop at plane offset `x`, moved `run` model units along
/// the belt's direction of travel (negative runs it back). The link's local -Z lies along
/// the belt, so on the lower run, where it travels toward -Z, the pose is the identity
/// and the tread faces -Y.
pub(super) fn link_pose(
    track: &TrackLoop,
    x: f32,
    links: usize,
    index: usize,
    run: f32,
) -> Transform {
    let [rear, front] = pins(track, links, index, run);
    let mid = (rear + front) / 2.0;
    let travel = front - rear;
    // Turning -Z by `angle` about X gives (0, sin, -cos): the belt's (z, y) direction.
    let angle = travel.y.atan2(-travel.x);
    Transform::from_xyz(x, mid.y, mid.x).with_rotation(Quat::from_rotation_x(angle))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use super::*;

    // A stadium the size of the carrier's belt (about 1.9 model units round, 77 links):
    // (z, y) points, clockwise, the lower run toward -z, as `carrier_track` orders them.
    const HALF_STRAIGHT: f32 = 0.34;
    const RADIUS: f32 = 0.085;
    const LOWER_Y: f32 = 0.0075;
    const MID_Y: f32 = LOWER_Y + RADIUS;
    const UPPER_Y: f32 = LOWER_Y + 2.0 * RADIUS;
    const STEP: f32 = 0.002;
    const LINKS: usize = 77;
    const X: f32 = 0.19;

    fn stadium() -> TrackLoop {
        let straight = 2.0 * HALF_STRAIGHT;
        let mut points = Vec::new();
        let mut sample = |length: f32, at: &dyn Fn(f32) -> [f32; 2]| {
            let count = (length / STEP).ceil() as usize;
            points.extend((0..count).map(|i| at(i as f32 / count as f32)));
        };
        // Lower run front to back, up round the idler, upper run back to front,
        // down round the sprocket.
        sample(straight, &|u| [HALF_STRAIGHT - u * straight, LOWER_Y]);
        sample(PI * RADIUS, &|u| {
            let angle = -FRAC_PI_2 - PI * u;
            [
                -HALF_STRAIGHT + RADIUS * angle.cos(),
                MID_Y + RADIUS * angle.sin(),
            ]
        });
        sample(straight, &|u| [-HALF_STRAIGHT + u * straight, UPPER_Y]);
        sample(PI * RADIUS, &|u| {
            let angle = FRAC_PI_2 - PI * u;
            [
                HALF_STRAIGHT + RADIUS * angle.cos(),
                MID_Y + RADIUS * angle.sin(),
            ]
        });
        TrackLoop::new(&points)
    }

    fn poses(track: &TrackLoop, run: f32) -> Vec<Transform> {
        (0..LINKS)
            .map(|index| link_pose(track, X, LINKS, index, run))
            .collect()
    }

    fn assert_same_pose(a: &Transform, b: &Transform, what: &str) {
        assert!(
            a.translation.distance(b.translation) < 1e-5,
            "{what}: place"
        );
        let (a, b) = (a.rotation * Vec3::NEG_Z, b.rotation * Vec3::NEG_Z);
        assert!(a.distance(b) < 1e-4, "{what}: direction");
    }

    fn on_lower_run(pose: &Transform) -> bool {
        pose.translation.y < LOWER_Y + 1e-4 && pose.translation.z.abs() < HALF_STRAIGHT - 0.05
    }

    fn on_upper_run(pose: &Transform) -> bool {
        pose.translation.y > UPPER_Y - 1e-4 && pose.translation.z.abs() < HALF_STRAIGHT - 0.05
    }

    // Every link is as long pin to pin as the pitch (within 1 %, a chord cuts the arcs
    // a little short), and each link's front pin is the next link's rear pin.
    fn assert_even_spacing(track: &TrackLoop, run: f32) {
        let pitch = track.length() / LINKS as f32;
        for index in 0..LINKS {
            let [rear, front] = pins(track, LINKS, index, run);
            assert!(
                (rear.distance(front) / pitch - 1.0).abs() < 0.01,
                "link {index} is {} of a pitch long",
                rear.distance(front) / pitch
            );
            let [next_rear, _] = pins(track, LINKS, (index + 1) % LINKS, run);
            assert!(
                front.distance(next_rear) < 1e-5,
                "link {index} leaves a gap"
            );
        }
    }

    #[test]
    fn arc_length_wraps_round_the_loop() {
        let track = stadium();
        let length = track.length();
        let perimeter = 4.0 * HALF_STRAIGHT + 2.0 * PI * RADIUS;
        assert!((length / perimeter - 1.0).abs() < 0.005, "length {length}");
        for s in [0.0, 0.3, 1.1] {
            assert!(track.point(s).distance(track.point(s + length)) < 1e-5);
            assert!(track.point(s).distance(track.point(s - length)) < 1e-5);
        }
        // A hair below zero rounds up to the whole length; that is still the start.
        assert!(track.point(-1e-9).distance(track.point(0.0)) < 1e-5);
    }

    #[test]
    fn the_loop_closes() {
        let track = stadium();
        let before = poses(&track, 0.0);
        let after = poses(&track, track.length() / LINKS as f32);
        for index in 0..LINKS {
            assert_same_pose(
                &after[index],
                &before[(index + 1) % LINKS],
                &format!("link {index}"),
            );
        }
    }

    #[test]
    fn links_are_evenly_spaced() {
        let track = stadium();
        assert_even_spacing(&track, 0.0);
        assert_even_spacing(&track, 0.1234);
    }

    #[test]
    fn a_link_is_posed_on_its_pins() {
        let track = stadium();
        for (index, run) in [(0, 0.0), (17, 0.37), (40, -0.2), (76, 1.5)] {
            let [rear, front] = pins(&track, LINKS, index, run);
            let pose = link_pose(&track, X, LINKS, index, run);
            let mid = (rear + front) / 2.0;
            assert!(pose.translation.distance(Vec3::new(X, mid.y, mid.x)) < 1e-6);
            let travel = (front - rear).normalize();
            let facing = pose.rotation * Vec3::NEG_Z;
            assert!(facing.distance(Vec3::new(0.0, travel.y, travel.x)) < 1e-5);
        }
    }

    #[test]
    fn a_lower_link_runs_rearward_going_forward() {
        let track = stadium();
        let (now, later) = (poses(&track, 0.0), poses(&track, 0.001));
        let (mut lower, mut upper) = (0, 0);
        for (before, after) in now.iter().zip(&later) {
            if on_lower_run(before) {
                lower += 1;
                assert!(after.translation.z < before.translation.z);
            }
            if on_upper_run(before) {
                upper += 1;
                assert!(after.translation.z > before.translation.z);
            }
        }
        assert!(
            lower > 10 && upper > 10,
            "{lower} lower, {upper} upper links"
        );
    }

    #[test]
    fn a_link_faces_out() {
        let track = stadium();
        let (mut lower, mut upper) = (0, 0);
        for pose in poses(&track, 0.0) {
            let tread = pose.rotation * Vec3::NEG_Y;
            if on_lower_run(&pose) {
                lower += 1;
                assert!(tread.distance(Vec3::NEG_Y) < 1e-4);
                assert!(pose.rotation.dot(Quat::IDENTITY).abs() > 1.0 - 1e-6);
            }
            if on_upper_run(&pose) {
                upper += 1;
                assert!(tread.distance(Vec3::Y) < 1e-4);
            }
            // Round the arcs too: the tread points away from the loop's middle.
            let from_middle = Vec2::new(pose.translation.z, pose.translation.y - MID_Y);
            assert!(from_middle.dot(Vec2::new(tread.z, tread.y)) > 0.0);
        }
        assert!(
            lower > 10 && upper > 10,
            "{lower} lower, {upper} upper links"
        );
    }

    #[test]
    fn a_link_keeps_the_tracks_x() {
        let track = stadium();
        assert!(
            poses(&track, 0.4)
                .iter()
                .all(|pose| pose.translation.x == X)
        );
    }

    #[test]
    fn reversing_runs_it_back() {
        let track = stadium();
        let length = track.length();
        // The second distance is more than a whole turn.
        for d in [0.03, 2.5] {
            let (back, round) = (poses(&track, -d), poses(&track, length - d));
            for index in 0..LINKS {
                assert_same_pose(&back[index], &round[index], &format!("d {d}, link {index}"));
            }
        }
    }

    #[test]
    fn a_long_drive_keeps_the_spacing() {
        let track = stadium();
        // 100 km in meters, at the carrier's scale.
        assert_even_spacing(&track, 1e5 / 7.0);
        assert_even_spacing(&track, -1e5 / 7.0);
    }
}
