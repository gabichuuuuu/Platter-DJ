//! One-knob channel filter: low-pass when turned one way, high-pass the other, and no
//! effect at the centre.
//!
//! The filter is a state-variable filter in its trapezoidal form, which stays stable while
//! its corner moves every sample. Source: A. Simper, "Linear Trapezoidal Integrated SVF"
//! (<https://cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf>). The mapping of the knob is
//! recorded in `docs/decisions/0006-channel-filter.md`.

use std::f32::consts::PI;

use platter_core::SampleRate;

use crate::Frame;

/// Half-width of the zone around the centre where the filter fades in.
pub const BLEND_ZONE: f32 = 0.05;
const RESONANCE_Q: f32 = 0.9;
const LOW_PASS_RANGE_HZ: (f32, f32) = (20_000.0, 60.0);
const HIGH_PASS_RANGE_HZ: (f32, f32) = (20.0, 10_000.0);

#[derive(Clone, Copy, Default)]
struct State {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy)]
pub struct OneKnobFilter {
    rate: f32,
    position: f32,
    // Coefficients of the trapezoidal form, kept between samples while the knob rests.
    a1: f32,
    a2: f32,
    a3: f32,
    high_pass: bool,
    wet: f32,
    channels: [State; 2],
}

impl OneKnobFilter {
    pub fn new(rate: SampleRate) -> Self {
        let mut filter = Self {
            rate: rate.as_f32(),
            position: f32::NAN,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            high_pass: false,
            wet: 0.0,
            channels: [State::default(); 2],
        };
        filter.retune(0.0);
        filter
    }

    /// Processes one frame with the knob at `position`: `-1.0` is the lowest low-pass,
    /// `0.0` leaves the signal untouched and `1.0` is the highest high-pass. The caller
    /// smooths the position. A position that is not a number is treated as the centre.
    pub fn process(&mut self, frame: Frame, position: f32) -> Frame {
        let position = if position.is_finite() {
            position.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        if position != self.position {
            self.retune(position);
        }
        let mut output = frame;
        for (sample, state) in output.iter_mut().zip(&mut self.channels) {
            let v3 = *sample - state.ic2;
            let v1 = self.a1 * state.ic1 + self.a2 * v3;
            let v2 = state.ic2 + self.a2 * state.ic1 + self.a3 * v3;
            state.ic1 = flush(2.0 * v1 - state.ic1);
            state.ic2 = flush(2.0 * v2 - state.ic2);
            let filtered = if self.high_pass {
                *sample - v1 / RESONANCE_Q - v2
            } else {
                v2
            };
            *sample += self.wet * (filtered - *sample);
        }
        output
    }

    pub fn reset(&mut self) {
        self.channels = [State::default(); 2];
    }

    fn retune(&mut self, position: f32) {
        self.position = position;
        self.high_pass = position > 0.0;
        let travel = position.abs();
        self.wet = (travel / BLEND_ZONE).min(1.0);
        let (from, to) = if self.high_pass {
            HIGH_PASS_RANGE_HZ
        } else {
            LOW_PASS_RANGE_HZ
        };
        // Exponential sweep: equal knob travel moves the corner by equal musical intervals.
        let hz = (from * (to / from).powf(travel)).clamp(10.0, 0.45 * self.rate);
        let g = (PI * hz / self.rate).tan();
        let k = 1.0 / RESONANCE_Q;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
}

/// Keeps a decaying state out of the denormal range, where arithmetic becomes slow.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-25 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use platter_testkit::measure::{db, max_step, rms};
    use platter_testkit::signal::sine;

    use super::OneKnobFilter;

    const RATE: u32 = 48_000;

    fn response_db(hz: f64, position: f32) -> f64 {
        let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
        let input = sine(hz, RATE, 48_000, 0.5);
        let output: Vec<f32> = input
            .iter()
            .map(|x| filter.process([*x, *x], position)[1])
            .collect();
        db(rms(&output[24_000..]).max(1e-12) / rms(&input[24_000..]))
    }

    #[test]
    fn the_centre_leaves_the_signal_untouched() {
        let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
        for x in sine(3_000.0, RATE, 4_800, 0.8) {
            assert_eq!(filter.process([x, -x], 0.0), [x, -x]);
        }
    }

    #[test]
    fn turning_left_is_a_low_pass() {
        assert!(response_db(100.0, -0.5).abs() < 1.0);
        assert!(response_db(10_000.0, -0.5) < -30.0);
        // Fully left the corner sits at 60 Hz.
        assert!(response_db(1_000.0, -1.0) < -40.0);
    }

    #[test]
    fn turning_right_is_a_high_pass() {
        assert!(response_db(10_000.0, 0.5).abs() < 1.0);
        assert!(response_db(40.0, 0.5) < -30.0);
        assert!(response_db(500.0, 1.0) < -40.0);
    }

    #[test]
    fn sweeping_the_knob_neither_clicks_nor_blows_up() {
        let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
        let input = sine(200.0, RATE, 96_000, 0.5);
        let output: Vec<f32> = input
            .iter()
            .enumerate()
            .map(|(n, x)| {
                let position = (n as f32 / 96_000.0) * 2.0 - 1.0;
                filter.process([*x, *x], position)[0]
            })
            .collect();
        assert!(output.iter().all(|y| y.is_finite() && y.abs() < 1.0));
        assert!(max_step(&output) < 4.0 * max_step(&input));
    }

    #[test]
    fn positions_that_are_not_numbers_act_as_the_centre() {
        let mut filter = OneKnobFilter::new(SampleRate::new(RATE).unwrap());
        assert_eq!(filter.process([0.25, 0.5], f32::NAN), [0.25, 0.5]);
        assert_eq!(filter.process([0.25, 0.5], f32::INFINITY), [0.25, 0.5]);
    }
}
