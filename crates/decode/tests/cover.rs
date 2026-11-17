//! Cover pictures built in the test, embedded in audio files synthesised in the test.
//! See `docs/behavior/cover-art.md`.
//!
//! PNG files are written by hand with stored (uncompressed) deflate blocks. JPEG files come
//! from a baseline encoder of flat 8 by 8 blocks: each block holds only its DC coefficient,
//! so it decodes to one grey level. Sources: the PNG specification, third edition
//! (<https://www.w3.org/TR/png-3/>), RFC 1950 and RFC 1951 for the zlib stream, ITU-T T.81
//! for JPEG, the ID3v2.3 informal standard (<https://id3.org/id3v2.3.0>), the FLAC format
//! (RFC 9639) and RFC 4648 for base64.

// The lint configuration allows these in test functions; this whole file is test code.
#![allow(clippy::unwrap_used)]

use platter_decode::{
    COVER_SIDE, Cover, CoverError, DecodeError, MAX_PICTURE_BYTES, cover_bytes, cover_file,
};
use platter_testkit::files::{aiff_pcm16, flac_pcm16, flac_pcm16_tagged, wav_pcm16};
use platter_testkit::pictures::{adler32, crc32, flac_with_pictures, picture_block, png, rgba_png};
use proptest::prelude::{ProptestConfig, any, prop, proptest};

const RATE: u32 = 44_100;
const SILENCE: [i16; 2_000] = [0; 2_000];

/// Picture types shared by ID3v2 and FLAC.
const OTHER: u8 = 0;
const FRONT_COVER: u8 = 3;
const BACK_COVER: u8 = 4;

// ---------------------------------------------------------------------------------------
// Checksums and encodings

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(
                    ALPHABET[(value >> (18 - 6 * index) & 63) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// Pictures

fn solid_png(colour: [u8; 4]) -> Vec<u8> {
    rgba_png(2, 2, |_, _| colour)
}

/// Writes bits from the most significant end, with the byte stuffing of T.81 F.1.2.3.
struct BitWriter {
    out: Vec<u8>,
    byte: u8,
    used: u8,
}

impl BitWriter {
    fn push(&mut self, value: u32, bits: u8) {
        for index in (0..bits).rev() {
            self.byte = (self.byte << 1) | ((value >> index) & 1) as u8;
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.byte);
                if self.byte == 0xFF {
                    self.out.push(0);
                }
                self.byte = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            // Padding with one bits (T.81 F.1.2.3).
            let left = 8 - self.used;
            self.push((1 << left) - 1, left);
        }
        self.out
    }
}

/// A greyscale baseline JPEG whose 8 by 8 blocks are each one grey level, `levels` in raster
/// order of the blocks. The image is `blocks_wide` blocks wide.
fn grey_jpeg(blocks_wide: u16, levels: &[u8], declared: Option<(u16, u16)>) -> Vec<u8> {
    let blocks_high = (levels.len() / usize::from(blocks_wide)) as u16;
    let (width, height) = declared.unwrap_or((blocks_wide * 8, blocks_high * 8));
    let mut out = vec![0xFF, 0xD8];
    // Quantisation table 0, all steps 1.
    out.extend_from_slice(&[0xFF, 0xDB, 0, 67, 0x00]);
    out.extend_from_slice(&[1; 64]);
    // Baseline frame, 8 bits, one component with id 1, no subsampling, table 0.
    out.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8]);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&[1, 1, 0x11, 0]);
    // DC table 0: categories 0 to 11, each with a code of four bits, 0000 to 1011.
    let mut counts = [0u8; 16];
    counts[3] = 12;
    out.extend_from_slice(&[0xFF, 0xC4, 0, 3 + 16 + 12, 0x00]);
    out.extend_from_slice(&counts);
    out.extend_from_slice(&(0..12).collect::<Vec<u8>>());
    // AC table 0: only the end of block, coded as one zero bit.
    let mut counts = [0u8; 16];
    counts[0] = 1;
    out.extend_from_slice(&[0xFF, 0xC4, 0, 3 + 16 + 1, 0x10]);
    out.extend_from_slice(&counts);
    out.push(0x00);
    // Scan of component 1 with tables 0 and 0, full spectrum.
    out.extend_from_slice(&[0xFF, 0xDA, 0, 8, 1, 1, 0x00, 0, 63, 0]);
    let mut bits = BitWriter {
        out: Vec::new(),
        byte: 0,
        used: 0,
    };
    let mut previous = 0i32;
    for &level in levels {
        // A flat block of value v has the DC coefficient 8 * (v - 128) (T.81 A.3.3).
        let dc = (i32::from(level) - 128) * 8;
        let difference = dc - previous;
        previous = dc;
        let category = 32 - difference.unsigned_abs().leading_zeros();
        bits.push(category, 4);
        if category > 0 {
            let value = if difference >= 0 {
                difference
            } else {
                difference + (1 << category) - 1
            };
            bits.push(value as u32, category as u8);
        }
        bits.push(0, 1);
    }
    out.extend_from_slice(&bits.finish());
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

// ---------------------------------------------------------------------------------------
// Containers

/// An ID3v2.3 tag with one attached-picture frame per entry (ID3v2.3, section 4.15).
fn id3v2(pictures: &[(u8, &[u8])]) -> Vec<u8> {
    let mut frames = Vec::new();
    for (kind, data) in pictures {
        let mut body = vec![0];
        body.extend_from_slice(b"image/png\0");
        body.push(*kind);
        body.push(0);
        body.extend_from_slice(data);
        frames.extend_from_slice(b"APIC");
        frames.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frames.extend_from_slice(&[0, 0]);
        frames.extend_from_slice(&body);
    }
    let size = frames.len() as u32;
    let mut out = b"ID3\x03\x00\x00".to_vec();
    // The tag size is stored seven bits per byte.
    out.extend(
        (0..4)
            .rev()
            .map(|index| ((size >> (7 * index)) & 0x7F) as u8),
    );
    out.extend_from_slice(&frames);
    out
}

/// An AIFF file of silence with the tag in an ID3 chunk at the end.
fn aiff_with_id3(tag: &[u8]) -> Vec<u8> {
    let mut out = aiff_pcm16(RATE, 2, &SILENCE);
    out.extend_from_slice(b"ID3 ");
    out.extend_from_slice(&(tag.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    if tag.len() % 2 == 1 {
        out.push(0);
    }
    let form = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&form.to_be_bytes());
    out
}

/// Silent MPEG-1 Layer III frames: 128 kbit/s, 44.1 kHz, stereo, no CRC, so 417 bytes each
/// (ISO/IEC 11172-3, 2.4.2.3). Side information and main data are all zero: no audio bits.
fn silent_mp3(frames: usize) -> Vec<u8> {
    let mut frame = vec![0u8; 417];
    frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
    frame.repeat(frames)
}

fn cover_of(bytes: Vec<u8>, extension: &str) -> Cover {
    cover_bytes(bytes, Some(extension)).unwrap().unwrap()
}

fn pixel(cover: &Cover, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * cover.width + x) * 4) as usize;
    cover.rgba[at..at + 4].try_into().unwrap()
}

// ---------------------------------------------------------------------------------------
// Tests

#[test]
fn the_test_encodings_match_their_check_values() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
}

#[test]
fn a_png_cover_keeps_its_size_and_pixels() {
    let picture = rgba_png(3, 2, |x, y| {
        [x as u8 * 100, y as u8 * 200, 7, 255 - x as u8]
    });
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &picture)]), "flac");
    assert_eq!((cover.width, cover.height), (3, 2));
    assert_eq!(cover.rgba.len(), 3 * 2 * 4);
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(
                pixel(&cover, x, y),
                [x as u8 * 100, y as u8 * 200, 7, 255 - x as u8]
            );
        }
    }
}

#[test]
fn grey_and_rgb_pngs_become_opaque_rgba() {
    let grey = png(2, 1, 0, &[10, 240]);
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &grey)]), "flac");
    assert_eq!(cover.rgba, [10, 10, 10, 255, 240, 240, 240, 255]);

    let rgb = png(2, 1, 2, &[1, 2, 3, 4, 5, 6]);
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &rgb)]), "flac");
    assert_eq!(cover.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
}

#[test]
fn a_jpeg_cover_decodes_to_its_grey_levels() {
    // Two blocks side by side over two rows: 16 by 16 pixels.
    let levels = [30, 200, 128, 90];
    let picture = grey_jpeg(2, &levels, None);
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &picture)]), "flac");
    assert_eq!((cover.width, cover.height), (16, 16));
    for y in 0..16 {
        for x in 0..16 {
            let level = levels[(y / 8 * 2 + x / 8) as usize];
            let [r, g, b, a] = pixel(&cover, x, y);
            assert_eq!(a, 255);
            assert!(r == g && g == b, "({x}, {y}) is not grey");
            // The inverse DCT of the decoder may round a flat block one step off.
            assert!(r.abs_diff(level) <= 1, "({x}, {y}): {r} for {level}");
        }
    }
}

#[test]
fn a_large_picture_is_scaled_down_by_averaging() {
    // A checkerboard of single pixels: each pixel of the cover covers two black and two
    // white ones.
    let picture = rgba_png(512, 512, |x, y| {
        let level = if (x + y) % 2 == 0 { 255 } else { 0 };
        [level, level, level, 255]
    });
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &picture)]), "flac");
    assert_eq!((cover.width, cover.height), (256, 256));
    assert!(
        cover
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [128, 128, 128, 255])
    );
}

#[test]
fn scaling_keeps_the_proportions() {
    // Left half red, right half blue, twice as wide as high.
    let picture = rgba_png(600, 300, |x, _| {
        if x < 300 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    });
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &picture)]), "flac");
    assert_eq!((cover.width, cover.height), (256, 128));
    assert_eq!(pixel(&cover, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&cover, 127, 127), [255, 0, 0, 255]);
    assert_eq!(pixel(&cover, 128, 0), [0, 0, 255, 255]);
    assert_eq!(pixel(&cover, 255, 127), [0, 0, 255, 255]);

    // A picture of one row keeps a row.
    let picture = rgba_png(4_096, 1, |_, _| [9, 9, 9, 255]);
    let cover = cover_of(flac_with_pictures(&[(FRONT_COVER, &picture)]), "flac");
    assert_eq!((cover.width, cover.height), (COVER_SIDE as u32, 1));
}

#[test]
fn the_front_cover_is_preferred() {
    let other = solid_png([255, 0, 0, 255]);
    let front = solid_png([0, 0, 255, 255]);
    let cover = cover_of(
        flac_with_pictures(&[(OTHER, &other), (FRONT_COVER, &front)]),
        "flac",
    );
    assert_eq!(pixel(&cover, 0, 0), [0, 0, 255, 255]);

    // Without a front cover, the first picture.
    let back = solid_png([0, 255, 0, 255]);
    let cover = cover_of(
        flac_with_pictures(&[(OTHER, &other), (BACK_COVER, &back)]),
        "flac",
    );
    assert_eq!(pixel(&cover, 0, 0), [255, 0, 0, 255]);

    // The same through ID3v2 frames.
    let tag = id3v2(&[(BACK_COVER, &back), (FRONT_COVER, &front)]);
    let cover = cover_of(aiff_with_id3(&tag), "aiff");
    assert_eq!(pixel(&cover, 0, 0), [0, 0, 255, 255]);
}

#[test]
fn reads_an_id3v2_picture_in_aiff_and_in_front_of_flac_and_mp3() {
    let front = solid_png([12, 34, 56, 255]);
    let tag = id3v2(&[(FRONT_COVER, &front)]);
    let cover = cover_of(aiff_with_id3(&tag), "aiff");
    assert_eq!(pixel(&cover, 1, 1), [12, 34, 56, 255]);

    // The FLAC file carries tags of its own, without a picture, read after the ID3v2 tag.
    let mut bytes = tag.clone();
    bytes.extend_from_slice(&flac_pcm16_tagged(
        RATE,
        2,
        &SILENCE,
        &[("TITLE", "Test tone")],
    ));
    let cover = cover_of(bytes, "flac");
    assert_eq!(pixel(&cover, 0, 0), [12, 34, 56, 255]);

    // And in front of MP3 frames, where the tag usually is.
    let mut bytes = tag;
    bytes.extend_from_slice(&silent_mp3(12));
    let cover = cover_of(bytes, "mp3");
    assert_eq!(pixel(&cover, 0, 0), [12, 34, 56, 255]);
}

#[test]
fn reads_a_picture_from_a_vorbis_comment() {
    let front = solid_png([200, 100, 50, 255]);
    let comment = base64(&picture_block(FRONT_COVER, "image/png", &front));
    let bytes = flac_pcm16_tagged(
        RATE,
        2,
        &SILENCE,
        &[("TITLE", "Test tone"), ("METADATA_BLOCK_PICTURE", &comment)],
    );
    let cover = cover_of(bytes, "flac");
    assert_eq!(pixel(&cover, 0, 0), [200, 100, 50, 255]);
}

#[test]
fn a_file_without_a_picture_has_no_cover() {
    for (bytes, extension) in [
        (wav_pcm16(RATE, 2, &SILENCE), "wav"),
        (aiff_pcm16(RATE, 2, &SILENCE), "aiff"),
        (flac_pcm16(RATE, 2, &SILENCE), "flac"),
        (silent_mp3(12), "mp3"),
        (
            flac_pcm16_tagged(RATE, 2, &SILENCE, &[("TITLE", "Test tone")]),
            "flac",
        ),
    ] {
        assert_eq!(
            cover_bytes(bytes, Some(extension)).unwrap(),
            None,
            "{extension}"
        );
    }
}

#[test]
fn a_damaged_picture_is_an_error() {
    let good = rgba_png(16, 16, |x, y| [x as u8, y as u8, 0, 255]);
    // Cut inside the image data.
    let cut = &good[..good.len() / 2];
    let result = cover_bytes(flac_with_pictures(&[(FRONT_COVER, cut)]), Some("flac"));
    assert!(matches!(result, Err(CoverError::Damaged(_))), "{result:?}");

    // A JPEG cut inside its tables.
    let mut jpeg = grey_jpeg(1, &[100], None);
    jpeg.truncate(40);
    let result = cover_bytes(flac_with_pictures(&[(FRONT_COVER, &jpeg)]), Some("flac"));
    assert!(matches!(result, Err(CoverError::Damaged(_))), "{result:?}");

    // Neither JPEG nor PNG.
    let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;";
    let result = cover_bytes(flac_with_pictures(&[(FRONT_COVER, gif)]), Some("flac"));
    assert!(
        matches!(result, Err(CoverError::Unsupported(_))),
        "{result:?}"
    );
}

#[test]
fn oversized_pictures_are_refused_before_decoding() {
    // The header declares 9000 by 10 pixels; no image data follows.
    let mut header = png(1, 1, 6, &[0; 4]);
    header[16..20].copy_from_slice(&9_000u32.to_be_bytes());
    header[20..24].copy_from_slice(&10u32.to_be_bytes());
    let result = cover_bytes(flac_with_pictures(&[(FRONT_COVER, &header)]), Some("flac"));
    assert!(matches!(result, Err(CoverError::TooLarge)), "{result:?}");

    // A valid 8 by 8 JPEG scan under a frame header that declares 9000 by 8 pixels.
    let jpeg = grey_jpeg(1, &[100], Some((9_000, 8)));
    let result = cover_bytes(flac_with_pictures(&[(FRONT_COVER, &jpeg)]), Some("flac"));
    assert!(matches!(result, Err(CoverError::TooLarge)), "{result:?}");

    // More bytes than allowed, stored in an ID3v2 frame (a FLAC block cannot hold them).
    let mut huge = solid_png([1, 2, 3, 255]);
    huge.resize(MAX_PICTURE_BYTES + 1, 0);
    let mut bytes = id3v2(&[(FRONT_COVER, &huge)]);
    bytes.extend_from_slice(&flac_pcm16(RATE, 2, &SILENCE));
    let result = cover_bytes(bytes, Some("flac"));
    assert!(matches!(result, Err(CoverError::TooLarge)), "{result:?}");
}

#[test]
fn an_unreadable_audio_file_is_a_file_error() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-cover.flac");
    assert!(matches!(
        cover_file(&path),
        Err(CoverError::File(DecodeError::Io(_)))
    ));
    assert!(matches!(
        cover_bytes(b"not audio".repeat(100), Some("flac")),
        Err(CoverError::File(_))
    ));
}

#[test]
fn reads_a_cover_from_disk() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("cover-from-disk.flac");
    let picture = solid_png([3, 4, 5, 255]);
    std::fs::write(&path, flac_with_pictures(&[(FRONT_COVER, &picture)])).unwrap();
    let cover = cover_file(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(pixel(&cover.unwrap().unwrap(), 0, 0), [3, 4, 5, 255]);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn damaged_pictures_never_panic(
        jpeg in any::<bool>(),
        damage in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..16),
    ) {
        let mut picture = if jpeg {
            grey_jpeg(2, &[10, 60, 110, 160], None)
        } else {
            rgba_png(4, 4, |x, y| [x as u8 * 60, y as u8 * 60, 9, 255])
        };
        for (index, value) in damage {
            let position = index.index(picture.len());
            picture[position] = value;
        }
        if let Ok(Some(cover)) =
            cover_bytes(flac_with_pictures(&[(FRONT_COVER, &picture)]), Some("flac"))
        {
            assert!(cover.width as usize <= COVER_SIDE && cover.height as usize <= COVER_SIDE);
            assert_eq!(cover.rgba.len(), cover.width as usize * cover.height as usize * 4);
        }
    }
}
