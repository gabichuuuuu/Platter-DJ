//! Importing the music of a folder and of the folders inside it.
//!
//! Behaviour note: `docs/behavior/library.md`, section *Importing folders*; the choices:
//! `docs/decisions/0023-folder-import.md`. The library knows no audio format: whoever calls
//! gives a [`TagReader`] that says what each file is.

use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

use crate::{Library, LibraryError, Timestamp, TrackId, TrackTags};

/// Levels of folders read below the folder imported.
pub const MAX_DEPTH: usize = 32;
/// Entries of one folder that are looked at. A folder with more has the rest left out.
pub const MAX_ENTRIES: usize = 10_000;
/// Tracks written in one transaction. One transaction per track costs about 2 ms on a
/// solid-state disk, so a large folder would take minutes.
const BATCH: usize = 500;

/// What a file turned out to be.
#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// Not a file of a format Platter plays; it is left out without a word.
    NotAudio,
    /// It looks like one but cannot be read.
    Unreadable,
    Tags(TrackTags),
}

pub trait TagReader {
    /// Reads the file at `path`. Called only for files the collection does not hold yet.
    fn read(&mut self, path: &Path) -> Reading;
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// The tracks added to the collection, in the order they were found.
    pub added: Vec<TrackId>,
    /// Files already in the collection, left as they are.
    pub known: usize,
    /// Files that could not be read.
    pub unreadable: usize,
    /// Entries left out: links, names that are not valid Unicode, folders too deep or that
    /// cannot be read, and anything that is neither a file nor a folder.
    pub skipped: usize,
}

/// Adds to the collection the music in `folder` and in the folders inside it, down to
/// [`MAX_DEPTH`] levels. Files the collection already holds are left as they are. Hidden
/// entries, those whose name starts with a dot, are not looked at, and links are not
/// followed. Each folder gives its files first, by name, then its folders, by name.
///
/// Tracks are written in batches; when the library fails, the batches written before stay.
pub fn import_folder(
    library: &mut dyn Library,
    folder: &Path,
    reader: &mut dyn TagReader,
    at: Timestamp,
) -> Result<ImportReport, LibraryError> {
    let root = plain(std::fs::canonicalize(folder).map_err(LibraryError::NotAFolder)?);
    if !root.is_dir() {
        return Err(LibraryError::NotAFolder(
            io::ErrorKind::NotADirectory.into(),
        ));
    }
    if root.to_str().is_none() {
        return Err(LibraryError::NotUnicode);
    }
    let mut report = ImportReport::default();
    let mut batch: Vec<(PathBuf, TrackTags)> = Vec::new();
    // Folders still to read with their level; the last one is read next.
    let mut pending = vec![(root, 0)];
    while let Some((folder, depth)) = pending.pop() {
        let (files, folders) = list(&folder, &mut report);
        for path in files {
            if library.track_at(&path)?.is_some() {
                report.known += 1;
                continue;
            }
            match reader.read(&path) {
                Reading::NotAudio => {}
                Reading::Unreadable => report.unreadable += 1,
                Reading::Tags(tags) => {
                    batch.push((path, tags));
                    if batch.len() == BATCH {
                        report.added.extend(library.import_all(&batch, at)?);
                        batch.clear();
                    }
                }
            }
        }
        if depth == MAX_DEPTH {
            report.skipped += folders.len();
            continue;
        }
        pending.extend(folders.into_iter().rev().map(|inner| (inner, depth + 1)));
    }
    if !batch.is_empty() {
        report.added.extend(library.import_all(&batch, at)?);
    }
    Ok(report)
}

/// The files and the folders of `folder`, each sorted by name with case ignored.
fn list(folder: &Path, report: &mut ImportReport) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut files, mut folders) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(folder) else {
        report.skipped += 1;
        return (files, folders);
    };
    for entry in entries.take(MAX_ENTRIES) {
        let Ok(entry) = entry else {
            report.skipped += 1;
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            report.skipped += 1;
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        // The type of the entry itself: a link, junctions included, is not followed, so a
        // link back up the tree cannot make the walk go round.
        match entry.file_type() {
            Ok(kind) if kind.is_file() => files.push(entry.path()),
            Ok(kind) if kind.is_dir() => folders.push(entry.path()),
            _ => report.skipped += 1,
        }
    }
    let key = |path: &PathBuf| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    };
    files.sort_by_key(key);
    folders.sort_by_key(key);
    (files, folders)
}

/// The path without the prefix `\\?\` that Windows gives a canonical path, so that one file
/// has one path however it was reached and the path reads as the user knows it.
fn plain(path: PathBuf) -> PathBuf {
    let mut components = path.components();
    let mut plain = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(letter) => PathBuf::from(format!("{}:", char::from(letter))),
            Prefix::VerbatimUNC(server, share) => {
                let mut start = OsString::from(r"\\");
                start.push(server);
                start.push(r"\");
                start.push(share);
                PathBuf::from(start)
            }
            _ => return path,
        },
        _ => return path,
    };
    for component in components {
        plain.push(component);
    }
    plain
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_verbatim_prefix_is_dropped() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\C:\Music\a b.flac")),
            PathBuf::from(r"C:\Music\a b.flac")
        );
        assert_eq!(
            plain(PathBuf::from(r"\\?\UNC\server\share\set.mp3")),
            PathBuf::from(r"\\server\share\set.mp3")
        );
        assert_eq!(plain(PathBuf::from(r"D:\x")), PathBuf::from(r"D:\x"));
    }
}
