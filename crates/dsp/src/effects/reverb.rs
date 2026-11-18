//! Reverb: parallel comb filters with a low-pass in each loop, followed by all-pass filters in
//! series, after M. R. Schroeder, "Natural sounding artificial reverberation", J. Audio Eng.
//! Soc. 10 (3), 1962, and J. A. Moorer, "About this reverberation business", Computer Music
//! Journal 3 (2), 1979. The lengths are Platter's own, spread between 25 and 37 ms so their
//! echoes do not line up; the right side is a little longer than the left for width.
//!
//! Knob 1 cuts the lows of the reverb, knob 2 its highs, knob 3 is the size; key 1 freezes it.

use platter_core::SampleRate;

use super::Controls;
use super::shared::{OnePole, exponential, flush};
use crate::Frame;

const COMB_MS: [f32; 8] = [25.31, 26.93, 28.96, 30.75, 32.24, 33.81, 35.31, 36.67];
const ALL_PASS_MS: [f32; 4] = [12.6, 10.0, 7.73, 5.1];
const RIGHT_OFFSET_MS: f32 = 0.52;
const ALL_PASS_GAIN: f32 = 0.5;
const DAMPING: f32 = 0.25;
const INPUT_GAIN: f32 = 0.5;
const WET_GAIN: f32 = 0.12;
/// The feedback of the combs at the smallest and the largest size.
const FEEDBACK: (f32, f32) = (0.7, 0.98);

struct Comb {
    buffer: Vec<f32>,
    at: usize,
    damped: f32,
}

impl Comb {
    fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.max(1)],
            at: 0,
            damped: 0.0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let output = self.buffer[self.at];
        self.damped = flush(output * (1.0 - damping) + self.damped * damping);
        self.buffer[self.at] = flush(input + self.damped * feedback);
        self.at = (self.at + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.damped = 0.0;
    }
}

struct AllPass {
    buffer: Vec<f32>,
    at: usize,
}

impl AllPass {
    fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.max(1)],
            at: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.at];
        let output = delayed - input * ALL_PASS_GAIN;
        self.buffer[self.at] = flush(input + delayed * ALL_PASS_GAIN);
        self.at = (self.at + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
    }
}

struct Side {
    combs: Vec<Comb>,
    all_passes: Vec<AllPass>,
    low_cut: OnePole,
    high_cut: OnePole,
}

impl Side {
    fn new(rate: f32, offset_ms: f32) -> Self {
        let frames = |ms: f32| ((ms + offset_ms) * rate / 1_000.0) as usize;
        Self {
            combs: COMB_MS.iter().map(|ms| Comb::new(frames(*ms))).collect(),
            all_passes: ALL_PASS_MS
                .iter()
                .map(|ms| AllPass::new(frames(*ms)))
                .collect(),
            low_cut: OnePole::default(),
            high_cut: OnePole::default(),
        }
    }

    fn reset(&mut self) {
        self.combs.iter_mut().for_each(Comb::reset);
        self.all_passes.iter_mut().for_each(AllPass::reset);
        self.low_cut.reset();
        self.high_cut.reset();
    }
}

pub(super) struct Reverb {
    rate: f32,
    sides: [Side; 2],
    cuts: [f32; 2],
    coefficients: (f32, f32),
}

impl Reverb {
    pub(super) fn new(rate: SampleRate) -> Self {
        let rate = rate.as_f32();
        let mut reverb = Self {
            rate,
            sides: [Side::new(rate, 0.0), Side::new(rate, RIGHT_OFFSET_MS)],
            cuts: [f32::NAN; 2],
            coefficients: (0.0, 0.0),
        };
        reverb.retune([0.0, 0.0]);
        reverb
    }

    fn retune(&mut self, cuts: [f32; 2]) {
        if cuts == self.cuts {
            return;
        }
        self.cuts = cuts;
        self.coefficients = (
            OnePole::coefficient(exponential(cuts[0], 20.0, 1_000.0), self.rate),
            OnePole::coefficient(exponential(cuts[1], 20_000.0, 1_500.0), self.rate),
        );
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls) -> Frame {
        let [low_cut, high_cut, size] = controls.knobs;
        let freeze = controls.keys[0];
        self.retune([low_cut, high_cut]);
        let (feedback, damping, gain) = if freeze {
            (1.0, 0.0, 0.0)
        } else {
            let size = size.clamp(0.0, 1.0);
            (
                FEEDBACK.0 + (FEEDBACK.1 - FEEDBACK.0) * size,
                DAMPING,
                INPUT_GAIN,
            )
        };
        let mono = (input[0] + input[1]) * 0.5 * gain;
        let (low, high) = self.coefficients;
        let mut output = input;
        for (side, sample) in self.sides.iter_mut().zip(output.iter_mut()) {
            let mut wet: f32 = side
                .combs
                .iter_mut()
                .map(|comb| comb.process(mono, feedback, damping))
                .sum();
            for all_pass in &mut side.all_passes {
                wet = all_pass.process(wet);
            }
            let wet = side.high_cut.low(side.low_cut.high(wet, low), high);
            *sample += wet * WET_GAIN;
        }
        output
    }

    pub(super) fn reset(&mut self) {
        self.sides.iter_mut().for_each(Side::reset);
    }
}
