//! Roadside-Picnic — a sci-fi game set in the Zone.
//!
//! See `CLAUDE.md` for architecture conventions.

mod assets;
mod audio;
mod player;
mod save;
mod states;
mod ui;
mod world;
mod zone;

use bevy::prelude::*;

use assets::{PreloadedAssets, preload_game_assets};
use player::PlayerPlugin;
use save::SavePlugin;
use states::{AppState, InGamePhase, SplashPhase, toggle_pause};
use ui::UiPlugin;
use world::WorldPlugin;
use zone::ZonePlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_state::<AppState>()
        .add_sub_state::<SplashPhase>()
        .add_sub_state::<InGamePhase>()
        .init_resource::<PreloadedAssets>()
        .add_systems(Startup, preload_game_assets)
        .add_plugins((
            audio::GameAudioPlugin,
            WorldPlugin,
            PlayerPlugin,
            ZonePlugin,
            UiPlugin,
            SavePlugin,
        ))
        .add_systems(Update, toggle_pause.run_if(in_state(AppState::InGame)))
        .add_systems(Update, quit_on_q.run_if(in_state(AppState::InGame)))
        .run();
}

/// Quits the app when Q is pressed.
fn quit_on_q(keys: Res<ButtonInput<KeyCode>>, mut exits: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        exits.write(AppExit::Success);
    }
}
