//! The part of an input stream that runs inside its callback: the samples of the device become
//! `f32` and go to the sink the application gave. Like the output driver it does not allocate,
//! lock or block; its scratch buffer is reserved when it is built.
//! `docs/decisions/0040-audio-input.md` records the design.

use std::sync::Arc;
use std::time::Instant;

use crate::Sink;
use crate::driver::StreamStats;

/// Frames converted at a time. A larger callback is served in pieces.
const PIECE_FRAMES: usize = 1_024;
/// Most channels an input stream is opened with, as for the output.
pub const MAX_INPUT_CHANNELS: u16 = 8;

/// A sample format a device may deliver.
pub trait InputSample: Copy {
    fn to_f32(self) -> f32;
}

impl InputSample for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}

impl InputSample for i16 {
    fn to_f32(self) -> f32 {
        f32::from(self) / 32_768.0
    }
}

impl InputSample for i32 {
    fn to_f32(self) -> f32 {
        (f64::from(self) / 2_147_483_648.0) as f32
    }
}

pub struct InputDriver {
    sink: Sink,
    sample_rate: u32,
    scratch: Vec<f32>,
    stats: Arc<StreamStats>,
}

impl InputDriver {
    /// Builds the driver for a stream of at most [`MAX_INPUT_CHANNELS`] channels. All its
    /// memory is reserved here.
    pub fn new(sink: Sink, sample_rate: u32) -> Self {
        Self {
            sink,
            sample_rate,
            scratch: vec![0.0; PIECE_FRAMES * usize::from(MAX_INPUT_CHANNELS)],
            stats: Arc::new(StreamStats::default()),
        }
    }

    pub fn stats(&self) -> Arc<StreamStats> {
        Arc::clone(&self.stats)
    }

    /// Ends the driver and returns its sink, for the next input.
    pub fn into_sink(self) -> Sink {
        self.sink
    }

    /// Hands `input`, interleaved with `channels` channels per frame, to the sink as `f32`.
    /// A trailing part of a frame is dropped; a stream wider than the scratch buffer gives
    /// nothing, which the device module never opens.
    pub fn feed<S: InputSample>(&mut self, input: &[S], channels: usize) {
        let started = Instant::now();
        if channels == 0 || channels > usize::from(MAX_INPUT_CHANNELS) {
            return;
        }
        let frames = input.len() / channels;
        let whole = input.get(..frames * channels).unwrap_or_default();
        for piece in whole.chunks(PIECE_FRAMES * channels) {
            let scratch = &mut self.scratch[..piece.len()];
            for (to, from) in scratch.iter_mut().zip(piece) {
                *to = from.to_f32();
            }
            (self.sink)(scratch, channels);
        }
        self.stats
            .record(started.elapsed(), frames, self.sample_rate);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{InputDriver, InputSample, PIECE_FRAMES};

    #[test]
    fn samples_convert_to_the_unit_range() {
        assert_eq!(i16::MIN.to_f32(), -1.0);
        assert_eq!(0i16.to_f32(), 0.0);
        assert!((i16::MAX.to_f32() - 1.0).abs() < 1e-4);
        assert_eq!(i32::MIN.to_f32(), -1.0);
        assert!((i32::MAX.to_f32() - 1.0).abs() < 1e-6);
        assert_eq!(0.5f32.to_f32(), 0.5);
    }

    /// Each call of the sink: its channel count and its samples.
    type Calls = Arc<Mutex<Vec<(usize, Vec<f32>)>>>;

    fn recording() -> (Calls, InputDriver) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let driver = InputDriver::new(
            Box::new(move |samples: &[f32], channels| {
                if let Ok(mut seen) = kept.lock() {
                    seen.push((channels, samples.to_vec()));
                }
            }),
            48_000,
        );
        (seen, driver)
    }

    #[test]
    fn a_large_callback_reaches_the_sink_in_whole_frames_and_drops_a_partial_one() {
        let (seen, mut driver) = recording();
        let channels = 2;
        let frames = PIECE_FRAMES * 2 + 10;
        let mut input: Vec<i16> = (0..frames * channels).map(|i| (i % 100) as i16).collect();
        input.push(7);
        driver.feed(&input, channels);
        let seen = seen.lock().map(|seen| seen.clone()).unwrap_or_default();
        assert_eq!(seen.len(), 3);
        let total: usize = seen.iter().map(|(_, samples)| samples.len()).sum();
        assert_eq!(total, frames * channels);
        assert!(
            seen.iter()
                .all(|(count, samples)| *count == channels && samples.len() % channels == 0)
        );
        assert_eq!(seen[0].1[3], 3.0 / 32_768.0);
        assert_eq!(driver.stats().dropouts(), 0);
    }

    #[test]
    fn a_stream_without_channels_or_wider_than_eight_gives_nothing() {
        let (seen, mut driver) = recording();
        driver.feed(&[0.0f32; 18], 0);
        driver.feed(&[0.0f32; 18], 9);
        assert!(seen.lock().map(|seen| seen.is_empty()).unwrap_or(false));
    }
}
