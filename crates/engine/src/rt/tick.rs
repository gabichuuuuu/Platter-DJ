//! Beat tick: a short decaying sine on each beat of a deck, played in the headphones.
//!
//! Behaviour note: `docs/behavior/beatgrid.md`, Beat tick.

use std::f32::consts::TAU;

use platter_core::SampleRate;

const HZ: f32 = 1_500.0;
/// 12 dB below full scale at the start.
const AMPLITUDE: f32 = 0.25;
const SECONDS: f32 = 0.03;
/// The decay over the length of the tick, as a power of e: it ends 43 dB below its start, so
/// cutting it there is not heard.
const DECAY: f32 = 5.0;

pub(super) struct Tick {
    /// Position in the cycle, `0.0..1.0`.
    phase: f32,
    step: f32,
    gain: f32,
    /// What the gain is multiplied by on each frame.
    fall: f32,
    remaining: u32,
    length: u32,
}

impl Tick {
    pub(super) fn new(rate: SampleRate) -> Self {
        let length = (SECONDS * rate.as_f32()).ceil();
        Self {
            phase: 0.0,
            step: HZ / rate.as_f32(),
            gain: 0.0,
            fall: (-DECAY / length.max(1.0)).exp(),
            remaining: 0,
            length: length as u32,
        }
    }

    /// Starts the tick from the beginning; a tick still sounding starts over.
    pub(super) fn play(&mut self) {
        self.phase = 0.0;
        self.gain = AMPLITUDE;
        self.remaining = self.length;
    }

    /// The next sample of the tick: silence once it has ended.
    pub(super) fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return 0.0;
        }
        self.remaining -= 1;
        // The sine starts at zero, so the tick starts without a step.
        let sample = (self.phase * TAU).sin() * self.gain;
        self.phase = (self.phase + self.step).fract();
        self.gain *= self.fall;
        sample
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_is_short_starts_at_zero_and_decays() {
        let rate = SampleRate::new(48_000).unwrap();
        let mut tick = Tick::new(rate);
        assert_eq!(tick.next(), 0.0, "silent until played");
        tick.play();
        let samples: Vec<f32> = (0..2_000).map(|_| tick.next()).collect();
        assert_eq!(samples[0], 0.0);
        let peak = |range: std::ops::Range<usize>| {
            samples[range]
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
        };
        assert!(peak(0..100) > 0.2 && peak(0..100) <= AMPLITUDE);
        assert!(peak(1_300..1_440) < 0.005, "it has decayed by its end");
        assert!(
            samples[1_440..].iter().all(|sample| *sample == 0.0),
            "30 ms long"
        );
    }
}
