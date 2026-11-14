//! Reads the edit list of the audio track of an MP4 file.
//!
//! An AAC encoder adds silent priming frames before the audio and padding frames after it.
//! The file says which part to present in the edit list of the track: where the presented
//! audio starts in the media and how long it lasts. The decoding library in use does not
//! apply it, so this module reads it and the caller trims the decoded audio.
//!
//! Box layout: ISO/IEC 14496-12, sections 8.2.2 (`mvhd`), 8.4.2 (`mdhd`), 8.4.3 (`hdlr`) and
//! 8.6.6 (`elst`); the same boxes are described in the QuickTime file format
//! (<https://developer.apple.com/documentation/quicktime-file-format/edit_list_atom>).
//!
//! The file is untrusted: every size is checked against its parent, and the number of boxes
//! and edits read is bounded.

use std::io::{Read, Seek, SeekFrom};

/// Boxes looked at inside one parent before giving up.
const MAX_BOXES: usize = 1024;
/// Edits looked at in one list. Files with gapless audio have one, sometimes after an empty one.
const MAX_EDITS: u32 = 16;

/// The part of the media of the track that is presented, in units of the media timescale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Edit {
    pub start: u64,
    /// Absent when the list does not bound the presented audio.
    pub length: Option<u64>,
    pub timescale: u32,
}

impl Edit {
    /// Start and length converted to frames at `rate`, which may differ from the timescale
    /// (an HE-AAC track declares half the rate it decodes to).
    pub fn frames(&self, rate: u32) -> (u64, Option<u64>) {
        let convert = |value: u64| {
            let frames = u128::from(value) * u128::from(rate) / u128::from(self.timescale.max(1));
            u64::try_from(frames).unwrap_or(u64::MAX)
        };
        (convert(self.start), self.length.map(convert))
    }

    pub fn seconds(&self) -> Option<f64> {
        self.length
            .map(|length| length as f64 / f64::from(self.timescale.max(1)))
    }
}

/// The edit of the first audio track, or `None` when the file has no usable edit list or is
/// not an MP4 file at all.
pub(crate) fn read_edit<R: Read + Seek>(reader: &mut R) -> Option<Edit> {
    let end = reader.seek(SeekFrom::End(0)).ok()?;
    let moov = find(reader, 0, end, b"moov")?;
    let mvhd = find(reader, moov.0, moov.1, b"mvhd")?;
    let movie_timescale = timescale(reader, mvhd.0)?;
    let traks: Vec<_> = boxes(reader, moov.0, moov.1, b"trak").collect();
    for trak in traks {
        let Some(mdia) = find(reader, trak.0, trak.1, b"mdia") else {
            continue;
        };
        let Some(hdlr) = find(reader, mdia.0, mdia.1, b"hdlr") else {
            continue;
        };
        // Version and flags, then a reserved word, then the handler type.
        if read_array::<4>(reader, hdlr.0 + 8)? != *b"soun" {
            continue;
        }
        let mdhd = find(reader, mdia.0, mdia.1, b"mdhd")?;
        let media_timescale = timescale(reader, mdhd.0)?;
        let edts = find(reader, trak.0, trak.1, b"edts")?;
        let elst = find(reader, edts.0, edts.1, b"elst")?;
        return read_elst(reader, elst.0, movie_timescale, media_timescale);
    }
    None
}

fn read_elst<R: Read + Seek>(
    reader: &mut R,
    body: u64,
    movie_timescale: u32,
    media_timescale: u32,
) -> Option<Edit> {
    if movie_timescale == 0 || media_timescale == 0 {
        return None;
    }
    let [version, ..] = read_array::<4>(reader, body)?;
    let count = u32::from_be_bytes(read_array::<4>(reader, body + 4)?);
    let entry_size: u64 = if version == 1 { 20 } else { 12 };
    let mut found = None;
    for index in 0..count.min(MAX_EDITS) {
        let at = body + 8 + u64::from(index) * entry_size;
        let (duration, media_time) = if version == 1 {
            let entry = read_array::<16>(reader, at)?;
            let (duration, media_time) = entry.split_at(8);
            (
                u64::from_be_bytes(duration.try_into().ok()?),
                i64::from_be_bytes(media_time.try_into().ok()?),
            )
        } else {
            let entry = read_array::<8>(reader, at)?;
            let (duration, media_time) = entry.split_at(4);
            (
                u64::from(u32::from_be_bytes(duration.try_into().ok()?)),
                i64::from(i32::from_be_bytes(media_time.try_into().ok()?)),
            )
        };
        // An empty edit (media time -1) delays the presentation; it holds no audio to trim.
        if media_time < 0 {
            continue;
        }
        // More than one edit with media is a cut list, not gapless trimming: leave it alone.
        if found.is_some() {
            return None;
        }
        let length = (duration > 0).then(|| {
            let units =
                u128::from(duration) * u128::from(media_timescale) / u128::from(movie_timescale);
            u64::try_from(units).unwrap_or(u64::MAX)
        });
        found = Some(Edit {
            start: media_time.unsigned_abs(),
            length,
            timescale: media_timescale,
        });
    }
    found
}

/// The timescale of an `mvhd` or `mdhd` box, which sits after two dates whose width depends
/// on the version.
fn timescale<R: Read + Seek>(reader: &mut R, body: u64) -> Option<u32> {
    let [version, ..] = read_array::<4>(reader, body)?;
    let offset = if version == 1 { 4 + 16 } else { 4 + 8 };
    Some(u32::from_be_bytes(read_array::<4>(reader, body + offset)?))
}

/// The body of the first child of the given kind, as a range of the file.
fn find<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    end: u64,
    kind: &[u8; 4],
) -> Option<(u64, u64)> {
    boxes(reader, start, end, kind).next()
}

/// The bodies of the children of the given kind between `start` and `end`.
fn boxes<'a, R: Read + Seek>(
    reader: &'a mut R,
    start: u64,
    end: u64,
    kind: &'a [u8; 4],
) -> impl Iterator<Item = (u64, u64)> + 'a {
    let mut position = start;
    let mut seen = 0;
    std::iter::from_fn(move || {
        while seen < MAX_BOXES {
            seen += 1;
            let (child_kind, body, child_end) = header(reader, position, end)?;
            position = child_end;
            if child_kind == *kind {
                return Some((body, child_end));
            }
        }
        None
    })
}

/// Kind, body start and end of the box at `position`, which must fit before `limit`.
fn header<R: Read + Seek>(
    reader: &mut R,
    position: u64,
    limit: u64,
) -> Option<([u8; 4], u64, u64)> {
    if limit.checked_sub(position)? < 8 {
        return None;
    }
    let head = read_array::<8>(reader, position)?;
    let (size, kind) = head.split_at(4);
    let kind: [u8; 4] = kind.try_into().ok()?;
    let (size, header_size) = match u32::from_be_bytes(size.try_into().ok()?) {
        0 => (limit - position, 8),
        1 => (
            u64::from_be_bytes(read_array::<8>(reader, position + 8)?),
            16,
        ),
        size => (u64::from(size), 8),
    };
    if size < header_size {
        return None;
    }
    let end = position.checked_add(size)?;
    (end <= limit).then_some((kind, position + header_size, end))
}

fn read_array<const N: usize>(reader: &mut (impl Read + Seek), at: u64) -> Option<[u8; N]> {
    reader.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let size = u32::try_from(body.len() + 8).unwrap();
        [&size.to_be_bytes()[..], kind, body].concat()
    }

    fn mvhd(timescale: u32) -> Vec<u8> {
        boxed(
            b"mvhd",
            &[&[0u8; 12][..], &timescale.to_be_bytes(), &[0; 84]].concat(),
        )
    }

    fn track(handler: &[u8; 4], timescale: u32, edits: &[(u32, i32)]) -> Vec<u8> {
        let hdlr = boxed(b"hdlr", &[&[0u8; 8][..], handler, &[0; 13]].concat());
        let mdhd = boxed(
            b"mdhd",
            &[&[0u8; 12][..], &timescale.to_be_bytes(), &[0; 8]].concat(),
        );
        let mut list = vec![0, 0, 0, 0];
        list.extend_from_slice(&u32::try_from(edits.len()).unwrap().to_be_bytes());
        for (duration, media_time) in edits {
            list.extend_from_slice(&duration.to_be_bytes());
            list.extend_from_slice(&media_time.to_be_bytes());
            list.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        }
        let edts = boxed(b"edts", &boxed(b"elst", &list));
        boxed(
            b"trak",
            &[edts, boxed(b"mdia", &[hdlr, mdhd].concat())].concat(),
        )
    }

    fn file(tracks: &[Vec<u8>]) -> Vec<u8> {
        let ftyp = boxed(b"ftyp", b"M4A \0\0\0\0");
        let moov = boxed(
            b"moov",
            &[vec![mvhd(1000)], tracks.to_vec()].concat().concat(),
        );
        [ftyp, moov, boxed(b"mdat", &[0; 32])].concat()
    }

    #[test]
    fn the_edit_of_the_audio_track_gives_priming_and_length() {
        // 30.001 s at a movie timescale of 1000, after 2112 frames of priming at 44.1 kHz.
        let bytes = file(&[
            track(b"vide", 600, &[(5, 0)]),
            track(b"soun", 44_100, &[(30_001, 2112)]),
        ]);
        let edit = read_edit(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(edit.start, 2112);
        assert_eq!(edit.length, Some(1_323_044));
        assert_eq!(edit.frames(44_100), (2112, Some(1_323_044)));
        // A track that decodes at twice its declared rate trims twice as many frames.
        assert_eq!(edit.frames(88_200), (4224, Some(2_646_088)));
    }

    #[test]
    fn a_leading_empty_edit_is_skipped() {
        let bytes = file(&[track(b"soun", 48_000, &[(10, -1), (2000, 1024)])]);
        let edit = read_edit(&mut Cursor::new(bytes)).unwrap();
        assert_eq!((edit.start, edit.length), (1024, Some(96_000)));
    }

    #[test]
    fn a_cut_list_and_a_file_without_edits_are_left_alone() {
        let cut = file(&[track(b"soun", 48_000, &[(1000, 0), (1000, 96_000)])]);
        assert_eq!(read_edit(&mut Cursor::new(cut)), None);
        assert_eq!(
            read_edit(&mut Cursor::new(b"RIFF\0\0\0\0WAVE".to_vec())),
            None
        );
    }

    #[test]
    fn sizes_that_do_not_fit_their_parent_are_rejected() {
        let mut bytes = file(&[track(b"soun", 44_100, &[(30_001, 2112)])]);
        // The size of `moov`, right after the 16 bytes of `ftyp`, now runs past the file.
        bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(read_edit(&mut Cursor::new(bytes)), None);
        for length in 0..64 {
            let bytes = file(&[track(b"soun", 44_100, &[(30_001, 2112)])]);
            let _ = read_edit(&mut Cursor::new(&bytes[..length]));
        }
    }
}
