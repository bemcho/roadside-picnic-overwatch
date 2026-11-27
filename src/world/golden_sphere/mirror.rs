//! What the Golden Sphere mirrors on modest GPUs, which get no live reflection
//! map: the stalker as a capsule and the car and carrier as boxes, traced in
//! its fragment shader (`assets/shaders/golden_sphere.wesl`).

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::assets;

pub(super) type MirrorMaterial = ExtendedMaterial<StandardMaterial, Mirror>;

/// How far from the Sphere a body still shows in it, in metres.
const REACH: f32 = 15.0;
/// The stalker's capsule: standing and lying heights, and its radius.
const STANDING: f32 = 1.8;
const LYING: f32 = 0.5;
const STALKER_RADIUS: f32 = 0.3;
/// The stand-ins' paints: the mean linear albedo of each model's base-colour
/// texture, measured once with PIL from the workspace sources
/// (`soldier/uhd/soldier.glb`, `vehicles/uhd/armored_vehicle.glb`,
/// `vehicles/uhd/institute-carrier.glb`).
pub(super) const STALKER_PAINT: Vec3 = Vec3::new(0.0448, 0.0526, 0.0526);
pub(super) const CAR_PAINT: Vec3 = Vec3::new(0.0598, 0.0359, 0.0266);
pub(super) const CARRIER_PAINT: Vec3 = Vec3::new(0.0636, 0.0494, 0.0409);
/// A stand-in's kind, in `half_size.w`.
const OFF: f32 = 0.0;
const CAPSULE: f32 = 1.0;
const BOX: f32 = 2.0;

/// One body the Sphere mirrors.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub(super) struct StandIn {
    /// xyz: its centre, w: its turn about the vertical in radians.
    pub centre_yaw: Vec4,
    /// xyz: its half size, w: its kind (`OFF`, `CAPSULE` or `BOX`).
    pub half_size: Vec4,
    /// rgb: its linear albedo.
    pub paint: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub(super) struct Mirror {
    #[uniform(100)]
    pub stand_ins: [StandIn; 3],
    /// xyz: the way to the Sun, w: its illuminance in lux.
    #[uniform(101)]
    pub sun: Vec4,
}

impl MaterialExtension for Mirror {
    fn fragment_shader() -> ShaderRef {
        assets::GOLDEN_SPHERE_SHADER.into()
    }
}

/// The stalker, for the mirror: where their feet are and how they move.
pub(super) struct Stalker {
    pub feet: Vec3,
    pub prone: bool,
    pub driving: bool,
}

/// A vehicle's hull, for the mirror: the middle of its bottom, its turn, its
/// half size and its paint.
pub(super) struct Hull {
    pub base: Vec3,
    pub yaw: f32,
    pub half: Vec3,
    pub paint: Vec3,
}

/// The bodies near enough to the Sphere at `sphere` to show in it: the stalker
/// as a capsule unless they are driving (the vehicle shows them), and each
/// hull as a box. The rest of the three slots stay off.
pub(super) fn stand_ins(sphere: Vec3, stalker: Option<Stalker>, hulls: &[Hull]) -> [StandIn; 3] {
    let figure = stalker.filter(|stalker| !stalker.driving).map(|stalker| {
        let height = if stalker.prone { LYING } else { STANDING };
        let half = Vec3::new(STALKER_RADIUS, height / 2.0, STALKER_RADIUS);
        (
            stalker.feet + Vec3::Y * half.y,
            0.0,
            half,
            CAPSULE,
            STALKER_PAINT,
        )
    });
    let boxes = hulls.iter().map(|hull| {
        (
            hull.base + Vec3::Y * hull.half.y,
            hull.yaw,
            hull.half,
            BOX,
            hull.paint,
        )
    });
    let off = StandIn {
        half_size: Vec4::W * OFF,
        ..default()
    };
    let mut shown = [off; 3];
    let near = figure
        .into_iter()
        .chain(boxes)
        .filter(|(centre, _, half, ..)| centre.distance(sphere) - half.xz().length() <= REACH);
    for (slot, (centre, yaw, half, kind, paint)) in shown.iter_mut().zip(near) {
        *slot = StandIn {
            centre_yaw: centre.extend(yaw),
            half_size: half.extend(kind),
            paint: paint.extend(1.0),
        };
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPHERE: Vec3 = Vec3::new(0.0, 2.25, 0.0);

    fn car(at: Vec3) -> Hull {
        Hull {
            base: at,
            yaw: 0.3,
            half: Vec3::new(1.5, 1.2, 2.6),
            paint: CAR_PAINT,
        }
    }

    fn on(stand_ins: &[StandIn; 3]) -> Vec<&StandIn> {
        stand_ins.iter().filter(|s| s.half_size.w != OFF).collect()
    }

    #[test]
    fn only_what_is_within_reach_shows_in_the_sphere() {
        let stalker = Stalker {
            feet: Vec3::new(5.0, 0.0, 0.0),
            prone: false,
            driving: false,
        };
        let shown = stand_ins(SPHERE, Some(stalker), &[car(Vec3::new(20.0, 0.0, 0.0))]);
        let on = on(&shown);
        assert_eq!(on.len(), 1, "the car 20 m off stays out");
        assert_eq!(on[0].half_size.w, CAPSULE);
        assert!(
            (on[0].half_size.y * 2.0 - 1.8).abs() < 1e-4,
            "a standing stalker"
        );
        assert!(
            (on[0].centre_yaw.y - 0.9).abs() < 1e-4,
            "standing on their feet"
        );
    }

    #[test]
    fn a_driving_stalker_shows_only_as_the_car() {
        let stalker = Stalker {
            feet: Vec3::new(4.0, 0.5, 0.0),
            prone: false,
            driving: true,
        };
        let shown = stand_ins(SPHERE, Some(stalker), &[car(Vec3::new(4.0, 0.0, 0.0))]);
        let on = on(&shown);
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].half_size.w, BOX);
        assert_eq!(on[0].centre_yaw.w, 0.3, "turned as the car");
        assert_eq!(on[0].paint.truncate(), CAR_PAINT);
    }

    #[test]
    fn a_crawling_stalker_is_a_low_capsule() {
        let stalker = Stalker {
            feet: Vec3::new(3.0, 0.0, 3.0),
            prone: true,
            driving: false,
        };
        let shown = stand_ins(SPHERE, Some(stalker), &[]);
        let on = on(&shown);
        assert_eq!(on.len(), 1);
        assert!((on[0].half_size.y * 2.0 - 0.5).abs() < 1e-4, "lying down");
    }
}
