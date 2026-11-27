//! The Golden Sphere's live reflection on strong and exceptional GPUs: six
//! cameras at its centre render the scene around it into the faces of a cube,
//! which its shader and the ferrofluid's sample.

use std::f32::consts::FRAC_PI_2;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::diagnostic::FrameCount;
use bevy::light::cluster::ClusterConfig;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::Msaa;

use crate::assets::gpu_scans::{GpuTier, TierLevel};
use crate::states::AppState;

/// The render layer only the Sphere is on: the player's camera sees it, its
/// own probe does not.
pub(crate) const SPHERE_LAYER: usize = 1;
/// A face's side in pixels on strong and exceptional GPUs.
const STRONG_FACE: u32 = 256;
const EXCEPTIONAL_FACE: u32 = 1024;
/// How far the probe sees, in metres: the crater and its rim.
const PROBE_FAR: f32 = 300.0;
/// The faces' directions and their cameras' up, in face order. The shaders
/// (`golden_sphere.wesl`, `ferrofluid.wesl`) pick faces by the same table.
const FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::NEG_Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];
const ALL_FACES: [usize; 6] = [0, 1, 2, 3, 4, 5];

/// The six faces the probe renders into.
#[derive(Resource)]
pub(super) struct ReflectionProbe {
    pub faces: [Handle<Image>; 6],
}

/// One of the probe's cameras, rendering face `0`.
#[derive(Component)]
pub(super) struct ProbeFace(pub usize);

/// The faces to render this `frame`: none on modest GPUs or with nobody near
/// (the faces keep what they last showed), one a frame in turn on strong
/// GPUs, all six every frame on exceptional ones.
pub(super) fn faces_to_render(
    level: TierLevel,
    frame: u32,
    someone_near: bool,
) -> &'static [usize] {
    match level {
        _ if !someone_near => &[],
        TierLevel::Modest => &[],
        TierLevel::Strong => {
            let face = frame as usize % ALL_FACES.len();
            &ALL_FACES[face..=face]
        }
        TierLevel::Exceptional => &ALL_FACES,
    }
}

/// The probe's faces at the size the GPU's tier asks, made the first time the
/// Zone is entered, and its six cameras, made each time. Modest GPUs get none.
pub(super) fn spawn_reflection_probe(
    mut commands: Commands,
    tier: Option<Res<GpuTier>>,
    probe: Option<Res<ReflectionProbe>>,
    mut images: ResMut<Assets<Image>>,
) {
    let size = match tier.map_or(TierLevel::default(), |tier| tier.level()) {
        TierLevel::Modest => return,
        TierLevel::Strong => STRONG_FACE,
        TierLevel::Exceptional => EXCEPTIONAL_FACE,
    };
    let faces = match probe {
        Some(probe) => probe.faces.clone(),
        None => {
            let faces = std::array::from_fn(|_| {
                images.add(Image::new_target_texture(
                    size,
                    size,
                    TextureFormat::Rgba16Float,
                    None,
                ))
            });
            commands.insert_resource(ReflectionProbe {
                faces: faces.clone(),
            });
            faces
        }
    };
    for (face, image) in faces.into_iter().enumerate() {
        commands.spawn((
            Name::new("Golden Sphere probe face"),
            ProbeFace(face),
            Camera3d::default(),
            Camera {
                is_active: false,
                order: -10 + face as isize,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.into()),
            Projection::Perspective(PerspectiveProjection {
                fov: FRAC_PI_2,
                aspect_ratio: 1.0,
                near: 0.05,
                far: PROBE_FAR,
                ..default()
            }),
            // The player's camera's exposure, so the faces' light matches.
            Exposure { ev100: 13.0 },
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            // One cluster: clustering a view at the default grid costs tens of
            // milliseconds of CPU a frame (traced 2026-11-27). `None` would cost
            // nothing, but Bevy 0.20's GPU clustering then fails on a zero-sized
            // texture.
            ClusterConfig::Single,
            RenderLayers::layer(0),
            Transform::default(),
            DespawnOnExit(AppState::InGame),
        ));
    }
}

/// Moves the probe's cameras to the Sphere at `centre` and wakes the faces
/// this frame renders.
pub(super) fn steer_reflection_probe(
    centre: Vec3,
    someone_near: bool,
    level: TierLevel,
    frame: &FrameCount,
    cameras: &mut Query<(&ProbeFace, &mut Camera, &mut Transform)>,
) {
    let awake = faces_to_render(level, frame.0, someone_near);
    for (face, mut camera, mut transform) in cameras.iter_mut() {
        let (forward, up) = FACES[face.0];
        *transform = Transform::from_translation(centre).looking_to(forward, up);
        camera.is_active = awake.contains(&face.0);
    }
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::assets::gpu_scans::{GpuTier, TierLevel};
    use crate::states::AppState;

    #[test]
    fn the_probe_renders_one_face_a_frame_on_strong_cards() {
        let mut seen = Vec::new();
        for frame in 0..6 {
            let faces = faces_to_render(TierLevel::Strong, frame, true);
            assert_eq!(faces.len(), 1);
            seen.push(faces[0]);
        }
        seen.sort();
        assert_eq!(seen, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_probe_renders_every_face_on_exceptional_cards() {
        assert_eq!(
            faces_to_render(TierLevel::Exceptional, 7, true),
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn the_probe_rests_when_nobody_is_near_or_on_modest_cards() {
        assert!(faces_to_render(TierLevel::Strong, 3, false).is_empty());
        assert!(faces_to_render(TierLevel::Exceptional, 3, false).is_empty());
        assert!(faces_to_render(TierLevel::Modest, 3, true).is_empty());
    }

    #[test]
    fn re_entering_the_zone_keeps_six_probe_cameras() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_state::<AppState>()
            .insert_resource(GpuTier::fallback(6.0, Some(TierLevel::Strong)))
            .add_systems(OnEnter(AppState::InGame), spawn_reflection_probe);
        let faces = |app: &mut App| {
            app.world_mut()
                .query::<&ProbeFace>()
                .iter(app.world())
                .count()
        };
        for state in [AppState::InGame, AppState::Loading, AppState::InGame] {
            app.world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(state);
            app.update();
        }
        assert_eq!(faces(&mut app), 6);
        let probe = app.world().resource::<ReflectionProbe>();
        assert!(
            probe
                .faces
                .iter()
                .all(|face| face.id() != Handle::<Image>::default().id())
        );
    }
}
