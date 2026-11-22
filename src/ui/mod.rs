//! UI: HUD and menus.
//!
//! All player-facing text lives in [`text`] as localization-ready constants —
//! never hardcode display strings in systems.

mod curtain;
mod locator;
mod map;
mod splash;
pub mod text;
mod tooltip;

use bevy::diagnostic::{Diagnostic, DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::audio::AudioMode;
use crate::player::{
    Locomotion, OrbitCamera, Player,
    vehicle::{Driving, Vehicle, can_enter_vehicle},
};
use crate::save::ProgressSaved;
use crate::states::{AppState, InGamePhase};
use crate::world::{ArtifactDisplay, Sky, Weather, WorldClock};
use crate::zone::ZoneRun;

const ARTIFACT_READ_RANGE: f32 = 3.2;

pub struct UiPlugin;

/// Root marker for the in-game HUD.
#[derive(Component)]
struct UiRoot;

/// Marks the HUD text node that shows per-run Zone statistics.
#[derive(Component)]
struct HudStats;

#[derive(Component)]
struct HudFps;

/// Clock, temperature, humidity and sky under the Zone statistics.
#[derive(Component)]
struct HudWeather;

#[derive(Component)]
struct VehiclePrompt;

#[derive(Component)]
struct HudControls;

/// Banner shown while the camera roams free of the stalker.
#[derive(Component)]
struct HudFreeCamera;

/// "Progress saved", shown for a moment after each save.
#[derive(Component, Default)]
struct HudSaved {
    seconds_left: f32,
}

const SAVED_NOTICE_SECONDS: f32 = 2.5;

#[derive(Component)]
struct ArtifactReadout;

#[derive(Component)]
struct ArtifactReadoutTitle;

#[derive(Component)]
struct ArtifactReadoutDescription;

/// Root marker for the pause menu overlay.
#[derive(Component)]
struct PauseMenuRoot;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            splash::SplashPlugin,
            curtain::CurtainPlugin,
            FrameTimeDiagnosticsPlugin::new(20),
        ))
        .add_systems(
            OnEnter(AppState::InGame),
            (spawn_hud, locator::spawn_locator, map::spawn_map),
        )
        .add_systems(OnExit(AppState::InGame), (despawn_hud, map::despawn_map))
        .add_systems(
            Update,
            (map::toggle_map, map::update_map, map::fly_map_birds)
                .chain()
                .run_if(in_state(InGamePhase::Running)),
        )
        .add_systems(
            Update,
            (update_hud_stats, update_hud_weather).run_if(in_state(InGamePhase::Running)),
        )
        .add_systems(Update, update_fps.run_if(in_state(AppState::InGame)))
        .add_systems(
            PostUpdate,
            (
                update_vehicle_prompt,
                update_artifact_readout,
                update_free_camera_banner,
                show_saved_notice,
                tooltip::update_tooltip,
                locator::update_locator,
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(OnEnter(InGamePhase::Paused), spawn_pause_menu)
        .add_systems(OnExit(InGamePhase::Paused), despawn_pause_menu);
    }
}

fn spawn_hud(mut commands: Commands) {
    let root = commands
        .spawn((
            UiRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ))
        .id();

    commands.entity(root).with_children(|hud| {
        // Top-left corner: place title.
        hud.spawn((
            Text::new(text::HUD_TITLE),
            TextFont {
                font_size: FontSize::Px(24.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.85, 0.8)),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
        ));

        // Below the title: live Zone statistics, refreshed each frame.
        hud.spawn((
            HudStats,
            Text::new(text::hud_zone_stats(0, 0)),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.75, 0.75, 0.7)),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(44.0),
                left: Val::Px(12.0),
                ..default()
            },
        ));

        hud.spawn((
            HudWeather,
            Text::default(),
            TextFont {
                font_size: FontSize::Px(17.0),
                ..default()
            },
            TextColor(Color::srgb(0.78, 0.84, 0.86)),
            TextShadow {
                offset: Vec2::splat(1.0),
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(70.0),
                left: Val::Px(12.0),
                ..default()
            },
        ));

        hud.spawn((
            HudFps,
            Text::new(text::HUD_FPS_PENDING),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.85, 0.8)),
            TextShadow {
                offset: Vec2::splat(1.0),
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                right: Val::Px(12.0),
                ..default()
            },
        ));

        // Bottom-left corner: controls hint.
        hud.spawn((
            HudControls,
            Text::new(text::HUD_CONTROLS_HINT),
            TextFont {
                font_size: FontSize::Px(16.0),
                ..default()
            },
            TextColor(Color::srgb(0.7, 0.7, 0.65)),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
        ));

        hud.spawn((
            HudFreeCamera,
            Text::new(text::HUD_FREE_CAMERA),
            Visibility::Hidden,
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.97, 0.9, 0.66)),
            TextShadow {
                offset: Vec2::splat(1.0),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Px(14.0),
                ..default()
            },
        ));

        hud.spawn((
            HudSaved::default(),
            Text::new(text::PROGRESS_SAVED),
            Visibility::Hidden,
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.85, 0.85, 0.8)),
            TextShadow {
                offset: Vec2::splat(1.0),
                ..default()
            },
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                bottom: Val::Px(64.0),
                ..default()
            },
        ));

        hud.spawn((
            VehiclePrompt,
            Text::default(),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(Color::srgb(0.97, 0.9, 0.66)),
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                bottom: Val::Percent(14.0),
                ..default()
            },
        ));

        tooltip::spawn_tooltip(hud);

        hud.spawn((
            ArtifactReadout,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(24.0),
                top: Val::Percent(18.0),
                width: Val::Percent(52.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(18.0), Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.66, 0.52, 0.30)),
            BackgroundColor(Color::srgba(0.025, 0.035, 0.042, 0.94)),
        ))
        .with_children(|readout| {
            readout.spawn((
                Text::new(text::ARTIFACT_READOUT_LABEL),
                TextLayout::justify(Justify::Center),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(Color::srgb(0.72, 0.76, 0.78)),
            ));
            readout.spawn((
                ArtifactReadoutTitle,
                Text::default(),
                TextLayout::justify(Justify::Center),
                TextFont {
                    font_size: FontSize::Px(24.0),
                    ..default()
                },
                TextColor(Color::srgb(0.98, 0.80, 0.34)),
            ));
            readout.spawn((
                ArtifactReadoutDescription,
                Text::default(),
                TextLayout::justify(Justify::Center),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(Color::srgb(0.90, 0.90, 0.84)),
                Node {
                    width: Val::Percent(100.0),
                    ..default()
                },
            ));
        });
    });
}

fn despawn_hud(mut commands: Commands, roots: Query<Entity, With<UiRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
}

/// Refreshes the HUD statistics line from [`ZoneRun`].
fn update_hud_stats(run: Res<ZoneRun>, mut stats: Query<&mut Text, With<HudStats>>) {
    let Ok(mut text) = stats.single_mut() else {
        return;
    };
    **text = text::hud_zone_stats(run.artifacts_collected, run.anomalies_triggered);
}

fn update_hud_weather(
    clock: Option<Res<WorldClock>>,
    weather: Option<Res<Weather>>,
    mut line: Single<&mut Text, With<HudWeather>>,
) {
    let (Some(clock), Some(weather)) = (clock, weather) else {
        return;
    };
    let sky = match weather.sky {
        Sky::Clear => text::SKY_CLEAR,
        Sky::Humid => text::SKY_HUMID,
        Sky::Overcast => text::SKY_OVERCAST,
        Sky::Rain => text::SKY_RAIN,
    };
    let updated = text::hud_weather(clock.hour(), weather.temperature, weather.humidity, sky);
    if line.0 != updated {
        line.0 = updated;
    }
}

fn update_fps(diagnostics: Res<DiagnosticsStore>, mut fps: Single<&mut Text, With<HudFps>>) {
    let Some(milliseconds) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(Diagnostic::smoothed)
        .filter(|&value| value.is_finite() && value > 0.0)
    else {
        return;
    };
    let line = text::hud_fps(1000.0 / milliseconds);
    if fps.0 != line {
        fps.0 = line;
    }
}

fn update_vehicle_prompt(
    phase: Res<State<InGamePhase>>,
    audio: Res<AudioMode>,
    player: Query<(&Transform, &Locomotion, Option<&Driving>), With<Player>>,
    vehicles: Query<&Transform, With<Vehicle>>,
    mut prompts: Query<&mut Text, (With<VehiclePrompt>, Without<HudControls>)>,
    mut controls: Query<&mut Text, With<HudControls>>,
) {
    let (Ok((feet, locomotion, driving)), Ok(mut prompt), Ok(mut controls)) =
        (player.single(), prompts.single_mut(), controls.single_mut())
    else {
        return;
    };
    let driving = driving.is_some();
    let hint = if *phase.get() == InGamePhase::Paused {
        ""
    } else if driving {
        text::HUD_EXIT_VEHICLE
    } else if vehicles
        .iter()
        .any(|at| can_enter_vehicle(locomotion, at, feet.translation))
    {
        text::HUD_ENTER_VEHICLE
    } else {
        ""
    };
    let controls_hint = match (driving, *audio == AudioMode::Muted) {
        (false, false) => text::HUD_CONTROLS_HINT,
        (false, true) => text::HUD_CONTROLS_MUTED,
        (true, false) => text::HUD_DRIVER_CONTROLS,
        (true, true) => text::HUD_DRIVER_MUTED,
    };
    if prompt.0 != hint {
        prompt.0 = hint.to_owned();
    }
    if controls.0 != controls_hint {
        controls.0 = controls_hint.to_owned();
    }
}

fn show_saved_notice(
    time: Res<Time>,
    mut saves: MessageReader<ProgressSaved>,
    notice: Single<(&mut HudSaved, &mut Visibility)>,
) {
    let (mut notice, mut visibility) = notice.into_inner();
    if saves.read().count() > 0 {
        notice.seconds_left = SAVED_NOTICE_SECONDS;
    }
    notice.seconds_left = (notice.seconds_left - time.delta_secs()).max(0.0);
    visibility.set_if_neq(if notice.seconds_left > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

fn update_free_camera_banner(
    cameras: Query<&OrbitCamera>,
    mut banner: Single<&mut Visibility, With<HudFreeCamera>>,
) {
    let roaming = cameras.single().is_ok_and(OrbitCamera::roaming);
    banner.set_if_neq(if roaming {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

fn update_artifact_readout(
    phase: Res<State<InGamePhase>>,
    player: Query<&Transform, With<Player>>,
    exhibits: Query<(&ArtifactDisplay, &GlobalTransform)>,
    mut visibility: Single<&mut Visibility, With<ArtifactReadout>>,
    mut title: Single<
        &mut Text,
        (
            With<ArtifactReadoutTitle>,
            Without<ArtifactReadoutDescription>,
        ),
    >,
    mut description: Single<
        &mut Text,
        (
            With<ArtifactReadoutDescription>,
            Without<ArtifactReadoutTitle>,
        ),
    >,
) {
    let selected = if *phase.get() == InGamePhase::Running {
        player
            .single()
            .ok()
            .and_then(|at| nearest_artifact(at.translation.xz(), exhibits.iter()))
    } else {
        None
    };

    if let Some(copy) = selected.and_then(|index| text::ARTIFACT_COPY.get(index)) {
        if title.0 != copy.title {
            title.0 = copy.title.to_owned();
        }
        if description.0 != copy.description {
            description.0 = copy.description.to_owned();
        }
        visibility.set_if_neq(Visibility::Inherited);
    } else {
        visibility.set_if_neq(Visibility::Hidden);
    }
}

fn nearest_artifact<'a>(
    at: Vec2,
    exhibits: impl IntoIterator<Item = (&'a ArtifactDisplay, &'a GlobalTransform)>,
) -> Option<usize> {
    exhibits
        .into_iter()
        .filter_map(|(display, pose)| {
            let distance = at.distance(pose.translation().xz());
            (distance <= ARTIFACT_READ_RANGE).then_some((distance, display.index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

fn spawn_pause_menu(mut commands: Commands) {
    let root = commands
        .spawn((PauseMenuRoot, Transform::default(), Visibility::default()))
        .id();

    commands.entity(root).with_children(|menu| {
        // Dimmed full-screen backdrop.
        menu.spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        });

        // Centered "PAUSED" label.
        menu.spawn((
            Text::new(text::PAUSE_TITLE),
            TextFont {
                font_size: FontSize::Px(40.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.85)),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(45.0),
                left: Val::Percent(0.0),
                width: Val::Percent(100.0),
                ..default()
            },
        ));

        // Below it: resume hint.
        menu.spawn((
            Text::new(text::PAUSE_HINT),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(Color::srgb(0.7, 0.7, 0.65)),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(52.0),
                left: Val::Percent(0.0),
                width: Val::Percent(100.0),
                ..default()
            },
        ));
    });
}

fn despawn_pause_menu(mut commands: Commands, roots: Query<Entity, With<PauseMenuRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;

    #[test]
    fn hud_displays_frames_per_second_from_real_frame_time() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, FrameTimeDiagnosticsPlugin::new(20)))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                40,
            )))
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, update_fps);
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let line = world
            .query_filtered::<&Text, With<HudFps>>()
            .single(world)
            .expect("one FPS counter in the HUD");
        assert_eq!(line.0, "FPS: 25");
    }

    #[test]
    fn nearest_artifact_only_selects_the_closest_exhibit_in_range() {
        let mut world = World::new();
        for (index, at) in [
            (3, Vec2::new(1.0, 0.0)),
            (7, Vec2::new(2.5, 0.0)),
            (9, Vec2::new(8.0, 0.0)),
        ] {
            world.spawn((
                ArtifactDisplay { index },
                GlobalTransform::from_xyz(at.x, 0.0, at.y),
            ));
        }
        let mut query = world.query::<(&ArtifactDisplay, &GlobalTransform)>();
        assert_eq!(
            nearest_artifact(Vec2::new(2.1, 0.0), query.iter(&world)),
            Some(7)
        );
        assert_eq!(
            nearest_artifact(Vec2::new(20.0, 0.0), query.iter(&world)),
            None
        );
    }

    #[test]
    fn artifact_readout_tracks_proximity_and_hides_while_paused() {
        let mut app = App::new();
        app.add_plugins(StatesPlugin)
            .init_state::<AppState>()
            .add_sub_state::<InGamePhase>()
            .add_systems(Update, update_artifact_readout);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        app.update();
        app.update();

        let player = app
            .world_mut()
            .spawn((Player::default(), Transform::from_xyz(0.0, 0.0, 0.0)))
            .id();
        app.world_mut().spawn((
            ArtifactDisplay { index: 1 },
            GlobalTransform::from_xyz(2.0, 0.0, 0.0),
        ));
        let readout = app
            .world_mut()
            .spawn((ArtifactReadout, Visibility::Hidden))
            .id();
        let title = app
            .world_mut()
            .spawn((ArtifactReadoutTitle, Text::default()))
            .id();
        let description = app
            .world_mut()
            .spawn((ArtifactReadoutDescription, Text::default()))
            .id();
        app.update();

        assert_eq!(
            app.world().get::<Visibility>(readout),
            Some(&Visibility::Inherited)
        );
        assert_eq!(
            app.world().get::<Text>(title).expect("title").0,
            text::ARTIFACT_COPY[1].title
        );
        assert_eq!(
            app.world().get::<Text>(description).expect("description").0,
            text::ARTIFACT_COPY[1].description
        );

        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation = Vec3::X * 20.0;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(readout),
            Some(&Visibility::Hidden)
        );

        app.world_mut()
            .get_mut::<Transform>(player)
            .expect("player")
            .translation = Vec3::ZERO;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(readout),
            Some(&Visibility::Inherited)
        );
        app.world_mut()
            .resource_mut::<NextState<InGamePhase>>()
            .set(InGamePhase::Paused);
        app.update();
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(readout),
            Some(&Visibility::Hidden)
        );
    }
}
