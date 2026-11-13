//! Three-band equaliser with full kill.
//!
//! The signal is split into three bands with fourth-order Linkwitz-Riley crossovers (two
//! Butterworth sections in series per slope), each band is scaled, and the bands are summed.
//! With the three gains at unity the sum has a flat magnitude; with a gain at zero the band
//! is removed, not merely attenuated. The choice is recorded in
//! `docs/decisions/0005-equaliser-topology.md`.
//!
//! Crossover design: S. Linkwitz, "Active Crossover Networks for Noncoincident Drivers",
//! J. Audio Eng. Soc. 24(1), 1976 (<https://www.linkwitzlab.com/crossovers.htm>).

use platter_core::SampleRate;

use crate::Frame;
use crate::biquad::{BUTTERWORTH_Q, Biquad, Coefficients};

pub const LOW_MID_HZ: f64 = 250.0;
pub const MID_HIGH_HZ: f64 = 2_500.0;
/// Gain of a band with its knob fully clockwise.
pub const MAX_BOOST_DB: f32 = 12.0;

/// Index of each band in the gain array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Low = 0,
    Mid = 1,
    High = 2,
}

/// Maps a knob position to a band gain: `0.0` removes the band, `0.5` leaves it untouched
/// and `1.0` boosts it by [`MAX_BOOST_DB`]. Positions outside `0.0..=1.0` are clamped; a
/// position that is not a number leaves the band untouched.
pub fn band_gain(position: f32) -> f32 {
    if !position.is_finite() {
        return 1.0;
    }
    let position = position.clamp(0.0, 1.0);
    if position <= 0.5 {
        let fraction = position * 2.0;
        fraction * fraction
    } else {
        10f32.powf((position - 0.5) * 2.0 * MAX_BOOST_DB / 20.0)
    }
}

/// A fourth-order slope: two identical Butterworth sections.
#[derive(Clone, Copy)]
struct Slope([Biquad; 2]);

impl Slope {
    fn new(coefficients: Coefficients) -> Self {
        Self([Biquad::new(coefficients); 2])
    }

    fn process(&mut self, input: f32) -> f32 {
        let first = self.0[0].process(input);
        self.0[1].process(first)
    }

    fn reset(&mut self) {
        self.0.iter_mut().for_each(Biquad::reset);
    }
}

#[derive(Clone, Copy)]
struct ChannelSplit {
    low: Slope,
    above_low: Slope,
    mid: Slope,
    high: Slope,
    // The low band skips the second crossover, so it passes through the same pair of
    // slopes summed: an all-pass with the phase of that crossover. Without it the bands
    // would not add up flat.
    low_align_low: Slope,
    low_align_high: Slope,
}

impl ChannelSplit {
    fn new(rate: SampleRate) -> Self {
        let low_pass = |hz| Slope::new(Coefficients::low_pass(rate, hz, BUTTERWORTH_Q));
        let high_pass = |hz| Slope::new(Coefficients::high_pass(rate, hz, BUTTERWORTH_Q));
        Self {
            low: low_pass(LOW_MID_HZ),
            above_low: high_pass(LOW_MID_HZ),
            mid: low_pass(MID_HIGH_HZ),
            high: high_pass(MID_HIGH_HZ),
            low_align_low: low_pass(MID_HIGH_HZ),
            low_align_high: high_pass(MID_HIGH_HZ),
        }
    }

    fn process(&mut self, input: f32, gains: [f32; 3]) -> f32 {
        let low = self.low.process(input);
        let low = self.low_align_low.process(low) + self.low_align_high.process(low);
        let above_low = self.above_low.process(input);
        let mid = self.mid.process(above_low);
        let high = self.high.process(above_low);
        low * gains[0] + mid * gains[1] + high * gains[2]
    }

    fn reset(&mut self) {
        for slope in [
            &mut self.low,
            &mut self.above_low,
            &mut self.mid,
            &mut self.high,
            &mut self.low_align_low,
            &mut self.low_align_high,
        ] {
            slope.reset();
        }
    }
}

#[derive(Clone, Copy)]
pub struct ThreeBandEq {
    channels: [ChannelSplit; 2],
}

impl ThreeBandEq {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            channels: [ChannelSplit::new(rate); 2],
        }
    }

    /// Processes one frame. `gains` are linear factors for low, mid and high (see [`Band`]);
    /// the caller smooths them.
    pub fn process(&mut self, frame: Frame, gains: [f32; 3]) -> Frame {
        [
            self.channels[0].process(frame[0], gains),
            self.channels[1].process(frame[1], gains),
        ]
    }

    pub fn reset(&mut self) {
        self.channels.iter_mut().for_each(ChannelSplit::reset);
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::{db, rms};
    use platter_testkit::signal::sine;

    use super::{Band, ThreeBandEq, band_gain};

    const RATE: u32 = 48_000;

    /// Steady-state gain in decibels of a sine through the equaliser.
    fn response_db(hz: f64, gains: [f32; 3]) -> f64 {
        let mut eq = ThreeBandEq::new(SampleRate::new(RATE).unwrap());
        let input = sine(hz, RATE, 96_000, 0.5);
        let output: Vec<f32> = input
            .iter()
            .map(|x| eq.process([*x, *x], gains)[0])
            .collect();
        db(rms(&output[48_000..]).max(1e-12) / rms(&input[48_000..]))
    }

    #[test]
    fn unity_gains_give_a_flat_response() {
        for hz in [30.0, 100.0, 250.0, 700.0, 2_500.0, 6_000.0, 15_000.0] {
            let gain = response_db(hz, [1.0; 3]);
            assert!(gain.abs() < 0.05, "{hz} Hz: {gain:.3} dB");
        }
    }

    #[test]
    fn a_killed_band_is_removed() {
        let mut gains = [1.0; 3];
        gains[Band::Low as usize] = 0.0;
        assert!(response_db(40.0, gains) < -60.0);
        assert!(response_db(8_000.0, gains).abs() < 0.05);

        let mut gains = [1.0; 3];
        gains[Band::High as usize] = 0.0;
        assert!(response_db(16_000.0, gains) < -60.0);
        assert!(response_db(60.0, gains).abs() < 0.05);

        let mut gains = [1.0; 3];
        gains[Band::Mid as usize] = 0.0;
        // Halfway between the crossovers each neighbour is about 40 dB down, and the two
        // add up.
        assert!(response_db(790.0, gains) < -30.0);
    }

    #[test]
    fn all_bands_killed_is_exact_silence() {
        let mut eq = ThreeBandEq::new(SampleRate::new(RATE).unwrap());
        for x in sine(1_000.0, RATE, 4_800, 1.0) {
            assert_eq!(eq.process([x, -x], [0.0; 3]), [0.0, 0.0]);
        }
    }

    #[test]
    fn a_boosted_band_rises_by_its_gain() {
        let boost = band_gain(1.0);
        assert!((f64::from(boost) - 3.981).abs() < 0.01);
        let mut gains = [1.0; 3];
        gains[Band::Low as usize] = boost;
        assert!((response_db(40.0, gains) - 12.0).abs() < 0.2);
    }

    #[test]
    fn knob_positions_map_to_gains() {
        assert_eq!(band_gain(0.0), 0.0);
        assert_eq!(band_gain(0.5), 1.0);
        assert_eq!(band_gain(-3.0), 0.0);
        assert_eq!(band_gain(7.0), band_gain(1.0));
        assert_eq!(band_gain(f32::NAN), 1.0);
        assert!(band_gain(0.25) < band_gain(0.3));
    }

    #[test]
    fn output_stays_finite() {
        let mut eq = ThreeBandEq::new(SampleRate::new(RATE).unwrap());
        let gains = [band_gain(1.0); 3];
        for n in 0..48_000 {
            let x = if n % 2 == 0 { 1.0 } else { -1.0 };
            let [left, right] = eq.process([x, -x], gains);
            assert!(left.is_finite() && right.is_finite());
        }
    }
}
