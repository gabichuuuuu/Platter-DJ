//! Separation with the 4-stem Spleeter model: the short-time transform of the mixture, the four
//! networks on its magnitude, soft masks with an exponent of two, and the inverse transform of
//! each masked spectrum, as the code of its authors does it (MIT, Deezer Research,
//! <https://github.com/deezer/spleeter>, read on 21 November 2026).
//!
//! The mixture is processed a segment of 512 frames at a time, so the stems come out as they
//! are finished and are never all in memory. Behaviour note: `docs/behavior/stems.md`,
//! *Separation*.

use platter_core::Stem;

use crate::fft::{Complex, Fft};
use crate::net::{Map, UNet};
use crate::{SeparationError, Separator};

/// The rate the model works at.
pub const RATE: u32 = 44_100;
const FRAME: usize = 4096;
const HOP: usize = 1024;
/// Frames of a segment and bins of the spectrum the networks see.
const T: usize = 512;
const F: usize = 1024;
const BINS: usize = FRAME / 2 + 1;
/// The overlap of the Hann window at a quarter of its length adds up to 1.5.
const WINDOW_COMPENSATION: f32 = 2.0 / 3.0;
const EPSILON: f32 = 1e-10;
/// The order of the networks in the model: vocals, drums, bass, other.
const MODEL_ORDER: [Stem; 4] = [Stem::Vocals, Stem::Drums, Stem::Bass, Stem::Other];

/// The model: one network per stem, in the order of [`MODEL_ORDER`].
pub struct Spleeter {
    nets: [UNet; 4],
    fft: Fft,
    window: Vec<f32>,
    threads: usize,
}

impl Spleeter {
    /// The networks in the order vocals, drums, bass, other.
    pub fn new(nets: [UNet; 4], threads: usize) -> Option<Self> {
        let window = (0..FRAME)
            .map(|n| {
                (0.5 - 0.5 * (2.0 * std::f64::consts::PI * n as f64 / FRAME as f64).cos()) as f32
            })
            .collect();
        Some(Self {
            nets,
            fft: Fft::new(FRAME)?,
            window,
            threads: threads.max(1),
        })
    }

    /// The spectrum of the frames `first..first + T` of the padded mixture, per channel.
    fn spectra(
        &self,
        padded: &impl Fn(usize, usize) -> f32,
        first: usize,
        frames: usize,
    ) -> [Vec<Complex>; 2] {
        let mut scratch = Vec::with_capacity(FRAME);
        let mut samples = vec![0.0f32; FRAME];
        [0, 1].map(|channel| {
            let mut spectrum = vec![Complex::ZERO; T * BINS];
            for frame in 0..frames.min(T) {
                let start = (first + frame) * HOP;
                for (n, sample) in samples.iter_mut().enumerate() {
                    *sample = padded(start + n, channel) * self.window[n];
                }
                self.fft.real_forward(
                    &samples,
                    &mut scratch,
                    &mut spectrum[frame * BINS..(frame + 1) * BINS],
                );
            }
            spectrum
        })
    }
}

impl Separator for Spleeter {
    fn rate(&self) -> u32 {
        RATE
    }

    fn separate(
        &self,
        stereo: &[f32],
        sink: &mut dyn FnMut(Stem, &[f32]),
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<(), SeparationError> {
        let length = stereo.len() / 2;
        // A frame of silence before the track, as the model was trained with it.
        let padded = |at: usize, channel: usize| {
            at.checked_sub(FRAME)
                .and_then(|index| stereo.get(index * 2 + channel))
                .copied()
                .unwrap_or(0.0)
        };
        let total = FRAME + length;
        let frames = total.div_ceil(HOP);
        let segments = frames.div_ceil(T).max(1);
        // What each stem adds up, from the first sample of the current segment on.
        let mut added: [[Vec<f32>; 2]; 4] =
            std::array::from_fn(|_| [vec![0.0; T * HOP + FRAME], vec![0.0; T * HOP + FRAME]]);
        let mut emitted = 0usize;
        let mut scratch = Vec::with_capacity(FRAME);
        let mut frame_out = vec![0.0f32; FRAME];
        for segment in 0..segments {
            if !progress(segment as f32 / segments as f32) {
                return Err(SeparationError::Cancelled);
            }
            let first = segment * T;
            let count = (frames - first).min(T);
            let spectra = self.spectra(&padded, first, count);
            let mut input = Map::zeros(T, F, 2);
            for frame in 0..T {
                for bin in 0..F {
                    for (channel, spectrum) in spectra.iter().enumerate() {
                        input.data[(frame * F + bin) * 2 + channel] =
                            spectrum[frame * BINS + bin].norm();
                    }
                }
            }
            let mut estimates = Vec::with_capacity(4);
            for net in &self.nets {
                estimates.push(
                    net.apply(&input, self.threads)
                        .ok_or(SeparationError::Weights)?,
                );
            }
            for (model_index, stem) in MODEL_ORDER.into_iter().enumerate() {
                for (channel, spectrum) in spectra.iter().enumerate() {
                    let buffer = &mut added[stem.index()][channel];
                    for frame in 0..count {
                        let mut masked = [Complex::ZERO; BINS];
                        for bin in 0..F {
                            let at = (frame * F + bin) * 2 + channel;
                            let power = |estimate: &Map| estimate.data[at] * estimate.data[at];
                            let sum: f32 = estimates.iter().map(power).sum::<f32>() + EPSILON;
                            let mask = (power(&estimates[model_index]) + EPSILON / 4.0) / sum;
                            masked[bin] = spectrum[frame * BINS + bin].scale(mask);
                        }
                        // Above the bins of the networks the mask is zero.
                        self.fft.real_inverse(&masked, &mut scratch, &mut frame_out);
                        let start = frame * HOP;
                        for (n, sample) in frame_out.iter().enumerate() {
                            buffer[start + n] += sample * self.window[n] * WINDOW_COMPENSATION;
                        }
                    }
                }
            }
            // The samples before the next segment are finished; those of the first frame of
            // silence are dropped, and nothing past the end of the track goes out.
            let finished_end = ((segment + 1) * T * HOP).min(total);
            let segment_start = segment * T * HOP;
            let from = emitted.max(FRAME).max(segment_start);
            if finished_end > from {
                for stem in Stem::ALL {
                    let [left, right] = &added[stem.index()];
                    let mut interleaved = Vec::with_capacity((finished_end - from) * 2);
                    for at in from..finished_end {
                        interleaved.push(left[at - segment_start]);
                        interleaved.push(right[at - segment_start]);
                    }
                    sink(stem, &interleaved);
                }
            }
            emitted = finished_end;
            // What the last frames of this segment add past it belongs to the next one.
            for pair in &mut added {
                for buffer in pair {
                    buffer.copy_within(T * HOP.., 0);
                    let keep = buffer.len() - T * HOP;
                    buffer[keep..].fill(0.0);
                }
            }
        }
        progress(1.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Conv, FILTERS, Norm};

    fn conv(size: usize, inputs: usize, outputs: usize, transposed: bool) -> Conv {
        Conv {
            size,
            stride: if size == 4 { 1 } else { 2 },
            dilation: if size == 4 { 2 } else { 1 },
            inputs,
            outputs,
            kernel: vec![0.0; size * size * inputs * outputs],
            bias: vec![0.0; outputs],
            transposed,
        }
    }

    fn norm(channels: usize) -> Norm {
        Norm::from_batch(
            &vec![1.0; channels],
            &vec![0.0; channels],
            &vec![0.0; channels],
            &vec![1.0; channels],
        )
    }

    /// A network whose mask is the same for every stem.
    fn flat_net() -> UNet {
        let mut encoder = Vec::new();
        let mut inputs = 2;
        for filters in FILTERS {
            encoder.push((conv(5, inputs, filters, false), norm(filters)));
            inputs = filters;
        }
        let outs = [256, 128, 64, 32, 16, 1];
        let ins = [512, 512, 256, 128, 64, 32];
        let decoder = outs
            .into_iter()
            .zip(ins)
            .map(|(out, inp)| (conv(5, inp, out, true), norm(out)))
            .collect();
        UNet {
            encoder,
            decoder,
            last: conv(4, 1, 2, false),
        }
    }

    #[test]
    fn four_equal_masks_give_each_stem_a_quarter_of_the_low_band() {
        let model = Spleeter::new(std::array::from_fn(|_| flat_net()), 4).unwrap();
        // Two seconds of a 440 Hz sine, below the 11 kHz the networks see.
        let length = 2 * RATE as usize;
        let stereo: Vec<f32> = (0..length * 2)
            .map(|i| {
                (2.0 * std::f32::consts::PI * 440.0 * (i / 2) as f32 / RATE as f32).sin() * 0.5
            })
            .collect();
        let mut stems: [Vec<f32>; 4] = Default::default();
        let mut steps = 0;
        model
            .separate(
                &stereo,
                &mut |stem, samples| stems[stem.index()].extend_from_slice(samples),
                &mut |_| {
                    steps += 1;
                    true
                },
            )
            .unwrap();
        assert!(steps >= 2);
        for stem in &stems {
            assert_eq!(stem.len(), stereo.len());
            // Away from the edges each stem is a quarter of the mixture.
            for at in (20_000..stem.len() - 20_000).step_by(997) {
                assert!((stem[at] - stereo[at] / 4.0).abs() < 1e-3, "{at}");
            }
        }
    }

    #[test]
    fn a_cancelled_separation_stops_and_says_so() {
        let model = Spleeter::new(std::array::from_fn(|_| flat_net()), 1).unwrap();
        let stereo = vec![0.0; 2 * 1000];
        let outcome = model.separate(&stereo, &mut |_, _| {}, &mut |_| false);
        assert!(matches!(outcome, Err(SeparationError::Cancelled)));
    }
}
