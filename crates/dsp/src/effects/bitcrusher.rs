//! Bitcrusher: fewer bits and a lower sample rate. Knob 1 lowers the bit depth from 16 bits to
//! 1, knob 2 the sample rate from the stream rate to 100 Hz, knob 3 offsets the samples of the
//! right side from the left; key 1 smooths the steps of the lowered rate.

use platter_core::SampleRate;

use super::Controls;
use super::shared::{OnePole, exponential};
use crate::Frame;

const BITS: (f32, f32) = (16.0, 1.0);
const LOWEST_HZ: f32 = 100.0;

pub(super) struct Bitcrusher {
    rate: f32,
    /// How far each side is into its held sample, from 0 to 1.
    hold: [f32; 2],
    held: [f32; 2],
    smooth: [OnePole; 2],
}

impl Bitcrusher {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            rate: rate.as_f32(),
            hold: [1.0; 2],
            held: [0.0; 2],
            smooth: [OnePole::default(); 2],
        }
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls) -> Frame {
        let [bits, rate, spread] = controls.knobs.map(|knob| {
            if knob.is_finite() {
                knob.clamp(0.0, 1.0)
            } else {
                0.0
            }
        });
        let smooth = controls.keys[0];
        let depth = BITS.0 + (BITS.1 - BITS.0) * bits;
        let quantum = 2.0 / depth.exp2();
        let hz = exponential(rate, self.rate, LOWEST_HZ);
        let step = hz / self.rate;
        let coefficient = OnePole::coefficient(hz * 0.5, self.rate);
        let mut output = [0.0; 2];
        for side in 0..2 {
            self.hold[side] += step;
            if self.hold[side] >= 1.0 {
                self.hold[side] = self.hold[side].fract();
                self.held[side] = (input[side] / quantum).round() * quantum;
            }
            output[side] = if smooth {
                self.smooth[side].low(self.held[side], coefficient)
            } else {
                self.held[side]
            };
        }
        // The offset moves the right side through its hold by up to half a period.
        let target = (self.hold[0] + spread * 0.5).fract();
        if (self.hold[1] - target).abs() > step {
            self.hold[1] = target;
        }
        output
    }

    pub(super) fn reset(&mut self) {
        self.hold = [1.0; 2];
        self.held = [0.0; 2];
        self.smooth = [OnePole::default(); 2];
    }
}
