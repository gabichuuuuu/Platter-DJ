//! The library of Platter: the track collection, the playlists and their folders, and the
//! history of each session, kept in one file.
//!
//! Behaviour note: `docs/behavior/library.md`; how the file is kept:
//! `docs/decisions/0022-library-store.md`.
//!
//! Every call does its work at once, on the thread that makes it, and may touch the disk:
//! callers run the library on a worker thread, never on the audio or the interface thread.

mod backup;
mod error;
mod folder;
mod m3u;
mod rekordbox;
mod smart;
mod stems;
mod store;
mod text;
mod xml;

use std::path::{Path, PathBuf};

use platter_core::{DeckId, TrackMemory};

pub use backup::{BACKUPS_KEPT, back_up};
pub use error::{LibraryError, StoreError};
pub use folder::{
    ImportReport, MAX_DEPTH, MAX_ENTRIES, MAX_RELOCATE_FILES, Reading, RelocateReport, TagReader,
    import_file, import_folder, relocate_missing,
};
pub use m3u::{
    M3uEntry, M3uError, M3uReading, MAX_M3U_BYTES, MAX_M3U_ENTRIES, read_m3u, read_pls, read_xspf,
    write_m3u,
};
pub use rekordbox::{
    CollectionError, MAX_COLLECTION_BYTES, MAX_COLLECTION_NODES, MAX_COLLECTION_TRACKS,
    SharedCollection, SharedNode, SharedTrack, read_collection,
};
pub use smart::{MAX_RULES, RangeField, Rule, TagField, parse_rules, rules_text};
pub use stems::{StemFolderError, find_stems};
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

impl Timestamp {
    /// Year, month, day, hour, minute and second in UTC. The date comes from the count of days
    /// with the algorithm `civil_from_days` of Howard Hinnant,
    /// <https://howardhinnant.github.io/date_algorithms.html>.
    pub fn utc(self) -> (i64, i64, i64, i64, i64, i64) {
        let days = self.0.div_euclid(86_400);
        let seconds = self.0.rem_euclid(86_400);
        // Days since 1 March of the year 0, so the leap day comes last in a year.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        (
            year,
            month,
            day,
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(i64);

/// A playlist or a folder of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(i64);

impl NodeId {
    /// The node with the identifier the library file gives it, as a command names it. The
    /// library answers [`LibraryError::NoSuchNode`] when there is none.
    pub fn new(raw: i64) -> Self {
        Self(raw)
    }
}

/// One place of a track in a playlist; a track in a playlist twice has two entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(i64);

/// A session of the application, which holds the tracks played in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(i64);

impl SessionId {
    /// The session with the identifier the library file gives it, as a command names it. A
    /// search of a session the library does not have finds nothing.
    pub fn new(raw: i64) -> Self {
        Self(raw)
    }
}

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
    /// The colour tag, 1 to [`MAX_COLOUR`], or 0 for none.
    pub colour: u8,
    /// The rating, 0 to [`MAX_RATING`].
    pub rating: u8,
    pub comment: Option<String>,
    /// The key the analysis found: the pitch class of the tonic, 0 for C up to 11 for B, plus
    /// 12 for a minor key.
    pub key: Option<u8>,
    /// The loudness the analysis measured, in LUFS.
    pub loudness: Option<f64>,
    /// A set of stems is linked to the track: `docs/behavior/stems.md`.
    pub stems: bool,
}

/// The files of the stems of a track, in the order of [`platter_core::Stem::ALL`].
pub type StemPaths = [PathBuf; platter_core::Stem::COUNT];

/// The highest number of a key, B minor.
pub const MAX_KEY: u8 = 23;

/// The colours a track can be tagged with, numbered from 1.
pub const MAX_COLOUR: u8 = 8;
/// The highest rating of a track.
pub const MAX_RATING: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Folder,
    Playlist,
    /// The tracks of the collection that pass its rules: [`Library::rules`].
    Smart,
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
    /// Tracks played in it that are still in the collection.
    pub played: usize,
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
    /// By Open Key number, then major before minor.
    Key,
    Imported,
    PlayCount,
    /// From one star to five; none last.
    Rating,
    /// From the first colour to the eighth; none last.
    Colour,
    /// The own order of a playlist; in a history, the order the tracks started playing; in
    /// the collection, the path.
    Order,
}

/// Where a search looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Source {
    /// The whole collection.
    #[default]
    Collection,
    /// A playlist, or a smart playlist.
    Playlist(NodeId),
    /// The tracks played in a session that are still in the collection, once per time they
    /// were played.
    History(SessionId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// Words that all have to be found in the title, artist, album or genre.
    pub text: String,
    pub source: Source,
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
            source: Source::Collection,
            column: Column::Order,
            descending: false,
            offset: 0,
            limit: MAX_PAGE,
        }
    }
}

/// One track found, with its entry when the search ran in a playlist. A history gives a track
/// once per time it was played.
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
    /// Imports several tracks as [`Library::import`] does, all of them or, on an error, none.
    fn import_all(
        &mut self,
        tracks: &[(PathBuf, TrackTags)],
        at: Timestamp,
    ) -> Result<Vec<TrackId>, LibraryError>;
    fn track(&self, id: TrackId) -> Result<Option<Track>, LibraryError>;
    fn track_at(&self, path: &Path) -> Result<Option<Track>, LibraryError>;
    /// Removes the track from the collection and from every playlist.
    fn remove_track(&mut self, id: TrackId) -> Result<(), LibraryError>;
    /// Marks the tracks whose file is gone as missing, and those whose file is back as not.
    fn check_files(&mut self) -> Result<CheckReport, LibraryError>;
    /// Tags a track with a colour, 1 to [`MAX_COLOUR`], or none with 0.
    fn set_colour(&mut self, track: TrackId, colour: u8) -> Result<(), LibraryError>;
    /// Rates a track, 0 to [`MAX_RATING`].
    fn set_rating(&mut self, track: TrackId, rating: u8) -> Result<(), LibraryError>;
    /// The comment of a track, cut as tags are; `None` or an empty one clears it.
    fn set_comment(&mut self, track: TrackId, comment: Option<&str>) -> Result<(), LibraryError>;
    /// The beatgrid and the cues kept for the track at `path`. A track without them, a path
    /// the collection does not hold and a kept text this version cannot read give `None`.
    fn memory_at(&self, path: &Path) -> Result<Option<TrackMemory>, LibraryError>;
    /// Whether a text is kept for the track at `path`, read or not.
    fn has_memory(&self, path: &Path) -> Result<bool, LibraryError>;
    /// Keeps the beatgrid and the cues of a track; its tempo becomes that of the grid when
    /// the grid has one.
    fn set_memory(&mut self, track: TrackId, memory: &TrackMemory) -> Result<(), LibraryError>;
    /// Keeps the key, 0 to [`MAX_KEY`], and the loudness the analysis found, and counts the
    /// track as analysed, even with nothing found.
    fn set_analysis(
        &mut self,
        track: TrackId,
        key: Option<u8>,
        loudness: Option<f64>,
    ) -> Result<(), LibraryError>;
    /// Up to `limit` tracks never analysed whose file was there at the last consistency
    /// check, the most recently imported first.
    fn unanalysed(&self, limit: usize) -> Result<Vec<Track>, LibraryError>;
    /// How many tracks [`Self::unanalysed`] would give without a limit.
    fn count_unanalysed(&self) -> Result<usize, LibraryError>;
    /// The tracks marked missing at the last consistency check, by path.
    fn missing(&self) -> Result<Vec<Track>, LibraryError>;
    /// Points a track at another file, keeping everything else.
    fn relocate(&mut self, id: TrackId, path: &Path) -> Result<(), LibraryError>;
    /// The files of the stems linked to a track, if it has a set.
    fn stems(&self, track: TrackId) -> Result<Option<StemPaths>, LibraryError>;
    /// Links a set of stems to a track, in place of the one before, or with `None` forgets
    /// the link. No file is touched.
    fn set_stems(&mut self, track: TrackId, stems: Option<&StemPaths>) -> Result<(), LibraryError>;

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
    /// The rules of a smart playlist, in the order they were given.
    fn rules(&self, smart: NodeId) -> Result<Vec<Rule>, LibraryError>;
    /// Replaces the rules of a smart playlist, up to [`MAX_RULES`]; on an error none change.
    fn set_rules(&mut self, smart: NodeId, rules: &[Rule]) -> Result<(), LibraryError>;

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
    /// The playlist used as the preparation list, if there is one.
    fn preparation(&self) -> Result<Option<NodeId>, LibraryError>;
    /// Makes `playlist` the preparation list, in place of the one before.
    fn set_preparation(&mut self, playlist: NodeId) -> Result<(), LibraryError>;
    /// The favourite playlists, each with its place, by place.
    fn favourites(&self) -> Result<Vec<(usize, NodeId)>, LibraryError>;
    /// Puts `playlist` in the favourite place `place`, taking it out of any other place and
    /// putting out the playlist that was there.
    fn set_favourite(&mut self, place: usize, playlist: NodeId) -> Result<(), LibraryError>;
    /// Empties a favourite place.
    fn clear_favourite(&mut self, place: usize) -> Result<(), LibraryError>;

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
    /// Forgets a session and what was played in it; the tracks and their play counts stay.
    fn delete_session(&mut self, session: SessionId) -> Result<(), LibraryError>;
    /// Sets the play count of a track to zero and forgets when it was last played.
    fn reset_played(&mut self, track: TrackId) -> Result<(), LibraryError>;

    fn search(&self, query: &Query) -> Result<Page, LibraryError>;
}
