//! The library in one SQLite file: `docs/decisions/0022-library-store.md`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use platter_core::DeckId;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Row, Transaction, params, params_from_iter,
};

use crate::text::{contains, fold, positive, tag};
use crate::{
    CheckReport, Column, Entry, EntryId, Hit, Library, LibraryError, MAX_NAME_CHARS, MAX_PAGE,
    Node, NodeId, NodeKind, Page, Played, Query, Session, SessionId, Timestamp, Track, TrackId,
    TrackTags,
};

/// Marks the file as a library of Platter: "PLTR".
const APPLICATION_ID: i64 = 0x504C_5452;
/// The version of the tables this code writes. A file at a higher one is refused.
const SCHEMA_VERSION: i64 = 1;
/// How long a call waits for a file another program is writing.
const BUSY_WAIT: Duration = Duration::from_secs(2);

const SCHEMA: &str = "
CREATE TABLE track (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT, artist TEXT, album TEXT, genre TEXT,
    title_key TEXT, artist_key TEXT, album_key TEXT, genre_key TEXT,
    seconds REAL, sample_rate INTEGER, bpm REAL,
    imported INTEGER NOT NULL,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    missing INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE node (
    id INTEGER PRIMARY KEY,
    parent INTEGER REFERENCES node(id) ON DELETE CASCADE,
    kind INTEGER NOT NULL,
    name TEXT NOT NULL
);
CREATE INDEX node_parent ON node(parent);
CREATE TABLE entry (
    id INTEGER PRIMARY KEY,
    playlist INTEGER NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    track INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
    position INTEGER NOT NULL
);
CREATE INDEX entry_playlist ON entry(playlist, position);
CREATE INDEX entry_track ON entry(track);
CREATE TABLE session (
    id INTEGER PRIMARY KEY,
    opened INTEGER NOT NULL
);
CREATE TABLE played (
    id INTEGER PRIMARY KEY,
    session INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    track INTEGER REFERENCES track(id) ON DELETE SET NULL,
    title TEXT, artist TEXT,
    deck INTEGER NOT NULL,
    started INTEGER NOT NULL,
    seconds REAL NOT NULL
);
CREATE INDEX played_session ON played(session);
";

const TRACK_COLUMNS: &str = "t.id, t.path, t.title, t.artist, t.album, t.genre, t.seconds, \
     t.sample_rate, t.bpm, t.imported, t.play_count, t.last_played, t.missing";

/// The library in one SQLite file.
pub struct SqliteLibrary {
    connection: Connection,
}

impl SqliteLibrary {
    /// Opens the library at `path`, creating it when the file does not exist or is empty. A
    /// file that is not a library, or one written by a newer version, is refused unchanged.
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(BUSY_WAIT)?;
        // Reading these writes nothing; a file that is not a database fails here.
        let application: i64 =
            connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let objects: i64 =
            connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;
        match (application, version) {
            (0, 0) if objects == 0 => create(&connection)?,
            (APPLICATION_ID, SCHEMA_VERSION) => {}
            (APPLICATION_ID, newer) if newer > SCHEMA_VERSION => {
                return Err(LibraryError::Newer { version: newer });
            }
            _ => return Err(LibraryError::NotALibrary),
        }
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(Self { connection })
    }

    fn node(&self, id: NodeId) -> Result<Node, LibraryError> {
        self.connection
            .query_row(
                "SELECT id, parent, kind, name FROM node WHERE id = ?1",
                [id.0],
                node_from,
            )
            .optional()?
            .ok_or(LibraryError::NoSuchNode)
    }

    fn playlist(&self, id: NodeId) -> Result<(), LibraryError> {
        match self.node(id)?.kind {
            NodeKind::Playlist => Ok(()),
            NodeKind::Folder => Err(LibraryError::WrongKind),
        }
    }

    fn has_track(&self, id: TrackId) -> Result<bool, LibraryError> {
        Ok(self
            .connection
            .query_row("SELECT 1 FROM track WHERE id = ?1", [id.0], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Rewrites the playlist with `order`, its entries in their new order.
    fn renumber(transaction: &Transaction<'_>, order: &[i64]) -> Result<(), LibraryError> {
        let mut update = transaction.prepare("UPDATE entry SET position = ?1 WHERE id = ?2")?;
        for (position, id) in order.iter().enumerate() {
            update.execute(params![position as i64, id])?;
        }
        Ok(())
    }

    fn entry_ids(
        transaction: &Transaction<'_>,
        playlist: NodeId,
    ) -> Result<Vec<i64>, LibraryError> {
        let mut select = transaction
            .prepare("SELECT id FROM entry WHERE playlist = ?1 ORDER BY position, id")?;
        let ids = select
            .query_map([playlist.0], |row| row.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        Ok(ids)
    }
}

fn create(connection: &Connection) -> Result<(), LibraryError> {
    connection.execute_batch(&format!(
        "BEGIN; {SCHEMA} PRAGMA application_id = {APPLICATION_ID}; \
         PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
    ))?;
    Ok(())
}

fn text_of(path: &Path) -> Result<&str, LibraryError> {
    path.to_str().ok_or(LibraryError::NotUnicode)
}

fn valid_name(name: &str) -> Result<(), LibraryError> {
    let length = name.chars().count();
    if (1..=MAX_NAME_CHARS).contains(&length) {
        Ok(())
    } else {
        Err(LibraryError::InvalidName)
    }
}

fn kind_code(kind: NodeKind) -> i64 {
    match kind {
        NodeKind::Folder => 0,
        NodeKind::Playlist => 1,
    }
}

fn node_from(row: &Row<'_>) -> rusqlite::Result<Node> {
    let kind: i64 = row.get(2)?;
    Ok(Node {
        id: NodeId(row.get(0)?),
        parent: row.get::<_, Option<i64>>(1)?.map(NodeId),
        kind: if kind == 0 {
            NodeKind::Folder
        } else {
            NodeKind::Playlist
        },
        name: row.get(3)?,
    })
}

/// A track from the columns of [`TRACK_COLUMNS`], starting at `first`.
fn track_from(row: &Row<'_>, first: usize) -> rusqlite::Result<Track> {
    let column = |offset: usize| first + offset;
    let path: String = row.get(column(1))?;
    let play_count: i64 = row.get(column(10))?;
    let missing: i64 = row.get(column(12))?;
    Ok(Track {
        id: TrackId(row.get(column(0))?),
        path: PathBuf::from(path),
        tags: TrackTags {
            title: row.get(column(2))?,
            artist: row.get(column(3))?,
            album: row.get(column(4))?,
            genre: row.get(column(5))?,
            seconds: positive(row.get(column(6))?),
            sample_rate: row
                .get::<_, Option<i64>>(column(7))?
                .and_then(|rate| u32::try_from(rate).ok()),
            bpm: positive(row.get(column(8))?),
        },
        imported: Timestamp(row.get(column(9))?),
        play_count: u32::try_from(play_count).unwrap_or(u32::MAX),
        last_played: row.get::<_, Option<i64>>(column(11))?.map(Timestamp),
        missing: missing != 0,
    })
}

fn sort_expression(column: Column, in_playlist: bool) -> &'static str {
    match column {
        Column::Title => "t.title_key",
        Column::Artist => "t.artist_key",
        Column::Album => "t.album_key",
        Column::Genre => "t.genre_key",
        Column::Tempo => "t.bpm",
        Column::Duration => "t.seconds",
        Column::Imported => "t.imported",
        Column::PlayCount => "t.play_count",
        Column::Order if in_playlist => "e.position",
        Column::Order => "t.path",
    }
}

/// Adds the track at `path`, or updates the tags of the one already there.
fn upsert(
    connection: &Connection,
    path: &Path,
    tags: &TrackTags,
    at: Timestamp,
) -> Result<TrackId, LibraryError> {
    let path = text_of(path)?;
    let title = tag(tags.title.as_deref());
    let artist = tag(tags.artist.as_deref());
    let album = tag(tags.album.as_deref());
    let genre = tag(tags.genre.as_deref());
    let key = |value: &Option<String>| value.as_deref().map(fold);
    let id = connection.query_row(
        "INSERT INTO track (path, title, artist, album, genre, title_key, artist_key, \
                 album_key, genre_key, seconds, sample_rate, bpm, imported) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
             ON CONFLICT (path) DO UPDATE SET title = excluded.title, artist = excluded.artist, \
                 album = excluded.album, genre = excluded.genre, title_key = excluded.title_key, \
                 artist_key = excluded.artist_key, album_key = excluded.album_key, \
                 genre_key = excluded.genre_key, seconds = excluded.seconds, \
                 sample_rate = excluded.sample_rate, bpm = excluded.bpm \
             RETURNING id",
        params![
            path,
            title,
            artist,
            album,
            genre,
            key(&title),
            key(&artist),
            key(&album),
            key(&genre),
            positive(tags.seconds),
            tags.sample_rate.filter(|rate| *rate > 0),
            positive(tags.bpm),
            at.0,
        ],
        |row| row.get(0),
    )?;
    Ok(TrackId(id))
}

impl Library for SqliteLibrary {
    fn import(
        &mut self,
        path: &Path,
        tags: &TrackTags,
        at: Timestamp,
    ) -> Result<TrackId, LibraryError> {
        upsert(&self.connection, path, tags, at)
    }

    fn import_all(
        &mut self,
        tracks: &[(PathBuf, TrackTags)],
        at: Timestamp,
    ) -> Result<Vec<TrackId>, LibraryError> {
        let transaction = self.connection.transaction()?;
        let ids = tracks
            .iter()
            .map(|(path, tags)| upsert(&transaction, path, tags, at))
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(ids)
    }

    fn track(&self, id: TrackId) -> Result<Option<Track>, LibraryError> {
        Ok(self
            .connection
            .query_row(
                &format!("SELECT {TRACK_COLUMNS} FROM track t WHERE t.id = ?1"),
                [id.0],
                |row| track_from(row, 0),
            )
            .optional()?)
    }

    fn track_at(&self, path: &Path) -> Result<Option<Track>, LibraryError> {
        Ok(self
            .connection
            .query_row(
                &format!("SELECT {TRACK_COLUMNS} FROM track t WHERE t.path = ?1"),
                [text_of(path)?],
                |row| track_from(row, 0),
            )
            .optional()?)
    }

    fn remove_track(&mut self, id: TrackId) -> Result<(), LibraryError> {
        let transaction = self.connection.transaction()?;
        let playlists: Vec<i64> = transaction
            .prepare("SELECT DISTINCT playlist FROM entry WHERE track = ?1")?
            .query_map([id.0], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        if transaction.execute("DELETE FROM track WHERE id = ?1", [id.0])? == 0 {
            return Err(LibraryError::NoSuchTrack);
        }
        for playlist in playlists {
            let order = Self::entry_ids(&transaction, NodeId(playlist))?;
            Self::renumber(&transaction, &order)?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn check_files(&mut self) -> Result<CheckReport, LibraryError> {
        let tracks: Vec<(i64, String, bool)> = self
            .connection
            .prepare("SELECT id, path, missing FROM track ORDER BY id")?
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0))
            })?
            .collect::<Result<_, _>>()?;
        let mut report = CheckReport::default();
        let transaction = self.connection.transaction()?;
        for (id, path, was_missing) in tracks {
            let missing = !Path::new(&path).is_file();
            if missing != was_missing {
                transaction.execute(
                    "UPDATE track SET missing = ?1 WHERE id = ?2",
                    params![missing, id],
                )?;
                let list = if missing {
                    &mut report.missing
                } else {
                    &mut report.found
                };
                list.push(TrackId(id));
            }
        }
        transaction.commit()?;
        Ok(report)
    }

    fn relocate(&mut self, id: TrackId, path: &Path) -> Result<(), LibraryError> {
        let text = text_of(path)?;
        if let Some(other) = self.track_at(path)? {
            return if other.id == id {
                Ok(())
            } else {
                Err(LibraryError::PathTaken)
            };
        }
        let missing = !path.is_file();
        let changed = self.connection.execute(
            "UPDATE track SET path = ?1, missing = ?2 WHERE id = ?3",
            params![text, missing, id.0],
        )?;
        if changed == 0 {
            return Err(LibraryError::NoSuchTrack);
        }
        Ok(())
    }

    fn create(
        &mut self,
        parent: Option<NodeId>,
        kind: NodeKind,
        name: &str,
    ) -> Result<NodeId, LibraryError> {
        valid_name(name)?;
        if let Some(parent) = parent
            && self.node(parent)?.kind != NodeKind::Folder
        {
            return Err(LibraryError::WrongKind);
        }
        self.connection.execute(
            "INSERT INTO node (parent, kind, name) VALUES (?1, ?2, ?3)",
            params![parent.map(|parent| parent.0), kind_code(kind), name],
        )?;
        Ok(NodeId(self.connection.last_insert_rowid()))
    }

    fn rename(&mut self, id: NodeId, name: &str) -> Result<(), LibraryError> {
        valid_name(name)?;
        if self.connection.execute(
            "UPDATE node SET name = ?1 WHERE id = ?2",
            params![name, id.0],
        )? == 0
        {
            return Err(LibraryError::NoSuchNode);
        }
        Ok(())
    }

    fn move_node(&mut self, id: NodeId, parent: Option<NodeId>) -> Result<(), LibraryError> {
        self.node(id)?;
        if let Some(target) = parent {
            if self.node(target)?.kind != NodeKind::Folder {
                return Err(LibraryError::WrongKind);
            }
            // Walk up from the target: meeting the node means it would go inside itself.
            let mut at = Some(target);
            while let Some(current) = at {
                if current == id {
                    return Err(LibraryError::Cycle);
                }
                at = self.node(current)?.parent;
            }
        }
        self.connection.execute(
            "UPDATE node SET parent = ?1 WHERE id = ?2",
            params![parent.map(|parent| parent.0), id.0],
        )?;
        Ok(())
    }

    fn delete_node(&mut self, id: NodeId) -> Result<(), LibraryError> {
        if self
            .connection
            .execute("DELETE FROM node WHERE id = ?1", [id.0])?
            == 0
        {
            return Err(LibraryError::NoSuchNode);
        }
        Ok(())
    }

    fn children(&self, parent: Option<NodeId>) -> Result<Vec<Node>, LibraryError> {
        if let Some(parent) = parent {
            self.node(parent)?;
        }
        let mut select = self.connection.prepare(
            "SELECT id, parent, kind, name FROM node WHERE parent IS ?1 ORDER BY name, id",
        )?;
        let mut nodes = select
            .query_map([parent.map(|parent| parent.0)], node_from)?
            .collect::<Result<Vec<_>, _>>()?;
        // SQLite orders English letters only; the order of a name is that of its folded form.
        nodes.sort_by(|a, b| fold(&a.name).cmp(&fold(&b.name)).then(a.id.cmp(&b.id)));
        Ok(nodes)
    }

    fn add(
        &mut self,
        playlist: NodeId,
        tracks: &[TrackId],
        at: Option<usize>,
    ) -> Result<(), LibraryError> {
        self.playlist(playlist)?;
        for track in tracks {
            if !self.has_track(*track)? {
                return Err(LibraryError::NoSuchTrack);
            }
        }
        let transaction = self.connection.transaction()?;
        let mut order = Self::entry_ids(&transaction, playlist)?;
        let at = at.unwrap_or(order.len()).min(order.len());
        let mut added = Vec::with_capacity(tracks.len());
        for track in tracks {
            transaction.execute(
                "INSERT INTO entry (playlist, track, position) VALUES (?1, ?2, 0)",
                params![playlist.0, track.0],
            )?;
            added.push(transaction.last_insert_rowid());
        }
        order.splice(at..at, added);
        Self::renumber(&transaction, &order)?;
        transaction.commit()?;
        Ok(())
    }

    fn entries(&self, playlist: NodeId) -> Result<Vec<Entry>, LibraryError> {
        self.playlist(playlist)?;
        let mut select = self
            .connection
            .prepare("SELECT id, track FROM entry WHERE playlist = ?1 ORDER BY position, id")?;
        let entries = select
            .query_map([playlist.0], |row| {
                Ok(Entry {
                    id: EntryId(row.get(0)?),
                    track: TrackId(row.get(1)?),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(entries)
    }

    fn remove_entries(
        &mut self,
        playlist: NodeId,
        entries: &[EntryId],
    ) -> Result<(), LibraryError> {
        self.playlist(playlist)?;
        let transaction = self.connection.transaction()?;
        for entry in entries {
            transaction.execute(
                "DELETE FROM entry WHERE id = ?1 AND playlist = ?2",
                params![entry.0, playlist.0],
            )?;
        }
        let order = Self::entry_ids(&transaction, playlist)?;
        Self::renumber(&transaction, &order)?;
        transaction.commit()?;
        Ok(())
    }

    fn move_entry(
        &mut self,
        playlist: NodeId,
        entry: EntryId,
        to: usize,
    ) -> Result<(), LibraryError> {
        self.playlist(playlist)?;
        let transaction = self.connection.transaction()?;
        let mut order = Self::entry_ids(&transaction, playlist)?;
        let Some(from) = order.iter().position(|id| *id == entry.0) else {
            return Err(LibraryError::NoSuchEntry);
        };
        let moved = order.remove(from);
        order.insert(to.min(order.len()), moved);
        Self::renumber(&transaction, &order)?;
        transaction.commit()?;
        Ok(())
    }

    fn remove_repeats(&mut self, playlist: NodeId) -> Result<(), LibraryError> {
        self.playlist(playlist)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM entry WHERE playlist = ?1 AND id NOT IN ( \
                 SELECT id FROM ( \
                     SELECT id, row_number() OVER ( \
                         PARTITION BY track ORDER BY position, id) AS rank \
                     FROM entry WHERE playlist = ?1) \
                 WHERE rank = 1)",
            [playlist.0],
        )?;
        let order = Self::entry_ids(&transaction, playlist)?;
        Self::renumber(&transaction, &order)?;
        transaction.commit()?;
        Ok(())
    }

    fn clear(&mut self, playlist: NodeId) -> Result<(), LibraryError> {
        self.playlist(playlist)?;
        self.connection
            .execute("DELETE FROM entry WHERE playlist = ?1", [playlist.0])?;
        Ok(())
    }

    fn open_session(&mut self, at: Timestamp) -> Result<SessionId, LibraryError> {
        self.connection
            .execute("INSERT INTO session (opened) VALUES (?1)", [at.0])?;
        Ok(SessionId(self.connection.last_insert_rowid()))
    }

    fn sessions(&self) -> Result<Vec<Session>, LibraryError> {
        let mut select = self
            .connection
            .prepare("SELECT id, opened FROM session ORDER BY opened DESC, id DESC")?;
        let sessions = select
            .query_map([], |row| {
                Ok(Session {
                    id: SessionId(row.get(0)?),
                    opened: Timestamp(row.get(1)?),
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(sessions)
    }

    fn record_played(
        &mut self,
        session: SessionId,
        track: TrackId,
        deck: DeckId,
        started: Timestamp,
        seconds: f64,
    ) -> Result<(), LibraryError> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        let transaction = self.connection.transaction()?;
        let known = transaction
            .query_row("SELECT 1 FROM session WHERE id = ?1", [session.0], |_| {
                Ok(())
            })
            .optional()?;
        if known.is_none() {
            return Err(LibraryError::NoSuchSession);
        }
        let inserted = transaction.execute(
            "INSERT INTO played (session, track, title, artist, deck, started, seconds) \
             SELECT ?1, id, title, artist, ?3, ?4, ?5 FROM track WHERE id = ?2",
            params![session.0, track.0, deck.index() as i64, started.0, seconds],
        )?;
        if inserted == 0 {
            return Err(LibraryError::NoSuchTrack);
        }
        transaction.execute(
            "UPDATE track SET play_count = play_count + 1, last_played = ?1 WHERE id = ?2",
            params![started.0, track.0],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn history(&self, session: SessionId) -> Result<Vec<Played>, LibraryError> {
        let mut select = self.connection.prepare(
            "SELECT track, title, artist, deck, started, seconds FROM played \
             WHERE session = ?1 ORDER BY started, id",
        )?;
        let played = select
            .query_map([session.0], |row| {
                let deck: i64 = row.get(3)?;
                let seconds: f64 = row.get(5)?;
                Ok(Played {
                    track: row.get::<_, Option<i64>>(0)?.map(TrackId),
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    deck: usize::try_from(deck)
                        .ok()
                        .and_then(|index| DeckId::ALL.get(index).copied())
                        .unwrap_or(DeckId::A),
                    started: Timestamp(row.get(4)?),
                    seconds: if seconds.is_finite() { seconds } else { 0.0 },
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(played)
    }

    fn reset_played(&mut self, track: TrackId) -> Result<(), LibraryError> {
        if self.connection.execute(
            "UPDATE track SET play_count = 0, last_played = NULL WHERE id = ?1",
            [track.0],
        )? == 0
        {
            return Err(LibraryError::NoSuchTrack);
        }
        Ok(())
    }

    fn search(&self, query: &Query) -> Result<Page, LibraryError> {
        if let Some(playlist) = query.playlist {
            self.playlist(playlist)?;
        }
        let in_playlist = query.playlist.is_some();
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        let mut from = String::from("FROM track t");
        if let Some(playlist) = query.playlist {
            from.push_str(" JOIN entry e ON e.track = t.id AND e.playlist = ?1");
            values.push(playlist.0.into());
        }
        let mut filter = String::from(" WHERE 1");
        for word in query.text.split_whitespace() {
            values.push(contains(word).into());
            let n = values.len();
            filter.push_str(&format!(
                " AND (t.title_key LIKE ?{n} ESCAPE '\\' OR t.artist_key LIKE ?{n} ESCAPE '\\' \
                 OR t.album_key LIKE ?{n} ESCAPE '\\' OR t.genre_key LIKE ?{n} ESCAPE '\\')"
            ));
        }
        let total: i64 = self.connection.query_row(
            &format!("SELECT count(*) {from}{filter}"),
            params_from_iter(values.iter()),
            |row| row.get(0),
        )?;
        let sort = sort_expression(query.column, in_playlist);
        let direction = if query.descending { "DESC" } else { "ASC" };
        let entry = if in_playlist { "e.id" } else { "NULL" };
        let limit = query.limit.min(MAX_PAGE) as i64;
        let offset = i64::try_from(query.offset).unwrap_or(i64::MAX);
        let sql = format!(
            "SELECT {entry}, {TRACK_COLUMNS} {from}{filter} \
             ORDER BY ({sort} IS NULL), {sort} {direction}, t.path LIMIT {limit} OFFSET {offset}"
        );
        let mut select = self.connection.prepare(&sql)?;
        let hits = select
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(Hit {
                    entry: row.get::<_, Option<i64>>(0)?.map(EntryId),
                    track: track_from(row, 1)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(Page {
            hits,
            total: usize::try_from(total).unwrap_or(0),
        })
    }
}
