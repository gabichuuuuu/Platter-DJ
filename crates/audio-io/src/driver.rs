//! The part of the output that runs inside the audio callback.
//!
//! It asks the source for the master, and for the headphones when the device can carry
//! them, places each on the outputs the routing names and measures how long that took.
//! Like everything the callback runs, it does not allocate, lock or block. Its one call
//! outside the process is reading the monotonic clock, twice per callback, which the load
//! reading needs; `docs/decisions/0010-audio-output.md` records why that is acceptable.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::{Routing, Source};

/// Frames the driver asks of the source at a time. A larger callback is served in pieces.
const PIECE_FRAMES: usize = 1_024;
/// Channels of the source with headphones: the master pair and the headphone pair.
const BUS_WITH_HEADPHONES: usize = 4;
const BUS_MASTER_ONLY: usize = 2;
/// How much of the load reading remains after one callback without load.
const LOAD_DECAY: f32 = 0.98;
/// A report of the system and a late callback this close together are one dropout.
const SAME_DROPOUT_CALLBACKS: u64 = 2;

const ORDER: Ordering = Ordering::Relaxed;

/// A sample format a device may ask for.
pub trait OutputSample: Copy {
    const SILENCE: Self;
    fn from_f32(value: f32) -> Self;
}

impl OutputSample for f32 {
    const SILENCE: Self = 0.0;

    fn from_f32(value: f32) -> Self {
        value
    }
}

impl OutputSample for i16 {
    const SILENCE: Self = 0;

    fn from_f32(value: f32) -> Self {
        // The cast saturates, and a value that is not a number becomes zero.
        (value * 32_767.0).round() as Self
    }
}

impl OutputSample for i32 {
    const SILENCE: Self = 0;

    fn from_f32(value: f32) -> Self {
        (f64::from(value) * 2_147_483_647.0).round() as Self
    }
}

/// What the callback measures, read by the interface through atomics.
#[derive(Debug, Default)]
pub struct StreamStats {
    load: AtomicU32,
    dropouts: AtomicU64,
    callbacks: AtomicU64,
    last_late_callback: AtomicU64,
    lost: AtomicBool,
}

impl StreamStats {
    /// Share of the time of one buffer that producing it took, `0.0` to `1.0` and beyond.
    /// It rises at once and falls slowly, so short peaks stay visible.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(ORDER))
    }

    /// Buffers that were late, or that the system reported as lost, since the last reset.
    pub fn dropouts(&self) -> u64 {
        self.dropouts.load(ORDER)
    }

    pub fn reset_dropouts(&self) {
        self.dropouts.store(0, ORDER);
    }

    /// The device went away or changed under the stream; the stream has to be opened again.
    pub fn is_lost(&self) -> bool {
        self.lost.load(ORDER)
    }

    pub(crate) fn mark_lost(&self) {
        self.lost.store(true, ORDER);
    }

    /// The system reported a dropout. One that follows a late callback is the same event
    /// and is not counted twice.
    pub(crate) fn report_dropout(&self) {
        let since_late = self
            .callbacks
            .load(ORDER)
            .wrapping_sub(self.last_late_callback.load(ORDER));
        if self.last_late_callback.load(ORDER) == 0 || since_late > SAME_DROPOUT_CALLBACKS {
            self.dropouts.fetch_add(1, ORDER);
        }
    }

    pub(crate) fn record(&self, took: Duration, frames: usize, sample_rate: u32) {
        let callback = self.callbacks.fetch_add(1, ORDER) + 1;
        if frames == 0 || sample_rate == 0 {
            return;
        }
        let available = frames as f32 / sample_rate as f32;
        let used = took.as_secs_f32() / available;
        let faded = self.load() * LOAD_DECAY;
        self.load.store(used.max(faded).to_bits(), ORDER);
        if used > 1.0 {
            self.dropouts.fetch_add(1, ORDER);
            self.last_late_callback.store(callback, ORDER);
        }
    }
}

pub struct OutputDriver {
    source: Source,
    sample_rate: u32,
    /// Channels asked of the source: two, or four with headphones.
    bus_channels: usize,
    /// Output of the device for each channel of the source.
    targets: [usize; BUS_WITH_HEADPHONES],
    scratch: Vec<f32>,
    stats: Arc<StreamStats>,
}

impl OutputDriver {
    /// Builds the driver for a device with `device_channels` outputs. All its memory is
    /// reserved here.
    pub fn new(source: Source, sample_rate: u32, device_channels: u16, routing: Routing) -> Self {
        let bus_channels = if routing.has_headphones(device_channels) {
            BUS_WITH_HEADPHONES
        } else {
            BUS_MASTER_ONLY
        };
        let [master_left, master_right] = routing.master.map(usize::from);
        let [phones_left, phones_right] = routing.headphones.map(usize::from);
        Self {
            source,
            sample_rate,
            bus_channels,
            targets: [master_left, master_right, phones_left, phones_right],
            scratch: vec![0.0; PIECE_FRAMES * BUS_WITH_HEADPHONES],
            stats: Arc::new(StreamStats::default()),
        }
    }

    pub fn stats(&self) -> Arc<StreamStats> {
        Arc::clone(&self.stats)
    }

    /// Ends the driver and returns its source, for the next output.
    pub fn into_source(self) -> Source {
        self.source
    }

    /// Fills `output`, interleaved with `device_channels` channels per frame. A callback of
    /// any size is served: larger than the scratch buffer in pieces, a trailing part of a
    /// frame with silence.
    pub fn fill<S: OutputSample>(&mut self, output: &mut [S], device_channels: usize) {
        let started = Instant::now();
        if device_channels == 0 {
            output.fill(S::SILENCE);
            return;
        }
        let frames = output.len() / device_channels;
        let (whole, rest) = output.split_at_mut(frames * device_channels);
        rest.fill(S::SILENCE);
        for piece in whole.chunks_mut(PIECE_FRAMES * device_channels) {
            let piece_frames = piece.len() / device_channels;
            let scratch = &mut self.scratch[..piece_frames * self.bus_channels];
            (self.source)(scratch, self.bus_channels);
            let targets = &self.targets[..self.bus_channels];
            for (frame, bus) in piece
                .chunks_mut(device_channels)
                .zip(scratch.chunks(self.bus_channels))
            {
                frame.fill(S::SILENCE);
                for (sample, target) in bus.iter().zip(targets) {
                    if let Some(output) = frame.get_mut(*target) {
                        *output = S::from_f32(*sample);
                    }
                }
            }
        }
        self.stats
            .record(started.elapsed(), frames, self.sample_rate);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{OutputSample, StreamStats};

    #[test]
    fn samples_convert_with_saturation() {
        assert_eq!(i16::from_f32(1.0), 32_767);
        assert_eq!(i16::from_f32(-1.0), -32_767);
        assert_eq!(i16::from_f32(4.0), i16::MAX);
        assert_eq!(i16::from_f32(f32::NAN), 0);
        assert_eq!(i32::from_f32(1.0), i32::MAX);
        assert_eq!(i32::from_f32(-7.0), i32::MIN);
        assert_eq!(f32::from_f32(0.25), 0.25);
    }

    #[test]
    fn the_load_rises_at_once_and_falls_slowly() {
        let stats = StreamStats::default();
        // 256 frames at 48 kHz last 5.33 ms; 2 ms of work is 37.5 % of that.
        stats.record(Duration::from_millis(2), 256, 48_000);
        assert!((stats.load() - 0.375).abs() < 0.001);
        stats.record(Duration::ZERO, 256, 48_000);
        assert!((stats.load() - 0.375 * 0.98).abs() < 0.001);
        assert_eq!(stats.dropouts(), 0);
    }

    #[test]
    fn a_late_callback_is_a_dropout_and_the_report_that_follows_is_the_same_one() {
        let stats = StreamStats::default();
        stats.record(Duration::from_millis(9), 256, 48_000);
        assert_eq!(stats.dropouts(), 1);
        stats.report_dropout();
        assert_eq!(stats.dropouts(), 1);
        for _ in 0..5 {
            stats.record(Duration::ZERO, 256, 48_000);
        }
        stats.report_dropout();
        assert_eq!(stats.dropouts(), 2);
        stats.reset_dropouts();
        assert_eq!(stats.dropouts(), 0);
    }

    #[test]
    fn a_report_without_a_late_callback_counts() {
        let stats = StreamStats::default();
        stats.report_dropout();
        assert_eq!(stats.dropouts(), 1);
    }
}
