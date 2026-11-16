//! Writes the tags of a track into its file, with Platter's own code for each container.
//! Behaviour note: `docs/behavior/tag-writing.md`.
//!
//! Each format reads only its metadata from the original file and streams a new file: the
//! new metadata where the old one was, every other byte copied from the original. The new
//! file is a copy in the same folder, which replaces the original by a rename only once it is
//! complete and reads back as the same format, so a failure never leaves a half-written
//! track.
//!
//! Specifications, one module each:
//! - `id3`: ID3v2.2 (the draft with version bytes 2.0, <https://id3.org/id3v2-00>), ID3v2.3
//!   (<https://id3.org/id3v2.3.0>),
//!   ID3v2.4 (<https://id3.org/id3v2.4.0-structure>, <https://id3.org/id3v2.4.0-frames>) and
//!   ID3v1 (<https://id3.org/ID3v1>).
//! - `vorbis` and `flac`: RFC 9639 (<https://www.rfc-editor.org/rfc/rfc9639>), sections 8.1
//!   to 8.6, and the Vorbis comment (<https://xiph.org/vorbis/doc/v-comment.html>).
//! - `ogg`: RFC 3533 (<https://www.rfc-editor.org/rfc/rfc3533>) and the Vorbis I
//!   specification (<https://xiph.org/vorbis/doc/Vorbis_I_spec.html>), section 5.2 and
//!   appendix A.2.
//! - `mp4`: ISO/IEC 14496-12 (<https://standards.iso.org/ittf/PubliclyAvailableStandards/>),
//!   for the box structure and the boxes `free`, `stco`, `co64`, `udta` and `meta`, and the
//!   metadata item list of the QuickTime file format (<https://developer.apple.com/documentation/quicktime-file-format/data_atom>,
//!   <https://developer.apple.com/documentation/quicktime-file-format/well-known_types>).
//! - `riff`: the RIFF WAVE and INFO list of the Multimedia Programming Interface and Data
//!   Specifications 1.0
//!   (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/Docs/riffmci.pdf>) and AIFF
//!   1.3 (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/AIFF/Docs/AIFF-1.3.pdf>).
//!
//! Every size read from the file is checked against what holds it, and the metadata read
//! into memory is bounded by [`MAX_METADATA_BYTES`].

mod flac;
mod id3;
mod mp4;
mod ogg;
mod riff;
mod vorbis;

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::{Container, DecodeError, TAG_BPM_RANGE, probe_file};

/// Longest text written into one field, in characters; longer text is cut. The same bound
/// as the library of Platter keeps, so a value it stores fits the file unchanged.
pub const MAX_TAG_CHARS: usize = 1000;
/// Longest data of Platter's own field, in bytes of UTF-8.
pub const MAX_PLATTER_BYTES: usize = 64 * 1024;

/// Most metadata read into memory from one file: an ID3v2 tag, the metadata blocks of FLAC,
/// the header packets of Ogg Vorbis or the `moov` box of MP4. Covers make most of it.
const MAX_METADATA_BYTES: u64 = 64 * 1024 * 1024;
/// Padding left after a tag that had to grow, so other software can edit it in place.
const GROWTH_PADDING: usize = 1024;
/// Names tried for the copy before giving up; another file already holding each one is the
/// only reason to try the next.
const COPY_ATTEMPTS: u32 = 16;

/// The changes to make to the tags of one file. A field left at `None` is not touched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagEdit {
    /// For each text field, `Some` with text sets it and `Some` with empty text, or spaces
    /// only, removes it.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    /// The musical key, as text (`8A`, `Am`): Platter does not check the notation.
    pub key: Option<String>,
    /// Tempo in beats per minute: `Some(Some(bpm))` sets it, `Some(None)` removes it.
    pub bpm: Option<Option<f64>>,
    /// Platter's own data, opaque to the writer: `Some(Some(text))` sets it as given, even
    /// empty, `Some(None)` removes it. At most [`MAX_PLATTER_BYTES`].
    pub platter: Option<Option<String>>,
}

#[derive(Debug)]
pub enum TagWriteError {
    /// The file or its folder could not be read or written.
    Io(io::Error),
    /// The file is marked read-only.
    ReadOnly,
    /// The content is none of the formats Platter plays, or a form of it the writer does not
    /// handle.
    Unsupported(String),
    /// The content or its tags break the rules of their format.
    Corrupt(String),
    /// The tempo is outside the range a tag may declare, or not a finite number.
    BpmOutOfRange,
    /// Platter's own data is longer than [`MAX_PLATTER_BYTES`].
    PlatterDataTooLong,
}

impl fmt::Display for TagWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot write the file: {error}"),
            Self::ReadOnly => f.write_str("the file is read-only"),
            Self::Unsupported(what) => write!(f, "cannot write tags into this file ({what})"),
            Self::Corrupt(what) => write!(f, "the file or its tags are damaged: {what}"),
            Self::BpmOutOfRange => write!(
                f,
                "the tempo is out of range: it must be from {} to {} beats per minute",
                TAG_BPM_RANGE.start(),
                TAG_BPM_RANGE.end()
            ),
            Self::PlatterDataTooLong => write!(
                f,
                "Platter's data for this track is longer than {MAX_PLATTER_BYTES} bytes"
            ),
        }
    }
}

impl std::error::Error for TagWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecodeError> for TagWriteError {
    fn from(error: DecodeError) -> Self {
        match error {
            DecodeError::Io(error) => Self::Io(error),
            DecodeError::Unsupported(what) => Self::Unsupported(what),
            other => Self::Corrupt(other.to_string()),
        }
    }
}

fn corrupt(what: &str) -> TagWriteError {
    TagWriteError::Corrupt(what.to_owned())
}

fn unsupported(what: &str) -> TagWriteError {
    TagWriteError::Unsupported(what.to_owned())
}

/// Writes the changes of `edit` into the tags of the file at `path`, keeping every other tag
/// and the audio. On error the file is left as it was.
pub fn write_tags(path: &Path, edit: &TagEdit) -> Result<(), TagWriteError> {
    let changes = Changes::new(edit)?;
    // A link is followed: the rename then replaces the track it points to, and the link
    // stays a link.
    let target;
    let path = if fs::symlink_metadata(path)
        .map_err(TagWriteError::Io)?
        .file_type()
        .is_symlink()
    {
        target = fs::canonicalize(path).map_err(TagWriteError::Io)?;
        &target
    } else {
        path
    };
    let metadata = fs::metadata(path).map_err(TagWriteError::Io)?;
    if !metadata.is_file() {
        return Err(TagWriteError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a file",
        )));
    }
    // Checked here because a rename replaces a read-only file on some systems.
    if metadata.permissions().readonly() {
        return Err(TagWriteError::ReadOnly);
    }
    let container = probe_file(path)?.container;

    let mut source = Source::open(path)?;
    let (copy, file) = Copy::create(path)?;
    let mut out = BufWriter::new(file);
    match container {
        Container::Mp3 => id3::write_mp3(&mut source, &mut out, &changes)?,
        Container::Flac => flac::write(&mut source, &mut out, &changes)?,
        Container::Ogg => ogg::write(&mut source, &mut out, &changes)?,
        Container::M4a => mp4::write(&mut source, &mut out, &changes)?,
        Container::Wav => riff::write_wav(&mut source, &mut out, &changes)?,
        Container::Aiff => riff::write_aiff(&mut source, &mut out, &changes)?,
    }
    let file = out
        .into_inner()
        .map_err(|error| TagWriteError::Io(error.into_error()))?;
    file.sync_all().map_err(TagWriteError::Io)?;
    drop(file);
    drop(source);
    // The copy is a new file: without this the track would come back with the default
    // permissions of the folder instead of its own.
    fs::set_permissions(&copy.path, metadata.permissions()).map_err(TagWriteError::Io)?;
    // The copy must still be the track Platter read; a write that broke it never replaces
    // the original.
    if probe_file(&copy.path)?.container != container {
        return Err(corrupt("the written file reads as another format"));
    }
    copy.replace(path)
}

/// The fields a tag can carry, apart from the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Title,
    Artist,
    Album,
    Genre,
    Comment,
    Key,
    Platter,
}

impl Field {
    const ALL: [Self; 7] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::Genre,
        Self::Comment,
        Self::Key,
        Self::Platter,
    ];
}

/// What happens to one field: `None` leaves it, `Some(None)` removes it, `Some(Some(text))`
/// sets it.
type Change<'a> = Option<Option<&'a str>>;

/// The edit with its text cleaned and bounded and its tempo checked.
struct Changes {
    title: Option<Option<String>>,
    artist: Option<Option<String>>,
    album: Option<Option<String>>,
    genre: Option<Option<String>>,
    comment: Option<Option<String>>,
    key: Option<Option<String>>,
    platter: Option<Option<String>>,
    bpm: Option<Option<f64>>,
}

impl Changes {
    fn new(edit: &TagEdit) -> Result<Self, TagWriteError> {
        if let Some(Some(bpm)) = edit.bpm
            && !(bpm.is_finite() && TAG_BPM_RANGE.contains(&bpm))
        {
            return Err(TagWriteError::BpmOutOfRange);
        }
        if let Some(Some(data)) = &edit.platter
            && data.len() > MAX_PLATTER_BYTES
        {
            return Err(TagWriteError::PlatterDataTooLong);
        }
        let text = |value: &Option<String>| value.as_deref().map(clean);
        Ok(Self {
            title: text(&edit.title),
            artist: text(&edit.artist),
            album: text(&edit.album),
            genre: text(&edit.genre),
            comment: text(&edit.comment),
            key: text(&edit.key),
            platter: edit.platter.clone(),
            bpm: edit.bpm,
        })
    }

    fn field(&self, field: Field) -> Change<'_> {
        let value = match field {
            Field::Title => &self.title,
            Field::Artist => &self.artist,
            Field::Album => &self.album,
            Field::Genre => &self.genre,
            Field::Comment => &self.comment,
            Field::Key => &self.key,
            Field::Platter => &self.platter,
        };
        value.as_ref().map(Option::as_deref)
    }

    /// The tempo as a whole number, as ID3v2 `TBPM` holds it.
    fn bpm_whole(&self) -> Option<Option<String>> {
        self.bpm
            .map(|bpm| bpm.map(|bpm| format!("{:.0}", bpm.round())))
    }

    /// The tempo with at most two decimals and no trailing zeros, for Vorbis comments.
    fn bpm_decimal(&self) -> Option<Option<String>> {
        self.bpm.map(|bpm| bpm.map(decimal))
    }

    /// Whether the edit sets a value of `field` (not only removes it).
    fn sets(&self, field: Field) -> bool {
        matches!(self.field(field), Some(Some(_)))
    }

    /// Whether the edit sets a value of any field, the tempo included: only then is a tag
    /// created in a file that has none.
    fn sets_any(&self) -> bool {
        Field::ALL.into_iter().any(|field| self.sets(field)) || matches!(self.bpm, Some(Some(_)))
    }
}

/// Text as written: trimmed, without control characters other than line feed and tab, cut at
/// [`MAX_TAG_CHARS`]; `None` when nothing is left.
fn clean(value: &str) -> Option<String> {
    let text: String = value
        .trim()
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .take(MAX_TAG_CHARS)
        .collect();
    let text = text.trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A tempo with at most two decimals and no trailing zeros: `128`, `127.5`, `99.99`.
fn decimal(bpm: f64) -> String {
    let text = format!("{bpm:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The original file, read by position.
struct Source {
    file: File,
    len: u64,
}

impl Source {
    fn open(path: &Path) -> Result<Self, TagWriteError> {
        let file = File::open(path).map_err(TagWriteError::Io)?;
        let len = file.metadata().map_err(TagWriteError::Io)?.len();
        Ok(Self { file, len })
    }

    /// `len` bytes at `at`, which must lie inside the file and within the metadata bound.
    fn read(&mut self, at: u64, len: u64) -> Result<Vec<u8>, TagWriteError> {
        if len > MAX_METADATA_BYTES {
            return Err(unsupported("its metadata is larger than 64 MiB"));
        }
        let end = at
            .checked_add(len)
            .filter(|end| *end <= self.len)
            .ok_or_else(|| corrupt("the data ends unexpectedly"))?;
        let mut bytes = vec![0; usize::try_from(end - at).map_err(|_| corrupt("size"))?];
        self.file
            .seek(SeekFrom::Start(at))
            .map_err(TagWriteError::Io)?;
        self.file
            .read_exact(&mut bytes)
            .map_err(TagWriteError::Io)?;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self, at: u64) -> Result<[u8; N], TagWriteError> {
        let bytes = self.read(at, N as u64)?;
        bytes
            .try_into()
            .map_err(|_| corrupt("the data ends unexpectedly"))
    }

    /// Copies the bytes from `start` to `end` of the file into `out`.
    fn copy(&mut self, start: u64, end: u64, out: &mut impl Write) -> Result<(), TagWriteError> {
        if start > end || end > self.len {
            return Err(corrupt("the data ends unexpectedly"));
        }
        self.file
            .seek(SeekFrom::Start(start))
            .map_err(TagWriteError::Io)?;
        let copied =
            io::copy(&mut (&mut self.file).take(end - start), out).map_err(TagWriteError::Io)?;
        if copied != end - start {
            return Err(corrupt("the file changed while it was written"));
        }
        Ok(())
    }
}

fn put(out: &mut impl Write, bytes: &[u8]) -> Result<(), TagWriteError> {
    out.write_all(bytes).map_err(TagWriteError::Io)
}

/// `bytes[start..start + len]`, or an error when the range does not fit.
fn slice(bytes: &[u8], start: usize, len: usize) -> Result<&[u8], TagWriteError> {
    start
        .checked_add(len)
        .and_then(|end| bytes.get(start..end))
        .ok_or_else(|| corrupt("a size runs past what holds it"))
}

fn be_u32(bytes: &[u8], at: usize) -> Result<u32, TagWriteError> {
    let field: [u8; 4] = slice(bytes, at, 4)?
        .try_into()
        .map_err(|_| corrupt("size"))?;
    Ok(u32::from_be_bytes(field))
}

fn le_u32(bytes: &[u8], at: usize) -> Result<u32, TagWriteError> {
    let field: [u8; 4] = slice(bytes, at, 4)?
        .try_into()
        .map_err(|_| corrupt("size"))?;
    Ok(u32::from_le_bytes(field))
}

fn to_usize(value: impl TryInto<usize>) -> Result<usize, TagWriteError> {
    value.try_into().map_err(|_| corrupt("a size does not fit"))
}

fn to_u32(value: usize) -> Result<u32, TagWriteError> {
    u32::try_from(value).map_err(|_| unsupported("a tag is too large for its format"))
}

/// A copy of the track in its folder, deleted when dropped unless it replaced the track.
struct Copy {
    path: PathBuf,
    kept: bool,
}

impl Copy {
    fn create(original: &Path) -> Result<(Self, File), TagWriteError> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let folder = match original.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder,
            _ => Path::new("."),
        };
        let mut attempt = 0;
        loop {
            let number = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = folder.join(format!(".platter-tags-{}-{number}.tmp", std::process::id()));
            // `create_new` never takes over a file that is already there.
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                // From here on the copy is deleted on every way out.
                Ok(file) => return Ok((Self { path, kept: false }, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                    if attempt >= COPY_ATTEMPTS {
                        return Err(TagWriteError::Io(error));
                    }
                }
                Err(error) => return Err(TagWriteError::Io(error)),
            }
        }
    }

    fn replace(mut self, original: &Path) -> Result<(), TagWriteError> {
        fs::rename(&self.path, original).map_err(TagWriteError::Io)?;
        self.kept = true;
        Ok(())
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        if !self.kept {
            // Nothing more can be done if the copy cannot be deleted; the original is intact
            // and the error that brought us here is what the caller needs.
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clean, decimal};

    #[test]
    fn text_is_trimmed_cleaned_and_bounded() {
        assert_eq!(clean("  Title \0 x\n "), Some("Title  x".to_owned()));
        assert_eq!(clean("a\tb\nc"), Some("a\tb\nc".to_owned()));
        assert_eq!(clean("   "), None);
        assert_eq!(clean("\u{1}"), None);
        let long = "é".repeat(super::MAX_TAG_CHARS + 5);
        assert_eq!(
            clean(&long).map(|text| text.chars().count()),
            Some(super::MAX_TAG_CHARS)
        );
    }

    #[test]
    fn the_tempo_keeps_two_decimals_without_trailing_zeros() {
        assert_eq!(decimal(128.0), "128");
        assert_eq!(decimal(127.5), "127.5");
        assert_eq!(decimal(99.994), "99.99");
        assert_eq!(decimal(120.10), "120.1");
    }
}
