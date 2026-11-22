//! An Ogg Opus stream after RFC 7845, *Ogg Encapsulation for the Opus Audio Codec*
//! (<https://www.rfc-editor.org/rfc/rfc7845>): the identification header on a page of its own,
//! the comment header on the next one, then the audio packets, with granule positions counted
//! in samples at 48 kHz from the start, pre-skip included. The codec is libopus, through the
//! `opus` crate, which nothing outside this module sees.

use std::fmt;

use opus::{Application, Bitrate, Channels, Encoder};

use crate::ogg::PageWriter;
use crate::resample::Resampler;

/// Opus codes at 48 kHz and decoders play at 48 kHz (RFC 7845, section 4).
pub const OPUS_RATE: u32 = 48_000;
/// Samples of each channel in one packet: 20 ms.
const FRAME: usize = 960;
/// Packets on one page: 200 ms, so a listener waits little and a page carries little overhead.
const PACKETS_PER_PAGE: usize = 10;
/// Largest packet asked of the encoder: libopus advises 4 000 bytes.
const MAX_PACKET: usize = 4_000;
/// The bitrates offered, in kilobits per second.
pub const BITRATES: [u32; 5] = [64, 96, 128, 192, 256];
/// The name the stream gives the encoder in its comment header.
const VENDOR: &str = "Platter";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// The encoder could not be made, with the reason libopus gave.
    Start(String),
    /// A packet could not be coded.
    Encode(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(reason) => write!(f, "the Opus encoder cannot start: {reason}"),
            Self::Encode(reason) => write!(f, "the Opus encoder failed: {reason}"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Encodes stereo audio at any rate into an Ogg Opus stream.
pub struct OpusStream {
    encoder: Encoder,
    resampler: Resampler,
    pages: PageWriter,
    /// Samples at 48 kHz, interleaved, that do not make a packet yet.
    pending: Vec<f32>,
    packet: Vec<u8>,
    /// Samples at 48 kHz coded so far: the granule position after the last packet, which
    /// counts the pre-skip among them (RFC 7845, section 4).
    granule: u64,
    /// Frames of input taken so far, at `rate`.
    taken: u64,
    rate: u32,
    /// The delay of the resampler and of the encoder, in frames at 48 kHz; their sum is the
    /// pre-skip.
    delay: u64,
    lookahead: usize,
    /// Where the real audio ends, once the stream is finished: its last packet is padded.
    end: Option<u64>,
    packets_on_page: usize,
}

impl OpusStream {
    /// A stream of stereo audio at `rate` frames per second, coded at `kbps` kilobits per
    /// second, with `serial` as the serial number of its pages. Its headers go to `out`.
    pub fn new(rate: u32, kbps: u32, serial: u32, out: &mut Vec<u8>) -> Result<Self, EncodeError> {
        let start = |error: opus::Error| EncodeError::Start(error.to_string());
        let mut encoder =
            Encoder::new(OPUS_RATE, Channels::Stereo, Application::Audio).map_err(start)?;
        let bits = i32::try_from(kbps.clamp(6, 510) * 1_000).unwrap_or(128_000);
        encoder.set_bitrate(Bitrate::Bits(bits)).map_err(start)?;
        let lookahead = usize::try_from(encoder.get_lookahead().map_err(start)?).unwrap_or(0);
        let resampler = Resampler::new(rate, OPUS_RATE);
        let delay = resampler.delay_frames() as u64;
        // The samples a player leaves out at the start: those of the encoder and the resampler.
        let pre_skip = u16::try_from(lookahead + resampler.delay_frames()).unwrap_or(u16::MAX);
        let mut pages = PageWriter::new(serial);

        // Identification header (RFC 7845, section 5.1): one channel pair, mapping family 0.
        let mut head = Vec::with_capacity(19);
        head.extend_from_slice(b"OpusHead");
        head.push(1);
        head.push(2);
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&rate.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes());
        head.push(0);
        pages.packet(&head, 0, out);
        pages.flush(false, out);

        // Comment header (section 5.2): the vendor and no comments.
        let mut tags = Vec::new();
        tags.extend_from_slice(b"OpusTags");
        tags.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
        tags.extend_from_slice(VENDOR.as_bytes());
        tags.extend_from_slice(&0u32.to_le_bytes());
        pages.packet(&tags, 0, out);
        pages.flush(false, out);

        Ok(Self {
            encoder,
            resampler,
            pages,
            pending: Vec::new(),
            packet: vec![0; MAX_PACKET],
            granule: 0,
            taken: 0,
            rate,
            delay,
            lookahead,
            end: None,
            packets_on_page: 0,
        })
    }

    /// Codes the interleaved stereo `input`; the pages it completes go to `out`.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.taken += (input.len() / 2) as u64;
        self.resampler.process(input, &mut self.pending);
        self.code(out)
    }

    /// Codes what is left, padded with silence to a whole packet, and ends the stream.
    pub fn finish(mut self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        // What the resampler still holds comes out behind silence.
        self.resampler.drain(&mut self.pending);
        // The audio ends where the input did, after the pre-skip; the encoder needs its
        // lookahead of silence more to let it out.
        let real = (self.taken as f64 * f64::from(OPUS_RATE) / f64::from(self.rate.max(1))).round();
        self.end = Some(self.delay + self.lookahead as u64 + real as u64);
        self.pending
            .resize(self.pending.len() + 2 * self.lookahead, 0.0);
        let whole = self.pending.len().div_ceil(2 * FRAME) * 2 * FRAME;
        self.pending.resize(whole, 0.0);
        self.code(out)?;
        self.pages.flush(true, out);
        Ok(())
    }

    fn code(&mut self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let mut used = 0;
        while self.pending.len() - used >= 2 * FRAME {
            let frame = &self.pending[used..used + 2 * FRAME];
            let length = self
                .encoder
                .encode_float(frame, &mut self.packet)
                .map_err(|error| EncodeError::Encode(error.to_string()))?;
            used += 2 * FRAME;
            // The last page may end before its padding (RFC 7845, section 4.4).
            self.granule += FRAME as u64;
            let granule = self.end.map_or(self.granule, |end| self.granule.min(end));
            // A full page goes out when the next packet comes, so the last page of the stream
            // always holds packets and can carry the end of the stream.
            if self.packets_on_page == PACKETS_PER_PAGE {
                self.pages.flush(false, out);
                self.packets_on_page = 0;
            }
            self.pages
                .packet(&self.packet[..length.min(MAX_PACKET)], granule, out);
            self.packets_on_page += 1;
        }
        self.pending.drain(..used);
        Ok(())
    }
}
