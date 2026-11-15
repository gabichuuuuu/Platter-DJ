//! The metronome: a click on every beat of the master clock, for the headphones.
//!
//! Behaviour note: `docs/behavior/sync.md`, section *Metronome*.

use std::f32::consts::TAU;

use platter_core::SampleRate;

const HZ: f32 = 1_000.0;
/// The level of the test tone, 18 dB below full scale.
const AMPLITUDE: f32 = 0.125;
/// Shorter than a beat at the fastest tempo of the clock, 150 ms, so clicks never overlap.
const CLICK_MS: f32 = 20.0;
/// A phase that drops by more than this has crossed the start of a beat going forward; a
/// smaller drop is a jump backwards inside the beat.
const WRAP: f64 = 0.5;

pub(super) struct Metronome {
    on: bool,
    /// Phase of the clock on the previous frame.
    previous: f64,
    step: f32,
    length: u32,
    /// Frames of the click played so far; `length` when no click sounds.
    played: u32,
}

impl Metronome {
    pub(super) fn new(rate: SampleRate) -> Self {
        let length = (CLICK_MS * rate.as_f32() / 1_000.0).ceil() as u32;
        Self {
            on: false,
            previous: 0.0,
            step: HZ / rate.as_f32(),
            length,
            played: length,
        }
    }

    /// A click that sounds plays to its end, so switching off cuts nothing.
    pub(super) fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    /// The next sample, given the phase of the clock for this frame. Call it once per frame,
    /// with or without headphones, so the beats are counted all along.
    pub(super) fn next(&mut self, phase: f64) -> f32 {
        if self.on && self.previous - phase > WRAP {
            self.played = 0;
        }
        self.previous = phase;
        if self.played >= self.length {
            return 0.0;
        }
        let time = self.played as f32;
        self.played += 1;
        // Starts at the zero of the sine and falls to zero with a flat end: no step at either.
        let fall = 1.0 - time / self.length as f32;
        (time * self.step * TAU).sin() * AMPLITUDE * fall * fall
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;

    use super::Metronome;

    fn metronome() -> Metronome {
        let mut metronome = Metronome::new(SampleRate::new(48_000).unwrap());
        metronome.set_on(true);
        metronome
    }

    /// The frames, among `phases`, on which a click starts.
    fn onsets(metronome: &mut Metronome, phases: &[f64]) -> Vec<usize> {
        let mut out = Vec::new();
        for (frame, phase) in phases.iter().enumerate() {
            metronome.next(*phase);
            // A click that has just started has played its first frame.
            if metronome.played == 1 {
                out.push(frame);
            }
        }
        out
    }

    #[test]
    fn a_click_starts_where_the_phase_crosses_the_start_of_a_beat_going_forward() {
        let mut metronome = metronome();
        assert_eq!(onsets(&mut metronome, &[0.8, 0.95, 0.02, 0.1]), [2]);
    }

    #[test]
    fn a_jump_backwards_inside_the_beat_does_not_click() {
        let mut metronome = metronome();
        assert!(onsets(&mut metronome, &[0.4, 0.1, 0.3, 0.9, 0.95]).is_empty());
    }

    #[test]
    fn a_click_rises_from_zero_and_falls_to_zero() {
        let mut metronome = metronome();
        metronome.next(0.9);
        let click: Vec<f32> = (0..960).map(|_| metronome.next(0.0)).collect();
        assert_eq!(click[0], 0.0);
        assert!(click.iter().all(|sample| sample.abs() <= 0.125));
        assert!(click.iter().any(|sample| sample.abs() > 0.1));
        assert!(click[959].abs() < 1e-5);
        assert_eq!(metronome.next(0.0), 0.0, "it lasts 20 ms");
    }

    #[test]
    fn it_is_silent_while_off_and_finishes_a_click_when_switched_off() {
        let mut metronome = metronome();
        metronome.set_on(false);
        assert!(onsets(&mut metronome, &[0.9, 0.0]).is_empty());
        metronome.set_on(true);
        metronome.next(0.9);
        metronome.next(0.0);
        metronome.set_on(false);
        let rest: Vec<f32> = (0..959).map(|_| metronome.next(0.0)).collect();
        assert!(rest.iter().any(|sample| *sample != 0.0));
        assert_eq!(metronome.next(0.0), 0.0);
    }
}
