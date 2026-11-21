//! Reading a gzip file (RFC 1952) and the deflate data inside it (RFC 1951), for the archive of
//! the model. The output is bounded: a file that would grow past the limit is refused.

#[derive(Debug, PartialEq, Eq)]
pub enum InflateError {
    /// Not a gzip file, or damaged data.
    Format,
    /// The data would grow past the limit given.
    TooLarge,
}

struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
    bit: u32,
    count: u32,
}

impl Bits<'_> {
    fn need(&mut self, count: u32) -> Result<(), InflateError> {
        while self.count < count {
            let byte = *self.bytes.get(self.at).ok_or(InflateError::Format)?;
            self.at += 1;
            self.bit |= u32::from(byte) << self.count;
            self.count += 8;
        }
        Ok(())
    }

    fn take(&mut self, count: u32) -> Result<u32, InflateError> {
        if count == 0 {
            return Ok(0);
        }
        self.need(count)?;
        let value = self.bit & ((1u32 << count) - 1);
        self.bit >>= count;
        self.count -= count;
        Ok(value)
    }

    fn align(&mut self) {
        self.bit = 0;
        self.count = 0;
    }
}

/// A canonical Huffman code: the counts of each length and the symbols in code order.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, InflateError> {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[usize::from(length)] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for length in 1..16 {
            offsets[length] = offsets[length - 1] + counts[length - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let slot = &mut offsets[usize::from(length)];
                *symbols
                    .get_mut(usize::from(*slot))
                    .ok_or(InflateError::Format)? = symbol as u16;
                *slot += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, InflateError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for length in 1..16 {
            code |= bits.take(1)? as i32;
            let count = i32::from(self.counts[length]);
            if code - count < first {
                return self
                    .symbols
                    .get((index + code - first) as usize)
                    .copied()
                    .ok_or(InflateError::Format);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(InflateError::Format)
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn codes(
    bits: &mut Bits<'_>,
    literals: &Huffman,
    distances: &Huffman,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), InflateError> {
    loop {
        let symbol = literals.decode(bits)?;
        match symbol {
            0..=255 => {
                if out.len() >= limit {
                    return Err(InflateError::TooLarge);
                }
                out.push(symbol as u8);
            }
            256 => return Ok(()),
            _ => {
                let index = usize::from(symbol - 257);
                let length = usize::from(*LENGTH_BASE.get(index).ok_or(InflateError::Format)?)
                    + bits.take(u32::from(LENGTH_EXTRA[index]))? as usize;
                let code = usize::from(distances.decode(bits)?);
                let distance = usize::from(*DISTANCE_BASE.get(code).ok_or(InflateError::Format)?)
                    + bits.take(u32::from(DISTANCE_EXTRA[code]))? as usize;
                let start = out
                    .len()
                    .checked_sub(distance)
                    .ok_or(InflateError::Format)?;
                if out.len() + length > limit {
                    return Err(InflateError::TooLarge);
                }
                for offset in 0..length {
                    out.push(out[start + offset]);
                }
            }
        }
    }
}

fn dynamic(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), InflateError> {
    let literals = bits.take(5)? as usize + 257;
    let distances = bits.take(5)? as usize + 1;
    let lengths = bits.take(4)? as usize + 4;
    if literals > 286 || distances > 30 {
        return Err(InflateError::Format);
    }
    let mut code_lengths = [0u8; 19];
    for &slot in ORDER.iter().take(lengths) {
        code_lengths[slot] = bits.take(3)? as u8;
    }
    let code = Huffman::new(&code_lengths)?;
    let mut all = Vec::with_capacity(literals + distances);
    while all.len() < literals + distances {
        let symbol = code.decode(bits)?;
        let (value, repeat) = match symbol {
            0..=15 => (symbol as u8, 1),
            16 => (
                *all.last().ok_or(InflateError::Format)?,
                3 + bits.take(2)? as usize,
            ),
            17 => (0, 3 + bits.take(3)? as usize),
            18 => (0, 11 + bits.take(7)? as usize),
            _ => return Err(InflateError::Format),
        };
        if all.len() + repeat > literals + distances {
            return Err(InflateError::Format);
        }
        all.extend(std::iter::repeat_n(value, repeat));
    }
    Ok((
        Huffman::new(&all[..literals])?,
        Huffman::new(&all[literals..])?,
    ))
}

/// Inflates raw deflate data, up to `limit` bytes.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    let mut bits = Bits {
        bytes: data,
        at: 0,
        bit: 0,
        count: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => {
                bits.align();
                let header = data.get(bits.at..bits.at + 4).ok_or(InflateError::Format)?;
                let length = usize::from(u16::from_le_bytes([header[0], header[1]]));
                let check = u16::from_le_bytes([header[2], header[3]]);
                if check != !(length as u16) {
                    return Err(InflateError::Format);
                }
                bits.at += 4;
                let stored = data
                    .get(bits.at..bits.at + length)
                    .ok_or(InflateError::Format)?;
                if out.len() + length > limit {
                    return Err(InflateError::TooLarge);
                }
                out.extend_from_slice(stored);
                bits.at += length;
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let literals = Huffman::new(&lengths)?;
                let distances = Huffman::new(&[5u8; 30])?;
                codes(&mut bits, &literals, &distances, &mut out, limit)?;
            }
            2 => {
                let (literals, distances) = dynamic(&mut bits)?;
                codes(&mut bits, &literals, &distances, &mut out, limit)?;
            }
            _ => return Err(InflateError::Format),
        }
        if last {
            return Ok(out);
        }
    }
}

/// The content of a gzip file, up to `limit` bytes. Its CRC is not checked: the whole archive
/// is checked against its SHA-256 before.
pub fn gunzip(file: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    let header = file.get(..10).ok_or(InflateError::Format)?;
    if header[0] != 0x1f || header[1] != 0x8b || header[2] != 8 {
        return Err(InflateError::Format);
    }
    let flags = header[3];
    let mut at = 10;
    if flags & 0x04 != 0 {
        let extra = file.get(at..at + 2).ok_or(InflateError::Format)?;
        at += 2 + usize::from(u16::from_le_bytes([extra[0], extra[1]]));
    }
    for flag in [0x08, 0x10] {
        if flags & flag != 0 {
            let end = file
                .get(at..)
                .and_then(|rest| rest.iter().position(|byte| *byte == 0))
                .ok_or(InflateError::Format)?;
            at += end + 1;
        }
    }
    if flags & 0x02 != 0 {
        at += 2;
    }
    let body = file.get(at..).ok_or(InflateError::Format)?;
    let out = inflate(body, limit)?;
    // The last four bytes give the size of the content, modulo 2^32.
    let size = file
        .get(file.len().saturating_sub(4)..)
        .ok_or(InflateError::Format)?;
    if size.len() == 4
        && u32::from_le_bytes([size[0], size[1], size[2], size[3]]) != out.len() as u32
    {
        return Err(InflateError::Format);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made with Python 3: `gzip.compress(b"hello hello hello world\n" * 3, mtime=0)`, a block of
    // fixed codes, and the raw deflate at level 9 of 400 words drawn with `random.seed(7)` from
    // a list of words of Platter, a block of dynamic codes.
    const HELLO: [u8; 39] = [
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
        0x57, 0xc8, 0x40, 0x22, 0xcb, 0xf3, 0x8b, 0x72, 0x52, 0xb8, 0x90, 0x45, 0x88, 0x11, 0x07,
        0x00, 0xb7, 0xfb, 0xa4, 0xa5, 0x48, 0x00, 0x00, 0x00,
    ];
    const WORDS_DEFLATE: [u8; 561] = [
        0x6d, 0x55, 0x51, 0x76, 0x83, 0x30, 0x0c, 0xbb, 0x0a, 0x57, 0xa3, 0x90, 0x6e, 0x7d, 0xa5,
        0xa5, 0x8f, 0xd0, 0xbd, 0xf5, 0xf6, 0xc3, 0x92, 0x13, 0x2b, 0x61, 0x3f, 0x29, 0x25, 0x8a,
        0x2c, 0x2b, 0xb6, 0xc9, 0x7b, 0x7a, 0x0c, 0x73, 0x9a, 0xee, 0x43, 0x4e, 0xcf, 0x79, 0xb8,
        0x8c, 0x39, 0x0f, 0xeb, 0xfe, 0x9d, 0xb6, 0xe1, 0x92, 0xc6, 0x7d, 0xf8, 0x59, 0xa7, 0x71,
        0xc9, 0xc3, 0xf5, 0xb6, 0xec, 0xc7, 0xab, 0xaf, 0xed, 0xe6, 0x90, 0xfc, 0x79, 0x4e, 0xc3,
        0xb4, 0xad, 0x39, 0x5f, 0xc7, 0xd9, 0xc0, 0x71, 0xee, 0xf1, 0xde, 0x13, 0x17, 0xfe, 0x3f,
        0x02, 0xbc, 0x56, 0xe5, 0xc4, 0x1e, 0x0e, 0x80, 0xcf, 0x43, 0x10, 0x16, 0x11, 0xf0, 0x84,
        0x25, 0x84, 0x11, 0x83, 0x47, 0x30, 0x41, 0xf7, 0xf4, 0xf6, 0x70, 0xf8, 0xa7, 0xaa, 0x71,
        0xda, 0xb6, 0x19, 0xf6, 0xf6, 0x7b, 0x28, 0xd0, 0x2d, 0xee, 0x47, 0x12, 0x9e, 0xa5, 0x43,
        0x70, 0x88, 0xb2, 0x43, 0x95, 0xa0, 0x97, 0xb5, 0x84, 0x43, 0xf4, 0x6c, 0x46, 0xde, 0xd3,
        0x87, 0x60, 0x7b, 0x70, 0x3a, 0x13, 0x40, 0xe1, 0x54, 0xa0, 0x7e, 0x54, 0x85, 0xf0, 0x13,
        0x8c, 0x95, 0xc7, 0x5e, 0x13, 0xe5, 0x82, 0x80, 0xa1, 0xbb, 0x20, 0xca, 0xf5, 0xea, 0x70,
        0x30, 0x6c, 0x15, 0xaf, 0x69, 0xa0, 0x01, 0xb1, 0xb8, 0x24, 0xe0, 0xab, 0x4e, 0xc2, 0xb9,
        0x2e, 0xeb, 0xfa, 0xe2, 0xb6, 0x93, 0x20, 0xe9, 0x43, 0x49, 0x45, 0x43, 0xad, 0xdd, 0x88,
        0x73, 0xcd, 0xdb, 0xfb, 0x91, 0x35, 0xdf, 0xc6, 0x67, 0x9a, 0xd4, 0x39, 0x67, 0x14, 0xd0,
        0x4d, 0x2b, 0xc0, 0x86, 0x45, 0x02, 0x93, 0xc5, 0x68, 0x79, 0xfd, 0x96, 0x0c, 0xc4, 0xe1,
        0x20, 0x73, 0xad, 0xef, 0xf0, 0xd7, 0xc3, 0x03, 0x4e, 0x62, 0x40, 0x95, 0x4e, 0x82, 0x3a,
        0x02, 0xe2, 0xc3, 0x0e, 0xc2, 0xc0, 0x09, 0x91, 0xd8, 0xee, 0x22, 0x6a, 0x2f, 0xc4, 0x15,
        0xe0, 0x72, 0x90, 0xa8, 0x69, 0x06, 0x30, 0xf2, 0x8a, 0x45, 0x6d, 0x89, 0xba, 0x16, 0x6f,
        0x28, 0x57, 0x5e, 0x18, 0x5d, 0x63, 0x29, 0x62, 0xe2, 0x98, 0xbf, 0xa0, 0x4a, 0xe8, 0x39,
        0xb5, 0x40, 0x73, 0x49, 0x27, 0x6e, 0x28, 0xf0, 0x0a, 0x3a, 0x52, 0x76, 0xb0, 0x16, 0x89,
        0xd3, 0xa8, 0x6c, 0x13, 0x84, 0x07, 0x2c, 0x51, 0xa5, 0xe0, 0x51, 0x8d, 0x51, 0x4b, 0x5e,
        0xaf, 0x66, 0x11, 0x95, 0xa8, 0x06, 0x7b, 0x5b, 0x64, 0x56, 0xf9, 0x84, 0x71, 0xca, 0xd4,
        0x08, 0x20, 0x54, 0x3c, 0x79, 0xfd, 0x8f, 0x0f, 0x07, 0x3b, 0x4d, 0xfb, 0x0d, 0x08, 0x21,
        0xdc, 0x91, 0x98, 0x52, 0x77, 0xa7, 0xbd, 0xe8, 0xc4, 0xc6, 0xbb, 0x52, 0x29, 0x25, 0x25,
        0xd6, 0xc8, 0x69, 0x76, 0x48, 0x1f, 0xf8, 0x8f, 0x0e, 0xc1, 0x66, 0xd4, 0xd1, 0x3e, 0x09,
        0x6c, 0x52, 0xfb, 0xf9, 0x22, 0x05, 0xda, 0xf0, 0x15, 0x9f, 0x2d, 0x85, 0xfe, 0x0c, 0x5d,
        0x89, 0xa1, 0xc4, 0x13, 0x80, 0xd6, 0x7e, 0xd2, 0xae, 0x90, 0xde, 0x90, 0x82, 0xaf, 0x0d,
        0x1f, 0x13, 0xe6, 0x74, 0x4b, 0x31, 0x84, 0xd5, 0x24, 0x1d, 0x57, 0xd1, 0x3b, 0xda, 0xff,
        0xf1, 0x28, 0xb6, 0x76, 0x03, 0x02, 0xc7, 0xe5, 0xa3, 0x10, 0x25, 0x15, 0x33, 0x97, 0x4a,
        0xac, 0x15, 0xc4, 0x7e, 0xa2, 0xeb, 0xac, 0x0c, 0x19, 0xa1, 0x96, 0xa5, 0x11, 0xe5, 0x18,
        0x3d, 0xa6, 0x1d, 0x1f, 0xb6, 0x68, 0x2b, 0x30, 0x71, 0x7c, 0x03, 0x31, 0x56, 0x2b, 0x95,
        0x8f, 0xa3, 0xff, 0x90, 0xa7, 0x22, 0x63, 0x1e, 0xd2, 0xc3, 0x60, 0xa8, 0x93, 0x43, 0xdb,
        0x15, 0x57, 0xd6, 0x46, 0xea, 0x89, 0x00, 0x69, 0x25, 0x88, 0x7b, 0x5e, 0xd0, 0xc5, 0xb8,
        0x6e, 0xb4, 0xc4, 0x05, 0x69, 0x49, 0xd5, 0xa0, 0x5a, 0xbd, 0xf2, 0x4d, 0xef, 0xee, 0xaa,
        0x0c, 0xa2, 0xd4, 0x15, 0x49, 0x0c, 0x6c, 0x23, 0x6c, 0x7a, 0x20, 0xe6, 0xbd, 0x7e, 0x15,
        0xb5, 0x74, 0x91, 0x2e, 0x06, 0xa7, 0x09, 0xe9, 0x6b, 0xa8, 0x4c, 0xfb, 0x10, 0xd9, 0xb4,
        0x2b, 0x87, 0x64, 0x1a, 0xf7, 0x3f,
    ];
    const WORDS_SEED_LEN: usize = 2272;
    const WORDS_SHA256: &str = "d8e7f7770dfe397d8c5ebb842d644581c8b0597216220cc9c98e5c0f9359fbff";

    #[test]
    fn fixed_and_dynamic_blocks_are_inflated() {
        assert_eq!(
            gunzip(&HELLO, 1000),
            Ok(b"hello hello hello world\n".repeat(3))
        );
        let words = inflate(&WORDS_DEFLATE, 10_000).unwrap();
        assert_eq!(words.len(), WORDS_SEED_LEN);
        assert_eq!(crate::sha256::hex_digest(&words), WORDS_SHA256);
        assert_eq!(inflate(&WORDS_DEFLATE, 1000), Err(InflateError::TooLarge));
    }

    #[test]
    fn a_stored_block_comes_out_as_it_went_in() {
        // A last stored block of three bytes.
        let data = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
        assert_eq!(inflate(&data, 10), Ok(b"abc".to_vec()));
        assert_eq!(inflate(&data, 2), Err(InflateError::TooLarge));
        assert_eq!(inflate(&data[..6], 10), Err(InflateError::Format));
    }

    #[test]
    fn damaged_gzip_is_refused() {
        assert_eq!(gunzip(b"not gzip at all", 100), Err(InflateError::Format));
        assert_eq!(gunzip(&HELLO[..20], 100), Err(InflateError::Format));
    }
}
