//! The library of Platter: the track collection, the playlists and their folders, and the
//! history of each session, kept in one file.
//!
//! Behaviour note: `docs/behavior/library.md`; how the file is kept:
//! `docs/decisions/0022-library-store.md`.
//!
//! Every call does its work at once, on the thread that makes it, and may touch the disk:
//! callers run the library on a worker thread, never on the audio or the interface thread.

mod error;
mod store;
mod text;

use std::path::{Path, PathBuf};

use platter_core::DeckId;

pub use error::{LibraryError, StoreError};
pub use store::SqliteLibrary;

/// Longest tag kept, in characters; longer tags are cut.
pub const MAX_TAG_CHARS: usize = 1000;
/// Longest name of a playlist or a folder, in characters.
pub const MAX_NAME_CHARS: usize = 200;
/// Most tracks one search returns.
pub const MAX_PAGE: usize = 1000;

/// A moment, in whole seconds since 1 January 1970 UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(i64);

/// A playlist or a folder of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(i64);

/// One place of a track in a playlist; a track in a playlist twice has two entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(i64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(i64);

/// What the importer read from a file. Values that make no sense are stored as none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    /// Length in seconds.
    pub seconds: Option<f64>,
    pub sample_rate: Option<u32>,
    /// Tempo in beats per minute.
    pub bpm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: TrackId,
    pub path: PathBuf,
    pub tags: TrackTags,
    pub imported: Timestamp,
    pub play_count: u32,
    pub last_played: Option<Timestamp>,
    /// The file was not there at the last consistency check.
    pub missing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Folder,
    Playlist,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    /// `None` at the root of the tree.
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    pub track: TrackId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub opened: Timestamp,
}

/// A track played in a session, as it was when it was played.
#[derive(Clone, Debug, PartialEq)]
pub struct Played {
    /// `None` once the track is removed from the collection.
    pub track: Option<TrackId>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub deck: DeckId,
    pub started: Timestamp,
    pub seconds: f64,
}

/// What a consistency check changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Tracks whose file is gone, now marked missing.
    pub missing: Vec<TrackId>,
    /// Tracks whose file is back, no longer missing.
    pub found: Vec<TrackId>,
}

/// A column a search sorts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Title,
    Artist,
    Album,
    Genre,
    Tempo,
    Duration,
    Imported,
    PlayCount,
    /// The own order of a playlist; in the collection, the path.
    Order,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// Words that all have to be found in the title, artist, album or genre.
    pub text: String,
    /// A playlist to search in; `None` for the whole collection.
    pub playlist: Option<NodeId>,
    pub column: Column,
    pub descending: bool,
    pub offset: usize,
    /// At most [`MAX_PAGE`].
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            playlist: None,
            column: Column::Order,
            descending: false,
            offset: 0,
            limit: MAX_PAGE,
        }
    }
}

/// One track found, with its entry when the search ran in a playlist.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub track: Track,
    pub entry: Option<EntryId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub hits: Vec<Hit>,
    /// How many matched in all, beyond this page too.
    pub total: usize,
}

/// The library as Platter uses it. [`SqliteLibrary`] keeps it in one SQLite file.
pub trait Library: Send {
    /// Adds the track at `path`, or updates the tags of the one already there.
    fn import(
        &mut self,
        path: &Path,
        tags: &TrackTags,
        at: Timestamp,
    ) -> Result<TrackId, LibraryError>;
    fn track(&self, id: TrackId) -> Result<Option<Track>, LibraryError>;
    fn track_at(&self, path: &Path) -> Result<Option<Track>, LibraryError>;
    /// Removes the track from the collection and from every playlist.
    fn remove_track(&mut self, id: TrackId) -> Result<(), LibraryError>;
    /// Marks the tracks whose file is gone as missing, and those whose file is back as not.
    fn check_files(&mut self) -> Result<CheckReport, LibraryError>;
    /// Points a track at another file, keeping everything else.
    fn relocate(&mut self, id: TrackId, path: &Path) -> Result<(), LibraryError>;

    fn create(
        &mut self,
        parent: Option<NodeId>,
        kind: NodeKind,
        name: &str,
    ) -> Result<NodeId, LibraryError>;
    fn rename(&mut self, id: NodeId, name: &str) -> Result<(), LibraryError>;
    fn move_node(&mut self, id: NodeId, parent: Option<NodeId>) -> Result<(), LibraryError>;
    /// Deletes a playlist, or a folder with everything in it.
    fn delete_node(&mut self, id: NodeId) -> Result<(), LibraryError>;
    /// The folders and playlists in `parent`, by name.
    fn children(&self, parent: Option<NodeId>) -> Result<Vec<Node>, LibraryError>;

    /// Adds tracks to a playlist at `at`, or at its end.
    fn add(
        &mut self,
        playlist: NodeId,
        tracks: &[TrackId],
        at: Option<usize>,
    ) -> Result<(), LibraryError>;
    /// The entries of a playlist in its own order.
    fn entries(&self, playlist: NodeId) -> Result<Vec<Entry>, LibraryError>;
    fn remove_entries(&mut self, playlist: NodeId, entries: &[EntryId])
    -> Result<(), LibraryError>;
    /// Moves an entry to position `to`, or to the end when `to` is past it.
    fn move_entry(
        &mut self,
        playlist: NodeId,
        entry: EntryId,
        to: usize,
    ) -> Result<(), LibraryError>;
    /// Removes the repeats of each track, keeping its first entry.
    fn remove_repeats(&mut self, playlist: NodeId) -> Result<(), LibraryError>;
    fn clear(&mut self, playlist: NodeId) -> Result<(), LibraryError>;

    fn open_session(&mut self, at: Timestamp) -> Result<SessionId, LibraryError>;
    /// The sessions, the latest first.
    fn sessions(&self) -> Result<Vec<Session>, LibraryError>;
    /// Records a track played in a session and counts it as played.
    fn record_played(
        &mut self,
        session: SessionId,
        track: TrackId,
        deck: DeckId,
        started: Timestamp,
        seconds: f64,
    ) -> Result<(), LibraryError>;
    /// The tracks played in a session, in order.
    fn history(&self, session: SessionId) -> Result<Vec<Played>, LibraryError>;
    /// Sets the play count of a track to zero and forgets when it was last played.
    fn reset_played(&mut self, track: TrackId) -> Result<(), LibraryError>;

    fn search(&self, query: &Query) -> Result<Page, LibraryError>;
}
