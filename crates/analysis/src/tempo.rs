//! The tempo of a track and the position of its first beat, from its spectral flux.
//!
//! The flux is how much the log-magnitude spectrum rises from one frame to the next (Bello and
//! others, 2005, <https://doi.org/10.1109/TSA.2005.851998>). The period of the beats is the lag
//! that repeats the flux best, then the period and offset of a comb that collects the most of
//! it over the whole track.

use platter_core::Bpm;

use crate::AnalysisError;
use crate::fft::Spectrum;

const FRAME: usize = 512;
const HOP: usize = 64;
/// Frames on either side of the local mean taken off the flux, about 50 ms.
const MEAN_RADIUS: usize = 8;
/// Bins of the phase histogram of a period.
const PHASE_BINS: usize = 48;
/// How far above the mean of its histogram the peak of the beats stands at the least, for a
/// steady beat. Noise stays near 1.5.
const MIN_SALIENCE: f64 = 4.0;
/// Fewest beats a tempo is found from.
const MIN_BEATS: f64 = 8.0;
/// The beats between the beats of a tempo, as a share of those beats, from which the double
/// tempo is taken.
const DOUBLE_SHARE: f64 = 0.6;
/// A tempo this close to a whole number is that number.
const WHOLE: f64 = 0.02;
/// The share of a period the range is widened by at either end.
const EDGE: f64 = 0.005;
/// The bins of the low band, where the kick sits: up to about 200 Hz at the work rate.
const LOW_BINS: usize = 9;
/// How much more the low band must hold half a period later for the beat to be put there.
const LOW_LEAD: f64 = 1.2;
/// How far the peak of the flux comes before the start of the sound that raises it, in seconds:
/// the window peaks once it has taken in the first of the sound. Measured on kicks, 13 to 15 ms.
const ONSET_LEAD: f64 = 0.0138;

/// The tempos a track is searched in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoRange {
    min: f64,
    max: f64,
}

impl TempoRange {
    pub fn new(min: f64, max: f64) -> Result<Self, AnalysisError> {
        let inside = |bpm: f64| bpm.is_finite() && (Bpm::MIN..=Bpm::MAX).contains(&bpm);
        if !inside(min) || !inside(max) || max < 1.5 * min {
            return Err(AnalysisError::Range);
        }
        Ok(Self { min, max })
    }
}

impl Default for TempoRange {
    fn default() -> Self {
        Self {
            min: 70.0,
            max: 180.0,
        }
    }
}

/// A tempo and where its beats fall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatTempo {
    pub bpm: f64,
    /// The first beat at or after the start of the track, in seconds.
    pub first_beat: f64,
}

pub(crate) fn detect(mono: &[f32], rate: f64, range: TempoRange) -> Option<BeatTempo> {
    let Onsets { flux, low } = onsets(mono);
    let frames_per_second = rate / HOP as f64;
    // A little room at either end, so a tempo on the edge of the range is found.
    let shortest = frames_per_second * 60.0 / range.max * (1.0 - EDGE);
    let longest = frames_per_second * 60.0 / range.min * (1.0 + EDGE);
    if (flux.len() as f64) < MIN_BEATS * longest {
        return None;
    }
    // A bright hi-hat can outweigh the kick in the flux and make a pattern of kicks and
    // off-beats repeat at a third of a beat; the low band, where the kick is, decides then.
    let period = candidates(&flux, shortest, longest)
        .into_iter()
        .map(|lag| {
            let (period, sharpness) = refine(&flux, lag);
            (period, sharpness * salience(&low, period).0.max(1.0))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(period, _)| octave(&flux, period, shortest, longest))?;
    if salience(&flux, period).0 < MIN_SALIENCE {
        return None;
    }
    let mut phase = fine_phase(&flux, period, best_phase(&flux, period));
    // A beat and its off-beat can hold the same flux; the beat is where the low band is.
    let half = (phase + period / 2.0) % period;
    if comb(&low, period, half) > LOW_LEAD * comb(&low, period, phase) {
        phase = half;
    }
    let bpm = whole(60.0 * frames_per_second / period);
    let beat = 60.0 / bpm;
    let start = (phase * HOP as f64 + FRAME as f64 / 2.0) / rate + ONSET_LEAD;
    Some(BeatTempo {
        bpm,
        first_beat: start.rem_euclid(beat),
    })
}

/// The flux of each frame, over the whole spectrum and over its low band.
struct Onsets {
    flux: Vec<f32>,
    low: Vec<f32>,
}

/// The flux of each frame with its local mean taken off, so the onsets stand out of the hum.
fn onsets(mono: &[f32]) -> Onsets {
    let mut spectrum = Spectrum::new(FRAME);
    let mut previous: Vec<f32> = Vec::new();
    let mut current = Vec::new();
    let mut flux = Vec::new();
    let mut low = Vec::new();
    let mut start = 0;
    while start + spectrum.size() <= mono.len() {
        spectrum.magnitudes(&mono[start..start + FRAME], &mut current);
        for magnitude in &mut current {
            *magnitude = (1.0 + 100.0 * *magnitude).ln();
        }
        let rises = |bins: std::ops::Range<usize>| -> f32 {
            if previous.is_empty() {
                return 0.0;
            }
            current[bins.clone()]
                .iter()
                .zip(&previous[bins])
                .map(|(now, before)| (now - before).max(0.0))
                .sum()
        };
        flux.push(rises(1..current.len()));
        low.push(rises(1..LOW_BINS + 1));
        std::mem::swap(&mut previous, &mut current);
        start += HOP;
    }
    Onsets {
        flux: above_mean(&flux),
        low: above_mean(&low),
    }
}

/// What each value holds above the mean of its neighbourhood.
fn above_mean(values: &[f32]) -> Vec<f32> {
    let mut sum = 0.0f64;
    let mut prefix = Vec::with_capacity(values.len() + 1);
    prefix.push(0.0);
    for value in values {
        sum += f64::from(*value);
        prefix.push(sum);
    }
    (0..values.len())
        .map(|index| {
            let low = index.saturating_sub(MEAN_RADIUS);
            let high = (index + MEAN_RADIUS + 1).min(values.len());
            let mean = (prefix[high] - prefix[low]) / (high - low) as f64;
            (f64::from(values[index]) - mean).max(0.0) as f32
        })
        .collect()
}

/// The lags, in frames, where the autocorrelation of the flux peaks inside the range: the
/// strongest three.
fn candidates(flux: &[f32], shortest: f64, longest: f64) -> Vec<f64> {
    let low = shortest.floor().max(1.0) as usize;
    let high = (longest.ceil() as usize + 1).min(flux.len().saturating_sub(1));
    let correlation: Vec<f64> = (low.saturating_sub(1)..=high + 1)
        .map(|lag| {
            flux.iter()
                .zip(flux.iter().skip(lag))
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum()
        })
        .collect();
    let mut peaks: Vec<(usize, f64)> = (1..correlation.len().saturating_sub(1))
        .filter(|&index| {
            correlation[index] >= correlation[index - 1]
                && correlation[index] > correlation[index + 1]
        })
        .map(|index| (index + low.saturating_sub(1), correlation[index]))
        .filter(|(lag, _)| (shortest..=longest).contains(&(*lag as f64)))
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    peaks
        .into_iter()
        .take(3)
        .map(|(lag, _)| lag as f64)
        .collect()
}

/// The period near `lag` whose phase histogram is sharpest, then the one near it whose comb
/// collects the most, with the salience of that period.
fn refine(flux: &[f32], lag: f64) -> (f64, f64) {
    let steps = 100i64;
    let coarse = (-steps..=steps)
        .map(|index| {
            let period = lag + index as f64 * 0.01;
            (period, salience(flux, period).0)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(lag, |(period, _)| period);
    let phase = best_phase(flux, coarse);
    // Fine enough that the beats drift by a fifth of a frame over the track at most.
    let beats = flux.len() as f64 / coarse;
    let step = (0.2 / beats).min(0.001);
    // Wider than a step of the coarse search, whose peak can sit a little off on a short track.
    let steps = (0.06 / step).ceil() as i64;
    let fine = (-steps..=steps)
        .map(|index| {
            let period = coarse + index as f64 * step;
            let phase = fine_phase(flux, period, phase);
            (period, comb(flux, period, phase))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(coarse, |(period, _)| period);
    (fine, salience(flux, fine).0)
}

/// The phase within two frames of `around` whose comb collects the most, to a twentieth of
/// a frame.
fn fine_phase(flux: &[f32], period: f64, around: f64) -> f64 {
    (0..=80)
        .map(|index| (around - 2.0 + index as f64 * 0.05).rem_euclid(period))
        .map(|phase| (phase, comb(flux, period, phase)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(around, |(phase, _)| phase)
}

/// How far the peak of the phase histogram of `period` stands above its mean, and the phase of
/// that peak in frames.
fn salience(flux: &[f32], period: f64) -> (f64, f64) {
    let mut bins = [0.0f64; PHASE_BINS];
    for (index, value) in flux.iter().enumerate() {
        let phase = (index as f64 / period).fract();
        let bin = ((phase * PHASE_BINS as f64) as usize).min(PHASE_BINS - 1);
        bins[bin] += f64::from(*value);
    }
    let mean = bins.iter().sum::<f64>() / PHASE_BINS as f64;
    if mean <= 0.0 {
        return (0.0, 0.0);
    }
    let (bin, peak) = (0..PHASE_BINS)
        .map(|bin| {
            let around = bins[(bin + PHASE_BINS - 1) % PHASE_BINS]
                + bins[bin]
                + bins[(bin + 1) % PHASE_BINS];
            (bin, around / 3.0)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, 0.0));
    (peak / mean, (bin as f64 + 0.5) / PHASE_BINS as f64 * period)
}

/// The flux a comb with teeth `period` apart collects from `phase` on, per tooth.
fn comb(flux: &[f32], period: f64, phase: f64) -> f64 {
    let mut total = 0.0;
    let mut teeth = 0usize;
    let mut position = phase;
    while position + 1.0 < flux.len() as f64 {
        let index = position as usize;
        let fraction = position - index as f64;
        total += f64::from(flux[index]) * (1.0 - fraction) + f64::from(flux[index + 1]) * fraction;
        teeth += 1;
        position += period;
    }
    if teeth == 0 {
        0.0
    } else {
        total / teeth as f64
    }
}

/// The phase in frames, within one period, whose comb collects the most.
fn best_phase(flux: &[f32], period: f64) -> f64 {
    let around = salience(flux, period).1;
    let width = period / PHASE_BINS as f64 * 2.0;
    let steps = (2.0 * width / 0.05).ceil() as usize;
    (0..=steps)
        .map(|index| (around - width + index as f64 * 0.05).rem_euclid(period))
        .map(|phase| (phase, comb(flux, period, phase)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(around, |(phase, _)| phase)
}

/// Brings `period` to the slowest tempo of its octaves in the range, then doubles the tempo
/// while the beats between its beats are nearly as strong as they are.
fn octave(flux: &[f32], mut period: f64, shortest: f64, longest: f64) -> f64 {
    while period * 2.0 <= longest {
        period *= 2.0;
    }
    while period / 2.0 >= shortest {
        let phase = best_phase(flux, period);
        let on = comb(flux, period, phase);
        let off = comb(flux, period, (phase + period / 2.0) % period);
        if on <= 0.0 || off < DOUBLE_SHARE * on {
            break;
        }
        period /= 2.0;
    }
    period
}

/// The tempo to a hundredth, or the whole number it sits near.
fn whole(bpm: f64) -> f64 {
    let rounded = bpm.round();
    if (bpm - rounded).abs() <= WHOLE {
        rounded
    } else {
        (bpm * 100.0).round() / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::{TempoRange, whole};

    #[test]
    fn a_range_lies_in_the_tempos_of_a_grid_and_holds_every_tempo() {
        assert!(TempoRange::new(70.0, 180.0).is_ok());
        assert!(TempoRange::new(10.0, 180.0).is_err());
        assert!(TempoRange::new(100.0, 140.0).is_err());
        assert!(TempoRange::new(f64::NAN, 180.0).is_err());
    }

    #[test]
    fn a_tempo_near_a_whole_number_is_that_number() {
        assert_eq!(whole(127.985), 128.0);
        assert_eq!(whole(127.3049), 127.3);
        assert_eq!(whole(127.96), 127.96);
    }
}
