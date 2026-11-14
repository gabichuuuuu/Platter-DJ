//! Test tone: a short sine sent to one output, to check the routing of the audio device.

use std::f32::consts::TAU;

use platter_core::SampleRate;
use platter_dsp::smooth::Smoothed;

use super::Output;

const HZ: f32 = 440.0;
/// 18 dB below full scale: clearly audible, and far from the limiter.
const AMPLITUDE: f32 = 0.125;
const SECONDS: f32 = 1.0;
const FADE_MS: f32 = 10.0;

pub(super) struct TestTone {
    output: Output,
    /// Position in the cycle, `0.0..1.0`.
    phase: f32,
    step: f32,
    remaining: u32,
    length: u32,
    fade_samples: u32,
    envelope: Smoothed,
}

impl TestTone {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            output: Output::Master,
            phase: 0.0,
            step: HZ / rate.as_f32(),
            remaining: 0,
            length: (SECONDS * rate.as_f32()) as u32,
            fade_samples: (FADE_MS * rate.as_f32() / 1_000.0).ceil() as u32,
            envelope: Smoothed::new(0.0, rate, FADE_MS),
        }
    }

    /// Starts the tone on `output`. A tone that is still sounding starts over there.
    pub(super) fn play(&mut self, output: Output) {
        self.output = output;
        self.remaining = self.length;
    }

    /// The next sample of the tone for `output`: silence unless the tone is playing there.
    /// Call it once per frame for each output.
    pub(super) fn next(&mut self, output: Output) -> f32 {
        if output != self.output {
            return 0.0;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
        }
        self.envelope.set(if self.remaining > self.fade_samples {
            1.0
        } else {
            0.0
        });
        let gain = self.envelope.advance();
        if gain == 0.0 {
            self.phase = 0.0;
            return 0.0;
        }
        let sample = (self.phase * TAU).sin() * AMPLITUDE * gain;
        self.phase = (self.phase + self.step).fract();
        sample
    }
}
