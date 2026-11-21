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
use platter_engine::midi_clock::{ClockEvent, ClockFeed, ClockMessage, FrameClock};

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

/// A message more than this late is dropped rather than sent: it was found while no output was
/// open, or the thread stalled, and a burst of old ticks would be heard as a jump of tempo.
const STALE: Duration = Duration::from_millis(30);

/// What to do with the oldest message waiting.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Next {
    /// Not due yet: wait this long.
    Wait(Duration),
    Send,
    /// Too late to be of use.
    Drop,
}

/// The fate of a message due at `due` seconds, at `now`.
fn next(due: f64, now: f64) -> Next {
    if !due.is_finite() || now - due > STALE.as_secs_f64() {
        Next::Drop
    } else if due > now {
        Next::Wait(Duration::from_secs_f64(due - now))
    } else {
        Next::Send
    }
}

fn run(mut feed: ClockFeed, output: &mut Output, sample_rate: u32, shared: &Shared) -> ClockFeed {
    let start = Instant::now();
    let mut clock = FrameClock::new(sample_rate);
    let mut block = None;
    let mut pending: VecDeque<ClockEvent> = VecDeque::with_capacity(PENDING);
    // A Start has gone out and no Stop after it: the thread ends with a Stop.
    let mut started = false;
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
        while let Some(event) = pending.front().copied() {
            let Some(due) = clock.time_of(event.frame).map(|time| time + delay) else {
                break;
            };
            match next(due, start.elapsed().as_secs_f64()) {
                Next::Wait(until) => {
                    wait = wait.min(until);
                    break;
                }
                Next::Drop => {}
                Next::Send => {
                    if output.send(&[event.message.byte()]).is_err() {
                        shared.gone.store(true, Ordering::Relaxed);
                        return feed;
                    }
                    match event.message {
                        ClockMessage::Start => started = true,
                        ClockMessage::Stop => started = false,
                        ClockMessage::Tick | ClockMessage::Continue => {}
                    }
                }
            }
            pending.pop_front();
        }
        std::thread::sleep(wait);
    }
    if started {
        // A device left running without its clock would keep its last tempo.
        let _ = output.send(&[ClockMessage::Stop.byte()]);
    }
    feed
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Next, next, signed_delay};

    #[test]
    fn a_negative_offset_sends_earlier_and_never_before_the_frame() {
        let latency = Duration::from_millis(12);
        assert_eq!(signed_delay(latency, 0), latency);
        assert_eq!(signed_delay(latency, -5), Duration::from_millis(7));
        assert_eq!(signed_delay(latency, 30), Duration::from_millis(42));
        assert_eq!(signed_delay(latency, -50), Duration::ZERO);
    }

    #[test]
    fn a_message_waits_for_its_time_and_one_long_past_is_dropped() {
        assert!(matches!(
            next(1.010, 1.000),
            Next::Wait(wait) if (wait.as_secs_f64() - 0.010).abs() < 1e-6
        ));
        assert_eq!(next(1.000, 1.000), Next::Send);
        assert_eq!(next(1.000, 1.020), Next::Send, "a little late still goes");
        // Found while no output was open: seconds old.
        assert_eq!(next(1.000, 4.000), Next::Drop);
        assert_eq!(next(f64::NAN, 1.000), Next::Drop);
    }
}
