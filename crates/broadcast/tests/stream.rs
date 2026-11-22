//! The Ogg Opus stream, read back page by page and decoded: `docs/behavior/broadcasting.md`.
//! The pages are parsed here from RFC 3533 and the headers from RFC 7845, apart from the
//! writer, and the audio is a synthesised sine.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::f64::consts::PI;

use platter_broadcast::{OPUS_RATE, OpusStream};

struct Page {
    flags: u8,
    granule: u64,
    serial: u32,
    sequence: u32,
    segments: Vec<u8>,
    body: Vec<u8>,
}

/// The CRC of RFC 3533, bit by bit, apart from the table of the writer.
fn crc(bytes: &[u8]) -> u32 {
    let mut crc = 0u32;
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn pages(mut bytes: &[u8]) -> Vec<Page> {
    let mut pages = Vec::new();
    while !bytes.is_empty() {
        assert_eq!(&bytes[..4], b"OggS");
        assert_eq!(bytes[4], 0, "version");
        let count = usize::from(bytes[26]);
        let segments = bytes[27..27 + count].to_vec();
        let length = 27 + count + segments.iter().map(|s| usize::from(*s)).sum::<usize>();
        let mut whole = bytes[..length].to_vec();
        let stored = u32::from_le_bytes(whole[22..26].try_into().unwrap());
        whole[22..26].fill(0);
        assert_eq!(crc(&whole), stored, "CRC of page {}", pages.len());
        pages.push(Page {
            flags: bytes[5],
            granule: u64::from_le_bytes(bytes[6..14].try_into().unwrap()),
            serial: u32::from_le_bytes(bytes[14..18].try_into().unwrap()),
            sequence: u32::from_le_bytes(bytes[18..22].try_into().unwrap()),
            body: bytes[27 + count..length].to_vec(),
            segments,
        });
        bytes = &bytes[length..];
    }
    pages
}

/// The packets of the pages, joined across pages where they continue.
fn packets(pages: &[Page]) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut current = Vec::new();
    for page in pages {
        let mut at = 0;
        for segment in &page.segments {
            let length = usize::from(*segment);
            current.extend_from_slice(&page.body[at..at + length]);
            at += length;
            if length < 255 {
                packets.push(std::mem::take(&mut current));
            }
        }
    }
    assert!(current.is_empty(), "a packet is cut short");
    packets
}

fn sine(rate: u32, seconds: f64) -> Vec<f32> {
    let frames = (f64::from(rate) * seconds) as usize;
    (0..frames)
        .flat_map(|frame| {
            let value = (2.0 * PI * 1_000.0 * frame as f64 / f64::from(rate)).sin() as f32 * 0.5;
            [value, -value]
        })
        .collect()
}

fn encode(rate: u32, seconds: f64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut stream = OpusStream::new(rate, 128, 0x5eed, &mut out).unwrap();
    for block in sine(rate, seconds).chunks(2 * 441) {
        stream.push(block, &mut out).unwrap();
    }
    stream.finish(&mut out).unwrap();
    out
}

#[test]
fn the_pages_are_whole_in_order_and_mark_the_start_and_the_end() {
    let bytes = encode(44_100, 2.0);
    let pages = pages(&bytes);
    assert!(pages.len() > 10);
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.sequence, index as u32);
        assert_eq!(page.serial, 0x5eed);
        assert_eq!(page.flags & 0x02 != 0, index == 0, "start of stream");
        assert_eq!(
            page.flags & 0x04 != 0,
            index == pages.len() - 1,
            "end of stream"
        );
    }
    // The two headers are alone on their pages, with granule 0.
    assert_eq!((pages[0].granule, pages[1].granule), (0, 0));
    let granules: Vec<u64> = pages[2..].iter().map(|page| page.granule).collect();
    assert!(
        granules.windows(2).all(|pair| pair[0] < pair[1]),
        "{granules:?}"
    );
}

#[test]
fn the_headers_say_what_the_stream_holds() {
    let bytes = encode(44_100, 0.5);
    let packets = packets(&pages(&bytes));
    let head = &packets[0];
    assert_eq!(&head[..8], b"OpusHead");
    assert_eq!(head.len(), 19);
    assert_eq!((head[8], head[9]), (1, 2), "version and channels");
    let pre_skip = u16::from_le_bytes([head[10], head[11]]);
    assert!(pre_skip > 0);
    assert_eq!(u32::from_le_bytes(head[12..16].try_into().unwrap()), 44_100);
    assert_eq!(head[18], 0, "mapping family");
    let tags = &packets[1];
    assert_eq!(&tags[..8], b"OpusTags");
    let vendor = u32::from_le_bytes(tags[8..12].try_into().unwrap()) as usize;
    assert_eq!(&tags[12..12 + vendor], b"Platter");
}

#[test]
fn the_decoded_stream_is_the_sine_at_48000_for_as_long_as_the_input() {
    for rate in [44_100, 48_000, 96_000] {
        let seconds = 2.0;
        let bytes = encode(rate, seconds);
        let pages = pages(&bytes);
        let packets = packets(&pages);
        let pre_skip = usize::from(u16::from_le_bytes([packets[0][10], packets[0][11]]));
        let mut decoder = opus::Decoder::new(OPUS_RATE, opus::Channels::Stereo).unwrap();
        let mut decoded = Vec::new();
        let mut buffer = vec![0f32; 2 * 5_760];
        for packet in &packets[2..] {
            let frames = decoder.decode_float(packet, &mut buffer, false).unwrap();
            decoded.extend_from_slice(&buffer[..2 * frames]);
        }
        // The last granule says where the audio ends, the pre-skip included.
        let end = pages.last().unwrap().granule as usize;
        let length = end - pre_skip;
        let expected = (f64::from(OPUS_RATE) * seconds) as usize;
        assert!(length.abs_diff(expected) < 2, "{rate}: {length} frames");
        let audio = &decoded[2 * pre_skip..2 * end];
        // Past the first 20 ms the left channel is the sine and the right one its opposite.
        let left: Vec<f64> = audio
            .iter()
            .step_by(2)
            .skip(960)
            .map(|v| f64::from(*v))
            .collect();
        let right: Vec<f64> = audio
            .iter()
            .skip(1)
            .step_by(2)
            .skip(960)
            .map(|v| f64::from(*v))
            .collect();
        let rms = (left.iter().map(|v| v * v).sum::<f64>() / left.len() as f64).sqrt();
        assert!((rms - 0.5 / 2f64.sqrt()).abs() < 0.02, "{rate}: {rms}");
        let rises = left
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count();
        let hz = rises as f64 * f64::from(OPUS_RATE) / left.len() as f64;
        assert!((hz - 1_000.0).abs() < 3.0, "{rate}: {hz} Hz");
        let opposite = left
            .iter()
            .zip(&right)
            .map(|(l, r)| (l + r).abs())
            .fold(0.0, f64::max);
        assert!(opposite < 0.05, "{rate}: {opposite}");
    }
}
