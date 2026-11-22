//! The audio of the input device on its way into the mix.
//!
//! The input callback writes frames of eight channels into a queue; the processor reads one
//! frame per output frame through [`InputReader`], which keeps the queue near a target by
//! reading a little slower or faster, through the cubic interpolation of the decks.
//! `docs/decisions/0040-audio-input.md` gives the reasons and the measurements.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::interpolate::cubic;
use platter_dsp::meter::PeakMeter;
use platter_dsp::smooth::Smoothed;
use rtrb::{Consumer, Producer};

use super::strip::PARAMETER_RAMP_MS;

/// Channels of one frame of the queue: the most an input stream is opened with.
pub const INPUT_CHANNELS: usize = 8;
/// One frame of the input, the channels the device does not have silent.
pub type InputFrame = [f32; INPUT_CHANNELS];
/// Seconds of input the queue holds.
pub(super) const INPUT_QUEUE_SECONDS: f64 = 0.25;

/// The reading speed moves at most this far from one: half a percent.
const MAX_CORRECTION: f64 = 0.005;
/// Correction for a fill one whole target away from it.
const PROPORTIONAL: f64 = 0.005;
/// Correction added per second spent one whole target away.
const INTEGRAL: f64 = 0.0005;
/// Time over which the fill level is averaged, in seconds. The queue fills and empties by a
/// whole callback at a time; the average sees through that.
const FILL_SECONDS: f64 = 1.0;
/// The least the target may be, in frames, whatever the buffers.
const MIN_TARGET_FRAMES: usize = 64;

/// Where a target of the routing takes its signal, inputs counted from zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    /// One input, heard on both sides.
    Mono(u8),
    /// Two inputs, the first on the left: the first is even.
    Pair(u8),
}

impl InputSource {
    /// Every source the routing offers, the single inputs first.
    pub const ALL: [Self; 12] = [
        Self::Mono(0),
        Self::Mono(1),
        Self::Mono(2),
        Self::Mono(3),
        Self::Mono(4),
        Self::Mono(5),
        Self::Mono(6),
        Self::Mono(7),
        Self::Pair(0),
        Self::Pair(2),
        Self::Pair(4),
        Self::Pair(6),
    ];

    /// The name commands and settings use: the inputs counted from one.
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 12] = [
            "1", "2", "3", "4", "5", "6", "7", "8", "1-2", "3-4", "5-6", "7-8",
        ];
        let index = match self {
            Self::Mono(index) => usize::from(index),
            Self::Pair(first) => 8 + usize::from(first / 2),
        };
        NAMES.get(index).copied().unwrap_or("")
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.name() == name)
    }

    /// The inputs it takes, counted from zero: the highest one the device must have.
    pub fn last_input(self) -> u8 {
        match self {
            Self::Mono(index) => index,
            Self::Pair(first) => first.saturating_add(1),
        }
    }

    /// The stereo frame of this source in a frame of the input; silence for an input past
    /// the eighth.
    pub fn take(self, frame: &InputFrame) -> Frame {
        let at = |index: u8| frame.get(usize::from(index)).copied().unwrap_or(0.0);
        match self {
            Self::Mono(index) => [at(index); 2],
            Self::Pair(first) => [at(first), at(first.saturating_add(1))],
        }
    }
}

/// The writing end of the queue. The application moves it into the sink of each input stream
/// in turn; it runs on the input callback and follows the real-time rules.
pub struct InputFeed {
    queue: Producer<InputFrame>,
}

impl InputFeed {
    pub(super) fn new(queue: Producer<InputFrame>) -> Self {
        Self { queue }
    }

    /// Queues `samples`, interleaved with `channels` channels per frame; channels past the
    /// eighth are left out, and a trailing part of a frame is dropped. While the queue is
    /// full, because no output reads it, the newest frames are dropped.
    pub fn push(&mut self, samples: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let used = channels.min(INPUT_CHANNELS);
        for chunk in samples.chunks_exact(channels) {
            let mut frame = [0.0; INPUT_CHANNELS];
            frame[..used].copy_from_slice(&chunk[..used]);
            if self.queue.push(frame).is_err() {
                return;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// The queue fills up to the target before the input is heard.
    Filling,
    Reading,
}

/// The reading end of the queue, in the processor.
pub(super) struct InputReader {
    queue: Consumer<InputFrame>,
    /// The last four frames read; the output lies between the second and the third.
    history: [InputFrame; 4],
    /// Where the output lies past the second frame, from 0 to 1.
    fraction: f64,
    ratio: f64,
    state: State,
    fade: Smoothed,
    /// The fill level averaged over [`FILL_SECONDS`].
    fill: f64,
    integral: f64,
    /// The fill level the reader keeps, in frames.
    target: usize,
    /// What the control side asks the target to be; the block size raises it.
    wanted: usize,
    rate: f64,
    gaps: u64,
    meters: [PeakMeter; INPUT_CHANNELS / 2],
}

impl InputReader {
    pub(super) fn new(rate: SampleRate, queue: Consumer<InputFrame>) -> Self {
        Self {
            queue,
            history: [[0.0; INPUT_CHANNELS]; 4],
            fraction: 0.0,
            ratio: 1.0,
            state: State::Filling,
            fade: Smoothed::new(0.0, rate, PARAMETER_RAMP_MS),
            fill: 0.0,
            integral: 0.0,
            target: MIN_TARGET_FRAMES,
            wanted: 0,
            rate: rate.as_f64(),
            gaps: 0,
            meters: [PeakMeter::new(rate); INPUT_CHANNELS / 2],
        }
    }

    /// The delay the control side asks for, in frames: an input and an output buffer. One more
    /// block of the output is added as a margin.
    pub(super) fn set_latency(&mut self, frames: usize) {
        self.wanted = frames;
    }

    /// Called once per block, before its frames: sets the reading speed from the fill level.
    pub(super) fn prepare(&mut self, block_frames: usize) {
        let capacity = self.queue.buffer().capacity();
        // Two blocks at least, so a block of output always finds one waiting, and one more
        // for a callback of the input that comes late.
        self.target = (self.wanted.max(2 * block_frames) + block_frames)
            .max(MIN_TARGET_FRAMES)
            .min(capacity / 2);
        let slots = self.queue.slots();
        match self.state {
            State::Filling => {
                if slots >= self.target {
                    self.state = State::Reading;
                    self.fill = slots as f64;
                    self.integral = 0.0;
                    self.ratio = 1.0;
                    self.fade.set(1.0);
                }
            }
            State::Reading => {
                let seconds = block_frames as f64 / self.rate;
                let weight = (seconds / FILL_SECONDS).min(1.0);
                self.fill += (slots as f64 - self.fill) * weight;
                let error = (self.fill - self.target as f64) / self.target as f64;
                let limit = MAX_CORRECTION / INTEGRAL;
                self.integral = (self.integral + error * seconds).clamp(-limit, limit);
                let correction = PROPORTIONAL * error + INTEGRAL * self.integral;
                self.ratio = 1.0 + correction.clamp(-MAX_CORRECTION, MAX_CORRECTION);
            }
        }
    }

    /// The next frame of the input. An empty queue starts a gap: the input fades out and
    /// waits for the target to fill again.
    pub(super) fn next(&mut self) -> InputFrame {
        let gain = self.fade.advance();
        if self.state == State::Reading {
            self.fraction += self.ratio;
            // At most two frames, since the speed stays below 1.005.
            for _ in 0..2 {
                if self.fraction < 1.0 {
                    break;
                }
                match self.queue.pop() {
                    Ok(frame) => {
                        self.history.rotate_left(1);
                        self.history[3] = frame;
                        self.fraction -= 1.0;
                    }
                    Err(_) => {
                        self.state = State::Filling;
                        self.gaps += 1;
                        self.fade.set(0.0);
                        self.fraction = 0.0;
                        break;
                    }
                }
            }
        }
        let mut frame = [0.0; INPUT_CHANNELS];
        if gain > 0.0 {
            let fraction = self.fraction as f32;
            for (channel, sample) in frame.iter_mut().enumerate() {
                let taps = self.history.map(|past| past[channel]);
                *sample = cubic(taps, fraction) * gain;
            }
        }
        for (meter, pair) in self.meters.iter_mut().zip(frame.as_chunks::<2>().0) {
            meter.feed(*pair);
        }
        frame
    }

    /// The level of each input, linear.
    pub(super) fn levels(&self) -> [f32; INPUT_CHANNELS] {
        let mut levels = [0.0; INPUT_CHANNELS];
        for (pair, meter) in levels.as_chunks_mut::<2>().0.iter_mut().zip(&self.meters) {
            *pair = meter.levels();
        }
        levels
    }

    /// Times the queue ran dry while it was read.
    pub(super) fn gaps(&self) -> u64 {
        self.gaps
    }

    /// The speed the input is read at, against the nominal one.
    pub(super) fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Whether the input is heard, or fills up after a gap or at the start.
    pub(super) fn is_reading(&self) -> bool {
        self.state == State::Reading
    }
}

#[cfg(test)]
mod tests {
    use platter_core::SampleRate;
    use rtrb::RingBuffer;

    use super::{InputFeed, InputFrame, InputReader, InputSource};

    const RATE: u32 = 48_000;
    const BLOCK: usize = 256;

    fn pair() -> (InputFeed, InputReader) {
        let (producer, consumer) = RingBuffer::<InputFrame>::new(RATE as usize / 4);
        let rate = SampleRate::new(RATE).unwrap();
        (InputFeed::new(producer), InputReader::new(rate, consumer))
    }

    /// A writer at `speed` times the rate of the reader, in callbacks of `BLOCK` frames of a
    /// 1 kHz sine on the first input, for `seconds`. Returns the fill levels seen at the start
    /// of each block of the last second, and the frames read.
    fn run(
        feed: &mut InputFeed,
        reader: &mut InputReader,
        speed: f64,
        seconds: f64,
    ) -> (Vec<usize>, Vec<f32>) {
        let blocks = (seconds * f64::from(RATE) / BLOCK as f64) as usize;
        let mut written = 0.0f64;
        let mut phase = 0.0f64;
        let mut fills = Vec::new();
        let mut read = Vec::new();
        let step = std::f64::consts::TAU * 1_000.0 / (f64::from(RATE) * speed);
        for block in 0..blocks {
            // The writer runs ahead or behind; it delivers whole callbacks of its own.
            let due = ((block + 1) as f64 * BLOCK as f64 * speed - written) as usize;
            let mut samples = Vec::new();
            for _ in 0..due / BLOCK * BLOCK {
                samples.push(phase.sin() as f32 * 0.5);
                samples.push(0.0);
                phase += step;
            }
            written += (due / BLOCK * BLOCK) as f64;
            feed.push(&samples, 2);
            reader.prepare(BLOCK);
            if block + (f64::from(RATE) / BLOCK as f64) as usize >= blocks {
                fills.push(reader.queue.slots());
            }
            for _ in 0..BLOCK {
                read.push(reader.next()[0]);
            }
        }
        (fills, read)
    }

    fn settles_for(speed: f64) {
        let (mut feed, mut reader) = pair();
        reader.set_latency(2 * BLOCK);
        let (fills, _) = run(&mut feed, &mut reader, speed, 60.0);
        let target = reader.target as f64;
        let mean = fills.iter().sum::<usize>() as f64 / fills.len() as f64;
        assert_eq!(reader.gaps(), 0, "speed {speed}");
        assert!(
            (mean - target).abs() <= BLOCK as f64,
            "speed {speed}: mean fill {mean}, target {target}"
        );
        assert!(
            (reader.ratio() - speed).abs() < 0.0005,
            "ratio {}",
            reader.ratio()
        );
    }

    #[test]
    fn a_faster_writer_is_read_faster_and_the_fill_stays_near_the_target() {
        settles_for(1.002);
    }

    #[test]
    fn a_slower_writer_is_read_slower_and_the_fill_stays_near_the_target() {
        settles_for(0.998);
    }

    #[test]
    fn at_the_same_pace_a_sine_keeps_its_level() {
        let (mut feed, mut reader) = pair();
        let (_, read) = run(&mut feed, &mut reader, 1.0, 4.0);
        let tail = &read[read.len() - RATE as usize..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        let expected = 0.5 / 2.0f32.sqrt();
        let db = 20.0 * (rms / expected).log10();
        assert!(db.abs() < 0.1, "{db} dB");
        assert_eq!(reader.gaps(), 0);
    }

    #[test]
    fn a_writer_that_stops_gives_one_gap_then_silence_and_the_input_comes_back() {
        let (mut feed, mut reader) = pair();
        run(&mut feed, &mut reader, 1.0, 1.0);
        assert!(reader.is_reading());
        let mut last = 1.0;
        for _ in 0..40 {
            reader.prepare(BLOCK);
            for _ in 0..BLOCK {
                last = reader.next()[0];
            }
        }
        assert_eq!(reader.gaps(), 1);
        assert_eq!(last, 0.0);
        assert!(!reader.is_reading());
        run(&mut feed, &mut reader, 1.0, 1.0);
        assert!(reader.is_reading());
        assert_eq!(reader.gaps(), 1);
    }

    #[test]
    fn the_feed_keeps_eight_channels_and_drops_when_full() {
        let (mut feed, mut reader) = pair();
        let ten: Vec<f32> = (0..10).map(|value| value as f32).collect();
        feed.push(&ten, 10);
        let frame = reader.queue.pop().unwrap();
        assert_eq!(frame, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        let full = vec![0.0f32; RATE as usize];
        feed.push(&full, 1);
        assert_eq!(reader.queue.slots(), reader.queue.buffer().capacity());
    }

    #[test]
    fn a_source_takes_one_input_on_both_sides_or_a_pair() {
        let frame = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        assert_eq!(InputSource::Mono(2).take(&frame), [0.3, 0.3]);
        assert_eq!(InputSource::Pair(6).take(&frame), [0.7, 0.8]);
        assert_eq!(InputSource::Pair(7).take(&frame), [0.8, 0.0]);
        assert_eq!(InputSource::Mono(9).take(&frame), [0.0, 0.0]);
    }
}
