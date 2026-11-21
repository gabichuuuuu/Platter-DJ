//! Output and input devices of the system, through `cpal`. This is the only module that names
//! it.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, InterfaceType, SampleFormat, StreamConfig};

use crate::driver::{OutputDriver, OutputSample};
use crate::input::{InputDriver, InputSample, MAX_INPUT_CHANNELS};
use crate::{
    AudioError, DeviceInfo, DeviceKind, Input, InputInfo, InputRequest, InputStartError, Output,
    Routing, Running, SAMPLE_RATES, Sink, Source, StartError, StreamInfo, StreamRequest,
};

/// Most channels a stream is opened with. The engine uses four at most; a device with more
/// is opened with its first eight so the routing can reach them.
const MAX_CHANNELS: u16 = 8;

/// How long to wait for a closed stream to give its driver back.
const RETURN_TIMEOUT: Duration = Duration::from_secs(2);

/// An open device stream. Dropping it closes the stream.
pub(crate) struct DeviceStream {
    stream: cpal::Stream,
    returns: Receiver<OutputDriver>,
}

impl From<cpal::Error> for AudioError {
    fn from(error: cpal::Error) -> Self {
        match error.kind() {
            ErrorKind::DeviceBusy => Self::Busy,
            ErrorKind::DeviceNotAvailable => Self::NoDevice,
            ErrorKind::UnsupportedConfig => Self::Unsupported(error.to_string()),
            _ => Self::Backend(error.to_string()),
        }
    }
}

fn kind_of(interface: InterfaceType) -> DeviceKind {
    match interface {
        InterfaceType::BuiltIn => DeviceKind::BuiltIn,
        InterfaceType::Usb => DeviceKind::Usb,
        InterfaceType::Bluetooth => DeviceKind::Bluetooth,
        _ => DeviceKind::Other,
    }
}

/// Name and kind of a device; a device that went away has none.
fn identity(device: &cpal::Device) -> Option<(String, DeviceKind)> {
    let description = device.description().ok()?;
    Some((
        description.name().to_owned(),
        kind_of(description.interface_type()),
    ))
}

/// The output devices of the system. An error of the system gives an empty list: to the
/// user both mean that there is nothing to play through.
pub fn devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| identity(&device))
        .map(|(name, _)| name);
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let (name, kind) = identity(&device)?;
            let ranges: Vec<_> = device.supported_output_configs().ok()?.collect();
            let outputs = ranges.iter().map(|range| range.channels()).max()?;
            let inputs = device
                .supported_input_configs()
                .ok()
                .and_then(|ranges| ranges.map(|range| range.channels()).max())
                .unwrap_or(0);
            let sample_rates = SAMPLE_RATES
                .into_iter()
                .filter(|rate| ranges.iter().any(|range| range.contains_rate(*rate)))
                .collect();
            Some(DeviceInfo {
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
                kind,
                outputs,
                inputs,
                sample_rates,
            })
        })
        .collect()
}

/// A device and the stream configuration chosen for a request, not yet running. The
/// application reads the sample rate from it to build the engine, then starts it.
pub struct Prepared {
    device: cpal::Device,
    format: SampleFormat,
    info: StreamInfo,
    routing: Routing,
}

/// Lower is better: the engine works in `f32`, and the integer formats lose nothing audible.
fn format_rank(format: SampleFormat) -> Option<u8> {
    match format {
        SampleFormat::F32 => Some(0),
        SampleFormat::I32 => Some(1),
        SampleFormat::I16 => Some(2),
        _ => None,
    }
}

/// Finds the device of a request and a configuration it accepts.
pub fn prepare(request: &StreamRequest) -> Result<Prepared, AudioError> {
    let host = cpal::default_host();
    let device = match &request.device {
        None => host.default_output_device().ok_or(AudioError::NoDevice)?,
        Some(wanted) => host
            .output_devices()?
            .find(|device| identity(device).is_some_and(|(name, _)| &name == wanted))
            .ok_or_else(|| AudioError::DeviceNotFound(wanted.clone()))?,
    };
    let (name, kind) = identity(&device).ok_or(AudioError::NoDevice)?;
    let (format, channels) = device
        .supported_output_configs()?
        .filter(|range| range.contains_rate(request.sample_rate))
        .filter_map(|range| {
            let channels = range.channels().min(MAX_CHANNELS);
            Some((
                format_rank(range.sample_format())?,
                range.sample_format(),
                channels,
            ))
        })
        // The best format first; among equals, the most channels.
        .min_by_key(|(rank, _, channels)| (*rank, MAX_CHANNELS - *channels))
        .map(|(_, format, channels)| (format, channels))
        .ok_or_else(|| AudioError::Unsupported(format!("{} Hz", request.sample_rate)))?;
    Ok(Prepared {
        device,
        format,
        info: StreamInfo {
            device: name,
            kind,
            sample_rate: request.sample_rate,
            channels,
            buffer_frames: request.buffer_frames,
            headphones: request.routing.has_headphones(channels),
        },
        routing: request.routing,
    })
}

impl Prepared {
    pub fn info(&self) -> &StreamInfo {
        &self.info
    }

    /// Opens the stream and starts it. The device may refuse the buffer size asked for;
    /// then the stream runs with the size the device prefers.
    pub fn start(self, source: Source) -> Result<Output, StartError> {
        let driver = OutputDriver::new(
            source,
            self.info.sample_rate,
            self.info.channels,
            self.routing,
        );
        match self.format {
            SampleFormat::F32 => self.run::<f32>(driver),
            SampleFormat::I32 => self.run::<i32>(driver),
            SampleFormat::I16 => self.run::<i16>(driver),
            other => Err(StartError {
                error: AudioError::Unsupported(format!("the sample format {other}")),
                source: Some(driver.into_source()),
            }),
        }
    }

    fn run<S>(self, driver: OutputDriver) -> Result<Output, StartError>
    where
        S: OutputSample + cpal::SizedSample + Send + 'static,
    {
        let stats = driver.stats();
        let (returned, returns) = channel();
        let open = |driver: OutputDriver, buffer_size: BufferSize| {
            let mut lent = Lent::new(driver, returned.clone());
            let channels = usize::from(self.info.channels);
            let errors = Arc::clone(&stats);
            self.device.build_output_stream(
                StreamConfig {
                    channels: self.info.channels,
                    sample_rate: self.info.sample_rate,
                    buffer_size,
                },
                move |output: &mut [S], _: &cpal::OutputCallbackInfo| lent.fill(output, channels),
                move |error: cpal::Error| match error.kind() {
                    ErrorKind::Xrun => errors.report_dropout(),
                    ErrorKind::DeviceNotAvailable
                    | ErrorKind::StreamInvalidated
                    | ErrorKind::DeviceChanged => errors.mark_lost(),
                    _ => {}
                },
                None,
            )
        };
        // A stream that is refused or dropped lets go of its callback, which sends the
        // driver back; that is how the source survives a failure.
        let take_back = || {
            returns
                .recv_timeout(RETURN_TIMEOUT)
                .ok()
                .map(OutputDriver::into_source)
        };
        let failed = |error: cpal::Error, source: Option<Source>| StartError {
            error: error.into(),
            source,
        };
        let stream = match open(driver, BufferSize::Fixed(self.info.buffer_frames)) {
            Ok(stream) => stream,
            Err(refused) => {
                let Ok(driver) = returns.recv_timeout(RETURN_TIMEOUT) else {
                    return Err(failed(refused, None));
                };
                match open(driver, BufferSize::Default) {
                    Ok(stream) => stream,
                    Err(refused) => return Err(failed(refused, take_back())),
                }
            }
        };
        if let Err(error) = stream.play() {
            drop(stream);
            return Err(failed(error, take_back()));
        }
        let mut info = self.info;
        if let Ok(frames) = stream.buffer_size() {
            info.buffer_frames = frames;
        }
        Ok(Output {
            info,
            stats,
            stream: Running::Device(DeviceStream { stream, returns }),
        })
    }
}

/// The driver while a stream callback holds it. When the callback is dropped, because the
/// stream closed or never opened, the driver travels back instead of being lost: its source
/// is the processor of the engine, which outlives any one stream.
struct Lent {
    driver: Option<OutputDriver>,
    back: Sender<OutputDriver>,
}

impl Lent {
    fn new(driver: OutputDriver, back: Sender<OutputDriver>) -> Self {
        Self {
            driver: Some(driver),
            back,
        }
    }

    fn fill<S: OutputSample>(&mut self, output: &mut [S], channels: usize) {
        match &mut self.driver {
            Some(driver) => driver.fill(output, channels),
            None => output.fill(S::SILENCE),
        }
    }
}

impl Drop for Lent {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            // Nobody waits for it once the output itself is gone.
            let _ = self.back.send(driver);
        }
    }
}

impl DeviceStream {
    /// Closes the stream and takes the driver back from its callback.
    pub(crate) fn stop(self) -> Option<OutputDriver> {
        drop(self.stream);
        self.returns.recv_timeout(RETURN_TIMEOUT).ok()
    }
}

/// The input devices of the system, each with the inputs it offers in one stream. An error
/// of the system gives an empty list.
pub fn input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| identity(&device))
        .map(|(name, _)| name);
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let (name, kind) = identity(&device)?;
            let ranges: Vec<_> = device.supported_input_configs().ok()?.collect();
            let inputs = ranges.iter().map(|range| range.channels()).max()?;
            let outputs = device
                .supported_output_configs()
                .ok()
                .and_then(|ranges| ranges.map(|range| range.channels()).max())
                .unwrap_or(0);
            let sample_rates = SAMPLE_RATES
                .into_iter()
                .filter(|rate| ranges.iter().any(|range| range.contains_rate(*rate)))
                .collect();
            Some(DeviceInfo {
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
                kind,
                outputs,
                inputs,
                sample_rates,
            })
        })
        .collect()
}

/// An input device and the configuration chosen for a request, not yet running.
pub struct PreparedInput {
    device: cpal::Device,
    format: SampleFormat,
    info: InputInfo,
}

/// Finds the input device of a request and a configuration it accepts at the rate of the
/// output.
pub fn prepare_input(request: &InputRequest) -> Result<PreparedInput, AudioError> {
    let host = cpal::default_host();
    let device = host
        .input_devices()?
        .find(|device| identity(device).is_some_and(|(name, _)| name == request.device))
        .ok_or_else(|| AudioError::DeviceNotFound(request.device.clone()))?;
    let (format, channels) = device
        .supported_input_configs()?
        .filter(|range| range.contains_rate(request.sample_rate))
        .filter_map(|range| {
            let channels = range.channels().min(MAX_INPUT_CHANNELS);
            Some((
                format_rank(range.sample_format())?,
                range.sample_format(),
                channels,
            ))
        })
        .min_by_key(|(rank, _, channels)| (*rank, MAX_INPUT_CHANNELS - *channels))
        .map(|(_, format, channels)| (format, channels))
        .ok_or_else(|| AudioError::Unsupported(format!("{} Hz", request.sample_rate)))?;
    Ok(PreparedInput {
        device,
        format,
        info: InputInfo {
            device: request.device.clone(),
            sample_rate: request.sample_rate,
            channels,
            buffer_frames: request.buffer_frames,
        },
    })
}

impl PreparedInput {
    pub fn info(&self) -> &InputInfo {
        &self.info
    }

    /// Opens the input and starts it. As with the output, a refused buffer size falls back
    /// to the one the device prefers.
    pub fn start(self, sink: Sink) -> Result<Input, InputStartError> {
        let driver = InputDriver::new(sink, self.info.sample_rate);
        match self.format {
            SampleFormat::F32 => self.run::<f32>(driver),
            SampleFormat::I32 => self.run::<i32>(driver),
            SampleFormat::I16 => self.run::<i16>(driver),
            other => Err(InputStartError {
                error: AudioError::Unsupported(format!("the sample format {other}")),
                sink: Some(driver.into_sink()),
            }),
        }
    }

    fn run<S>(self, driver: InputDriver) -> Result<Input, InputStartError>
    where
        S: InputSample + cpal::SizedSample + Send + 'static,
    {
        let stats = driver.stats();
        let (returned, returns) = channel();
        let open = |driver: InputDriver, buffer_size: BufferSize| {
            let mut lent = LentInput {
                driver: Some(driver),
                back: returned.clone(),
            };
            let channels = usize::from(self.info.channels);
            let errors = Arc::clone(&stats);
            self.device.build_input_stream(
                StreamConfig {
                    channels: self.info.channels,
                    sample_rate: self.info.sample_rate,
                    buffer_size,
                },
                move |input: &[S], _: &cpal::InputCallbackInfo| {
                    if let Some(driver) = &mut lent.driver {
                        driver.feed(input, channels);
                    }
                },
                move |error: cpal::Error| match error.kind() {
                    ErrorKind::Xrun => errors.report_dropout(),
                    ErrorKind::DeviceNotAvailable
                    | ErrorKind::StreamInvalidated
                    | ErrorKind::DeviceChanged => errors.mark_lost(),
                    _ => {}
                },
                None,
            )
        };
        let take_back = || {
            returns
                .recv_timeout(RETURN_TIMEOUT)
                .ok()
                .map(InputDriver::into_sink)
        };
        let failed = |error: cpal::Error, sink: Option<Sink>| InputStartError {
            error: error.into(),
            sink,
        };
        let stream = match open(driver, BufferSize::Fixed(self.info.buffer_frames)) {
            Ok(stream) => stream,
            Err(refused) => {
                let Ok(driver) = returns.recv_timeout(RETURN_TIMEOUT) else {
                    return Err(failed(refused, None));
                };
                match open(driver, BufferSize::Default) {
                    Ok(stream) => stream,
                    Err(refused) => return Err(failed(refused, take_back())),
                }
            }
        };
        if let Err(error) = stream.play() {
            drop(stream);
            return Err(failed(error, take_back()));
        }
        let mut info = self.info;
        if let Ok(frames) = stream.buffer_size() {
            info.buffer_frames = frames;
        }
        Ok(Input {
            info,
            stats,
            stream: InputStream { stream, returns },
        })
    }
}

/// An open input stream. Dropping it closes the stream.
pub(crate) struct InputStream {
    stream: cpal::Stream,
    returns: Receiver<InputDriver>,
}

impl InputStream {
    /// Closes the stream and takes the driver back from its callback.
    pub(crate) fn stop(self) -> Option<InputDriver> {
        drop(self.stream);
        self.returns.recv_timeout(RETURN_TIMEOUT).ok()
    }
}

/// The input driver while a stream callback holds it; it travels back when the callback is
/// dropped, as [`Lent`] does for the output, because its sink holds the writing end of the
/// queue of the engine.
struct LentInput {
    driver: Option<InputDriver>,
    back: Sender<InputDriver>,
}

impl Drop for LentInput {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            let _ = self.back.send(driver);
        }
    }
}
