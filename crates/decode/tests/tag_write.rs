//! Writing tags into files synthesised in the test. See `docs/behavior/tag-writing.md`.
//!
//! WAV, AIFF and FLAC come from the test kit and MP3 is silent frames. Ogg Vorbis and M4A are
//! built here from their containers with headers but no audio that decodes: reading the
//! header and the tags needs none. The written tags are read back with Platter's reader
//! where it shows the field, and otherwise with the small parsers of this file, written from
//! the same specifications as the writer but separately from it; some tests compare with
//! bytes written out by hand. Sources: the Ogg encapsulation (RFC 3533), the Vorbis I
//! specification (sections 4.2, 5 and appendix A.2), RFC 9639 for FLAC, ISO/IEC 14496-12 and
//! 14496-14 for the MP4 boxes with the QuickTime data atom, the ID3v2.3 and ID3v2.4
//! documents (<https://id3.org/id3v2.3.0>, <https://id3.org/id3v2.4.0-structure>,
//! <https://id3.org/id3v2.4.0-frames>), RIFF INFO, AIFF 1.3, and RFC 1950 and 1951 for the
//! zlib stream.

// The lint configuration allows these in test functions; this whole file is test code.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use platter_decode::{
    Container, CoverError, CoverPicture, MAX_COVER_IMPORT_BYTES, MAX_PLATTER_BYTES, MAX_TAG_CHARS,
    PictureFormat, TagEdit, TagWriteError, cover_file, decode_file, probe_file, write_tags,
};
use platter_testkit::files::{aiff_pcm16, flac_pcm16, flac_pcm16_tagged, wav_pcm16};
use platter_testkit::pictures::{adler32, flac_with_pictures, picture_block, rgba_png};
use platter_testkit::signal::{sine, stereo, to_i16};
use proptest::prelude::*;

const RATE: u32 = 44_100;

// ---------------------------------------------------------------------------------------
// Files

fn folder(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("tag_write")
        .join(name);
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

fn file(folder: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = folder.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

fn sine_pcm() -> Vec<i16> {
    to_i16(&stereo(&sine(440.0, RATE, 4_410, 0.5)))
}

/// MPEG-1 Layer III frames, 128 kbit/s at 44.1 kHz, with no audio bits.
fn silent_mp3(frames: usize) -> Vec<u8> {
    let mut frame = vec![0u8; 417];
    frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
    frame.repeat(frames)
}

fn synchsafe(value: usize) -> [u8; 4] {
    let value = value as u32;
    [
        (value >> 21 & 0x7F) as u8,
        (value >> 14 & 0x7F) as u8,
        (value >> 7 & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

/// An ID3v2.4 frame; `flags` is the second flag byte (format flags).
fn id3_frame(id: &[u8; 4], flags: u8, body: &[u8]) -> Vec<u8> {
    [id.as_slice(), &synchsafe(body.len()), &[0, flags], body].concat()
}

fn id3_text(id: &[u8; 4], text: &str) -> Vec<u8> {
    id3_frame(id, 0, &[&[3], text.as_bytes()].concat())
}

/// A user text frame, UTF-8.
fn id3_txxx(description: &str, value: &str) -> Vec<u8> {
    id3_frame(
        b"TXXX",
        0,
        &[&[3], description.as_bytes(), &[0], value.as_bytes()].concat(),
    )
}

fn id3_picture(data: &[u8]) -> Vec<u8> {
    id3_frame(
        b"APIC",
        0,
        &[&[0], b"image/png\0".as_slice(), &[3, 0], data].concat(),
    )
}

/// A comment frame, UTF-8, language "eng".
fn id3_comment(description: &str, text: &str) -> Vec<u8> {
    id3_frame(
        b"COMM",
        0,
        &[
            &[3],
            b"eng".as_slice(),
            description.as_bytes(),
            &[0],
            text.as_bytes(),
        ]
        .concat(),
    )
}

fn id3v24(frames: &[Vec<u8>]) -> Vec<u8> {
    id3v24_padded(frames, 0)
}

fn id3v24_padded(frames: &[Vec<u8>], padding: usize) -> Vec<u8> {
    let mut body = frames.concat();
    body.resize(body.len() + padding, 0);
    [b"ID3".as_slice(), &[4, 0, 0], &synchsafe(body.len()), &body].concat()
}

/// A zlib stream of one stored deflate block.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let length = data.len() as u16;
    [
        &[0x78, 0x01, 0x01][..],
        &length.to_le_bytes(),
        &(!length).to_le_bytes(),
        data,
        &adler32(data).to_be_bytes(),
    ]
    .concat()
}

/// The CRC of Ogg pages: polynomial 0x04C11DB7, not reflected, starting at zero, bit by bit
/// as RFC 3533 defines it.
fn ogg_crc(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |crc, byte| {
        (0..8).fold(crc ^ (u32::from(*byte) << 24), |crc, _| {
            if crc & 0x8000_0000 != 0 {
                crc << 1 ^ 0x04C1_1DB7
            } else {
                crc << 1
            }
        })
    })
}

/// One Ogg page holding whole packets.
fn ogg_page(kind: u8, granule: u64, sequence: u32, packets: &[Vec<u8>]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for packet in packets {
        lacing.extend(std::iter::repeat_n(255u8, packet.len() / 255));
        lacing.push((packet.len() % 255) as u8);
    }
    let mut page = b"OggS".to_vec();
    page.push(0);
    page.push(kind);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&0x5EED_u32.to_le_bytes());
    page.extend_from_slice(&sequence.to_le_bytes());
    page.extend_from_slice(&[0; 4]);
    page.push(lacing.len() as u8);
    page.extend_from_slice(&lacing);
    for packet in packets {
        page.extend_from_slice(packet);
    }
    let crc = ogg_crc(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

/// The audio packet of the Ogg fixture: an even first byte marks an audio packet.
const OGG_AUDIO: &[u8; 20] = b"\0platter-ogg-audio-1";

fn vorbis_ident() -> Vec<u8> {
    let mut ident = vec![1];
    ident.extend_from_slice(b"vorbis");
    ident.extend_from_slice(&0u32.to_le_bytes());
    ident.push(2);
    ident.extend_from_slice(&RATE.to_le_bytes());
    ident.extend_from_slice(&[0; 12]);
    // Block sizes 256 and 2048, then the framing bit.
    ident.extend_from_slice(&[0xB8, 1]);
    ident
}

fn vorbis_comment_packet(comments: &[(&str, &str)]) -> Vec<u8> {
    let mut comment = vec![3];
    comment.extend_from_slice(b"vorbis");
    let vendor = b"platter test";
    comment.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    comment.extend_from_slice(vendor);
    comment.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for (key, value) in comments {
        let entry = format!("{key}={value}");
        comment.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        comment.extend_from_slice(entry.as_bytes());
    }
    comment.push(1);
    comment
}

fn vorbis_setup() -> Vec<u8> {
    let mut setup = vec![5];
    setup.extend_from_slice(b"vorbis");
    setup.extend_from_slice(&[0; 8]);
    setup
}

/// An Ogg Vorbis stream: identification header, comment header with `comments`, a setup
/// header without codebooks and one audio packet.
fn ogg_vorbis(comments: &[(&str, &str)]) -> Vec<u8> {
    [
        ogg_page(0x02, 0, 0, &[vorbis_ident()]),
        ogg_page(
            0x00,
            0,
            1,
            &[vorbis_comment_packet(comments), vorbis_setup()],
        ),
        ogg_page(0x04, 1_024, 2, &[OGG_AUDIO.to_vec()]),
    ]
    .concat()
}

fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&((body.len() + 8) as u32).to_be_bytes()[..], kind, body].concat()
}

fn full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    mp4_box(kind, &[&[0, 0, 0, 0][..], body].concat())
}

const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

fn be(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// The `moov` box of one AAC track whose single sample sits at `offset` in the file, with
/// the user data of [`mp4_udta`] when `udta`.
fn mp4_moov(offset: u32, sample_len: u32, udta: bool) -> Vec<u8> {
    mp4_moov_with(u64::from(offset), sample_len, udta, false, &[])
}

/// Like [`mp4_moov`], with 64-bit chunk offsets (`co64`) when `wide` and `extra` boxes at
/// the end of `moov`.
fn mp4_moov_with(offset: u64, sample_len: u32, udta: bool, wide: bool, extra: &[u8]) -> Vec<u8> {
    let offsets = if wide {
        full_box(b"co64", &[be(&[1]), offset.to_be_bytes().to_vec()].concat())
    } else {
        full_box(b"stco", &be(&[1, offset as u32]))
    };
    let mvhd = full_box(
        b"mvhd",
        &[
            be(&[0, 0, RATE, 1_024, 0x0001_0000]),
            vec![1, 0],
            vec![0; 10],
            be(&MATRIX),
            vec![0; 24],
            be(&[2]),
        ]
        .concat(),
    );
    let tkhd = mp4_box(
        b"tkhd",
        &[
            be(&[7, 0, 0, 1, 0, 1_024, 0, 0]),
            vec![0, 0, 0, 0, 1, 0, 0, 0],
            be(&MATRIX),
            be(&[0, 0]),
        ]
        .concat(),
    );
    let mdhd = full_box(
        b"mdhd",
        &[be(&[0, 0, RATE, 1_024]), vec![0x55, 0xC4, 0, 0]].concat(),
    );
    let hdlr = full_box(
        b"hdlr",
        &[be(&[0]), b"soun".to_vec(), vec![0; 12], vec![0]].concat(),
    );
    // AAC LC at 44.1 kHz in stereo (ISO/IEC 14496-3, AudioSpecificConfig).
    let decoder_info = [0x05, 2, 0x12, 0x10];
    let decoder_config = [
        &[0x04, 13 + decoder_info.len() as u8, 0x40, 0x15, 0, 0, 0][..],
        &be(&[128_000, 128_000]),
        &decoder_info,
    ]
    .concat();
    let es = [
        &[0x03, 3 + decoder_config.len() as u8 + 3, 0, 1, 0][..],
        &decoder_config,
        &[0x06, 1, 0x02],
    ]
    .concat();
    let esds = full_box(b"esds", &es);
    let mp4a = mp4_box(
        b"mp4a",
        &[
            vec![0; 6],
            vec![0, 1],
            vec![0; 8],
            vec![0, 2, 0, 16, 0, 0, 0, 0],
            be(&[RATE << 16]),
            esds,
        ]
        .concat(),
    );
    let stbl = mp4_box(
        b"stbl",
        &[
            full_box(b"stsd", &[be(&[1]), mp4a].concat()),
            full_box(b"stts", &be(&[1, 1, 1_024])),
            full_box(b"stsc", &be(&[1, 1, 1, 1])),
            full_box(b"stsz", &be(&[0, 1, sample_len])),
            offsets,
        ]
        .concat(),
    );
    // `url ` with the flag "media in this file".
    let dinf = mp4_box(
        b"dinf",
        &full_box(
            b"dref",
            &[be(&[1]), mp4_box(b"url ", &[0, 0, 0, 1])].concat(),
        ),
    );
    let minf = mp4_box(
        b"minf",
        &[full_box(b"smhd", &[0, 0, 0, 0]), dinf, stbl].concat(),
    );
    let mdia = mp4_box(b"mdia", &[mdhd, hdlr, minf].concat());
    let trak = mp4_box(b"trak", &[tkhd, mdia].concat());
    let udta = if udta { mp4_udta() } else { Vec::new() };
    mp4_box(b"moov", &[mvhd, trak, udta, extra.to_vec()].concat())
}

fn mp4_freeform(mean: &str, name: &str, value: &str) -> Vec<u8> {
    mp4_box(
        b"----",
        &[
            full_box(b"mean", mean.as_bytes()),
            full_box(b"name", name.as_bytes()),
            mp4_box(b"data", &[be(&[1, 0]), value.as_bytes().to_vec()].concat()),
        ]
        .concat(),
    )
}

/// User data with an `ilst` that holds one custom (freeform) item.
fn mp4_udta() -> Vec<u8> {
    let custom = mp4_freeform("org.platter.test", "CUSTOM", "custom kept");
    let handler = full_box(
        b"hdlr",
        &[be(&[0]), b"mdirappl".to_vec(), vec![0; 9]].concat(),
    );
    let meta = full_box(b"meta", &[handler, mp4_box(b"ilst", &custom)].concat());
    mp4_box(b"udta", &meta)
}

const MDAT_MARK: &[u8; 16] = b"platter-mdat-123";

/// An M4A file with its `moov` before `mdat`, so a larger `moov` moves the media; a `free`
/// box of `free` bytes sits between them when it is not zero.
fn m4a_with(udta: bool, free: usize) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"M4A \0\0\0\0M4A mp42isom");
    let moov_len = mp4_moov(0, 16, udta).len();
    let offset = (ftyp.len() + moov_len + free + 8) as u32;
    let free = if free == 0 {
        Vec::new()
    } else {
        mp4_box(b"free", &vec![0; free - 8])
    };
    [
        ftyp,
        mp4_moov(offset, 16, udta),
        free,
        mp4_box(b"mdat", MDAT_MARK),
    ]
    .concat()
}

fn m4a() -> Vec<u8> {
    m4a_with(true, 0)
}

/// The offset the first `stco` box of an MP4 file gives its first chunk.
fn chunk_offset(bytes: &[u8]) -> usize {
    let at = find(bytes, b"stco").unwrap();
    u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn png() -> Vec<u8> {
    rgba_png(2, 2, |x, y| [x as u8 * 90, y as u8 * 90, 40, 255])
}

/// An ID3v1 tag: 128 bytes at the end of the file, with artist, album and comment empty and
/// the genre "none".
fn id3v1(title: &str, year: &[u8; 4]) -> Vec<u8> {
    let mut tag = b"TAG".to_vec();
    let mut field = title.as_bytes().to_vec();
    field.resize(30, 0);
    tag.extend_from_slice(&field);
    tag.extend_from_slice(&[0; 30 + 30]);
    tag.extend_from_slice(year);
    tag.extend_from_slice(&[0; 30]);
    tag.push(255);
    tag
}

/// A RIFF or IFF chunk, padded to an even length.
fn chunk(id: &[u8; 4], body: &[u8], big_endian: bool) -> Vec<u8> {
    let size = body.len() as u32;
    let size = if big_endian {
        size.to_be_bytes()
    } else {
        size.to_le_bytes()
    };
    let mut out = [id.as_slice(), &size, body].concat();
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// `file` with `extra` chunks appended and the size of its outer chunk updated.
fn with_chunks(mut file: Vec<u8>, extra: &[u8], big_endian: bool) -> Vec<u8> {
    file.extend_from_slice(extra);
    let size = (file.len() - 8) as u32;
    let size = if big_endian {
        size.to_be_bytes()
    } else {
        size.to_le_bytes()
    };
    file[4..8].copy_from_slice(&size);
    file
}

// ---------------------------------------------------------------------------------------
// Reading back, separately from the writer

#[derive(Clone, Copy, Debug, PartialEq)]
enum F {
    Title,
    Artist,
    Album,
    Genre,
    Comment,
    Key,
    Bpm,
    Platter,
}

const ALL: [F; 8] = [
    F::Title,
    F::Artist,
    F::Album,
    F::Genre,
    F::Comment,
    F::Key,
    F::Bpm,
    F::Platter,
];

/// Text in an ID3v2 encoding: ISO-8859-1, UTF-16 with a byte order mark, UTF-8.
fn id3_decode(encoding: u8, bytes: &[u8]) -> String {
    match encoding {
        0 => bytes.iter().map(|byte| char::from(*byte)).collect(),
        1 => {
            let units: Vec<u16> = bytes[2..]
                .chunks(2)
                .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                .collect();
            assert_eq!(&bytes[..2], [0xFF, 0xFE], "little-endian byte order mark");
            String::from_utf16(&units).unwrap()
        }
        _ => String::from_utf8(bytes.to_vec()).unwrap(),
    }
}

/// The frames of an ID3v2.3 or 2.4 tag that is at the start of `tag`.
fn id3_frames(tag: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(&tag[..3], b"ID3");
    let version = tag[3];
    let size = tag[6..10]
        .iter()
        .fold(0usize, |size, byte| size << 7 | usize::from(*byte));
    let body = &tag[10..10 + size];
    let mut frames = Vec::new();
    let mut at = 0;
    while at + 10 <= body.len() && body[at] != 0 {
        let id = String::from_utf8(body[at..at + 4].to_vec()).unwrap();
        let len = if version == 4 {
            body[at + 4..at + 8]
                .iter()
                .fold(0usize, |size, byte| size << 7 | usize::from(*byte))
        } else {
            u32::from_be_bytes(body[at + 4..at + 8].try_into().unwrap()) as usize
        };
        frames.push((id, body[at + 10..at + 10 + len].to_vec()));
        at += 10 + len;
    }
    frames
}

/// The description and the text of a comment or user text frame; `skip` is 3 for the
/// language of a comment.
fn id3_described(body: &[u8], skip: usize) -> (String, String) {
    let encoding = body[0];
    let rest = &body[1 + skip..];
    let end = if encoding == 1 {
        rest.chunks(2).position(|unit| unit == [0, 0]).unwrap() * 2
    } else {
        rest.iter().position(|byte| *byte == 0).unwrap()
    };
    let gap = if encoding == 1 { 2 } else { 1 };
    let description = if end == 0 {
        String::new()
    } else {
        id3_decode(encoding, &rest[..end])
    };
    (description, id3_decode(encoding, &rest[end + gap..]))
}

fn id3_field(tag: &[u8], field: F) -> Option<String> {
    let id = match field {
        F::Title => "TIT2",
        F::Artist => "TPE1",
        F::Album => "TALB",
        F::Genre => "TCON",
        F::Comment => "COMM",
        F::Key => "TKEY",
        F::Bpm => "TBPM",
        F::Platter => "TXXX",
    };
    id3_frames(tag)
        .into_iter()
        .filter(|(found, _)| found == id)
        .find_map(|(_, body)| match field {
            F::Comment => {
                let (description, text) = id3_described(&body, 3);
                description.is_empty().then_some(text)
            }
            F::Platter => {
                let (description, text) = id3_described(&body, 0);
                (description == "PLATTER").then_some(text)
            }
            _ => Some(id3_decode(body[0], &body[1..])),
        })
}

/// The chunks of a RIFF or FORM file: identifier and body.
fn riff_chunks(bytes: &[u8], big_endian: bool) -> Vec<([u8; 4], Vec<u8>)> {
    let mut chunks = Vec::new();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        let size: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
        let size = if big_endian {
            u32::from_be_bytes(size)
        } else {
            u32::from_le_bytes(size)
        } as usize;
        chunks.push((id, bytes[at + 8..at + 8 + size].to_vec()));
        at += 8 + size + size % 2;
    }
    chunks
}

fn riff_chunk(bytes: &[u8], ids: &[&[u8; 4]], big_endian: bool) -> Option<Vec<u8>> {
    riff_chunks(bytes, big_endian)
        .into_iter()
        .find(|(id, _)| ids.contains(&id))
        .map(|(_, body)| body)
}

fn zero_terminated(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).unwrap()
}

fn wav_field(bytes: &[u8], field: F) -> Option<String> {
    let id: &[u8; 4] = match field {
        F::Title => b"INAM",
        F::Artist => b"IART",
        F::Album => b"IPRD",
        F::Genre => b"IGNR",
        F::Comment => b"ICMT",
        _ => return id3_field(&riff_chunk(bytes, &[b"id3 ", b"ID3 "], false)?, field),
    };
    let list = riff_chunks(bytes, false)
        .into_iter()
        .find(|(id, body)| id == b"LIST" && body.starts_with(b"INFO"))?
        .1;
    let mut at = 4;
    while at + 8 <= list.len() {
        let size = u32::from_le_bytes(list[at + 4..at + 8].try_into().unwrap()) as usize;
        if &list[at..at + 4] == id {
            return Some(zero_terminated(&list[at + 8..at + 8 + size]));
        }
        at += 8 + size + size % 2;
    }
    None
}

fn aiff_field(bytes: &[u8], field: F) -> Option<String> {
    let id: &[u8; 4] = match field {
        F::Title => b"NAME",
        F::Artist => b"AUTH",
        F::Comment => b"ANNO",
        _ => return id3_field(&riff_chunk(bytes, &[b"ID3 ", b"id3 "], true)?, field),
    };
    riff_chunk(bytes, &[id], true).map(|body| String::from_utf8(body).unwrap())
}

/// The fields of a Vorbis comment (without the framing bit of Ogg).
fn vorbis_fields(comment: &[u8]) -> Vec<String> {
    let vendor = u32::from_le_bytes(comment[..4].try_into().unwrap()) as usize;
    let mut at = 4 + vendor;
    let count = u32::from_le_bytes(comment[at..at + 4].try_into().unwrap());
    at += 4;
    let mut fields = Vec::new();
    for _ in 0..count {
        let len = u32::from_le_bytes(comment[at..at + 4].try_into().unwrap()) as usize;
        fields.push(String::from_utf8(comment[at + 4..at + 4 + len].to_vec()).unwrap());
        at += 4 + len;
    }
    fields
}

fn vorbis_field(fields: &[String], field: F) -> Option<String> {
    let key = match field {
        F::Title => "TITLE",
        F::Artist => "ARTIST",
        F::Album => "ALBUM",
        F::Genre => "GENRE",
        F::Comment => "COMMENT",
        F::Key => "INITIALKEY",
        F::Bpm => "BPM",
        F::Platter => "PLATTER",
    };
    fields.iter().find_map(|entry| {
        let (name, value) = entry.split_once('=')?;
        name.eq_ignore_ascii_case(key).then(|| value.to_owned())
    })
}

/// The metadata blocks of a FLAC file: type, last flag and body.
fn flac_blocks(bytes: &[u8]) -> Vec<(u8, bool, Vec<u8>)> {
    let mut at = if bytes.starts_with(b"ID3") {
        10 + bytes[6..10]
            .iter()
            .fold(0usize, |size, byte| size << 7 | usize::from(*byte))
    } else {
        0
    };
    assert_eq!(&bytes[at..at + 4], b"fLaC");
    at += 4;
    let mut blocks = Vec::new();
    loop {
        let header = &bytes[at..at + 4];
        let len = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
        let last = header[0] & 0x80 != 0;
        blocks.push((header[0] & 0x7F, last, bytes[at + 4..at + 4 + len].to_vec()));
        at += 4 + len;
        if last {
            return blocks;
        }
    }
}

fn flac_field(bytes: &[u8], field: F) -> Option<String> {
    let comment = flac_blocks(bytes)
        .into_iter()
        .find(|(kind, _, _)| *kind == 4)?
        .2;
    vorbis_field(&vorbis_fields(&comment), field)
}

struct TestPage {
    kind: u8,
    granule: u64,
    sequence: u32,
    lacing: Vec<u8>,
    body: Vec<u8>,
}

/// The pages of an Ogg file, each checked against its CRC.
fn ogg_pages(bytes: &[u8]) -> Vec<TestPage> {
    let mut pages = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        assert_eq!(&bytes[at..at + 4], b"OggS");
        let segments = usize::from(bytes[at + 26]);
        let lacing = bytes[at + 27..at + 27 + segments].to_vec();
        let body_len: usize = lacing.iter().map(|value| usize::from(*value)).sum();
        let end = at + 27 + segments + body_len;
        let mut page = bytes[at..end].to_vec();
        let stored = u32::from_le_bytes(page[22..26].try_into().unwrap());
        page[22..26].copy_from_slice(&[0; 4]);
        assert_eq!(ogg_crc(&page), stored, "CRC of the page at {at}");
        pages.push(TestPage {
            kind: bytes[at + 5],
            granule: u64::from_le_bytes(bytes[at + 6..at + 14].try_into().unwrap()),
            sequence: u32::from_le_bytes(bytes[at + 18..at + 22].try_into().unwrap()),
            body: bytes[at + 27 + segments..end].to_vec(),
            lacing,
        });
        at = end;
    }
    pages
}

/// The packets of a one-stream Ogg file, after checking the sequence numbers run on.
fn ogg_packets(bytes: &[u8]) -> Vec<Vec<u8>> {
    let pages = ogg_pages(bytes);
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.sequence as usize, index, "page sequence");
    }
    let mut packets = Vec::new();
    let mut packet = Vec::new();
    for page in &pages {
        let mut at = 0;
        for value in &page.lacing {
            let len = usize::from(*value);
            packet.extend_from_slice(&page.body[at..at + len]);
            at += len;
            if len < 255 {
                packets.push(std::mem::take(&mut packet));
            }
        }
    }
    packets
}

fn ogg_field(bytes: &[u8], field: F) -> Option<String> {
    let comment = &ogg_packets(bytes)[1];
    assert_eq!(&comment[..7], b"\x03vorbis");
    assert_eq!(comment.last(), Some(&1), "framing bit");
    vorbis_field(&vorbis_fields(&comment[7..comment.len() - 1]), field)
}

/// The boxes inside `bytes`: kind and whole box.
fn mp4_boxes(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut boxes = Vec::new();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        boxes.push((
            bytes[at + 4..at + 8].try_into().unwrap(),
            &bytes[at..at + size],
        ));
        at += size;
    }
    boxes
}

fn mp4_child<'a>(parent: &'a [u8], kind: &[u8; 4], skip: usize) -> Option<&'a [u8]> {
    mp4_boxes(&parent[8 + skip..])
        .into_iter()
        .find(|(found, _)| found == kind)
        .map(|(_, bytes)| bytes)
}

/// The items of `moov/udta/meta/ilst`.
fn mp4_items(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let Some(moov) = mp4_boxes(bytes)
        .into_iter()
        .find(|(kind, _)| kind == b"moov")
    else {
        return Vec::new();
    };
    let Some(ilst) = mp4_child(moov.1, b"udta", 0)
        .and_then(|udta| mp4_child(udta, b"meta", 0))
        .and_then(|meta| mp4_child(meta, b"ilst", 4))
    else {
        return Vec::new();
    };
    mp4_boxes(&ilst[8..])
        .into_iter()
        .map(|(kind, bytes)| (kind, bytes.to_vec()))
        .collect()
}

/// The value of the `data` box of an item.
fn mp4_value(item: &[u8]) -> Vec<u8> {
    let data = mp4_boxes(&item[8..])
        .into_iter()
        .find(|(kind, _)| kind == b"data")
        .unwrap()
        .1;
    data[16..].to_vec()
}

fn mp4_freeform_value(bytes: &[u8], mean: &str, name: &str) -> Option<String> {
    mp4_items(bytes).into_iter().find_map(|(kind, item)| {
        if &kind != b"----" {
            return None;
        }
        let children = mp4_boxes(&item[8..]);
        let text = |wanted: &[u8; 4]| {
            children
                .iter()
                .find(|(kind, _)| kind == wanted)
                .map(|(_, bytes)| bytes[12..].to_vec())
        };
        (text(b"mean")? == mean.as_bytes() && text(b"name")? == name.as_bytes())
            .then(|| String::from_utf8(mp4_value(&item)).unwrap())
    })
}

fn m4a_field(bytes: &[u8], field: F) -> Option<String> {
    let fourcc: &[u8; 4] = match field {
        F::Title => b"\xA9nam",
        F::Artist => b"\xA9ART",
        F::Album => b"\xA9alb",
        F::Genre => b"\xA9gen",
        F::Comment => b"\xA9cmt",
        F::Key => return mp4_freeform_value(bytes, "com.apple.iTunes", "initialkey"),
        F::Platter => return mp4_freeform_value(bytes, "org.platter-dj", "PLATTER"),
        F::Bpm => b"tmpo",
    };
    let item = mp4_items(bytes)
        .into_iter()
        .find(|(kind, _)| kind == fourcc)?
        .1;
    let value = mp4_value(&item);
    Some(if field == F::Bpm {
        i16::from_be_bytes(value.try_into().unwrap()).to_string()
    } else {
        String::from_utf8(value).unwrap()
    })
}

fn mp3_field(bytes: &[u8], field: F) -> Option<String> {
    bytes
        .starts_with(b"ID3")
        .then(|| id3_field(bytes, field))
        .flatten()
}

type Reader = fn(&[u8], F) -> Option<String>;

const PLATTER_DATA: &str = "cues=1:0.5,2:31.25\ngrid=126.5@0.012";

fn full_edit() -> TagEdit {
    TagEdit {
        title: Some("Night Drive".to_owned()),
        artist: Some("The Testers".to_owned()),
        album: Some("Signals".to_owned()),
        genre: Some("Techno".to_owned()),
        comment: Some("Peak time".to_owned()),
        key: Some("8A".to_owned()),
        bpm: Some(Some(126.5)),
        platter: Some(Some(PLATTER_DATA.to_owned())),
        cover: None,
    }
}

/// What Platter's own reader shows of a format.
#[derive(Clone, Copy, PartialEq)]
enum Shows {
    Nothing,
    /// Title, artist, album, genre and tempo, the tempo as the format holds it.
    Fields(f64),
}

/// Writes the full edit into a file and checks it reads back, then changes and clears
/// fields.
fn round_trip(name: &str, bytes: &[u8], read: Reader, shows: Shows) -> PathBuf {
    let folder = folder(name);
    let path = file(&folder, name, bytes);
    let before = probe_file(&path).unwrap();

    write_tags(&path, &full_edit()).unwrap();
    let after = probe_file(&path).unwrap();
    assert_eq!(after.container, before.container, "{name}");
    assert_eq!(after.sample_rate, before.sample_rate, "{name}");
    assert_eq!(after.channels, before.channels, "{name}");
    if let Shows::Fields(tempo) = shows {
        assert_eq!(after.tags.title.as_deref(), Some("Night Drive"), "{name}");
        assert_eq!(after.tags.artist.as_deref(), Some("The Testers"), "{name}");
        assert_eq!(after.tags.album.as_deref(), Some("Signals"), "{name}");
        assert_eq!(after.tags.genre.as_deref(), Some("Techno"), "{name}");
        assert_eq!(after.tags.bpm, Some(tempo), "{name}");
    }
    check_full_edit(name, &path, read);
    check_second_edit(name, &path, read, shows);

    // Removing every field leaves none of them.
    write_tags(
        &path,
        &TagEdit {
            title: Some(String::new()),
            artist: Some(String::new()),
            album: Some(String::new()),
            genre: Some(String::new()),
            comment: Some(String::new()),
            key: Some(String::new()),
            bpm: Some(None),
            platter: Some(None),
            cover: None,
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    for field in ALL {
        assert_eq!(read(&written, field), None, "{name} {field:?}");
    }
    assert_eq!(probe_file(&path).unwrap().container, before.container);
    assert_no_copy_left(&folder);
    path
}

/// The fields of [`full_edit`], read back from the file. Vorbis comments keep two decimals
/// of the tempo; the other formats hold a whole number.
fn check_full_edit(name: &str, path: &Path, read: Reader) {
    let written = fs::read(path).unwrap();
    let tempo = if name.ends_with(".flac") || name.ends_with(".ogg") {
        "126.5"
    } else {
        "127"
    };
    assert_eq!(read(&written, F::Bpm).as_deref(), Some(tempo), "{name}");
    for (field, value) in [
        (F::Title, "Night Drive"),
        (F::Artist, "The Testers"),
        (F::Album, "Signals"),
        (F::Genre, "Techno"),
        (F::Comment, "Peak time"),
        (F::Key, "8A"),
        (F::Platter, PLATTER_DATA),
    ] {
        assert_eq!(
            read(&written, field).as_deref(),
            Some(value),
            "{name} {field:?}"
        );
    }
}

/// A second edit changes two fields, clears two and leaves the rest.
fn check_second_edit(name: &str, path: &Path, read: Reader, shows: Shows) {
    write_tags(
        path,
        &TagEdit {
            title: Some("Night Drive (Edit)".to_owned()),
            genre: Some("  ".to_owned()),
            bpm: Some(None),
            platter: Some(Some("v2".to_owned())),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(path).unwrap();
    assert_eq!(
        read(&written, F::Title).as_deref(),
        Some("Night Drive (Edit)"),
        "{name}"
    );
    assert_eq!(read(&written, F::Genre), None, "{name}");
    assert_eq!(read(&written, F::Bpm), None, "{name}");
    assert_eq!(read(&written, F::Platter).as_deref(), Some("v2"), "{name}");
    for (field, value) in [
        (F::Artist, "The Testers"),
        (F::Album, "Signals"),
        (F::Comment, "Peak time"),
        (F::Key, "8A"),
    ] {
        assert_eq!(
            read(&written, field).as_deref(),
            Some(value),
            "{name} {field:?}"
        );
    }
    if shows != Shows::Nothing {
        let tags = probe_file(path).unwrap().tags;
        assert_eq!(tags.title.as_deref(), Some("Night Drive (Edit)"), "{name}");
        assert_eq!(tags.artist.as_deref(), Some("The Testers"), "{name}");
        assert_eq!(tags.bpm, None, "{name}");
    }
}

fn assert_no_copy_left(folder: &Path) {
    let names: Vec<_> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|name| !name.starts_with(".platter-tags-")),
        "{names:?}"
    );
}

fn title(text: &str) -> TagEdit {
    TagEdit {
        title: Some(text.to_owned()),
        ..TagEdit::default()
    }
}

// ---------------------------------------------------------------------------------------
// Each format

#[test]
fn writes_the_tags_of_a_wav_file() {
    let bytes = wav_pcm16(RATE, 2, &sine_pcm());
    let reference = decode_file(&file(&folder("wav-reference"), "a.wav", &bytes)).unwrap();
    // The decoding library in use shows no tag of a WAV file.
    let path = round_trip("a.wav", &bytes, wav_field, Shows::Nothing);
    assert_eq!(decode_file(&path).unwrap().samples, reference.samples);
}

#[test]
fn writes_the_tags_of_an_aiff_file() {
    let bytes = aiff_pcm16(RATE, 2, &sine_pcm());
    let reference = decode_file(&file(&folder("aiff-reference"), "a.aiff", &bytes)).unwrap();
    // The ID3 chunk the edit creates is what Platter's reader shows.
    let path = round_trip("a.aiff", &bytes, aiff_field, Shows::Fields(127.0));
    assert_eq!(decode_file(&path).unwrap().samples, reference.samples);
}

#[test]
fn writes_the_tags_of_a_flac_file() {
    let bytes = flac_pcm16(RATE, 2, &sine_pcm());
    let reference = decode_file(&file(&folder("flac-reference"), "a.flac", &bytes)).unwrap();
    let path = round_trip("a.flac", &bytes, flac_field, Shows::Fields(126.5));
    assert_eq!(decode_file(&path).unwrap().samples, reference.samples);
}

#[test]
fn writes_the_tags_of_an_mp3_file() {
    let bytes = silent_mp3(20);
    let path = round_trip("a.mp3", &bytes, mp3_field, Shows::Fields(127.0));
    let written = fs::read(&path).unwrap();
    assert!(written.starts_with(b"ID3\x04"), "ID3v2.4 at the start");
    assert!(
        written.ends_with(&bytes),
        "the MPEG frames stay as they were"
    );
}

#[test]
fn writes_the_tags_of_an_ogg_vorbis_file() {
    let path = round_trip(
        "a.ogg",
        &ogg_vorbis(&[("TITLE", "Old title")]),
        ogg_field,
        Shows::Fields(126.5),
    );
    let packets = ogg_packets(&fs::read(&path).unwrap());
    assert_eq!(packets[0], vorbis_ident());
    assert_eq!(packets[2], vorbis_setup());
    assert_eq!(packets[3], OGG_AUDIO);
}

#[test]
fn writes_the_tags_of_an_m4a_file() {
    let bytes = m4a();
    assert_eq!(&bytes[chunk_offset(&bytes)..][..16], MDAT_MARK);
    let path = round_trip("a.m4a", &bytes, m4a_field, Shows::Fields(127.0));
    // The round trip ends with the tempo removed; set it again to look at the atom.
    write_tags(
        &path,
        &TagEdit {
            bpm: Some(Some(126.5)),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    // The tempo atom holds a 16-bit integer of data type 21, written out by hand.
    let tempo = find(&written, b"tmpo").unwrap();
    let atom = &written[tempo + 4..tempo + 22];
    assert_eq!(atom, b"\0\0\0\x12data\0\0\0\x15\0\0\0\0\0\x7F");
    assert!(find(&written, b"custom kept").is_some());

    // An edit that does not name the tempo leaves the one atom as it was.
    write_tags(
        &path,
        &TagEdit {
            album: Some("Other".to_owned()),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    let tempo = find(&written, b"tmpo").unwrap();
    assert_eq!(&written[tempo + 4..tempo + 22], atom);
    assert_eq!(
        written
            .windows(4)
            .filter(|window| *window == b"tmpo")
            .count(),
        1
    );
    assert!(find(&written, b"custom kept").is_some());
    assert_eq!(probe_file(&path).unwrap().tags.bpm, Some(127.0));
    // The metadata grew `moov`, which sits before the media: the chunk offset follows it.
    assert!(written.len() > bytes.len());
    assert_eq!(&written[chunk_offset(&written)..][..16], MDAT_MARK);
}

#[test]
fn an_m4a_file_without_user_data_gets_it() {
    let bytes = m4a_with(false, 0);
    let folder = folder("m4a-no-udta");
    let path = file(&folder, "a.m4a", &bytes);
    write_tags(&path, &full_edit()).unwrap();
    let written = fs::read(&path).unwrap();
    assert_eq!(
        m4a_field(&written, F::Title).as_deref(),
        Some("Night Drive")
    );
    assert_eq!(m4a_field(&written, F::Key).as_deref(), Some("8A"));
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Night Drive")
    );
    assert_eq!(&written[chunk_offset(&written)..][..16], MDAT_MARK);
}

#[test]
fn a_free_box_after_moov_takes_up_the_change() {
    let bytes = m4a_with(true, 600);
    let offset = chunk_offset(&bytes);
    let folder = folder("m4a-free");
    let path = file(&folder, "a.m4a", &bytes);
    write_tags(&path, &full_edit()).unwrap();
    let written = fs::read(&path).unwrap();
    // The file keeps its length and the media its place: no offset had to move.
    assert_eq!(written.len(), bytes.len());
    assert_eq!(chunk_offset(&written), offset);
    assert_eq!(&written[offset..][..16], MDAT_MARK);
    assert_eq!(
        m4a_field(&written, F::Title).as_deref(),
        Some("Night Drive")
    );
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Night Drive")
    );
    // What is left of the free box is zeros.
    let free = mp4_boxes(&written)
        .into_iter()
        .find(|(kind, _)| kind == b"free")
        .unwrap()
        .1;
    assert!(free.len() >= 8 && free[8..].iter().all(|byte| *byte == 0));

    // A shorter edit gives the bytes back to the free box.
    write_tags(&path, &title("X")).unwrap();
    let shorter = fs::read(&path).unwrap();
    assert_eq!(shorter.len(), bytes.len());
    assert_eq!(chunk_offset(&shorter), offset);
}

#[test]
fn an_ogg_comment_that_grows_past_a_page_renumbers_the_pages() {
    // The comment header and the setup header fill one page to its 255 lacing values:
    // 253 full segments and one of 254 bytes for the comment, one for the setup.
    let filler = "x".repeat(64_725);
    let comment = vorbis_comment_packet(&[("PLATTER_BIG", &filler)]);
    assert_eq!(comment.len(), 254 * 255 - 1);
    let bytes = [
        ogg_page(0x02, 0, 0, &[vorbis_ident()]),
        ogg_page(0x00, 0, 1, &[comment.clone(), vorbis_setup()]),
        ogg_page(0x04, 1_024, 2, &[OGG_AUDIO.to_vec()]),
    ]
    .concat();
    assert_eq!(ogg_pages(&bytes).len(), 3);
    let folder = folder("ogg-pages");
    let path = file(&folder, "a.ogg", &bytes);

    write_tags(&path, &title("Night Drive")).unwrap();
    let written = fs::read(&path).unwrap();
    let pages = ogg_pages(&written);
    assert_eq!(pages.len(), 4, "the headers now take two pages");
    // The comment header fills the first header page; the setup header starts the second.
    assert_eq!((pages[1].lacing.len(), pages[2].lacing.len()), (255, 1));
    assert_eq!(pages[2].kind, 0x00);
    assert_eq!(pages[1].granule, 0);
    assert_eq!(pages[2].granule, 0);
    // The audio page keeps its flags and granule position under its new number.
    assert_eq!(
        (pages[3].kind, pages[3].granule, pages[3].sequence),
        (0x04, 1_024, 3)
    );
    let packets = ogg_packets(&written);
    assert_eq!(packets.len(), 4);
    assert_eq!(packets[0], vorbis_ident());
    assert_eq!(packets[2], vorbis_setup());
    assert_eq!(packets[3], OGG_AUDIO);
    assert_eq!(
        ogg_field(&written, F::Title).as_deref(),
        Some("Night Drive")
    );
    assert_eq!(ogg_field(&written, F::Platter), None);
    assert!(find(&written, filler.as_bytes()).is_some());
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Night Drive")
    );

    // Removing the title brings the headers back to one page and the pages to their numbers.
    write_tags(&path, &title("")).unwrap();
    let written = fs::read(&path).unwrap();
    assert_eq!(written, bytes);
}

// ---------------------------------------------------------------------------------------
// Bytes written out by hand from the specifications

#[test]
fn id3v2_frames_are_written_as_the_specification_lays_them_out() {
    // Tag of 54 bytes after its header: a title frame of 14 and 40 of padding.
    let old = id3v24_padded(&[id3_text(b"TIT2", "Old")], 40);
    let folder = folder("id3-bytes");
    let path = file(&folder, "a.mp3", &[old, silent_mp3(4)].concat());
    write_tags(
        &path,
        &TagEdit {
            title: Some("Night Drive".to_owned()),
            platter: Some(Some("data".to_owned())),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    let expected: Vec<u8> = [
        // Header: version 2.4.0, no flags, size 54 in synchsafe bytes.
        b"ID3\x04\x00\x00\x00\x00\x00\x36".as_slice(),
        // TIT2, size 12, no flags, UTF-8, then the text.
        b"TIT2\x00\x00\x00\x0C\x00\x00\x03Night Drive",
        // TXXX, size 13: UTF-8, the description and its terminator, then the value.
        b"TXXX\x00\x00\x00\x0D\x00\x00\x03PLATTER\x00data",
        // 54 - 22 - 23 = 9 bytes of padding.
        &[0; 9],
    ]
    .concat();
    assert_eq!(&written[..expected.len()], expected.as_slice());
    assert_eq!(&written[expected.len()..], silent_mp3(4).as_slice());
}

#[test]
fn an_id3v23_tag_stays_version_3_with_utf16_text() {
    // ID3v2.3: frame sizes are plain 32-bit numbers.
    let frame = [b"TIT2".as_slice(), &4u32.to_be_bytes(), &[0, 0, 0], b"Old"].concat();
    let tag = [
        b"ID3\x03\x00\x00".as_slice(),
        &synchsafe(frame.len()),
        &frame,
    ]
    .concat();
    let folder = folder("id3v23");
    let path = file(&folder, "a.mp3", &[tag, silent_mp3(4)].concat());
    write_tags(&path, &title("Nuit \u{263E}")).unwrap();
    let written = fs::read(&path).unwrap();
    assert_eq!(&written[..4], b"ID3\x03");
    // UTF-16 with a little-endian byte order mark: 1 + 2 + 6 * 2 = 15 bytes.
    let frame = b"TIT2\x00\x00\x00\x0F\x00\x00\x01\xFF\xFEN\x00u\x00i\x00t\x00 \x00\x3E\x26";
    assert_eq!(&written[10..10 + frame.len()], frame.as_slice());
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Nuit \u{263E}")
    );
}

#[test]
fn a_flac_comment_block_is_written_as_the_specification_lays_it_out() {
    let bytes = flac_pcm16(RATE, 2, &sine_pcm());
    let folder = folder("flac-bytes");
    let path = file(&folder, "a.flac", &bytes);
    write_tags(&path, &title("Night Drive")).unwrap();
    let written = fs::read(&path).unwrap();
    // STREAMINFO is no longer the last block.
    assert_eq!(written[4], 0x00);
    assert_eq!(&written[5..42], &bytes[5..42]);
    let expected: Vec<u8> = [
        // Block type 4, 29 bytes.
        b"\x04\x00\x00\x1D".as_slice(),
        // Empty vendor string, one field of 17 bytes, little-endian lengths.
        b"\x00\x00\x00\x00\x01\x00\x00\x00\x11\x00\x00\x00TITLE=Night Drive",
        // Last block: padding of 1024 bytes.
        b"\x81\x00\x04\x00",
    ]
    .concat();
    assert_eq!(&written[42..42 + expected.len()], expected.as_slice());
    let rest = &written[42 + expected.len()..];
    assert!(rest[..1024].iter().all(|byte| *byte == 0));
    assert_eq!(&rest[1024..], &bytes[42..]);
}

#[test]
fn an_ogg_comment_page_is_written_as_the_specification_lays_it_out() {
    let bytes = ogg_vorbis(&[("TITLE", "Old title")]);
    let folder = folder("ogg-bytes");
    let path = file(&folder, "a.ogg", &bytes);
    write_tags(&path, &title("Night Drive")).unwrap();
    let written = fs::read(&path).unwrap();
    let first = 27 + 1 + vorbis_ident().len();
    assert_eq!(&written[..first], &bytes[..first]);
    let comment = b"\x03vorbis\x0C\x00\x00\x00platter test\x01\x00\x00\x00\x11\x00\x00\x00TITLE=Night Drive\x01";
    let mut page: Vec<u8> = [
        // Version 0, no flags, granule position 0, the serial of the fixture, page 1, the
        // CRC field zeroed, two lacing values.
        b"OggS\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xED\x5E\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x02".as_slice(),
        &[comment.len() as u8, 15],
        comment.as_slice(),
        &vorbis_setup(),
    ]
    .concat();
    let crc = ogg_crc(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    assert_eq!(&written[first..first + page.len()], page.as_slice());
    assert_eq!(
        &written[first + page.len()..],
        &bytes[bytes.len() - (27 + 1 + 20)..]
    );
}

#[test]
fn m4a_items_are_written_as_the_specification_lays_them_out() {
    let folder = folder("m4a-bytes");
    let path = file(&folder, "a.m4a", &m4a());
    write_tags(
        &path,
        &TagEdit {
            title: Some("Night Drive".to_owned()),
            platter: Some(Some("data".to_owned())),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    // Item of 35 bytes holding a data box of 27: type 1 (UTF-8), locale 0, then the text.
    let name =
        b"\x00\x00\x00\x23\xA9nam\x00\x00\x00\x1Bdata\x00\x00\x00\x01\x00\x00\x00\x00Night Drive";
    assert!(find(&written, name).is_some());
    let platter: Vec<u8> = [
        b"\x00\x00\x00\x49----".as_slice(),
        b"\x00\x00\x00\x1Amean\x00\x00\x00\x00org.platter-dj",
        b"\x00\x00\x00\x13name\x00\x00\x00\x00PLATTER",
        b"\x00\x00\x00\x14data\x00\x00\x00\x01\x00\x00\x00\x00data",
    ]
    .concat();
    assert!(find(&written, &platter).is_some());
}

#[test]
fn riff_and_aiff_chunks_are_written_as_the_specifications_lay_them_out() {
    let folder = folder("riff-bytes");
    let wav = wav_pcm16(RATE, 2, &sine_pcm());
    let path = file(&folder, "a.wav", &wav);
    write_tags(&path, &title("Night Drive")).unwrap();
    let written = fs::read(&path).unwrap();
    let list = b"LIST\x18\x00\x00\x00INFOINAM\x0C\x00\x00\x00Night Drive\x00";
    assert_eq!(written.len(), wav.len() + list.len());
    assert_eq!(&written[wav.len()..], list.as_slice());
    let size = u32::from_le_bytes(written[4..8].try_into().unwrap()) as usize;
    assert_eq!(size, written.len() - 8);
    // A title alone needs no ID3 chunk.
    assert!(find(&written, b"id3 ").is_none());

    let aiff = aiff_pcm16(RATE, 2, &sine_pcm());
    let path = file(&folder, "a.aiff", &aiff);
    write_tags(
        &path,
        &TagEdit {
            title: Some("Night Drive".to_owned()),
            bpm: Some(Some(127.0)),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    // Eleven bytes of text and a pad byte.
    let name = b"NAME\x00\x00\x00\x0BNight Drive\x00";
    // A chunk of 46 bytes: an ID3v2.4 tag with the title it mirrors and the tempo, no
    // padding.
    let id3: Vec<u8> = [
        b"ID3 \x00\x00\x00\x2EID3\x04\x00\x00\x00\x00\x00\x24".as_slice(),
        b"TIT2\x00\x00\x00\x0C\x00\x00\x03Night Drive",
        b"TBPM\x00\x00\x00\x04\x00\x00\x03127",
    ]
    .concat();
    assert_eq!(
        &written[aiff.len()..],
        [name.as_slice(), &id3].concat().as_slice()
    );
    let size = u32::from_be_bytes(written[4..8].try_into().unwrap()) as usize;
    assert_eq!(size, written.len() - 8);
    let tags = probe_file(&path).unwrap().tags;
    assert_eq!(
        (tags.title.as_deref(), tags.bpm),
        (Some("Night Drive"), Some(127.0))
    );
}

// ---------------------------------------------------------------------------------------
// Platter's own field

#[test]
fn platter_data_is_set_replaced_and_removed_in_every_format() {
    let other_id3 = id3_txxx("OTHER_APP", "other kept");
    let cases: Vec<(&str, Vec<u8>, Reader, &[u8])> = vec![
        (
            "a.mp3",
            [id3v24(std::slice::from_ref(&other_id3)), silent_mp3(20)].concat(),
            mp3_field,
            b"other kept",
        ),
        (
            "a.flac",
            flac_pcm16_tagged(RATE, 2, &sine_pcm(), &[("OTHER_APP", "other kept")]),
            flac_field,
            b"OTHER_APP=other kept",
        ),
        (
            "a.ogg",
            ogg_vorbis(&[("OTHER_APP", "other kept")]),
            ogg_field,
            b"OTHER_APP=other kept",
        ),
        ("a.m4a", m4a(), m4a_field, b"custom kept"),
        (
            "a.wav",
            with_chunks(
                wav_pcm16(RATE, 2, &sine_pcm()),
                &chunk(b"id3 ", &id3v24(std::slice::from_ref(&other_id3)), false),
                false,
            ),
            wav_field,
            b"other kept",
        ),
        (
            "a.aiff",
            with_chunks(
                aiff_pcm16(RATE, 2, &sine_pcm()),
                &chunk(b"ID3 ", &id3v24(&[other_id3]), true),
                true,
            ),
            aiff_field,
            b"other kept",
        ),
    ];
    for (name, bytes, read, other) in cases {
        let folder = folder(&format!("platter-{name}"));
        let path = file(&folder, name, &bytes);
        for value in [Some("one"), Some("two\nlines\twith tabs  "), Some(""), None] {
            write_tags(
                &path,
                &TagEdit {
                    platter: Some(value.map(str::to_owned)),
                    ..TagEdit::default()
                },
            )
            .unwrap();
            let written = fs::read(&path).unwrap();
            assert_eq!(read(&written, F::Platter).as_deref(), value, "{name}");
            assert!(find(&written, other).is_some(), "{name}: other data kept");
            probe_file(&path).unwrap();
        }
        assert_no_copy_left(&folder);
    }
}

#[test]
fn platter_data_over_the_bound_is_refused_before_the_file_is_opened() {
    let path = folder("platter-long").join("missing.flac");
    let edit = TagEdit {
        platter: Some(Some("x".repeat(MAX_PLATTER_BYTES + 1))),
        ..TagEdit::default()
    };
    assert!(matches!(
        write_tags(&path, &edit),
        Err(TagWriteError::PlatterDataTooLong)
    ));
}

// ---------------------------------------------------------------------------------------
// What the edit does not name

#[test]
fn other_id3v2_frames_and_pictures_stay() {
    let picture = png();
    let bytes = [
        id3v24(&[
            id3_text(b"TIT2", "Old title"),
            id3_text(b"TCOM", "Composer Kept"),
            id3_txxx("PLATTER_TEST", "custom value kept"),
            id3_picture(&picture),
        ]),
        silent_mp3(20),
    ]
    .concat();
    let folder = folder("id3-others");
    let path = file(&folder, "a.mp3", &bytes);
    write_tags(&path, &title("New title")).unwrap();
    let written = fs::read(&path).unwrap();
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("New title")
    );
    assert!(find(&written, b"Old title").is_none());
    let frames = id3_frames(&written);
    // The frames the edit does not name keep their bytes and their order.
    assert_eq!(
        frames.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["TIT2", "TCOM", "TXXX", "APIC"]
    );
    assert!(find(&written, &id3_text(b"TCOM", "Composer Kept")).is_some());
    assert!(find(&written, &id3_txxx("PLATTER_TEST", "custom value kept")).is_some());
    assert!(find(&written, &id3_picture(&picture)).is_some());
}

#[test]
fn frames_the_edit_does_not_name_keep_their_values() {
    let bytes = [
        id3v24(&[
            id3_text(b"TIT2", "Old title"),
            id3_text(b"TBPM", "126.50"),
            id3_comment("", "Plain comment"),
            id3_comment("PLATTER_NORM", "described comment kept"),
        ]),
        silent_mp3(20),
    ]
    .concat();
    let folder = folder("id3-unnamed");
    let path = file(&folder, "a.mp3", &bytes);
    write_tags(&path, &title("New title")).unwrap();
    let tags = probe_file(&path).unwrap().tags;
    assert_eq!(tags.title.as_deref(), Some("New title"));
    assert_eq!(
        tags.bpm,
        Some(126.5),
        "a tempo with decimals stays as it was"
    );
    let written = fs::read(&path).unwrap();
    assert!(find(&written, b"Plain comment").is_some());
    assert!(find(&written, b"described comment kept").is_some());

    // Editing the comment replaces the plain one only, and keeps its language.
    write_tags(
        &path,
        &TagEdit {
            comment: Some("New comment".to_owned()),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    assert!(find(&written, b"Plain comment").is_none());
    assert!(find(&written, &id3_comment("", "New comment")).is_some());
    assert!(find(&written, b"described comment kept").is_some());
}

#[test]
fn an_id3v2_tag_in_front_of_flac_is_left_as_it_is() {
    let tag = id3v24(&[id3_text(b"TIT2", "Front title")]);
    let flac = flac_pcm16(RATE, 2, &sine_pcm());
    let folder = folder("flac-id3");
    let reference = decode_file(&file(&folder, "reference.flac", &flac)).unwrap();
    let path = file(&folder, "a.flac", &[tag.clone(), flac].concat());
    write_tags(&path, &title("New title")).unwrap();
    let written = fs::read(&path).unwrap();
    assert!(written.starts_with(&tag));
    assert_eq!(decode_file(&path).unwrap().samples, reference.samples);
    // Platter's reader takes the Vorbis comments, which come after the ID3v2 tag.
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("New title")
    );
    assert_eq!(flac_field(&written, F::Title).as_deref(), Some("New title"));
}

#[test]
fn a_compressed_id3v2_frame_passes_through_as_it_is() {
    let value = b"\x03PLATTER_ZIP\0compressed value";
    let body = [&synchsafe(value.len())[..], &zlib_stored(value)].concat();
    // Format flags: compression (0x08) and data length indicator (0x01).
    let compressed = id3_frame(b"TXXX", 0x09, &body);
    let bytes = [id3v24(std::slice::from_ref(&compressed)), silent_mp3(20)].concat();
    let folder = folder("id3-compressed");
    let path = file(&folder, "a.mp3", &bytes);
    write_tags(&path, &title("New title")).unwrap();
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("New title")
    );
    let written = fs::read(&path).unwrap();
    assert!(
        find(&written, &compressed).is_some(),
        "the frame keeps its bytes"
    );
}

/// `bytes` with a zero after every 0xFF that comes before a zero or a byte from 0xE0
/// (ID3v2.4 structure, section 6.1).
fn unsynchronised(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        out.push(*byte);
        if *byte == 0xFF
            && bytes
                .get(index + 1)
                .is_none_or(|next| *next == 0 || *next >= 0xE0)
        {
            out.push(0);
        }
    }
    out
}

#[test]
fn an_unsynchronised_tag_is_written_plain() {
    // Picture data with the byte pairs unsynchronisation changes.
    let data = [0x89, b'P', b'N', b'G', 0xFF, 0x00, 0x01, 0xFF, 0xE2, 0x07];
    let plain = [&[0], b"image/png\0".as_slice(), &[3, 0], &data].concat();
    // Format flags: unsynchronisation (0x02); the tag header flags it too (0x80).
    let frame = id3_frame(b"APIC", 0x02, &unsynchronised(&plain));
    let mut tag = id3v24(&[frame]);
    tag[5] = 0x80;
    let bytes = [tag, silent_mp3(20)].concat();
    let folder = folder("id3-unsynchronised");
    let path = file(&folder, "a.mp3", &bytes);
    write_tags(&path, &title("New title")).unwrap();
    let written = fs::read(&path).unwrap();
    assert_eq!(
        written[5] & 0x80,
        0,
        "the tag is no longer flagged unsynchronised"
    );
    // The picture frame is plain and no longer flagged.
    assert!(find(&written, &id3_frame(b"APIC", 0, &plain)).is_some());
    assert_eq!(mp3_field(&written, F::Title).as_deref(), Some("New title"));
}

#[test]
fn other_vorbis_comments_and_flac_pictures_stay() {
    let picture = png();
    let with_pictures = flac_with_pictures(&[(3, &picture)]);
    let folder = folder("flac-others");
    let path = file(&folder, "pictures.flac", &with_pictures);
    write_tags(&path, &title("New title")).unwrap();
    let written = fs::read(&path).unwrap();
    assert!(find(&written, &picture).is_some());
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("New title")
    );
    // STREAMINFO first, the picture block as it was, one comment block, padding last.
    let blocks = flac_blocks(&written);
    let kinds: Vec<_> = blocks.iter().map(|(kind, _, _)| *kind).collect();
    assert_eq!(kinds, [0, 4, 6, 1]);
    assert!(blocks.iter().filter(|(_, last, _)| *last).count() == 1 && blocks[3].1);

    let tagged = flac_pcm16_tagged(
        RATE,
        2,
        &sine_pcm(),
        &[("TITLE", "Old"), ("PLATTER_TEST", "custom kept")],
    );
    let path = file(&folder, "tagged.flac", &tagged);
    write_tags(
        &path,
        &TagEdit {
            artist: Some("New artist".to_owned()),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let tags = probe_file(&path).unwrap().tags;
    assert_eq!(tags.title.as_deref(), Some("Old"));
    assert_eq!(tags.artist.as_deref(), Some("New artist"));
    assert!(find(&fs::read(&path).unwrap(), b"PLATTER_TEST=custom kept").is_some());
}

#[test]
fn flac_padding_takes_up_the_change() {
    let folder = folder("flac-padding");
    let path = file(&folder, "a.flac", &flac_pcm16(RATE, 2, &sine_pcm()));
    write_tags(&path, &title("A first title")).unwrap();
    let first = fs::read(&path).unwrap();
    write_tags(&path, &full_edit()).unwrap();
    let second = fs::read(&path).unwrap();
    // The comment grew inside the padding: the file keeps its length.
    assert_eq!(second.len(), first.len());
    assert_eq!(
        flac_field(&second, F::Title).as_deref(),
        Some("Night Drive")
    );
    let padding = flac_blocks(&second).pop().unwrap();
    assert_eq!(padding.0, 1);
    assert!(padding.2.iter().all(|byte| *byte == 0));
}

#[test]
fn a_second_tag_the_file_carries_is_edited_too() {
    let folder = folder("second-tags");
    let edit = title("New title");

    let mp3 = file(
        &folder,
        "a.mp3",
        &[silent_mp3(20), id3v1("Old title", b"2001")].concat(),
    );
    write_tags(&mp3, &edit).unwrap();
    let written = fs::read(&mp3).unwrap();
    let id3v1 = &written[written.len() - 128..];
    assert!(id3v1.starts_with(b"TAGNew title\0"), "{:?}", &id3v1[..16]);
    assert_eq!(&id3v1[93..97], b"2001");
    assert_eq!(
        probe_file(&mp3).unwrap().tags.title.as_deref(),
        Some("New title")
    );

    let info = chunk(
        b"LIST",
        &[b"INFO".as_slice(), &chunk(b"INAM", b"Old title\0", false)].concat(),
        false,
    );
    let wav = with_chunks(wav_pcm16(RATE, 2, &sine_pcm()), &info, false);
    let wav = file(&folder, "a.wav", &wav);
    write_tags(&wav, &edit).unwrap();
    let written = fs::read(&wav).unwrap();
    assert!(find(&written, b"Old title").is_none());
    assert_eq!(wav_field(&written, F::Title).as_deref(), Some("New title"));
    // The decoding library in use drops the INFO chunk: Platter shows no WAV tags.
    assert_eq!(probe_file(&wav).unwrap().tags.title, None);

    let name = chunk(b"NAME", b"Old title", true);
    let aiff = with_chunks(aiff_pcm16(RATE, 2, &sine_pcm()), &name, true);
    let aiff = file(&folder, "a.aiff", &aiff);
    write_tags(&aiff, &edit).unwrap();
    let written = fs::read(&aiff).unwrap();
    assert!(find(&written, b"Old title").is_none());
    assert_eq!(aiff_field(&written, F::Title).as_deref(), Some("New title"));
    assert_eq!(
        probe_file(&aiff).unwrap().tags.title.as_deref(),
        Some("New title")
    );
    assert_no_copy_left(&folder);
}

#[test]
fn an_id3_chunk_created_in_aiff_carries_the_title_of_the_text_chunks() {
    // Without the title in the new ID3 chunk, a reader that prefers it would lose the title.
    let name = chunk(b"NAME", b"Kept title", true);
    let aiff = with_chunks(aiff_pcm16(RATE, 2, &sine_pcm()), &name, true);
    let folder = folder("aiff-seed");
    let path = file(&folder, "a.aiff", &aiff);
    write_tags(
        &path,
        &TagEdit {
            key: Some("8A".to_owned()),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    let tag = riff_chunk(&written, &[b"ID3 "], true).unwrap();
    assert_eq!(id3_field(&tag, F::Title).as_deref(), Some("Kept title"));
    assert_eq!(id3_field(&tag, F::Key).as_deref(), Some("8A"));
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Kept title")
    );
}

#[test]
fn an_empty_edit_keeps_the_tags() {
    let bytes = flac_pcm16_tagged(RATE, 2, &sine_pcm(), &[("TITLE", "Kept")]);
    let folder = folder("empty-edit");
    let path = file(&folder, "a.flac", &bytes);
    write_tags(&path, &TagEdit::default()).unwrap();
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Kept")
    );
    // A file without a tag gets none from an edit that sets nothing.
    let removal = TagEdit {
        title: Some(String::new()),
        bpm: Some(None),
        ..TagEdit::default()
    };
    let mp3 = file(&folder, "a.mp3", &silent_mp3(20));
    let flac = flac_pcm16(RATE, 2, &sine_pcm());
    let untagged = file(&folder, "untagged.flac", &flac);
    for edit in [TagEdit::default(), removal] {
        write_tags(&mp3, &edit).unwrap();
        assert_eq!(fs::read(&mp3).unwrap(), silent_mp3(20));
        write_tags(&untagged, &edit).unwrap();
        assert_eq!(fs::read(&untagged).unwrap(), flac);
    }
}

#[cfg(unix)]
#[test]
fn the_file_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let folder = folder("permissions");
    let path = file(&folder, "a.flac", &flac_pcm16(RATE, 2, &sine_pcm()));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    write_tags(&path, &title("New title")).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn an_id3v22_tag_is_edited_in_its_own_version() {
    // ID3v2.2: three-character identifiers, three-byte sizes, no frame flags.
    let tag = [
        b"ID3\x02\x00\x00\x00\x00\x00\x2A".as_slice(),
        b"TT2\x00\x00\x04\x00Old",
        b"COM\x00\x00\x06\x00eng\x00c",
        &[0; 20],
    ]
    .concat();
    let folder = folder("id3v22");
    let path = file(&folder, "a.mp3", &[tag, silent_mp3(4)].concat());
    write_tags(
        &path,
        &TagEdit {
            title: Some("Night".to_owned()),
            comment: Some("Peak".to_owned()),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let written = fs::read(&path).unwrap();
    let expected = [
        b"ID3\x02\x00\x00\x00\x00\x00\x2A".as_slice(),
        b"TT2\x00\x00\x06\x00Night",
        // The comment keeps its language.
        b"COM\x00\x00\x09\x00eng\x00Peak",
        // 42 - 12 - 15 bytes of padding.
        &[0; 15],
    ]
    .concat();
    assert_eq!(&written[..expected.len()], expected.as_slice());
    assert_eq!(&written[expected.len()..], silent_mp3(4).as_slice());
}

#[test]
fn an_unsynchronised_id3v23_tag_is_written_plain() {
    // In ID3v2.3 unsynchronisation covers the whole tag after its header.
    let old_title = b"TIT2\x00\x00\x00\x04\x00\x00\x00Old".as_slice();
    let private = b"PRIV\x00\x00\x00\x08\x00\x00x\x00\xFF\xE0\xFF\x00\x01\x02".as_slice();
    let body = unsynchronised(&[old_title, private].concat());
    assert_eq!(body.len(), 34);
    let tag = [b"ID3\x03\x00\x80\x00\x00\x00\x22".as_slice(), &body].concat();
    let folder = folder("id3v23-unsynchronised");
    let path = file(&folder, "a.mp3", &[tag, silent_mp3(4)].concat());
    write_tags(&path, &title("New")).unwrap();
    let written = fs::read(&path).unwrap();
    let expected = [
        // Same version and size, the unsynchronisation flag cleared.
        b"ID3\x03\x00\x00\x00\x00\x00\x22".as_slice(),
        b"TIT2\x00\x00\x00\x04\x00\x00\x00New",
        private,
        // The two bytes unsynchronisation had added are now padding.
        &[0; 2],
    ]
    .concat();
    assert_eq!(&written[..expected.len()], expected.as_slice());
    assert_eq!(&written[expected.len()..], silent_mp3(4).as_slice());
}

// ---------------------------------------------------------------------------------------
// Bounds and refusals

#[test]
fn long_text_is_cut_and_control_characters_dropped() {
    let folder = folder("bounds");
    let path = file(&folder, "a.flac", &flac_pcm16(RATE, 2, &sine_pcm()));
    write_tags(
        &path,
        &TagEdit {
            title: Some("é".repeat(MAX_TAG_CHARS + 50)),
            artist: Some(" A\0B\u{7}C ".to_owned()),
            comment: Some("line one\nline two".to_owned()),
            ..TagEdit::default()
        },
    )
    .unwrap();
    let tags = probe_file(&path).unwrap().tags;
    assert_eq!(
        tags.title.map(|title| title.chars().count()),
        Some(MAX_TAG_CHARS)
    );
    assert_eq!(tags.artist.as_deref(), Some("ABC"));
    assert_eq!(
        flac_field(&fs::read(&path).unwrap(), F::Comment).as_deref(),
        Some("line one\nline two")
    );
}

#[test]
fn a_tempo_out_of_range_is_refused_before_the_file_is_opened() {
    // The path does not exist: the tempo is checked first.
    let path = folder("tempo").join("missing.flac");
    for bpm in [19.9, 1_000.0, f64::NAN, f64::INFINITY, -120.0] {
        let edit = TagEdit {
            bpm: Some(Some(bpm)),
            ..TagEdit::default()
        };
        assert!(
            matches!(write_tags(&path, &edit), Err(TagWriteError::BpmOutOfRange)),
            "{bpm}"
        );
    }
}

fn assert_refused(name: &str, bytes: &[u8], check: impl Fn(&TagWriteError) -> bool) {
    let folder = folder(name);
    let path = file(&folder, name, bytes);
    let error = write_tags(&path, &full_edit()).unwrap_err();
    assert!(check(&error), "{name}: {error}");
    assert_eq!(fs::read(&path).unwrap(), bytes, "{name}");
    assert_no_copy_left(&folder);
}

fn unsupported_or_corrupt(error: &TagWriteError) -> bool {
    matches!(
        error,
        TagWriteError::Unsupported(_) | TagWriteError::Corrupt(_)
    )
}

#[test]
fn a_file_that_is_not_audio_is_refused_and_unchanged() {
    assert_refused(
        "notes.mp3",
        b"These are plain words, not audio frames.",
        unsupported_or_corrupt,
    );
    assert_refused("empty.flac", b"", unsupported_or_corrupt);
}

#[test]
fn a_damaged_file_is_refused_and_unchanged() {
    // The WAV header ends inside its format chunk.
    let wav = wav_pcm16(RATE, 2, &sine_pcm());
    assert_refused("cut.wav", &wav[..30], unsupported_or_corrupt);
    let corrupt = |error: &TagWriteError| matches!(error, TagWriteError::Corrupt(_));
    // A frame of the ID3v2 tag declares more bytes than the tag holds.
    let mut tag = id3v24(&[id3_text(b"TPE1", "Artist")]);
    tag[17] = 0x7F;
    assert_refused("long-frame.mp3", &[tag, silent_mp3(20)].concat(), corrupt);
    // A frame identifier with characters ID3v2 does not allow.
    let tag = id3v24(&[id3_text(b"Tit2", "Title")]);
    assert_refused("bad-id.mp3", &[tag, silent_mp3(20)].concat(), corrupt);
    // A comment field whose length runs past its block.
    let mut flac = flac_pcm16_tagged(RATE, 2, &sine_pcm(), &[("TITLE", "Old")]);
    let at = find(&flac, b"TITLE=Old").unwrap() - 4;
    flac[at] = 0xF0;
    assert_refused("bad-comment.flac", &flac, corrupt);
}

#[test]
fn tags_other_software_writes_are_accepted() {
    let folder = folder("lenient");
    // An ID3v1 tag with an empty year, common in files.
    let path = file(
        &folder,
        "blank-year.mp3",
        &[silent_mp3(20), id3v1("Title", &[0; 4])].concat(),
    );
    write_tags(&path, &title("New title")).unwrap();
    let written = fs::read(&path).unwrap();
    assert!(written[written.len() - 128..].starts_with(b"TAGNew title"));
    // A frame of size zero, which ID3v2.4 forbids, passes through as it was.
    let empty = id3_frame(b"TCOM", 0, b"");
    let tag = id3v24(&[empty.clone(), id3_text(b"TPE1", "Artist")]);
    let path = file(&folder, "empty-frame.mp3", &[tag, silent_mp3(20)].concat());
    write_tags(&path, &title("New title")).unwrap();
    let written = fs::read(&path).unwrap();
    assert!(find(&written, &empty).is_some());
    assert_eq!(mp3_field(&written, F::Title).as_deref(), Some("New title"));
}

#[test]
fn a_read_only_file_is_refused_and_unchanged() {
    let folder = folder("read-only");
    let bytes = flac_pcm16(RATE, 2, &sine_pcm());
    let path = file(&folder, "a.flac", &bytes);
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).unwrap();

    let result = write_tags(&path, &full_edit());

    // Writable again, so the folder can be cleaned by the next run.
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).unwrap();
    assert!(matches!(result, Err(TagWriteError::ReadOnly)), "{result:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_no_copy_left(&folder);
}

#[test]
fn a_missing_file_or_a_folder_is_an_error() {
    let folder = folder("missing");
    assert!(matches!(
        write_tags(&folder.join("none.flac"), &full_edit()),
        Err(TagWriteError::Io(_))
    ));
    assert!(matches!(
        write_tags(&folder, &full_edit()),
        Err(TagWriteError::Io(_))
    ));
    assert_no_copy_left(&folder);
}

#[test]
fn a_link_is_followed_and_stays_a_link() {
    let folder = folder("link");
    let target = file(&folder, "track.flac", &flac_pcm16(RATE, 2, &sine_pcm()));
    let link = folder.join("link.flac");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link);
    if let Err(error) = made {
        // Windows lets only some accounts create links.
        eprintln!("no link could be created, the test checks nothing: {error}");
        return;
    }
    write_tags(&link, &full_edit()).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        probe_file(&target).unwrap().tags.title.as_deref(),
        Some("Night Drive")
    );
    assert_no_copy_left(&folder);
}

#[test]
fn the_container_stays_the_one_platter_reads() {
    // A FLAC file named .mp3 is written as FLAC.
    let folder = folder("misnamed");
    let path = file(&folder, "a.mp3", &flac_pcm16(RATE, 2, &sine_pcm()));
    write_tags(&path, &full_edit()).unwrap();
    let info = probe_file(&path).unwrap();
    assert_eq!(info.container, Container::Flac);
    assert_eq!(info.tags.title.as_deref(), Some("Night Drive"));
}

// ---------------------------------------------------------------------------------------
// Sizes and layouts checked by hand

#[test]
fn id3v2_sizes_over_127_bytes_are_coded_as_each_version_says() {
    // A user text frame of 200 bytes: encoding, `PLATTER`, its terminator and 191 bytes.
    let data = "x".repeat(191);
    let edit = TagEdit {
        platter: Some(Some(data.clone())),
        ..TagEdit::default()
    };
    let folder = folder("id3-sizes");

    // ID3v2.4: synchsafe sizes. 200 = 1 * 128 + 0x48; 210 + 1024 of padding = 1234 =
    // 9 * 128 + 0x52.
    let path = file(&folder, "v24.mp3", &silent_mp3(4));
    write_tags(&path, &edit).unwrap();
    let written = fs::read(&path).unwrap();
    let expected = [
        b"ID3\x04\x00\x00\x00\x00\x09\x52".as_slice(),
        b"TXXX\x00\x00\x01\x48\x00\x00\x03PLATTER\x00",
        data.as_bytes(),
    ]
    .concat();
    assert_eq!(&written[..expected.len()], expected.as_slice());
    assert_eq!(written.len(), 10 + 1234 + silent_mp3(4).len());

    // ID3v2.3: the frame size is a plain number, 200 = 0xC8; the tag size stays synchsafe,
    // 14 + 210 + 1024 = 1248 = 9 * 128 + 0x60.
    let frame = b"TIT2\x00\x00\x00\x04\x00\x00\x00Old".as_slice();
    let tag = [b"ID3\x03\x00\x00\x00\x00\x00\x0E".as_slice(), frame].concat();
    let path = file(&folder, "v23.mp3", &[tag, silent_mp3(4)].concat());
    write_tags(&path, &edit).unwrap();
    let written = fs::read(&path).unwrap();
    let expected = [
        b"ID3\x03\x00\x00\x00\x00\x09\x60".as_slice(),
        frame,
        b"TXXX\x00\x00\x00\xC8\x00\x00\x00PLATTER\x00",
        data.as_bytes(),
    ]
    .concat();
    assert_eq!(&written[..expected.len()], expected.as_slice());
    assert_eq!(written.len(), 10 + 1248 + silent_mp3(4).len());
}

/// The size field of the first box of `kind` in `bytes`.
fn box_size(bytes: &[u8], kind: &[u8; 4]) -> usize {
    let at = find(bytes, kind).unwrap() - 4;
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

#[test]
fn m4a_parent_boxes_take_the_size_of_what_they_hold() {
    let folder = folder("m4a-sizes");
    // Without user data, the whole path is created: ilst 8 + 35, hdlr 8 + 25, meta 8 + 4 +
    // 33 + 43 = 88, udta 8 + 88 = 96.
    let bytes = m4a_with(false, 0);
    let path = file(&folder, "bare.m4a", &bytes);
    write_tags(&path, &title("Night Drive")).unwrap();
    let written = fs::read(&path).unwrap();
    let udta = [
        b"\x00\x00\x00\x60udta".as_slice(),
        b"\x00\x00\x00\x58meta\x00\x00\x00\x00",
        b"\x00\x00\x00\x21hdlr\x00\x00\x00\x00\x00\x00\x00\x00mdirappl",
        &[0; 9],
        b"\x00\x00\x00\x2Bilst",
        b"\x00\x00\x00\x23\xA9nam\x00\x00\x00\x1Bdata\x00\x00\x00\x01\x00\x00\x00\x00Night Drive",
    ]
    .concat();
    assert!(find(&written, &udta).is_some());
    assert_eq!(box_size(&written, b"moov"), box_size(&bytes, b"moov") + 96);
    assert_eq!(chunk_offset(&written), chunk_offset(&bytes) + 96);

    // With user data, each box on the path grows by the 35 bytes of the new item.
    let bytes = m4a();
    let path = file(&folder, "tagged.m4a", &bytes);
    write_tags(&path, &title("Night Drive")).unwrap();
    let written = fs::read(&path).unwrap();
    for kind in [b"moov", b"udta", b"meta", b"ilst"] {
        assert_eq!(box_size(&written, kind), box_size(&bytes, kind) + 35);
    }
    assert_eq!(&written[chunk_offset(&written)..][..16], MDAT_MARK);
}

/// An M4A file: `co64` offsets when `wide`, `extra` boxes in `moov`, and `moov` after `mdat`
/// when `moov_last`.
fn m4a_layout(wide: bool, extra: &[u8], moov_last: bool) -> Vec<u8> {
    let ftyp = mp4_box(b"ftyp", b"M4A \0\0\0\0M4A mp42isom");
    let mdat = mp4_box(b"mdat", MDAT_MARK);
    if moov_last {
        let offset = (ftyp.len() + 8) as u64;
        let moov = mp4_moov_with(offset, 16, true, wide, extra);
        return [ftyp, mdat, moov].concat();
    }
    let moov_len = mp4_moov_with(0, 16, true, wide, extra).len();
    let offset = (ftyp.len() + moov_len + 8) as u64;
    [ftyp, mp4_moov_with(offset, 16, true, wide, extra), mdat].concat()
}

#[test]
fn co64_offsets_follow_a_larger_moov() {
    let bytes = m4a_layout(true, &[], false);
    let offset_of = |bytes: &[u8]| {
        let at = find(bytes, b"co64").unwrap();
        u64::from_be_bytes(bytes[at + 12..at + 20].try_into().unwrap()) as usize
    };
    assert_eq!(&bytes[offset_of(&bytes)..][..16], MDAT_MARK);
    let folder = folder("m4a-co64");
    let path = file(&folder, "a.m4a", &bytes);
    write_tags(&path, &full_edit()).unwrap();
    let written = fs::read(&path).unwrap();
    let grown = written.len() - bytes.len();
    assert!(grown > 0);
    assert_eq!(offset_of(&written), offset_of(&bytes) + grown);
    assert_eq!(&written[offset_of(&written)..][..16], MDAT_MARK);
    assert_eq!(
        probe_file(&path).unwrap().tags.title.as_deref(),
        Some("Night Drive")
    );
}

#[test]
fn a_moov_after_the_media_moves_no_offset() {
    let bytes = m4a_layout(false, &[], true);
    let offset = chunk_offset(&bytes);
    let folder = folder("m4a-moov-last");
    let path = file(&folder, "a.m4a", &bytes);
    write_tags(&path, &full_edit()).unwrap();
    let written = fs::read(&path).unwrap();
    assert!(written.len() > bytes.len());
    assert_eq!(chunk_offset(&written), offset);
    assert_eq!(&written[offset..][..16], MDAT_MARK);
    assert_eq!(&written[..offset + 16], &bytes[..offset + 16]);
    assert_eq!(
        m4a_field(&written, F::Title).as_deref(),
        Some("Night Drive")
    );
}

#[test]
fn a_fragmented_m4a_file_is_refused_when_moov_must_grow() {
    // `mvex` announces movie fragments, whose offsets the writer does not move.
    let mvex = mp4_box(b"mvex", &full_box(b"trex", &be(&[1, 1, 0, 0, 0])));
    let bytes = m4a_layout(false, &mvex, false);
    assert_refused("fragmented.m4a", &bytes, |error| {
        matches!(error, TagWriteError::Unsupported(_))
    });
}

#[test]
fn bytes_after_the_last_chunk_do_not_shift_the_new_chunks() {
    // Four stray bytes after the audio, counted by the size of the container: they are not a
    // chunk and are dropped, so the chunks added after the audio start on a chunk boundary.
    let cases = [
        ("a.wav", wav_pcm16(RATE, 2, &sine_pcm()), false),
        ("a.aiff", aiff_pcm16(RATE, 2, &sine_pcm()), true),
    ];
    let folder = folder("riff-stray");
    for (name, plain, big_endian) in cases {
        let bytes = with_chunks(plain.clone(), &[1, 2, 3, 4], big_endian);
        let path = file(&folder, name, &bytes);
        write_tags(&path, &full_edit()).unwrap();
        let written = fs::read(&path).unwrap();
        let chunks = riff_chunks(&written, big_endian);
        let walked: usize = 12
            + chunks
                .iter()
                .map(|(_, body)| 8 + body.len() + body.len() % 2)
                .sum::<usize>();
        assert_eq!(walked, written.len(), "{name}");
        let ids: Vec<&[u8]> = chunks.iter().map(|(id, _)| id.as_slice()).collect();
        let expected: &[&[u8]] = if big_endian {
            &[b"COMM", b"SSND", b"NAME", b"AUTH", b"ANNO", b"ID3 "]
        } else {
            &[b"fmt ", b"data", b"LIST", b"id3 "]
        };
        assert_eq!(ids, expected, "{name}");
        assert_eq!(&written[12..plain.len()], &plain[12..], "{name}");
        assert_eq!(decode_file(&path).unwrap().channels, 2);
    }
}

#[test]
fn ogg_pages_that_must_be_renumbered_are_never_left_with_a_gap() {
    let filler = "x".repeat(64_725);
    let comment = vorbis_comment_packet(&[("PLATTER_BIG", &filler)]);
    let junk = b"these bytes are not an Ogg page";
    let bytes = [
        ogg_page(0x02, 0, 0, &[vorbis_ident()]),
        ogg_page(0x00, 0, 1, &[comment, vorbis_setup()]),
        junk.to_vec(),
        ogg_page(0x04, 1_024, 2, &[OGG_AUDIO.to_vec()]),
    ]
    .concat();
    // The comment grows past its page: the audio page after the junk could not be renumbered.
    assert_refused("junk.ogg", &bytes, |error| {
        matches!(error, TagWriteError::Corrupt(_))
    });

    // When the pages keep their number, the bytes after the headers are copied as they are.
    let bytes = [ogg_vorbis(&[("TITLE", "Old")]), junk.to_vec()].concat();
    let folder = folder("ogg-junk");
    let path = file(&folder, "a.ogg", &bytes);
    write_tags(&path, &title("New")).unwrap();
    let written = fs::read(&path).unwrap();
    assert!(written.ends_with(junk));
}

fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "f.mp3",
            [
                id3v24(&[id3_text(b"TIT2", "Old"), id3_comment("", "c")]),
                silent_mp3(8),
                id3v1("Old", b"2001"),
            ]
            .concat(),
        ),
        (
            "f.flac",
            flac_pcm16_tagged(RATE, 1, &[0; 64], &[("TITLE", "Old")]),
        ),
        ("f.ogg", ogg_vorbis(&[("TITLE", "Old")])),
        ("f.m4a", m4a_with(true, 64)),
        (
            "f.wav",
            with_chunks(
                wav_pcm16(RATE, 1, &[0; 64]),
                &[
                    chunk(
                        b"LIST",
                        &[b"INFO".as_slice(), &chunk(b"INAM", b"Old\0", false)].concat(),
                        false,
                    ),
                    chunk(b"id3 ", &id3v24(&[id3_text(b"TIT2", "Old")]), false),
                ]
                .concat(),
                false,
            ),
        ),
        (
            "f.aiff",
            with_chunks(
                aiff_pcm16(RATE, 1, &[0; 64]),
                &[
                    chunk(b"NAME", b"Old", true),
                    chunk(b"ID3 ", &id3v24(&[id3_text(b"TIT2", "Old")]), true),
                ]
                .concat(),
                true,
            ),
        ),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// A damaged file of any format never makes the writer panic: it is refused and left as
    /// it was, or written and still read as the same format.
    #[test]
    fn damaged_files_never_panic(
        which in 0usize..6,
        cut in any::<prop::sample::Index>(),
        at in any::<prop::sample::Index>(),
        value in any::<u8>(),
        truncate in any::<bool>(),
        head in any::<bool>(),
    ) {
        let (name, mut bytes) = fixtures().swap_remove(which);
        if truncate {
            bytes.truncate(cut.index(bytes.len() + 1));
        } else if !bytes.is_empty() {
            // The metadata sits in the first kilobyte (ID3v2, FLAC blocks, Ogg headers, moov)
            // or the last half (ID3v1, the chunks added to WAV and AIFF).
            let at = if head {
                at.index(bytes.len().min(1024))
            } else {
                bytes.len() - 1 - at.index(bytes.len().min(512))
            };
            bytes[at] = value;
        }
        let folder = folder(&format!("fuzz-{name}"));
        let path = file(&folder, name, &bytes);
        let before = probe_file(&path).map(|info| info.container);
        match write_tags(&path, &full_edit()) {
            Ok(()) => {
                prop_assert_eq!(probe_file(&path).unwrap().container, before.unwrap());
            }
            Err(_) => prop_assert_eq!(fs::read(&path).unwrap(), bytes),
        }
        assert_no_copy_left(&folder);
    }
}

// ---------------------------------------------------------------------------------------
// Covers. Behaviour note: `docs/behavior/tag-writing.md`, *Covers*, and
// `docs/behavior/cover-art.md`.

const FRONT: u8 = 3;
const BACK: u8 = 4;
const OLD: [u8; 4] = [200, 0, 0, 255];
const NEW: [u8; 4] = [0, 200, 0, 255];
const OTHER: [u8; 4] = [0, 0, 200, 255];

fn solid(colour: [u8; 4]) -> Vec<u8> {
    rgba_png(4, 4, |_, _| colour)
}

fn new_cover() -> CoverPicture {
    CoverPicture::from_bytes(solid(NEW)).unwrap()
}

fn cover_edit(cover: Option<CoverPicture>) -> TagEdit {
    TagEdit {
        cover: Some(cover),
        ..TagEdit::default()
    }
}

/// The colour of the first pixel of the cover Platter's own reader shows.
fn shown(path: &Path) -> Option<[u8; 4]> {
    cover_file(path)
        .unwrap()
        .map(|cover| cover.rgba[..4].try_into().unwrap())
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// An ID3v2.3 tag with one attached-picture frame per entry (ID3v2.3, section 4.15).
fn id3v23_pictures(pictures: &[(u8, &[u8])]) -> Vec<u8> {
    let mut frames = Vec::new();
    for (kind, data) in pictures {
        let body = [&[0][..], b"image/png\0", &[*kind, 0], data].concat();
        frames.extend_from_slice(b"APIC");
        frames.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frames.extend_from_slice(&[0, 0]);
        frames.extend_from_slice(&body);
    }
    [
        b"ID3".as_slice(),
        &[3, 0, 0],
        &synchsafe(frames.len()),
        &frames,
    ]
    .concat()
}

/// An AIFF file with `tag` in an ID3 chunk at the end.
fn aiff_with_id3(tag: &[u8]) -> Vec<u8> {
    let mut out = aiff_pcm16(RATE, 2, &sine_pcm());
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

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let bits = group.iter().enumerate().fold(0u32, |bits, (index, byte)| {
            bits | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            if index <= group.len() {
                out.push(char::from(
                    ALPHABET[(bits >> (18 - 6 * index)) as usize & 0x3F],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut bits = 0u32;
    let mut held = 0;
    for character in text.bytes().filter(|character| *character != b'=') {
        let sextet = ALPHABET
            .iter()
            .position(|letter| *letter == character)
            .unwrap();
        bits = bits << 6 | sextet as u32;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
        }
    }
    out
}

#[test]
fn an_imported_cover_is_the_one_the_reader_shows_and_a_deleted_one_is_gone() {
    let old = solid(OLD);
    let other = solid(OTHER);
    let in_comment = base64_encode(&picture_block(FRONT, "image/png", &old));
    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        (
            "blocks.flac",
            flac_with_pictures(&[(BACK, &other), (FRONT, &old)]),
            true,
        ),
        (
            "comment.flac",
            flac_pcm16_tagged(
                RATE,
                2,
                &sine_pcm(),
                &[("TITLE", "Tone"), ("METADATA_BLOCK_PICTURE", &in_comment)],
            ),
            false,
        ),
        (
            "id3.flac",
            [
                id3v23_pictures(&[(FRONT, &old)]),
                flac_pcm16(RATE, 2, &sine_pcm()),
            ]
            .concat(),
            false,
        ),
        (
            "tagged.mp3",
            [
                id3v23_pictures(&[(BACK, &other), (FRONT, &old)]),
                silent_mp3(20),
            ]
            .concat(),
            true,
        ),
        ("plain.mp3", silent_mp3(20), false),
        (
            "tagged.aiff",
            aiff_with_id3(&id3v23_pictures(&[(FRONT, &old)])),
            false,
        ),
        ("plain.aiff", aiff_pcm16(RATE, 2, &sine_pcm()), false),
    ];
    for (name, bytes, keeps_other) in cases {
        let folder = folder(&format!("cover-{name}"));
        let path = file(&folder, name, &bytes);
        let before = probe_file(&path).unwrap();

        write_tags(&path, &cover_edit(Some(new_cover()))).unwrap();
        assert_eq!(shown(&path), Some(NEW), "{name}");
        let written = fs::read(&path).unwrap();
        assert_eq!(
            count(&written, &old),
            0,
            "{name}: the old front cover is left"
        );
        assert!(
            !String::from_utf8_lossy(&written).contains("METADATA_BLOCK_PICTURE"),
            "{name}: the old picture comment is left"
        );
        assert_eq!(count(&written, &other) > 0, keeps_other, "{name}");

        // A second import replaces the first: one front cover.
        write_tags(&path, &cover_edit(Some(new_cover()))).unwrap();
        assert_eq!(count(&fs::read(&path).unwrap(), &solid(NEW)), 1, "{name}");

        write_tags(&path, &cover_edit(None)).unwrap();
        assert_eq!(shown(&path), None, "{name}");
        let written = fs::read(&path).unwrap();
        assert_eq!(count(&written, &other), 0, "{name}: a picture is left");
        let after = probe_file(&path).unwrap();
        assert_eq!(
            (after.container, after.sample_rate, after.channels),
            (before.container, before.sample_rate, before.channels),
            "{name}"
        );
        assert_no_copy_left(&folder);
    }
}

#[test]
fn deleting_the_cover_of_a_file_without_pictures_changes_no_tag() {
    let folder = folder("cover-none");
    let bytes = silent_mp3(20);
    let path = file(&folder, "plain.mp3", &bytes);
    write_tags(&path, &cover_edit(None)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn a_cover_goes_into_the_comment_of_ogg_vorbis() {
    let folder = folder("cover-ogg");
    let old = base64_encode(&picture_block(BACK, "image/png", &solid(OTHER)));
    let path = file(
        &folder,
        "tone.ogg",
        &ogg_vorbis(&[
            ("TITLE", "Tone"),
            ("METADATA_BLOCK_PICTURE", &old),
            ("COVERART", "AAAA"),
        ]),
    );
    write_tags(&path, &cover_edit(Some(new_cover()))).unwrap();
    let bytes = fs::read(&path).unwrap();
    probe_file(&path).unwrap();
    let fields = vorbis_fields(&ogg_packets(&bytes)[1][7..]);
    let pictures: Vec<&str> = fields
        .iter()
        .filter_map(|field| field.strip_prefix("METADATA_BLOCK_PICTURE="))
        .collect();
    // The back cover stays; the new front cover comes first; the untyped picture goes.
    assert_eq!(pictures.len(), 2, "{fields:?}");
    assert_eq!(pictures[1], old);
    assert!(!fields.iter().any(|field| field.starts_with("COVERART=")));
    let block = base64_decode(pictures[0]);
    let mime = b"image/png";
    assert_eq!(block[..4], [0, 0, 0, FRONT]);
    assert_eq!(block[4..8], (mime.len() as u32).to_be_bytes());
    assert_eq!(&block[8..17], mime);
    let after_mime = 17;
    // No description; width, height, depth, no colour table.
    assert_eq!(block[after_mime..after_mime + 4], [0; 4]);
    assert_eq!(block[after_mime + 4..after_mime + 8], 4u32.to_be_bytes());
    assert_eq!(block[after_mime + 8..after_mime + 12], 4u32.to_be_bytes());
    assert_eq!(block[after_mime + 12..after_mime + 16], 32u32.to_be_bytes());
    assert_eq!(block[after_mime + 16..after_mime + 20], [0; 4]);
    let picture = solid(NEW);
    assert_eq!(
        block[after_mime + 20..after_mime + 24],
        (picture.len() as u32).to_be_bytes()
    );
    assert_eq!(&block[after_mime + 24..], picture);

    write_tags(&path, &cover_edit(None)).unwrap();
    let bytes = fs::read(&path).unwrap();
    let fields = vorbis_fields(&ogg_packets(&bytes)[1][7..]);
    assert_eq!(fields, ["TITLE=Tone"]);
}

#[test]
fn a_cover_goes_into_the_covr_item_of_m4a() {
    let folder = folder("cover-m4a");
    let path = file(&folder, "tone.m4a", &m4a());
    write_tags(&path, &cover_edit(Some(new_cover()))).unwrap();
    probe_file(&path).unwrap();
    let items = mp4_items(&fs::read(&path).unwrap());
    let covers: Vec<&Vec<u8>> = items
        .iter()
        .filter(|(kind, _)| kind == b"covr")
        .map(|(_, item)| item)
        .collect();
    assert_eq!(covers.len(), 1);
    let data = mp4_boxes(&covers[0][8..])
        .into_iter()
        .find(|(kind, _)| kind == b"data")
        .unwrap()
        .1;
    // Type 14 is PNG.
    assert_eq!(data[8..12], [0, 0, 0, 14]);
    assert_eq!(mp4_value(covers[0]), solid(NEW));

    write_tags(&path, &cover_edit(None)).unwrap();
    probe_file(&path).unwrap();
    let items = mp4_items(&fs::read(&path).unwrap());
    assert!(!items.iter().any(|(kind, _)| kind == b"covr"));
}

#[test]
fn a_picture_to_import_must_be_one_the_reader_shows() {
    let picture = CoverPicture::from_bytes(rgba_png(3, 2, |_, _| NEW)).unwrap();
    assert_eq!(
        (
            picture.format(),
            picture.width(),
            picture.height(),
            picture.depth()
        ),
        (PictureFormat::Png, 3, 2, 32)
    );
    assert_eq!(picture.format().mime(), "image/png");

    let refused = CoverPicture::from_bytes(b"not a picture".to_vec());
    assert!(
        matches!(refused, Err(CoverError::Unsupported(_))),
        "{refused:?}"
    );
    let mut damaged = solid(NEW);
    damaged.truncate(40);
    let refused = CoverPicture::from_bytes(damaged);
    assert!(
        matches!(refused, Err(CoverError::Damaged(_))),
        "{refused:?}"
    );
    let mut huge = solid(NEW);
    huge.resize(MAX_COVER_IMPORT_BYTES + 1, 0);
    let refused = CoverPicture::from_bytes(huge);
    assert!(
        matches!(refused, Err(CoverError::TooLargeToImport)),
        "{refused:?}"
    );
}
