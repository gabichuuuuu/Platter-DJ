//! Gater: opens the signal on every step of the master clock and lets it decay until the next.
//! Knob 1 adds noise through the same gate, knob 2 is the shape, from a short click to a long
//! decay, knob 3 the rate; key 1 mutes the music and leaves the noise, key 2 stutters with a
//! gate of three quarters of a beat.

use platter_core::SampleRate;

use super::shared::{Noise, exponential, phase, step};
use super::{Controls, Tempo};
use crate::Frame;

/// The periods of knob 3, in beats.
const PERIODS: [f64; 4] = [1.0, 0.5, 0.25, 0.125];
/// Knob 3 at the middle of the step of a quarter of a beat.
pub(super) const DEFAULT_RATE: f32 = 2.5 / PERIODS.len() as f32;
const STUTTER: f64 = 0.75;
/// How fast the gate decays over one period, at the shortest and the longest shape.
const DECAY: (f32, f32) = (40.0, 1.5);
const NOISE_LEVEL: f32 = 0.3;
/// The gate opens over this time, so its edge does not click.
const ATTACK_MS: f32 = 1.0;

pub(super) struct Gater {
    attack_frames: f32,
    noise: Noise,
}

impl Gater {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            attack_frames: (rate.as_f32() * ATTACK_MS / 1_000.0).max(1.0),
            noise: Noise::default(),
        }
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls, tempo: Tempo) -> Frame {
        let [noise, shape, rate] = controls.knobs;
        let [mute, stutter] = controls.keys;
        let period = if stutter {
            STUTTER
        } else {
            PERIODS[step(rate, PERIODS.len())]
        };
        let at = phase(tempo.beat, period);
        let since = at * period as f32 * tempo.samples_per_beat;
        let attack = (since / self.attack_frames).min(1.0);
        let gate = attack * (-at * exponential(shape, DECAY.0, DECAY.1)).exp();
        let music = if mute { 0.0 } else { gate };
        let hiss = noise.clamp(0.0, 1.0) * NOISE_LEVEL * gate;
        [
            input[0] * music + hiss * self.noise.next(),
            input[1] * music + hiss * self.noise.next(),
        ]
    }

    pub(super) fn reset(&mut self) {
        self.noise = Noise::default();
    }
}
