//! Analysis of a track: its tempo, where its beats fall and its key.
//!
//! Behaviour note: `docs/behavior/analysis.md`. The method and its measurements:
//! `docs/decisions/0020-own-analysis.md`.

mod fft;
mod key;
mod tempo;

use std::fmt;

pub use key::{Key, Mode, Transposed};
pub use tempo::{BeatTempo, TempoRange};

/// The longest stretch of a track that is read, in seconds.
pub const MAX_SECONDS: f64 = 20.0 * 60.0;

/// What the detection found in a track: each part only when it was found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub tempo: Option<BeatTempo>,
    pub key: Option<Key>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnalysisError {
    NoSamples,
    /// A sample rate or a channel count of zero.
    NoFormat,
    /// A sample is NaN or infinite.
    NotFinite,
    /// A tempo range outside 20 to 400 BPM, or too narrow to hold every tempo.
    Range,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSamples => write!(f, "there are no samples to analyse"),
            Self::NoFormat => write!(f, "the sample rate and the channel count must not be zero"),
            Self::NotFinite => write!(f, "a sample is not a finite number"),
            Self::Range => write!(
                f,
                "the tempo range must lie in 20 to 400 BPM, with its top at least 1.5 times its bottom"
            ),
        }
    }
}

impl std::error::Error for AnalysisError {}

/// The sample rate the detection works at, about a quarter of the usual rates: the beats and the
/// pitch classes it looks for sit well below its Nyquist frequency.
const WORK_RATE: f64 = 11_025.0;

/// Detects the tempo, the first beat and the key of interleaved samples with `channels`
/// channels at `rate` frames per second.
pub fn detect(
    samples: &[f32],
    channels: usize,
    rate: u32,
    range: TempoRange,
) -> Result<Detection, AnalysisError> {
    if channels == 0 || rate == 0 {
        return Err(AnalysisError::NoFormat);
    }
    if samples.len() < channels {
        return Err(AnalysisError::NoSamples);
    }
    let limit = (MAX_SECONDS * f64::from(rate)) as usize * channels;
    let samples = &samples[..samples.len().min(limit)];
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(AnalysisError::NotFinite);
    }
    let (mono, work_rate) = reduce(samples, channels, rate);
    Ok(Detection {
        tempo: tempo::detect(&mono, work_rate, range),
        key: key::detect(&mono, work_rate),
    })
}

/// Mixes the channels down and averages groups of frames to bring the rate near `WORK_RATE`.
/// The average is a gentle low-pass, enough for what the detection reads.
fn reduce(samples: &[f32], channels: usize, rate: u32) -> (Vec<f32>, f64) {
    let factor = (f64::from(rate) / WORK_RATE).round().max(1.0) as usize;
    let group = factor * channels;
    let mono = samples
        .chunks(group)
        .filter(|chunk| chunk.len() == group)
        .map(|chunk| chunk.iter().sum::<f32>() / group as f32)
        .collect();
    (mono, f64::from(rate) / factor as f64)
}
