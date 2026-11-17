//! FLAC: the `VORBIS_COMMENT` metadata block is edited, or created when the edit sets a
//! field; every other block stays as it is. The padding blocks absorb the change when they
//! can, so the audio frames stay where they were. An ID3v2 tag in front of the stream is kept
//! byte for byte, unless the edit changes the cover: its pictures go as the edit says, so no
//! old picture there wins over the new one.
//!
//! A cover is a `PICTURE` block (RFC 9639, section 8.8).

use std::io::Write;

use super::vorbis::Comments;
use super::{Changes, GROWTH_PADDING, Source, TagWriteError, corrupt, id3, put, unsupported};
use crate::CoverPicture;

const STREAMINFO: u8 = 0;
const PADDING: u8 = 1;
const VORBIS_COMMENT: u8 = 4;
const PICTURE: u8 = 6;
/// Metadata blocks read before the file is refused.
const MAX_BLOCKS: usize = 4096;
/// Largest body of one metadata block: its length has 24 bits.
const MAX_BLOCK_LEN: usize = 0xFF_FFFF;

pub(super) fn write(
    source: &mut Source,
    out: &mut impl Write,
    changes: &Changes,
) -> Result<(), TagWriteError> {
    let start = if source.len >= 10 {
        id3::tag_len(&source.array::<10>(0)?)?.unwrap_or(0) as u64
    } else {
        0
    };
    if source.array::<4>(start)? != *b"fLaC" {
        return Err(corrupt("the FLAC signature is missing"));
    }
    let first_block = start + 4;
    let mut blocks: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut at = first_block;
    loop {
        if blocks.len() >= MAX_BLOCKS {
            return Err(unsupported("too many FLAC metadata blocks"));
        }
        let [flags, a, b, c] = source.array::<4>(at)?;
        let kind = flags & 0x7F;
        if kind == 127 {
            return Err(corrupt("a FLAC metadata block has the forbidden type 127"));
        }
        let len = u64::from(u32::from_be_bytes([0, a, b, c]));
        // The bound on metadata read into memory applies to the blocks together.
        if at - first_block + len > super::MAX_METADATA_BYTES {
            return Err(unsupported("its metadata is larger than 64 MiB"));
        }
        blocks.push((kind, source.read(at + 4, len)?));
        at += 4 + len;
        if flags & 0x80 != 0 {
            break;
        }
    }
    if blocks.first().map(|(kind, _)| *kind) != Some(STREAMINFO) {
        return Err(corrupt("the first FLAC metadata block is not STREAMINFO"));
    }
    let old_len = to_len(at - first_block)?;

    let mut kept: Vec<(u8, Vec<u8>)> = Vec::with_capacity(blocks.len() + 2);
    let mut comment_seen = false;
    let mut first_picture = None;
    let cover = changes.cover();
    for (kind, body) in blocks {
        match kind {
            PADDING => {}
            VORBIS_COMMENT if comment_seen => {
                return Err(corrupt("more than one FLAC comment block"));
            }
            VORBIS_COMMENT => {
                comment_seen = true;
                let (mut comments, _) = Comments::parse(&body)?;
                comments.apply(changes, false);
                kept.push((VORBIS_COMMENT, comments.to_bytes()?));
            }
            PICTURE => {
                first_picture = first_picture.or(Some(kept.len()));
                let gone = match cover {
                    Some(Some(_)) => body.get(..4) == Some(&[0, 0, 0, id3::FRONT_COVER][..]),
                    Some(None) => true,
                    None => false,
                };
                if !gone {
                    kept.push((kind, body));
                }
            }
            _ => kept.push((kind, body)),
        }
    }
    if !comment_seen && changes.sets_any() {
        let mut comments = Comments::new();
        comments.apply(changes, false);
        // After STREAMINFO, which is always first.
        kept.insert(1.min(kept.len()), (VORBIS_COMMENT, comments.to_bytes()?));
        first_picture = first_picture.map(|at| at + 1);
    }
    if let Some(Some(picture)) = cover {
        // Where the first picture was, or after the other blocks.
        let at = first_picture.unwrap_or(kept.len()).min(kept.len());
        kept.insert(at, (PICTURE, picture_block(picture)));
    }
    let content: usize = kept.iter().map(|(_, body)| 4 + body.len()).sum();
    // The padding keeps the old size when the rest fits with room for a block header;
    // otherwise the audio moves, and a tag that grew keeps room for later edits.
    let padding = match old_len.checked_sub(content) {
        Some(0) => None,
        Some(room) if room >= 4 => Some(room - 4),
        Some(_) => None,
        None => Some(GROWTH_PADDING),
    };
    if let Some(padding) = padding {
        kept.push((PADDING, vec![0; padding]));
    }

    match cover {
        Some(cover) if start > 0 => {
            let mut tag = id3::Tag::parse(&source.read(0, start)?)?;
            tag.set_room(to_len(start)?);
            if cover.is_some() {
                tag.remove_front_covers();
            } else {
                tag.remove_pictures();
            }
            // The tag only loses frames, so it keeps its size and the stream does not move.
            put(out, &tag.to_bytes(GROWTH_PADDING)?)?;
        }
        _ => source.copy(0, start, out)?,
    }
    put(out, b"fLaC")?;
    let last = kept.len().saturating_sub(1);
    for (index, (kind, body)) in kept.iter().enumerate() {
        if body.len() > MAX_BLOCK_LEN {
            return Err(unsupported("a FLAC metadata block would be too large"));
        }
        let len = (body.len() as u32).to_be_bytes();
        let flag = if index == last { 0x80 } else { 0 };
        put(out, &[flag | kind, len[1], len[2], len[3]])?;
        put(out, body)?;
    }
    source.copy(at, source.len, out)
}

/// The body of a `PICTURE` block for a front cover (RFC 9639, section 8.8): big-endian
/// lengths, an empty description and no colour table.
pub(super) fn picture_block(picture: &CoverPicture) -> Vec<u8> {
    let mime = picture.format().mime().as_bytes();
    let data = picture.data();
    // Both fit in 32 bits: the MIME type is a constant and the picture is bounded at 8 MiB.
    let len = |bytes: &[u8]| (bytes.len() as u32).to_be_bytes();
    [
        &u32::from(id3::FRONT_COVER).to_be_bytes()[..],
        &len(mime),
        mime,
        &[0, 0, 0, 0],
        &picture.width().to_be_bytes(),
        &picture.height().to_be_bytes(),
        &picture.depth().to_be_bytes(),
        &[0, 0, 0, 0],
        &len(data),
        data,
    ]
    .concat()
}

fn to_len(value: u64) -> Result<usize, TagWriteError> {
    usize::try_from(value).map_err(|_| corrupt("size"))
}
