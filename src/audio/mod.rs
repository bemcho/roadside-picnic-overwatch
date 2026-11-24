//! Game audio, synthesized once and played through the system's default output.

mod ambience;
mod creatures;
mod effects;
mod guns;

use std::sync::Arc;

use bevy::audio::{AudioSink, AudioSinkPlayback, PlaybackSettings, SpatialAudioSink, Volume};
use bevy::prelude::*;

use crate::player::{
    Gait, Locomotion, Player,
    vehicle::{Driving, Vehicle},
};
use crate::states::{AppState, InGamePhase, SplashPhase};
use crate::world::{CreatureSound, GunSound};

const SAMPLE_RATE: u32 = 22_050;

pub struct GameAudioPlugin;

#[derive(Resource, Default, PartialEq, Eq)]
pub(crate) enum AudioMode {
    #[default]
    Audible,
    Muted,
}

impl AudioMode {
    fn settings(&self, mut settings: PlaybackSettings) -> PlaybackSettings {
        settings.muted = *self == Self::Muted;
        settings
    }
}

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioMode>()
            .add_message::<CreatureSound>()
            .add_message::<GunSound>()
            .add_systems(
                Startup,
                (
                    load_sounds,
                    creatures::load_creature_sounds,
                    guns::load_gun_sounds,
                ),
            )
            .add_systems(OnEnter(AppState::Loading), start_hangar_audio)
            .add_systems(OnEnter(SplashPhase::Ready), play_ready_signal)
            .add_systems(OnEnter(AppState::InGame), start_ambience)
            .add_systems(
                Last,
                (
                    toggle_mute,
                    (
                        play_actions,
                        sync_engine,
                        creatures::play_creature_sounds,
                        guns::play_gun_sounds,
                    )
                        .run_if(in_state(AppState::InGame)),
                )
                    .chain(),
            );
    }
}

#[derive(Resource)]
struct Sounds {
    ambience: Handle<AudioSource>,
    steps: [Handle<AudioSource>; 3],
    jump: Handle<AudioSource>,
    land: Handle<AudioSource>,
    enter: Handle<AudioSource>,
    exit: Handle<AudioSource>,
    engine: Handle<AudioSource>,
}

#[derive(Component)]
struct Ambience;

#[derive(Component)]
struct Engine;

fn start_hangar_audio(
    mut commands: Commands,
    mut bank: ResMut<Assets<AudioSource>>,
    mode: Res<AudioMode>,
) {
    commands.spawn((
        AudioPlayer(bank.add(AudioSource {
            bytes: effects::hangar(),
        })),
        mode.settings(PlaybackSettings::LOOP),
        DespawnOnExit(AppState::Loading),
    ));
}

fn play_ready_signal(
    mut commands: Commands,
    mut bank: ResMut<Assets<AudioSource>>,
    mode: Res<AudioMode>,
) {
    commands.spawn((
        AudioPlayer(bank.add(AudioSource {
            bytes: effects::ready_signal(),
        })),
        mode.settings(PlaybackSettings::DESPAWN),
        DespawnOnExit(AppState::Loading),
    ));
}

fn load_sounds(mut commands: Commands, mut bank: ResMut<Assets<AudioSource>>) {
    let mut clip = |bytes| bank.add(AudioSource { bytes });
    commands.insert_resource(Sounds {
        ambience: clip(ambience::synth_ambience_wav()),
        steps: [
            clip(effects::step(0xBE07, 0.16, 105.0, 0.36)),
            clip(effects::step(0xB007, 0.21, 86.0, 0.52)),
            clip(effects::step(0xC2A1, 0.38, 42.0, 0.26)),
        ],
        jump: clip(effects::step(0xA13, 0.12, 144.0, 0.29)),
        land: clip(effects::step(0x313, 0.23, 80.0, 0.43)),
        enter: clip(effects::door(true)),
        exit: clip(effects::door(false)),
        engine: clip(effects::engine()),
    });
}

fn start_ambience(mut commands: Commands, sounds: Res<Sounds>, mode: Res<AudioMode>) {
    commands.spawn((
        Ambience,
        AudioPlayer(sounds.ambience.clone()),
        mode.settings(PlaybackSettings::LOOP),
        DespawnOnExit(AppState::InGame),
    ));
}

fn toggle_mute(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<AudioMode>,
    mut sinks: Query<&mut AudioSink>,
    mut spatial: Query<&mut SpatialAudioSink>,
    mut pending: Query<&mut PlaybackSettings, (Without<AudioSink>, Without<SpatialAudioSink>)>,
) {
    if !keys.just_pressed(KeyCode::KeyN) {
        return;
    }
    let mute = *mode == AudioMode::Audible;
    *mode = if mute {
        AudioMode::Muted
    } else {
        AudioMode::Audible
    };
    for mut sink in &mut sinks {
        if mute { sink.mute() } else { sink.unmute() }
    }
    for mut sink in &mut spatial {
        if mute { sink.mute() } else { sink.unmute() }
    }
    for mut settings in &mut pending {
        settings.muted = mute;
    }
}

#[derive(Default)]
struct StepClock {
    previous: Option<Vec3>,
    distance: f32,
    gait: Gait,
    variation: usize,
    driving: bool,
}

impl StepClock {
    fn advance(&mut self, pos: Vec3, gait: Gait) -> Option<(usize, f32)> {
        let previous = self.previous.replace(pos);
        let old_gait = std::mem::replace(&mut self.gait, gait);
        let distance = previous.map_or(0.0, |p| p.xz().distance(pos.xz()));
        if distance > 1.5 || previous.is_none() {
            self.distance = 0.0;
            return None;
        }
        let (clip, stride) = match gait {
            Gait::Walk => (0, 0.64),
            Gait::Run => (1, 1.20),
            Gait::Crawl => (2, 0.39),
            _ => (0, 1.0),
        };
        if gait != old_gait {
            if gait == Gait::Jump {
                return Some((3, 1.0));
            }
            if old_gait == Gait::Jump {
                self.distance = 0.0;
                return Some((4, 1.0));
            }
            self.distance = stride * 0.75;
        }
        if !matches!(gait, Gait::Walk | Gait::Run | Gait::Crawl) || distance < 0.001 {
            return None;
        }
        self.distance += distance;
        if self.distance < stride {
            return None;
        }
        self.distance %= stride;
        self.variation = (self.variation + 1) % 4;
        Some((clip, [0.93, 1.06, 0.98, 1.11][self.variation]))
    }
}

type AudioPlayerState<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        &'static Locomotion,
        Option<&'static Driving>,
    ),
    With<Player>,
>;

fn play_actions(
    mut commands: Commands,
    sounds: Res<Sounds>,
    mode: Res<AudioMode>,
    phase: Res<State<InGamePhase>>,
    player: AudioPlayerState,
    ambience: Query<(), (With<Ambience>, With<AudioSink>)>,
    mut clock: Local<StepClock>,
) {
    let Ok((at, locomotion, driving)) = player.single() else {
        return;
    };
    let running = *phase.get() == InGamePhase::Running;
    let seated = driving.is_some();
    let changed_seat = clock.driving != seated;
    clock.driving = seated;
    let mut one_shot = |sound: &Handle<AudioSource>, speed| {
        commands.spawn((
            AudioPlayer(sound.clone()),
            mode.settings(PlaybackSettings::DESPAWN.with_speed(speed)),
            DespawnOnExit(AppState::InGame),
        ));
    };
    if !ambience.is_empty() && running {
        if changed_seat {
            one_shot(if seated { &sounds.enter } else { &sounds.exit }, 1.0);
        }
        if seated || changed_seat {
            clock.previous = None;
        } else if let Some((clip, speed)) = clock.advance(at.translation, locomotion.gait) {
            one_shot(
                match clip {
                    3 => &sounds.jump,
                    4 => &sounds.land,
                    _ => &sounds.steps[clip],
                },
                speed,
            );
        }
    }
}

fn sync_engine(
    mut commands: Commands,
    sounds: Res<Sounds>,
    mode: Res<AudioMode>,
    phase: Res<State<InGamePhase>>,
    driver: Query<&Driving, With<Player>>,
    vehicles: Query<&Vehicle>,
    mut engine: Query<(Entity, Option<&mut AudioSink>), With<Engine>>,
) {
    let car = driver.single().ok().and_then(|d| vehicles.get(d.0).ok());
    let running = *phase.get() == InGamePhase::Running;
    match (car, engine.single_mut().ok()) {
        (Some(car), Some((_, Some(mut sink)))) => {
            let load = car.engine_load();
            sink.set_speed(0.82 + load * 0.95);
            sink.set_volume(Volume::Linear(0.30 + load * 0.45));
            if running { sink.play() } else { sink.pause() }
        }
        (Some(_), None) => {
            commands.spawn((
                Engine,
                AudioPlayer(sounds.engine.clone()),
                mode.settings(PlaybackSettings::LOOP.with_volume(Volume::Linear(0.25))),
                DespawnOnExit(AppState::InGame),
            ));
        }
        (None, Some((entity, _))) => {
            commands.entity(entity).despawn();
        }
        _ => {}
    }
}

fn encode_wav(sample_count: usize, samples: impl IntoIterator<Item = f32>) -> Arc<[u8]> {
    let len = 2 * sample_count as u32;
    let mut out = Vec::with_capacity(44 + len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(2 * SAMPLE_RATE).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    for value in samples {
        out.extend_from_slice(&((value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_distance_instead_of_keys_and_do_not_play_on_teleport() {
        let mut clock = StepClock::default();
        assert!(clock.advance(Vec3::ZERO, Gait::Idle).is_none());
        for _ in 0..10 {
            assert!(clock.advance(Vec3::ZERO, Gait::Walk).is_none());
        }
        assert_eq!(
            clock.advance(Vec3::X * 0.3, Gait::Walk).map(|p| p.0),
            Some(0)
        );
        assert_eq!(
            clock.advance(Vec3::X * 0.7, Gait::Run).map(|p| p.0),
            Some(1)
        );
        assert_eq!(
            clock.advance(Vec3::X * 0.81, Gait::Crawl).map(|p| p.0),
            Some(2)
        );
        assert!(clock.advance(Vec3::X * 40.0, Gait::Idle).is_none());
    }

    #[test]
    fn m_mutes_newly_queued_sounds_and_unmutes_without_changing_gain() {
        let mut app = App::new();
        app.init_resource::<AudioMode>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, toggle_mute);
        let ambient = app
            .world_mut()
            .spawn(PlaybackSettings::LOOP.with_volume(Volume::Linear(0.6)))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyN);
        app.update();
        let mode = app.world().resource::<AudioMode>();
        assert!(mode.settings(PlaybackSettings::DESPAWN).muted);
        assert!(
            app.world()
                .get::<PlaybackSettings>(ambient)
                .expect("loop")
                .muted
        );
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::KeyN);
        keys.press(KeyCode::KeyN);
        app.update();
        let settings = app.world().get::<PlaybackSettings>(ambient).expect("loop");
        assert!(!settings.muted);
        assert_eq!(settings.volume.to_linear(), 0.6);
    }
}
