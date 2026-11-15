//! Reads the cover picture a track file carries in its tags and decodes it into small RGBA
//! pixels for the browser. See `docs/behavior/cover-art.md`.
//!
//! The tags are read by the decoding library, through the same probe as the audio. The
//! pictures are decoded by `zune-jpeg` and `zune-png`; none of their types is public.
//!
//! A picture is untrusted input: its stored size and the size it declares are bounded before
//! its pixels are decoded, and the image decoders run behind a panic boundary of their own.

use std::fmt;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

use symphonia::core::formats::FormatReader;
use symphonia::core::io::MediaSource;
use symphonia::core::meta::{StandardVisualKey, Visual};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_png::PngDecoder;

use crate::{DecodeError, guarded, open};

/// Largest picture accepted, in bytes as stored in the file. A front cover is rarely above a
/// few hundred kilobytes; this leaves room for print-quality scans.
pub const MAX_PICTURE_BYTES: usize = 16 * 1024 * 1024;
/// Longest side a picture may declare, in pixels. At this size the decoded picture takes
/// 256 MiB, the most Platter reserves for one cover.
pub const MAX_PICTURE_SIDE: usize = 8192;
/// Longest side of a decoded cover, in pixels.
pub const COVER_SIDE: usize = 256;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
/// Start-of-image marker followed by the first byte of the next marker.
const JPEG_START: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// A cover decoded and scaled down to fit in [`COVER_SIDE`] by [`COVER_SIDE`] pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
    pub width: u32,
    pub height: u32,
    /// Four bytes per pixel, red, green, blue and alpha, with straight (not premultiplied)
    /// alpha; rows from the top.
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub enum CoverError {
    /// The audio file could not be opened, or its tags could not be read.
    File(DecodeError),
    /// The picture is in a format Platter does not decode.
    Unsupported(String),
    /// The picture breaks the rules of its format.
    Damaged(String),
    /// The picture is over [`MAX_PICTURE_BYTES`] as stored, or declares more than
    /// [`MAX_PICTURE_SIDE`] pixels on a side.
    TooLarge,
}

impl fmt::Display for CoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(error) => error.fmt(f),
            Self::Unsupported(what) => {
                write!(
                    f,
                    "unsupported picture ({what}); Platter reads JPEG and PNG"
                )
            }
            Self::Damaged(what) => write!(f, "the picture is damaged: {what}"),
            Self::TooLarge => write!(
                f,
                "the picture is too large: over {} MiB or {MAX_PICTURE_SIDE} pixels on a side",
                MAX_PICTURE_BYTES / (1024 * 1024)
            ),
        }
    }
}

impl std::error::Error for CoverError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecodeError> for CoverError {
    fn from(error: DecodeError) -> Self {
        Self::File(error)
    }
}

/// Reads the cover of the file at `path`: `None` when the file carries no picture.
pub fn cover_file(path: &Path) -> Result<Option<Cover>, CoverError> {
    let file = File::open(path).map_err(DecodeError::Io)?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    guarded(|| cover_unguarded(Box::new(file), extension))
}

/// Reads the cover of a file held in memory. `extension` is an optional hint, without the dot.
pub fn cover_bytes(bytes: Vec<u8>, extension: Option<&str>) -> Result<Option<Cover>, CoverError> {
    guarded(|| cover_unguarded(Box::new(Cursor::new(bytes)), extension))
}

fn cover_unguarded(
    source: Box<dyn MediaSource>,
    extension: Option<&str>,
) -> Result<Option<Cover>, CoverError> {
    let (mut reader, _) = open(source, extension)?;
    pick_and_decode(reader.as_mut())
}

/// Looks at every set of tags of the file, not only the newest: an MP3 or FLAC file may
/// carry an ID3v2 tag with the picture and another set of tags without one.
fn pick_and_decode(reader: &mut dyn FormatReader) -> Result<Option<Cover>, CoverError> {
    let mut metadata = reader.metadata();
    let mut older = Vec::new();
    while let Some(revision) = metadata.pop() {
        older.push(revision);
    }
    let visuals = older.iter().chain(metadata.current()).flat_map(|revision| {
        revision.media.visuals.iter().chain(
            revision
                .per_track
                .iter()
                .flat_map(|track| track.metadata.visuals.iter()),
        )
    });
    match choose(visuals) {
        Some(visual) => decode_picture(&visual.data).map(Some),
        None => Ok(None),
    }
}

/// The front cover, or the first picture when none is marked as the front cover.
fn choose<'a>(visuals: impl Iterator<Item = &'a Visual>) -> Option<&'a Visual> {
    let mut first = None;
    for visual in visuals {
        if visual.usage == Some(StandardVisualKey::FrontCover) {
            return Some(visual);
        }
        first = first.or(Some(visual));
    }
    first
}

/// Pixels of a decoded picture, four bytes each.
struct Rgba {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

fn decode_picture(data: &[u8]) -> Result<Cover, CoverError> {
    if data.len() > MAX_PICTURE_BYTES {
        return Err(CoverError::TooLarge);
    }
    // The image decoders are third-party code fed with untrusted bytes, like the audio
    // decoder (`docs/decisions/0008-decoder-panic-boundary.md`).
    let decoded = std::panic::catch_unwind(|| {
        if data.starts_with(&PNG_SIGNATURE) {
            decode_png(data)
        } else if data.starts_with(&JPEG_START) {
            decode_jpeg(data)
        } else {
            Err(CoverError::Unsupported(
                "the picture is neither JPEG nor PNG".to_owned(),
            ))
        }
    })
    .unwrap_or_else(|_| {
        Err(CoverError::Damaged(
            "the image decoder gave up on it".to_owned(),
        ))
    })?;
    Ok(fit(&decoded))
}

fn check_side(width: usize, height: usize) -> Result<(), CoverError> {
    if width > MAX_PICTURE_SIDE || height > MAX_PICTURE_SIDE {
        return Err(CoverError::TooLarge);
    }
    if width == 0 || height == 0 {
        return Err(CoverError::Damaged("it declares no pixels".to_owned()));
    }
    Ok(())
}

fn decode_jpeg(data: &[u8]) -> Result<Rgba, CoverError> {
    // The decoder refuses sizes above its own limits while it reads the header, as a damaged
    // file. JPEG stores each side in 16 bits; with the limits at that maximum the check of
    // the size is the one below, and an oversized picture is reported as such.
    let side_limit = usize::from(u16::MAX);
    let options = DecoderOptions::default()
        .set_max_width(side_limit)
        .set_max_height(side_limit)
        .jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    let damaged = |error: zune_jpeg::errors::DecodeErrors| CoverError::Damaged(error.to_string());
    decoder.decode_headers().map_err(damaged)?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| CoverError::Damaged("the JPEG header is incomplete".to_owned()))?;
    check_side(width, height)?;
    // The decoder may keep another colour space than the one asked for.
    let colorspace = decoder
        .output_colorspace()
        .ok_or_else(|| CoverError::Damaged("the JPEG header is incomplete".to_owned()))?;
    let pixels = decoder.decode().map_err(damaged)?;
    to_rgba(pixels, colorspace, width, height)
}

fn decode_png(data: &[u8]) -> Result<Rgba, CoverError> {
    // The size is read from the header chunk, which the format puts first, before the
    // decoder is built.
    let (width, height) = png_size(data)
        .ok_or_else(|| CoverError::Damaged("the PNG header is incomplete".to_owned()))?;
    check_side(width, height)?;
    let options = DecoderOptions::default()
        .set_max_width(MAX_PICTURE_SIDE)
        .set_max_height(MAX_PICTURE_SIDE)
        .png_set_strip_to_8bit(true)
        .png_set_add_alpha_channel(true)
        .png_set_decode_animated(false);
    let mut decoder = PngDecoder::new_with_options(ZCursor::new(data), options);
    let damaged = |error: zune_png::error::PngDecodeErrors| CoverError::Damaged(error.to_string());
    decoder.decode_headers().map_err(damaged)?;
    let colorspace = decoder
        .colorspace()
        .ok_or_else(|| CoverError::Damaged("the PNG header is incomplete".to_owned()))?;
    let pixels = decoder.decode_raw().map_err(damaged)?;
    to_rgba(pixels, colorspace, width, height)
}

/// Width and height from the header chunk, which follows the signature: length, type,
/// width, height (PNG specification, third edition, sections 5.2 and 11.2.1).
fn png_size(data: &[u8]) -> Option<(usize, usize)> {
    let header = data.get(8..24)?;
    if header.get(4..8)? != b"IHDR" {
        return None;
    }
    let read = |range: std::ops::Range<usize>| -> Option<usize> {
        let bytes: [u8; 4] = header.get(range)?.try_into().ok()?;
        usize::try_from(u32::from_be_bytes(bytes)).ok()
    };
    Some((read(8..12)?, read(12..16)?))
}

fn to_rgba(
    pixels: Vec<u8>,
    colorspace: ColorSpace,
    width: usize,
    height: usize,
) -> Result<Rgba, CoverError> {
    let channels = match colorspace {
        ColorSpace::Luma => 1,
        ColorSpace::LumaA => 2,
        ColorSpace::RGB => 3,
        ColorSpace::RGBA => 4,
        other => {
            return Err(CoverError::Unsupported(format!(
                "pixels in the {other:?} colour space"
            )));
        }
    };
    if pixels.len() != width * height * channels {
        return Err(CoverError::Damaged(
            "it holds another number of pixels than it declares".to_owned(),
        ));
    }
    let pixels = match channels {
        1 => pixels.iter().flat_map(|&l| [l, l, l, u8::MAX]).collect(),
        2 => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[l, a]| [l, l, l, a])
            .collect(),
        3 => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, u8::MAX])
            .collect(),
        _ => pixels,
    };
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Scales the picture down to fit in [`COVER_SIDE`] pixels with a box filter: each pixel of
/// the result averages the pixels it covers. Colours are weighted by opacity, so the colour
/// of fully transparent pixels, which the eye never sees, does not bleed into the result.
fn fit(image: &Rgba) -> Cover {
    let longest = image.width.max(image.height);
    let (width, height) = if longest <= COVER_SIDE {
        (image.width, image.height)
    } else {
        (
            (image.width * COVER_SIDE / longest).max(1),
            (image.height * COVER_SIDE / longest).max(1),
        )
    };
    let row_bytes = image.width * 4;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for out_y in 0..height {
        let rows = span(out_y, height, image.height);
        for out_x in 0..width {
            let columns = span(out_x, width, image.width);
            let mut weighted = [0u64; 3];
            let mut alpha = 0u64;
            let mut count = 0u64;
            for row in image
                .pixels
                .chunks_exact(row_bytes)
                .skip(rows.start)
                .take(rows.len())
            {
                let part = row.get(columns.start * 4..columns.end * 4).unwrap_or(&[]);
                for &[r, g, b, a] in part.as_chunks::<4>().0 {
                    let a = u64::from(a);
                    weighted[0] += u64::from(r) * a;
                    weighted[1] += u64::from(g) * a;
                    weighted[2] += u64::from(b) * a;
                    alpha += a;
                    count += 1;
                }
            }
            let rounded =
                |sum: u64, by: u64| u8::try_from((sum + by / 2) / by.max(1)).unwrap_or(u8::MAX);
            if alpha == 0 {
                rgba.extend_from_slice(&[0; 4]);
            } else {
                rgba.extend_from_slice(&[
                    rounded(weighted[0], alpha),
                    rounded(weighted[1], alpha),
                    rounded(weighted[2], alpha),
                    rounded(alpha, count),
                ]);
            }
        }
    }
    Cover {
        // Both sides are at most `MAX_PICTURE_SIDE`, checked before decoding.
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        rgba,
    }
}

/// The source pixels that output pixel `index` of `out` covers, along a side of `size`
/// source pixels. `out` is at most `size`, so every span holds at least one pixel.
fn span(index: usize, out: usize, size: usize) -> std::ops::Range<usize> {
    let out = out.max(1);
    index * size / out..((index + 1) * size / out).min(size)
}

#[cfg(test)]
mod tests {
    use super::{Rgba, fit, span};

    #[test]
    fn spans_cover_the_side_without_gaps() {
        for (out, size) in [(256, 8192), (256, 300), (3, 7), (1, 9), (5, 5)] {
            let mut next = 0;
            for index in 0..out {
                let range = span(index, out, size);
                assert_eq!(range.start, next);
                assert!(!range.is_empty());
                next = range.end;
            }
            assert_eq!(next, size);
        }
    }

    #[test]
    fn transparent_pixels_do_not_tint_the_average() {
        // 512 by 1: one opaque red pixel beside one transparent green pixel, repeated. Each
        // output pixel averages one of each.
        let pixels = [[255, 0, 0, 255], [0, 255, 0, 0]].repeat(256).concat();
        let cover = fit(&Rgba {
            width: 512,
            height: 1,
            pixels,
        });
        assert_eq!((cover.width, cover.height), (256, 1));
        for pixel in cover.rgba.as_chunks::<4>().0 {
            assert_eq!(pixel, &[255, 0, 0, 128]);
        }
    }
}
