//! The key of a track: its energy in each pitch class against a profile of each key.
//!
//! The profiles are the probe-tone ratings of Krumhansl and Kessler, 1982,
//! <https://doi.org/10.1037/0033-295X.89.4.334>.

use crate::fft::Spectrum;

const FRAME: usize = 8_192;
const HOP: usize = 4_096;
/// The pitches read, in Hz: from A1 to about B6. Lower bins are too wide for a semitone.
const LOWEST_HZ: f64 = 55.0;
const HIGHEST_HZ: f64 = 2_000.0;
/// How far the best key must correlate above the next one that is neither its relative nor
/// the same tonic, for a key to be given. Noise and drums stay under it.
const MIN_LEAD: f64 = 0.05;
/// The least correlation of the best key.
const MIN_CORRELATION: f64 = 0.5;

const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Major,
    Minor,
}

/// A major or minor key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    /// The pitch class of the tonic, 0 for C up to 11 for B.
    pub tonic: u8,
    pub mode: Mode,
}

/// A key moved by a number of semitones that need not be whole: the nearest key and how far
/// it is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transposed {
    pub key: Key,
    /// From -50 to +50.
    pub cents: f64,
}

impl Key {
    /// This key moved by `semitones`, the mode kept; `None` for a shift that is not a number.
    pub fn transposed(self, semitones: f64) -> Option<Transposed> {
        if !semitones.is_finite() {
            return None;
        }
        let nearest = semitones.round();
        let steps = (nearest % 12.0 + 12.0) % 12.0;
        Some(Transposed {
            key: Key {
                tonic: (self.tonic % 12 + steps as u8) % 12,
                mode: self.mode,
            },
            cents: (semitones - nearest) * 100.0,
        })
    }
}

pub(crate) fn detect(mono: &[f32], rate: f64) -> Option<Key> {
    let chroma = chroma(mono, rate)?;
    let mut scores: Vec<(Key, f64)> = (0..12u8)
        .flat_map(|tonic| {
            [(Mode::Major, &MAJOR), (Mode::Minor, &MINOR)].map(|(mode, profile)| {
                let rotated: [f64; 12] =
                    std::array::from_fn(|class| profile[(class + 12 - usize::from(tonic)) % 12]);
                (Key { tonic, mode }, correlation(&chroma, &rotated))
            })
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (best, score) = *scores.first()?;
    if score < MIN_CORRELATION {
        return None;
    }
    // The relative key and the parallel key share most of the notes; any other key that
    // comes close means the pitch classes do not point at one key.
    let rival = scores
        .iter()
        .skip(1)
        .find(|(key, _)| !related(best, *key))
        .map_or(0.0, |(_, score)| *score);
    (score - rival >= MIN_LEAD).then_some(best)
}

/// The share of the energy of the track in each pitch class, C first.
fn chroma(mono: &[f32], rate: f64) -> Option<[f64; 12]> {
    let mut spectrum = Spectrum::new(FRAME);
    let bins: Vec<(usize, usize)> = (1..=FRAME / 2)
        .filter_map(|bin| {
            let hz = bin as f64 * rate / FRAME as f64;
            (LOWEST_HZ..=HIGHEST_HZ).contains(&hz).then(|| {
                let pitch = 69.0 + 12.0 * (hz / 440.0).log2();
                (bin, (pitch.round() as i64).rem_euclid(12) as usize)
            })
        })
        .collect();
    let mut chroma = [0.0f64; 12];
    let mut magnitudes = Vec::new();
    let mut start = 0;
    while start < mono.len() {
        let end = (start + FRAME).min(mono.len());
        spectrum.magnitudes(&mono[start..end], &mut magnitudes);
        let mut frame = [0.0f64; 12];
        for (bin, class) in &bins {
            frame[*class] += f64::from(magnitudes[*bin]);
        }
        // Every frame counts alike, loud or quiet, so a loud passage does not decide alone.
        let total: f64 = frame.iter().sum();
        if total > 1e-6 {
            for (sum, value) in chroma.iter_mut().zip(frame) {
                *sum += value / total;
            }
        }
        start += HOP;
    }
    let total: f64 = chroma.iter().sum();
    (total > 0.0).then(|| chroma.map(|value| value / total))
}

fn correlation(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let mean = |values: &[f64; 12]| values.iter().sum::<f64>() / 12.0;
    let (mean_a, mean_b) = (mean(a), mean(b));
    let mut product = 0.0;
    let mut square_a = 0.0;
    let mut square_b = 0.0;
    for (x, y) in a.iter().zip(b) {
        product += (x - mean_a) * (y - mean_b);
        square_a += (x - mean_a).powi(2);
        square_b += (y - mean_b).powi(2);
    }
    let spread = (square_a * square_b).sqrt();
    if spread > 0.0 { product / spread } else { 0.0 }
}

/// The same key, its relative or its parallel.
fn related(key: Key, other: Key) -> bool {
    let relative = match key.mode {
        Mode::Major => (key.tonic + 9) % 12,
        Mode::Minor => (key.tonic + 3) % 12,
    };
    other.tonic == key.tonic || (other.mode != key.mode && other.tonic == relative)
}
