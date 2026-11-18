//! Delay: echoes at a time in beats of the master clock, with feedback, a tone filter in the
//! loop, freeze and ping-pong. Knob 1 is the tone, knob 2 the feedback, knob 3 the time; key 1
//! freezes, key 2 makes the echoes alternate between left and right.

use platter_core::SampleRate;

use super::shared::{Line, OnePole, buffer_frames, exponential, step};
use super::{Controls, Tempo};
use crate::Frame;

/// The times of knob 3, in beats.
const TIMES: [f32; 8] = [0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];
/// Knob 3 at the middle of the step of half a beat.
pub(super) const DEFAULT_TIME: f32 = 2.5 / TIMES.len() as f32;
const MAX_FEEDBACK: f32 = 0.95;
/// A change of time larger than this fraction crossfades; a smaller one, as the tempo of the
/// clock drifts, slides the read position.
const SLIDE_FRACTION: f32 = 0.02;
/// The fastest slide of the read position, in frames per frame.
const SLIDE_SPEED: f32 = 0.5;
const CROSSFADE_MS: f32 = 50.0;

pub(super) struct Delay {
    rate: f32,
    line: Line,
    /// The distance read now, in frames.
    time: f32,
    /// The distance the crossfade leaves, and the frames left of it.
    previous: f32,
    fade_left: u32,
    fade_frames: u32,
    tone: [[OnePole; 2]; 2],
    tone_knob: f32,
    coefficients: (f32, f32),
}

impl Delay {
    pub(super) fn new(rate: SampleRate) -> Self {
        let line = Line::new(buffer_frames(rate));
        let rate = rate.as_f32();
        let mut delay = Self {
            rate,
            line,
            time: 0.0,
            previous: 0.0,
            fade_left: 0,
            fade_frames: ((rate * CROSSFADE_MS / 1_000.0) as u32).max(1),
            tone: [[OnePole::default(); 2]; 2],
            tone_knob: f32::NAN,
            coefficients: (0.0, 0.0),
        };
        delay.retune(0.0);
        delay
    }

    /// The time knob 3 points at, in frames, halved until it fits the buffer.
    fn target(&self, knob: f32, tempo: Tempo) -> f32 {
        let beats = TIMES[step(knob, TIMES.len())];
        let mut frames = beats * tempo.samples_per_beat;
        if !(frames.is_finite() && frames > 0.0) {
            frames = self.rate / 2.0;
        }
        while frames > self.line.longest() {
            frames /= 2.0;
        }
        frames.max(2.0)
    }

    fn retune(&mut self, knob: f32) {
        if knob == self.tone_knob {
            return;
        }
        self.tone_knob = knob;
        // Turning the tone narrows the loop from both ends.
        let high_pass = exponential(knob, 20.0, 1_000.0);
        let low_pass = exponential(knob, 20_000.0, 2_000.0);
        self.coefficients = (
            OnePole::coefficient(high_pass, self.rate),
            OnePole::coefficient(low_pass, self.rate),
        );
    }

    fn follow(&mut self, target: f32) {
        if self.time == 0.0 {
            self.time = target;
            return;
        }
        let difference = target - self.time;
        if difference.abs() <= self.time * SLIDE_FRACTION {
            self.time += difference.clamp(-SLIDE_SPEED, SLIDE_SPEED);
        } else if self.fade_left == 0 {
            self.previous = self.time;
            self.time = target;
            self.fade_left = self.fade_frames;
        }
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls, tempo: Tempo) -> Frame {
        let [tone, feedback, time] = controls.knobs;
        let [freeze, ping_pong] = controls.keys;
        self.retune(tone);
        let target = self.target(time, tempo);
        self.follow(target);

        let mut echo = self.line.read(self.time - 1.0);
        if self.fade_left > 0 {
            let weight = 1.0 - self.fade_left as f32 / self.fade_frames as f32;
            let old = self.line.read(self.previous - 1.0);
            echo = [
                old[0] + (echo[0] - old[0]) * weight,
                old[1] + (echo[1] - old[1]) * weight,
            ];
            self.fade_left -= 1;
        }

        let (high, low) = self.coefficients;
        let filtered = if freeze {
            echo
        } else {
            let mut out = [0.0; 2];
            for (side, sample) in echo.into_iter().enumerate() {
                let [high_pass, low_pass] = &mut self.tone[side];
                out[side] = low_pass.low(high_pass.high(sample, high), low);
            }
            out
        };
        let feedback = if freeze {
            1.0
        } else {
            feedback.clamp(0.0, 1.0) * MAX_FEEDBACK
        };
        let incoming = if freeze { [0.0; 2] } else { input };
        let written = if ping_pong {
            [
                (incoming[0] + incoming[1]) * 0.5 + feedback * filtered[1],
                feedback * filtered[0],
            ]
        } else {
            [
                incoming[0] + feedback * filtered[0],
                incoming[1] + feedback * filtered[1],
            ]
        };
        self.line.write(written);
        [input[0] + filtered[0], input[1] + filtered[1]]
    }

    pub(super) fn reset(&mut self) {
        self.line.reset();
        self.tone = [[OnePole::default(); 2]; 2];
        self.fade_left = 0;
    }
}
