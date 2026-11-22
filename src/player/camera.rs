//! Tactical orbit camera: scroll zooms from over the shoulder out to the whole
//! map (pitch steepens with the zoom), right-drag rotates, and the arrow keys,
//! middle-drag or the screen edges roam freely; F returns to following the stalker.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::audio::SpatialListener;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::states::InGamePhase;
use crate::world::{CAMERA_FAR, StreamAnchor, TERRAIN_HALF_SIZE, Terrain, ViewFocus};

use super::{Player, response, vehicle::Driving};

/// Height above the stalker's feet the camera orbits around and looks at.
pub(super) const LOOK_HEIGHT: f32 = 1.6;

/// Radians of rotation per pixel of mouse motion.
const MOUSE_SENSITIVITY: f32 = 0.004;

/// Allowed camera pitch: from slightly below the look point to near top-down.
const PITCH_LIMITS: (f32, f32) = (-0.25, FRAC_PI_2 - 0.1);

/// Allowed orbit distance, in meters: over the shoulder out to the whole map.
const DISTANCE_LIMITS: (f32, f32) = (6.0, 4_600.0);
/// Zoom distance of the tactical view; pitch rises fastest below it.
const TACTICAL_DISTANCE: f32 = 90.0;
/// Pitch over the shoulder, at the tactical view and at the full-map overview.
const ZOOM_PITCH: (f32, f32, f32) = (0.3, 0.98, 1.4);
/// The view focus streams nearby detail only while zoomed in this close.
const DETAIL_ZOOM: f32 = 250.0;
/// Start of a run: a raised view a little above the stalker.
const START_DISTANCE: f32 = 28.0;
/// Window border, in pixels, that pans the view while the cursor rests in it.
const EDGE_MARGIN: f32 = 6.0;
/// Edge panning speed per meter of zoom distance, in meters per second.
const EDGE_PAN_RATE: f32 = 1.1;

/// Smallest gap kept between the camera and the ground below it.
const GROUND_CLEARANCE: f32 = 0.5;
const MAX_FOLLOW_LAG: f32 = 1.4;

/// Orbit around the stalker. `yaw` 0 places the camera behind a stalker
/// walking toward -Z; positive `pitch` raises it.
#[derive(Component)]
#[require(CameraFollow)]
pub struct OrbitCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Ground point the view roams over, or `None` to follow the stalker.
    pub(super) free_focus: Option<Vec2>,
    pub(super) pending_yaw: f32,
    pub(super) reset_follow: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: zoom_pitch(START_DISTANCE),
            distance: START_DISTANCE,
            free_focus: None,
            pending_yaw: 0.0,
            reset_follow: true,
        }
    }
}

impl OrbitCamera {
    /// Camera position relative to the look point.
    pub fn offset(&self) -> Vec3 {
        Quat::from_euler(EulerRot::YXZ, self.yaw, -self.pitch, 0.0) * Vec3::Z * self.distance
    }

    /// Ground-plane rotation matching where the camera looks.
    pub fn heading(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }

    pub(super) fn face_vehicle(&mut self, forward: Vec3) {
        let target = (-forward.x).atan2(-forward.z);
        self.pending_yaw = (target - self.yaw + PI).rem_euclid(TAU) - PI;
    }

    /// Whether the view roams freely instead of following the stalker.
    pub fn roaming(&self) -> bool {
        self.free_focus.is_some()
    }

    pub(super) fn cancel_assist(&mut self) {
        self.pending_yaw = 0.0;
    }

    pub(super) fn reset_heading(&mut self, yaw: f32) {
        self.yaw = yaw;
        self.free_focus = None;
        self.cancel_assist();
        self.reset_follow = true;
    }

    pub(super) fn record_turn(&mut self, turn: f32) {
        self.pending_yaw += turn;
    }

    fn ease_heading(&mut self, dt: f32) {
        let yaw = self.pending_yaw * response(10.0, dt);
        self.yaw += yaw;
        if self.yaw.abs() > PI {
            self.yaw = (self.yaw + PI).rem_euclid(TAU) - PI;
        }
        self.pending_yaw -= yaw;
    }
}

/// Hears the Zone from the camera's focus, not from the raised camera itself.
#[derive(Component)]
#[require(SpatialListener::new(0.85), Transform, ViewFocus)]
pub(super) struct Ears;

type EarsPose<'w, 's> =
    Query<'w, 's, &'static mut Transform, (With<Ears>, Without<Player>, Without<OrbitCamera>)>;

#[derive(Component, Default)]
pub(super) struct CameraFollow {
    focus: Vec3,
    distance: f32,
}

/// Rotates the orbit while the right mouse button is held; zooms on scroll.
pub(super) fn orbit_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let Ok(mut orbit) = cameras.single_mut() else {
        return;
    };

    // Skip the press frame: motion accumulated before the grab (or caused by
    // it) would otherwise jerk the view.
    if mouse.pressed(MouseButton::Right) {
        orbit.pending_yaw = 0.0;
        if !mouse.just_pressed(MouseButton::Right) {
            orbit.yaw -= motion.delta.x * MOUSE_SENSITIVITY;
            orbit.pitch = (orbit.pitch + motion.delta.y * MOUSE_SENSITIVITY)
                .clamp(PITCH_LIMITS.0, PITCH_LIMITS.1);
        }
    }

    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if notches != 0.0 {
        orbit.distance =
            (orbit.distance * 0.84_f32.powf(notches)).clamp(DISTANCE_LIMITS.0, DISTANCE_LIMITS.1);
        orbit.pitch = zoom_pitch(orbit.distance);
    }
}

/// Pitch for a zoom distance: low behind the stalker, steep from far away.
fn zoom_pitch(distance: f32) -> f32 {
    let along = |low: f32, high: f32| ((distance / low).ln() / (high / low).ln()).clamp(0.0, 1.0);
    let (close, tactical, overview) = ZOOM_PITCH;
    if distance <= TACTICAL_DISTANCE {
        close + (tactical - close) * along(DISTANCE_LIMITS.0, TACTICAL_DISTANCE)
    } else {
        tactical + (overview - tactical) * along(TACTICAL_DISTANCE, DISTANCE_LIMITS.1)
    }
}

/// Roams the view with the arrow keys, a middle-button drag or the window edges.
/// F toggles between the free camera (held where it is) and following the stalker.
pub(super) fn pan_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    players: Query<&Transform, With<Player>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    let Ok(mut orbit) = cameras.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyF) {
        orbit.free_focus = match orbit.free_focus {
            Some(_) => None,
            None => players.single().ok().map(|player| player.translation.xz()),
        };
        return;
    }
    // Screen right and screen up, projected onto the ground.
    let heading = orbit.heading();
    let right = (heading * Vec3::X).xz();
    let up = (heading * Vec3::NEG_Z).xz();
    let axis = |positive: KeyCode, negative: KeyCode| {
        f32::from(keys.pressed(positive)) - f32::from(keys.pressed(negative))
    };
    let mut push = right * axis(KeyCode::ArrowRight, KeyCode::ArrowLeft)
        + up * axis(KeyCode::ArrowUp, KeyCode::ArrowDown);
    let mut shift = Vec2::ZERO;
    if mouse.pressed(MouseButton::Middle) {
        let meters_per_pixel = orbit.distance * 0.0016;
        shift -= (right * motion.delta.x - up * motion.delta.y) * meters_per_pixel;
    } else if let Some(cursor) = window.focused.then(|| window.cursor_position()).flatten() {
        let size = window.size();
        let edge = |at: f32, extent: f32| {
            f32::from(at >= extent - EDGE_MARGIN) - f32::from(at <= EDGE_MARGIN)
        };
        push += right * edge(cursor.x, size.x) - up * edge(cursor.y, size.y);
    }
    shift += push.clamp_length_max(1.0) * orbit.distance * EDGE_PAN_RATE * time.delta_secs();
    if shift == Vec2::ZERO {
        return;
    }
    let Some(from) = orbit
        .free_focus
        .or_else(|| players.single().ok().map(|player| player.translation.xz()))
    else {
        return;
    };
    let limit = Vec2::splat(TERRAIN_HALF_SIZE);
    orbit.free_focus = Some((from + shift).clamp(-limit, limit));
}

/// Places the camera on its orbit and keeps it from sinking into hillsides.
pub(super) fn follow_player(
    time: Res<Time>,
    terrain: Res<Terrain>,
    players: Query<(&Transform, Option<&Driving>), With<Player>>,
    mut cameras: Query<(&mut OrbitCamera, &mut CameraFollow, &mut Transform), Without<Player>>,
    mut ears: EarsPose,
) {
    let Ok((player, driving)) = players.single() else {
        return;
    };
    let Ok((mut orbit, mut follow, mut camera)) = cameras.single_mut() else {
        return;
    };
    let seated = driving.is_some();
    let dt = time.delta_secs();
    orbit.ease_heading(dt);
    let height = if seated { 2.1 } else { LOOK_HEIGHT };
    let focus = match orbit.free_focus {
        Some(at) => at.extend(terrain.mesh_height_at(at) + LOOK_HEIGHT).xzy(),
        None => player.translation + Vec3::Y * height,
    };
    if orbit.reset_follow {
        follow.focus = focus;
        follow.distance = orbit.distance;
        orbit.reset_follow = false;
    } else {
        let follow_rate = if seated {
            Vec3::new(12.0, 7.0, 12.0)
        } else {
            Vec3::splat(22.0)
        };
        let blend = follow_rate.to_array().map(|rate| response(rate, dt));
        let lag = (follow.focus - focus) * (Vec3::ONE - Vec3::from_array(blend));
        follow.focus = focus + lag.clamp_length_max(MAX_FOLLOW_LAG);
        follow.distance += (orbit.distance - follow.distance) * response(14.0, dt);
    }
    let mut position = follow.focus + orbit.offset() * (follow.distance / orbit.distance);
    let floor = terrain
        .height_at(position.x, position.z)
        .max(terrain.mesh_height_at(position.xz()))
        + GROUND_CLEARANCE;
    position.y = position.y.max(floor);

    camera.translation = position;
    camera.look_at(follow.focus, Vec3::Y);
    for mut ears in &mut ears {
        *ears = Transform::from_translation(follow.focus).with_rotation(camera.rotation);
    }
}

/// Keeps the far plane, the sun's shadow cascades and the focus's detail
/// streaming in step with the zoom.
pub(super) fn fit_view_to_zoom(
    mut commands: Commands,
    mut cameras: Query<(&OrbitCamera, &mut Projection)>,
    ears: Query<Entity, With<Ears>>,
    mut suns: Query<&mut CascadeShadowConfig, With<DirectionalLight>>,
    mut fitted: Local<f32>,
) {
    let Ok((orbit, mut projection)) = cameras.single_mut() else {
        return;
    };
    let distance = orbit.distance;
    if (distance / *fitted - 1.0).abs() < 0.05 && !suns.is_empty() {
        return;
    }
    *fitted = distance;
    if let Projection::Perspective(perspective) = &mut *projection {
        perspective.far = CAMERA_FAR.max(distance * 2.5 + 800.0);
    }
    for mut cascades in &mut suns {
        *cascades = CascadeShadowConfigBuilder {
            maximum_distance: (distance * 2.5).clamp(150.0, 3_000.0),
            first_cascade_far_bound: (distance * 0.5).max(10.0),
            ..default()
        }
        .build();
    }
    for ears in &ears {
        if distance < DETAIL_ZOOM {
            commands.entity(ears).insert(StreamAnchor);
        } else {
            commands.entity(ears).remove::<StreamAnchor>();
        }
    }
}

/// Hides and locks the cursor while orbiting, as third-person games do.
pub(super) fn grab_cursor_while_orbiting(
    mouse: Res<ButtonInput<MouseButton>>,
    phase: Option<Res<State<InGamePhase>>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let running = phase.is_some_and(|phase| *phase.get() == InGamePhase::Running);
    let orbiting = running && mouse.pressed(MouseButton::Right);
    let grab_mode = if orbiting {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != grab_mode {
        cursor.grab_mode = grab_mode;
        cursor.visible = !orbiting;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BASE;

    #[test]
    fn vehicle_alignment_eases_along_the_short_arc() {
        let mut orbit = OrbitCamera {
            yaw: PI - 0.1,
            ..default()
        };
        let target = -PI + 0.1;
        orbit.face_vehicle(Quat::from_rotation_y(target) * Vec3::NEG_Z);
        orbit.ease_heading(0.02);
        assert!(orbit.yaw > PI - 0.1 && orbit.yaw < PI);
        for _ in 0..50 {
            orbit.ease_heading(0.02);
        }
        assert!((orbit.yaw - target).abs() < 0.001);
    }

    #[test]
    fn chase_focus_and_zoom_ease_but_respawn_resets_follow() {
        let terrain = Terrain::new();
        let base = Vec3::new(BASE.0, terrain.height_at(BASE.0, BASE.1), BASE.1);
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs_f32(0.05));
        app.insert_resource(time)
            .insert_resource(terrain)
            .add_systems(Update, follow_player);
        let world = app.world_mut();
        let car = world.spawn_empty().id();
        let player = world
            .spawn((
                Player::default(),
                Transform::from_translation(base),
                Driving(car),
            ))
            .id();
        let camera = world
            .spawn((OrbitCamera::default(), Transform::IDENTITY))
            .id();
        app.update();

        let world = app.world_mut();
        world
            .get_mut::<Transform>(player)
            .expect("player")
            .translation
            .x += 1.0;
        world
            .get_mut::<OrbitCamera>(camera)
            .expect("orbit")
            .distance = 12.0;
        app.update();
        let follow = app.world().get::<CameraFollow>(camera).expect("follow");
        assert!((base.x..base.x + 0.8).contains(&follow.focus.x));
        assert!((12.0..START_DISTANCE).contains(&follow.distance));

        let world = app.world_mut();
        world
            .get_mut::<Transform>(player)
            .expect("player")
            .translation = base;
        world.entity_mut(player).remove::<Driving>();
        world
            .get_mut::<OrbitCamera>(camera)
            .expect("orbit")
            .reset_heading(0.0);
        app.update();
        let follow = app.world().get::<CameraFollow>(camera).expect("follow");
        assert_eq!(follow.focus, base + Vec3::Y * LOOK_HEIGHT);
        assert_eq!(follow.distance, 12.0);
    }

    #[test]
    fn default_orbit_sits_behind_and_above_the_stalker() {
        let offset = OrbitCamera::default().offset();
        assert!(offset.z > 0.0 && offset.y > 0.0, "offset {offset}");
        assert!(offset.x.abs() < 1e-5);
        assert!((offset.length() - START_DISTANCE).abs() < 1e-4);
    }

    #[test]
    fn panning_roams_freely_inside_the_map_and_f_follows_again() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .add_systems(Update, pan_camera);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.world_mut()
            .spawn((Player::default(), Transform::from_xyz(40.0, 0.0, -120.0)));
        let camera = app.world_mut().spawn(OrbitCamera::default()).id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Middle);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(0.0, 1.0e7);
        app.update();
        let focus = app
            .world()
            .get::<OrbitCamera>(camera)
            .expect("orbit")
            .free_focus;
        // Dragging the ground down roams forward (-Z at yaw 0), up to the map edge.
        assert_eq!(focus, Some(Vec2::new(40.0, -TERRAIN_HALF_SIZE)));

        let press_f = |app: &mut App| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::KeyF);
            keys.clear();
            keys.press(KeyCode::KeyF);
            app.update();
            app.world()
                .get::<OrbitCamera>(camera)
                .expect("orbit")
                .free_focus
        };
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Middle);
        assert_eq!(press_f(&mut app), None);
        // F alone detaches the view where the stalker stands.
        assert_eq!(press_f(&mut app), Some(Vec2::new(40.0, -120.0)));
    }

    #[test]
    fn zooming_out_tilts_from_behind_the_shoulder_to_a_map_overview() {
        let pitches: Vec<f32> = [6.0, 30.0, 90.0, 600.0, 4_600.0].map(zoom_pitch).to_vec();
        assert!(
            pitches.windows(2).all(|pair| pair[0] < pair[1]),
            "{pitches:?}"
        );
        assert!(pitches[0] < 0.35 && pitches[4] > 1.35 && pitches[4] < FRAC_PI_2);
    }

    #[test]
    fn heading_turns_forward_with_the_camera() {
        let orbit = OrbitCamera {
            yaw: FRAC_PI_2,
            ..default()
        };
        // Camera swung round to the stalker's +X side, so forward is -X.
        let forward = orbit.heading() * Vec3::NEG_Z;
        assert!((forward - Vec3::NEG_X).length() < 1e-5, "forward {forward}");
        assert!(orbit.offset().x > 0.0);
    }
}
