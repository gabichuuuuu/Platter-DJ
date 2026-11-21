//! What the sender of the MIDI clock needs from the engine: the messages the processor finds,
//! the frame each block began at, and a loop that turns frames into time.
//!
//! Behaviour note: `docs/behavior/midi-clock.md`; design: `docs/decisions/0041-midi-clock.md`.
//! The thread that sends lives in the application: this module holds no thread and no port.

use std::f64::consts::{PI, SQRT_2};
use std::sync::Arc;

use rtrb::Consumer;

use crate::rt::Shared;
pub use crate::rt::{ClockEvent, ClockMessage, TICKS_PER_BEAT};

/// Bandwidth of the loop, in hertz: slow enough to smooth the lateness of the callbacks, fast
/// enough to follow the clock of the device within a few seconds.
const BANDWIDTH_HZ: f64 = 1.0;
/// An error larger than this, in seconds, is a new stream or a stall, not jitter: the loop
/// starts again from that observation.
const RESET_SECONDS: f64 = 0.05;

/// The reading end of the queue of the MIDI clock and the frame count the processor publishes.
pub struct ClockFeed {
    events: Consumer<ClockEvent>,
    shared: Arc<Shared>,
}

impl ClockFeed {
    pub(crate) fn new(events: Consumer<ClockEvent>, shared: Arc<Shared>) -> Self {
        Self { events, shared }
    }

    /// The next message the processor found, oldest first.
    pub fn pop(&mut self) -> Option<ClockEvent> {
        self.events.pop().ok()
    }

    /// The frames the processor had rendered when its latest block began.
    pub fn block_frame(&self) -> u64 {
        self.shared.block_frame.get()
    }
}

/// A second-order delay-locked loop that maps frames of the output to a time of the system, in
/// seconds: F. Adriaensen, *Using a DLL to filter time*, sections 2 to 4. Each observation is
/// the frame a block began at and the time it was seen; the loop keeps a line through them,
/// steady against the lateness of each one.
#[derive(Clone, Debug)]
pub struct FrameClock {
    rate: f64,
    /// The frame and the time of the latest point of the line.
    frame: u64,
    time: f64,
    /// Seconds one frame lasts, as the loop finds it.
    period: f64,
    started: bool,
}

impl FrameClock {
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate.max(1));
        Self {
            rate,
            frame: 0,
            time: 0.0,
            period: 1.0 / rate,
            started: false,
        }
    }

    /// A block began at `frame`, seen at `time` seconds. An observation that goes back, or
    /// lies far off the line, starts the loop again from there.
    pub fn observe(&mut self, frame: u64, time: f64) {
        if !time.is_finite() {
            return;
        }
        let Some(frames) = frame.checked_sub(self.frame).filter(|_| self.started) else {
            self.start(frame, time);
            return;
        };
        if frames == 0 {
            return;
        }
        let frames = frames as f64;
        let predicted = self.time + frames * self.period;
        let error = time - predicted;
        if error.abs() > RESET_SECONDS {
            self.start(frame, time);
            return;
        }
        // The coefficients of a critically damped loop run once every `frames` frames.
        let omega = 2.0 * PI * BANDWIDTH_HZ * frames / self.rate;
        let b = SQRT_2 * omega;
        let c = omega * omega;
        self.frame = frame;
        self.time = predicted + b * error;
        self.period += c * error / frames;
    }

    /// The time of `frame` on the line, once one block has been seen.
    pub fn time_of(&self, frame: u64) -> Option<f64> {
        self.started
            .then_some(self.time + (frame as f64 - self.frame as f64) * self.period)
    }

    fn start(&mut self, frame: u64, time: f64) {
        self.frame = frame;
        self.time = time;
        self.period = 1.0 / self.rate;
        self.started = true;
    }
}

#[cfg(test)]
mod tests {
    use super::FrameClock;

    /// A pseudo-random lateness from zero up to `most`, the same on every run.
    fn lateness(index: u64, most: f64) -> f64 {
        let mixed = index
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (mixed >> 11) as f64 / (1u64 << 53) as f64 * most
    }

    #[test]
    fn the_line_is_steady_against_callbacks_that_start_late() {
        // 256 frames at 48 kHz, a device clock 0.01 % fast, callbacks up to a quarter of a
        // buffer late.
        let rate = 48_000;
        let block = 256u64;
        let true_period = 1.0 / (f64::from(rate) * 1.0001);
        let most = block as f64 * true_period / 4.0;
        let mut clock = FrameClock::new(rate);
        let mut errors = Vec::new();
        for index in 0..20 * 48_000 / block {
            let frame = index * block;
            let truth = frame as f64 * true_period;
            clock.observe(frame, truth + lateness(index, most));
            // After ten seconds, where the line puts the frame half a block on.
            if index > 10 * 48_000 / block {
                let probe = frame + block / 2;
                let predicted = clock.time_of(probe).unwrap_or(f64::NAN);
                errors.push(predicted - probe as f64 * true_period);
            }
        }
        let low = errors.iter().copied().fold(f64::INFINITY, f64::min);
        let high = errors.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // The line sits at the mean lateness; what matters is that it does not wobble.
        assert!(high - low < 0.000_25, "spread {} s", high - low);
        assert!(low > -0.000_1 && high < most + 0.000_1, "{low} {high}");
    }

    #[test]
    fn a_new_stream_starts_the_line_again() {
        let mut clock = FrameClock::new(48_000);
        assert_eq!(clock.time_of(0), None);
        clock.observe(48_000, 1.0);
        clock.observe(96_000, 2.0);
        // The frames start again from zero, later.
        clock.observe(0, 10.0);
        let at = clock.time_of(48_000).unwrap_or(f64::NAN);
        assert!((at - 11.0).abs() < 1e-9, "{at}");
        // A stall of a second is not jitter either.
        clock.observe(96_000, 13.0);
        assert!((clock.time_of(96_000).unwrap_or(f64::NAN) - 13.0).abs() < 1e-9);
    }
}
