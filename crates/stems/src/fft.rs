//! A radix-2 fast Fourier transform for the frames of the short-time transform.

/// A complex number of single precision.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    pub fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    pub fn norm(self) -> f32 {
        self.re.hypot(self.im)
    }

    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.re * factor, self.im * factor)
    }

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }

    fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }
}

/// A transform of one size, a power of two, with its twiddle factors.
pub struct Fft {
    size: usize,
    twiddles: Vec<Complex>,
    order: Vec<usize>,
}

impl Fft {
    /// `None` when `size` is not a power of two of at least 2.
    pub fn new(size: usize) -> Option<Self> {
        if size < 2 || !size.is_power_of_two() {
            return None;
        }
        let twiddles = (0..size / 2)
            .map(|index| {
                let angle = -2.0 * std::f64::consts::PI * index as f64 / size as f64;
                Complex::new(angle.cos() as f32, angle.sin() as f32)
            })
            .collect();
        let bits = size.trailing_zeros();
        let order = (0..size)
            .map(|index| index.reverse_bits() >> (usize::BITS - bits))
            .collect();
        Some(Self {
            size,
            twiddles,
            order,
        })
    }

    /// The forward transform of `data`, in place, without scaling. `data` has the size of
    /// the transform; another length is left as it is.
    pub fn forward(&self, data: &mut [Complex]) {
        if data.len() != self.size {
            return;
        }
        for (index, &target) in self.order.iter().enumerate() {
            if index < target {
                data.swap(index, target);
            }
        }
        let mut half = 1;
        while half < self.size {
            let stride = self.size / (2 * half);
            for start in (0..self.size).step_by(2 * half) {
                for offset in 0..half {
                    let twiddle = self.twiddles[offset * stride];
                    let even = data[start + offset];
                    let odd = data[start + offset + half].mul(twiddle);
                    data[start + offset] = even.add(odd);
                    data[start + offset + half] = even.sub(odd);
                }
            }
            half *= 2;
        }
    }

    /// The spectrum of a real frame: its `size / 2 + 1` first bins.
    pub fn real_forward(&self, frame: &[f32], scratch: &mut Vec<Complex>, bins: &mut [Complex]) {
        scratch.clear();
        scratch.extend(frame.iter().map(|&sample| Complex::new(sample, 0.0)));
        scratch.resize(self.size, Complex::ZERO);
        self.forward(scratch);
        for (bin, value) in bins.iter_mut().zip(scratch.iter()) {
            *bin = *value;
        }
    }

    /// The real frame of a spectrum given by its `size / 2 + 1` first bins, scaled by
    /// `1 / size` as an inverse transform is.
    pub fn real_inverse(&self, bins: &[Complex], scratch: &mut Vec<Complex>, frame: &mut [f32]) {
        let half = self.size / 2;
        scratch.clear();
        scratch.resize(self.size, Complex::ZERO);
        for (index, bin) in bins.iter().take(half + 1).enumerate() {
            // The inverse is the forward transform of the conjugate, conjugated.
            scratch[index] = bin.conj();
            if index > 0 && index < half {
                scratch[self.size - index] = *bin;
            }
        }
        self.forward(scratch);
        let scale = 1.0 / self.size as f32;
        for (sample, value) in frame.iter_mut().zip(scratch.iter()) {
            *sample = value.re * scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_lands_in_its_bin_and_comes_back() {
        let size = 64;
        let fft = Fft::new(size).unwrap();
        let frame: Vec<f32> = (0..size)
            .map(|index| (2.0 * std::f32::consts::PI * 5.0 * index as f32 / size as f32).cos())
            .collect();
        let mut scratch = Vec::new();
        let mut bins = vec![Complex::ZERO; size / 2 + 1];
        fft.real_forward(&frame, &mut scratch, &mut bins);
        let peak = bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.norm().total_cmp(&b.1.norm()))
            .map(|(index, _)| index);
        assert_eq!(peak, Some(5));
        assert!((bins[5].norm() - size as f32 / 2.0).abs() < 1e-3);
        let mut back = vec![0.0; size];
        fft.real_inverse(&bins, &mut scratch, &mut back);
        for (a, b) in frame.iter().zip(&back) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn only_powers_of_two_make_a_transform() {
        assert!(Fft::new(4096).is_some());
        assert!(Fft::new(1000).is_none());
        assert!(Fft::new(1).is_none());
    }
}
