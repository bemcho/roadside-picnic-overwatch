use std::f32::consts::TAU;
use std::sync::Arc;

use crate::world::CreatureKind as Kind;

use super::{SAMPLE_RATE, ambience::XorShift64, encode_wav};

fn render(duration: f32, mut sample: impl FnMut(f32) -> f32) -> Arc<[u8]> {
    let count = (duration * SAMPLE_RATE as f32) as usize;
    encode_wav(
        count,
        (0..count).map(|i| sample(i as f32 / SAMPLE_RATE as f32)),
    )
}

pub fn hangar() -> Arc<[u8]> {
    const LOOP: f32 = 8.0;
    let mut rng = XorShift64::new(0xFA60_1A12);
    let (mut low, mut air) = (0.0, 0.0);
    render(LOOP, |t| {
        let white = rng.next_f32() * 2.0 - 1.0;
        low += (white - low) * 0.04;
        air += (white - air) * 0.18;
        let phase = TAU * t;
        let hum = (phase * 52.0).sin() * 0.055
            + (phase * 104.0).sin() * 0.032
            + (phase * 157.0).sin() * 0.018;
        let fan = (air * 0.24 + low * 0.31) * (0.78 + (phase * 1.5).sin() * 0.14);
        hum + fan * (t.min(LOOP - t) * 10.0).min(1.0)
    })
}

pub fn ready_signal() -> Arc<[u8]> {
    render(0.55, |t| {
        let ping = |start: f32, freq: f32| {
            let age = (t - start).max(0.0);
            let phase = TAU * freq * age;
            (phase.sin() + 0.25 * (phase * 2.0).sin())
                * (age / 0.015).min(1.0)
                * (-age * 13.0).exp()
        };
        (ping(0.0, 393.0) * 0.17 + ping(0.11, 523.0) * 0.15) * ((0.55 - t) * 20.0).min(1.0)
    })
}

pub fn step(seed: u64, duration: f32, pitch: f32, gain: f32) -> Arc<[u8]> {
    let mut rng = XorShift64::new(seed);
    let mut low = 0.0;
    render(duration, |t| {
        let white = rng.next_f32() * 2.0 - 1.0;
        low += (white - low) * 0.18;
        let phase = TAU * pitch * t * (1.0 - t / duration * 0.33);
        let impact = phase.sin() * (-t / 0.032).exp();
        let grit = (white * 0.30 + low * 1.55) * (1.0 - t / duration).powi(2);
        let envelope = (t / 0.006).min(1.0) * (1.0 - t / duration);
        (impact * 0.58 + grit) * gain * envelope
    })
}

pub fn door(enter: bool) -> Arc<[u8]> {
    let mut rng = XorShift64::new(0xD002);
    let mut low = 0.0;
    render(0.65, |t| {
        let white = rng.next_f32() * 2.0 - 1.0;
        low += (white - low) * 0.24;
        let knock = |start: f32, pitch: f32, decay: f32| {
            let elapsed: f32 = t - start;
            if elapsed < 0.0 {
                return 0.0;
            }
            let phase = TAU * pitch * elapsed;
            let tone = phase.sin() * 0.5 + (phase * 2.57).sin() * 0.3 + low;
            tone * (elapsed * 300.0).min(1.0) * (-elapsed * decay).exp()
        };
        (knock(0.0, 670.0, 58.0) * 0.28
            + knock(0.08, 240.0, 24.0) * 0.39
            + knock(if enter { 0.37 } else { 0.46 }, 95.0, 26.0) * 0.62)
            * (1.0 - t / 0.65)
    })
}

pub fn engine() -> Arc<[u8]> {
    render(1.0, |t| {
        let phase = TAU * 48.0 * t;
        let firing = 0.62 * phase.sin() + 0.31 * (phase * 2.0).sin() + 0.16 * (phase * 4.0).sin();
        let rumble = 0.38 * (TAU * 24.0 * t).sin() + 0.15 * (TAU * 137.0 * t).sin();
        let rattle = (TAU * 397.0 * t).sin() * phase.cos().powi(4) * 0.12;
        (firing + rumble + rattle) * (0.32 + 0.06 * (TAU * 5.0 * t).sin())
    })
}

// Each note is (start, length, opening frequency, closing frequency).
type Note = (f32, f32, f32, f32);

fn voice(seed: u64, notes: &[Note], rough: f32, tremolo: f32) -> Arc<[u8]> {
    let duration = notes.iter().map(|n| n.0 + n.1).fold(0.0, f32::max);
    let mut random = XorShift64::new(seed);
    let (mut warm, mut hiss) = (0.0, 0.0);
    render(duration, |t| {
        let white = 2.0 * random.next_f32() - 1.0;
        warm += (white - warm) * 0.11;
        hiss += (white - hiss) * 0.48;
        notes
            .iter()
            .filter_map(|&(start, length, from, to)| {
                let age = t - start;
                let u = age / length;
                if !(0.0..1.0).contains(&u) {
                    return None;
                }
                let phase = TAU * age * (from + (to - from) * u * 0.5);
                let pulse = (tremolo * age * TAU).sin();
                let pitch = phase + 1.25 * rough * warm + pulse * 0.28;
                let upper = (pitch * 2.02).sin() * 0.37 + (pitch * 3.04).sin() * 0.20;
                let reed = pitch.sin() * (0.86 - rough * 0.26) + upper * (0.29 + rough * 0.64);
                let grain = (phase * 0.52 + warm * 1.3).sin() * 0.21 + (hiss - warm) * 0.91;
                let envelope = (age / 0.017).min(1.0)
                    * ((length - age) / 0.065).min(1.0)
                    * (0.76 + pulse * 0.18)
                    * (1.0 - u * 0.22);
                Some((reed + rough * grain).tanh() * envelope * 0.78)
            })
            .sum::<f32>()
    })
}

pub(super) fn creature_call(kind: Kind) -> Arc<[u8]> {
    let (notes, rough, tremolo): (&[Note], _, _) = match kind {
        Kind::Sparrow => (
            &[
                (0.0, 0.072, 2580.0, 3440.0),
                (0.099, 0.093, 3020.0, 2280.0),
                (0.229, 0.078, 2250.0, 2950.0),
            ],
            0.09,
            32.0,
        ),
        Kind::Raven => (
            &[(0.0, 0.23, 720.0, 385.0), (0.33, 0.27, 646.0, 335.0)],
            0.82,
            29.0,
        ),
        Kind::Owl => (
            &[(0.0, 0.23, 549.0, 412.0), (0.36, 0.43, 456.0, 365.0)],
            0.11,
            6.0,
        ),
        Kind::Hawk => (
            &[(0.0, 0.41, 2340.0, 1170.0), (0.54, 0.25, 1730.0, 1040.0)],
            0.30,
            19.0,
        ),
        Kind::Heron => (
            &[(0.0, 0.20, 1185.0, 584.0), (0.24, 0.19, 942.0, 415.0)],
            0.91,
            28.0,
        ),
        Kind::Skyglider => (
            &[(0.0, 0.33, 740.0, 1720.0), (0.38, 0.23, 1690.0, 990.0)],
            0.30,
            16.0,
        ),
        Kind::Frostwolf => (
            &[(0.0, 0.21, 390.0, 210.0), (0.30, 0.33, 375.0, 154.0)],
            0.68,
            13.0,
        ),
        Kind::Hexadeer => (
            &[(0.0, 0.34, 505.0, 393.0), (0.39, 0.19, 462.0, 340.0)],
            0.29,
            12.0,
        ),
        Kind::Bovine => (&[(0.0, 0.69, 187.0, 108.0)], 0.57, 14.0),
        Kind::Chimera => (&[(0.0, 0.55, 193.0, 78.0)], 0.92, 26.0),
        Kind::Fleshdeer => (&[(0.0, 0.48, 415.0, 216.0)], 0.44, 13.0),
        Kind::Mutt => (
            &[(0.0, 0.17, 398.0, 239.0), (0.24, 0.19, 336.0, 174.0)],
            0.74,
            15.0,
        ),
        Kind::Chameleon => (&[(0.0, 0.25, 1268.0, 730.0)], 0.98, 31.0),
    };
    voice(0x700D + kind as u64, notes, rough, tremolo)
}

pub(super) fn predator_voice(kind: Kind, strike: bool) -> Arc<[u8]> {
    let (growl, bark, flutter) = match kind {
        Kind::Frostwolf => (167.0, 491.0, 22.0),
        Kind::Mutt => (237.0, 570.0, 21.0),
        _ => (115.0, 303.0, 33.0),
    };
    let seed = 0xC0B4 + kind as u64;
    if strike {
        voice(
            seed + 75,
            &[
                (0.0, 0.27, bark, growl),
                (0.19, 0.34, growl * 1.34, growl * 0.73),
            ],
            0.96,
            flutter,
        )
    } else {
        voice(
            seed,
            &[(0.0, 0.66, growl * 0.84, growl * 1.12)],
            0.88,
            flutter,
        )
    }
}

pub(super) fn deer_alarm() -> Arc<[u8]> {
    voice(
        0xA1A4,
        &[(0.0, 0.19, 530.0, 830.0), (0.22, 0.43, 1120.0, 388.0)],
        0.41,
        22.0,
    )
}

pub(super) fn wing_rustle() -> Arc<[u8]> {
    let mut rng = XorShift64::new(0xFA7E);
    let mut low = 0.0;
    render(0.62, |t| {
        let white = rng.next_f32() * 2.0 - 1.0;
        low += (white - low) * 0.085;
        let flutter = (TAU * 5.1 * t + 0.6).sin().max(0.0).powi(2);
        let envelope = (t * 29.0).min(1.0) * (1.0 - t / 0.62);
        (low * 1.85 + (white - low) * 0.17) * flutter * envelope
    })
}
