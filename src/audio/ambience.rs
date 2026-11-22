//! Synthesized Zone ambience — no audio files, everything is generated.
//!
//! Layers, mixed into one looping WAV buffer:
//! - wind: low-passed noise shaped by slow, incommensurate gusts
//! - a low, uneasy drone: two detuned sines beating against each other
//! - distant animals: sparse bird chirps
//! - something unknown far away: occasional deep moans and faint metallic
//!   pings, smeared by echo so they sound distant
//!
//! Deterministic: the same seed produces the same ambience every run.

use std::f32::consts::TAU;
use std::sync::Arc;

use super::{SAMPLE_RATE, encode_wav};

/// Loop length in seconds.
const DURATION_SECS: u32 = 150;

/// Leave space for effects while keeping the ambience audible.
const TARGET_RMS: f32 = 0.19;

/// Deterministic PRNG so the ambience is reproducible.
pub(super) struct XorShift64(u64);

impl XorShift64 {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    /// Uniform in `[0, 1)`.
    pub(super) fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

/// Synthesizes the full ambience loop as a WAV byte buffer.
pub fn synth_ambience_wav() -> Arc<[u8]> {
    let n = (SAMPLE_RATE * DURATION_SECS) as usize;
    let mut mix = vec![0.0f32; n];
    let mut rng = XorShift64::new(0x5EED_20E0);

    add_wind(&mut mix, &mut rng);
    add_drone(&mut mix);
    add_birds(&mut mix, &mut rng);
    add_moans(&mut mix, &mut rng);
    add_pings(&mut mix, &mut rng);

    // Fade the loop edges so the restart is seamless.
    let fade = (SAMPLE_RATE / 2) as usize;
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        mix[i] *= g;
        mix[n - 1 - i] *= g;
    }

    // Normalize, soft clip, and encode to 16-bit PCM.
    let rms = (mix.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
    let gain = if rms > 0.0 { TARGET_RMS / rms } else { 1.0 };
    encode_wav(
        mix.len(),
        mix.into_iter().map(|sample| (sample * gain).tanh() * 0.95),
    )
}

/// Wind: white noise through a one-pole low-pass, shaped by slow gust LFOs.
fn add_wind(mix: &mut [f32], rng: &mut XorShift64) {
    let lp_alpha = 0.095; // ~350 Hz cutoff at 22050 Hz
    let mut lp = 0.0f32;
    let mut brown = 0.0f32;
    let mut air = 0.0f32;

    for (i, sample) in mix.iter_mut().enumerate() {
        let t = i as f32 / SAMPLE_RATE as f32;

        let gust = 0.55
            + 0.35 * (TAU * 0.030 * t + 1.7).sin()
            + 0.25 * (TAU * 0.011 * t + 4.1).sin()
            + 0.15 * (TAU * 0.007 * t + 2.3).sin();
        let gust = gust.clamp(0.0, 1.5);

        let white = rng.next_f32() * 2.0 - 1.0;
        lp += lp_alpha * (white - lp);
        air += 0.38 * (white - air);
        brown = 0.992 * brown + 0.008 * white * 3.0;

        // Rushing air + a deeper body of moving air.
        *sample += (lp * 0.50 + brown * 0.2 + (air - lp) * 0.30) * gust * 0.23;
    }
}

/// The drone: two detuned low sines beating slowly, with tremolo.
fn add_drone(mix: &mut [f32]) {
    for (i, sample) in mix.iter_mut().enumerate() {
        let t = i as f32 / SAMPLE_RATE as f32;
        let body = 0.5 * (TAU * 41.0 * t).sin() + 0.5 * (TAU * 44.7 * t + 0.5).sin();
        let harmonics = 0.42 * (TAU * 123.0 * t).sin() + 0.2 * (TAU * 178.8 * t).sin();
        let tremolo = 0.75 + 0.25 * (TAU * 0.07 * t).sin();
        *sample += (body * 0.45 + harmonics) * tremolo * 0.055;
    }
}

/// A single bird chirp: a quick descending frequency sweep with a sharp
/// attack and exponential decay.
fn render_chirp(mix: &mut [f32], start_secs: f32, f_start: f32, f_end: f32, amp: f32) {
    let dur = 0.16;
    let start = (start_secs * SAMPLE_RATE as f32) as usize;
    let len = (dur * SAMPLE_RATE as f32) as usize;
    let n = mix.len();

    for s in 0..len {
        let i = start + s;
        if i >= n {
            break;
        }
        let t = s as f32 / SAMPLE_RATE as f32;
        let phase = TAU * (f_start * t + 0.5 * (f_end - f_start) / dur * t * t);
        let env = (-t * 18.0).exp() * (1.0 - (-t * 400.0).exp());
        let harmonic = 0.4 * (phase * 2.0).sin();
        mix[i] += (phase.sin() + harmonic) * env * amp;
    }
}

/// Sparse distant birds: a handful of chirp bursts, some doubled like a
/// call-and-response.
fn add_birds(mix: &mut [f32], rng: &mut XorShift64) {
    let mut t = rng.range(6.0, 15.0);
    while t < (DURATION_SECS - 4) as f32 {
        let amp = rng.range(0.030, 0.055);
        let f = rng.range(2400.0, 3800.0);
        render_chirp(mix, t, f, f * 0.6, amp);
        if rng.next_f32() < 0.5 {
            render_chirp(mix, t + rng.range(0.3, 0.5), f * 1.15, f * 0.7, amp * 0.7);
        }
        t += rng.range(7.0, 18.0);
    }
}

/// Something unknown, far away: a deep moan — a slow descending sweep with
/// vibrato, echoed twice so it sounds like it comes from beyond the hills.
fn render_moan(mix: &mut [f32], start_secs: f32, f_start: f32, f_end: f32, dur: f32, amp: f32) {
    let start = (start_secs * SAMPLE_RATE as f32) as usize;
    let len = (dur * SAMPLE_RATE as f32) as usize;
    let n = mix.len();

    let mut sweep = |i: usize, gain: f32| {
        if i >= n {
            return;
        }
        let t = (i - start) as f32 / SAMPLE_RATE as f32;
        let f = f_start + (f_end - f_start) * (t / dur);
        let vibrato = 1.0 + 0.06 * (TAU * 4.7 * t).sin();
        let phase = TAU * f * vibrato * t;
        let env = (t / dur).powf(1.5) * (1.0 - (t / dur).powi(4));
        mix[i] += (phase.sin() + 0.35 * (phase * 0.5).sin()) * env * amp * gain;
    };

    for s in 0..len {
        let i = start + s;
        // Direct sound + two echo taps (0.45s and 0.9s delay).
        sweep(i, 1.0);
        let echo1 = i + (0.45 * SAMPLE_RATE as f32) as usize;
        let echo2 = i + (0.9 * SAMPLE_RATE as f32) as usize;
        if (s as f32 / SAMPLE_RATE as f32) < dur {
            sweep(echo1, 0.45);
            sweep(echo2, 0.2);
        }
    }
}

fn add_moans(mix: &mut [f32], rng: &mut XorShift64) {
    let mut t = rng.range(12.0, 25.0);
    while t < (DURATION_SECS - 10) as f32 {
        let dur = rng.range(3.5, 6.0);
        let f = rng.range(110.0, 150.0);
        render_moan(mix, t, f, f * 0.45, dur, rng.range(0.07, 0.10));
        t += rng.range(22.0, 40.0);
    }
}

/// A single metallic ping: high, pure, decaying fast, with one echo.
fn render_ping(mix: &mut [f32], start_secs: f32, f: f32, amp: f32) {
    let start = (start_secs * SAMPLE_RATE as f32) as usize;
    let len = (2.5 * SAMPLE_RATE as f32) as usize;
    let n = mix.len();

    for s in 0..len {
        let i = start + s;
        if i >= n {
            break;
        }
        let t = s as f32 / SAMPLE_RATE as f32;
        let env = (-t * 3.2).exp();
        mix[i] += (TAU * f * t).sin() * env * amp;

        let echo = i + (0.35 * SAMPLE_RATE as f32) as usize;
        if echo < n {
            mix[echo] += (TAU * f * (t - 0.35)).sin() * env * amp * 0.35;
        }
    }
}

fn add_pings(mix: &mut [f32], rng: &mut XorShift64) {
    let mut t = rng.range(20.0, 45.0);
    while t < (DURATION_SECS - 5) as f32 {
        render_ping(mix, t, rng.range(1050.0, 1450.0), rng.range(0.020, 0.035));
        t += rng.range(25.0, 55.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_well_formed() {
        let wav = encode_wav(1000, [0.0; 1000]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        // Data chunk size must match the declared length.
        let declared = u32::from_le_bytes(wav[40..44].try_into().expect("4 bytes"));
        assert_eq!(declared, 2000);
    }

    #[test]
    fn wav_decodes_to_expected_size() {
        let wav = synth_ambience_wav();
        // 44-byte header + 2 bytes per sample.
        assert_eq!(wav.len(), 44 + (SAMPLE_RATE * DURATION_SECS * 2) as usize);
        // The declared data size matches the buffer.
        let declared = u32::from_le_bytes(wav[40..44].try_into().expect("4 bytes"));
        assert_eq!(declared as usize, wav.len() - 44);
    }

    #[test]
    fn ambience_is_loud_enough_to_hear() {
        let wav = synth_ambience_wav();
        let samples: Vec<f32> = wav[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| i16::from_le_bytes(c) as f32 / i16::MAX as f32)
            .collect();
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!((0.15..0.21).contains(&rms), "ambience RMS {rms}");
    }
}
