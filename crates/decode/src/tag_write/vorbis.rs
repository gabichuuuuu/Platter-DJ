//! The Vorbis comment: a vendor string and a list of `NAME=value` fields with little-endian
//! lengths, the same in FLAC and in the comment header of Ogg Vorbis (which adds a framing
//! bit the caller handles). Field names compare without case.
//!
//! A picture in a comment is a FLAC picture block in base64 (`METADATA_BLOCK_PICTURE`), or,
//! in older files, a picture without a type (`COVERART`); the decoding library reads both.

use super::flac::picture_block;
use super::id3::FRONT_COVER;
use super::{Changes, Field, TagWriteError, corrupt, le_u32, slice, to_u32, to_usize};

const PICTURE: &str = "METADATA_BLOCK_PICTURE";
const OLD_PICTURE: &str = "COVERART";
/// The alphabet of base64 (RFC 4648, section 4).
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(super) struct Comments {
    vendor: Vec<u8>,
    /// Each field as stored, `NAME=value`; fields the edit does not name stay as they are.
    fields: Vec<Vec<u8>>,
}

fn name(field: Field) -> &'static str {
    match field {
        Field::Title => "TITLE",
        Field::Artist => "ARTIST",
        Field::Album => "ALBUM",
        Field::Genre => "GENRE",
        Field::Comment => "COMMENT",
        Field::Key => "INITIALKEY",
        Field::Platter => "PLATTER",
    }
}

impl Comments {
    pub(super) fn new() -> Self {
        Self {
            vendor: Vec::new(),
            fields: Vec::new(),
        }
    }

    /// The comment at the start of `bytes`, and the number of bytes it takes.
    pub(super) fn parse(bytes: &[u8]) -> Result<(Self, usize), TagWriteError> {
        let vendor_len = to_usize(le_u32(bytes, 0)?)?;
        let vendor = slice(bytes, 4, vendor_len)?.to_vec();
        let mut at = 4 + vendor_len;
        let count = to_usize(le_u32(bytes, at)?)?;
        at += 4;
        // Each field takes at least its four-byte length.
        if count > bytes.len().saturating_sub(at) / 4 {
            return Err(corrupt("the comment declares more fields than it holds"));
        }
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            let len = to_usize(le_u32(bytes, at)?)?;
            fields.push(slice(bytes, at + 4, len)?.to_vec());
            at += 4 + len;
        }
        Ok((Self { vendor, fields }, at))
    }

    /// Applies the changes. With `picture_inside`, an imported cover goes into the comment;
    /// otherwise the caller stores it elsewhere (a FLAC picture block) and the comment only
    /// loses the pictures the edit replaces.
    pub(super) fn apply(&mut self, changes: &Changes, picture_inside: bool) {
        for field in Field::ALL {
            if let Some(value) = changes.field(field) {
                self.set(name(field), value);
            }
        }
        if let Some(bpm) = changes.bpm_decimal() {
            self.set("BPM", bpm.as_deref());
        }
        match changes.cover() {
            Some(Some(picture)) => {
                let first = self
                    .fields
                    .iter()
                    .position(|field| is_named(field, PICTURE) || is_named(field, OLD_PICTURE));
                self.fields.retain(|field| {
                    !is_named(field, OLD_PICTURE)
                        && !(is_named(field, PICTURE) && is_front_cover(field))
                });
                if picture_inside {
                    let field = format!("{PICTURE}={}", base64(&picture_block(picture)));
                    let at = first.unwrap_or(self.fields.len()).min(self.fields.len());
                    self.fields.insert(at, field.into_bytes());
                }
            }
            Some(None) => self
                .fields
                .retain(|field| !is_named(field, PICTURE) && !is_named(field, OLD_PICTURE)),
            None => {}
        }
    }

    /// Removes every field called `key` and puts `value`, when given, where the first was.
    fn set(&mut self, key: &str, value: Option<&str>) {
        let matches = |field: &Vec<u8>| is_named(field, key);
        let first = self.fields.iter().position(matches);
        self.fields.retain(|field| !matches(field));
        if let Some(value) = value {
            let field = format!("{key}={value}").into_bytes();
            match first {
                Some(at) if at <= self.fields.len() => self.fields.insert(at, field),
                _ => self.fields.push(field),
            }
        }
    }

    pub(super) fn to_bytes(&self) -> Result<Vec<u8>, TagWriteError> {
        let mut out = Vec::new();
        out.extend_from_slice(&to_u32(self.vendor.len())?.to_le_bytes());
        out.extend_from_slice(&self.vendor);
        out.extend_from_slice(&to_u32(self.fields.len())?.to_le_bytes());
        for field in &self.fields {
            out.extend_from_slice(&to_u32(field.len())?.to_le_bytes());
            out.extend_from_slice(field);
        }
        Ok(out)
    }
}

/// Whether `field` is `key=value`, the name compared without case.
fn is_named(field: &[u8], key: &str) -> bool {
    field
        .split(|byte| *byte == b'=')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(key.as_bytes()))
        && field.contains(&b'=')
}

/// Whether a `METADATA_BLOCK_PICTURE` field holds a front cover: the picture type is the first
/// four bytes of the block, the first eight characters of its base64.
fn is_front_cover(field: &[u8]) -> bool {
    let value = field
        .iter()
        .position(|byte| *byte == b'=')
        .and_then(|at| field.get(at + 1..at + 9));
    let Some(value) = value else {
        return false;
    };
    let mut bits: u64 = 0;
    for character in value {
        let Some(sextet) = BASE64.iter().position(|letter| letter == character) else {
            return false;
        };
        bits = bits << 6 | sextet as u64;
    }
    // Eight characters carry six bytes; the type is the first four.
    bits >> 16 == u64::from(FRONT_COVER)
}

/// `bytes` in base64 with padding (RFC 4648, section 4).
pub(super) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|index| group.get(index).copied().unwrap_or(0));
        let bits = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        for index in 0..4 {
            if index <= group.len() {
                let sextet = (bits >> (18 - 6 * index)) & 0x3F;
                out.push(char::from(BASE64[sextet as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64, is_front_cover};

    #[test]
    fn base64_follows_the_examples_of_its_specification() {
        // RFC 4648, section 10.
        for (input, output) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), output);
        }
    }

    #[test]
    fn the_type_of_an_encoded_picture_is_read_from_its_first_characters() {
        let field = |kind: u32| {
            let block = [&kind.to_be_bytes()[..], &[0, 0, 0, 9, b'i']].concat();
            format!("metadata_block_picture={}", base64(&block)).into_bytes()
        };
        assert!(is_front_cover(&field(3)));
        assert!(!is_front_cover(&field(4)));
        assert!(!is_front_cover(b"METADATA_BLOCK_PICTURE=AA"));
        assert!(!is_front_cover(b"METADATA_BLOCK_PICTURE=!!!!!!!!"));
    }
}
