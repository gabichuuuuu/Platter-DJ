//! Ogg pages, after RFC 3533, *The Ogg Encapsulation Format Version 0*
//! (<https://www.rfc-editor.org/rfc/rfc3533>): packets are cut into segments of up to 255 bytes,
//! listed in the segment table of a page, and every page carries a CRC of its bytes.

/// Most segments one page lists (RFC 3533, section 6).
const MAX_SEGMENTS: usize = 255;

/// The flags of the header type of a page (RFC 3533, section 6).
const CONTINUED: u8 = 0x01;
const FIRST: u8 = 0x02;
const LAST: u8 = 0x04;

/// The CRC of Ogg: polynomial 0x04c11db7, initial value 0, no reflection, no final XOR
/// (RFC 3533, section 6, *CRC checksum*).
fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (index, entry) in table.iter_mut().enumerate() {
        let mut value = (index as u32) << 24;
        for _ in 0..8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04c1_1db7
            } else {
                value << 1
            };
        }
        *entry = value;
    }
    table
}

pub(crate) fn crc(table: &[u32; 256], bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |crc, byte| {
        (crc << 8) ^ table[((crc >> 24) as u8 ^ byte) as usize]
    })
}

/// Writes the pages of one logical stream.
pub struct PageWriter {
    serial: u32,
    sequence: u32,
    started: bool,
    table: [u32; 256],
    /// The packets of the page being filled: their segment table and their bytes.
    segments: Vec<u8>,
    body: Vec<u8>,
    /// A packet that did not fit continues on the next page.
    continued: bool,
    /// The granule of the last packet added.
    pending_granule: u64,
}

impl PageWriter {
    pub fn new(serial: u32) -> Self {
        Self {
            serial,
            sequence: 0,
            started: false,
            table: crc_table(),
            segments: Vec::new(),
            body: Vec::new(),
            continued: false,
            pending_granule: 0,
        }
    }

    /// Adds a packet to the page being filled; full pages go to `out`, each with `granule`, the
    /// position after the last packet that ends on it.
    pub fn packet(&mut self, packet: &[u8], granule: u64, out: &mut Vec<u8>) {
        // A packet of n bytes takes n / 255 segments of 255 and one shorter, which may be empty.
        let mut rest = packet;
        loop {
            if self.segments.len() == MAX_SEGMENTS {
                // The packet goes on: this page ends no packet, so its granule is -1.
                self.write(u64::MAX, false, out);
                self.continued = true;
            }
            let length = rest.len().min(255);
            self.segments.push(length as u8);
            self.body.extend_from_slice(&rest[..length]);
            rest = &rest[length..];
            if length < 255 {
                break;
            }
        }
        self.pending_granule = granule;
    }

    /// Writes the page being filled, if it holds anything, as the last of the stream when
    /// `last`.
    pub fn flush(&mut self, last: bool, out: &mut Vec<u8>) {
        if !self.segments.is_empty() || last {
            self.write(self.pending_granule, last, out);
        }
    }

    fn write(&mut self, granule: u64, last: bool, out: &mut Vec<u8>) {
        let start = out.len();
        let mut flags = 0;
        if self.continued {
            flags |= CONTINUED;
        }
        if !self.started {
            flags |= FIRST;
        }
        if last {
            flags |= LAST;
        }
        out.extend_from_slice(b"OggS");
        out.push(0);
        out.push(flags);
        out.extend_from_slice(&granule.to_le_bytes());
        out.extend_from_slice(&self.serial.to_le_bytes());
        out.extend_from_slice(&self.sequence.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.push(self.segments.len() as u8);
        out.extend_from_slice(&self.segments);
        out.extend_from_slice(&self.body);
        let checksum = crc(&self.table, &out[start..]);
        out[start + 22..start + 26].copy_from_slice(&checksum.to_le_bytes());
        self.sequence = self.sequence.wrapping_add(1);
        self.started = true;
        self.continued = false;
        self.segments.clear();
        self.body.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pages in `bytes`: their flags, granule and segment table.
    fn read(mut bytes: &[u8]) -> Vec<(u8, u64, Vec<u8>)> {
        let mut pages = Vec::new();
        while !bytes.is_empty() {
            let count = usize::from(bytes[26]);
            let segments = bytes[27..27 + count].to_vec();
            let length = 27 + count + segments.iter().map(|s| usize::from(*s)).sum::<usize>();
            let granule = u64::from_le_bytes(bytes[6..14].try_into().unwrap_or_default());
            pages.push((bytes[5], granule, segments));
            bytes = &bytes[length..];
        }
        pages
    }

    #[test]
    fn a_packet_longer_than_a_page_continues_on_the_next_one() {
        let mut writer = PageWriter::new(1);
        let mut out = Vec::new();
        // 300 segments of 255 bytes and one of 10: more than one page holds.
        writer.packet(&vec![7; 300 * 255 + 10], 960, &mut out);
        writer.flush(true, &mut out);
        let pages = read(&out);
        assert_eq!(pages.len(), 2);
        let (first_flags, first_granule, first) = &pages[0];
        assert_eq!(first.len(), 255);
        // No packet ends on the first page.
        assert_eq!(*first_granule, u64::MAX);
        assert_eq!(first_flags & CONTINUED, 0);
        let (second_flags, second_granule, second) = &pages[1];
        assert_eq!(second_flags & CONTINUED, CONTINUED);
        assert_eq!(second_flags & LAST, LAST);
        assert_eq!(*second_granule, 960);
        assert_eq!(second.len(), 46);
        assert_eq!(second.last(), Some(&10));
    }

    #[test]
    fn a_packet_of_whole_segments_ends_with_an_empty_one() {
        let mut writer = PageWriter::new(1);
        let mut out = Vec::new();
        writer.packet(&[1; 510], 0, &mut out);
        writer.flush(false, &mut out);
        assert_eq!(read(&out)[0].2, [255, 255, 0]);
    }

    #[test]
    fn the_crc_of_a_known_page_matches() {
        // The CRC of the bytes "OggS" with this polynomial, worked by hand bit by bit.
        let table = crc_table();
        let mut slow = 0u32;
        for byte in b"OggS" {
            slow ^= u32::from(*byte) << 24;
            for _ in 0..8 {
                slow = if slow & 0x8000_0000 != 0 {
                    (slow << 1) ^ 0x04c1_1db7
                } else {
                    slow << 1
                };
            }
        }
        assert_eq!(crc(&table, b"OggS"), slow);
    }
}
