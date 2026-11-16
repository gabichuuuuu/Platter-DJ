//! The Vorbis comment: a vendor string and a list of `NAME=value` fields with little-endian
//! lengths, the same in FLAC and in the comment header of Ogg Vorbis (which adds a framing
//! bit the caller handles). Field names compare without case.

use super::{Changes, Field, TagWriteError, corrupt, le_u32, slice, to_u32, to_usize};

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

    pub(super) fn apply(&mut self, changes: &Changes) {
        for field in Field::ALL {
            if let Some(value) = changes.field(field) {
                self.set(name(field), value);
            }
        }
        if let Some(bpm) = changes.bpm_decimal() {
            self.set("BPM", bpm.as_deref());
        }
    }

    /// Removes every field called `key` and puts `value`, when given, where the first was.
    fn set(&mut self, key: &str, value: Option<&str>) {
        let matches = |field: &Vec<u8>| {
            field
                .split(|byte| *byte == b'=')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(key.as_bytes()))
                && field.contains(&b'=')
        };
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
