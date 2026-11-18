//! Flanger and phaser: a copy of the signal, delayed (flanger) or shifted in phase (phaser) by
//! an amount that sweeps up and down, mixed with the signal. The sweep follows the master clock,
//! or runs free. Knob 1 offsets the sweep of the right side from the left, knob 2 is the
//! feedback, knob 3 the rate; key 1 turns the direction of the sweep, key 2 frees the rate from
//! the clock.

use std::f32::consts::PI;

use platter_core::SampleRate;

use super::shared::{Line, exponential, flush, phase, step};
use super::{Controls, Tempo};
use crate::Frame;

/// The periods of knob 3 with the clock, in beats.
const PERIODS: [f64; 9] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];
/// Knob 3 at the middle of the step of four beats.
pub(super) const DEFAULT_RATE: f32 = 4.5 / PERIODS.len() as f32;
/// The free period of knob 3 at its two ends, in seconds.
const FREE_PERIOD: (f32, f32) = (30.0, 1.0 / 30.0);

/// The sweep shared by both effects: a triangle between 0 and 1.
#[derive(Default)]
struct Sweep {
    free_phase: f32,
}

impl Sweep {
    /// The position of the sweep for the left and the right side.
    fn advance(&mut self, controls: &Controls, tempo: Tempo, rate: f32) -> [f32; 2] {
        let [spread, _, knob] = controls.knobs;
        let [down, free] = controls.keys;
        let start = if free {
            let period = exponential(knob, FREE_PERIOD.0, FREE_PERIOD.1);
            self.free_phase = (self.free_phase + 1.0 / (period * rate)).fract();
            self.free_phase
        } else {
            phase(tempo.beat, PERIODS[step(knob, PERIODS.len())])
        };
        let offset = spread.clamp(0.0, 1.0) * 0.5;
        [start, (start + offset).fract()].map(|at| {
            let triangle = 1.0 - (2.0 * at - 1.0).abs();
            if down { 1.0 - triangle } else { triangle }
        })
    }
}

const FLANGE_MS: (f32, f32) = (0.3, 5.0);
const FLANGER_FEEDBACK: f32 = 0.9;

pub(super) struct Flanger {
    rate: f32,
    line: Line,
    sweep: Sweep,
}

impl Flanger {
    pub(super) fn new(rate: SampleRate) -> Self {
        let rate = rate.as_f32();
        Self {
            rate,
            line: Line::new((rate * FLANGE_MS.1 * 2.0 / 1_000.0) as usize + 4),
            sweep: Sweep::default(),
        }
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls, tempo: Tempo) -> Frame {
        let positions = self.sweep.advance(controls, tempo, self.rate);
        let feedback = controls.knobs[1].clamp(0.0, 1.0) * FLANGER_FEEDBACK;
        let frames = |position: f32| {
            (FLANGE_MS.0 + (FLANGE_MS.1 - FLANGE_MS.0) * position) * self.rate / 1_000.0
        };
        let left = self.line.read(frames(positions[0]))[0];
        let right = self.line.read(frames(positions[1]))[1];
        let delayed = [left, right];
        // The input share falls as the feedback rises, so the peaks stay near unity.
        self.line.write([
            (1.0 - feedback) * input[0] + feedback * delayed[0],
            (1.0 - feedback) * input[1] + feedback * delayed[1],
        ]);
        [0.5 * (input[0] + delayed[0]), 0.5 * (input[1] + delayed[1])]
    }

    pub(super) fn reset(&mut self) {
        self.line.reset();
        self.sweep = Sweep::default();
    }
}

const STAGES: usize = 6;
const PHASE_HZ: (f32, f32) = (200.0, 4_000.0);
const PHASER_FEEDBACK: f32 = 0.7;

pub(super) struct Phaser {
    rate: f32,
    sweep: Sweep,
    /// The state of each first-order all-pass stage, per side.
    stages: [[f32; STAGES]; 2],
    last: [f32; 2],
}

impl Phaser {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            rate: rate.as_f32(),
            sweep: Sweep::default(),
            stages: [[0.0; STAGES]; 2],
            last: [0.0; 2],
        }
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls, tempo: Tempo) -> Frame {
        let positions = self.sweep.advance(controls, tempo, self.rate);
        let feedback = controls.knobs[1].clamp(0.0, 1.0) * PHASER_FEEDBACK;
        let mut output = [0.0; 2];
        for side in 0..2 {
            let hz = exponential(positions[side], PHASE_HZ.0, PHASE_HZ.1);
            let t = (PI * hz / self.rate).tan();
            let a = (t - 1.0) / (t + 1.0);
            let mut signal = input[side] + feedback * self.last[side];
            for state in &mut self.stages[side] {
                let out = a * signal + *state;
                *state = flush(signal - a * out);
                signal = out;
            }
            self.last[side] = flush(signal);
            output[side] = 0.5 * (input[side] + signal);
        }
        output
    }

    pub(super) fn reset(&mut self) {
        self.stages = [[0.0; STAGES]; 2];
        self.last = [0.0; 2];
        self.sweep = Sweep::default();
    }
}
