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

/// Bandwidth of the loop that reads a received clock, in hertz: it takes a step of tempo in
/// about a beat at the tempos DJs play.
const FOLLOW_BANDWIDTH_HZ: f64 = 2.0;
/// A tick later than this after the one before means the clock had stopped arriving.
const SILENCE_SECONDS: f64 = 0.5;
/// Ticks the follower waits for before it gives a tempo: one beat.
const TICKS_TO_TRUST: u64 = TICKS_PER_BEAT as u64;
/// Tempos a received clock may have: those of the master clock.
const FOLLOW_BPM: std::ops::RangeInclusive<f64> = 20.0..=400.0;

/// Reads a MIDI clock that arrives: its tempo and where it is in its beat, from the times its
/// messages are received. The same loop as [`FrameClock`] runs over the ticks, so the jitter of
/// the sender and of the system is smoothed: `docs/decisions/0041-midi-clock.md`.
#[derive(Clone, Debug, Default)]
pub struct ClockFollower {
    /// Ticks since Start, and the time the loop puts the latest one at.
    count: u64,
    time: f64,
    /// Seconds between two ticks, as the loop finds it; zero until two ticks came.
    period: f64,
    /// When the latest tick was received.
    last: Option<f64>,
    /// Start or Continue came and Stop has not.
    running: bool,
    /// The next tick puts the loop where it arrives, keeping the period: after Continue.
    reseat: bool,
}

impl ClockFollower {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one message received at `time` seconds.
    pub fn take(&mut self, time: f64, message: ClockMessage) {
        if !time.is_finite() {
            return;
        }
        match message {
            ClockMessage::Start => {
                self.running = true;
                self.count = 0;
                self.period = 0.0;
                self.last = None;
            }
            ClockMessage::Continue => {
                self.running = true;
                self.reseat = true;
            }
            ClockMessage::Stop => self.running = false,
            ClockMessage::Tick => self.tick(time),
        }
    }

    fn tick(&mut self, time: f64) {
        let gap = self.last.map(|last| time - last);
        self.last = Some(time);
        if !self.running {
            return;
        }
        match gap {
            // The first tick after Start is the start of the first beat.
            None => {
                self.time = time;
                return;
            }
            Some(gap) if gap > SILENCE_SECONDS || gap <= 0.0 => {
                // The clock had stopped arriving: start the loop again from this tick.
                self.count += 1;
                self.time = time;
                self.period = 0.0;
                return;
            }
            Some(gap) if self.period == 0.0 => {
                self.count += 1;
                self.time = time;
                self.period = gap;
                return;
            }
            Some(_) => {}
        }
        self.count += 1;
        let predicted = self.time + self.period;
        let error = time - predicted;
        // Ticks after Continue, or one far off its time, are not jitter: the loop takes the
        // tick where it is and keeps its period, rather than swing the tempo.
        if std::mem::take(&mut self.reseat) || error.abs() > self.period / 2.0 {
            self.time = time;
            return;
        }
        let omega = 2.0 * PI * FOLLOW_BANDWIDTH_HZ * self.period;
        self.time = predicted + SQRT_2 * omega * error;
        self.period += omega * omega * error;
    }

    /// The tempo received, once a beat of ticks has come since Start and while they arrive.
    pub fn bpm(&self, now: f64) -> Option<f64> {
        let bpm = 60.0 / (self.period * f64::from(TICKS_PER_BEAT));
        (self.is_arriving(now) && self.count >= TICKS_TO_TRUST && self.period > 0.0)
            .then_some(bpm)
            .filter(|bpm| FOLLOW_BPM.contains(bpm))
    }

    /// Where the clock received is in its beat at `time`, from zero up to one.
    pub fn phase_at(&self, time: f64) -> Option<f64> {
        (self.period > 0.0 && self.running).then(|| {
            let ticks = self.count as f64 + (time - self.time) / self.period;
            (ticks / f64::from(TICKS_PER_BEAT)).rem_euclid(1.0)
        })
    }

    /// Started, and a tick came less than half a second before `now`.
    pub fn is_arriving(&self, now: f64) -> bool {
        self.running && self.last.is_some_and(|last| now - last <= SILENCE_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::{ClockFollower, ClockMessage, FrameClock, TICKS_PER_BEAT};

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

    /// Sends Start and then ticks at `bpm` from `from` for `seconds`, each up to `jitter` late.
    fn send(follower: &mut ClockFollower, from: f64, bpm: f64, seconds: f64, jitter: f64) -> f64 {
        let period = 60.0 / (bpm * f64::from(TICKS_PER_BEAT));
        let count = (seconds / period) as u64;
        for index in 0..count {
            let time = from + index as f64 * period;
            follower.take(time + lateness(index, jitter), ClockMessage::Tick);
        }
        from + count as f64 * period
    }

    #[test]
    fn the_tempo_and_the_beat_of_a_received_clock_are_found_through_jitter() {
        let mut follower = ClockFollower::new();
        follower.take(0.0, ClockMessage::Start);
        let end = send(&mut follower, 0.0, 120.0, 1.0, 0.002);
        // Two beats in: within a tenth of a BPM.
        let bpm = follower.bpm(end).unwrap_or(0.0);
        assert!((bpm - 120.0).abs() < 0.1, "{bpm}");
        let end = send(&mut follower, end, 120.0, 4.0, 0.002);
        // The ticks started on a beat; every half second is a beat at 120 BPM.
        let phase = follower.phase_at(end).unwrap_or(f64::NAN);
        let gap = (phase + 0.5).rem_euclid(1.0) - 0.5;
        assert!(gap.abs() < 0.02, "{phase}");
    }

    #[test]
    fn a_step_of_tempo_is_followed_within_two_beats() {
        let mut follower = ClockFollower::new();
        follower.take(0.0, ClockMessage::Start);
        let end = send(&mut follower, 0.0, 120.0, 4.0, 0.002);
        // Two beats at 126 BPM.
        let end = send(&mut follower, end, 126.0, 2.0 * 60.0 / 126.0, 0.002);
        let bpm = follower.bpm(end).unwrap_or(0.0);
        assert!((bpm - 126.0).abs() < 0.5, "{bpm}");
    }

    #[test]
    fn stop_and_continue_with_the_ticks_running_keep_the_tempo() {
        let mut follower = ClockFollower::new();
        follower.take(0.0, ClockMessage::Start);
        let end = send(&mut follower, 0.0, 120.0, 2.0, 0.001);
        follower.take(end, ClockMessage::Stop);
        // The sender goes on ticking for three seconds while stopped, then continues.
        let end = send(&mut follower, end, 120.0, 3.0, 0.001);
        follower.take(end, ClockMessage::Continue);
        let end = send(&mut follower, end, 120.0, 1.0, 0.001);
        let bpm = follower.bpm(end).unwrap_or(0.0);
        assert!((bpm - 120.0).abs() < 0.2, "{bpm}");
    }

    #[test]
    fn one_tick_far_late_does_not_swing_the_tempo() {
        let mut follower = ClockFollower::new();
        follower.take(0.0, ClockMessage::Start);
        let end = send(&mut follower, 0.0, 120.0, 2.0, 0.0);
        let period = 60.0 / (120.0 * f64::from(TICKS_PER_BEAT));
        follower.take(end + 0.1, ClockMessage::Tick);
        let end = send(&mut follower, end + 0.1 + period, 120.0, 0.25, 0.0);
        let bpm = follower.bpm(end).unwrap_or(0.0);
        assert!((bpm - 120.0).abs() < 1.0, "{bpm}");
    }

    #[test]
    fn a_clock_that_stops_arriving_gives_no_tempo() {
        let mut follower = ClockFollower::new();
        assert_eq!(follower.bpm(0.0), None, "nothing before Start");
        follower.take(0.0, ClockMessage::Start);
        let end = send(&mut follower, 0.0, 128.0, 0.2, 0.0);
        assert_eq!(follower.bpm(end), None, "less than a beat");
        let end = send(&mut follower, end, 128.0, 2.0, 0.0);
        assert!(follower.bpm(end).is_some());
        assert!(!follower.is_arriving(end + 0.6));
        assert_eq!(follower.bpm(end + 0.6), None);
        follower.take(end + 0.7, ClockMessage::Stop);
        assert!(follower.phase_at(end + 0.7).is_none());
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
