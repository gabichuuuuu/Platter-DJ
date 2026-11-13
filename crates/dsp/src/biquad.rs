//! Second-order filter sections.
//!
//! Coefficients follow the Audio EQ Cookbook by Robert Bristow-Johnson
//! (<https://www.w3.org/TR/audio-eq-cookbook/>). The section runs in transposed direct
//! form II.

use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use platter_core::SampleRate;

/// Quality factor of a second-order Butterworth section.
pub const BUTTERWORTH_Q: f64 = FRAC_1_SQRT_2;

/// State values below this magnitude are flushed to zero so that a decaying tail does not
/// reach the denormal range, where arithmetic becomes slow.
const FLUSH_BELOW: f32 = 1e-25;

#[derive(Clone, Copy, Debug)]
pub struct Coefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coefficients {
    pub fn low_pass(rate: SampleRate, hz: f64, q: f64) -> Self {
        let (cos, alpha) = prototype(rate, hz, q);
        let b1 = 1.0 - cos;
        Self::normalised([b1 / 2.0, b1, b1 / 2.0], cos, alpha)
    }

    pub fn high_pass(rate: SampleRate, hz: f64, q: f64) -> Self {
        let (cos, alpha) = prototype(rate, hz, q);
        let b1 = -(1.0 + cos);
        Self::normalised([-b1 / 2.0, b1, -b1 / 2.0], cos, alpha)
    }

    fn normalised(b: [f64; 3], cos: f64, alpha: f64) -> Self {
        let a0 = 1.0 + alpha;
        Self {
            b0: (b[0] / a0) as f32,
            b1: (b[1] / a0) as f32,
            b2: (b[2] / a0) as f32,
            a1: (-2.0 * cos / a0) as f32,
            a2: ((1.0 - alpha) / a0) as f32,
        }
    }
}

/// Cosine of the corner frequency and the `alpha` term of the cookbook. The corner is kept
/// inside the band where the section is stable, whatever the caller passes.
fn prototype(rate: SampleRate, hz: f64, q: f64) -> (f64, f64) {
    let hz = if hz.is_finite() { hz } else { 1_000.0 };
    let q = if q.is_finite() {
        q.max(0.1)
    } else {
        BUTTERWORTH_Q
    };
    let omega = TAU * hz.clamp(10.0, 0.45 * rate.as_f64()) / rate.as_f64();
    (omega.cos(), omega.sin() / (2.0 * q))
}

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    coefficients: Coefficients,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn new(coefficients: Coefficients) -> Self {
        Self {
            coefficients,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let c = &self.coefficients;
        let output = c.b0 * input + self.z1;
        self.z1 = flush(c.b1 * input - c.a1 * output + self.z2);
        self.z2 = flush(c.b2 * input - c.a2 * output);
        output
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

fn flush(value: f32) -> f32 {
    if value.abs() < FLUSH_BELOW {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::{db, rms};
    use platter_testkit::signal::sine;

    use super::{BUTTERWORTH_Q, Biquad, Coefficients};

    const RATE: u32 = 48_000;

    fn gain_db(coefficients: Coefficients, hz: f64) -> f64 {
        let mut section = Biquad::new(coefficients);
        let input = sine(hz, RATE, 48_000, 0.5);
        let output: Vec<f32> = input.iter().map(|x| section.process(*x)).collect();
        db(rms(&output[24_000..]) / rms(&input[24_000..]))
    }

    #[test]
    fn low_pass_is_three_decibels_down_at_the_corner() {
        let rate = SampleRate::new(RATE).unwrap();
        let low_pass = Coefficients::low_pass(rate, 1_000.0, BUTTERWORTH_Q);
        assert!(gain_db(low_pass, 100.0).abs() < 0.1);
        assert!((gain_db(low_pass, 1_000.0) + 3.01).abs() < 0.1);
        // Second order: 12 dB per octave.
        assert!((gain_db(low_pass, 4_000.0) + 24.1).abs() < 0.5);
    }

    #[test]
    fn high_pass_mirrors_the_low_pass() {
        let rate = SampleRate::new(RATE).unwrap();
        let high_pass = Coefficients::high_pass(rate, 1_000.0, BUTTERWORTH_Q);
        assert!(gain_db(high_pass, 10_000.0).abs() < 0.1);
        assert!((gain_db(high_pass, 1_000.0) + 3.01).abs() < 0.1);
        assert!((gain_db(high_pass, 250.0) + 24.1).abs() < 0.5);
    }

    #[test]
    fn unusable_parameters_still_give_a_stable_section() {
        let rate = SampleRate::new(RATE).unwrap();
        for (hz, q) in [
            (f64::NAN, 0.7),
            (1e9, 0.7),
            (-5.0, 0.7),
            (1_000.0, f64::NAN),
            (1_000.0, 0.0),
        ] {
            let mut section = Biquad::new(Coefficients::low_pass(rate, hz, q));
            let output: Vec<f32> = sine(440.0, RATE, 48_000, 1.0)
                .iter()
                .map(|x| section.process(*x))
                .collect();
            assert!(output.iter().all(|y| y.is_finite() && y.abs() < 20.0));
        }
    }
}
