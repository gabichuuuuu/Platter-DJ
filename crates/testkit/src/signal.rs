//! Synthesised signals.

use std::f64::consts::TAU;

/// A mono sine that starts at phase zero.
pub fn sine(hz: f64, rate: u32, frames: usize, amplitude: f32) -> Vec<f32> {
    (0..frames)
        .map(|n| (f64::from(amplitude) * (TAU * hz * n as f64 / f64::from(rate)).sin()) as f32)
        .collect()
}

/// Duplicates a mono signal into interleaved stereo.
pub fn stereo(mono: &[f32]) -> Vec<f32> {
    mono.iter().flat_map(|sample| [*sample, *sample]).collect()
}

/// One channel of an interleaved signal.
pub fn channel(interleaved: &[f32], channels: usize, index: usize) -> Vec<f32> {
    interleaved
        .iter()
        .skip(index)
        .step_by(channels.max(1))
        .copied()
        .collect()
}

/// Converts to 16-bit integers the way the file generators store them.
pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
        .collect()
}
