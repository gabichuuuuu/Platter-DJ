//! The MIDI clock the master clock sends: on which frame each 24th of a beat falls.
//!
//! Behaviour note: `docs/behavior/midi-clock.md`; design: `docs/decisions/0041-midi-clock.md`.
//! The processor only finds the frames; a thread of the application turns them into times
//! and sends the messages.

use rtrb::Producer;

/// Timing Clock messages to a beat, as the MIDI specification counts them to a quarter note.
pub const TICKS_PER_BEAT: u32 = 24;
/// A jump of the phase forward by more than this many 24ths of a beat in one frame is a jump
/// of the tempo master, not the clock running: it is not sent as a burst of messages.
const MAX_STEP: u32 = 3;

/// A message of the MIDI clock, in the order the specification gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMessage {
    /// Timing Clock, `F8`.
    Tick,
    /// Start, `FA`.
    Start,
    /// Stop, `FC`.
    Stop,
}

impl ClockMessage {
    /// The byte of the message.
    pub fn byte(self) -> u8 {
        match self {
            Self::Tick => 0xF8,
            Self::Start => 0xFA,
            Self::Stop => 0xFC,
        }
    }
}

/// A message and the frame of the output it belongs to, counted from the first frame the
/// processor rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockEvent {
    pub frame: u64,
    pub message: ClockMessage,
}

/// Finds the messages of the clock frame by frame.
pub(super) struct MidiClock {
    queue: Producer<ClockEvent>,
    /// The user asked to send.
    wanted: bool,
    /// Start has gone out and Stop has not.
    running: bool,
    /// Stop and Start again on the next beat.
    restart: bool,
    /// The 24th of the beat of the previous frame.
    previous: u32,
    frame: u64,
}

impl MidiClock {
    pub(super) fn new(queue: Producer<ClockEvent>) -> Self {
        Self {
            queue,
            wanted: false,
            running: false,
            restart: false,
            previous: 0,
            frame: 0,
        }
    }

    pub(super) fn set_sending(&mut self, on: bool) {
        self.wanted = on;
        self.restart = false;
    }

    pub(super) fn restart(&mut self) {
        self.restart = self.running;
    }

    pub(super) fn is_sending(&self) -> bool {
        self.running
    }

    /// The frames rendered so far.
    pub(super) fn frame(&self) -> u64 {
        self.frame
    }

    /// Takes the phase of the master clock for this frame, from zero up to one.
    pub(super) fn next(&mut self, phase: f64) {
        let current = ((phase * f64::from(TICKS_PER_BEAT)) as u32).min(TICKS_PER_BEAT - 1);
        let step = (current + TICKS_PER_BEAT - self.previous) % TICKS_PER_BEAT;
        self.previous = current;
        let moved = (1..=MAX_STEP).contains(&step);
        // A beat starts when the count wraps round to its first 24th going forward.
        let beat = moved && current < step;
        if self.running && !self.wanted {
            self.push(ClockMessage::Stop);
            self.running = false;
        } else if beat && self.wanted && (!self.running || self.restart) {
            if self.running {
                self.push(ClockMessage::Stop);
            }
            self.push(ClockMessage::Start);
            self.push(ClockMessage::Tick);
            self.running = true;
            self.restart = false;
        } else if self.running && moved {
            for _ in 0..step {
                self.push(ClockMessage::Tick);
            }
        }
        self.frame += 1;
    }

    fn push(&mut self, message: ClockMessage) {
        // A full queue means nobody reads it: the message is not needed.
        let _ = self.queue.push(ClockEvent {
            frame: self.frame,
            message,
        });
    }
}

#[cfg(test)]
mod tests {
    use rtrb::{Consumer, RingBuffer};

    use super::{ClockEvent, ClockMessage, MidiClock, TICKS_PER_BEAT};

    fn clock() -> (MidiClock, Consumer<ClockEvent>) {
        let (producer, consumer) = RingBuffer::new(4_096);
        (MidiClock::new(producer), consumer)
    }

    /// Runs `beats` beats of `frames` frames each.
    fn run(clock: &mut MidiClock, beats: usize, frames: usize) {
        for frame in 0..beats * frames {
            clock.next((frame % frames) as f64 / frames as f64);
        }
    }

    fn drain(queue: &mut Consumer<ClockEvent>) -> Vec<ClockEvent> {
        std::iter::from_fn(|| queue.pop().ok()).collect()
    }

    #[test]
    fn sending_starts_on_the_next_beat_with_twenty_four_ticks_to_each() {
        let (mut clock, mut queue) = clock();
        // Into the middle of a beat, then asked to send.
        for frame in 0..500 {
            clock.next(f64::from(frame) / 1_000.0);
        }
        clock.set_sending(true);
        for frame in 500..1_000 {
            clock.next(f64::from(frame) / 1_000.0);
        }
        assert!(drain(&mut queue).is_empty(), "nothing before the beat");
        run(&mut clock, 3, 1_000);
        let events = drain(&mut queue);
        assert_eq!(events[0].message, ClockMessage::Start);
        assert_eq!(events[0].frame, 1_000);
        let ticks: Vec<u64> = events
            .iter()
            .filter(|event| event.message == ClockMessage::Tick)
            .map(|event| event.frame)
            .collect();
        assert_eq!(ticks.len(), 3 * TICKS_PER_BEAT as usize);
        // Each 24th of a beat of 1000 frames: frame 1000 + 41.67 × n, rounded up.
        for (index, frame) in ticks.iter().enumerate() {
            let expected = 1_000.0 + 1_000.0 * index as f64 / f64::from(TICKS_PER_BEAT);
            assert_eq!(*frame, expected.ceil() as u64, "tick {index}");
        }
    }

    #[test]
    fn stopping_sends_stop_at_once_and_nothing_after() {
        let (mut clock, mut queue) = clock();
        clock.set_sending(true);
        run(&mut clock, 2, 960);
        drain(&mut queue);
        clock.set_sending(false);
        clock.next(0.3);
        assert_eq!(
            drain(&mut queue)
                .iter()
                .map(|event| event.message)
                .collect::<Vec<_>>(),
            [ClockMessage::Stop]
        );
        run(&mut clock, 2, 960);
        assert!(drain(&mut queue).is_empty());
    }

    #[test]
    fn a_restart_stops_and_starts_on_the_next_beat() {
        let (mut clock, mut queue) = clock();
        clock.set_sending(true);
        run(&mut clock, 2, 960);
        for frame in 0..480 {
            clock.next(f64::from(frame) / 960.0);
        }
        drain(&mut queue);
        // Asked in the middle of a beat: ticks until the beat ends.
        clock.restart();
        for frame in 480..960 {
            clock.next(f64::from(frame) / 960.0);
        }
        assert!(
            drain(&mut queue)
                .iter()
                .all(|event| event.message == ClockMessage::Tick)
        );
        run(&mut clock, 1, 960);
        let messages: Vec<ClockMessage> = drain(&mut queue)
            .iter()
            .map(|event| event.message)
            .take(3)
            .collect();
        assert_eq!(
            messages,
            [ClockMessage::Stop, ClockMessage::Start, ClockMessage::Tick]
        );
    }

    #[test]
    fn a_jump_of_the_phase_sends_no_burst() {
        let (mut clock, mut queue) = clock();
        clock.set_sending(true);
        run(&mut clock, 2, 960);
        clock.next(0.0);
        drain(&mut queue);
        clock.next(0.02);
        clock.next(0.6);
        clock.next(0.3);
        assert!(
            drain(&mut queue).len() <= 1,
            "one tick at most for the 24th crossed"
        );
        assert!(clock.is_sending());
    }
}
