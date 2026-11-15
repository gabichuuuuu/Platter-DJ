//! Reading and writing playlists in the M3U format.
//! Behaviour note: docs/behavior/library.md, section *M3U files*.
//!
//! The functions work on bytes and text and never touch the file system: the caller reads and
//! writes the file and checks that each path exists.

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

/// Largest playlist file read, in bytes; a larger one is refused.
pub const MAX_M3U_BYTES: usize = 4 * 1024 * 1024;
/// Most entries one playlist file yields; the entry lines past it are counted as skipped.
pub const MAX_M3U_ENTRIES: usize = 100_000;

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// One track of a playlist file, with what its `#EXTINF` line said about it.
#[derive(Clone, Debug, PartialEq)]
pub struct M3uEntry {
    pub path: PathBuf,
    pub artist: Option<String>,
    pub title: Option<String>,
    /// Length in seconds, only when the file gave a positive one.
    pub seconds: Option<f64>,
}

/// The entries of a playlist file and how many of its entry lines could not be used.
#[derive(Clone, Debug, PartialEq)]
pub struct M3uReading {
    pub entries: Vec<M3uEntry>,
    /// Lines of other URL schemes, with a broken escape or an empty path, and lines past
    /// [`MAX_M3U_ENTRIES`].
    pub skipped: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum M3uError {
    /// The file is larger than [`MAX_M3U_BYTES`].
    TooLarge,
}

impl fmt::Display for M3uError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(
                formatter,
                "the playlist file is larger than {MAX_M3U_BYTES} bytes"
            ),
        }
    }
}

impl Error for M3uError {}

/// What an `#EXTINF` line said, waiting for the entry it describes.
#[derive(Default)]
struct Info {
    artist: Option<String>,
    title: Option<String>,
    seconds: Option<f64>,
}

/// Reads a playlist file. `folder` is the folder that holds it, which relative paths are
/// resolved against.
pub fn read_m3u(bytes: &[u8], folder: &Path) -> Result<M3uReading, M3uError> {
    if bytes.len() > MAX_M3U_BYTES {
        return Err(M3uError::TooLarge);
    }
    let text = decode(bytes);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut entries = Vec::new();
    let mut skipped = 0;
    // Set by an `#EXTINF` line and taken by the next entry line, whether or not that line is
    // usable, so a description never lands on a later entry.
    let mut pending: Option<Info> = None;
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(info) = rest.strip_prefix("EXTINF:") {
                pending = Some(parse_info(info));
            }
            continue;
        }
        let info = pending.take().unwrap_or_default();
        if entries.len() >= MAX_M3U_ENTRIES {
            skipped += 1;
            continue;
        }
        match entry_path(line, folder) {
            Some(path) => entries.push(M3uEntry {
                path,
                artist: info.artist,
                title: info.title,
                seconds: info.seconds,
            }),
            None => skipped += 1,
        }
    }
    Ok(M3uReading { entries, skipped })
}

/// Writes entries as an extended M3U in UTF-8 with `\n` line ends (the `.m3u8` convention).
/// Paths are written as they are, with the separators of the platform.
pub fn write_m3u(entries: &[M3uEntry]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for entry in entries {
        let artist = one_line(entry.artist.as_deref());
        let title = one_line(entry.title.as_deref());
        let display = match (artist.is_empty(), title.is_empty()) {
            (false, false) => format!("{artist} - {title}"),
            (true, false) => title,
            (false, true) => artist,
            (true, true) => String::new(),
        };
        let path = entry.path.to_string_lossy();
        // Writing to a String cannot fail.
        let _ = writeln!(out, "#EXTINF:{},{display}", whole_seconds(entry.seconds));
        out.push_str(&one_line(Some(&path)));
        out.push('\n');
    }
    out
}

/// A line break inside a value would split it into two lines of the file.
fn one_line(value: Option<&str>) -> String {
    value
        .map(|text| text.replace(['\r', '\n'], " ").trim().to_owned())
        .unwrap_or_default()
}

/// The length rounded to whole seconds, or -1 when it is unknown or rounds to zero, which the
/// reader would not take for a length either.
fn whole_seconds(seconds: Option<f64>) -> i64 {
    match seconds {
        Some(value) if value.is_finite() && value >= 0.5 => value.round() as i64,
        _ => -1,
    }
}

/// UTF-8 when the bytes are valid, otherwise Latin-1, the usual encoding of the old `.m3u`.
fn decode(bytes: &[u8]) -> Cow<'_, str> {
    let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(bytes.iter().map(|&byte| char::from(byte)).collect()),
    }
}

/// The part of an `#EXTINF` line after the colon: `<seconds>[ attributes],<display>`.
fn parse_info(text: &str) -> Info {
    let (head, display) = text.split_once(',').unwrap_or((text, ""));
    let seconds = head
        .split_whitespace()
        .next()
        .and_then(|word| word.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);
    let (artist, title) = match display.split_once(" - ") {
        Some((artist, title)) => (non_empty(artist), non_empty(title)),
        None => (None, non_empty(display)),
    };
    Info {
        artist,
        title,
        seconds,
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The path of an entry line, or `None` when the line cannot be used.
fn entry_path(line: &str, folder: &Path) -> Option<PathBuf> {
    if line.contains('\0') {
        return None;
    }
    let text = match url_scheme(line) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            Cow::Owned(file_url_path(&line[scheme.len() + 3..])?)
        }
        Some(_) => return None,
        None => Cow::Borrowed(line),
    };
    resolve(&text, folder)
}

/// The scheme of a URL (`^[a-zA-Z][a-zA-Z0-9+.-]+://`), which needs two characters at least so
/// that a drive letter is not taken for one.
fn url_scheme(line: &str) -> Option<&str> {
    let (scheme, _) = line.split_once("://")?;
    let mut chars = scheme.chars();
    let valid = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && scheme.len() >= 2
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    valid.then_some(scheme)
}

/// The path in what follows `file://`: `/C:/x.mp3` becomes `C:/x.mp3`, `/music/x.mp3` stays, an
/// empty or `localhost` host is dropped and another host makes a UNC path.
fn file_url_path(rest: &str) -> Option<String> {
    let (host, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let path = percent_decode(path)?;
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return Some(format!("\\\\{host}{path}"));
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && bytes.get(3).is_none_or(|&byte| byte == b'/');
    Some(if drive { path[1..].to_owned() } else { path })
}

/// Decodes `%XX` escapes as UTF-8; `None` for a broken escape or bytes that are not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let high = hex_value(*bytes.get(at + 1)?)?;
            let low = hex_value(*bytes.get(at + 2)?)?;
            out.push(high << 4 | low);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

/// Keeps an absolute path as written and puts a relative one under `folder`, taking both `/`
/// and `\` for separators whatever the platform.
fn resolve(text: &str, folder: &Path) -> Option<PathBuf> {
    let bytes = text.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return Some(PathBuf::from(text));
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        // `C:\x` and `C:/x` are absolute; `C:x` depends on a current folder of the drive.
        return matches!(bytes.get(2), Some(b'/' | b'\\')).then(|| PathBuf::from(text));
    }
    let mut path = folder.to_path_buf();
    let mut pushed = false;
    for part in text.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        path.push(part);
        pushed = true;
    }
    pushed.then_some(path)
}
