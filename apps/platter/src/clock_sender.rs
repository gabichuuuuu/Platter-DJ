//! The thread that sends the MIDI clock: it times each message the processor found by the
//! frame it belongs to, and sends it when that frame is heard.
//!
//! Behaviour note: `docs/behavior/midi-clock.md`; design: `docs/decisions/0041-midi-clock.md`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use platter_device_io::Output;
use platter_engine::midi_clock::{ClockEvent, ClockFeed, FrameClock};

/// The longest the thread sleeps between two looks at the frame count of the processor.
const LOOK: Duration = Duration::from_millis(1);
/// Messages found and not yet due that the thread holds: more than a second at any tempo.
const PENDING: usize = 256;

/// What the window and the thread share.
#[derive(Debug, Default)]
struct Shared {
    stop: AtomicBool,
    /// The output refused a message: the thread has ended.
    gone: AtomicBool,
    /// Microseconds added to the time each message is due: the latency of the output plus
    /// the offset of the preferences.
    delay_us: AtomicI64,
}

/// A running sender. It stops when dropped.
pub struct ClockSender {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<ClockFeed>>,
}

impl ClockSender {
    /// Starts sending what `feed` gives to `output`, for an engine at `sample_rate`, each
    /// message `delay` after its frame is played.
    pub fn start(
        feed: ClockFeed,
        mut output: Output,
        sample_rate: u32,
        delay: Duration,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        shared.delay_us.store(micros(delay), Ordering::Relaxed);
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("platter-midi-clock".to_owned())
                .spawn(move || run(feed, &mut output, sample_rate, &shared))?
        };
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn set_delay(&self, delay: Duration) {
        self.shared.delay_us.store(micros(delay), Ordering::Relaxed);
    }

    /// The output refused a message and the thread has ended.
    pub fn is_gone(&self) -> bool {
        self.shared.gone.load(Ordering::Relaxed)
    }

    /// Stops the thread and gives the feed back, for the next output.
    pub fn stop(mut self) -> Option<ClockFeed> {
        self.end()
    }

    fn end(&mut self) -> Option<ClockFeed> {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.thread.take()?.join().ok()
    }
}

impl Drop for ClockSender {
    fn drop(&mut self) {
        self.end();
    }
}

/// A signed number of microseconds, for an offset that may send earlier than the latency.
pub fn signed_delay(latency: Duration, offset_ms: i32) -> Duration {
    let total = micros(latency) + i64::from(offset_ms) * 1_000;
    Duration::from_micros(u64::try_from(total.max(0)).unwrap_or(0))
}

fn micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}

fn run(mut feed: ClockFeed, output: &mut Output, sample_rate: u32, shared: &Shared) -> ClockFeed {
    let start = Instant::now();
    let mut clock = FrameClock::new(sample_rate);
    let mut block = None;
    let mut pending: VecDeque<ClockEvent> = VecDeque::with_capacity(PENDING);
    while !shared.stop.load(Ordering::Relaxed) {
        let now = start.elapsed().as_secs_f64();
        let frame = feed.block_frame();
        if block != Some(frame) {
            clock.observe(frame, now);
            block = Some(frame);
        }
        while pending.len() < PENDING {
            match feed.pop() {
                Some(event) => pending.push_back(event),
                None => break,
            }
        }
        let delay = shared.delay_us.load(Ordering::Relaxed) as f64 / 1e6;
        let mut wait = LOOK;
        while let Some(event) = pending.front() {
            let Some(due) = clock.time_of(event.frame).map(|time| time + delay) else {
                break;
            };
            let now = start.elapsed().as_secs_f64();
            if due > now {
                wait = wait.min(Duration::from_secs_f64(due - now));
                break;
            }
            if output.send(&[event.message.byte()]).is_err() {
                shared.gone.store(true, Ordering::Relaxed);
                return feed;
            }
            pending.pop_front();
        }
        std::thread::sleep(wait);
    }
    feed
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::signed_delay;

    #[test]
    fn a_negative_offset_sends_earlier_and_never_before_the_frame() {
        let latency = Duration::from_millis(12);
        assert_eq!(signed_delay(latency, 0), latency);
        assert_eq!(signed_delay(latency, -5), Duration::from_millis(7));
        assert_eq!(signed_delay(latency, 30), Duration::from_millis(42));
        assert_eq!(signed_delay(latency, -50), Duration::ZERO);
    }
}
