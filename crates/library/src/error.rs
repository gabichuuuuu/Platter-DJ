use std::error::Error;
use std::fmt;

/// Why the file of the library failed; its cause comes from the store behind it.
#[derive(Debug)]
pub struct StoreError(rusqlite::Error);

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

#[derive(Debug)]
pub enum LibraryError {
    /// The file could not be read or written, or is damaged.
    Store(StoreError),
    /// The file is not a library of Platter.
    NotALibrary,
    /// The file was written by a newer version of Platter.
    Newer {
        version: i64,
    },
    /// A path that is not valid Unicode.
    NotUnicode,
    /// A name that is empty or longer than [`crate::MAX_NAME_CHARS`] characters.
    InvalidName,
    NoSuchTrack,
    NoSuchNode,
    NoSuchSession,
    /// The entry is not in that playlist.
    NoSuchEntry,
    /// A playlist was expected and the node is a folder or a smart playlist, or the other way
    /// round.
    WrongKind,
    /// A rule of a smart playlist that means nothing, or more than [`crate::MAX_RULES`] of
    /// them.
    InvalidRule,
    /// A folder cannot move into itself or into a folder inside it.
    Cycle,
    /// Another track already has that path.
    PathTaken,
    /// A colour or a rating outside its range.
    OutOfRange,
    /// The folder to import does not exist, is not a folder or cannot be read.
    NotAFolder(std::io::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "the library file failed: {error}"),
            Self::NotALibrary => formatter.write_str("the file is not a library of Platter"),
            Self::Newer { version } => write!(
                formatter,
                "the library was written by a newer version of Platter (schema {version})"
            ),
            Self::NotUnicode => formatter.write_str("the path is not valid Unicode"),
            Self::InvalidName => write!(
                formatter,
                "a name has from 1 to {} characters",
                crate::MAX_NAME_CHARS
            ),
            Self::NoSuchTrack => formatter.write_str("no such track in the library"),
            Self::NoSuchNode => formatter.write_str("no such playlist or folder"),
            Self::NoSuchSession => formatter.write_str("no such session"),
            Self::NoSuchEntry => formatter.write_str("no such entry in the playlist"),
            Self::WrongKind => formatter.write_str("a playlist and a folder were mixed up"),
            Self::InvalidRule => write!(
                formatter,
                "a smart playlist has up to {} rules, each with a value in its range",
                crate::MAX_RULES
            ),
            Self::Cycle => formatter.write_str("a folder cannot go inside itself"),
            Self::PathTaken => formatter.write_str("another track already has that path"),
            Self::OutOfRange => formatter.write_str("a colour or a rating out of its range"),
            Self::NotAFolder(error) => write!(formatter, "the folder cannot be read: {error}"),
        }
    }
}

impl Error for LibraryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::NotAFolder(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for LibraryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Store(StoreError(error))
    }
}
