use bevy::prelude::*;

use super::{GROUND_CLEARANCE, MODEL_SCALE};

// Tire centers in model coordinates, on the ground plane under each hub.
const TIRE_POINTS: [Vec3; 4] = [
    Vec3::new(-0.2131, 0.0, 0.3223),
    Vec3::new(0.2119, 0.0, 0.3247),
    Vec3::new(-0.2129, 0.0, -0.2761),
    Vec3::new(0.2134, 0.0, -0.2759),
];
const WHEEL_TRAVEL: f32 = 0.32;

pub(super) fn heading(rotation: Quat) -> Quat {
    let forward = rotation * Vec3::Z;
    Quat::from_rotation_y(forward.x.atan2(forward.z))
}

#[derive(Clone, Copy)]
pub(super) struct Contact {
    pub rotation: Quat,
    pub height: f32,
    tire_heights: [f32; 4],
}

impl Contact {
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    pub fn suspension(&self) -> [Vec3; 4] {
        let local_up = self.rotation.inverse() * Vec3::Y / MODEL_SCALE;
        self.tire_heights
            .map(|y| local_up * (y - self.height).clamp(-WHEEL_TRAVEL, WHEEL_TRAVEL))
    }
}

pub(super) fn sample(center: Vec2, yaw: Quat, height_at: impl Fn(Vec2) -> f32) -> Contact {
    let tires = TIRE_POINTS.map(|p| p * MODEL_SCALE);
    let [fr, fl, rr, rl] = tires.map(|p| height_at(center + (yaw * p).xz()));
    let pitch = ((fr + fl) - (rr + rl)) / (2.0 * (tires[0].z - tires[2].z));
    let bank = ((fl + rl) - (fr + rr)) / (2.0 * (tires[1].x - tires[0].x));
    let forward = (yaw * Vec3::Z + Vec3::Y * pitch).normalize();
    let up = forward.cross(yaw * Vec3::X + Vec3::Y * bank).normalize();
    let rotation = Quat::from_mat3(&Mat3::from_cols(up.cross(forward), up, forward));

    // Fit the tilted footprint a second time, then spend limited suspension
    // travel on differences between the four contact heights.
    let root_height = |p: Vec3| {
        let p = rotation * p;
        height_at(center + p.xz()) - p.y + GROUND_CLEARANCE
    };
    let tire_heights = tires.map(root_height);
    let average = tire_heights.iter().sum::<f32>() * 0.25;
    let high = tire_heights.into_iter().fold(average, f32::max);
    let mut height = average.max(high - WHEEL_TRAVEL);
    // The chassis must also clear crests between the axles.
    for (x, z) in [
        (0.0, 0.0),
        (-0.95, 0.0),
        (0.95, 0.0),
        (0.0, 2.35),
        (0.0, -2.35),
    ] {
        height = height.max(root_height(Vec3::new(x, 0.25, z)));
    }
    Contact {
        rotation,
        height,
        tire_heights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_tires_touch_a_banked_slope_without_the_hull_staying_level() {
        let surface = |p: Vec2| 0.29 * p.x + 0.42 * p.y;
        let fit = sample(Vec2::ZERO, Quat::IDENTITY, surface);
        assert!(fit.up().dot(Vec3::new(-0.29, 1.0, -0.42).normalize()) > 0.99999);
        for (tire, suspension) in TIRE_POINTS.into_iter().zip(fit.suspension()) {
            let foot = Vec3::Y * fit.height + fit.rotation * ((tire + suspension) * MODEL_SCALE);
            assert!((foot.y - surface(foot.xz()) - GROUND_CLEARANCE).abs() < 0.001);
        }
    }
}
