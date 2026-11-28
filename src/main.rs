//! Roadside-Picnic — a sci-fi game set in the Zone.
//!
//! See `CLAUDE.md` for architecture conventions.

mod assets;
mod audio;
mod launch;
mod player;
mod render;
mod save;
mod states;
mod ui;
mod world;
mod zone;

use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{MemoryHints, WgpuSettings};

use assets::gpu_scans::GpuScansPlugin;
#[cfg(not(target_arch = "wasm32"))]
use assets::ram_source::{RamSourcePlugin, register_ram_source};
use assets::{PreloadedAssets, preload_game_assets};
use launch::LaunchOptions;
use player::PlayerPlugin;
use save::SavePlugin;
use states::{AppState, InGamePhase, SplashPhase, toggle_pause};
use ui::UiPlugin;
use world::WorldPlugin;
use zone::ZonePlugin;

fn main() {
    let mut app = App::new();
    // Asset sources are built with `AssetPlugin`, so the RAM source comes first.
    #[cfg(not(target_arch = "wasm32"))]
    register_ram_source(&mut app, "assets");
    // wgpu's default 256 MiB memory blocks left ~1.2 GB of a 6 GB card
    // reserved but unused, and a dropped scan freed nothing while its block
    // held anything else; 8-64 MiB blocks keep ~0.4 GB less in use at the
    // same frame rate (measured on the RTX 2060).
    let render = RenderPlugin {
        render_creation: WgpuSettings {
            memory_hints: MemoryHints::MemoryUsage,
            ..default()
        }
        .into(),
        ..default()
    };
    app.add_plugins(DefaultPlugins.set(render))
        // Parsed once the log is up, so a bad `--vram` value is reported.
        .insert_resource(LaunchOptions::from_env())
        // After the options: its fallback budget follows `--vram`.
        .add_plugins(GpuScansPlugin)
        // After `DefaultPlugins`: its `finish` edits what `PbrPlugin`'s inserted.
        .add_plugins(render::RenderTuningPlugin)
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
        .add_systems(Update, quit_on_q.run_if(in_state(AppState::InGame)));
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(RamSourcePlugin);
    app.run();
}

/// Quits the app when Q is pressed.
fn quit_on_q(keys: Res<ButtonInput<KeyCode>>, mut exits: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::KeyQ) {
        exits.write(AppExit::Success);
    }
}
