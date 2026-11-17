//! Backups of the library file: a copy of the file as the last run left it, made when the
//! library is opened again.
//!
//! Behaviour note: `docs/behavior/library.md`, section *Backups*.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::Timestamp;

/// Backups kept; older ones are deleted.
pub const BACKUPS_KEPT: usize = 20;
const PREFIX: &str = "library-";
const SUFFIX: &str = ".sqlite";
/// Files of the folder of backups looked at; a larger folder is not one Platter made.
const MAX_LISTED: usize = 10_000;

/// Copies the library `file` into `folder` as it is now, named by `at` in UTC, unless it is the
/// same as the newest copy there or SQLite left a journal beside it. Then deletes the copies
/// beyond the [`BACKUPS_KEPT`] newest. Returns the copy made, if one was.
pub fn back_up(file: &Path, folder: &Path, at: Timestamp) -> io::Result<Option<PathBuf>> {
    if !file.is_file() || journal_of(file).exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(folder)?;
    let mut copies = copies(folder)?;
    if let Some(newest) = copies.last()
        && same_contents(file, newest)?
    {
        return Ok(None);
    }
    let (year, month, day, hour, minute, second) = at.utc();
    let copy = folder.join(format!(
        "{PREFIX}{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}{SUFFIX}"
    ));
    if copy.exists() {
        return Ok(None);
    }
    std::fs::copy(file, &copy)?;
    copies.push(copy.clone());
    copies.sort();
    let excess = copies.len().saturating_sub(BACKUPS_KEPT);
    for old in &copies[..excess] {
        std::fs::remove_file(old)?;
    }
    Ok(Some(copy))
}

/// The journal SQLite keeps beside a file while a change is not finished.
fn journal_of(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push("-journal");
    PathBuf::from(name)
}

/// The copies in `folder`, oldest first: their names sort as their times.
fn copies(folder: &Path) -> io::Result<Vec<PathBuf>> {
    let mut copies = Vec::new();
    for entry in std::fs::read_dir(folder)?.take(MAX_LISTED) {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let stamp = name
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.strip_suffix(SUFFIX));
        let named = stamp.is_some_and(|stamp| {
            stamp.len() == 15
                && stamp.char_indices().all(|(index, character)| {
                    if index == 8 {
                        character == '-'
                    } else {
                        character.is_ascii_digit()
                    }
                })
        });
        if named && entry.file_type()?.is_file() {
            copies.push(entry.path());
        }
    }
    copies.sort();
    Ok(copies)
}

/// Whether two files hold the same bytes, read a block at a time.
fn same_contents(one: &Path, other: &Path) -> io::Result<bool> {
    if std::fs::metadata(one)?.len() != std::fs::metadata(other)?.len() {
        return Ok(false);
    }
    let (mut one, mut other) = (File::open(one)?, File::open(other)?);
    let (mut a, mut b) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let read = one.read(&mut a)?;
        if read == 0 {
            return Ok(true);
        }
        other.read_exact(&mut b[..read])?;
        if a[..read] != b[..read] {
            return Ok(false);
        }
    }
}
