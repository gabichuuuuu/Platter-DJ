//! Importing the music of a folder and of the folders inside it.
//!
//! Behaviour note: `docs/behavior/library.md`, section *Importing folders*; the choices:
//! `docs/decisions/0023-folder-import.md`. The library knows no audio format: whoever calls
//! gives a [`TagReader`] that says what each file is.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

use crate::{Library, LibraryError, Timestamp, TrackId, TrackTags};

/// Levels of folders read below the folder imported.
pub const MAX_DEPTH: usize = 32;
/// Entries of one folder that are looked at. A folder with more has the rest left out.
pub const MAX_ENTRIES: usize = 10_000;
/// Files looked at in all by a relocation; the rest of a larger folder is left out, since the
/// user may point it at the root of a drive.
pub const MAX_RELOCATE_FILES: usize = 200_000;
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

    /// Called before each file an import looks at, with how many it has looked at so far;
    /// `false` stops the import there. The default goes on to the end.
    fn keep_going(&mut self, _looked: usize) -> bool {
        true
    }
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
    /// cannot be read, entries of a folder past [`MAX_ENTRIES`], and anything that is neither
    /// a file nor a folder.
    pub skipped: usize,
    /// The reader stopped the import before its end.
    pub stopped: bool,
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
    let mut looked = 0;
    'folders: while let Some((folder, depth)) = pending.pop() {
        let (files, folders) = list(&folder, &mut report);
        for path in files {
            if !reader.keep_going(looked) {
                report.stopped = true;
                break 'folders;
            }
            looked += 1;
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

/// What a relocation did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelocateReport {
    /// The tracks now pointed at their file.
    pub relocated: Vec<TrackId>,
    /// Missing tracks whose file name was not found.
    pub not_found: usize,
    /// Missing tracks left as they are because the name was found more than once, two missing
    /// tracks share it, or another track already has the path found.
    pub ambiguous: usize,
}

/// Looks in `folder` and in the folders inside it for the files of the tracks marked missing
/// whose file is still gone, by file name with case ignored, and points each track found at its
/// file, keeping everything the library knows about it. Only a name found once, and wanted by
/// one track, is taken: a wrong match would silently swap music. At most
/// [`MAX_RELOCATE_FILES`] files and [`MAX_DEPTH`] levels are looked at.
pub fn relocate_missing(
    library: &mut dyn Library,
    folder: &Path,
) -> Result<RelocateReport, LibraryError> {
    let root = plain(std::fs::canonicalize(folder).map_err(LibraryError::NotAFolder)?);
    if !root.is_dir() {
        return Err(LibraryError::NotAFolder(
            io::ErrorKind::NotADirectory.into(),
        ));
    }
    let key = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    };
    let missing: Vec<_> = library
        .missing()?
        .into_iter()
        .filter(|track| !track.path.is_file())
        .collect();
    let mut wanted: HashMap<String, usize> = HashMap::new();
    for name in missing.iter().filter_map(|track| key(&track.path)) {
        *wanted.entry(name).or_default() += 1;
    }
    // Where each wanted name was found; two places are enough to know it is ambiguous.
    let mut found: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut looked = 0;
    let mut ignored = ImportReport::default();
    let mut pending = vec![(root, 0)];
    'walk: while let Some((folder, depth)) = pending.pop() {
        let (files, folders) = list(&folder, &mut ignored);
        for path in files {
            if looked == MAX_RELOCATE_FILES {
                break 'walk;
            }
            looked += 1;
            if let Some(name) = key(&path).filter(|name| wanted.contains_key(name)) {
                let places = found.entry(name).or_default();
                if places.len() < 2 {
                    places.push(path);
                }
            }
        }
        if depth < MAX_DEPTH {
            pending.extend(folders.into_iter().rev().map(|inner| (inner, depth + 1)));
        }
    }
    let mut report = RelocateReport::default();
    for track in missing {
        let Some(name) = key(&track.path) else {
            report.not_found += 1;
            continue;
        };
        match found.get(&name).map(Vec::as_slice) {
            None | Some([]) => report.not_found += 1,
            Some([path]) if wanted.get(&name) == Some(&1) => {
                match library.relocate(track.id, path) {
                    Ok(()) => report.relocated.push(track.id),
                    Err(LibraryError::PathTaken) => report.ambiguous += 1,
                    Err(error) => return Err(error),
                }
            }
            Some(_) => report.ambiguous += 1,
        }
    }
    Ok(report)
}

/// The track of the collection at `path`, taking the file into the collection first when it
/// is not there yet. `None` when there is no file there, or the reader finds no music in it.
pub fn import_file(
    library: &mut dyn Library,
    path: &Path,
    reader: &mut dyn TagReader,
    at: Timestamp,
) -> Result<Option<TrackId>, LibraryError> {
    // A track of the collection whose file is gone keeps its place there.
    if let Some(track) = library.track_at(path)? {
        return Ok(Some(track.id));
    }
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return Ok(None);
    };
    let path = plain(canonical);
    if let Some(track) = library.track_at(&path)? {
        return Ok(Some(track.id));
    }
    if !path.is_file() {
        return Ok(None);
    }
    match reader.read(&path) {
        Reading::Tags(tags) => library.import(&path, &tags, at).map(Some),
        Reading::Unreadable | Reading::NotAudio => Ok(None),
    }
}

/// The files and the folders of `folder`, each sorted by name with case ignored.
fn list(folder: &Path, report: &mut ImportReport) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut files, mut folders) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(folder) else {
        report.skipped += 1;
        return (files, folders);
    };
    for (index, entry) in entries.enumerate() {
        if index >= MAX_ENTRIES {
            report.skipped += 1;
            continue;
        }
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
