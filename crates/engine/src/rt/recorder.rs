//! The tap of the mix recorder: the processor hands each frame of the master to the queue the
//! recorder worker empties. Behaviour note: `docs/behavior/recording.md`; design:
//! `docs/decisions/0034-mix-recorder.md`.
//!
//! The queue is allocated with the processor and never waited on: a frame that finds it full
//! is counted as dropped. The start and the end of a recording travel through the same queue
//! as marks carrying the number of the recording, so the worker of one recording can tell its
//! frames from those another one left behind.

use platter_dsp::Frame;
use rtrb::Producer;

/// What goes through the queue of the recorder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recorded {
    /// The recording with this number starts with the next frame.
    Start(u32),
    Frame(Frame),
    /// The recording with this number ended with the frame before.
    End(u32),
}

pub(super) struct Tap {
    queue: Producer<Recorded>,
    on: bool,
    /// The marks the queue had no room for yet; the end of the previous recording goes
    /// before the start of the next.
    end: Option<u32>,
    start: Option<u32>,
    /// Frames since the start, dropped ones included.
    frames: u64,
    dropped: u64,
}

impl Tap {
    pub(super) fn new(queue: Producer<Recorded>) -> Self {
        Self {
            queue,
            on: false,
            end: None,
            start: None,
            frames: 0,
            dropped: 0,
        }
    }

    pub(super) fn set(&mut self, on: bool, recording: u32) {
        if on {
            self.on = true;
            self.start = Some(recording);
            self.frames = 0;
            self.dropped = 0;
        } else {
            self.on = false;
            self.end = Some(recording);
        }
    }

    /// Takes one frame of the master. Marks waiting for room go first, so no frame lands
    /// before the start it belongs after.
    pub(super) fn feed(&mut self, frame: Frame) {
        if let Some(recording) = self.end
            && self.queue.push(Recorded::End(recording)).is_ok()
        {
            self.end = None;
        }
        if self.end.is_none()
            && let Some(recording) = self.start
            && self.queue.push(Recorded::Start(recording)).is_ok()
        {
            self.start = None;
        }
        if !self.on {
            return;
        }
        self.frames = self.frames.saturating_add(1);
        let waiting = self.end.is_some() || self.start.is_some();
        if waiting || self.queue.push(Recorded::Frame(frame)).is_err() {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    pub(super) fn frames(&self) -> u64 {
        self.frames
    }

    pub(super) fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use rtrb::RingBuffer;

    use super::{Recorded, Tap};

    #[test]
    fn a_full_queue_counts_what_it_drops_and_keeps_the_marks_in_order() {
        let (producer, mut consumer) = RingBuffer::new(3);
        let mut tap = Tap::new(producer);
        tap.set(true, 1);
        for n in 0..5 {
            tap.feed([n as f32, 0.0]);
        }
        // The start and two frames fit; three frames did not.
        assert_eq!((tap.frames(), tap.dropped()), (5, 3));
        tap.set(false, 1);
        tap.set(true, 2);
        // The queue is still full: the end and the start wait, and the frame is dropped.
        tap.feed([9.0, 0.0]);
        assert_eq!((tap.frames(), tap.dropped()), (1, 1));
        let mut seen = Vec::new();
        while let Ok(item) = consumer.pop() {
            seen.push(item);
        }
        tap.feed([10.0, 0.0]);
        while let Ok(item) = consumer.pop() {
            seen.push(item);
        }
        assert_eq!(
            seen,
            [
                Recorded::Start(1),
                Recorded::Frame([0.0, 0.0]),
                Recorded::Frame([1.0, 0.0]),
                Recorded::End(1),
                Recorded::Start(2),
                Recorded::Frame([10.0, 0.0]),
            ]
        );
        assert_eq!((tap.frames(), tap.dropped()), (2, 1));
    }

    #[test]
    fn nothing_goes_out_while_off() {
        let (producer, consumer) = RingBuffer::new(8);
        let mut tap = Tap::new(producer);
        tap.feed([1.0, 1.0]);
        assert!(consumer.is_empty());
        assert_eq!(tap.frames(), 0);
    }
}
