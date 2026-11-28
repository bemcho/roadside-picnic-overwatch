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
/// The Sphere's shadow caster, its medium model, on a layer no camera draws:
/// only the sun's shadows see it. The shown Sphere casts none, since the UHD
/// scan's two million triangles would fill every camera's cascades, the six
/// probe faces' included (they sit inside it).
pub(crate) const SPHERE_SHADOW_LAYER: usize = 2;
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
/// How far the player's camera can be and still see the Sphere, in metres
/// (its `detail_range`): beyond it the probe sleeps.
pub(super) const SEEN: f32 = 300.0;
/// While nobody is near, one face every this many frames: the whole cube
/// within about two seconds, so the light and the departed stalker never
/// linger in it.
const RESTING_EVERY: u32 = 10;

/// Who could see the probe's faces in the Sphere and the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Watch {
    /// The player's camera is too far to see the Sphere.
    Unseen,
    /// Seen, with nobody near enough to show in it.
    Resting,
    /// Seen, with the stalker or a vehicle near enough to show in it.
    Close,
}

/// The watch for a player's camera `viewer` metres from the Sphere (`None`
/// without one), and whether someone is near it.
pub(super) fn watch(viewer: Option<f32>, someone_near: bool) -> Watch {
    match viewer {
        Some(distance) if distance <= SEEN && someone_near => Watch::Close,
        Some(distance) if distance <= SEEN => Watch::Resting,
        _ => Watch::Unseen,
    }
}

/// The six faces the probe renders into.
#[derive(Resource)]
pub(super) struct ReflectionProbe {
    pub faces: [Handle<Image>; 6],
}

/// One of the probe's cameras, rendering face `0`.
#[derive(Component)]
pub(super) struct ProbeFace(pub usize);

/// The faces to render this `frame`: none on modest GPUs or unseen, one now
/// and then while resting, and one a frame in turn while someone is close.
/// Exceptional GPUs too: each face is a full view with its own sun cascades,
/// about 7 ms on a 2060, so all six a frame fell to 17 FPS where one holds 42.
pub(super) fn faces_to_render(level: TierLevel, frame: u32, watch: Watch) -> &'static [usize] {
    let one = |turn: u32| {
        let face = turn as usize % ALL_FACES.len();
        &ALL_FACES[face..=face]
    };
    match (level, watch) {
        (TierLevel::Modest, _) | (_, Watch::Unseen) => &[],
        (_, Watch::Resting) if frame.is_multiple_of(RESTING_EVERY) => one(frame / RESTING_EVERY),
        (_, Watch::Resting) => &[],
        (_, Watch::Close) => one(frame),
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
            // The player's camera's exposure each frame, so the faces' light
            // matches the view they are seen in (`steer_reflection_probe`).
            Exposure::default(),
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            // One cluster: lamps barely show in the reflection, and it keeps
            // the CPU's light assignment for six more views at next to nothing
            // (`render::use_cpu_clustering`).
            ClusterConfig::Single,
            RenderLayers::layer(0),
            Transform::default(),
            DespawnOnExit(AppState::InGame),
        ));
    }
}

/// Moves the probe's cameras to the Sphere at `centre`, gives them the
/// player's `exposure` and wakes the faces this frame renders.
pub(super) fn steer_reflection_probe(
    centre: Vec3,
    watch: Watch,
    level: TierLevel,
    frame: &FrameCount,
    exposure: Exposure,
    cameras: &mut Query<(&ProbeFace, &mut Camera, &mut Transform, &mut Exposure)>,
) {
    let awake = faces_to_render(level, frame.0, watch);
    for (face, mut camera, mut transform, mut seen_at) in cameras.iter_mut() {
        let (forward, up) = FACES[face.0];
        *transform = Transform::from_translation(centre).looking_to(forward, up);
        camera.is_active = awake.contains(&face.0);
        *seen_at = exposure;
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::assets::gpu_scans::{GpuTier, TierLevel};
    use crate::states::AppState;

    #[test]
    fn the_probe_renders_one_face_a_frame_on_strong_and_exceptional_cards() {
        for level in [TierLevel::Strong, TierLevel::Exceptional] {
            let mut seen = Vec::new();
            for frame in 0..6 {
                let faces = faces_to_render(level, frame, Watch::Close);
                assert_eq!(faces.len(), 1, "{level:?}");
                seen.push(faces[0]);
            }
            seen.sort();
            assert_eq!(seen, [0, 1, 2, 3, 4, 5], "{level:?}: each face once");
        }
    }

    #[test]
    fn a_resting_probe_refreshes_a_face_now_and_then_so_the_cube_never_goes_stale() {
        for level in [TierLevel::Strong, TierLevel::Exceptional] {
            let mut seen = Vec::new();
            for frame in 0..6 * RESTING_EVERY {
                seen.extend_from_slice(faces_to_render(level, frame, Watch::Resting));
            }
            seen.sort();
            assert_eq!(seen, [0, 1, 2, 3, 4, 5], "{level:?}: each face once");
        }
    }

    #[test]
    fn the_probe_sleeps_unseen_or_on_modest_cards() {
        assert!(faces_to_render(TierLevel::Strong, 0, Watch::Unseen).is_empty());
        assert!(faces_to_render(TierLevel::Exceptional, 0, Watch::Unseen).is_empty());
        assert!(faces_to_render(TierLevel::Modest, 0, Watch::Close).is_empty());
    }

    #[test]
    fn the_probe_wakes_only_while_the_player_can_see_the_sphere() {
        assert_eq!(watch(Some(SEEN), true), Watch::Close);
        assert_eq!(watch(Some(SEEN), false), Watch::Resting);
        assert_eq!(
            watch(Some(SEEN + 1.0), true),
            Watch::Unseen,
            "a car parked by the Sphere while the player looks elsewhere"
        );
        assert_eq!(watch(None, true), Watch::Unseen);
    }

    #[test]
    fn the_faces_take_the_players_exposure() {
        let mut world = World::new();
        world.insert_resource(FrameCount(0));
        for face in 0..6 {
            world.spawn((
                ProbeFace(face),
                Camera::default(),
                Transform::default(),
                Exposure { ev100: 13.0 },
            ));
        }
        let night = Exposure { ev100: 9.0 };
        world
            .run_system_once(
                move |frame: Res<FrameCount>,
                      mut cameras: Query<(
                    &ProbeFace,
                    &mut Camera,
                    &mut Transform,
                    &mut Exposure,
                )>| {
                    steer_reflection_probe(
                        Vec3::ZERO,
                        Watch::Close,
                        TierLevel::Strong,
                        &frame,
                        night,
                        &mut cameras,
                    );
                },
            )
            .expect("steer the probe");
        let mut exposures = world.query::<&Exposure>();
        assert!(exposures.iter(&world).all(|exposure| exposure.ev100 == 9.0));
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
