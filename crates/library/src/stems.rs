//! Finding the files of a set of stems in a folder.
//!
//! Behaviour note: `docs/behavior/stems.md`, *Stems of a track*. The library knows no audio
//! format: whoever calls says which files are audio.

use std::fmt;
use std::path::{Path, PathBuf};

use platter_core::Stem;

use crate::{MAX_ENTRIES, StemPaths};

/// Why a folder gives no set of stems.
#[derive(Debug)]
pub enum StemFolderError {
    /// The folder cannot be read.
    Unreadable(std::io::Error),
    /// No audio file of the folder is named after this stem.
    Missing(Stem),
    /// Two audio files of the folder are named after this stem.
    Twice(Stem),
}

impl fmt::Display for StemFolderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(error) => write!(formatter, "the folder cannot be read: {error}"),
            Self::Missing(stem) => write!(formatter, "the folder has no file named {stem}"),
            Self::Twice(stem) => write!(formatter, "the folder has two files named {stem}"),
        }
    }
}

impl std::error::Error for StemFolderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable(error) => Some(error),
            Self::Missing(_) | Self::Twice(_) => None,
        }
    }
}

/// The four files of `folder` whose names without the extension are those of the stems, in
/// any case, among the files `is_audio` accepts. Folders inside it are not looked into, and
/// only its first [`MAX_ENTRIES`] entries are looked at.
pub fn find_stems(
    folder: &Path,
    is_audio: impl Fn(&Path) -> bool,
) -> Result<StemPaths, StemFolderError> {
    let mut found: [Option<PathBuf>; Stem::COUNT] = Default::default();
    let entries = std::fs::read_dir(folder).map_err(StemFolderError::Unreadable)?;
    for entry in entries.take(MAX_ENTRIES).flatten() {
        let path = entry.path();
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(stem) = Stem::parse(name) else {
            continue;
        };
        if !is_file || !is_audio(&path) {
            continue;
        }
        let slot = &mut found[stem.index()];
        if slot.is_some() {
            return Err(StemFolderError::Twice(stem));
        }
        *slot = Some(path);
    }
    let mut paths = Vec::with_capacity(Stem::COUNT);
    for (stem, path) in Stem::ALL.into_iter().zip(found) {
        paths.push(path.ok_or(StemFolderError::Missing(stem))?);
    }
    paths
        .try_into()
        .map_err(|_| StemFolderError::Missing(Stem::Drums))
}
