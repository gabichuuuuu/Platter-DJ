//! M4A: the item list `moov/udta/meta/ilst` is edited, and the boxes that lead to it are
//! created when missing. Items the edit does not name stay byte for byte.
//!
//! A `moov` that changes size is balanced by the `free` box right after it when there is
//! one with room; otherwise everything after `moov` moves, and the chunk offsets (`stco`,
//! `co64`) that point there move with it. A fragmented file, whose fragments hold offsets of
//! their own, is refused in that case.

use std::io::{Read, Write};

use super::{
    Changes, Field, Source, TagWriteError, be_u32, corrupt, put, slice, to_usize, unsupported,
};
use crate::PictureFormat;

/// Boxes read inside one parent, or at the top of the file, before the file is refused.
const MAX_BOXES: usize = 4096;
/// The handler of the metadata: `mdir`, from `appl`, with an empty name.
const HANDLER: [u8; 25] = [
    0, 0, 0, 0, 0, 0, 0, 0, b'm', b'd', b'i', b'r', b'a', b'p', b'p', b'l', 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];
/// Data type of the value of an item: UTF-8 text, and a big-endian signed integer.
const UTF8: u8 = 1;
const JPEG: u8 = 13;
const PNG: u8 = 14;
const SIGNED: u8 = 21;
const ITUNES: &[u8] = b"com.apple.iTunes";
const KEY_NAME: &[u8] = b"initialkey";
const PLATTER_MEAN: &[u8] = b"org.platter-dj";
const PLATTER_NAME: &[u8] = b"PLATTER";

/// A box inside a byte slice: kind, start, start of the body and end.
#[derive(Clone, Copy, Debug)]
struct BoxAt {
    kind: [u8; 4],
    start: usize,
    body: usize,
    end: usize,
}

/// The box at `at`, which must end by `limit`.
fn header(bytes: &[u8], at: usize, limit: usize) -> Result<BoxAt, TagWriteError> {
    let size = be_u32(bytes, at)?;
    let kind: [u8; 4] = slice(bytes, at + 4, 4)?
        .try_into()
        .map_err(|_| corrupt("box"))?;
    let (size, header_len) = match size {
        0 => (limit.saturating_sub(at), 8),
        1 => {
            let large: [u8; 8] = slice(bytes, at + 8, 8)?
                .try_into()
                .map_err(|_| corrupt("box"))?;
            (to_usize(u64::from_be_bytes(large))?, 16)
        }
        size => (to_usize(size)?, 8),
    };
    let end = at
        .checked_add(size)
        .filter(|end| size >= header_len && *end <= limit)
        .ok_or_else(|| corrupt("an MP4 box runs past what holds it"))?;
    Ok(BoxAt {
        kind,
        start: at,
        body: at + header_len,
        end,
    })
}

/// The boxes from `from` to `to`. Fewer than eight bytes left at the end are not a box: a
/// QuickTime user data list may end with a zero word.
fn children(bytes: &[u8], from: usize, to: usize) -> Result<Vec<BoxAt>, TagWriteError> {
    let mut boxes = Vec::new();
    let mut at = from;
    while to.saturating_sub(at) >= 8 {
        if boxes.len() >= MAX_BOXES {
            return Err(unsupported("too many MP4 boxes"));
        }
        let found = header(bytes, at, to)?;
        at = found.end;
        boxes.push(found);
    }
    Ok(boxes)
}

fn make_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    match u32::try_from(body.len() + 8) {
        Ok(size) => [&size.to_be_bytes()[..], kind, body].concat(),
        Err(_) => [
            &1u32.to_be_bytes()[..],
            kind,
            &((body.len() + 16) as u64).to_be_bytes(),
            body,
        ]
        .concat(),
    }
}

type Edit<'a> = dyn FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>, TagWriteError> + 'a;

/// The box `whole` rebuilt with its first child of kind `kind` passed through `edit`, or with
/// what `edit(None)` gives added at the end when it has none. `prefix` bytes of the body come
/// before the children (the version and flags of a full box).
fn with_child(
    whole: &[u8],
    prefix: usize,
    kind: &[u8; 4],
    edit: Box<Edit<'_>>,
) -> Result<Vec<u8>, TagWriteError> {
    let own = header(whole, 0, whole.len())?;
    let first_child = own.body + prefix;
    let mut body = slice(whole, own.body, prefix)?.to_vec();
    let mut edit = Some(edit);
    let mut end = first_child;
    for child in children(whole, first_child, own.end)? {
        let bytes = slice(whole, child.start, child.end - child.start)?;
        match edit.take_if(|_| child.kind == *kind) {
            Some(edit) => body.extend_from_slice(&edit(Some(bytes))?.unwrap_or_default()),
            None => body.extend_from_slice(bytes),
        }
        end = child.end;
    }
    let tail = whole.get(end..own.end).unwrap_or_default();
    if let Some(edit) = edit
        && let Some(child) = edit(None)?
    {
        body.extend_from_slice(&child);
    }
    body.extend_from_slice(tail);
    Ok(make_box(&own.kind, &body))
}

fn edit_moov(moov: &[u8], changes: &Changes) -> Result<Vec<u8>, TagWriteError> {
    with_child(moov, 0, b"udta", Box::new(|udta| edit_udta(udta, changes)))
}

fn edit_udta(udta: Option<&[u8]>, changes: &Changes) -> Result<Option<Vec<u8>>, TagWriteError> {
    match udta {
        Some(udta) => {
            with_child(udta, 0, b"meta", Box::new(|meta| edit_meta(meta, changes))).map(Some)
        }
        None => Ok(edit_meta(None, changes)?.map(|meta| make_box(b"udta", &meta))),
    }
}

fn edit_meta(meta: Option<&[u8]>, changes: &Changes) -> Result<Option<Vec<u8>>, TagWriteError> {
    match meta {
        Some(meta) => {
            // ISO `meta` is a full box; the QuickTime one starts with its children.
            let own = header(meta, 0, meta.len())?;
            let prefix = if meta.get(own.body + 4..own.body + 8) == Some(b"hdlr".as_slice()) {
                0
            } else {
                4
            };
            with_child(
                meta,
                prefix,
                b"ilst",
                Box::new(|ilst| edit_ilst(ilst, changes)),
            )
            .map(Some)
        }
        None => Ok(edit_ilst(None, changes)?.map(|ilst| {
            let handler = make_box(b"hdlr", &HANDLER);
            make_box(b"meta", &[&[0, 0, 0, 0][..], &handler, &ilst].concat())
        })),
    }
}

fn edit_ilst(ilst: Option<&[u8]>, changes: &Changes) -> Result<Option<Vec<u8>>, TagWriteError> {
    let mut items = match ilst {
        Some(ilst) => {
            let own = header(ilst, 0, ilst.len())?;
            children(ilst, own.body, own.end)?
                .into_iter()
                .map(|item| slice(ilst, item.start, item.end - item.start).map(<[u8]>::to_vec))
                .collect::<Result<Vec<_>, _>>()?
        }
        None => Vec::new(),
    };
    apply(&mut items, changes);
    if ilst.is_none() && items.is_empty() {
        return Ok(None);
    }
    Ok(Some(make_box(b"ilst", &items.concat())))
}

fn kind(item: &[u8]) -> Option<&[u8]> {
    item.get(4..8)
}

/// The mean and the name of a freeform (`----`) item.
fn freeform(item: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if kind(item) != Some(b"----".as_slice()) {
        return None;
    }
    let own = header(item, 0, item.len()).ok()?;
    let mut mean = None;
    let mut name = None;
    for child in children(item, own.body, own.end).ok()? {
        // Both are full boxes: four bytes of version and flags, then the text.
        let text = item.get(child.body + 4..child.end)?.to_vec();
        match &child.kind {
            b"mean" => mean = Some(text),
            b"name" => name = Some(text),
            _ => {}
        }
    }
    Some((mean?, name?))
}

fn is_freeform(item: &[u8], mean: &[u8], name: &[u8]) -> bool {
    freeform(item).is_some_and(|(found_mean, found_name)| {
        found_mean == mean && found_name.eq_ignore_ascii_case(name)
    })
}

fn data(kind: u8, value: &[u8]) -> Vec<u8> {
    make_box(b"data", &[&[0, 0, 0, kind][..], &[0; 4], value].concat())
}

fn text_item(fourcc: &[u8; 4], value: &str) -> Vec<u8> {
    make_box(fourcc, &data(UTF8, value.as_bytes()))
}

fn freeform_item(mean: &[u8], name: &[u8], value: &str) -> Vec<u8> {
    let body = [
        make_box(b"mean", &[&[0; 4][..], mean].concat()),
        make_box(b"name", &[&[0; 4][..], name].concat()),
        data(UTF8, value.as_bytes()),
    ]
    .concat();
    make_box(b"----", &body)
}

fn apply(items: &mut Vec<Vec<u8>>, changes: &Changes) {
    let fourccs: [(Field, &[u8; 4]); 4] = [
        (Field::Title, b"\xA9nam"),
        (Field::Artist, b"\xA9ART"),
        (Field::Album, b"\xA9alb"),
        (Field::Comment, b"\xA9cmt"),
    ];
    for (field, fourcc) in fourccs {
        if let Some(value) = changes.field(field) {
            let new = value.map(|value| text_item(fourcc, value));
            set(items, |item| kind(item) == Some(fourcc.as_slice()), new);
        }
    }
    // The genre is text in `©gen`; `gnre` holds an ID3v1 genre number, which would hide it.
    if let Some(value) = changes.field(Field::Genre) {
        let new = value.map(|value| text_item(b"\xA9gen", value));
        set(
            items,
            |item| matches!(kind(item), Some(b"\xA9gen" | b"gnre")),
            new,
        );
    }
    if let Some(value) = changes.field(Field::Key) {
        let new = value.map(|value| freeform_item(ITUNES, KEY_NAME, value));
        set(items, |item| is_freeform(item, ITUNES, KEY_NAME), new);
    }
    if let Some(value) = changes.field(Field::Platter) {
        let new = value.map(|value| freeform_item(PLATTER_MEAN, PLATTER_NAME, value));
        set(
            items,
            |item| is_freeform(item, PLATTER_MEAN, PLATTER_NAME),
            new,
        );
    }
    // Every picture of `covr` is a front cover to the reader, so an import replaces them all.
    if let Some(cover) = changes.cover() {
        let new = cover.map(|picture| {
            let kind = match picture.format() {
                PictureFormat::Jpeg => JPEG,
                PictureFormat::Png => PNG,
            };
            make_box(b"covr", &data(kind, picture.data()))
        });
        set(items, |item| kind(item) == Some(b"covr".as_slice()), new);
    }
    // The tempo item holds a 16-bit integer; a freeform tempo with decimals would show the
    // old value to software that prefers it, so it goes too.
    if let Some(bpm) = changes.bpm {
        // The range check keeps the rounded tempo within 20..=999.
        let new =
            bpm.map(|bpm| make_box(b"tmpo", &data(SIGNED, &(bpm.round() as i16).to_be_bytes())));
        set(
            items,
            |item| kind(item) == Some(b"tmpo".as_slice()) || is_freeform(item, ITUNES, b"BPM"),
            new,
        );
    }
}

/// Removes the items `matches` picks and puts `new`, when given, where the first was.
fn set(items: &mut Vec<Vec<u8>>, matches: impl Fn(&[u8]) -> bool, new: Option<Vec<u8>>) {
    let first = items.iter().position(|item| matches(item));
    items.retain(|item| !matches(item));
    if let Some(item) = new {
        match first {
            Some(at) if at <= items.len() => items.insert(at, item),
            _ => items.push(item),
        }
    }
}

/// The chunk offset tables of every track: start of the body and end, and whether the
/// entries have 64 bits.
fn offset_tables(moov: &[u8]) -> Result<Vec<(usize, usize, bool)>, TagWriteError> {
    let own = header(moov, 0, moov.len())?;
    let mut tables = Vec::new();
    let inside = |parent: BoxAt, kind: &[u8; 4]| -> Result<Vec<BoxAt>, TagWriteError> {
        Ok(children(moov, parent.body, parent.end)?
            .into_iter()
            .filter(|child| child.kind == *kind)
            .collect())
    };
    for trak in inside(own, b"trak")? {
        for mdia in inside(trak, b"mdia")? {
            for minf in inside(mdia, b"minf")? {
                for stbl in inside(minf, b"stbl")? {
                    for table in children(moov, stbl.body, stbl.end)? {
                        match &table.kind {
                            b"stco" => tables.push((table.body, table.end, false)),
                            b"co64" => tables.push((table.body, table.end, true)),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(tables)
}

/// Moves by `shift` every chunk offset at or past `from`.
fn shift_offsets(moov: &mut [u8], from: u64, shift: i64) -> Result<(), TagWriteError> {
    for (body, end, wide) in offset_tables(moov)? {
        let count = to_usize(be_u32(moov, body + 4)?)?;
        let width = if wide { 8 } else { 4 };
        if count > end.saturating_sub(body + 8) / width {
            return Err(corrupt(
                "a chunk offset table declares more entries than it holds",
            ));
        }
        for index in 0..count {
            let at = body + 8 + index * width;
            let field = moov
                .get_mut(at..at + width)
                .ok_or_else(|| corrupt("chunk offset"))?;
            let offset = field
                .iter()
                .fold(0u64, |value, byte| value << 8 | u64::from(*byte));
            if offset < from {
                continue;
            }
            let moved = i128::from(offset) + i128::from(shift);
            if wide {
                let moved = u64::try_from(moved).map_err(|_| corrupt("chunk offset"))?;
                field.copy_from_slice(&moved.to_be_bytes());
            } else {
                let moved = u32::try_from(moved)
                    .map_err(|_| unsupported("a chunk offset no longer fits 32 bits"))?;
                field.copy_from_slice(&moved.to_be_bytes());
            }
        }
    }
    Ok(())
}

/// A box at the top of the file: kind, start, header length and end.
struct Top {
    kind: [u8; 4],
    start: u64,
    header: u64,
    end: u64,
}

fn top_boxes(source: &mut Source) -> Result<Vec<Top>, TagWriteError> {
    let mut boxes = Vec::new();
    let mut at = 0u64;
    while source.len.saturating_sub(at) >= 8 {
        if boxes.len() >= MAX_BOXES {
            return Err(unsupported("too many MP4 boxes"));
        }
        let head = source.array::<8>(at)?;
        let size = be_u32(&head, 0)?;
        let kind: [u8; 4] = slice(&head, 4, 4)?.try_into().map_err(|_| corrupt("box"))?;
        let (size, header_len) = match size {
            0 => (source.len - at, 8),
            1 => (u64::from_be_bytes(source.array::<8>(at + 8)?), 16),
            size => (u64::from(size), 8),
        };
        let end = at
            .checked_add(size)
            .filter(|end| size >= header_len && *end <= source.len)
            .ok_or_else(|| corrupt("an MP4 box runs past the end of the file"))?;
        boxes.push(Top {
            kind,
            start: at,
            header: header_len,
            end,
        });
        at = end;
    }
    Ok(boxes)
}

pub(super) fn write(
    source: &mut Source,
    out: &mut impl Write,
    changes: &Changes,
) -> Result<(), TagWriteError> {
    let top = top_boxes(source)?;
    let index = top
        .iter()
        .position(|found| found.kind == *b"moov")
        .ok_or_else(|| corrupt("the MP4 file has no moov box"))?;
    let moov = top.get(index).ok_or_else(|| corrupt("moov"))?;
    let old = source.read(moov.start, moov.end - moov.start)?;
    let mut new = edit_moov(&old, changes)?;
    let shift = i64::try_from(new.len()).map_err(|_| corrupt("size"))?
        - i64::try_from(old.len()).map_err(|_| corrupt("size"))?;

    let next = top.get(index + 1);
    let free_room = next
        .filter(|next| matches!(&next.kind, b"free" | b"skip") && next.header == 8)
        .map(|next| i64::try_from(next.end - next.start).unwrap_or(0));
    let (free, rest) = match free_room {
        Some(room) if room - shift == 0 || room - shift >= 8 => {
            (room - shift, next.map_or(moov.end, |next| next.end))
        }
        _ if shift <= -8 => (-shift, moov.end),
        _ => {
            if shift != 0 {
                let fragmented = top.iter().any(|found| found.kind == *b"moof")
                    || header(&new, 0, new.len())
                        .and_then(|own| children(&new, own.body, own.end))?
                        .iter()
                        .any(|child| child.kind == *b"mvex");
                if fragmented {
                    return Err(unsupported("a fragmented MP4 file whose moov changes size"));
                }
                shift_offsets(&mut new, moov.end, shift)?;
            }
            (0, moov.end)
        }
    };

    source.copy(0, moov.start, out)?;
    put(out, &new)?;
    if free > 0 {
        write_free(out, u64::try_from(free).map_err(|_| corrupt("free box"))?)?;
    }
    source.copy(rest, source.len, out)
}

/// A `free` box of `len` bytes in all. Its zeros are streamed, never allocated: the length
/// comes from the file and may be large.
fn write_free(out: &mut impl Write, len: u64) -> Result<(), TagWriteError> {
    let header = match u32::try_from(len) {
        Ok(size) => [&size.to_be_bytes()[..], b"free"].concat(),
        Err(_) => [&1u32.to_be_bytes()[..], b"free", &len.to_be_bytes()].concat(),
    };
    let zeros = len
        .checked_sub(header.len() as u64)
        .ok_or_else(|| corrupt("free box"))?;
    put(out, &header)?;
    let written =
        std::io::copy(&mut std::io::repeat(0).take(zeros), out).map_err(TagWriteError::Io)?;
    if written != zeros {
        return Err(corrupt("free box"));
    }
    Ok(())
}
