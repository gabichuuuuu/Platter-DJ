//! Decodes an audio file into `f32` PCM in memory.
//!
//! The decoding itself is done by `symphonia`, which is confined to this crate: no type of
//! that library appears in the public interface, so replacing it touches this file and the
//! module `cover` only. The cover pictures of the tags are decoded in `cover`.
//!
//! A file is untrusted input. Whatever it contains, decoding returns audio or an error; the
//! sizes it declares are bounded before memory is reserved for them.

mod cover;
mod mp4_edit;

use std::fmt;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

pub use cover::{
    COVER_SIDE, Cover, CoverError, MAX_PICTURE_BYTES, MAX_PICTURE_SIDE, cover_bytes, cover_file,
};
use mp4_edit::{Edit, read_edit};

use platter_core::SampleRate;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::{MetadataOptions, RawValue, StandardTag};

/// Longest audio a single file may hold. A longer declaration is treated as a corrupt or
/// hostile header rather than as a reason to reserve the memory.
pub const MAX_SECONDS: u64 = 6 * 60 * 60;
pub const MAX_CHANNELS: usize = 8;

/// Container of the file, as its content shows it; the extension is only a hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Wav,
    Aiff,
    Flac,
    Ogg,
    Mp3,
    M4a,
}

impl Container {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Aiff => "AIFF",
            Self::Flac => "FLAC",
            Self::Ogg => "Ogg Vorbis",
            Self::Mp3 => "MP3",
            Self::M4a => "M4A (AAC)",
        }
    }

    fn from_reader_name(name: &str) -> Option<Self> {
        match name {
            "wave" => Some(Self::Wav),
            "aiff" => Some(Self::Aiff),
            "flac" => Some(Self::Flac),
            "ogg" => Some(Self::Ogg),
            "mp3" => Some(Self::Mp3),
            "isomp4" => Some(Self::M4a),
            _ => None,
        }
    }
}

/// Tempo a tag may declare, in beats per minute. Anything outside is treated as noise.
const TAG_BPM_RANGE: std::ops::RangeInclusive<f64> = 20.0..=999.0;
/// Keys that carry the tempo: Vorbis comments and APE, ID3v2.3 and later, ID3v2.2.
const TEMPO_KEYS: [&str; 3] = ["BPM", "TBPM", "TBP"];

/// Descriptive tags read from the file. Each one is absent when the file does not carry it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Tempo in beats per minute, as the tag states it. Platter does not measure tempo
    /// before M3.
    pub bpm: Option<f64>,
}

/// What a file declares about itself, read without decoding its audio.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    pub container: Container,
    pub sample_rate: SampleRate,
    pub channels: usize,
    /// Length the header declares; absent when the format does not state it up front.
    pub seconds: Option<f64>,
    pub tags: Tags,
}

/// A whole file decoded into memory.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedAudio {
    pub container: Container,
    pub sample_rate: SampleRate,
    /// Channels of the file. `samples` holds this many per frame.
    pub channels: usize,
    /// Interleaved samples, nominally in `-1.0..=1.0`.
    pub samples: Vec<f32>,
    pub tags: Tags,
}

impl DecodedAudio {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.as_f64()
    }

    /// Interleaved stereo: a mono file feeds both sides, and a file with more than two
    /// channels keeps its first two.
    pub fn into_stereo(self) -> Vec<f32> {
        match self.channels {
            2 => self.samples,
            1 => self.samples.iter().flat_map(|s| [*s, *s]).collect(),
            channels => self
                .samples
                .chunks_exact(channels.max(1))
                .flat_map(|frame| [frame[0], frame.get(1).copied().unwrap_or(frame[0])])
                .collect(),
        }
    }
}

#[derive(Debug)]
pub enum DecodeError {
    /// The file could not be opened or read.
    Io(std::io::Error),
    /// The content is none of the formats Platter plays, or it is protected by DRM.
    Unsupported(String),
    /// The content claims to be a supported format but breaks its rules.
    Corrupt(String),
    /// The file ends before the audio its header declares.
    Truncated {
        expected_frames: u64,
        decoded_frames: u64,
    },
    /// The file declares more audio than [`MAX_SECONDS`].
    TooLong,
    /// The file holds no audio at all.
    Empty,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read the file: {error}"),
            Self::Unsupported(what) => write!(
                f,
                "unsupported file ({what}); Platter plays MP3, M4A (AAC), WAV, AIFF, FLAC and \
                 Ogg Vorbis without DRM"
            ),
            Self::Corrupt(what) => write!(f, "the file is damaged: {what}"),
            Self::Truncated {
                expected_frames,
                decoded_frames,
            } => write!(
                f,
                "the file is cut short: it declares {expected_frames} frames and holds \
                 {decoded_frames}"
            ),
            Self::TooLong => write!(
                f,
                "the file declares more than {MAX_SECONDS} seconds of audio"
            ),
            Self::Empty => f.write_str("the file holds no audio"),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SymphoniaError> for DecodeError {
    fn from(error: SymphoniaError) -> Self {
        match error {
            SymphoniaError::IoError(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                Self::Corrupt("the data ends unexpectedly".to_owned())
            }
            SymphoniaError::IoError(error) => Self::Io(error),
            SymphoniaError::Unsupported(what) => Self::Unsupported(what.to_owned()),
            SymphoniaError::LimitError(_) => Self::TooLong,
            other => Self::Corrupt(other.to_string()),
        }
    }
}

/// Decodes the file at `path`.
pub fn decode_file(path: &Path) -> Result<DecodedAudio, DecodeError> {
    let file = File::open(path).map_err(DecodeError::Io)?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    decode_source(Box::new(file), extension, || file_edit(path))
}

/// Decodes a file held in memory. `extension` is an optional hint, without the dot.
pub fn decode_bytes(bytes: Vec<u8>, extension: Option<&str>) -> Result<DecodedAudio, DecodeError> {
    let edit = read_edit(&mut Cursor::new(&bytes));
    decode_source(Box::new(Cursor::new(bytes)), extension, move || edit)
}

/// Reads the header and the tags of the file at `path` without decoding its audio. The
/// file list of the interface uses it; a file that passes may still fail to decode.
pub fn probe_file(path: &Path) -> Result<FileInfo, DecodeError> {
    let file = File::open(path).map_err(DecodeError::Io)?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    probe_source(Box::new(file), extension, || file_edit(path))
}

/// Reads the header and the tags of a file held in memory.
pub fn probe_bytes(bytes: Vec<u8>, extension: Option<&str>) -> Result<FileInfo, DecodeError> {
    let edit = read_edit(&mut Cursor::new(&bytes));
    probe_source(Box::new(Cursor::new(bytes)), extension, move || edit)
}

fn file_edit(path: &Path) -> Option<Edit> {
    File::open(path)
        .ok()
        .and_then(|mut file| read_edit(&mut file))
}

/// Runs the decoder behind a panic boundary.
///
/// The decoding library is third-party code fed with untrusted bytes. Its arithmetic on
/// header fields can overflow, which panics in builds with overflow checks; a damaged file
/// must be an error for the caller, never the end of the program. See
/// `docs/decisions/0008-decoder-panic-boundary.md`.
fn guarded<T, E: From<DecodeError>>(work: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| {
        Err(DecodeError::Corrupt("the decoder gave up on the content".to_owned()).into())
    })
}

/// `edit` reads the edit list of an MP4 file; it is called only when the content is one.
fn decode_source(
    source: Box<dyn MediaSource>,
    extension: Option<&str>,
    edit: impl FnOnce() -> Option<Edit>,
) -> Result<DecodedAudio, DecodeError> {
    guarded(|| decode_unguarded(source, extension, edit))
}

fn probe_source(
    source: Box<dyn MediaSource>,
    extension: Option<&str>,
    edit: impl FnOnce() -> Option<Edit>,
) -> Result<FileInfo, DecodeError> {
    guarded(|| probe_unguarded(source, extension, edit))
}

fn open(
    source: Box<dyn MediaSource>,
    extension: Option<&str>,
) -> Result<(Box<dyn FormatReader>, Container), DecodeError> {
    let stream = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let reader = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let reader_name = reader.format_info().short_name;
    let container = Container::from_reader_name(reader_name)
        .ok_or_else(|| DecodeError::Unsupported(format!("container `{reader_name}`")))?;
    Ok((reader, container))
}

fn probe_unguarded(
    source: Box<dyn MediaSource>,
    extension: Option<&str>,
    edit: impl FnOnce() -> Option<Edit>,
) -> Result<FileInfo, DecodeError> {
    let (mut reader, container) = open(source, extension)?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or(DecodeError::Empty)?;
    let declared_frames = track.num_frames;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or(DecodeError::Empty)?;
    let hz = parameters.sample_rate.ok_or(DecodeError::Empty)?;
    let sample_rate = SampleRate::new(hz)
        .map_err(|_| DecodeError::Unsupported(format!("sample rate {hz} Hz")))?;
    let channels = parameters
        .channels
        .as_ref()
        .map_or(0, |channels| channels.count());
    if channels > MAX_CHANNELS {
        return Err(DecodeError::Unsupported(format!("{channels} channels")));
    }
    let edited_seconds = (container == Container::M4a)
        .then(edit)
        .flatten()
        .and_then(|edit| edit.seconds());
    let seconds = edited_seconds
        .or_else(|| declared_frames.map(|frames| frames as f64 / sample_rate.as_f64()))
        .filter(|seconds| *seconds <= MAX_SECONDS as f64);
    let tags = read_tags(reader.as_mut());
    Ok(FileInfo {
        container,
        sample_rate,
        channels,
        seconds,
        tags,
    })
}

fn decode_unguarded(
    source: Box<dyn MediaSource>,
    extension: Option<&str>,
    edit: impl FnOnce() -> Option<Edit>,
) -> Result<DecodedAudio, DecodeError> {
    let (mut reader, container) = open(source, extension)?;

    let track = reader
        .default_track(TrackType::Audio)
        .ok_or(DecodeError::Empty)?;
    let track_id = track.id;
    let declared_frames = track.num_frames;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or(DecodeError::Empty)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())
        .map_err(|_| {
            DecodeError::Unsupported("the audio codec is not supported or is protected".to_owned())
        })?;

    let mut format: Option<(SampleRate, usize)> = None;
    let mut samples: Vec<f32> = Vec::new();
    let mut block: Vec<f32> = Vec::new();
    let mut damaged_packets = 0usize;

    while let Some(packet) = reader.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            // One damaged packet is skipped, as players do; a file with nothing else is
            // reported as corrupt below.
            Err(SymphoniaError::DecodeError(_)) => {
                damaged_packets += 1;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let spec = buffer.spec();
        let rate = SampleRate::new(spec.rate())
            .map_err(|_| DecodeError::Unsupported(format!("sample rate {} Hz", spec.rate())))?;
        let channels = spec.channels().count();
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(DecodeError::Unsupported(format!("{channels} channels")));
        }
        match format {
            None => format = Some((rate, channels)),
            Some(first) if first != (rate, channels) => {
                return Err(DecodeError::Unsupported(
                    "the sample rate or channel count changes inside the file".to_owned(),
                ));
            }
            Some(_) => {}
        }
        let max_samples = MAX_SECONDS * u64::from(rate.hz()) * channels as u64;
        if (samples.len() + buffer.samples_interleaved()) as u64 > max_samples {
            return Err(DecodeError::TooLong);
        }
        block.resize(buffer.samples_interleaved(), 0.0);
        buffer.copy_to_slice_interleaved(&mut block);
        samples.extend_from_slice(&block);
    }

    let Some((sample_rate, channels)) = format.filter(|_| !samples.is_empty()) else {
        return Err(if damaged_packets > 0 {
            DecodeError::Corrupt("no packet could be decoded".to_owned())
        } else {
            DecodeError::Empty
        });
    };
    // A decoder may leave a stray value in a damaged file; the engine relies on finite audio.
    for sample in &mut samples {
        if !sample.is_finite() {
            *sample = 0.0;
        }
    }
    let decoded_frames = (samples.len() / channels) as u64;
    if let Some(expected_frames) = declared_frames
        && decoded_frames < expected_frames
    {
        return Err(DecodeError::Truncated {
            expected_frames,
            decoded_frames,
        });
    }
    // The decoding library leaves the priming and padding of an AAC encoder in its output;
    // the edit list of the file says which frames are the track.
    if container == Container::M4a
        && let Some(edit) = edit()
    {
        trim(&mut samples, channels, edit.frames(sample_rate.hz()));
        if samples.is_empty() {
            return Err(DecodeError::Empty);
        }
    }
    let tags = read_tags(reader.as_mut());
    Ok(DecodedAudio {
        container,
        sample_rate,
        channels,
        samples,
        tags,
    })
}

/// Keeps `length` frames from frame `start` of interleaved `samples`, or all frames from
/// `start` when the length is absent.
fn trim(samples: &mut Vec<f32>, channels: usize, (start, length): (u64, Option<u64>)) {
    let to_samples =
        |frames: u64| usize::try_from(frames).map_or(usize::MAX, |f| f.saturating_mul(channels));
    samples.drain(..to_samples(start).min(samples.len()));
    if let Some(length) = length {
        samples.truncate(to_samples(length));
    }
}

fn read_tags(reader: &mut dyn FormatReader) -> Tags {
    let mut tags = Tags::default();
    let mut metadata = reader.metadata();
    let Some(revision) = metadata.skip_to_latest() else {
        return tags;
    };
    for tag in &revision.media.tags {
        // The library reads tempo as a whole number and drops a value with decimals, so
        // the tempo comes from the raw value, found by the standard form or by the key.
        let is_tempo = matches!(tag.std, Some(StandardTag::Bpm(_)))
            || TEMPO_KEYS
                .iter()
                .any(|key| tag.raw.key.eq_ignore_ascii_case(key));
        if is_tempo {
            if let Some(bpm) = raw_number(&tag.raw.value)
                && tags.bpm.is_none()
                && TAG_BPM_RANGE.contains(&bpm)
            {
                tags.bpm = Some(bpm);
            }
            continue;
        }
        let (slot, value) = match &tag.std {
            Some(StandardTag::TrackTitle(value)) => (&mut tags.title, value),
            Some(StandardTag::Artist(value)) => (&mut tags.artist, value),
            Some(StandardTag::Album(value)) => (&mut tags.album, value),
            _ => continue,
        };
        if slot.is_none() && !value.trim().is_empty() {
            *slot = Some(value.trim().to_owned());
        }
    }
    tags
}

fn raw_number(value: &RawValue) -> Option<f64> {
    let number = match value {
        RawValue::Float(number) => *number,
        RawValue::UnsignedInt(number) => *number as f64,
        RawValue::SignedInt(number) => *number as f64,
        RawValue::String(text) => text.trim().replace(',', ".").parse().ok()?,
        _ => return None,
    };
    number.is_finite().then_some(number)
}

#[cfg(test)]
mod tests {
    use super::trim;

    #[test]
    fn trim_drops_the_priming_and_the_padding() {
        // Two channels: frames 0, 1 are priming, 2..=4 the track, 5 padding.
        let mut samples: Vec<f32> = (0..12).map(|value| value as f32).collect();
        trim(&mut samples, 2, (2, Some(3)));
        assert_eq!(samples, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);

        let mut samples = vec![0.0; 6];
        trim(&mut samples, 2, (10, None));
        assert!(samples.is_empty());
    }
}
