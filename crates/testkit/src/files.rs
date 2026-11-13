//! Audio files built in memory from synthesised samples.
//!
//! The writers produce the simplest valid form of each container: uncompressed 16-bit audio.
//! Sources: the RIFF WAVE format (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/WAVE.html>),
//! AIFF 1.3 (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/AIFF/Docs/AIFF-1.3.pdf>)
//! and the FLAC format (RFC 9639, <https://www.rfc-editor.org/rfc/rfc9639>).

/// A WAV file with interleaved 16-bit PCM.
pub fn wav_pcm16(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let block_align = channels * 2;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// An AIFF file with interleaved 16-bit PCM.
pub fn aiff_pcm16(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let frames = (samples.len() / usize::from(channels.max(1))) as u32;
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(54 + samples.len() * 2);
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&(4 + 8 + 18 + 8 + 8 + data_len).to_be_bytes());
    out.extend_from_slice(b"AIFFCOMM");
    out.extend_from_slice(&18u32.to_be_bytes());
    out.extend_from_slice(&channels.to_be_bytes());
    out.extend_from_slice(&frames.to_be_bytes());
    out.extend_from_slice(&16u16.to_be_bytes());
    out.extend_from_slice(&extended_80(rate));
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&(8 + data_len).to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    for sample in samples {
        out.extend_from_slice(&sample.to_be_bytes());
    }
    out
}

/// An integer as the 80-bit extended float AIFF uses for the sample rate.
fn extended_80(value: u32) -> [u8; 10] {
    let mut out = [0; 10];
    if value == 0 {
        return out;
    }
    let top_bit = 31 - value.leading_zeros();
    let exponent = (16_383 + top_bit) as u16;
    let mantissa = u64::from(value) << (63 - top_bit);
    out[..2].copy_from_slice(&exponent.to_be_bytes());
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

const FLAC_BLOCK: usize = 4_096;

/// A FLAC file with 16-bit samples stored in verbatim subframes: valid FLAC, no compression.
pub fn flac_pcm16(rate: u32, channels: u8, samples: &[i16]) -> Vec<u8> {
    let channels = channels.clamp(1, 8);
    let frame_len = usize::from(channels);
    let total_frames = (samples.len() / frame_len) as u64;
    let mut out = Vec::new();
    out.extend_from_slice(b"fLaC");
    // Last metadata block, type 0 (STREAMINFO), 34 bytes.
    out.extend_from_slice(&[0x80, 0, 0, 34]);
    out.extend_from_slice(&(FLAC_BLOCK as u16).to_be_bytes());
    out.extend_from_slice(&(FLAC_BLOCK as u16).to_be_bytes());
    // Minimum and maximum frame size: unknown.
    out.extend_from_slice(&[0; 6]);
    let packed = (u64::from(rate) << 44)
        | (u64::from(channels - 1) << 41)
        | (15u64 << 36)
        | (total_frames & 0xF_FFFF_FFFF);
    out.extend_from_slice(&packed.to_be_bytes());
    // MD5 of the audio: zero means not computed.
    out.extend_from_slice(&[0; 16]);

    for (number, block) in samples.chunks(FLAC_BLOCK * frame_len).enumerate() {
        let start = out.len();
        let block_frames = block.len() / frame_len;
        // Sync code, fixed block size.
        out.extend_from_slice(&[0xFF, 0xF8]);
        // Block size in 16 bits at the end of the header; sample rate from STREAMINFO.
        out.push(0x70);
        // Independent channels, 16 bits per sample.
        out.push(((channels - 1) << 4) | 0x08);
        push_coded_number(&mut out, number as u32);
        out.extend_from_slice(&((block_frames - 1) as u16).to_be_bytes());
        out.push(crc8(&out[start..]));
        for channel in 0..frame_len {
            // Verbatim subframe, no wasted bits.
            out.push(0x02);
            for frame in block.chunks(frame_len) {
                out.extend_from_slice(&frame[channel].to_be_bytes());
            }
        }
        let crc = crc16(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    out
}

/// The frame number, coded like UTF-8.
fn push_coded_number(out: &mut Vec<u8>, number: u32) {
    match number {
        0..0x80 => out.push(number as u8),
        0x80..0x800 => {
            out.extend_from_slice(&[0xC0 | (number >> 6) as u8, 0x80 | (number & 0x3F) as u8])
        }
        _ => out.extend_from_slice(&[
            0xE0 | ((number >> 12) & 0x0F) as u8,
            0x80 | ((number >> 6) & 0x3F) as u8,
            0x80 | (number & 0x3F) as u8,
        ]),
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |mut crc, byte| {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
        crc
    })
}

fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |mut crc, byte| {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
        crc
    })
}

#[cfg(test)]
mod tests {
    use super::{aiff_pcm16, crc8, crc16, extended_80, flac_pcm16, wav_pcm16};

    #[test]
    fn headers_carry_the_declared_sizes() {
        let samples = [0i16; 200];
        let wav = wav_pcm16(44_100, 2, &samples);
        assert_eq!(wav.len(), 44 + 400);
        assert_eq!(&wav[4..8], &(wav.len() as u32 - 8).to_le_bytes());
        let aiff = aiff_pcm16(44_100, 2, &samples);
        assert_eq!(&aiff[4..8], &(aiff.len() as u32 - 8).to_be_bytes());
        let flac = flac_pcm16(44_100, 2, &samples);
        assert_eq!(&flac[..4], b"fLaC");
    }

    #[test]
    fn extended_float_matches_known_rates() {
        assert_eq!(
            extended_80(44_100),
            [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            extended_80(48_000),
            [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn checksums_match_their_check_values() {
        // Check values of CRC-8/SMBUS and CRC-16/UMTS for the ASCII digits 1 to 9.
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16(b"123456789"), 0xFEE8);
    }
}
