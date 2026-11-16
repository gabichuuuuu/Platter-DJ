//! ID3v2 tags (versions 2.2, 2.3 and 2.4) and the ID3v1 tag at the end of an MP3 file.
//!
//! A tag keeps the version it was found in, so every frame the edit does not name passes
//! through byte for byte, compressed and encrypted frames included: converting between
//! versions would rewrite their headers. A new tag is ID3v2.4. Unsynchronisation is undone
//! on reading and the tag is written plain. The extended header and the footer are not
//! written back: the CRC of the one would no longer match, and the other forbids padding.

use std::io::Write;

use super::{
    Change, Changes, Field, GROWTH_PADDING, Source, TagWriteError, be_u32, corrupt, put, slice,
    to_usize, unsupported,
};

/// Frames read from one tag before it is refused.
const MAX_FRAMES: usize = 8192;
/// Description of the user text frame that holds Platter's own data.
pub(super) const PLATTER_DESCRIPTION: &str = "PLATTER";
/// Language of a new comment frame: "undetermined" in ISO 639-2.
const UNDETERMINED: &[u8; 3] = b"und";

/// One frame: its identifier, its two flag bytes (zero in ID3v2.2) and its data.
#[derive(Clone, Debug)]
struct Frame {
    id: Vec<u8>,
    flags: [u8; 2],
    body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(super) struct Tag {
    version: u8,
    revision: u8,
    /// Tag flags to write back: only the experimental bit survives.
    flags: u8,
    frames: Vec<Frame>,
    /// Bytes the tag took in the file; zero for a new tag.
    room: usize,
}

/// The length of the ID3v2 tag that starts with `header` (its first ten bytes), footer
/// included; `None` when `header` does not start a tag.
pub(super) fn tag_len(header: &[u8]) -> Result<Option<usize>, TagWriteError> {
    if header.get(..3) != Some(b"ID3".as_slice()) {
        return Ok(None);
    }
    let version = *header.get(3).ok_or_else(|| corrupt("ID3v2 header"))?;
    let flags = *header.get(5).ok_or_else(|| corrupt("ID3v2 header"))?;
    let size = synchsafe(slice(header, 6, 4)?)?;
    let footer = if version == 4 && flags & 0x10 != 0 {
        10
    } else {
        0
    };
    Ok(Some(10 + footer + to_usize(size)?))
}

fn synchsafe(bytes: &[u8]) -> Result<u32, TagWriteError> {
    bytes.iter().try_fold(0u32, |value, byte| {
        if byte & 0x80 != 0 {
            return Err(corrupt("an ID3v2 size is not synchsafe"));
        }
        Ok(value << 7 | u32::from(*byte))
    })
}

fn synchsafe_bytes(value: usize) -> Result<[u8; 4], TagWriteError> {
    if value > 0x0FFF_FFFF {
        return Err(unsupported("an ID3v2 tag is too large"));
    }
    Ok([
        (value >> 21 & 0x7F) as u8,
        (value >> 14 & 0x7F) as u8,
        (value >> 7 & 0x7F) as u8,
        (value & 0x7F) as u8,
    ])
}

/// `bytes` with the zero that unsynchronisation puts after each 0xFF removed.
fn resynchronised(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut after_ff = false;
    for byte in bytes {
        if !(after_ff && *byte == 0) {
            out.push(*byte);
        }
        after_ff = *byte == 0xFF;
    }
    out
}

impl Tag {
    /// An empty ID3v2.4 tag.
    pub(super) fn new() -> Self {
        Self {
            version: 4,
            revision: 0,
            flags: 0,
            frames: Vec::new(),
            room: 0,
        }
    }

    /// The tag in `bytes`, which hold it whole, from its header to its footer.
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, TagWriteError> {
        let header = slice(bytes, 0, 10)?;
        let (version, revision, flags) = match header {
            [b'I', b'D', b'3', version, revision, flags, ..] => (*version, *revision, *flags),
            _ => return Err(corrupt("an ID3v2 tag has no header")),
        };
        if !(2..=4).contains(&version) {
            return Err(TagWriteError::Unsupported(format!(
                "an ID3v2.{version} tag"
            )));
        }
        let size = to_usize(synchsafe(slice(header, 6, 4)?)?)?;
        let stored = slice(bytes, 10, size)?;
        let unsynchronised = flags & 0x80 != 0;
        // Before 2.4, unsynchronisation covers the whole tag after its header.
        let body = if version < 4 && unsynchronised {
            resynchronised(stored)
        } else {
            stored.to_vec()
        };
        let start = match version {
            2 if flags & 0x40 != 0 => {
                return Err(unsupported("a compressed ID3v2.2 tag"));
            }
            3 if flags & 0x40 != 0 => to_usize(be_u32(&body, 0)?)?
                .checked_add(4)
                .ok_or_else(|| corrupt("ID3v2 extended header"))?,
            4 if flags & 0x40 != 0 => to_usize(synchsafe(slice(&body, 0, 4)?)?)?,
            _ => 0,
        };
        if start > body.len() {
            return Err(corrupt("the ID3v2 extended header runs past the tag"));
        }
        let frames = read_frames(&body, start, version, version == 4 && unsynchronised)?;
        Ok(Self {
            version,
            revision,
            flags: flags & 0x20,
            frames,
            room: bytes.len(),
        })
    }

    /// Sets the space the tag may fill without moving what follows it.
    pub(super) fn set_room(&mut self, room: usize) {
        self.room = room;
    }

    pub(super) fn has_frames(&self) -> bool {
        !self.frames.is_empty()
    }

    pub(super) fn apply(&mut self, changes: &Changes) {
        for field in Field::ALL {
            if let Some(value) = changes.field(field) {
                self.set(field, value);
            }
        }
        if let Some(bpm) = changes.bpm_whole() {
            let id = self.id(b"TBP", b"TBPM");
            self.set_text(id, bpm.as_deref());
        }
    }

    /// The identifier of a frame: three characters in ID3v2.2, four after.
    fn id(&self, v22: &'static [u8; 3], later: &'static [u8; 4]) -> &'static [u8] {
        if self.version == 2 { v22 } else { later }
    }

    /// Sets or removes one field.
    pub(super) fn set(&mut self, field: Field, value: Option<&str>) {
        match field {
            Field::Title => self.set_text(self.id(b"TT2", b"TIT2"), value),
            Field::Artist => self.set_text(self.id(b"TP1", b"TPE1"), value),
            Field::Album => self.set_text(self.id(b"TAL", b"TALB"), value),
            Field::Genre => self.set_text(self.id(b"TCO", b"TCON"), value),
            Field::Key => self.set_text(self.id(b"TKE", b"TKEY"), value),
            Field::Comment => self.set_comment(value),
            Field::Platter => self.set_platter(value),
        }
    }

    fn set_text(&mut self, id: &[u8], value: Option<&str>) {
        let new = value.map(|text| {
            let encoding = self.encoding(text);
            [vec![encoding], encode(encoding, text)].concat()
        });
        self.replace(|frame| frame.id == id, id, new);
    }

    /// Replaces the comment frames without a description. A comment with one is data other
    /// software keeps for itself, and so is a comment the writer cannot read (compressed or
    /// encrypted): both stay.
    fn set_comment(&mut self, value: Option<&str>) {
        let id = self.id(b"COM", b"COMM");
        let version = self.version;
        let plain = |frame: &Frame| {
            frame.id == id
                && frame
                    .content(version)
                    .is_some_and(|content| described(content, 4).is_none_or(|d| d.is_empty()))
        };
        let language = self
            .frames
            .iter()
            .find(|frame| plain(frame))
            .and_then(|frame| frame.content(version))
            .and_then(|content| content.get(1..4))
            .map_or(*UNDETERMINED, |language| {
                language.try_into().unwrap_or(*UNDETERMINED)
            });
        let new = value.map(|text| {
            let encoding = self.encoding(text);
            [
                vec![encoding],
                language.to_vec(),
                encode(encoding, ""),
                terminator(encoding).to_vec(),
                encode(encoding, text),
            ]
            .concat()
        });
        self.replace(plain, id, new);
    }

    /// Replaces the user text frames with Platter's description; other user text frames stay.
    fn set_platter(&mut self, value: Option<&str>) {
        let id = self.id(b"TXX", b"TXXX");
        let version = self.version;
        let ours = |frame: &Frame| {
            frame.id == id
                && frame
                    .content(version)
                    .and_then(|content| described(content, 1))
                    .is_some_and(|description| description == PLATTER_DESCRIPTION)
        };
        let new = value.map(|data| {
            let encoding = self.encoding(data);
            [
                vec![encoding],
                encode(encoding, PLATTER_DESCRIPTION),
                terminator(encoding).to_vec(),
                encode(encoding, data),
            ]
            .concat()
        });
        self.replace(ours, id, new);
    }

    /// Removes the frames `matches` picks and puts `new`, when given, where the first of them
    /// was, or at the end.
    fn replace(&mut self, matches: impl Fn(&Frame) -> bool, id: &[u8], new: Option<Vec<u8>>) {
        let first = self.frames.iter().position(&matches);
        self.frames.retain(|frame| !matches(frame));
        if let Some(body) = new {
            let frame = Frame {
                id: id.to_vec(),
                flags: [0, 0],
                body,
            };
            match first {
                Some(at) if at <= self.frames.len() => self.frames.insert(at, frame),
                _ => self.frames.push(frame),
            }
        }
    }

    /// UTF-8 in ID3v2.4; before, ISO-8859-1 when the text fits it and UTF-16 otherwise.
    fn encoding(&self, text: &str) -> u8 {
        if self.version == 4 {
            3
        } else if text.chars().all(|character| u32::from(character) <= 0xFF) {
            0
        } else {
            1
        }
    }

    /// The tag as bytes. It fills the room the old tag took when it fits, so the audio stays
    /// where it was; otherwise it is followed by `growth` bytes of padding.
    pub(super) fn to_bytes(&self, growth: usize) -> Result<Vec<u8>, TagWriteError> {
        let mut frames = Vec::new();
        for frame in &self.frames {
            frames.extend_from_slice(&frame.id);
            let size = frame.body.len();
            match self.version {
                2 => {
                    if size > 0xFF_FFFF {
                        return Err(unsupported("an ID3v2.2 frame is too large"));
                    }
                    frames.extend_from_slice(&[(size >> 16) as u8, (size >> 8) as u8, size as u8]);
                }
                3 => frames.extend_from_slice(&super::to_u32(size)?.to_be_bytes()),
                _ => frames.extend_from_slice(&synchsafe_bytes(size)?),
            }
            if self.version > 2 {
                frames.extend_from_slice(&frame.flags);
            }
            frames.extend_from_slice(&frame.body);
        }
        let padding = self.room.checked_sub(10 + frames.len()).unwrap_or(growth);
        let size = synchsafe_bytes(frames.len() + padding)?;
        let mut out = Vec::with_capacity(10 + frames.len() + padding);
        out.extend_from_slice(b"ID3");
        out.extend_from_slice(&[self.version, self.revision, self.flags]);
        out.extend_from_slice(&size);
        out.extend_from_slice(&frames);
        out.resize(out.len() + padding, 0);
        Ok(out)
    }
}

impl Frame {
    /// The data of the frame after the bytes its flags add in front, or `None` when it is
    /// compressed or encrypted.
    fn content(&self, version: u8) -> Option<&[u8]> {
        let [_, format] = self.flags;
        let skip = match version {
            3 if format & 0xC0 != 0 => return None,
            3 => usize::from(format & 0x20 != 0),
            4 if format & 0x0C != 0 => return None,
            4 => usize::from(format & 0x40 != 0) + 4 * usize::from(format & 0x01 != 0),
            _ => 0,
        };
        self.body.get(skip..)
    }
}

fn read_frames(
    body: &[u8],
    start: usize,
    version: u8,
    all_unsynchronised: bool,
) -> Result<Vec<Frame>, TagWriteError> {
    let (id_len, header_len) = if version == 2 { (3, 6) } else { (4, 10) };
    let mut frames = Vec::new();
    let mut at = start;
    // A zero where a frame would start is the padding; so is a remainder too short to hold
    // a frame header.
    while let Some(rest) = body.get(at..)
        && rest.len() >= header_len
        && rest.first() != Some(&0)
    {
        if frames.len() >= MAX_FRAMES {
            return Err(unsupported("an ID3v2 tag with too many frames"));
        }
        let id = slice(rest, 0, id_len)?;
        if !id
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(corrupt("an ID3v2 frame has an invalid identifier"));
        }
        let size = match version {
            2 => slice(rest, 3, 3)?
                .iter()
                .fold(0u32, |size, byte| size << 8 | u32::from(*byte)),
            3 => be_u32(rest, 4)?,
            _ => synchsafe(slice(rest, 4, 4)?)?,
        };
        let size = to_usize(size)?;
        let mut flags = [0, 0];
        if version > 2 {
            flags.copy_from_slice(slice(rest, 8, 2)?);
        }
        let data = slice(rest, header_len, size)
            .map_err(|_| corrupt("an ID3v2 frame runs past the tag"))?;
        // In 2.4 unsynchronisation is applied frame by frame, and the tag flag says every
        // frame has it.
        let body = if version == 4 && (flags[1] & 0x02 != 0 || all_unsynchronised) {
            flags[1] &= !0x02;
            resynchronised(data)
        } else {
            data.to_vec()
        };
        frames.push(Frame {
            id: id.to_vec(),
            flags,
            body,
        });
        at += header_len + size;
    }
    Ok(frames)
}

/// The description of a comment or user text frame, whose text starts at `offset` of
/// `content` (after the encoding byte, and the language of a comment). `None` when the frame
/// is too short to hold one.
fn described(content: &[u8], offset: usize) -> Option<String> {
    let encoding = *content.first()?;
    let text = content.get(offset..)?;
    let end = match encoding {
        1 | 2 => text
            .as_chunks::<2>()
            .0
            .iter()
            .position(|unit| *unit == [0, 0])
            .map(|units| units * 2),
        _ => text.iter().position(|byte| *byte == 0),
    }?;
    Some(decode(encoding, text.get(..end)?))
}

fn decode(encoding: u8, bytes: &[u8]) -> String {
    match encoding {
        0 => bytes.iter().map(|byte| char::from(*byte)).collect(),
        1 | 2 => {
            let (little, bytes) = match bytes {
                [0xFF, 0xFE, rest @ ..] => (true, rest),
                [0xFE, 0xFF, rest @ ..] => (false, rest),
                // Without a byte order mark, ID3v2.4 encoding 2 is big-endian.
                _ => (encoding == 1, bytes),
            };
            let units = bytes.as_chunks::<2>().0.iter().map(|unit| {
                if little {
                    u16::from_le_bytes(*unit)
                } else {
                    u16::from_be_bytes(*unit)
                }
            });
            char::decode_utf16(units)
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `text` in a text encoding of ID3v2: 0 is ISO-8859-1, 1 UTF-16 with a byte order mark,
/// 3 UTF-8. Characters ISO-8859-1 lacks become `?`; the caller picks UTF-16 for those.
fn encode(encoding: u8, text: &str) -> Vec<u8> {
    match encoding {
        0 => text
            .chars()
            .map(|character| u8::try_from(character).unwrap_or(b'?'))
            .collect(),
        1 => [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
        _ => text.as_bytes().to_vec(),
    }
}

fn terminator(encoding: u8) -> &'static [u8] {
    if matches!(encoding, 1 | 2) {
        &[0, 0]
    } else {
        &[0]
    }
}

/// Edits the ID3v2 tag that `body` (a RIFF or AIFF chunk) holds and returns the new body.
pub(super) fn edit_chunk(body: &[u8], changes: &Changes) -> Result<Vec<u8>, TagWriteError> {
    let len =
        tag_len(slice(body, 0, 10)?)?.ok_or_else(|| corrupt("an ID3 chunk holds no ID3v2 tag"))?;
    let mut tag = Tag::parse(slice(body, 0, len)?)?;
    tag.set_room(body.len());
    tag.apply(changes);
    tag.to_bytes(0)
}

/// Writes an MP3 file: its ID3v2 tag edited or created, its ID3v1 tag edited when present.
pub(super) fn write_mp3(
    source: &mut Source,
    out: &mut impl Write,
    changes: &Changes,
) -> Result<(), TagWriteError> {
    let found = if source.len >= 10 {
        tag_len(&source.array::<10>(0)?)?
    } else {
        None
    };
    let (tag, audio_start) = match found {
        Some(len) => (Some(Tag::parse(&source.read(0, len as u64)?)?), len as u64),
        None => (None, 0),
    };
    let v1 = if source.len.saturating_sub(audio_start) >= 128 {
        let bytes = source.read(source.len - 128, 128)?;
        bytes.starts_with(b"TAG").then_some(bytes)
    } else {
        None
    };
    let audio_end = source.len - if v1.is_some() { 128 } else { 0 };

    let existed = tag.is_some();
    let mut tag = tag.unwrap_or_else(Tag::new);
    tag.apply(changes);
    if existed || tag.has_frames() {
        put(out, &tag.to_bytes(GROWTH_PADDING)?)?;
    }
    source.copy(audio_start, audio_end, out)?;
    if let Some(mut v1) = v1 {
        edit_v1(&mut v1, changes);
        put(out, &v1)?;
    }
    Ok(())
}

/// Edits the text fields of a 128-byte ID3v1 tag. Its fields are ISO-8859-1 of fixed length:
/// text is cut to fit and other characters become `?`.
fn edit_v1(tag: &mut [u8], changes: &Changes) {
    // ID3v1.1 keeps the track number in the last byte of the comment, after a zero.
    let comment_len = if tag.get(125) == Some(&0) && tag.get(126) != Some(&0) {
        28
    } else {
        30
    };
    let fields = [
        (Field::Title, 3, 30),
        (Field::Artist, 33, 30),
        (Field::Album, 63, 30),
        (Field::Comment, 97, comment_len),
    ];
    for (field, start, len) in fields {
        let change: Change<'_> = changes.field(field);
        let Some(value) = change else {
            continue;
        };
        let Some(slot) = tag.get_mut(start..start + len) else {
            continue;
        };
        slot.fill(0);
        let text = value.unwrap_or_default().chars().map(|character| {
            if character.is_control() {
                b' '
            } else {
                u8::try_from(character).unwrap_or(b'?')
            }
        });
        for (byte, value) in slot.iter_mut().zip(text) {
            *byte = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, described, encode, resynchronised};

    #[test]
    fn unsynchronisation_is_undone() {
        assert_eq!(
            resynchronised(&[0xFF, 0x00, 0xE0, 0xFF, 0x00, 0x00, 0x01]),
            [0xFF, 0xE0, 0xFF, 0x00, 0x01]
        );
    }

    #[test]
    fn descriptions_are_read_in_each_encoding() {
        assert_eq!(
            described(b"\x00engDesc\x00text", 4).as_deref(),
            Some("Desc")
        );
        assert_eq!(described(b"\x03eng\x00text", 4).as_deref(), Some(""));
        let utf16 = [&[1u8][..], b"eng", &encode(1, "Dé"), &[0, 0], b"x\0"].concat();
        assert_eq!(described(&utf16, 4).as_deref(), Some("Dé"));
        assert_eq!(described(b"\x01eng", 4).as_deref(), None);
        assert_eq!(decode(2, &[0, b'A']), "A");
    }
}
