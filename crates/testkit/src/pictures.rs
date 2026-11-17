//! Pictures and audio files that carry them, built in tests: PNG files written by hand with
//! stored (uncompressed) deflate blocks, and FLAC files of silence with picture blocks.
//! Sources: the PNG specification, third edition (<https://www.w3.org/TR/png-3/>), RFC 1950
//! and RFC 1951 for the zlib stream, and the FLAC format (RFC 9639).

use crate::files::flac_pcm16;

/// Picture types shared by ID3v2 and FLAC.
pub const FRONT_COVER: u8 = 3;

pub fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |mut crc, byte| {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
        crc
    })
}

pub fn adler32(bytes: &[u8]) -> u32 {
    let (a, b) = bytes.iter().fold((1u32, 0u32), |(a, b), byte| {
        let a = (a + u32::from(*byte)) % 65_521;
        (a, (b + a) % 65_521)
    });
    (b << 16) | a
}

/// Colour type of the PNG header: 0 grey, 2 RGB, 6 RGBA.
pub fn png(width: u32, height: u32, colour_type: u8, pixels: &[u8]) -> Vec<u8> {
    let channels = match colour_type {
        0 => 1,
        2 => 3,
        _ => 4,
    };
    let row = width as usize * channels;
    assert_eq!(pixels.len(), row * height as usize);
    let mut raw = Vec::new();
    for line in pixels.chunks(row) {
        // Filter type 0: the row as it is.
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let length = block.len() as u16;
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, colour_type, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    for (kind, body) in [(b"IHDR", header), (b"IDAT", zlib), (b"IEND", Vec::new())] {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(&body);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

pub fn rgba_png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&pixel(x, y));
        }
    }
    png(width, height, 6, &data)
}

/// The body of a FLAC picture block (RFC 9639, section 8.8), also the content of a
/// `METADATA_BLOCK_PICTURE` comment.
pub fn picture_block(kind: u8, mime: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&u32::from(kind).to_be_bytes());
    out.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    out.extend_from_slice(mime.as_bytes());
    // No description; width, height, depth and palette size left at zero (unknown).
    out.extend_from_slice(&[0; 4 * 5]);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// A FLAC file of silence with picture blocks after the stream information.
pub fn flac_with_pictures(pictures: &[(u8, &[u8])]) -> Vec<u8> {
    let plain = flac_pcm16(44_100, 2, &[0; 2_000]);
    // The stream information block: its header at 4, its 34-byte body after.
    let mut out = plain[..42].to_vec();
    out[4] = 0x00;
    for (index, (kind, data)) in pictures.iter().enumerate() {
        let body = picture_block(*kind, "image/png", data);
        let last = if index + 1 == pictures.len() { 0x80 } else { 0 };
        out.push(last | 6);
        out.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        out.extend_from_slice(&body);
    }
    out.extend_from_slice(&plain[42..]);
    out
}
