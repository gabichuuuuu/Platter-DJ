//! Ogg Vorbis: the comment header, the second packet of the stream, is rebuilt and the pages
//! that held it and the setup header are cut again. When their number changes, every later
//! page of the stream is renumbered and its checksum computed again.
//!
//! Only the first logical stream is edited: a stream chained after it keeps its own
//! comment.

use std::io::Write;

use super::vorbis::Comments;
use super::{Changes, Source, TagWriteError, corrupt, put, slice, unsupported};

const CONTINUED: u8 = 0x01;
const FIRST_PAGE: u8 = 0x02;
/// Lacing values on one page.
const MAX_SEGMENTS: usize = 255;
/// Fixed part of a page header, before its lacing values.
const HEADER_LEN: usize = 27;
const VORBIS: &[u8; 6] = b"vorbis";

/// CRC-32 of Ogg pages (RFC 3533, section 6): polynomial 0x04C11DB7, not reflected, started
/// at zero, computed over the page with its checksum field set to zero.
const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut value = (index as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 {
                value << 1 ^ 0x04C1_1DB7
            } else {
                value << 1
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

fn crc(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |crc, byte| {
        crc << 8 ^ CRC_TABLE[usize::from((crc >> 24) as u8 ^ byte)]
    })
}

struct Page {
    kind: u8,
    granule: [u8; 8],
    serial: u32,
    sequence: u32,
    lacing: Vec<u8>,
    body: Vec<u8>,
    /// Bytes the page takes in the file.
    len: u64,
}

/// The page at `at`, or `None` when no whole page starts there.
fn read_page(source: &mut Source, at: u64) -> Result<Option<Page>, TagWriteError> {
    if source.len.saturating_sub(at) < HEADER_LEN as u64 {
        return Ok(None);
    }
    let header = source.read(at, HEADER_LEN as u64)?;
    if !header.starts_with(b"OggS") || header.get(4) != Some(&0) {
        return Ok(None);
    }
    let field = |start: usize| -> Result<[u8; 4], TagWriteError> {
        slice(&header, start, 4)?
            .try_into()
            .map_err(|_| corrupt("Ogg page header"))
    };
    let kind = *header.get(5).ok_or_else(|| corrupt("Ogg page header"))?;
    let granule = slice(&header, 6, 8)?
        .try_into()
        .map_err(|_| corrupt("Ogg page header"))?;
    let serial = u32::from_le_bytes(field(14)?);
    let sequence = u32::from_le_bytes(field(18)?);
    let segments = u64::from(*header.get(26).ok_or_else(|| corrupt("Ogg page header"))?);
    let lacing_at = at + HEADER_LEN as u64;
    if source.len.saturating_sub(lacing_at) < segments {
        return Ok(None);
    }
    let lacing = source.read(lacing_at, segments)?;
    let body_len: u64 = lacing.iter().map(|value| u64::from(*value)).sum();
    if source.len.saturating_sub(lacing_at + segments) < body_len {
        return Ok(None);
    }
    let body = source.read(lacing_at + segments, body_len)?;
    Ok(Some(Page {
        kind,
        granule,
        serial,
        sequence,
        lacing,
        body,
        len: HEADER_LEN as u64 + segments + body_len,
    }))
}

fn page_bytes(page: &Page) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + page.lacing.len() + page.body.len());
    out.extend_from_slice(b"OggS");
    out.push(0);
    out.push(page.kind);
    out.extend_from_slice(&page.granule);
    out.extend_from_slice(&page.serial.to_le_bytes());
    out.extend_from_slice(&page.sequence.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    // At most 255 lacing values, so the count fits its byte.
    out.push(page.lacing.len().min(MAX_SEGMENTS) as u8);
    out.extend_from_slice(&page.lacing);
    out.extend_from_slice(&page.body);
    let checksum = crc(&out).to_le_bytes();
    if let Some(field) = out.get_mut(22..26) {
        field.copy_from_slice(&checksum);
    }
    out
}

pub(super) fn write(
    source: &mut Source,
    out: &mut impl Write,
    changes: &Changes,
) -> Result<(), TagWriteError> {
    let first = read_page(source, 0)?.ok_or_else(|| corrupt("no Ogg page at the start"))?;
    if first.kind & FIRST_PAGE == 0 || !first.body.get(1..7).is_some_and(|id| id == VORBIS) {
        return Err(unsupported("the first Ogg stream is not Vorbis"));
    }
    if first.lacing.last().is_none_or(|value| *value == 255) {
        return Err(corrupt(
            "the Vorbis identification header does not end its page",
        ));
    }
    let serial = first.serial;

    // The comment and setup headers, from the second page to the page the setup ends.
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut packet = Vec::new();
    let mut at = first.len;
    let mut old_pages = 0u32;
    let first_sequence = first.sequence.wrapping_add(1);
    while packets.len() < 2 {
        let page = read_page(source, at)?
            .ok_or_else(|| corrupt("the Vorbis headers end before the setup header"))?;
        if page.serial != serial {
            return Err(unsupported(
                "another Ogg stream interleaved with the headers",
            ));
        }
        if at - first.len > super::MAX_METADATA_BYTES {
            return Err(unsupported("its metadata is larger than 64 MiB"));
        }
        let mut offset = 0usize;
        for (index, value) in page.lacing.iter().enumerate() {
            let len = usize::from(*value);
            packet.extend_from_slice(slice(&page.body, offset, len)?);
            offset += len;
            if len < 255 {
                packets.push(std::mem::take(&mut packet));
                // Appendix A.2: the setup header finishes the page it ends on.
                if packets.len() == 2 && index + 1 != page.lacing.len() {
                    return Err(unsupported(
                        "the audio starts on the page of the setup header",
                    ));
                }
            }
        }
        at += page.len;
        old_pages += 1;
    }
    let (comment, setup) = match packets.as_slice() {
        [comment, setup] => (comment, setup),
        _ => return Err(corrupt("the Vorbis headers")),
    };
    let comment = edit_comment(comment, changes)?;

    let pages = paginate(&[comment, setup.clone()], serial, first_sequence);
    let new_pages = u32::try_from(pages.len()).map_err(|_| corrupt("size"))?;
    put(out, &page_bytes(&first))?;
    for page in &pages {
        put(out, &page_bytes(page))?;
    }
    if new_pages == old_pages {
        return source.copy(at, source.len, out);
    }
    renumber(
        source,
        out,
        at,
        serial,
        i64::from(new_pages) - i64::from(old_pages),
    )
}

/// The comment header with the changes: packet type 3, `vorbis`, the comment, then the
/// framing bit (Vorbis I, section 5.2.1).
fn edit_comment(packet: &[u8], changes: &Changes) -> Result<Vec<u8>, TagWriteError> {
    if packet.first() != Some(&3) || packet.get(1..7) != Some(VORBIS.as_slice()) {
        return Err(corrupt(
            "the second Vorbis header is not the comment header",
        ));
    }
    let body = packet.get(7..).ok_or_else(|| corrupt("comment header"))?;
    let (mut comments, len) = Comments::parse(body)?;
    if body.get(len).is_none_or(|framing| framing & 1 == 0) {
        return Err(corrupt("the Vorbis comment header has no framing bit"));
    }
    comments.apply(changes, true);
    Ok([&[3][..], VORBIS, &comments.to_bytes()?, &[1]].concat())
}

/// `packets` cut into pages of at most 255 lacing values. Pages that hold only headers have
/// the granule position zero (Vorbis I, appendix A.2).
fn paginate(packets: &[Vec<u8>], serial: u32, first_sequence: u32) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut page = new_page(serial, first_sequence, 0);
    for packet in packets {
        // A packet is 255-byte segments, then one shorter segment, zero long when the packet
        // is a multiple of 255.
        let mut chunks = packet.chunks(255).peekable();
        let mut pieces: Vec<&[u8]> = Vec::new();
        while let Some(chunk) = chunks.next() {
            pieces.push(chunk);
            if chunks.peek().is_none() && chunk.len() == 255 {
                pieces.push(&[]);
            }
        }
        if pieces.is_empty() {
            pieces.push(&[]);
        }
        for (index, piece) in pieces.into_iter().enumerate() {
            if page.lacing.len() == MAX_SEGMENTS {
                let sequence = page.sequence.wrapping_add(1);
                let full = std::mem::replace(&mut page, new_page(serial, sequence, CONTINUED));
                pages.push(full);
            }
            // The page starts inside a packet only when this is not the packet's first piece.
            if page.lacing.is_empty() && index == 0 {
                page.kind = 0;
            }
            page.lacing.push(piece.len() as u8);
            page.body.extend_from_slice(piece);
        }
    }
    pages.push(page);
    pages
}

fn new_page(serial: u32, sequence: u32, kind: u8) -> Page {
    Page {
        kind,
        granule: [0; 8],
        serial,
        sequence,
        lacing: Vec::new(),
        body: Vec::new(),
        len: 0,
    }
}

/// Copies the pages from `at` on, moving the sequence number of each page of the stream by
/// `shift`. A chained stream after it is copied as it is. Bytes that are not a whole page are
/// refused: pages of the stream after them would keep their old numbers and leave a gap.
fn renumber(
    source: &mut Source,
    out: &mut impl Write,
    mut at: u64,
    serial: u32,
    shift: i64,
) -> Result<(), TagWriteError> {
    while at < source.len {
        let Some(mut page) = read_page(source, at)? else {
            return Err(corrupt(
                "bytes that are not Ogg pages follow the headers, whose pages had to be renumbered",
            ));
        };
        // A new first page with the same serial starts a chained stream, numbered on its own.
        if page.serial == serial && page.kind & FIRST_PAGE != 0 {
            break;
        }
        if page.serial == serial {
            page.sequence = u32::try_from(i64::from(page.sequence) + shift)
                .map_err(|_| unsupported("the Ogg page numbers overflow"))?;
            put(out, &page_bytes(&page))?;
        } else {
            source.copy(at, at + page.len, out)?;
        }
        at += page.len;
    }
    source.copy(at, source.len, out)
}

#[cfg(test)]
mod tests {
    use super::{crc, paginate};

    #[test]
    fn the_checksum_matches_the_bitwise_definition() {
        let bitwise = |bytes: &[u8]| {
            bytes.iter().fold(0u32, |crc, byte| {
                (0..8).fold(crc ^ (u32::from(*byte) << 24), |crc, _| {
                    if crc & 0x8000_0000 != 0 {
                        crc << 1 ^ 0x04C1_1DB7
                    } else {
                        crc << 1
                    }
                })
            })
        };
        let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        assert_eq!(crc(&bytes), bitwise(&bytes));
        assert_eq!(crc(b""), 0);
    }

    #[test]
    fn packets_are_laced_and_continued_across_pages() {
        // 255 bytes need a closing zero lacing value; 300 * 255 bytes need two pages.
        let pages = paginate(&[vec![1; 255], vec![2; 300 * 255]], 7, 1);
        assert_eq!(pages.len(), 2);
        let first = &pages[0];
        assert_eq!(&first.lacing[..3], &[255, 0, 255]);
        assert_eq!(first.lacing.len(), 255);
        assert_eq!(first.kind, 0);
        let second = &pages[1];
        assert_eq!(second.kind, super::CONTINUED);
        assert_eq!(second.sequence, 2);
        // 300 full segments and the closing zero, 253 of them on the first page.
        assert_eq!(second.lacing.len(), 301 - 253);
        assert_eq!(second.lacing.last(), Some(&0));
    }
}
