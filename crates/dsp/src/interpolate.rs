//! Reading a signal between its samples.
//!
//! The engine plays a track at a ratio that is rarely one, so every output sample falls
//! between two stored ones. The measurements that led to the cubic are in
//! `docs/decisions/0004-interpolation.md`.

/// Straight line between the two samples around the position.
pub fn linear(before: f32, after: f32, fraction: f32) -> f32 {
    before + (after - before) * fraction
}

/// Cubic Hermite interpolation with Catmull-Rom tangents through four samples; the position
/// lies between `samples[1]` and `samples[2]`.
///
/// Source: E. Catmull and R. Rom, "A class of local interpolating splines", in Computer
/// Aided Geometric Design, 1974 (<https://doi.org/10.1016/B978-0-12-079050-0.50020-5>).
pub fn cubic(samples: [f32; 4], fraction: f32) -> f32 {
    let [y0, y1, y2, y3] = samples;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * fraction + c2) * fraction + c1) * fraction + y1
}

#[cfg(test)]
mod tests {
    use platter_testkit::measure::sine_fit;
    use platter_testkit::signal::sine;

    use super::{cubic, linear};

    const RATE: u32 = 48_000;

    fn at(source: &[f32], index: isize) -> f32 {
        usize::try_from(index)
            .ok()
            .and_then(|index| source.get(index))
            .copied()
            .unwrap_or(0.0)
    }

    /// Reads `source` at `ratio` source samples per output sample.
    fn resample(source: &[f32], ratio: f64, use_cubic: bool) -> Vec<f32> {
        let frames = ((source.len() - 8) as f64 / ratio) as usize;
        (0..frames)
            .map(|n| {
                let position = 2.0 + n as f64 * ratio;
                let base = position.floor() as isize;
                let fraction = (position - position.floor()) as f32;
                if use_cubic {
                    let taps = [-1, 0, 1, 2].map(|offset| at(source, base + offset));
                    cubic(taps, fraction)
                } else {
                    linear(at(source, base), at(source, base + 1), fraction)
                }
            })
            .collect()
    }

    /// Signal-to-residual ratio of a resampled sine, in decibels.
    fn snr_db(hz: f64, ratio: f64, use_cubic: bool) -> f64 {
        let source = sine(hz, RATE, 48_000, 0.5);
        let output = resample(&source, ratio, use_cubic);
        sine_fit(&output, hz * ratio, f64::from(RATE)).snr_db()
    }

    #[test]
    fn both_pass_through_the_stored_samples() {
        assert_eq!(linear(0.25, 0.75, 0.0), 0.25);
        assert_eq!(cubic([0.1, 0.25, 0.75, 0.9], 0.0), 0.25);
        assert!((cubic([0.1, 0.25, 0.75, 0.9], 1.0) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn the_cubic_reproduces_a_straight_line() {
        for fraction in [0.1, 0.5, 0.9] {
            let value = cubic([0.0, 1.0, 2.0, 3.0], fraction);
            assert!((value - (1.0 + fraction)).abs() < 1e-6);
        }
    }

    #[test]
    fn the_cubic_is_cleaner_than_the_line() {
        // A tempo change of +6 % and the 44.1 kHz to 48 kHz ratio.
        for ratio in [1.06, 44_100.0 / 48_000.0] {
            for hz in [100.0, 1_000.0, 5_000.0] {
                let line = snr_db(hz, ratio, false);
                let curve = snr_db(hz, ratio, true);
                println!("ratio {ratio:.4} {hz:>6} Hz: linear {line:6.1} dB, cubic {curve:6.1} dB");
                assert!(curve > line + 6.0, "{hz} Hz at {ratio}: {curve} vs {line}");
            }
        }
    }

    #[test]
    fn the_cubic_meets_its_measured_floor() {
        // Thresholds a little under the figures recorded in the decision record.
        assert!(snr_db(100.0, 1.06, true) > 140.0);
        assert!(snr_db(1_000.0, 1.06, true) > 88.0);
        assert!(snr_db(5_000.0, 1.06, true) > 45.0);
    }
}
