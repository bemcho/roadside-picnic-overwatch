//! Gunfire from the Perimeter guns: a synthesized burst for each gun, played
//! where it fires and carrying far across the Zone.

use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::audio::{PlaybackSettings, SpatialListener, SpatialScale, Volume};
use bevy::prelude::*;

use crate::states::{AppState, InGamePhase};
use crate::world::{Gun, GunSound};

use super::{AudioMode, SAMPLE_RATE, ambience::XorShift64, encode_wav};

/// A burst is heard at full strength within `NEAR` meters, fading out by `FAR`.
const NEAR: f32 = 90.0;
const FAR: f32 = 650.0;
/// Seconds of fire in a burst (as `world::turrets` fires them) and of the whole clip.
const FIRING: f32 = 0.6;
const LENGTH: f32 = 0.8;

#[derive(Resource)]
pub(super) struct GunSounds {
    gatling: Handle<AudioSource>,
    naval: Handle<AudioSource>,
}

pub(super) fn load_gun_sounds(mut commands: Commands, mut bank: ResMut<Assets<AudioSource>>) {
    let mut clip = |heavy| {
        bank.add(AudioSource {
            bytes: burst(heavy),
        })
    };
    commands.insert_resource(GunSounds {
        gatling: clip(false),
        naval: clip(true),
    });
}

/// A burst of fire: quick light cracks from the gatling, slower heavy thumps from the naval gun.
fn burst(heavy: bool) -> Arc<[u8]> {
    let (rate, pitch, decay, gain) = if heavy {
        (7.0, 58.0, 11.0, 0.95)
    } else {
        (24.0, 132.0, 36.0, 0.6)
    };
    let last_shot = ((FIRING * rate).ceil() - 1.0) / rate;
    let mut rng = XorShift64::new(if heavy { 0x6A11 } else { 0x6A7C });
    let mut low = 0.0;
    let count = (LENGTH * SAMPLE_RATE as f32) as usize;
    encode_wav(
        count,
        (0..count).map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let white = rng.next_f32() * 2.0 - 1.0;
            low += (white - low) * 0.22;
            let age = t - ((t * rate).floor() / rate).min(last_shot);
            let crack = (white * 0.55 + low * 1.4) * (-age * decay).exp();
            let thump = (TAU * pitch * age).sin() * (-age * decay * 0.6).exp();
            (crack + thump * 0.8) * gain * ((LENGTH - t) * 25.0).min(1.0)
        }),
    )
}

fn settings(distance: f32, mode: &AudioMode) -> Option<PlaybackSettings> {
    if distance >= FAR {
        return None;
    }
    let feather = ((FAR - distance) / (FAR - NEAR)).clamp(0.0, 1.0);
    Some(
        mode.settings(
            PlaybackSettings::DESPAWN
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(0.02))
                .with_volume(Volume::Linear(0.8 * feather * feather)),
        ),
    )
}

pub(super) fn play_gun_sounds(
    mut commands: Commands,
    (mode, phase): (Res<AudioMode>, Res<State<InGamePhase>>),
    bank: Res<GunSounds>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
    mut sounds: MessageReader<GunSound>,
) {
    let Ok(listener) = listener.single() else {
        sounds.clear();
        return;
    };
    if *phase.get() != InGamePhase::Running || *mode == AudioMode::Muted {
        sounds.clear();
        return;
    }
    for sound in sounds.read() {
        let Some(playback) = settings(listener.translation().distance(sound.at), &mode) else {
            continue;
        };
        let clip = match sound.gun {
            Gun::Gatling => &bank.gatling,
            Gun::Naval => &bank.naval,
        };
        commands.spawn((
            AudioPlayer(clip.clone()),
            playback,
            Transform::from_translation(sound.at),
            DespawnOnExit(AppState::InGame),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gunfire_carries_far_and_fades_out() {
        let near = settings(50.0, &AudioMode::Audible).expect("heard near");
        assert!(near.spatial && near.volume.to_linear() > 0.75);
        let far = settings(500.0, &AudioMode::Audible).expect("heard far");
        assert!(far.volume.to_linear() < 0.1);
        assert!(settings(FAR, &AudioMode::Audible).is_none());
        assert!(!burst(false).is_empty() && !burst(true).is_empty());
    }
}
