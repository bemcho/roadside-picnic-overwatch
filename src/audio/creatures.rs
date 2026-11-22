use bevy::audio::{PlaybackSettings, SpatialListener, SpatialScale, Volume};
use bevy::prelude::*;

use crate::states::{AppState, InGamePhase};
use crate::world::{CreatureCue as Cue, CreatureKind as Kind, CreatureSound};

use super::{AudioMode, effects};

const VOICES: usize = 20;
const STEP_BUDGET: usize = 11;

#[derive(Resource)]
pub(super) struct CreatureSounds {
    calls: [Handle<AudioSource>; Kind::ALL.len()],
    steps: [Handle<AudioSource>; 3],
    predators: [[Handle<AudioSource>; 2]; 3],
    alarm: Handle<AudioSource>,
    impact: Handle<AudioSource>,
    wings: Handle<AudioSource>,
    land: Handle<AudioSource>,
}

pub(super) fn load_creature_sounds(mut commands: Commands, mut bank: ResMut<Assets<AudioSource>>) {
    let mut clip = |bytes| bank.add(AudioSource { bytes });
    commands.insert_resource(CreatureSounds {
        calls: Kind::ALL.map(|kind| clip(effects::creature_call(kind))),
        steps: [
            clip(effects::step(0xB12D, 0.095, 246.0, 0.37)),
            clip(effects::step(0xA908, 0.14, 109.0, 0.57)),
            clip(effects::step(0xC104, 0.16, 173.0, 0.72)),
        ],
        predators: [Kind::Frostwolf, Kind::Mutt, Kind::Chimera]
            .map(|kind| [false, true].map(|strike| clip(effects::predator_voice(kind, strike)))),
        alarm: clip(effects::deer_alarm()),
        impact: clip(effects::step(0xAA54, 0.27, 73.0, 0.85)),
        wings: clip(effects::wing_rustle()),
        land: clip(effects::step(0x1A2D, 0.14, 191.0, 0.40)),
    });
}

impl CreatureSounds {
    fn clip(&self, kind: Kind, cue: Cue) -> &Handle<AudioSource> {
        match cue {
            Cue::Step(_) => {
                let i = match kind {
                    bird if bird.is_bird() => 0,
                    Kind::Hexadeer | Kind::Fleshdeer | Kind::Bovine => 2,
                    _ => 1,
                };
                &self.steps[i]
            }
            Cue::Call => &self.calls[kind as usize],
            Cue::Growl | Cue::Strike => {
                let i = match kind {
                    Kind::Frostwolf => 0,
                    Kind::Mutt => 1,
                    _ => 2,
                };
                &self.predators[i][usize::from(cue == Cue::Strike)]
            }
            Cue::Alarm => &self.alarm,
            Cue::Impact => &self.impact,
            Cue::Wings => &self.wings,
            Cue::Land => &self.land,
        }
    }
}

#[derive(Component)]
pub(super) struct CreatureVoice(f64);

fn settings(sound: CreatureSound, distance: f32, mode: &AudioMode) -> Option<PlaybackSettings> {
    let (near, far, scale, volume) = match sound.cue {
        Cue::Step(run) => (
            20.0,
            38.0,
            0.075,
            if sound.kind.is_bird() {
                0.66
            } else {
                0.79 + run * 0.45
            },
        ),
        Cue::Call if sound.kind.is_bird() => (63.0, 155.0, 0.027, 0.66),
        Cue::Call => (40.0, 76.0, 0.047, 0.63),
        Cue::Growl | Cue::Alarm => (53.0, 108.0, 0.037, 0.86),
        Cue::Strike => (55.0, 105.0, 0.039, 1.0),
        Cue::Impact => (16.0, 40.0, 0.065, 1.05),
        Cue::Wings | Cue::Land => (24.0, 51.0, 0.058, 0.77),
    };
    if distance >= far {
        return None;
    }
    let feather = ((far - distance) / (far - near)).clamp(0.0, 1.0);
    let jitter = ((sound.at.x * 5.729 + sound.at.z * 11.931).sin() * 0.073).clamp(-0.073, 0.073);
    let mass = match (sound.cue, sound.kind) {
        (Cue::Step(_), Kind::Bovine) => 0.71,
        (Cue::Step(_), Kind::Chimera) => 0.78,
        (Cue::Step(_), Kind::Mutt) => 1.11,
        _ => 1.0,
    };
    Some(
        mode.settings(
            PlaybackSettings::DESPAWN
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(scale))
                .with_volume(Volume::Linear(volume * feather * feather))
                .with_speed(mass * (1.0 + jitter)),
        ),
    )
}

fn priority(cue: Cue) -> u8 {
    match cue {
        Cue::Impact | Cue::Strike => 0,
        Cue::Growl | Cue::Alarm => 1,
        Cue::Call | Cue::Wings | Cue::Land => 2,
        Cue::Step(_) => 3,
    }
}

pub(super) fn play_creature_sounds(
    mut commands: Commands,
    time: Res<Time<Real>>,
    (mode, phase): (Res<AudioMode>, Res<State<InGamePhase>>),
    bank: Res<CreatureSounds>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    voices: Query<(Entity, &CreatureVoice)>,
    mut sounds: MessageReader<CreatureSound>,
) {
    let now = time.elapsed_secs_f64();
    let running = *phase.get() == InGamePhase::Running;
    let mut used = 0;
    for (entity, voice) in &voices {
        if !running || voice.0 <= now {
            commands.entity(entity).despawn();
        } else {
            used += 1;
        }
    }
    let Ok(listener) = listener.single() else {
        sounds.clear();
        return;
    };
    if !running || *mode == AudioMode::Muted {
        sounds.clear();
        return;
    }
    let mut audible = sounds
        .read()
        .filter_map(|&sound| {
            let distance = listener.translation().distance(sound.at);
            Some((sound, distance, settings(sound, distance, &mode)?))
        })
        .collect::<Vec<_>>();
    audible.sort_unstable_by(|(a, da, _), (b, db, _)| {
        priority(a.cue)
            .cmp(&priority(b.cue))
            .then_with(|| da.total_cmp(db))
    });
    for (sound, _, playback) in audible {
        if used >= VOICES || (used >= STEP_BUDGET && matches!(sound.cue, Cue::Step(_))) {
            break;
        }
        trace!(kind = ?sound.kind, cue = ?sound.cue, at = ?sound.at, "Wildlife sound");
        commands.spawn((
            CreatureVoice(now + 2.4),
            AudioPlayer(bank.clip(sound.kind, sound.cue).clone()),
            playback,
            Transform::from_translation(sound.at),
            DespawnOnExit(AppState::InGame),
        ));
        used += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_animal_steps_and_distant_bird_calls_are_spatial_without_long_range_footsteps() {
        let bird = CreatureSound {
            kind: Kind::Raven,
            cue: Cue::Call,
            at: Vec3::ZERO,
        };
        let paw = CreatureSound {
            kind: Kind::Frostwolf,
            cue: Cue::Step(1.0),
            at: Vec3::ZERO,
        };
        let step = settings(paw, 8.0, &AudioMode::Audible).unwrap();
        assert!(step.spatial && !step.muted && step.volume.to_linear() > 0.75);
        assert!(
            settings(paw, 32.0, &AudioMode::Audible)
                .unwrap()
                .volume
                .to_linear()
                < 0.22
        );
        assert!(settings(paw, 40.0, &AudioMode::Audible).is_none());
        assert!(settings(bird, 73.0, &AudioMode::Audible).unwrap().spatial);
        assert!(settings(bird, 160.0, &AudioMode::Audible).is_none());
    }

    #[test]
    fn a_busy_encounter_keeps_fight_cues_and_discards_muted_events() {
        let mut app = App::new();
        app.init_resource::<Assets<AudioSource>>()
            .init_resource::<AudioMode>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time<Real>>()
            .insert_resource(State::new(InGamePhase::Running))
            .add_message::<CreatureSound>()
            .add_systems(Startup, load_creature_sounds)
            .add_systems(
                Update,
                (super::super::toggle_mute, play_creature_sounds).chain(),
            );
        app.world_mut()
            .spawn((SpatialListener::default(), GlobalTransform::IDENTITY));
        let emit = |cue, app: &mut App| {
            app.world_mut().write_message(CreatureSound {
                kind: Kind::Mutt,
                cue,
                at: Vec3::X * 5.0,
            });
        };
        for _ in 0..30 {
            emit(Cue::Step(1.0), &mut app);
        }
        emit(Cue::Strike, &mut app);
        app.update();
        let world = app.world_mut();
        let strike = world
            .resource::<CreatureSounds>()
            .clip(Kind::Mutt, Cue::Strike)
            .clone();
        let mut query =
            world.query_filtered::<(&AudioPlayer, &PlaybackSettings), With<CreatureVoice>>();
        assert_eq!(query.iter(world).count(), STEP_BUDGET);
        assert!(
            query
                .iter(world)
                .any(|(clip, settings)| clip.0 == strike && settings.spatial)
        );
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyN);
        emit(Cue::Impact, &mut app);
        app.update();
        let world = app.world();
        assert_eq!(query.iter(world).count(), STEP_BUDGET);
        assert!(query.iter(world).all(|(_, settings)| settings.muted));
        let world = app.world_mut();
        world.insert_resource(State::new(InGamePhase::Paused));
        app.update();
        assert_eq!(query.iter(app.world()).count(), 0);
    }
}
