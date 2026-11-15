//! A radix-2 fast Fourier transform and the magnitude spectrum of a windowed frame.

use std::f32::consts::TAU;

/// The magnitude spectrum of frames of one power-of-two length, with a Hann window.
pub(crate) struct Spectrum {
    size: usize,
    window: Vec<f32>,
    /// `cos` and `sin` of the twiddle factors, `size / 2` of each.
    twiddles: Vec<(f32, f32)>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl Spectrum {
    /// `size` is a power of two, at least 4; the callers pass constants.
    pub(crate) fn new(size: usize) -> Self {
        let window = (0..size)
            .map(|index| 0.5 - 0.5 * (TAU * index as f32 / size as f32).cos())
            .collect();
        let twiddles = (0..size / 2)
            .map(|index| {
                let angle = -TAU * index as f32 / size as f32;
                (angle.cos(), angle.sin())
            })
            .collect();
        Self {
            size,
            window,
            twiddles,
            re: vec![0.0; size],
            im: vec![0.0; size],
        }
    }

    pub(crate) fn size(&self) -> usize {
        self.size
    }

    /// Writes the magnitudes of the bins `0..=size / 2` of `frame` into `out`. A frame shorter
    /// than the size is padded with silence.
    pub(crate) fn magnitudes(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        for index in 0..self.size {
            let sample = frame.get(index).copied().unwrap_or(0.0);
            self.re[index] = sample * self.window[index];
            self.im[index] = 0.0;
        }
        self.transform();
        out.clear();
        out.extend(
            self.re[..=self.size / 2]
                .iter()
                .zip(&self.im)
                .map(|(re, im)| re.hypot(*im)),
        );
    }

    fn transform(&mut self) {
        let size = self.size;
        let bits = size.trailing_zeros();
        for index in 0..size {
            let reversed = index.reverse_bits() >> (usize::BITS - bits);
            if reversed > index {
                self.re.swap(index, reversed);
                self.im.swap(index, reversed);
            }
        }
        let mut half = 1;
        while half < size {
            let stride = size / (2 * half);
            for start in (0..size).step_by(2 * half) {
                for offset in 0..half {
                    let (cos, sin) = self.twiddles[offset * stride];
                    let even = start + offset;
                    let odd = even + half;
                    let re = self.re[odd] * cos - self.im[odd] * sin;
                    let im = self.re[odd] * sin + self.im[odd] * cos;
                    self.re[odd] = self.re[even] - re;
                    self.im[odd] = self.im[even] - im;
                    self.re[even] += re;
                    self.im[even] += im;
                }
            }
            half *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::Spectrum;

    #[test]
    fn a_sine_on_a_bin_peaks_on_that_bin_at_a_quarter_of_the_size_times_its_amplitude() {
        let mut spectrum = Spectrum::new(1024);
        let frame: Vec<f32> = (0..1024)
            .map(|index| 0.5 * (TAU * 64.0 * index as f32 / 1024.0).sin())
            .collect();
        let mut out = Vec::new();
        spectrum.magnitudes(&frame, &mut out);
        assert_eq!(out.len(), 513);
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(bin, _)| bin);
        assert_eq!(peak, Some(64));
        // A Hann window keeps half the amplitude; a real sine puts half of it in each half.
        assert!((out[64] - 0.5 * 1024.0 / 4.0).abs() < 0.01, "{}", out[64]);
        assert!(out[80] < 1e-3);
    }
}
