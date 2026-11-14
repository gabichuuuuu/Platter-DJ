//! Outputs without a device.
//!
//! [`ManualStream`] is driven by its caller, block by block: the tests use it in place of a
//! sound card. [`silent`] runs the source from a clock at the pace a device would, so the
//! engine keeps working when the system has no audio device: tracks load, positions move
//! and meters read, and nothing is heard.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::driver::{OutputDriver, StreamStats};
use crate::{AudioError, DeviceKind, Output, Routing, Running, Source, StreamInfo};

/// Frames per tick of the clocked output.
const CLOCK_FRAMES: u32 = 512;
const CLOCK_CHANNELS: u16 = 2;
/// How far the clock may fall behind before it starts over instead of making up the blocks.
const MAX_LAG: Duration = Duration::from_millis(250);

/// A stream that produces audio only when asked, into a buffer the caller can read.
pub struct ManualStream {
    driver: OutputDriver,
    channels: u16,
    buffer: Vec<f32>,
}

impl ManualStream {
    /// A stream for an imaginary device with `channels` outputs.
    pub fn new(source: Source, sample_rate: u32, channels: u16, routing: Routing) -> Self {
        Self {
            driver: OutputDriver::new(source, sample_rate, channels, routing),
            channels,
            buffer: Vec::new(),
        }
    }

    pub fn stats(&self) -> Arc<StreamStats> {
        self.driver.stats()
    }

    /// Asks for the next `frames` frames, as a device callback would, and returns them
    /// interleaved.
    pub fn pump(&mut self, frames: usize) -> &[f32] {
        // The marker shows any sample the driver leaves unwritten.
        self.buffer.clear();
        self.buffer
            .resize(frames * usize::from(self.channels), f32::NAN);
        self.driver
            .fill(&mut self.buffer, usize::from(self.channels));
        &self.buffer
    }
}

/// The thread of the clocked output. Dropping it stops the thread.
pub(crate) struct ClockThread {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<OutputDriver>>,
}

impl ClockThread {
    /// Stops the thread and takes the driver back from it.
    pub(crate) fn stop(mut self) -> Option<OutputDriver> {
        self.join()
    }

    fn join(&mut self) -> Option<OutputDriver> {
        self.stop.store(true, Ordering::Relaxed);
        // A source that panicked ended the thread and took the driver with it.
        self.thread.take()?.join().ok()
    }
}

impl Drop for ClockThread {
    fn drop(&mut self) {
        self.join();
    }
}

/// Starts an output that runs `source` at `sample_rate` without a device. It fails only
/// when the system refuses a thread, and then the source is lost with it.
pub fn silent(source: Source, sample_rate: u32) -> Result<Output, AudioError> {
    let mut driver = OutputDriver::new(source, sample_rate, CLOCK_CHANNELS, Routing::default());
    let stats = driver.stats();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let tick = Duration::from_secs_f64(f64::from(CLOCK_FRAMES) / f64::from(sample_rate.max(1)));
    let thread = std::thread::Builder::new()
        .name("platter-silent-output".to_owned())
        .spawn(move || {
            let mut block = vec![0.0f32; CLOCK_FRAMES as usize * usize::from(CLOCK_CHANNELS)];
            // When the next block is due. A sleep may last longer than asked, by a lot on a
            // coarse system timer, so the blocks it owes are made up at once.
            let mut next = Instant::now();
            while !stopped.load(Ordering::Relaxed) {
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                    continue;
                }
                if now - next > MAX_LAG {
                    // The machine was busy or asleep: start over instead of catching up.
                    next = now;
                }
                driver.fill(&mut block, usize::from(CLOCK_CHANNELS));
                next += tick;
            }
            driver
        })
        .map_err(|error| AudioError::Backend(error.to_string()))?;
    Ok(Output {
        info: StreamInfo {
            device: String::new(),
            kind: DeviceKind::Other,
            sample_rate,
            channels: CLOCK_CHANNELS,
            buffer_frames: CLOCK_FRAMES,
            headphones: false,
        },
        stats,
        stream: Running::Clock(ClockThread {
            stop,
            thread: Some(thread),
        }),
    })
}
