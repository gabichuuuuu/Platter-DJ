//! WAV and AIFF: chunks in a RIFF or FORM container, little-endian in WAV and big-endian in
//! AIFF, each padded to an even length.
//!
//! WAV keeps its text in the `LIST` chunk of form `INFO`, AIFF in the text chunks `NAME`,
//! `AUTH` and `ANNO`. Neither has a place for the key, the tempo or Platter's data, nor AIFF
//! for the album and the genre: those go into an ID3v2 tag in an `id3 ` (WAV) or `ID3 ` (AIFF)
//! chunk, edited when the file has one and created when the edit sets one of them. A created
//! tag also carries the text the native chunks hold, so a reader that prefers it finds the
//! same title. New chunks go at the end, so the audio does not move.

use std::io::Write;

use super::id3::{self, Tag};
use super::{Changes, Field, Source, TagWriteError, corrupt, put, slice, to_usize, unsupported};

/// Chunks read from one file, or from one `INFO` list, before it is refused.
const MAX_CHUNKS: usize = 4096;

/// Fields of the WAV `INFO` list (Multimedia Programming Interface and Data Specifications
/// 1.0): title, artist, product (the album), genre and comment.
const INFO_FIELDS: [(Field, &[u8; 4]); 5] = [
    (Field::Title, b"INAM"),
    (Field::Artist, b"IART"),
    (Field::Album, b"IPRD"),
    (Field::Genre, b"IGNR"),
    (Field::Comment, b"ICMT"),
];
/// Text chunks of AIFF 1.3.
const TEXT_FIELDS: [(Field, &[u8; 4]); 3] = [
    (Field::Title, b"NAME"),
    (Field::Artist, b"AUTH"),
    (Field::Comment, b"ANNO"),
];

/// Part of the new file: bytes of the original, from `start` to `end` with a pad byte added
/// when `pad`, or new bytes.
enum Piece {
    Copy { start: u64, end: u64, pad: bool },
    New(Vec<u8>),
}

impl Piece {
    fn len(&self) -> u64 {
        match self {
            Self::Copy { start, end, pad } => end - start + u64::from(*pad),
            Self::New(bytes) => bytes.len() as u64,
        }
    }
}

/// One chunk of the file, with the piece that writes it.
struct Chunk {
    id: [u8; 4],
    /// Start of the body in the original file; `None` for a new chunk.
    body: Option<u64>,
    size: u64,
    piece: Piece,
}

struct Layout {
    big_endian: bool,
    chunks: Vec<Chunk>,
    /// Bytes after the container, copied as they are (an ID3v1 tag, for one).
    after: u64,
}

fn size_bytes(size: u32, big_endian: bool) -> [u8; 4] {
    if big_endian {
        size.to_be_bytes()
    } else {
        size.to_le_bytes()
    }
}

fn read_size(bytes: &[u8], big_endian: bool) -> Result<u64, TagWriteError> {
    let field: [u8; 4] = slice(bytes, 4, 4)?
        .try_into()
        .map_err(|_| corrupt("chunk"))?;
    Ok(u64::from(if big_endian {
        u32::from_be_bytes(field)
    } else {
        u32::from_le_bytes(field)
    }))
}

/// A chunk with `body`, padded to an even length.
fn chunk_bytes(id: &[u8; 4], body: &[u8], big_endian: bool) -> Result<Vec<u8>, TagWriteError> {
    let size = super::to_u32(body.len())?;
    let mut out = [id.as_slice(), &size_bytes(size, big_endian), body].concat();
    if body.len() % 2 == 1 {
        out.push(0);
    }
    Ok(out)
}

fn new_chunk(id: &[u8; 4], body: &[u8], big_endian: bool) -> Result<Chunk, TagWriteError> {
    Ok(Chunk {
        id: *id,
        body: None,
        size: body.len() as u64,
        piece: Piece::New(chunk_bytes(id, body, big_endian)?),
    })
}

/// The chunks of the container whose header is the first twelve bytes of the file.
fn layout(source: &mut Source, big_endian: bool) -> Result<Layout, TagWriteError> {
    let head = source.read(0, 12)?;
    let declared_end = read_size(&head, big_endian)?.saturating_add(8);
    // A declared size too small to hold the form type still leaves the header in place.
    let end = declared_end.min(source.len).max(12);
    let mut chunks = Vec::new();
    let mut at = 12u64;
    while end.saturating_sub(at) >= 8 {
        if chunks.len() >= MAX_CHUNKS {
            return Err(unsupported("too many chunks"));
        }
        let header = source.read(at, 8)?;
        let id: [u8; 4] = slice(&header, 0, 4)?
            .try_into()
            .map_err(|_| corrupt("chunk"))?;
        let size = read_size(&header, big_endian)?;
        let body_end = at + 8 + size;
        if body_end > end {
            return Err(corrupt("a chunk runs past the end of its container"));
        }
        // The pad byte of the last chunk is sometimes missing; it is added back.
        let padded = body_end + (size % 2);
        let chunk_end = padded.min(end);
        chunks.push(Chunk {
            id,
            body: Some(at + 8),
            size,
            piece: Piece::Copy {
                start: at,
                end: chunk_end,
                pad: chunk_end < padded,
            },
        });
        at = chunk_end;
    }
    // Fewer than eight bytes left inside the container after the last chunk cannot be a
    // chunk. They are dropped: kept, they would put every chunk written after them off a
    // chunk boundary.
    Ok(Layout {
        big_endian,
        chunks,
        after: end,
    })
}

impl Layout {
    fn body(&self, source: &mut Source, chunk: &Chunk) -> Result<Vec<u8>, TagWriteError> {
        let start = chunk.body.ok_or_else(|| corrupt("chunk"))?;
        source.read(start, chunk.size)
    }

    /// Removes the chunks with `id` and puts `body`, when given, where the first was.
    fn set_chunk(&mut self, id: &[u8; 4], body: Option<&[u8]>) -> Result<(), TagWriteError> {
        let first = self.chunks.iter().position(|chunk| chunk.id == *id);
        self.chunks.retain(|chunk| chunk.id != *id);
        if let Some(body) = body {
            let chunk = new_chunk(id, body, self.big_endian)?;
            match first {
                Some(at) if at <= self.chunks.len() => self.chunks.insert(at, chunk),
                _ => self.chunks.push(chunk),
            }
        }
        Ok(())
    }

    /// The ID3v2 chunk: edited when the file has one, created when the edit sets a field
    /// only it can hold. `seed` gives the text the native chunks held before the edit.
    fn edit_id3(
        &mut self,
        source: &mut Source,
        changes: &Changes,
        new_id: &[u8; 4],
        needs_id3: bool,
        seed: &[(Field, String)],
    ) -> Result<(), TagWriteError> {
        let found = self
            .chunks
            .iter()
            .position(|chunk| matches!(&chunk.id, b"id3 " | b"ID3 "));
        if let Some(index) = found {
            let chunk = self.chunks.get(index).ok_or_else(|| corrupt("chunk"))?;
            let body = self.body(source, chunk)?;
            let id = chunk.id;
            let new = id3::edit_chunk(&body, changes)?;
            let replaced = new_chunk(&id, &new, self.big_endian)?;
            if let Some(slot) = self.chunks.get_mut(index) {
                *slot = replaced;
            }
        } else if needs_id3 {
            let mut tag = Tag::new();
            for (field, value) in seed {
                tag.set(*field, Some(value));
            }
            tag.apply(changes);
            let chunk = new_chunk(new_id, &tag.to_bytes(0)?, self.big_endian)?;
            self.chunks.push(chunk);
        }
        Ok(())
    }

    fn write(
        &self,
        source: &mut Source,
        out: &mut impl Write,
        magic: &[u8; 4],
        form: &[u8; 4],
    ) -> Result<(), TagWriteError> {
        let content: u64 = self.chunks.iter().map(|chunk| chunk.piece.len()).sum();
        let size = u32::try_from(content + 4)
            .map_err(|_| unsupported("the file would pass the 4 GiB its container allows"))?;
        put(out, magic)?;
        put(out, &size_bytes(size, self.big_endian))?;
        put(out, form)?;
        for chunk in &self.chunks {
            match &chunk.piece {
                Piece::Copy { start, end, pad } => {
                    source.copy(*start, *end, out)?;
                    if *pad {
                        put(out, &[0])?;
                    }
                }
                Piece::New(bytes) => put(out, bytes)?,
            }
        }
        source.copy(self.after, source.len, out)
    }
}

/// Text of an `INFO` field or AIFF text chunk: up to the first zero, read as UTF-8.
fn text(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or_default()).into_owned()
}

/// A field of an `INFO` list: its identifier and its body.
type InfoField = ([u8; 4], Vec<u8>);

/// The fields of an `INFO` list body, after its form type.
fn info_fields(bytes: &[u8]) -> Result<Vec<InfoField>, TagWriteError> {
    let mut fields = Vec::new();
    let mut at = 0usize;
    while bytes.len().saturating_sub(at) >= 8 {
        if fields.len() >= MAX_CHUNKS {
            return Err(unsupported("too many INFO fields"));
        }
        let id: [u8; 4] = slice(bytes, at, 4)?
            .try_into()
            .map_err(|_| corrupt("INFO"))?;
        let size = to_usize(super::le_u32(bytes, at + 4)?)?;
        let body =
            slice(bytes, at + 8, size).map_err(|_| corrupt("an INFO field runs past its list"))?;
        fields.push((id, body.to_vec()));
        at += 8 + size + size % 2;
    }
    Ok(fields)
}

/// Writes a WAV file.
pub(super) fn write_wav(
    source: &mut Source,
    out: &mut impl Write,
    changes: &Changes,
) -> Result<(), TagWriteError> {
    let head = source.read(0, 12)?;
    if head.get(..4) == Some(b"RF64".as_slice()) {
        return Err(unsupported("RF64 files"));
    }
    if head.get(..4) != Some(b"RIFF".as_slice()) || head.get(8..12) != Some(b"WAVE".as_slice()) {
        return Err(corrupt("the RIFF WAVE header is missing"));
    }
    let mut layout = layout(source, false)?;
    let mut seed: Vec<(Field, String)> = Vec::new();
    let mut first_list = true;
    for index in 0..layout.chunks.len() {
        let Some(chunk) = layout.chunks.get(index) else {
            continue;
        };
        if chunk.id != *b"LIST" {
            continue;
        }
        let body = layout.body(source, chunk)?;
        if body.get(..4) != Some(b"INFO".as_slice()) {
            continue;
        }
        let mut fields = info_fields(body.get(4..).unwrap_or_default())?;
        for (field, id) in INFO_FIELDS {
            if first_list && let Some((_, value)) = fields.iter().find(|(found, _)| found == id) {
                seed.push((field, text(value)));
            }
            let Some(change) = changes.field(field) else {
                continue;
            };
            // The value goes into the first list; a later list only loses the old one.
            let value = change.filter(|_| first_list);
            let first = fields.iter().position(|(found, _)| found == id);
            fields.retain(|(found, _)| found != id);
            if let Some(value) = value {
                let field = (*id, [value.as_bytes(), &[0]].concat());
                match first {
                    Some(at) if at <= fields.len() => fields.insert(at, field),
                    _ => fields.push(field),
                }
            }
        }
        let mut list = b"INFO".to_vec();
        for (id, body) in &fields {
            list.extend_from_slice(&chunk_bytes(id, body, false)?);
        }
        let replaced = new_chunk(b"LIST", &list, false)?;
        if let Some(slot) = layout.chunks.get_mut(index) {
            *slot = replaced;
        }
        first_list = false;
    }
    if first_list && INFO_FIELDS.iter().any(|(field, _)| changes.sets(*field)) {
        let mut list = b"INFO".to_vec();
        for (field, id) in INFO_FIELDS {
            if let Some(Some(value)) = changes.field(field) {
                list.extend_from_slice(&chunk_bytes(
                    id,
                    &[value.as_bytes(), &[0]].concat(),
                    false,
                )?);
            }
        }
        layout.chunks.push(new_chunk(b"LIST", &list, false)?);
    }
    let needs_id3 = changes.sets(Field::Key)
        || changes.sets(Field::Platter)
        || matches!(changes.bpm, Some(Some(_)));
    layout.edit_id3(source, changes, b"id3 ", needs_id3, &seed)?;
    layout.write(source, out, b"RIFF", b"WAVE")
}

/// Writes an AIFF or AIFF-C file.
pub(super) fn write_aiff(
    source: &mut Source,
    out: &mut impl Write,
    changes: &Changes,
) -> Result<(), TagWriteError> {
    let head = source.read(0, 12)?;
    let form: [u8; 4] = match head.get(..4).zip(head.get(8..12)) {
        Some((b"FORM", form @ (b"AIFF" | b"AIFC"))) => {
            form.try_into().map_err(|_| corrupt("AIFF header"))?
        }
        _ => return Err(corrupt("the AIFF header is missing")),
    };
    let mut layout = layout(source, true)?;
    let mut seed: Vec<(Field, String)> = Vec::new();
    for (field, id) in TEXT_FIELDS {
        if let Some(chunk) = layout.chunks.iter().find(|chunk| chunk.id == *id) {
            seed.push((field, text(&layout.body(source, chunk)?)));
        }
        if let Some(change) = changes.field(field) {
            layout.set_chunk(id, change.map(str::as_bytes))?;
        }
    }
    let needs_id3 = [Field::Album, Field::Genre, Field::Key, Field::Platter]
        .into_iter()
        .any(|field| changes.sets(field))
        || matches!(changes.bpm, Some(Some(_)));
    layout.edit_id3(source, changes, b"ID3 ", needs_id3, &seed)?;
    layout.write(source, out, b"FORM", &form)
}
