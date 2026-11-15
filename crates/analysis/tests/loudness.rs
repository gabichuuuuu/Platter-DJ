//! Integrated loudness of synthesised signals. Behaviour note: `docs/behavior/autogain.md`.

#![allow(clippy::unwrap_used)]

use std::f64::consts::TAU;

use platter_analysis::{AnalysisError, integrated_loudness};

/// Interleaved stereo: a 997 Hz sine of `amplitude` on the left and of `right` times it on the
/// right, for `seconds`.
fn sine(rate: u32, seconds: f64, amplitude: f64, right: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(rate)) as usize;
    (0..frames)
        .flat_map(|frame| {
            let sample = amplitude * (TAU * 997.0 * frame as f64 / f64::from(rate)).sin();
            [sample as f32, (sample * right) as f32]
        })
        .collect()
}

fn lufs(samples: &[f32], rate: u32) -> f64 {
    integrated_loudness(samples, 2, rate).unwrap().unwrap()
}

fn assert_near(measured: f64, expected: f64, within: f64) {
    assert!(
        (measured - expected).abs() <= within,
        "{measured} LUFS, expected {expected}"
    );
}

#[test]
fn a_full_scale_sine_in_one_channel_reads_minus_3_lufs() {
    // The recommendation: a 0 dBFS sine at 1 kHz in the left channel reads -3.01 LKFS.
    for rate in [44_100, 48_000, 96_000] {
        assert_near(lufs(&sine(rate, 10.0, 1.0, 0.0), rate), -3.01, 0.05);
        // The same in both channels doubles the power.
        assert_near(lufs(&sine(rate, 10.0, 1.0, 1.0), rate), 0.0, 0.05);
    }
}

#[test]
fn the_loudness_follows_the_level() {
    assert_near(lufs(&sine(48_000, 10.0, 0.1, 1.0), 48_000), -20.0, 0.05);
    assert_near(lufs(&sine(48_000, 10.0, 0.01, 1.0), 48_000), -40.0, 0.05);
}

#[test]
fn silence_is_left_out() {
    let mut samples = sine(48_000, 5.0, 0.1, 1.0);
    samples.extend(vec![0.0; 2 * 48_000 * 20]);
    // The 47 blocks inside the sine are kept, and so are the three that reach into the
    // silence by a quarter, a half and three quarters: within 10 LU of the rest. Their mean
    // power is 48.5 / 50 of the sine's.
    let expected = -20.0 + 10.0 * (48.5f64 / 50.0).log10();
    assert_near(lufs(&samples, 48_000), expected, 0.01);
}

#[test]
fn a_passage_more_than_10_lu_below_is_left_out() {
    let mut samples = sine(48_000, 10.0, 0.1, 1.0);
    samples.extend(sine(48_000, 30.0, 0.01, 1.0));
    // Without the relative gate the mean of the power would read about -25.9 LUFS.
    assert_near(lufs(&samples, 48_000), -20.0, 0.3);
}

#[test]
fn a_passage_less_than_10_lu_below_is_kept() {
    let mut samples = sine(48_000, 10.0, 0.1, 1.0);
    samples.extend(sine(48_000, 10.0, 0.1 * 10f64.powf(-5.0 / 20.0), 1.0));
    let mean = 10.0 * ((0.01 + 0.01 * 10f64.powf(-0.5)) / 2.0f64).log10();
    assert_near(lufs(&samples, 48_000), mean, 0.1);
}

#[test]
fn nothing_is_found_in_silence_or_below_the_absolute_gate() {
    assert_eq!(integrated_loudness(&[0.0; 960_000], 2, 48_000), Ok(None));
    // About -80 LUFS.
    let quiet = sine(48_000, 5.0, 1e-4, 1.0);
    assert_eq!(integrated_loudness(&quiet, 2, 48_000), Ok(None));
    let short = sine(48_000, 0.3, 0.5, 1.0);
    assert_eq!(integrated_loudness(&short, 2, 48_000), Ok(None));
}

#[test]
fn unreadable_input_is_an_error() {
    assert_eq!(
        integrated_loudness(&[0.0, f32::NAN], 2, 48_000),
        Err(AnalysisError::NotFinite)
    );
    assert_eq!(
        integrated_loudness(&[0.0; 4], 0, 48_000),
        Err(AnalysisError::NoFormat)
    );
    assert_eq!(
        integrated_loudness(&[0.0; 4], 2, 0),
        Err(AnalysisError::NoFormat)
    );
    assert_eq!(
        integrated_loudness(&[], 2, 48_000),
        Err(AnalysisError::NoSamples)
    );
}
