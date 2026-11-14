//! A PNG writer for the screenshots of the capture mode.
//!
//! It writes 8-bit RGB without compression: the image data goes into stored deflate blocks,
//! which every PNG reader accepts. The format is public: PNG is ISO/IEC 15948
//! (<https://www.w3.org/TR/png-3/>), the zlib container is RFC 1950 and deflate is RFC 1951.
//! The files are larger than a compressed PNG and are only ever written to
//! `target/screenshots/`.

use std::io::{self, Write};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// Most bytes a stored deflate block holds.
const STORED_BLOCK: usize = 65_535;
/// Largest side this writer accepts; a screenshot is far below it.
const MAX_SIDE: usize = 16_384;

/// The CRC-32 of PNG chunks: polynomial 0xEDB88320, as the PNG specification gives it.
fn crc32(bytes: &[u8], mut crc: u32) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// The Adler-32 checksum of the zlib container.
fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    // Sums of this many bytes cannot overflow a `u32` before the modulus is taken.
    for chunk in bytes.chunks(5_552) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= MODULUS;
        b %= MODULUS;
    }
    (b << 16) | a
}

fn chunk(out: &mut impl Write, kind: [u8; 4], data: &[u8]) -> io::Result<()> {
    out.write_all(&(data.len() as u32).to_be_bytes())?;
    out.write_all(&kind)?;
    out.write_all(data)?;
    let crc = !crc32(data, crc32(&kind, !0));
    out.write_all(&crc.to_be_bytes())
}

/// Writes `rgb`, rows of `width` pixels of three bytes each, as a PNG.
pub fn write_rgb(out: &mut impl Write, width: usize, height: usize, rgb: &[u8]) -> io::Result<()> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(invalid("the image has a side of zero or is too large"));
    }
    if rgb.len() != width * height * 3 {
        return Err(invalid(
            "the pixel data does not match the size of the image",
        ));
    }
    out.write_all(&SIGNATURE)?;
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    // Eight bits per channel, colour type 2 (RGB), deflate, no filter, no interlace.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(out, *b"IHDR", &header)?;

    // Each row starts with the byte of its filter: none.
    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    for row in rgb.chunks(width * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let blocks = raw.len().div_ceil(STORED_BLOCK);
    let mut data = Vec::with_capacity(raw.len() + blocks * 5 + 6);
    // The zlib header: deflate with a 32K window, no preset dictionary, fastest level.
    data.extend_from_slice(&[0x78, 0x01]);
    for (index, block) in raw.chunks(STORED_BLOCK).enumerate() {
        data.push(u8::from(index + 1 == blocks));
        let length = block.len() as u16;
        data.extend_from_slice(&length.to_le_bytes());
        data.extend_from_slice(&(!length).to_le_bytes());
        data.extend_from_slice(block);
    }
    data.extend_from_slice(&adler32(&raw).to_be_bytes());
    chunk(out, *b"IDAT", &data)?;
    chunk(out, *b"IEND", &[])
}

#[cfg(test)]
mod tests {
    use super::{adler32, crc32, write_rgb};

    #[test]
    fn checksums_match_their_check_values() {
        // The check values of CRC-32/ISO-HDLC and Adler-32 for the ASCII digits 1 to 9.
        assert_eq!(!crc32(b"123456789", !0), 0xCBF4_3926);
        assert_eq!(adler32(b"123456789"), 0x091E_01DE);
        assert_eq!(adler32(b""), 1);
        // Long input goes through the modulus more than once.
        assert_eq!(adler32(&vec![0xFF; 100_000]), 0x149A_302C);
    }

    #[test]
    fn a_small_image_has_the_expected_structure() {
        let pixels = [255, 0, 0, 0, 255, 0, 0, 0, 255, 17, 17, 17];
        let mut file = Vec::new();
        write_rgb(&mut file, 2, 2, &pixels).unwrap();
        assert_eq!(
            &file[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        // IHDR: length 13, then width 2 and height 2.
        assert_eq!(&file[8..16], &[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
        assert_eq!(&file[16..24], &[0, 0, 0, 2, 0, 0, 0, 2]);
        assert_eq!(&file[24..29], &[8, 2, 0, 0, 0]);
        // IDAT: zlib header, one final stored block of 14 bytes, the rows with their
        // filter bytes, then the checksum.
        let idat = 8 + 25;
        assert_eq!(&file[idat + 4..idat + 8], b"IDAT");
        let data = &file[idat + 8..];
        assert_eq!(&data[..7], &[0x78, 0x01, 1, 14, 0, !14u8, 0xFF]);
        assert_eq!(
            &data[7..21],
            &[0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 255, 17, 17, 17]
        );
        assert_eq!(
            &file[file.len() - 12..file.len() - 4],
            &[0, 0, 0, 0, b'I', b'E', b'N', b'D']
        );
    }

    #[test]
    fn a_large_image_is_split_into_stored_blocks() {
        let (width, height) = (300, 200);
        let pixels = vec![0x40u8; width * height * 3];
        let mut file = Vec::new();
        write_rgb(&mut file, width, height, &pixels).unwrap();
        // 180 200 bytes of rows: three blocks, the last one marked final.
        let data = &file[8 + 25 + 8..];
        assert_eq!(data[2], 0);
        assert_eq!(&data[3..5], &[0xFF, 0xFF]);
        let third = 2 + 2 * (5 + 65_535);
        assert_eq!(data[third], 1);
        let rest = (width * 3 + 1) * height - 2 * 65_535;
        assert_eq!(&data[third + 1..third + 3], &(rest as u16).to_le_bytes());
    }

    #[test]
    fn sizes_that_do_not_match_the_data_are_errors() {
        let mut file = Vec::new();
        assert!(write_rgb(&mut file, 2, 2, &[0; 11]).is_err());
        assert!(write_rgb(&mut file, 0, 2, &[]).is_err());
        assert!(write_rgb(&mut file, 100_000, 1, &[]).is_err());
    }
}
