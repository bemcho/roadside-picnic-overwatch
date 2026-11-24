//! Player: the stalker, their camera, and movement controls.

mod animation;
mod camera;
mod orders;
mod path;
mod route;
pub(crate) mod vehicle;

use std::f32::consts::PI;

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::AtmosphereEnvironmentMapLight;
use bevy::pbr::AtmosphereSettings;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;

use crate::assets::STALKER_MODEL;
use crate::states::{AppState, InGamePhase};
use crate::world::{
    Body, CAMERA_FAR, Collider, Crusher, SPAWN, StreamAnchor, TERRAIN_HALF_SIZE, Terrain,
    Unclimbable, Weight, camera_distance_fog, camera_fog, climb_top, fence_out, push_out,
    support_height,
};

pub(crate) use animation::Gait;
use animation::{
    StalkerModel, animate_stalker, attach_stalker_animator, load_stalker_animations,
    pause_stalker_animation, pose_stalker_body, resume_stalker_animation, swing_coat,
};
pub(crate) use camera::OrbitCamera;
use camera::{
    Ears, LOOK_HEIGHT, fit_view_to_zoom, follow_player, grab_cursor_while_orbiting, orbit_camera,
    pan_camera,
};
pub(crate) use orders::cast_to_ground;
use orders::{
    MoveOrder, Turning, issue_move_order, prepare_order_markers, replan_order, sync_order_markers,
};
use vehicle::{
    Driver, Driving, Patrol, Vehicle, drive_carrier, drive_patrol, drive_traffic, drive_vehicle,
    flash_beacons, interact_vehicle, pose_wheels, spawn_carrier, spawn_patrol, spawn_traffic,
    sync_spotlight,
};

pub struct PlayerPlugin;

/// Scales the 0.977-unit model (origin at its soles) to a 1.8 m stalker.
const MODEL_SCALE: f32 = 1.84;

/// Earth gravity, in m/s².
const GRAVITY: f32 = 9.81;

/// How far inside the terrain edge the stalker must stay.
const EDGE_MARGIN: f32 = 2.0;

/// The stalker's body for collision, standing and lying prone.
const STANDING: Body = Body {
    radius: 0.35,
    height: 1.8,
};
const PRONE: Body = Body {
    radius: 0.35,
    height: 0.5,
};

/// The stalker's weight and footprint: what they trample, and the solids light
/// enough for them to walk through.
const STALKER: Crusher = Crusher {
    mass: 90.0,
    half_size: Vec2::splat(0.35),
};

/// How fast the stalker climbs a solid they press against, in m/s.
const CLIMB_SPEED: f32 = 1.2;

/// The solids that stop the stalker: unclimbable ones (`fence`) or the rest.
type Solids<'w, 's> = Query<
    'w,
    's,
    (
        &'static Collider,
        &'static GlobalTransform,
        Option<&'static Weight>,
        Has<Unclimbable>,
    ),
>;

fn solids<'a>(
    colliders: &'a Solids,
    fence: bool,
) -> impl Iterator<Item = (&'a Collider, &'a GlobalTransform)> {
    colliders
        .iter()
        .filter(move |&(_, _, weight, unclimbable)| {
            unclimbable == fence && STALKER.blocked_by(weight)
        })
        .map(|(collider, transform, ..)| (collider, transform))
}

/// The stalker. Movement speeds are data, not magic numbers in systems.
#[derive(Component)]
#[require(Locomotion, MoveOrder, StreamAnchor, Visibility)]
pub struct Player {
    pub walk_speed: f32,
    pub run_speed: f32,
    pub crawl_speed: f32,
    /// Take-off speed of a jump, in m/s.
    pub jump_speed: f32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            walk_speed: 1.8,
            run_speed: 4.5,
            crawl_speed: 0.7,
            jump_speed: 3.65,
        }
    }
}

/// The stalker's vertical motion and current gait.
#[derive(Component, Default)]
pub struct Locomotion {
    planar_velocity: Vec2,
    pub vertical_speed: f32,
    pub grounded: bool,
    pub gait: Gait,
}

/// Sends the stalker back to base, as after dying in the Zone.
#[derive(Message)]
pub struct Respawn;

/// Tags the camera that follows the stalker.
#[derive(Component)]
#[require(OrbitCamera)]
pub struct PlayerCamera;

/// Everything spawned by [`spawn_player`] and torn down by [`despawn_player`].
type PlayerEntities<'w, 's> =
    Query<'w, 's, Entity, Or<(With<Player>, With<PlayerCamera>, With<Ears>, With<Patrol>)>>;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Respawn>()
            .add_plugins(route::RoutePlugin)
            .add_systems(Startup, load_stalker_animations)
            .add_systems(
                OnEnter(AppState::InGame),
                (
                    spawn_player,
                    prepare_order_markers,
                    spawn_patrol,
                    spawn_traffic,
                    spawn_carrier,
                ),
            )
            .add_systems(OnExit(AppState::InGame), despawn_player)
            .add_systems(
                Update,
                (
                    orbit_camera,
                    pan_camera,
                    issue_move_order,
                    replan_order,
                    respawn_player,
                    interact_vehicle,
                    move_player,
                    drive_vehicle,
                    pose_wheels,
                    sync_spotlight,
                    follow_player,
                    fit_view_to_zoom,
                    sync_order_markers,
                    animate_stalker,
                    pose_stalker_body,
                    swing_coat,
                )
                    .chain()
                    .run_if(in_state(InGamePhase::Running)),
            )
            .add_systems(
                Update,
                (drive_patrol, drive_traffic, flash_beacons, drive_carrier)
                    .run_if(in_state(InGamePhase::Running)),
            )
            .add_systems(OnEnter(InGamePhase::Paused), pause_stalker_animation)
            .add_systems(OnExit(InGamePhase::Paused), resume_stalker_animation)
            .add_systems(Update, grab_cursor_while_orbiting);
    }
}

/// Camera yaw at the start of a run: behind the stalker, looking north out
/// of the hangar door into the Zone.
const SPAWN_YAW: f32 = PI;

/// Where the stalker stands at the start of a run and after respawning:
/// inside the base, facing out (model +Z) and away from the camera.
fn spawn_transform(terrain: &Terrain) -> Transform {
    let spawn = Vec3::new(SPAWN.0, terrain.mesh_height_at(Vec2::from(SPAWN)), SPAWN.1);
    Transform::from_translation(spawn)
}

fn spawn_player(mut commands: Commands, asset_server: Res<AssetServer>, terrain: Res<Terrain>) {
    // The stalker: the rigged UHD scan (`STALKER_MODEL`). Its origin is at
    // the soles of its feet.
    let start = spawn_transform(&terrain);
    let spawn = start.translation;
    let player = commands
        .spawn((
            Player::default(),
            Locomotion {
                grounded: true,
                ..default()
            },
            STALKER,
            start,
        ))
        .id();
    commands
        .spawn((
            StalkerModel::new(player),
            WorldAssetRoot(asset_server.load(GltfAssetLabel::Scene(0).from_asset(STALKER_MODEL))),
            Transform::from_scale(Vec3::splat(MODEL_SCALE)),
            ChildOf(player),
        ))
        .observe(attach_stalker_animator);

    // Camera orbiting behind the stalker. The sky and haze come from the
    // world's atmosphere, which needs HDR at sunlight exposure.
    let orbit = OrbitCamera {
        yaw: SPAWN_YAW,
        ..default()
    };
    let focus = spawn + Vec3::Y * LOOK_HEIGHT;
    commands.spawn((
        PlayerCamera,
        // The ground fog pass reads the depth buffer.
        Camera3d {
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: CAMERA_FAR,
            ..default()
        }),
        camera_fog(),
        camera_distance_fog(),
        Transform::from_translation(focus + orbit.offset()).looking_at(focus, Vec3::Y),
        orbit,
        AtmosphereSettings::default(),
        AtmosphereEnvironmentMapLight::default(),
        Exposure { ev100: 13.0 },
        Tonemapping::AcesFitted,
        Bloom::NATURAL,
    ));
    commands.spawn((Ears, Transform::from_translation(focus)));
}

fn despawn_player(mut commands: Commands, player_entities: PlayerEntities) {
    for entity in &player_entities {
        commands.entity(entity).despawn();
    }
}

/// Puts the stalker back at the base, standing, with the camera behind.
fn respawn_player(
    mut commands: Commands,
    mut respawns: MessageReader<Respawn>,
    terrain: Res<Terrain>,
    mut players: Driver,
    mut vehicles: Query<&mut Vehicle>,
    mut cameras: Query<&mut OrbitCamera>,
    mut orders: Query<&mut MoveOrder>,
) {
    if respawns.read().count() == 0 {
        return;
    }
    for mut order in &mut orders {
        order.cancel();
    }
    for (entity, mut transform, mut locomotion, mut visibility, driving) in &mut players {
        if let Some(driving) = driving {
            if let Ok(mut vehicle) = vehicles.get_mut(driving.0) {
                vehicle.park();
            }
            commands.entity(entity).remove::<Driving>();
            *visibility = Visibility::Inherited;
        }
        *transform = spawn_transform(&terrain);
        *locomotion = Locomotion {
            grounded: true,
            ..default()
        };
    }
    for mut orbit in &mut cameras {
        orbit.reset_heading(SPAWN_YAW);
    }
}

/// WASD movement relative to where the camera looks, Shift to run, Space to
/// jump, Ctrl to crawl; without WASD input the stalker follows its
/// point-and-click [`MoveOrder`]. The stalker turns to face the direction of
/// travel, walks through solids lighter than them, climbs the others they press
/// against (all but the unclimbable ones), and stays on the terrain or the top
/// they stand on unless airborne.
fn move_player(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<Terrain>,
    colliders: Solids,
    cameras: Query<&OrbitCamera>,
    mut players: Query<
        (&Player, &mut Locomotion, &mut Transform, &mut MoveOrder),
        Without<Driving>,
    >,
) {
    let Ok((player, mut locomotion, mut transform, mut order)) = players.single_mut() else {
        return;
    };
    let heading = cameras
        .single()
        .map_or(Quat::IDENTITY, OrbitCamera::heading);

    let mut input = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        input -= Vec3::Z;
    }
    if keys.pressed(KeyCode::KeyS) {
        input += Vec3::Z;
    }
    if keys.pressed(KeyCode::KeyA) {
        input -= Vec3::X;
    }
    if keys.pressed(KeyCode::KeyD) {
        input += Vec3::X;
    }

    let ordered = if input == Vec3::ZERO {
        order.steer(transform.translation.xz())
    } else {
        order.cancel();
        None
    };
    let direction = ordered.map_or(heading * input.normalize_or_zero(), |toward| {
        Vec3::new(toward.x, 0.0, toward.y)
    });

    let crawling =
        locomotion.grounded && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let running = !crawling
        && (keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            || ordered.is_some() && order.runs());
    let speed = if crawling {
        player.crawl_speed
    } else if running {
        player.run_speed
    } else {
        player.walk_speed
    };
    let wading = terrain.water_depth_at(transform.translation.x, transform.translation.z);
    let speed = speed * (1.0 - 0.46 * wading.min(1.0));
    let dt = time.delta_secs();
    let desired = direction.xz() * speed;
    let rate = if crawling { 26.0 } else { 18.0 };
    locomotion.planar_velocity = locomotion.planar_velocity.lerp(desired, response(rate, dt));
    if crawling || wading > 0.0 {
        locomotion.planar_velocity = locomotion.planar_velocity.clamp_length_max(speed);
    }
    if desired == Vec2::ZERO && locomotion.planar_velocity.length_squared() < 0.001 {
        locomotion.planar_velocity = Vec2::ZERO;
    }
    // The stalker faces +Z in the rig's resting pose.
    let velocity = locomotion.planar_velocity;
    if velocity.length_squared() > 0.001 {
        let target = Quat::from_rotation_y(velocity.x.atan2(velocity.y));
        transform.rotation = transform.rotation.slerp(target, response(10.0, dt));
    }
    let step = Vec3::new(velocity.x, 0.0, velocity.y) * dt;
    let target = transform.translation + step;
    let feet = target.y;
    let body = if crawling { PRONE } else { STANDING };
    let clear = push_out(target.xz(), feet, body, solids(&colliders, false));
    let clear = fence_out(clear, feet, body, solids(&colliders, true));
    let from = transform.translation.xz();
    let limit = if crawling { 0.28 } else { 1.05 };
    let clear = if terrain.can_wade(from, clear, limit) {
        clear
    } else {
        from
    };
    let ground = ground_position(&terrain, Vec3::new(clear.x, feet, clear.y));
    let top = support_height(ground.xz(), feet, body.radius, solids(&colliders, false));
    let ground = ground.with_y(top.map_or(ground.y, |top| top.max(ground.y)));
    let toward = direction.xz().normalize_or_zero();
    let climb = (!crawling && toward != Vec2::ZERO)
        .then(|| climb_top(target.xz(), feet, body, toward, solids(&colliders, false)))
        .flatten();
    if dt > 0.0 && ground.xz().distance_squared(target.xz()) > 1e-8 {
        locomotion.planar_velocity = (ground.xz() - from).clamp_length_max(step.length()) / dt;
    }
    order.track_progress(
        ground.xz(),
        ground.xz().distance(from),
        speed * dt,
        dt,
        Turning::InPlace,
    );

    if locomotion.grounded && !crawling && keys.just_pressed(KeyCode::Space) {
        locomotion.vertical_speed = player.jump_speed;
    }
    let (y, vertical_speed, grounded) = match climb {
        Some(top) => ((feet + CLIMB_SPEED * dt).min(top), 0.0, true),
        None => fall(
            transform.translation.y,
            locomotion.vertical_speed,
            locomotion.grounded,
            ground.y,
            dt,
        ),
    };
    transform.translation = Vec3::new(ground.x, y, ground.z);
    locomotion.vertical_speed = vertical_speed;
    locomotion.grounded = grounded;
    let moving = ground.xz().distance_squared(from) > 1e-8;
    locomotion.gait = match (grounded, crawling, moving, running) {
        _ if climb.is_some() => Gait::Climb,
        (false, ..) => Gait::Jump,
        (true, true, false, _) => Gait::Prone,
        (true, true, true, _) => Gait::Crawl,
        (true, false, false, _) => Gait::Idle,
        (true, false, true, false) => Gait::Walk,
        (true, false, true, true) => Gait::Run,
    };
}

fn response(rate: f32, dt: f32) -> f32 {
    -(-rate * dt).exp_m1()
}

/// Advances height `y` with vertical speed `vy` over `dt` under gravity,
/// landing on `ground`. Returns the new height, speed, and whether grounded.
///
/// The stalker follows ordinary steps downhill, but falls over larger drops.
fn fall(y: f32, vy: f32, grounded: bool, ground: f32, dt: f32) -> (f32, f32, bool) {
    if grounded && vy <= 0.0 && y <= ground + 0.5 {
        return (ground, 0.0, true);
    }
    let (y, vy) = freefall(y, vy, dt);
    if y <= ground {
        (ground, 0.0, true)
    } else {
        (y, vy, false)
    }
}

fn freefall(y: f32, vy: f32, dt: f32) -> (f32, f32) {
    (y + (vy - 0.5 * GRAVITY * dt) * dt, vy - GRAVITY * dt)
}

/// Keeps a point on the terrain: inside its edges and on its surface.
fn ground_position(terrain: &Terrain, position: Vec3) -> Vec3 {
    let limit = TERRAIN_HALF_SIZE - EDGE_MARGIN;
    let x = position.x.clamp(-limit, limit);
    let z = position.z.clamp(-limit, limit);
    Vec3::new(x, terrain.mesh_height_at(Vec2::new(x, z)), z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalker_cannot_leave_the_terrain() {
        let terrain = Terrain::new();
        let frontier = ground_position(&terrain, Vec3::new(1200.0, 0.0, -1200.0));
        assert_eq!(frontier.xz(), Vec2::new(1200.0, -1200.0));
        let limit = TERRAIN_HALF_SIZE - EDGE_MARGIN;
        let outside = TERRAIN_HALF_SIZE + 20.0;
        let grounded = ground_position(&terrain, Vec3::new(outside, 0.0, -outside));
        assert_eq!((grounded.x, grounded.z), (limit, -limit));
    }

    #[test]
    fn walking_over_hills_keeps_stalker_and_camera_above_ground() {
        use std::time::Duration;

        use bevy::time::TimeUpdateStrategy;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, (move_player, follow_player).chain());

        // Camera swung round so W walks toward +X, over the hills and down
        // into the quarry.
        app.world_mut().spawn((
            PlayerCamera,
            Transform::default(),
            OrbitCamera {
                yaw: -std::f32::consts::FRAC_PI_2,
                ..default()
            },
        ));
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                Locomotion {
                    grounded: true,
                    ..default()
                },
                Transform::from_xyz(0.0, Terrain::new().height_at(0.0, 40.0), 40.0),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);

        let terrain = Terrain::new();
        for _ in 0..400 {
            app.update();
            let feet = app
                .world()
                .get::<Transform>(player)
                .expect("player")
                .translation;
            assert!((feet.y - terrain.mesh_height_at(feet.xz())).abs() < 1e-4);
            let mut cameras = app
                .world_mut()
                .query_filtered::<&Transform, With<PlayerCamera>>();
            let eye = cameras.single(app.world()).expect("camera").translation;
            assert!(
                eye.y > terrain.height_at(eye.x, eye.z),
                "camera underground at {eye}"
            );
        }
        let feet = app
            .world()
            .get::<Transform>(player)
            .expect("player")
            .translation;
        assert!(feet.x > 60.0, "stalker should have run east, at {feet}");
        let locomotion = app.world().get::<Locomotion>(player).expect("locomotion");
        assert_eq!(locomotion.gait, Gait::Run);
    }

    #[test]
    fn move_orders_walk_to_the_target_until_wasd_takes_over() {
        use std::time::Duration;

        use bevy::time::TimeUpdateStrategy;

        let terrain = Terrain::new();
        let start = ground_position(&terrain, Vec3::new(40.0, 0.0, -60.0));
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, move_player);
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                Locomotion {
                    grounded: true,
                    ..default()
                },
                Transform::from_translation(start),
            ))
            .id();
        let target = start.xz() + Vec2::new(4.0, -3.0);
        app.world_mut()
            .get_mut::<MoveOrder>(player)
            .expect("order")
            .give(vec![target], start.xz(), false, orders::STALKER_ARRIVAL);
        for _ in 0..120 {
            app.update();
        }
        let at = app
            .world()
            .get::<Transform>(player)
            .expect("stalker")
            .translation;
        assert!(at.xz().distance(target) < 0.5, "stopped at {at}");
        let mut order = app.world_mut().get_mut::<MoveOrder>(player).expect("order");
        assert_eq!(order.steer(at.xz()), None, "arrival clears the order");

        order.give(
            vec![target + Vec2::new(30.0, 0.0)],
            at.xz(),
            false,
            orders::STALKER_ARRIVAL,
        );
        assert!(order.runs());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyS);
        app.update();
        let mut order = app.world_mut().get_mut::<MoveOrder>(player).expect("order");
        assert_eq!(order.steer(at.xz()), None, "WASD cancels the order");
    }

    #[test]
    fn jump_rises_then_lands_on_the_ground() {
        let dt = 1.0 / 60.0;
        let (mut y, mut vy, mut grounded) = (0.0, Player::default().jump_speed, false);
        let mut peak: f32 = 0.0;
        let mut frames = 0;
        while !grounded {
            (y, vy, grounded) = fall(y, vy, grounded, 0.0, dt);
            peak = peak.max(y);
            frames += 1;
            assert!(frames < 120, "never landed");
        }
        assert_eq!(y, 0.0);
        assert!((0.5..1.0).contains(&peak), "jump peak {peak} m");
    }

    #[test]
    fn walking_downhill_stays_grounded() {
        let (y, vy, grounded) = fall(10.0, 0.0, true, 9.5, 1.0 / 60.0);
        assert_eq!((y, vy, grounded), (9.5, 0.0, true));
    }

    #[test]
    fn stepping_off_a_ledge_falls_instead_of_snapping_down() {
        let (y, vy, grounded) = fall(5.0, 0.0, true, 0.0, 0.25);
        assert!(!grounded);
        assert!((y - 4.6934375).abs() < 0.001);
        assert!((vy + 2.4525).abs() < 0.001);
    }

    /// An app that runs [`move_player`] at 20 Hz for a stalker at (0, 0)
    /// facing -Z, holding `keys`.
    fn moving_app(keys: &[KeyCode]) -> (App, Entity) {
        use std::time::Duration;

        use bevy::time::TimeUpdateStrategy;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(Terrain::new())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, move_player);
        app.world_mut()
            .spawn((Transform::default(), OrbitCamera::default()));
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                Locomotion {
                    grounded: true,
                    ..default()
                },
                Transform::from_xyz(0.0, Terrain::new().height_at(0.0, 0.0), 0.0),
            ))
            .id();
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        for &key in keys {
            input.press(key);
        }
        (app, player)
    }

    fn feet(app: &App, player: Entity) -> Vec3 {
        app.world()
            .get::<Transform>(player)
            .expect("player")
            .translation
    }

    /// A 1.2 m solid, 0.8 m across, 5 m ahead of the stalker.
    fn solid_ahead(app: &mut App) -> f32 {
        use crate::world::Pillar;

        const ROCK: &[Pillar] = &[Pillar::centered(0.4, 1.2)];
        let ground = Terrain::new().height_at(0.0, -5.0);
        let rock = Transform::from_xyz(0.0, ground + 0.6, -5.0);
        app.world_mut()
            .spawn((Collider(ROCK), rock, GlobalTransform::from(rock)));
        ground
    }

    #[test]
    fn stalker_bumps_into_unclimbable_solids() {
        let (mut app, player) = moving_app(&[KeyCode::KeyW]);
        solid_ahead(&mut app);
        let mut walls = app.world_mut().query_filtered::<Entity, With<Collider>>();
        let wall = walls.single(app.world()).expect("one solid");
        app.world_mut().entity_mut(wall).insert(Unclimbable);
        for _ in 0..100 {
            app.update();
            let distance = feet(&app, player).xz().distance(Vec2::new(0.0, -5.0));
            assert!(distance >= 0.75 - 1e-4, "inside the wall, {distance} m off");
        }
        assert!(feet(&app, player).z > -5.0, "walked through the wall");
    }

    #[test]
    fn stalker_climbs_onto_a_rock_and_over_it() {
        let (mut app, player) = moving_app(&[KeyCode::KeyW]);
        let ground = solid_ahead(&mut app);
        let (mut climbed, mut highest) = (false, f32::MIN);
        for _ in 0..140 {
            app.update();
            let gait = app
                .world()
                .get::<Locomotion>(player)
                .expect("locomotion")
                .gait;
            climbed |= gait == Gait::Climb;
            highest = highest.max(feet(&app, player).y - ground);
        }
        assert!(climbed, "never climbed");
        assert!(highest > 1.15, "never stood on the rock, {highest} m up");
        assert!(feet(&app, player).z < -5.8, "stuck at the rock");
    }

    #[test]
    fn ctrl_crawls_slowly_and_cannot_jump() {
        let (mut app, player) = moving_app(&[KeyCode::ControlLeft, KeyCode::KeyW, KeyCode::Space]);
        for _ in 0..40 {
            app.update();
        }
        let locomotion = app.world().get::<Locomotion>(player).expect("locomotion");
        assert_eq!(locomotion.gait, Gait::Crawl);
        assert!(locomotion.grounded);
        let travelled = feet(&app, player).xz().length();
        let crawl_speed = Player::default().crawl_speed;
        assert!(
            (1.30..crawl_speed * 39.0 * 0.05).contains(&travelled),
            "{travelled} m"
        );
    }

    #[test]
    fn movement_eases_in_and_stops_without_a_long_slide() {
        let (mut app, player) = moving_app(&[KeyCode::KeyW, KeyCode::ShiftLeft]);
        app.update();
        let start = feet(&app, player);
        app.update();
        let first = start.distance(feet(&app, player));
        assert!((0.05..0.20).contains(&first));
        for _ in 0..15 {
            app.update();
        }
        let release = feet(&app, player);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        for _ in 0..12 {
            app.update();
        }
        let stopped = feet(&app, player);
        assert!((0.08..0.3).contains(&release.xz().distance(stopped.xz())));
        app.update();
        assert_eq!(feet(&app, player), stopped);
    }

    #[test]
    fn respawn_returns_the_stalker_to_base() {
        let (mut app, player) = moving_app(&[]);
        app.add_message::<Respawn>()
            .add_systems(Update, respawn_player.before(move_player));
        app.world_mut()
            .get_mut::<Locomotion>(player)
            .expect("locomotion")
            .gait = Gait::Crawl;
        app.world_mut().write_message(Respawn);
        app.update();
        let at = feet(&app, player);
        let terrain = Terrain::new();
        assert_eq!(at, spawn_transform(&terrain).translation);
        let locomotion = app.world().get::<Locomotion>(player).expect("locomotion");
        assert_eq!((locomotion.gait, locomotion.grounded), (Gait::Idle, true));
    }
}
