//! What the Golden Sphere mirrors on modest GPUs, which get no live reflection
//! map: the stalker as a capsule and the car and carrier as boxes, traced in
//! its fragment shader (`assets/shaders/golden_sphere.wesl`).

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::assets;
use crate::world::terrain::smoothstep;

pub(super) type MirrorMaterial = ExtendedMaterial<StandardMaterial, Mirror>;

/// How far from the Sphere a body still shows in it, in metres.
pub(super) const REACH: f32 = 15.0;
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
    /// The live probe's faces (`reflection.rs`), on 5 GB GPUs and up.
    #[texture(102)]
    #[sampler(108)]
    pub face_px: Option<Handle<Image>>,
    #[texture(103)]
    pub face_nx: Option<Handle<Image>>,
    #[texture(104)]
    pub face_py: Option<Handle<Image>>,
    #[texture(105)]
    pub face_ny: Option<Handle<Image>>,
    #[texture(106)]
    pub face_pz: Option<Handle<Image>>,
    #[texture(107)]
    pub face_nz: Option<Handle<Image>>,
    /// x: 1 while the faces are live.
    #[uniform(109)]
    live: Vec4,
}

impl Mirror {
    /// A mirror of the live probe's `faces`, or of stand-ins without them.
    pub fn of(faces: Option<&[Handle<Image>; 6]>) -> Self {
        let Some([px, nx, py, ny, pz, nz]) = faces.cloned() else {
            return Self::default();
        };
        Self {
            face_px: Some(px),
            face_nx: Some(nx),
            face_py: Some(py),
            face_ny: Some(ny),
            face_pz: Some(pz),
            face_nz: Some(nz),
            live: Vec4::X,
            ..default()
        }
    }
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

/// The `sun` uniform for the stand-ins: the way to the Sun (`to_sun`) and the
/// light it gives them. The Sun keeps its illuminance after dark (only its
/// shadows go), so the light fades with it from just above to just below the
/// horizon; the stand-ins' sky term is a share of it and fades too.
pub(super) fn stand_in_light(to_sun: Vec3, illuminance: f32) -> Vec4 {
    let daylight = smoothstep(-0.05, 0.1, to_sun.y);
    to_sun.extend(illuminance * daylight)
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

    #[test]
    fn the_stand_ins_go_dark_once_the_sun_sets() {
        let lux = 130_000.0;
        let noon = stand_in_light(Vec3::new(0.0, 1.0, -0.3).normalize(), lux);
        assert_eq!(noon.w, lux, "full sunlight by day");
        let night = stand_in_light(Vec3::new(0.3, -0.5, 0.0).normalize(), lux);
        assert_eq!(night.w, 0.0, "the light keeps its illuminance after dark");
        let dusk = stand_in_light(Vec3::new(1.0, 0.02, 0.0).normalize(), lux);
        assert!(
            dusk.w > 0.0 && dusk.w < lux,
            "fades through dusk: {}",
            dusk.w
        );
    }

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
