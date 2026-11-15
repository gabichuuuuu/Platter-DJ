//! Integrated loudness of a track, as ITU-R BS.1770-5 defines it: K weighting, blocks of
//! 400 ms that overlap by three quarters, an absolute gate at -70 LUFS and a relative gate
//! 10 LU below the loudness of the blocks that pass the first.
//!
//! Behaviour note: `docs/behavior/autogain.md`.

/// The coefficients of the two stages of the K filter at 48 kHz, as the recommendation gives
/// them: `[b0, b1, b2, a1, a2]`, with `a0` one.
const SHELF_48K: [f64; 5] = [
    1.535_124_859_586_97,
    -2.691_696_189_406_38,
    1.198_392_810_852_85,
    -1.690_659_293_182_41,
    0.732_480_774_215_85,
];
const HIGH_PASS_48K: [f64; 5] = [1.0, -2.0, 1.0, -1.990_047_454_833_98, 0.990_072_250_366_21];
const REFERENCE_RATE: f64 = 48_000.0;

const BLOCK_SECONDS: f64 = 0.4;
/// A new block starts every quarter of a block.
const STEPS_PER_BLOCK: usize = 4;
const ABSOLUTE_GATE: f64 = -70.0;
const RELATIVE_GATE: f64 = -10.0;

/// The integrated loudness, in LUFS, of interleaved samples with `channels` channels at `rate`
/// frames per second. Every channel weighs one, as the left and the right channel do in the
/// recommendation. `None` when no block passes the gates: silence, near silence, or fewer
/// samples than one block.
pub(crate) fn integrated(samples: &[f32], channels: usize, rate: u32) -> Option<f64> {
    let step = (BLOCK_SECONDS * f64::from(rate) / STEPS_PER_BLOCK as f64).round() as usize;
    if channels == 0 || step == 0 {
        return None;
    }
    let mut filters: Vec<[Biquad; 2]> = (0..channels)
        .map(|_| {
            [
                Biquad::at(SHELF_48K, f64::from(rate)),
                Biquad::at(HIGH_PASS_48K, f64::from(rate)),
            ]
        })
        .collect();
    // The weighted energy of each quarter of a block, summed over the channels.
    let quarters: Vec<f64> = samples
        .chunks(step * channels)
        .filter(|chunk| chunk.len() == step * channels)
        .map(|chunk| {
            let mut energy = 0.0;
            for frame in chunk.chunks_exact(channels) {
                for (sample, [shelf, high_pass]) in frame.iter().zip(&mut filters) {
                    let weighted = high_pass.next(shelf.next(f64::from(*sample)));
                    energy += weighted * weighted;
                }
            }
            energy
        })
        .collect();
    let block_frames = (step * STEPS_PER_BLOCK) as f64;
    let blocks: Vec<f64> = quarters
        .windows(STEPS_PER_BLOCK)
        .map(|window| window.iter().sum::<f64>() / block_frames)
        .filter(|power| loudness(*power) > ABSOLUTE_GATE)
        .collect();
    let relative = loudness(mean(&blocks)?) + RELATIVE_GATE;
    let kept: Vec<f64> = blocks
        .into_iter()
        .filter(|power| loudness(*power) > relative)
        .collect();
    Some(loudness(mean(&kept)?))
}

fn loudness(power: f64) -> f64 {
    -0.691 + 10.0 * power.log10()
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// One stage of the K filter, in direct form I.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    inputs: [f64; 2],
    outputs: [f64; 2],
}

impl Biquad {
    /// The stage given at 48 kHz, carried to `rate` by a bilinear map: the filter at 48 kHz
    /// is taken back to the analogue one it came from and brought forward again at `rate`.
    /// The response at a frequency moves only where the two rates warp it differently, near
    /// their Nyquist frequencies, far above where the K filter changes.
    fn at(coefficients: [f64; 5], rate: f64) -> Self {
        let [b0, b1, b2, a1, a2] = coefficients;
        // z at 48 kHz is (p z' + q) / (q z' + p) of z' at `rate`.
        let (p, q) = (REFERENCE_RATE + rate, REFERENCE_RATE - rate);
        let map = |c0: f64, c1: f64, c2: f64| {
            [
                c0 * p * p + c1 * p * q + c2 * q * q,
                2.0 * p * q * (c0 + c2) + c1 * (p * p + q * q),
                c0 * q * q + c1 * p * q + c2 * p * p,
            ]
        };
        let [n0, n1, n2] = map(b0, b1, b2);
        let [d0, d1, d2] = map(1.0, a1, a2);
        Self {
            b: [n0 / d0, n1 / d0, n2 / d0],
            a: [d1 / d0, d2 / d0],
            inputs: [0.0; 2],
            outputs: [0.0; 2],
        }
    }

    fn next(&mut self, input: f64) -> f64 {
        let output = self.b[0] * input + self.b[1] * self.inputs[0] + self.b[2] * self.inputs[1]
            - self.a[0] * self.outputs[0]
            - self.a[1] * self.outputs[1];
        self.inputs = [input, self.inputs[0]];
        self.outputs = [output, self.outputs[0]];
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_48_khz_the_stages_are_the_ones_of_the_recommendation() {
        for coefficients in [SHELF_48K, HIGH_PASS_48K] {
            let stage = Biquad::at(coefficients, REFERENCE_RATE);
            let mapped = [stage.b[0], stage.b[1], stage.b[2], stage.a[0], stage.a[1]];
            for (mapped, given) in mapped.iter().zip(coefficients) {
                assert!((mapped - given).abs() < 1e-12, "{mapped} against {given}");
            }
        }
    }

    /// The gain of the two stages in decibels at `hz`, for a filter at `rate`.
    fn response_db(rate: f64, hz: f64) -> f64 {
        let omega = std::f64::consts::TAU * hz / rate;
        let gain = |coefficients| {
            let stage = Biquad::at(coefficients, rate);
            // |B(e^-jw)| / |A(e^-jw)|
            let magnitude = |c: [f64; 3]| {
                let re = c[0] + c[1] * omega.cos() + c[2] * (2.0 * omega).cos();
                let im = -c[1] * omega.sin() - c[2] * (2.0 * omega).sin();
                re.hypot(im)
            };
            magnitude(stage.b) / magnitude([1.0, stage.a[0], stage.a[1]])
        };
        20.0 * (gain(SHELF_48K) * gain(HIGH_PASS_48K)).log10()
    }

    #[test]
    fn other_rates_keep_the_response_of_48_khz() {
        for rate in [44_100.0, 96_000.0, 192_000.0] {
            for hz in [
                20.0, 40.0, 100.0, 500.0, 1_000.0, 1_500.0, 2_000.0, 5_000.0, 10_000.0,
            ] {
                let (here, there) = (response_db(rate, hz), response_db(REFERENCE_RATE, hz));
                assert!(
                    (here - there).abs() < 0.05,
                    "{hz} Hz at {rate}: {here} dB against {there} dB"
                );
            }
        }
        // The shelf adds about 4 dB high up. Near 1 kHz the filter adds the 0.691 dB the
        // constant of the loudness takes away, and the high-pass removes the bottom.
        assert!((response_db(REFERENCE_RATE, 10_000.0) - 4.0).abs() < 0.1);
        assert!((response_db(REFERENCE_RATE, 997.0) - 0.691).abs() < 0.001);
        assert!(response_db(REFERENCE_RATE, 10.0) < -20.0);
    }

    #[test]
    fn nothing_to_measure() {
        assert_eq!(integrated(&[], 2, 48_000), None);
        assert_eq!(integrated(&[0.0; 96_000], 2, 48_000), None);
        assert_eq!(
            integrated(&[0.5; 20], 2, 48_000),
            None,
            "shorter than a block"
        );
        assert_eq!(integrated(&[0.5; 20], 0, 48_000), None);
        assert_eq!(integrated(&[0.5; 20], 2, 0), None);
    }
}
