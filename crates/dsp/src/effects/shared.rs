//! Small pieces the effects share: knob mappings, one-pole and state-variable filters, a
//! delay line and a noise source.

use std::f32::consts::PI;

use platter_core::SampleRate;

use super::MAX_BUFFER_SECONDS;

/// The index among `count` equal steps that a knob between 0 and 1 points at.
pub(super) fn step(knob: f32, count: usize) -> usize {
    if count == 0 || !knob.is_finite() {
        return 0;
    }
    let index = (knob.clamp(0.0, 1.0) * count as f32) as usize;
    index.min(count - 1)
}

/// A knob between 0 and 1 mapped to `low..high` along a logarithmic travel, as the ear hears
/// frequencies and times.
pub(super) fn exponential(knob: f32, low: f32, high: f32) -> f32 {
    let knob = if knob.is_finite() {
        knob.clamp(0.0, 1.0)
    } else {
        0.0
    };
    low * (high / low).powf(knob)
}

/// Very small values become zero, so a decaying tail does not slow the processor down with
/// subnormal numbers.
pub(super) fn flush(value: f32) -> f32 {
    if value.abs() < 1e-20 { 0.0 } else { value }
}

/// The number of frames a buffer of [`MAX_BUFFER_SECONDS`] holds at `rate`.
pub(super) fn buffer_frames(rate: SampleRate) -> usize {
    (rate.as_f32() * MAX_BUFFER_SECONDS) as usize + 1
}

/// The fraction of the way from `beat` to the next multiple of `period` beats, from 0 to 1.
pub(super) fn phase(beat: f64, period: f64) -> f32 {
    if !(period.is_finite() && period > 0.0 && beat.is_finite()) {
        return 0.0;
    }
    (beat / period).rem_euclid(1.0) as f32
}

/// A one-pole low-pass filter.
#[derive(Clone, Copy, Default)]
pub(super) struct OnePole {
    state: f32,
}

impl OnePole {
    /// The coefficient of a corner at `hz`.
    pub(super) fn coefficient(hz: f32, rate: f32) -> f32 {
        let hz = hz.clamp(1.0, rate * 0.49);
        1.0 - (-2.0 * PI * hz / rate).exp()
    }

    pub(super) fn low(&mut self, input: f32, coefficient: f32) -> f32 {
        self.state = flush(self.state + coefficient * (input - self.state));
        self.state
    }

    pub(super) fn high(&mut self, input: f32, coefficient: f32) -> f32 {
        input - self.low(input, coefficient)
    }

    pub(super) fn reset(&mut self) {
        self.state = 0.0;
    }
}

/// The outputs of a state-variable filter for one sample.
pub(super) struct Bands {
    pub(super) low: f32,
    pub(super) high: f32,
}

/// A state-variable filter in its trapezoidal form, which stays stable while its corner moves
/// every sample. Source: A. Simper, "Linear Trapezoidal Integrated SVF"
/// (<https://cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf>).
#[derive(Clone, Copy, Default)]
pub(super) struct StateVariable {
    ic1: f32,
    ic2: f32,
}

/// The coefficients of [`StateVariable`] for a corner and a quality factor.
#[derive(Clone, Copy)]
pub(super) struct Tuning {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Tuning {
    pub(super) fn new(hz: f32, q: f32, rate: f32) -> Self {
        let hz = hz.clamp(10.0, rate * 0.45);
        let g = (PI * hz / rate).tan();
        let k = 1.0 / q.max(0.1);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { a1, a2, a3, k }
    }
}

impl StateVariable {
    pub(super) fn process(&mut self, input: f32, tuning: &Tuning) -> Bands {
        let v3 = input - self.ic2;
        let v1 = tuning.a1 * self.ic1 + tuning.a2 * v3;
        let v2 = self.ic2 + tuning.a2 * self.ic1 + tuning.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        Bands {
            low: v2,
            high: input - tuning.k * v1 - v2,
        }
    }
}

/// A stereo delay line read at a fractional distance behind the write position.
pub(super) struct Line {
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
}

impl Line {
    pub(super) fn new(frames: usize) -> Self {
        let frames = frames.max(4);
        Self {
            left: vec![0.0; frames],
            right: vec![0.0; frames],
            write: 0,
        }
    }

    /// The longest distance that can be read, in frames.
    pub(super) fn longest(&self) -> f32 {
        (self.left.len() - 2) as f32
    }

    /// The frame `distance` frames behind the last one written, between neighbours.
    pub(super) fn read(&self, distance: f32) -> [f32; 2] {
        let len = self.left.len();
        let distance = if distance.is_finite() {
            distance.clamp(1.0, self.longest())
        } else {
            1.0
        };
        let whole = distance as usize;
        let fraction = distance - whole as f32;
        let at = |offset: usize| (self.write + len - offset) % len;
        let (near, far) = (at(whole), at(whole + 1));
        [
            self.left[near] + (self.left[far] - self.left[near]) * fraction,
            self.right[near] + (self.right[far] - self.right[near]) * fraction,
        ]
    }

    pub(super) fn write(&mut self, frame: [f32; 2]) {
        self.write = (self.write + 1) % self.left.len();
        self.left[self.write] = flush(frame[0]);
        self.right[self.write] = flush(frame[1]);
    }

    pub(super) fn reset(&mut self) {
        self.left.fill(0.0);
        self.right.fill(0.0);
    }
}

/// White noise from a 32-bit xorshift generator: the same sequence on every run, so the tests
/// can repeat it.
#[derive(Clone, Copy)]
pub(super) struct Noise {
    state: u32,
}

impl Default for Noise {
    fn default() -> Self {
        Self { state: 0x9e37_79b9 }
    }
}

impl Noise {
    /// The next value, between -1 and 1.
    pub(super) fn next(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::{exponential, phase, step};

    #[test]
    fn a_knob_points_at_equal_steps() {
        assert_eq!(step(0.0, 8), 0);
        assert_eq!(step(0.124, 8), 0);
        assert_eq!(step(0.126, 8), 1);
        assert_eq!(step(1.0, 8), 7);
        assert_eq!(step(f32::NAN, 8), 0);
    }

    #[test]
    fn the_exponential_travel_reaches_both_ends() {
        assert!((exponential(0.0, 20.0, 20_000.0) - 20.0).abs() < 1e-3);
        assert!((exponential(1.0, 20.0, 20_000.0) - 20_000.0).abs() < 1.0);
        assert!((exponential(0.5, 20.0, 20_000.0) - 632.5).abs() < 1.0);
    }

    #[test]
    fn the_phase_wraps_at_each_period() {
        assert_eq!(phase(0.0, 1.0), 0.0);
        assert!((phase(2.5, 1.0) - 0.5).abs() < 1e-6);
        assert!((phase(-0.25, 1.0) - 0.75).abs() < 1e-6);
        assert_eq!(phase(1.0, 0.0), 0.0);
    }
}
