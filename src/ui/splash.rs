//! Hangar splash, shown until the Zone's assets are ready and a key is pressed.

mod stalker;

use bevy::asset::{LoadState, RecursiveDependencyLoadState, RenderAssetUsages};
use bevy::camera::ScalingMode;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::AmbientLight;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::assets::{PreloadedAssets, SPLASH_HANGAR};
use crate::states::{AppState, SplashPhase};

use super::text;

pub(super) struct SplashPlugin;

const VIEW_HEIGHT: f32 = 1.65;
const VIEW_CENTER: f32 = 0.60;

#[derive(Component)]
struct Backdrop;

#[derive(Component)]
struct SplashStatus;

impl Plugin for SplashPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(AppState::Loading),
            (spawn_stage, spawn_titles, stalker::spawn_stalker),
        )
        .add_systems(
            Update,
            (fit_backdrop, stalker::move_stalker).run_if(in_state(AppState::Loading)),
        )
        .add_systems(
            Update,
            (
                finish_loading.run_if(in_state(SplashPhase::Loading)),
                continue_from_splash.run_if(in_state(SplashPhase::Ready)),
            ),
        )
        .add_systems(OnEnter(SplashPhase::Ready), show_ready)
        .add_systems(OnEnter(SplashPhase::Failed), show_failure);
    }
}

fn spawn_stage(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: Color::srgb(0.06, 0.065, 0.07).into(),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: VIEW_HEIGHT,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, VIEW_CENTER, 4.0),
        Tonemapping::None,
        DebandDither::Disabled,
        AmbientLight {
            color: Color::srgb(0.78, 0.84, 0.95),
            brightness: 70.0,
            ..default()
        },
        DespawnOnExit(AppState::Loading),
    ));
    commands.spawn((
        Backdrop,
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(server.load(SPLASH_HANGAR)),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, VIEW_CENTER, -2.0),
        DespawnOnExit(AppState::Loading),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(0.72, 0.12))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(shadow_texture())),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.008, -0.25),
        DespawnOnExit(AppState::Loading),
    ));
    for (at, color, illuminance) in [
        (
            Vec3::new(-2.0, 4.0, 3.0),
            Color::srgb(1.0, 0.88, 0.69),
            2100.0,
        ),
        (
            Vec3::new(2.0, 3.0, -2.0),
            Color::srgb(0.65, 0.78, 1.0),
            1800.0,
        ),
    ] {
        commands.spawn((
            DirectionalLight {
                color,
                illuminance,
                ..default()
            },
            Transform::from_translation(at).looking_at(Vec3::Y * 0.5, Vec3::Y),
            DespawnOnExit(AppState::Loading),
        ));
    }
}

fn shadow_texture() -> Image {
    const SIZE: u32 = 32;
    let pixels = (0..SIZE * SIZE)
        .flat_map(|i| {
            let p = Vec2::new((i % SIZE) as f32, (i / SIZE) as f32) / (SIZE - 1) as f32;
            let r2 = (p * 2.0 - Vec2::ONE).length_squared();
            [0, 0, 0, (156.0 * (1.0 - r2).max(0.0).powi(3)) as u8]
        })
        .collect();
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn fit_backdrop(
    window: Single<&Window, With<PrimaryWindow>>,
    mut backdrop: Single<&mut Transform, With<Backdrop>>,
) {
    if window.height() > 0.0 {
        let cover = VIEW_HEIGHT * (window.width() / window.height()).max(1.0);
        if backdrop.scale.x != cover {
            backdrop.scale = Vec3::new(cover, cover, 1.0);
        }
    }
}

fn spawn_titles(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::vertical(Val::Vh(5.0)),
                ..default()
            },
            DespawnOnExit(AppState::Loading),
        ))
        .with_children(|splash| {
            splash.spawn((
                Text::new(text::SPLASH_TITLE),
                TextLayout::justify(Justify::Center),
                TextFont {
                    font_size: FontSize::VMin(7.5),
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.92, 0.81)),
                TextShadow {
                    offset: Vec2::new(2.0, 3.0),
                    ..default()
                },
            ));
            splash
                .spawn((
                    Node {
                        width: Val::VMin(58.0),
                        padding: UiRect::axes(Val::VMin(2.4), Val::VMin(0.9)),
                        border: UiRect::all(Val::Px(1.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.61, 0.48, 0.27)),
                    BackgroundColor(Color::srgba(0.035, 0.04, 0.047, 0.9)),
                ))
                .with_child((
                    SplashStatus,
                    Text::new(text::SPLASH_LOADING),
                    TextLayout::justify(Justify::Center),
                    TextFont {
                        font_size: FontSize::VMin(3.2),
                        ..default()
                    },
                    TextColor(Color::srgb(0.85, 0.84, 0.80)),
                ));
        });
}

fn finish_loading(
    server: Res<AssetServer>,
    preloaded: Res<PreloadedAssets>,
    mut phase: ResMut<NextState<SplashPhase>>,
) {
    let mut pending = false;
    for handle in &preloaded.0 {
        match server.get_load_states(handle.id()) {
            Some((LoadState::Failed(err), _, _))
            | Some((_, _, RecursiveDependencyLoadState::Failed(err))) => {
                error!("Could not preload {:?}: {err}", handle.path());
                phase.set(SplashPhase::Failed);
                return;
            }
            Some((LoadState::Loaded, _, RecursiveDependencyLoadState::Loaded)) => {}
            _ => pending = true,
        }
    }
    if !pending && !preloaded.0.is_empty() {
        phase.set(SplashPhase::Ready);
    }
}

fn show_ready(status: Single<(&mut Text, &mut TextColor), With<SplashStatus>>) {
    let (mut label, mut color) = status.into_inner();
    label.0 = text::SPLASH_READY.to_owned();
    color.0 = Color::srgb(1.0, 0.89, 0.63);
}

fn show_failure(mut status: Single<&mut Text, With<SplashStatus>>) {
    status.0 = text::SPLASH_FAILED.to_owned();
}

fn continue_from_splash(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if keys.get_just_pressed().next().is_some() {
        next_state.set(AppState::InGame);
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::state::app::StatesPlugin;

    use super::*;

    #[test]
    fn ready_waits_until_the_asset_server_finishes_loading() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), StatesPlugin))
            .init_asset::<Image>()
            .init_state::<AppState>()
            .add_sub_state::<SplashPhase>()
            .init_resource::<PreloadedAssets>()
            .add_systems(
                Update,
                finish_loading.run_if(in_state(SplashPhase::Loading)),
            );
        app.update();

        let image = app.world().resource::<AssetServer>().add(Image::default());
        app.world_mut()
            .resource_mut::<PreloadedAssets>()
            .0
            .push(image.untyped());
        app.world_mut().run_schedule(Update);
        assert!(matches!(
            app.world().resource::<NextState<SplashPhase>>(),
            NextState::Unchanged
        ));

        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<SplashPhase>>().get(),
            SplashPhase::Ready
        );
    }

    #[test]
    fn only_a_new_press_after_loading_enters_the_game_and_clears_the_splash() {
        let mut app = App::new();
        app.add_plugins(StatesPlugin)
            .init_state::<AppState>()
            .add_sub_state::<SplashPhase>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(
                Update,
                continue_from_splash.run_if(in_state(SplashPhase::Ready)),
            );
        let splash = app.world_mut().spawn(DespawnOnExit(AppState::Loading)).id();

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Loading
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<NextState<SplashPhase>>()
            .set(SplashPhase::Ready);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Loading
        );
        assert!(app.world().get_entity(splash).is_ok());

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyQ);
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::InGame
        );
        assert!(app.world().get_entity(splash).is_err());
    }
}
