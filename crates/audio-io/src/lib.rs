//! Audio output of Platter.
//!
//! The audio comes from a source: a function that fills a block of interleaved samples. In
//! the application that function is the processor of the engine; this crate does not know
//! it. An [`Output`] calls the source from the callback of a device, or from a clock when
//! there is no device, and keeps count of how long each call takes.
//!
//! The device library, `cpal`, is confined to the `device` module: none of its types
//! appears in this interface, so replacing it touches that file only.

mod device;
mod driver;
mod null;

use std::fmt;
use std::sync::Arc;

pub use device::{devices, prepare};
pub use driver::{OutputDriver, StreamStats};
pub use null::{ManualStream, silent};

/// The function that produces the audio: it fills interleaved samples for the number of
/// channels given. It runs on the audio thread and must follow the real-time rules.
pub type Source = Box<dyn FnMut(&mut [f32], usize) + Send + 'static>;

/// Sample rates the settings offer.
pub const SAMPLE_RATES: [u32; 3] = [44_100, 48_000, 96_000];
/// Buffer sizes the settings offer, in frames.
pub const BUFFER_SIZES: [u32; 5] = [64, 128, 256, 512, 1_024];

/// How a device is connected, as far as the system tells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceKind {
    BuiltIn,
    Usb,
    Bluetooth,
    #[default]
    Other,
}

/// An output device of the system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Name as the system shows it. It identifies the device in the settings.
    pub name: String,
    pub kind: DeviceKind,
    /// Most output channels the device offers in one stream.
    pub outputs: u16,
    /// Most input channels the device offers; zero for an output-only device.
    pub inputs: u16,
    /// Which of [`SAMPLE_RATES`] the device accepts.
    pub sample_rates: Vec<u32>,
    /// The device the system uses when none is chosen.
    pub is_default: bool,
}

/// Which outputs of the device carry the master and the headphones, counted from zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routing {
    pub master: [u16; 2],
    pub headphones: [u16; 2],
}

impl Default for Routing {
    fn default() -> Self {
        Self {
            master: [0, 1],
            headphones: [2, 3],
        }
    }
}

impl Routing {
    /// Whether a device with `channels` outputs can carry the headphones: both of their
    /// outputs exist and neither is shared with the master.
    pub fn has_headphones(&self, channels: u16) -> bool {
        self.headphones
            .iter()
            .all(|output| *output < channels && !self.master.contains(output))
            && self.headphones[0] != self.headphones[1]
    }
}

/// What the application asks of the audio device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamRequest {
    /// Name of the device; `None` is the default device of the system.
    pub device: Option<String>,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub routing: Routing,
}

/// What an open stream runs at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    /// Name of the device; empty for the deviceless output.
    pub device: String,
    pub kind: DeviceKind,
    pub sample_rate: u32,
    pub channels: u16,
    /// Frames per callback as far as the device tells; the request when it does not.
    pub buffer_frames: u32,
    /// The headphones have outputs of their own.
    pub headphones: bool,
}

impl StreamInfo {
    /// Time one buffer takes to play, in milliseconds.
    pub fn latency_ms(&self) -> f64 {
        f64::from(self.buffer_frames) * 1_000.0 / f64::from(self.sample_rate.max(1))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// The system has no output device.
    NoDevice,
    /// The device asked for by name is not there.
    DeviceNotFound(String),
    /// The device does not accept the sample rate or has no usable sample format.
    Unsupported(String),
    /// Another program holds the device.
    Busy,
    /// The system refused, in its own words.
    Backend(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("there is no audio output device"),
            Self::DeviceNotFound(name) => write!(f, "the audio device `{name}` is not connected"),
            Self::Unsupported(what) => write!(f, "the audio device does not support {what}"),
            Self::Busy => f.write_str("the audio device is in use by another program"),
            Self::Backend(message) => write!(f, "the audio system reported: {message}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// A stream that could not be started. It hands the source back, so the caller can run it
/// on another output; the source is missing only if the audio system kept hold of it.
pub struct StartError {
    pub error: AudioError,
    pub source: Option<Source>,
}

impl fmt::Debug for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartError")
            .field("error", &self.error)
            .field("source_returned", &self.source.is_some())
            .finish()
    }
}

/// A running output. The audio stops when it is dropped.
pub struct Output {
    info: StreamInfo,
    stats: Arc<StreamStats>,
    stream: Running,
}

enum Running {
    Device(device::DeviceStream),
    Clock(null::ClockThread),
}

impl Output {
    pub fn info(&self) -> &StreamInfo {
        &self.info
    }

    pub fn stats(&self) -> &Arc<StreamStats> {
        &self.stats
    }

    /// Whether a device plays the audio; false for the deviceless output.
    pub fn is_audible(&self) -> bool {
        matches!(self.stream, Running::Device(_))
    }

    /// Stops the output and gives the source back, to start another output with it. The
    /// source is lost only if the audio system does not let go of it in time.
    pub fn stop(self) -> Option<Source> {
        let driver = match self.stream {
            Running::Device(stream) => stream.stop(),
            Running::Clock(clock) => clock.stop(),
        };
        driver.map(OutputDriver::into_source)
    }
}
