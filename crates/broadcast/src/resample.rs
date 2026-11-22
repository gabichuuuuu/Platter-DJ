//! A streaming resampler for stereo: blocks of any length go in, and the samples at the new rate
//! come out, the state kept from one block to the next so the joins are seamless.
//!
//! Each output sample is a windowed sinc over the input around its position (band-limited
//! interpolation, J. O. Smith, *Digital Audio Resampling*,
//! <https://ccrma.stanford.edu/~jos/resample/>), with the cut-off below the lower of the two
//! Nyquist frequencies so nothing above it folds back. The window is Blackman.

use std::f64::consts::PI;

/// Input frames on each side of an output sample.
const HALF_TAPS: usize = 16;
/// The cut-off, as a share of the lower Nyquist frequency: room for the window to roll off.
const CUTOFF: f64 = 0.92;

pub struct Resampler {
    /// Input frames per output frame.
    step: f64,
    /// The cut-off as a share of the input Nyquist frequency.
    cutoff: f64,
    /// The last input frames, oldest first, long enough for the taps of the next output.
    history: Vec<[f32; 2]>,
    /// Where the next output frame falls, in input frames from the start of `history`.
    position: f64,
    same_rate: bool,
}

impl Resampler {
    /// From `from` to `to` frames per second, both above zero.
    pub fn new(from: u32, to: u32) -> Self {
        let from = f64::from(from.max(1));
        let to = f64::from(to.max(1));
        let step = from / to;
        Self {
            step,
            cutoff: CUTOFF * (to / from).min(1.0),
            // Silence before the first block, so its first samples have their taps.
            history: vec![[0.0; 2]; HALF_TAPS],
            position: HALF_TAPS as f64,
            same_rate: from == to,
        }
    }

    /// Resamples the interleaved stereo `input` and appends the result, interleaved, to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.same_rate {
            out.extend_from_slice(&input[..input.len() / 2 * 2]);
            return;
        }
        self.history
            .extend(input.as_chunks::<2>().0.iter().copied());
        // An output frame needs HALF_TAPS input frames after its position.
        while self.position + HALF_TAPS as f64 + 1.0 <= self.history.len() as f64 {
            let frame = self.at(self.position);
            out.extend_from_slice(&frame);
            self.position += self.step;
        }
        // Keep what the next outputs still need.
        let keep_from = (self.position.floor() as usize).saturating_sub(HALF_TAPS);
        if keep_from > 0 {
            self.history.drain(..keep_from);
            self.position -= keep_from as f64;
        }
    }

    fn at(&self, position: f64) -> [f32; 2] {
        let centre = position.floor() as isize;
        let fraction = position - centre as f64;
        let mut sum = [0.0f64; 2];
        let mut weight_sum = 0.0;
        for tap in -(HALF_TAPS as isize) + 1..=HALF_TAPS as isize {
            let index = centre + tap;
            let Some(frame) = usize::try_from(index)
                .ok()
                .and_then(|index| self.history.get(index))
            else {
                continue;
            };
            let distance = tap as f64 - fraction;
            let weight = self.kernel(distance);
            weight_sum += weight;
            sum[0] += weight * f64::from(frame[0]);
            sum[1] += weight * f64::from(frame[1]);
        }
        // The weights add up to about one; dividing by their sum keeps a constant level exact.
        let norm = if weight_sum.abs() > 1e-9 {
            weight_sum
        } else {
            1.0
        };
        [(sum[0] / norm) as f32, (sum[1] / norm) as f32]
    }

    /// The low-pass at `distance` input frames from the output sample.
    fn kernel(&self, distance: f64) -> f64 {
        let width = HALF_TAPS as f64;
        if distance.abs() >= width {
            return 0.0;
        }
        let x = PI * self.cutoff * distance;
        let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
        // Blackman, centred on the output sample.
        let phase = PI * (distance / width + 1.0);
        let window = 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
        self.cutoff * sinc * window
    }

    /// Lets out the frames the taps still hold, behind silence: what an end of the input needs.
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        if !self.same_rate {
            // The last input frame is HALF_TAPS frames behind the newest output, and that output
            // needs HALF_TAPS frames after it.
            let silence = vec![0.0; 2 * (2 * HALF_TAPS + 2)];
            self.process(&silence, out);
        }
    }

    /// Output frames this resampler adds to the stream before the first input frame: the delay
    /// of its taps, at the output rate.
    pub fn delay_frames(&self) -> usize {
        if self.same_rate {
            0
        } else {
            (HALF_TAPS as f64 / self.step).round() as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|frame| {
                let value = (2.0 * PI * hz * frame as f64 / f64::from(rate)).sin() as f32 * 0.5;
                [value, value]
            })
            .collect()
    }

    /// The level and the frequency, by its crossings of zero, of the left channel of `stereo`
    /// past its first `skip` frames.
    fn measure(stereo: &[f32], rate: u32, skip: usize) -> (f64, f64) {
        let left: Vec<f64> = stereo
            .as_chunks::<2>()
            .0
            .iter()
            .skip(skip)
            .map(|frame| f64::from(frame[0]))
            .collect();
        let rms = (left.iter().map(|value| value * value).sum::<f64>() / left.len() as f64).sqrt();
        let rises = left
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count();
        (rms, rises as f64 * f64::from(rate) / left.len() as f64)
    }

    #[test]
    fn a_sine_keeps_its_frequency_and_level_from_44100_to_48000() {
        let input = sine(44_100, 1_000.0, 44_100);
        let mut resampler = Resampler::new(44_100, 48_000);
        let mut out = Vec::new();
        // In uneven blocks: the joins must not show.
        for block in input.chunks(2 * 333) {
            resampler.process(block, &mut out);
        }
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() < 40, "{frames}");
        let (rms, hz) = measure(&out, 48_000, 200);
        assert!((rms - 0.5 / 2f64.sqrt()).abs() < 0.005, "{rms}");
        assert!((hz - 1_000.0).abs() < 2.0, "{hz}");
        // No jump at a join: neighbouring samples of a 1 kHz sine differ by at most 2πf/fs.
        let steepest = out
            .as_chunks::<2>()
            .0
            .iter()
            .skip(200)
            .map(|frame| frame[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(f64::from(steepest) < 0.5 * 2.0 * PI * 1_000.0 / 48_000.0 * 1.05);
    }

    #[test]
    fn a_tone_above_the_new_nyquist_frequency_is_removed_from_96000_to_48000() {
        let input = sine(96_000, 30_000.0, 96_000);
        let mut resampler = Resampler::new(96_000, 48_000);
        let mut out = Vec::new();
        resampler.process(&input, &mut out);
        let (rms, _) = measure(&out, 48_000, 200);
        // Folded back it would be a full-level tone at 18 kHz.
        assert!(rms < 0.01, "{rms}");
    }

    #[test]
    fn the_same_rate_passes_the_samples_as_they_are() {
        let input = sine(48_000, 440.0, 100);
        let mut resampler = Resampler::new(48_000, 48_000);
        let mut out = Vec::new();
        resampler.process(&input, &mut out);
        assert_eq!(out, input);
        assert_eq!(resampler.delay_frames(), 0);
    }
}
