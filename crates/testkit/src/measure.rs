//! Measurements on signals. Tests state what was measured; nobody can listen in CI.

use std::f64::consts::TAU;

pub fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    (sum / samples.len() as f64).sqrt()
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

/// Largest difference between consecutive samples. A click shows up as a step far above the
/// slope of the signal.
pub fn max_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .fold(0.0, |step, pair| step.max((pair[1] - pair[0]).abs()))
}

/// Ratio of two amplitudes in decibels.
pub fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The sine of a known frequency that best fits a signal, and what is left once it is removed.
#[derive(Clone, Copy, Debug)]
pub struct SineFit {
    pub amplitude: f64,
    /// RMS of everything that is not the fitted sine: distortion, aliasing and noise.
    pub residual_rms: f64,
}

impl SineFit {
    /// Signal-to-residual ratio in decibels.
    pub fn snr_db(self) -> f64 {
        db((self.amplitude / 2f64.sqrt()) / self.residual_rms.max(1e-30))
    }
}

/// Least-squares fit of `a*sin + b*cos` at `hz`.
pub fn sine_fit(samples: &[f32], hz: f64, rate: f64) -> SineFit {
    let phase = |n: usize| TAU * hz * n as f64 / rate;
    let (mut ss, mut cc, mut sc, mut xs, mut xc) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (n, sample) in samples.iter().enumerate() {
        let (sin, cos) = phase(n).sin_cos();
        let x = f64::from(*sample);
        ss += sin * sin;
        cc += cos * cos;
        sc += sin * cos;
        xs += x * sin;
        xc += x * cos;
    }
    let det = ss * cc - sc * sc;
    if det.abs() < 1e-12 {
        return SineFit {
            amplitude: 0.0,
            residual_rms: rms(samples),
        };
    }
    let a = (xs * cc - xc * sc) / det;
    let b = (xc * ss - xs * sc) / det;
    let residual: f64 = samples
        .iter()
        .enumerate()
        .map(|(n, sample)| {
            let (sin, cos) = phase(n).sin_cos();
            (f64::from(*sample) - a * sin - b * cos).powi(2)
        })
        .sum();
    SineFit {
        amplitude: a.hypot(b),
        residual_rms: (residual / samples.len() as f64).sqrt(),
    }
}

#[cfg(test)]
mod tests {
    use super::{max_step, peak, rms, sine_fit};
    use crate::signal::sine;

    #[test]
    fn fits_a_clean_sine() {
        let signal = sine(1_000.0, 48_000, 4_800, 0.5);
        let fit = sine_fit(&signal, 1_000.0, 48_000.0);
        assert!((fit.amplitude - 0.5).abs() < 1e-4);
        assert!(fit.snr_db() > 100.0);
        assert!((rms(&signal) - 0.5 / 2f64.sqrt()).abs() < 1e-4);
        assert!((peak(&signal) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn a_step_stands_out_from_the_slope() {
        let mut signal = sine(100.0, 48_000, 4_800, 0.5);
        let smooth = max_step(&signal);
        signal[2_400] += 0.3;
        assert!(max_step(&signal) > 10.0 * smooth);
    }
}
