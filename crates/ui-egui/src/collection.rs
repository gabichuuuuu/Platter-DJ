//! The collection behind the browser: the library runs on a worker thread of its own, and
//! the browser sends it what to read, import or change and takes in the answers.
//!
//! Every call of the library may touch the disk, and an import may take minutes, so none
//! runs on the interface thread. Imports and changes to the playlists run in the order they
//! were sent; of the reads waiting, only the last one runs, after them. Behaviour notes:
//! `docs/behavior/browser.md`, *The collection*, *Playlists*, *Played state* and *History*.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use platter_core::TrackMemory;
use platter_decode::{CoverPicture, MAX_COVER_IMPORT_BYTES, TagEdit, probe_file, write_tags};
use platter_library::{
    Column, EntryId, Library, LibraryError, M3uEntry, MAX_COLLECTION_BYTES, MAX_M3U_BYTES,
    MAX_NAME_CHARS, MAX_PAGE, NodeId, NodeKind, Page, Query, Reading, Rule, Session, SessionId,
    SharedNode, Source, TagReader, Timestamp, Track, TrackId, TrackTags, import_file,
    import_folder, read_collection, read_m3u, read_pls, read_xspf, relocate_missing, rules_text,
    write_m3u,
};

use crate::command::{DetailField, DetailFields, SortColumn, TrackDetails};
use crate::files::is_audio;
use crate::model::{
    CollectionView, FileRow, ImportOutcome, MaintenanceOutcome, PlaylistFileOutcome, PlaylistRow,
    RowKind, SessionRow, TrackRow,
};
use crate::played::Played;

/// Folders and playlists the tree shows; the rest of a larger library is left out.
const MAX_TREE_ITEMS: usize = 1000;
/// Levels of folders the tree shows.
const MAX_TREE_DEPTH: usize = 32;
/// Earlier sessions the tree shows, the latest ones.
pub const MAX_SESSIONS: usize = 100;

/// What the list asks of the library: the whole collection or one playlist, a search text,
/// where an empty one matches every track, and the column it is sorted by with whether the
/// order is turned round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Lookup {
    source: Source,
    text: String,
    sort: Option<(SortColumn, bool)>,
    /// Changes sent before the lookup. An answer read before a change is for another lookup,
    /// so it neither shows rows the change has altered nor ends the wait for the new ones.
    changes: u64,
}

impl Lookup {
    fn query(&self) -> Query {
        let (column, descending) = match self.sort {
            Some((column, descending)) => (
                match column {
                    SortColumn::Title => Column::Title,
                    SortColumn::Artist => Column::Artist,
                    SortColumn::Album => Column::Album,
                    SortColumn::Bpm => Column::Tempo,
                    SortColumn::Time => Column::Duration,
                    SortColumn::Key => Column::Key,
                    SortColumn::Genre => Column::Genre,
                    SortColumn::Rating => Column::Rating,
                    SortColumn::Colour => Column::Colour,
                },
                descending,
            ),
            None => (Column::Order, false),
        };
        Query {
            text: self.text.clone(),
            source: self.source,
            column,
            descending,
            ..Query::default()
        }
    }
}

enum Request {
    Read(Lookup),
    Write(Write),
}

/// Work that changes the library.
enum Write {
    Import(PathBuf),
    Change(Change),
    Maintain(Job),
    /// Writes a playlist, named as given, as an M3U file into a folder.
    Export(NodeId, String, PathBuf),
    /// Writes the history of a session, named as given, as an M3U file into a folder.
    ExportHistory(SessionId, String, PathBuf),
    /// Makes a playlist of each playlist file of a folder, in a folder of the tree.
    ImportPlaylists(Option<NodeId>, PathBuf),
    /// Imports each collection file of a folder: its tracks and its tree of playlists.
    ImportCollections(PathBuf),
}

/// Collection files of a folder one import reads, by name.
const MAX_COLLECTION_FILES: usize = 10;

/// Files with the same name an export tries before it gives up.
const MAX_EXPORT_NAMES: usize = 100;
/// Playlist files of a folder one import reads, by name.
const MAX_IMPORTED_FILES: usize = 100;

/// A task of the maintenance of the collection.
enum Job {
    /// Only says which tracks are missing.
    List,
    Check,
    Relocate(PathBuf),
    RemoveMissing,
}

/// Missing tracks the maintenance lists; the count says how many there are beyond them.
const MAX_MISSING_LISTED: usize = 1000;

/// A change to the playlists and their folders.
enum Change {
    Create(Option<NodeId>, NodeKind, String),
    Rename(NodeId, String),
    Move(NodeId, Option<NodeId>),
    Add(NodeId, PathBuf),
    /// A playlist becomes the preparation list.
    SetPreparation(NodeId),
    /// A playlist goes into a favourite place.
    SetFavourite(usize, NodeId),
    /// A favourite place is emptied.
    ClearFavourite(usize),
    /// A track goes into the preparation list: at its end, or after the entry of the track
    /// at the path, loaded last into a deck.
    Prepare(TrackId, Place),
    /// A track has counted as played in this session.
    Played(Played),
    /// The play count of a track goes back to zero.
    ResetPlayed(TrackId),
    /// A track leaves the collection and every playlist.
    DeleteTrack(TrackId),
    SetColour(TrackId, u8),
    /// Fields of the dialog of the details, for each of the tracks.
    Details(Vec<TrackId>, TrackDetails, DetailFields, TagWriting),
    /// The picture file to import as the cover of each of the tracks, or `None` to delete
    /// their pictures.
    Cover(Vec<TrackId>, Option<PathBuf>),
    SetRating(TrackId, u8),
    /// What a deck learned of the track at the path: its grid and cues, the key and the
    /// loudness its analysis found. The track is taken into the collection if it is not in.
    Keep(PathBuf, Box<Kept>, TagWriting),
    Delete(NodeId),
    Clear(NodeId),
    RemoveRepeats(NodeId),
    Remove(NodeId, EntryId),
    /// A playlist, named as given, of the tracks played in a session.
    HistoryToPlaylist(SessionId, String),
    /// A session of an earlier run is forgotten.
    HistoryDelete(SessionId),
    SetRules(NodeId, Vec<Rule>),
    /// A playlist, named as the smart playlist, of the tracks it holds.
    SmartToPlaylist(NodeId),
    /// A copy of a smart playlist, with its rules, in the same folder.
    DuplicateSmart(NodeId),
}

/// What Platter writes into the files: `docs/behavior/tag-writing.md`, *When Platter writes*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TagWriting {
    /// The tags of the details dialog, the tempo, the key and Platter's own field.
    #[default]
    Everything,
    /// Platter's own field alone: the grid and the cues.
    PlatterOnly,
    Nothing,
}

impl TagWriting {
    pub const ALL: [Self; 3] = [Self::Everything, Self::PlatterOnly, Self::Nothing];

    pub fn name(self) -> &'static str {
        match self {
            Self::Everything => "everything",
            Self::PlatterOnly => "platter",
            Self::Nothing => "nothing",
        }
    }
}

pub const TAG_WRITINGS: &[&str] = &["everything", "platter", "nothing"];

/// Writes into the file at `path` what `kept` holds, as far as `mode` lets it. `Err` says why
/// the file was left as it was.
pub fn write_learned(path: &Path, kept: &Kept, mode: TagWriting) -> Result<(), String> {
    let everything = mode == TagWriting::Everything;
    let mut edit = TagEdit::default();
    if mode != TagWriting::Nothing
        && let Some(memory) = kept.memory
    {
        edit.platter = Some(Some(memory.to_text()));
        if everything {
            edit.bpm = memory.grid.bpm().map(|bpm| Some(bpm.value()));
        }
    }
    if everything && let Some((Some(key), _)) = kept.analysis {
        edit.key = Some(crate::keys::id3_text(key));
    }
    if edit == TagEdit::default() {
        return Ok(());
    }
    write_tags(path, &edit).map_err(|error| format!("{}: {error}", path.display()))
}

/// Reads the picture file at `path` to import as a cover: at most
/// [`MAX_COVER_IMPORT_BYTES`], and one Platter's reader shows. `Err` says why not.
fn read_picture(path: &Path) -> Result<CoverPicture, String> {
    let why = |error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
    let file = std::fs::File::open(path).map_err(|error| why(&error))?;
    // One byte past the bound tells a picture too large from one that just fits; a file
    // that grows meanwhile is read no further.
    let limit = u64::try_from(MAX_COVER_IMPORT_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut data = Vec::new();
    file.take(limit)
        .read_to_end(&mut data)
        .map_err(|error| why(&error))?;
    CoverPicture::from_bytes(data).map_err(|error| why(&error))
}

/// What a deck learned of a track, for the collection to keep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Kept {
    /// The grid and the cues, when they changed.
    pub memory: Option<TrackMemory>,
    /// The key, 0 to 23, and the loudness in LUFS the analysis found, once it finished.
    pub analysis: Option<(Option<u8>, Option<f64>)>,
}

enum Reply {
    Unavailable,
    /// Files kept what they had: why, for each.
    NotWritten(Vec<String>),
    /// Covers were written into files; why the files named kept theirs.
    Covers(Vec<String>),
    /// The session of this run is open since that moment; sent once, before any page.
    Session(SessionId, Timestamp),
    /// A playlist was created; sent before the tree that follows the change.
    Created(NodeId),
    /// A playlist was deleted; sent before the tree and the page that follow the change.
    Deleted(NodeId),
    /// The history of a session was forgotten.
    HistoryDeleted(SessionId),
    /// The tracks found for a lookup.
    /// The rows of a lookup, with the tracks of the preparation list.
    Page(Lookup, Page, HashSet<TrackId>),
    Imported(ImportOutcome),
    /// The paths of the missing tracks, the first ones, and how many there are in all.
    Missing(Vec<String>, usize),
    Maintained(MaintenanceOutcome),
    PlaylistFile(PlaylistFileOutcome),
    /// The folders and playlists of the library, in the order of the tree, and the sessions
    /// it shows: the one of this run and the earlier ones where tracks were played.
    Tree(Vec<PlaylistRow>, Vec<Session>),
}

/// Where a track goes in the preparation list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    End,
    /// After the last entry of the track loaded last, given by its path; at the start when it
    /// is not in the list or no track was loaded.
    Next(Option<PathBuf>),
}

/// Reads the tags of a file the way the file list does: from its header, without decoding.
struct HeaderReader;

/// What the window and the worker share about the import that runs: how many files it has
/// looked at, and whether the user asked it to stop.
#[derive(Clone, Default)]
struct ImportWatch {
    looked: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

/// The reader of an import the window watches.
struct Watched<'a> {
    watch: &'a ImportWatch,
}

impl TagReader for Watched<'_> {
    fn read(&mut self, path: &Path) -> Reading {
        HeaderReader.read(path)
    }

    fn keep_going(&mut self, looked: usize) -> bool {
        self.watch.looked.store(looked, Ordering::Relaxed);
        !self.watch.stop.load(Ordering::Relaxed)
    }
}

impl TagReader for HeaderReader {
    fn read(&mut self, path: &Path) -> Reading {
        if !is_audio(path) {
            return Reading::NotAudio;
        }
        match probe_file(path) {
            Ok(info) => Reading::Tags(TrackTags {
                title: info.tags.title,
                artist: info.tags.artist,
                album: info.tags.album,
                genre: info.tags.genre,
                seconds: info.seconds,
                sample_rate: Some(info.sample_rate.hz()),
                bpm: info.tags.bpm,
            }),
            Err(_) => Reading::Unreadable,
        }
    }
}

fn now() -> Timestamp {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    Timestamp(i64::try_from(seconds).unwrap_or(i64::MAX))
}

/// The folders and playlists of the library, each level by name, a folder followed by what
/// it holds.
/// The tracks in the preparation list, which the rows mark.
fn prepared(library: &dyn Library) -> Result<HashSet<TrackId>, LibraryError> {
    Ok(match library.preparation()? {
        Some(list) => library
            .entries(list)?
            .into_iter()
            .map(|entry| entry.track)
            .collect(),
        None => HashSet::new(),
    })
}

fn tree(library: &dyn Library) -> Result<Vec<PlaylistRow>, LibraryError> {
    let preparation = library.preparation()?;
    let favourites = library.favourites()?;
    let mut rows = Vec::new();
    // Each level goes on the stack the other way round, so its first item comes off first.
    let mut stack: Vec<_> = library
        .children(None)?
        .into_iter()
        .rev()
        .map(|node| (node, 0))
        .collect();
    while let Some((node, depth)) = stack.pop() {
        if rows.len() == MAX_TREE_ITEMS {
            break;
        }
        let folder = node.kind == NodeKind::Folder;
        let entries = match node.kind {
            NodeKind::Folder => {
                if depth + 1 < MAX_TREE_DEPTH {
                    let children = library.children(Some(node.id))?;
                    stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
                }
                None
            }
            NodeKind::Playlist => Some(library.entries(node.id)?.len()),
            NodeKind::Smart => Some(smart_tracks(library, node.id, 0)?.total),
        };
        rows.push(PlaylistRow {
            id: node.id,
            name: node.name,
            depth,
            folder,
            smart: node.kind == NodeKind::Smart,
            rules: if node.kind == NodeKind::Smart {
                rules_text(&library.rules(node.id)?)
            } else {
                String::new()
            },
            entries,
            current: false,
            preparation: preparation == Some(node.id),
            favourite: favourites
                .iter()
                .find(|(_, playlist)| *playlist == node.id)
                .map(|(place, _)| *place),
        });
    }
    Ok(rows)
}

/// A page of the tracks a smart playlist holds now, by path, with how many it holds in all.
fn smart_tracks(library: &dyn Library, smart: NodeId, limit: usize) -> Result<Page, LibraryError> {
    library.search(&Query {
        source: Source::Playlist(smart),
        limit,
        ..Query::default()
    })
}

/// The tree as the browser shows it, or [`Reply::Unavailable`] when the library cannot be read.
/// `session` is the one of this run, listed even before anything is played in it.
fn tree_reply(library: &dyn Library, session: Option<SessionId>) -> Reply {
    let sessions = library.sessions().map(|sessions| {
        sessions
            .into_iter()
            .filter(|listed| listed.played > 0 || Some(listed.id) == session)
            .take(MAX_SESSIONS + 1)
            .collect()
    });
    match (tree(library), sessions) {
        (Ok(rows), Ok(sessions)) => Reply::Tree(rows, sessions),
        _ => Reply::Unavailable,
    }
}

/// Sends the answer to a lookup. A playlist that is gone is answered empty, and the tree is
/// sent again without it. Returns whether the browser is still there.
fn answer(
    library: &dyn Library,
    session: Option<SessionId>,
    lookup: Lookup,
    replies: &Sender<Reply>,
) -> bool {
    let reply = match library.search(&lookup.query()) {
        Ok(page) => match prepared(library) {
            Ok(prepared) => Reply::Page(lookup, page, prepared),
            Err(_) => Reply::Unavailable,
        },
        Err(LibraryError::NoSuchNode | LibraryError::WrongKind) => {
            let empty = Page {
                hits: Vec::new(),
                total: 0,
            };
            if replies
                .send(Reply::Page(lookup, empty, HashSet::new()))
                .is_err()
            {
                return false;
            }
            tree_reply(library, session)
        }
        Err(_) => Reply::Unavailable,
    };
    replies.send(reply).is_ok()
}

/// Makes a change and says what the browser has to know of it beyond the new tree. Played
/// tracks go to `session`; without one they are not recorded.
fn apply(
    library: &mut dyn Library,
    session: Option<SessionId>,
    change: Change,
) -> Result<Option<Reply>, LibraryError> {
    match change {
        Change::Played(played) => {
            if let Some(session) = session
                && let Some(track) = import_file(library, &played.path, &mut HeaderReader, now())?
            {
                library.record_played(
                    session,
                    track,
                    played.deck,
                    Timestamp(played.started),
                    played.seconds,
                )?;
            }
            Ok(None)
        }
        Change::ResetPlayed(track) => library.reset_played(track).map(|()| None),
        Change::DeleteTrack(track) => library.remove_track(track).map(|()| None),
        Change::SetColour(track, colour) => library.set_colour(track, colour).map(|()| None),
        Change::Details(tracks, details, fields, mode) => {
            let mut unwritten = Vec::new();
            for id in tracks {
                // A track deleted since the dialog opened is left out.
                let Some(track) = library.track(id)? else {
                    continue;
                };
                let text = |field: DetailField, value: &String| {
                    fields.contains(field).then(|| value.clone())
                };
                let edit = TagEdit {
                    title: text(DetailField::Title, &details.title),
                    artist: text(DetailField::Artist, &details.artist),
                    album: text(DetailField::Album, &details.album),
                    genre: text(DetailField::Genre, &details.genre),
                    comment: text(DetailField::Comment, &details.comment),
                    ..TagEdit::default()
                };
                if mode == TagWriting::Everything
                    && edit != TagEdit::default()
                    && let Err(error) = write_tags(&track.path, &edit)
                {
                    unwritten.push(format!("{}: {error}", track.path.display()));
                }
                // The fields the save does not name stay as the track has them.
                let mut tags: TrackTags = track.tags;
                for (value, slot) in [
                    (edit.title, &mut tags.title),
                    (edit.artist, &mut tags.artist),
                    (edit.album, &mut tags.album),
                    (edit.genre, &mut tags.genre),
                ] {
                    if value.is_some() {
                        *slot = value;
                    }
                }
                library.import(&track.path, &tags, track.imported)?;
                if fields.contains(DetailField::Comment) {
                    library.set_comment(id, Some(&details.comment))?;
                }
                if fields.contains(DetailField::Rating) {
                    library.set_rating(id, details.rating)?;
                }
            }
            Ok((!unwritten.is_empty()).then_some(Reply::NotWritten(unwritten)))
        }
        Change::Cover(tracks, picture) => {
            let cover = match picture {
                Some(path) => match read_picture(&path) {
                    Ok(picture) => Some(picture),
                    // Nothing is written when the picture cannot be: the log says why.
                    Err(why) => return Ok(Some(Reply::NotWritten(vec![why]))),
                },
                None => None,
            };
            let edit = TagEdit {
                cover: Some(cover),
                ..TagEdit::default()
            };
            let mut unwritten = Vec::new();
            for id in tracks {
                // A track deleted since it was selected is left out.
                let Some(track) = library.track(id)? else {
                    continue;
                };
                if let Err(error) = write_tags(&track.path, &edit) {
                    unwritten.push(format!("{}: {error}", track.path.display()));
                }
            }
            Ok(Some(Reply::Covers(unwritten)))
        }
        Change::SetRating(track, rating) => library.set_rating(track, rating).map(|()| None),
        Change::Keep(path, kept, mode) => {
            let Some(track) = import_file(library, &path, &mut HeaderReader, now())? else {
                return Ok(None);
            };
            if let Some(memory) = kept.memory {
                library.set_memory(track, &memory)?;
            }
            if let Some((key, loudness)) = kept.analysis {
                library.set_analysis(track, key, loudness)?;
            }
            // The collection keeps the change even when the file cannot take it.
            Ok(write_learned(&path, &kept, mode)
                .err()
                .map(|why| Reply::NotWritten(vec![why])))
        }
        Change::SetPreparation(playlist) => library.set_preparation(playlist).map(|()| None),
        Change::SetFavourite(place, playlist) => {
            library.set_favourite(place, playlist).map(|()| None)
        }
        Change::ClearFavourite(place) => library.clear_favourite(place).map(|()| None),
        Change::Prepare(track, place) => {
            let Some(list) = library.preparation()? else {
                return Ok(None);
            };
            let at = match place {
                Place::End => None,
                Place::Next(loaded) => {
                    let loaded = match loaded {
                        Some(path) => library.track_at(&path)?.map(|track| track.id),
                        None => None,
                    };
                    let entries = library.entries(list)?;
                    let after = loaded
                        .and_then(|loaded| entries.iter().rposition(|entry| entry.track == loaded));
                    Some(after.map_or(0, |index| index + 1))
                }
            };
            library.add(list, &[track], at).map(|()| None)
        }
        Change::Create(folder, kind, name) => {
            let created = library.create(folder, kind, &name)?;
            Ok((kind == NodeKind::Playlist).then_some(Reply::Created(created)))
        }
        Change::Rename(item, name) => library.rename(item, &name).map(|()| None),
        Change::Move(item, folder) => library.move_node(item, folder).map(|()| None),
        Change::Add(playlist, path) => {
            // The playlist first: a file is not taken into the collection for a playlist
            // that is not there.
            library.entries(playlist)?;
            if let Some(track) = import_file(library, &path, &mut HeaderReader, now())? {
                library.add(playlist, &[track], None)?;
            }
            Ok(None)
        }
        Change::Delete(playlist) => {
            // The library deletes a folder with everything in it, without a way back; this
            // change deletes playlists and smart playlists only: a folder has neither entries
            // nor rules, and both answer `WrongKind` for it.
            if library.entries(playlist).is_err() {
                library.rules(playlist)?;
            }
            library.delete_node(playlist)?;
            Ok(Some(Reply::Deleted(playlist)))
        }
        Change::Clear(playlist) => library.clear(playlist).map(|()| None),
        Change::HistoryToPlaylist(played_in, name) => {
            let tracks: Vec<TrackId> = library
                .history(played_in)?
                .into_iter()
                .filter_map(|played| played.track)
                .collect();
            let playlist = library.create(None, NodeKind::Playlist, &name)?;
            library.add(playlist, &tracks, None).map(|()| None)
        }
        Change::SetRules(smart, rules) => library.set_rules(smart, &rules).map(|()| None),
        Change::SmartToPlaylist(smart) => {
            let (parent, name) = place_of(library, smart)?;
            let tracks: Vec<TrackId> = smart_tracks(library, smart, MAX_PAGE)?
                .hits
                .into_iter()
                .map(|hit| hit.track.id)
                .collect();
            let playlist = library.create(parent, NodeKind::Playlist, &name)?;
            library.add(playlist, &tracks, None).map(|()| None)
        }
        Change::DuplicateSmart(smart) => {
            let rules = library.rules(smart)?;
            let (parent, name) = place_of(library, smart)?;
            let copy = library.create(parent, NodeKind::Smart, &name)?;
            library.set_rules(copy, &rules).map(|()| None)
        }
        // The history of this run is still being written.
        Change::HistoryDelete(gone) if Some(gone) == session => Ok(None),
        Change::HistoryDelete(gone) => library
            .delete_session(gone)
            .map(|()| Some(Reply::HistoryDeleted(gone))),
        Change::RemoveRepeats(playlist) => library.remove_repeats(playlist).map(|()| None),
        Change::Remove(playlist, entry) => {
            library.remove_entries(playlist, &[entry]).map(|()| None)
        }
    }
}

/// The folder a node is in and its name, found by walking the tree from its top.
fn place_of(library: &dyn Library, node: NodeId) -> Result<(Option<NodeId>, String), LibraryError> {
    let mut folders = vec![(None, 0)];
    while let Some((parent, depth)) = folders.pop() {
        for child in library.children(parent)? {
            if child.id == node {
                return Ok((parent, child.name));
            }
            if child.kind == NodeKind::Folder && depth + 1 < MAX_TREE_DEPTH {
                folders.push((Some(child.id), depth + 1));
            }
        }
    }
    Err(LibraryError::NoSuchNode)
}

/// Serves the requests of the browser until it goes away.
fn serve(
    library: &mut dyn Library,
    requests: &Receiver<Request>,
    replies: &Sender<Reply>,
    watch: &ImportWatch,
) {
    // The session of this run, for the tracks played in it.
    let opened = now();
    let session = library.open_session(opened).ok();
    if let Some(session) = session
        && replies.send(Reply::Session(session, opened)).is_err()
    {
        return;
    }
    if replies.send(tree_reply(library, session)).is_err() {
        return;
    }
    if session.is_none() && replies.send(Reply::Unavailable).is_err() {
        return;
    }
    let mut lookup = Lookup::default();
    while let Ok(first) = requests.recv() {
        // Of the searches waiting, typed while the library was busy, only the last one
        // matters; imports and changes all run, in order.
        let mut writes = Vec::new();
        for request in std::iter::once(first).chain(requests.try_iter()) {
            match request {
                Request::Read(newer) => lookup = newer,
                Request::Write(write) => writes.push(write),
            }
        }
        let (mut imported, mut changed) = (false, false);
        for write in writes {
            let reply = match write {
                Write::Import(folder) => {
                    imported = true;
                    let mut reader = Watched { watch };
                    let outcome = match import_folder(library, &folder, &mut reader, now()) {
                        Ok(report) => ImportOutcome::Done {
                            added: report.added.len(),
                            known: report.known,
                            unreadable: report.unreadable,
                            stopped: report.stopped,
                        },
                        Err(_) => ImportOutcome::Failed,
                    };
                    Reply::Imported(outcome)
                }
                Write::Export(playlist, name, folder) => {
                    let outcome = export_playlist(library, playlist, &name, &folder)
                        .unwrap_or(PlaylistFileOutcome::Failed);
                    Reply::PlaylistFile(outcome)
                }
                Write::ExportHistory(played_in, name, folder) => {
                    let tracks = library.history(played_in).map(|history| {
                        history
                            .into_iter()
                            .filter_map(|played| played.track)
                            .collect::<Vec<_>>()
                    });
                    let outcome = tracks
                        .and_then(|tracks| export_tracks(library, &tracks, &name, &folder))
                        .unwrap_or(PlaylistFileOutcome::Failed);
                    Reply::PlaylistFile(outcome)
                }
                Write::ImportPlaylists(parent, folder) => {
                    changed = true;
                    imported = true;
                    let outcome = import_playlists(library, parent, &folder)
                        .unwrap_or(PlaylistFileOutcome::Failed);
                    Reply::PlaylistFile(outcome)
                }
                Write::ImportCollections(folder) => {
                    changed = true;
                    imported = true;
                    let outcome =
                        import_collections(library, &folder).unwrap_or(PlaylistFileOutcome::Failed);
                    Reply::PlaylistFile(outcome)
                }
                Write::Maintain(job) => {
                    let outcome = match job {
                        Job::List => None,
                        Job::Check => Some(match library.check_files() {
                            Ok(report) => MaintenanceOutcome::Checked {
                                missing: report.missing.len(),
                                found: report.found.len(),
                            },
                            Err(_) => MaintenanceOutcome::Failed,
                        }),
                        Job::Relocate(folder) => Some(match relocate_missing(library, &folder) {
                            Ok(report) => MaintenanceOutcome::Relocated {
                                relocated: report.relocated.len(),
                                not_found: report.not_found,
                                ambiguous: report.ambiguous,
                            },
                            Err(_) => MaintenanceOutcome::Failed,
                        }),
                        Job::RemoveMissing => Some(match remove_missing(library) {
                            Ok(removed) => MaintenanceOutcome::Removed(removed),
                            Err(_) => MaintenanceOutcome::Failed,
                        }),
                    };
                    if let Some(outcome) = outcome {
                        changed = true;
                        if replies.send(Reply::Maintained(outcome)).is_err() {
                            return;
                        }
                    }
                    match missing_list(library) {
                        Ok((paths, total)) => Reply::Missing(paths, total),
                        Err(_) => Reply::Unavailable,
                    }
                }
                Write::Change(change) => {
                    changed = true;
                    match apply(library, session, change) {
                        Ok(Some(Reply::Deleted(playlist))) => {
                            if lookup.source == Source::Playlist(playlist) {
                                lookup.source = Source::Collection;
                            }
                            Reply::Deleted(playlist)
                        }
                        Ok(Some(Reply::HistoryDeleted(gone))) => {
                            if lookup.source == Source::History(gone) {
                                lookup.source = Source::Collection;
                            }
                            Reply::HistoryDeleted(gone)
                        }
                        Ok(Some(reply)) => reply,
                        Ok(None) => continue,
                        // A change the library refuses, such as one to a playlist deleted
                        // meanwhile, leaves it as it was.
                        Err(
                            LibraryError::InvalidName
                            | LibraryError::NoSuchEntry
                            | LibraryError::WrongKind
                            | LibraryError::NoSuchNode
                            | LibraryError::NoSuchTrack
                            | LibraryError::PathTaken
                            | LibraryError::OutOfRange
                            | LibraryError::InvalidRule
                            | LibraryError::Cycle,
                        ) => continue,
                        Err(_) => Reply::Unavailable,
                    }
                }
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
        // A change alters the counts of the tree. The tree goes before the page, so the
        // browser has both once it no longer waits for an answer.
        if changed && replies.send(tree_reply(library, session)).is_err() {
            return;
        }
        // After an import, or a track added to a playlist from the explorer, the size of the
        // whole collection changed too, which only an empty search of the whole collection
        // tells.
        let mut lookups = vec![lookup.clone()];
        if (imported || changed)
            && (lookup.source != Source::Collection || !lookup.text.trim().is_empty())
        {
            let everything = Lookup {
                source: Source::Collection,
                text: String::new(),
                ..lookup.clone()
            };
            lookups.insert(0, everything);
        }
        for lookup in lookups {
            if !answer(library, session, lookup, replies) {
                return;
            }
        }
    }
}

/// A name a file can take: the characters no system takes in a file name become `_`, and the
/// ends lose their spaces and dots.
fn file_name_of(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|character| {
            if character.is_control() || r#"<>:"/\|?*"#.contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let clean = clean.trim_matches(|character: char| character == ' ' || character == '.');
    if clean.is_empty() {
        "Playlist".to_owned()
    } else {
        clean.to_owned()
    }
}

/// Writes `playlist` into `folder` as `<name>.m3u8`, or `<name> (2).m3u8` and so on when the
/// name is taken; an existing file is never replaced.
fn export_playlist(
    library: &dyn Library,
    playlist: NodeId,
    name: &str,
    folder: &Path,
) -> Result<PlaylistFileOutcome, LibraryError> {
    // A smart playlist has no entries: its tracks are those its rules give now.
    let tracks: Vec<TrackId> = match library.entries(playlist) {
        Ok(entries) => entries.into_iter().map(|entry| entry.track).collect(),
        Err(LibraryError::WrongKind) => smart_tracks(library, playlist, MAX_PAGE)?
            .hits
            .into_iter()
            .map(|hit| hit.track.id)
            .collect(),
        Err(error) => return Err(error),
    };
    export_tracks(library, &tracks, name, folder)
}

/// Writes `tracks` into `folder` as an M3U file, named as [`export_playlist`] names it.
fn export_tracks(
    library: &dyn Library,
    tracks: &[TrackId],
    name: &str,
    folder: &Path,
) -> Result<PlaylistFileOutcome, LibraryError> {
    let mut entries = Vec::new();
    for id in tracks {
        if let Some(track) = library.track(*id)? {
            entries.push(M3uEntry {
                path: track.path,
                artist: track.tags.artist,
                title: track.tags.title,
                seconds: track.tags.seconds,
            });
        }
    }
    let text = write_m3u(&entries);
    let base = file_name_of(name);
    for attempt in 1..=MAX_EXPORT_NAMES {
        let file = if attempt == 1 {
            format!("{base}.m3u8")
        } else {
            format!("{base} ({attempt}).m3u8")
        };
        let path = folder.join(&file);
        let created = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path);
        match created {
            Ok(mut out) => {
                use std::io::Write as _;
                return Ok(match out.write_all(text.as_bytes()) {
                    Ok(()) => PlaylistFileOutcome::Exported {
                        file,
                        entries: entries.len(),
                    },
                    Err(_) => PlaylistFileOutcome::Failed,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Ok(PlaylistFileOutcome::Failed),
        }
    }
    Ok(PlaylistFileOutcome::Failed)
}

/// Makes a playlist in `parent` of each `.m3u` and `.m3u8` file of `folder`, by name, taking
/// the files they name into the collection.
fn import_playlists(
    library: &mut dyn Library,
    parent: Option<NodeId>,
    folder: &Path,
) -> Result<PlaylistFileOutcome, LibraryError> {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return Ok(PlaylistFileOutcome::Failed);
    };
    let mut files: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        ["m3u", "m3u8", "pls", "xspf"]
                            .iter()
                            .any(|known| extension.eq_ignore_ascii_case(known))
                    })
        })
        .collect();
    files.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    files.truncate(MAX_IMPORTED_FILES);
    let (mut playlists, mut tracks, mut skipped) = (0, 0, 0);
    for file in files {
        let fits =
            std::fs::metadata(&file).is_ok_and(|metadata| metadata.len() <= MAX_M3U_BYTES as u64);
        let reading = std::fs::read(&file)
            .ok()
            .filter(|_| fits)
            .and_then(|bytes| {
                let extension = file
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                match extension.as_str() {
                    "pls" => read_pls(&bytes, folder),
                    "xspf" => read_xspf(&bytes, folder),
                    _ => read_m3u(&bytes, folder),
                }
                .ok()
            });
        let Some(reading) = reading else {
            skipped += 1;
            continue;
        };
        skipped += reading.skipped;
        let stem = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name: String = stem.trim().chars().take(MAX_NAME_CHARS).collect();
        let name = if name.is_empty() {
            "Playlist".to_owned()
        } else {
            name
        };
        let playlist = library.create(parent, NodeKind::Playlist, &name)?;
        playlists += 1;
        let mut found = Vec::new();
        for entry in reading.entries {
            match import_file(library, &entry.path, &mut HeaderReader, now())? {
                Some(track) => found.push(track),
                None => skipped += 1,
            }
        }
        tracks += found.len();
        library.add(playlist, &found, None)?;
    }
    Ok(PlaylistFileOutcome::Imported {
        playlists,
        tracks,
        skipped,
    })
}

/// Imports each collection file of `folder`, at most [`MAX_COLLECTION_FILES`] by name:
/// `docs/behavior/browser.md`, *Playlist files*.
fn import_collections(
    library: &mut dyn Library,
    folder: &Path,
) -> Result<PlaylistFileOutcome, LibraryError> {
    let Ok(listing) = std::fs::read_dir(folder) else {
        return Ok(PlaylistFileOutcome::Failed);
    };
    let mut files: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
        })
        .collect();
    files.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    files.truncate(MAX_COLLECTION_FILES);
    let (mut playlists, mut tracks, mut skipped) = (0, 0, 0);
    let mut read_any = false;
    for file in files {
        let fits = std::fs::metadata(&file)
            .is_ok_and(|metadata| metadata.len() <= MAX_COLLECTION_BYTES as u64);
        // Another XML file of the folder is not a collection: it is passed over.
        let Some(shared) = std::fs::read(&file)
            .ok()
            .filter(|_| fits)
            .and_then(|bytes| read_collection(&bytes).ok())
        else {
            continue;
        };
        read_any = true;
        skipped += shared.skipped;
        let mut found: Vec<Option<TrackId>> = Vec::with_capacity(shared.tracks.len());
        for shared_track in &shared.tracks {
            let track = import_file(library, &shared_track.path, &mut HeaderReader, now())?;
            match track {
                Some(track) => {
                    tracks += 1;
                    take_shared(library, track, shared_track)?;
                }
                None => skipped += 1,
            }
            found.push(track);
        }
        let stem = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name: String = shared
            .product
            .clone()
            .unwrap_or(stem)
            .trim()
            .chars()
            .take(MAX_NAME_CHARS)
            .collect();
        let name = if name.is_empty() {
            "Collection".to_owned()
        } else {
            name
        };
        let top = library.create(None, NodeKind::Folder, &name)?;
        playlists += make_tree(library, top, &shared.playlists, &found)?;
    }
    Ok(if read_any {
        PlaylistFileOutcome::Imported {
            playlists,
            tracks,
            skipped,
        }
    } else {
        PlaylistFileOutcome::Failed
    })
}

/// Gives a track what the collection file knows and Platter does not: the beatgrid and the
/// cues of a track without them, the rating and the comment of one without them.
fn take_shared(
    library: &mut dyn Library,
    id: TrackId,
    shared: &platter_library::SharedTrack,
) -> Result<(), LibraryError> {
    let Some(track) = library.track(id)? else {
        return Ok(());
    };
    let memory = &shared.memory;
    let has_memory = !memory.grid.is_empty() || memory.cues.iter().next().is_some();
    if has_memory && !library.has_memory(&track.path)? {
        library.set_memory(track.id, memory)?;
    }
    if track.rating == 0 && shared.rating > 0 {
        library.set_rating(track.id, shared.rating)?;
    }
    if track.comment.is_none() && shared.comment.is_some() {
        library.set_comment(track.id, shared.comment.as_deref())?;
    }
    Ok(())
}

/// Makes the folders and playlists of `nodes` under `parent`; returns how many playlists.
fn make_tree(
    library: &mut dyn Library,
    parent: NodeId,
    nodes: &[SharedNode],
    found: &[Option<TrackId>],
) -> Result<usize, LibraryError> {
    let mut made = 0;
    for node in nodes {
        let name: String = node.name.chars().take(MAX_NAME_CHARS).collect();
        match &node.playlist {
            Some(entries) => {
                let playlist = library.create(Some(parent), NodeKind::Playlist, &name)?;
                made += 1;
                let entries: Vec<TrackId> = entries
                    .iter()
                    .filter_map(|index| found.get(*index).copied().flatten())
                    .collect();
                library.add(playlist, &entries, None)?;
            }
            None => {
                let folder = library.create(Some(parent), NodeKind::Folder, &name)?;
                made += make_tree(library, folder, &node.children, found)?;
            }
        }
    }
    Ok(made)
}

/// The paths of the missing tracks, the first [`MAX_MISSING_LISTED`], and how many there are.
fn missing_list(library: &dyn Library) -> Result<(Vec<String>, usize), LibraryError> {
    let missing = library.missing()?;
    let total = missing.len();
    let paths = missing
        .into_iter()
        .take(MAX_MISSING_LISTED)
        .map(|track| track.path.to_string_lossy().into_owned())
        .collect();
    Ok((paths, total))
}

/// Takes the tracks marked missing out of the collection; returns how many went.
fn remove_missing(library: &mut dyn Library) -> Result<usize, LibraryError> {
    let mut removed = 0;
    for track in library.missing()? {
        match library.remove_track(track.id) {
            Ok(()) => removed += 1,
            Err(LibraryError::NoSuchTrack) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(removed)
}

/// The row of a track; `session` is when the session of this run opened, so a track played
/// since is marked.
fn row_of(track: Track, session: Option<Timestamp>, prepared: &HashSet<TrackId>) -> FileRow {
    let in_preparation = prepared.contains(&track.id);
    let played = session.is_some_and(|opened| track.last_played.is_some_and(|at| at >= opened));
    let name = track.path.file_name().map_or_else(
        || track.path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let tags = track.tags;
    FileRow {
        name,
        kind: RowKind::Track(TrackRow {
            title: tags.title,
            artist: tags.artist,
            album: tags.album,
            bpm: tags.bpm,
            seconds: tags.seconds,
            // The collection does not keep the container of a file.
            format: String::new(),
            sample_rate: tags.sample_rate.unwrap_or(0),
            missing: track.missing,
            played,
            prepared: in_preparation,
            colour: track.colour,
            rating: Some(track.rating),
            genre: tags.genre,
            comment: track.comment,
            key: track.key,
        }),
        path: track.path,
    }
}

pub struct Collection {
    /// `None` once the worker has gone: there is nobody to ask.
    requests: Option<Sender<Request>>,
    replies: Receiver<Reply>,
    available: Option<bool>,
    rows: Vec<FileRow>,
    /// The entry of each row in the playlist shown; none in the whole collection.
    entries: Vec<(TrackId, Option<EntryId>)>,
    /// When the session of this run opened.
    session: Option<Timestamp>,
    /// The session of this run and the earlier ones the tree lists, the latest first.
    sessions: Vec<SessionRow>,
    current_session: Option<SessionId>,
    /// Tracks that matched the search, beyond the rows too.
    matched: Option<usize>,
    /// Tracks in the whole collection, as the last answer to an empty search said.
    total: Option<usize>,
    importing: bool,
    /// The progress of the import that runs, and the request to stop it.
    watch: ImportWatch,
    imported: Option<ImportOutcome>,
    missing: Vec<String>,
    missing_total: Option<usize>,
    maintaining: bool,
    maintained: Option<MaintenanceOutcome>,
    exchanging: bool,
    exchanged: Option<PlaylistFileOutcome>,
    /// What the rows are for, or will be once its answer comes.
    lookup: Lookup,
    playlists: Vec<PlaylistRow>,
    /// A playlist created and not yet shown.
    created: Option<NodeId>,
    /// What Platter writes into the files.
    tag_writing: TagWriting,
    /// Why files kept what they had, until the window asks.
    unwritten: Vec<String>,
    /// Covers were written into files since the window last asked.
    covers_changed: bool,
}

impl Collection {
    /// Opens the library with `open` on a worker thread of its own, and reads the
    /// collection once it is open.
    pub fn start<F>(open: F) -> Self
    where
        F: FnOnce() -> Result<Box<dyn Library>, LibraryError> + Send + 'static,
    {
        let (requests, waiting) = channel();
        let (answers, replies) = channel();
        let watch = ImportWatch::default();
        let shared = watch.clone();
        let spawned = std::thread::Builder::new()
            .name("platter-library".to_owned())
            .spawn(move || match open() {
                Ok(mut library) => serve(library.as_mut(), &waiting, &answers, &shared),
                Err(_) => {
                    // Nobody may be listening any more; then there is nothing to tell.
                    answers.send(Reply::Unavailable).ok();
                }
            });
        let mut collection = Self {
            requests: spawned.is_ok().then_some(requests),
            replies,
            available: if spawned.is_ok() { None } else { Some(false) },
            rows: Vec::new(),
            entries: Vec::new(),
            session: None,
            sessions: Vec::new(),
            current_session: None,
            matched: None,
            total: None,
            importing: false,
            watch,
            imported: None,
            missing: Vec::new(),
            missing_total: None,
            maintaining: false,
            maintained: None,
            exchanging: false,
            exchanged: None,
            lookup: Lookup::default(),
            playlists: Vec::new(),
            created: None,
            tag_writing: TagWriting::default(),
            unwritten: Vec::new(),
            covers_changed: false,
        };
        collection.send(Request::Read(Lookup::default()));
        collection
    }

    fn send(&mut self, request: Request) {
        let sent = self
            .requests
            .as_ref()
            .is_some_and(|requests| requests.send(request).is_ok());
        if !sent {
            self.requests = None;
            self.available = Some(false);
        }
    }

    /// Adds the music of `folder` to the collection. One import runs at a time.
    pub fn import(&mut self, folder: PathBuf) {
        if self.importing || self.available != Some(true) {
            return;
        }
        self.importing = true;
        self.watch.looked.store(0, Ordering::Relaxed);
        self.watch.stop.store(false, Ordering::Relaxed);
        self.send(Request::Write(Write::Import(folder)));
    }

    /// Asks the import that runs to stop before the next file.
    pub fn stop_import(&mut self) {
        if self.importing {
            self.watch.stop.store(true, Ordering::Relaxed);
        }
    }

    /// Asks which tracks are missing, for the maintenance dialog.
    pub fn list_missing(&mut self) {
        if self.available == Some(true) {
            self.send(Request::Write(Write::Maintain(Job::List)));
        }
    }

    /// Checks the files of the collection, one task of maintenance at a time.
    pub fn check(&mut self) {
        self.maintain(Job::Check);
    }

    /// Looks for the files of the missing tracks in `folder` and below.
    pub fn relocate(&mut self, folder: PathBuf) {
        self.maintain(Job::Relocate(folder));
    }

    /// Takes the tracks marked missing out of the collection.
    pub fn remove_missing(&mut self) {
        self.maintain(Job::RemoveMissing);
    }

    /// Writes a playlist as an M3U file into `folder`, one file at a time.
    pub fn export(&mut self, playlist: NodeId, folder: PathBuf) {
        let Some(name) = self
            .playlists
            .iter()
            .find(|row| row.id == playlist && !row.folder)
            .map(|row| row.name.clone())
        else {
            return;
        };
        if self.exchanging || self.available != Some(true) {
            return;
        }
        self.exchanging = true;
        self.send(Request::Write(Write::Export(playlist, name, folder)));
    }

    /// The label of a session, as the tree shows it.
    fn session_label(&self, session: SessionId) -> Option<String> {
        self.sessions
            .iter()
            .find(|row| row.id == session)
            .map(|row| row.label.clone())
    }

    /// Makes a playlist, named after the session, of the tracks played in it.
    pub fn history_to_playlist(&mut self, session: SessionId) {
        if let Some(label) = self.session_label(session) {
            self.change(Change::HistoryToPlaylist(session, label));
        }
    }

    /// Writes the history of a session as an M3U file into `folder`, named after the session.
    pub fn export_history(&mut self, session: SessionId, folder: PathBuf) {
        let Some(label) = self.session_label(session) else {
            return;
        };
        if self.exchanging || self.available != Some(true) {
            return;
        }
        self.exchanging = true;
        self.send(Request::Write(Write::ExportHistory(session, label, folder)));
    }

    /// Forgets the history of a session of an earlier run.
    pub fn delete_history(&mut self, session: SessionId) {
        if Some(session) != self.current_session {
            self.change(Change::HistoryDelete(session));
        }
    }

    /// Imports each collection file of `folder`: its tracks with their grids and cues, and its
    /// playlists under a new folder of the tree.
    pub fn import_collections(&mut self, folder: PathBuf) {
        if self.exchanging || self.importing || self.available != Some(true) {
            return;
        }
        self.exchanging = true;
        self.send(Request::Write(Write::ImportCollections(folder)));
        let lookup = Lookup {
            changes: self.lookup.changes.wrapping_add(1),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// Makes a playlist of each playlist file of `folder`, in the folder `parent` of the tree.
    pub fn import_playlists(&mut self, parent: Option<NodeId>, folder: PathBuf) {
        if self.exchanging || self.importing || self.available != Some(true) {
            return;
        }
        self.exchanging = true;
        self.send(Request::Write(Write::ImportPlaylists(parent, folder)));
        let lookup = Lookup {
            changes: self.lookup.changes.wrapping_add(1),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    fn maintain(&mut self, job: Job) {
        if self.maintaining || self.importing || self.available != Some(true) {
            return;
        }
        self.maintaining = true;
        self.send(Request::Write(Write::Maintain(job)));
        // The rows and the counts change: the list waits for them as after a change.
        let lookup = Lookup {
            changes: self.lookup.changes.wrapping_add(1),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// Deletes a playlist; a folder is left as it is. When the list shows the playlist, it
    /// shows the whole collection once the playlist is gone.
    pub fn delete(&mut self, playlist: NodeId) {
        self.change(Change::Delete(playlist));
    }

    /// Creates a playlist or a folder in `folder`, or at the top of the tree. A new playlist
    /// is handed out by [`Collection::take_created`] once it exists.
    pub fn create(&mut self, folder: Option<NodeId>, kind: NodeKind, name: String) {
        self.change(Change::Create(folder, kind, name));
    }

    /// Gives a playlist or a folder another name.
    pub fn rename(&mut self, item: NodeId, name: String) {
        self.change(Change::Rename(item, name));
    }

    /// Puts a playlist or a folder in another folder, or at the top of the tree. A folder does
    /// not go into itself or into a folder inside it.
    pub fn move_to(&mut self, item: NodeId, folder: Option<NodeId>) {
        self.change(Change::Move(item, folder));
    }

    /// Adds the track at `path` at the end of a playlist, taking the file into the collection
    /// first when it is not there.
    pub fn add(&mut self, playlist: NodeId, path: PathBuf) {
        self.change(Change::Add(playlist, path));
    }

    /// The playlist created last, once, so the browser can show it.
    pub fn take_created(&mut self) -> Option<NodeId> {
        self.created.take()
    }

    /// Takes every entry out of a playlist.
    pub fn clear(&mut self, playlist: NodeId) {
        self.change(Change::Clear(playlist));
    }

    /// Takes the repeats of each track out of a playlist, keeping its first entry.
    pub fn remove_repeats(&mut self, playlist: NodeId) {
        self.change(Change::RemoveRepeats(playlist));
    }

    /// Takes one entry out of a playlist.
    pub fn remove(&mut self, playlist: NodeId, entry: EntryId) {
        self.change(Change::Remove(playlist, entry));
    }

    /// Replaces the rules of a smart playlist.
    pub fn set_rules(&mut self, smart: NodeId, rules: Vec<Rule>) {
        self.change(Change::SetRules(smart, rules));
    }

    /// Makes a playlist of the tracks a smart playlist holds now, beside it and with its name.
    pub fn smart_to_playlist(&mut self, smart: NodeId) {
        self.change(Change::SmartToPlaylist(smart));
    }

    /// Makes a copy of a smart playlist beside it.
    pub fn duplicate_smart(&mut self, smart: NodeId) {
        self.change(Change::DuplicateSmart(smart));
    }

    fn change(&mut self, change: Change) {
        if self.available != Some(true) {
            return;
        }
        self.send(Request::Write(Write::Change(change)));
        // The rows and the counts change: the list waits for them as for a new lookup.
        let lookup = Lookup {
            changes: self.lookup.changes.wrapping_add(1),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// Asks for the tracks that match `text`; the rows change when the answer comes.
    pub fn search(&mut self, text: &str) {
        let lookup = Lookup {
            text: text.to_owned(),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// Asks for the tracks of the whole collection, a playlist or a history. The rows of what
    /// was shown before go at once; the new ones come with the answer.
    pub fn show(&mut self, source: Source) {
        let lookup = Lookup {
            source,
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// What the rows are for.
    pub fn source(&self) -> Source {
        self.lookup.source
    }

    /// The playlist the rows are for, if they are for one.
    pub fn playlist(&self) -> Option<NodeId> {
        match self.lookup.source {
            Source::Playlist(playlist) => Some(playlist),
            Source::Collection | Source::History(_) => None,
        }
    }

    /// The playlist used as the preparation list, as the tree last read it.
    pub fn preparation(&self) -> Option<NodeId> {
        self.playlists
            .iter()
            .find(|row| row.preparation)
            .map(|row| row.id)
    }

    /// The session of this run, once the library has opened it.
    pub fn current_session(&self) -> Option<SessionId> {
        self.current_session
    }

    /// Asks for the tracks in another order; the rows change when the answer comes.
    pub fn sort(&mut self, sort: Option<(SortColumn, bool)>) {
        let lookup = Lookup {
            sort,
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// Reads the rows shown again, after something outside the browser changed them.
    pub fn refresh(&mut self) {
        let lookup = Lookup {
            changes: self.lookup.changes.wrapping_add(1),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    fn ask(&mut self, lookup: Lookup) {
        if self.lookup == lookup {
            return;
        }
        if self.lookup.source != lookup.source {
            self.rows.clear();
            self.entries.clear();
        }
        self.lookup = lookup;
        self.matched = None;
        self.send(Request::Read(self.lookup.clone()));
    }

    /// Takes in the answers of the worker. Returns whether anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(reply) = self.replies.try_recv() {
            changed = true;
            match reply {
                Reply::Unavailable => {
                    self.available = Some(false);
                    self.importing = false;
                    self.maintaining = false;
                    self.exchanging = false;
                    self.rows.clear();
                    self.entries.clear();
                    self.matched = None;
                    self.total = None;
                    self.playlists.clear();
                    self.sessions.clear();
                }
                Reply::Page(lookup, page, prepared) => {
                    // The worker reads the collection again after an import: the import is
                    // over once the list shows what it added.
                    self.importing = false;
                    self.available = Some(true);
                    if lookup.source == Source::Collection && lookup.text.trim().is_empty() {
                        self.total = Some(page.total);
                    }
                    // An answer to a lookup the user has since changed is of no use.
                    if lookup == self.lookup {
                        self.matched = Some(page.total);
                        (self.entries, self.rows) = page
                            .hits
                            .into_iter()
                            .map(|hit| {
                                (
                                    (hit.track.id, hit.entry),
                                    row_of(hit.track, self.session, &prepared),
                                )
                            })
                            .unzip();
                    }
                }
                Reply::Created(playlist) => self.created = Some(playlist),
                Reply::HistoryDeleted(gone) => {
                    if self.lookup.source == Source::History(gone) {
                        self.ask(Lookup {
                            source: Source::Collection,
                            ..self.lookup.clone()
                        });
                    }
                }
                Reply::Deleted(playlist) => {
                    if self.lookup.source == Source::Playlist(playlist) {
                        // Asked again although the worker has already turned to the whole
                        // collection: a lookup sent since the change may still name the
                        // playlist, and no answer to it would match the one kept here.
                        self.ask(Lookup {
                            source: Source::Collection,
                            ..self.lookup.clone()
                        });
                    }
                }
                Reply::Imported(outcome) => {
                    self.imported = Some(outcome);
                    self.exchanged = None;
                }
                Reply::PlaylistFile(outcome) => {
                    self.exchanging = false;
                    self.exchanged = Some(outcome);
                    self.imported = None;
                }
                Reply::Missing(paths, total) => {
                    self.missing = paths;
                    self.missing_total = Some(total);
                }
                Reply::NotWritten(why) => self.unwritten.extend(why),
                Reply::Covers(why) => {
                    self.covers_changed = true;
                    self.unwritten.extend(why);
                }
                Reply::Maintained(outcome) => {
                    self.maintaining = false;
                    self.maintained = Some(outcome);
                }
                Reply::Tree(rows, sessions) => {
                    self.playlists = rows;
                    self.sessions = sessions
                        .into_iter()
                        .map(|listed| SessionRow {
                            id: listed.id,
                            label: utc_label(listed.opened),
                            tracks: listed.played,
                            current: false,
                        })
                        .collect();
                }
                Reply::Session(id, opened) => {
                    self.current_session = Some(id);
                    self.session = Some(opened);
                }
            }
        }
        changed
    }

    /// The tracks of the collection or of the playlist, in the order of the last answer.
    pub fn rows(&self) -> &[FileRow] {
        &self.rows
    }

    /// The entry of the row at `index` in the playlist shown; none in the whole collection.
    pub fn entry(&self, index: usize) -> Option<EntryId> {
        self.entries.get(index).and_then(|(_, entry)| *entry)
    }

    /// The track of the row at `index`.
    pub fn track(&self, index: usize) -> Option<TrackId> {
        self.entries.get(index).map(|(track, _)| *track)
    }

    /// Records a track that has counted as played, taking it into the collection first when
    /// it is not there.
    pub fn played(&mut self, played: Played) {
        self.change(Change::Played(played));
    }

    /// Keeps what a deck learned of the track at `path`.
    pub fn keep(&mut self, path: PathBuf, kept: Kept) {
        self.change(Change::Keep(path, Box::new(kept), self.tag_writing));
    }

    /// What Platter writes into the files from now on.
    pub fn set_tag_writing(&mut self, mode: TagWriting) {
        self.tag_writing = mode;
    }

    /// Why files kept what they had since the last call.
    pub fn take_unwritten(&mut self) -> Vec<String> {
        std::mem::take(&mut self.unwritten)
    }

    /// Imports the picture at `picture` as the front cover of each of `tracks`, or deletes
    /// their pictures, as one change. A cover lives only in the file: nothing happens unless
    /// the setting lets Platter write every tag.
    pub fn set_cover(&mut self, tracks: Vec<TrackId>, picture: Option<PathBuf>) {
        if self.tag_writing == TagWriting::Everything {
            self.change(Change::Cover(tracks, picture));
        }
    }

    /// Whether covers were written into files since the last call.
    pub fn take_covers_changed(&mut self) -> bool {
        std::mem::take(&mut self.covers_changed)
    }

    /// Makes a playlist the preparation list.
    pub fn set_preparation(&mut self, playlist: NodeId) {
        self.change(Change::SetPreparation(playlist));
    }

    /// Puts a playlist in a favourite place.
    pub fn set_favourite(&mut self, place: usize, playlist: NodeId) {
        self.change(Change::SetFavourite(place, playlist));
    }

    /// Empties a favourite place.
    pub fn clear_favourite(&mut self, place: usize) {
        self.change(Change::ClearFavourite(place));
    }

    /// Adds a track to the preparation list, when there is one.
    pub fn prepare(&mut self, track: TrackId, place: Place) {
        self.change(Change::Prepare(track, place));
    }

    /// Stores what the dialog of the details of a track holds.
    /// Stores `fields` of `details` for each of `tracks`, as one change.
    pub fn save_details(
        &mut self,
        tracks: Vec<TrackId>,
        details: TrackDetails,
        fields: DetailFields,
    ) {
        self.change(Change::Details(tracks, details, fields, self.tag_writing));
    }

    /// Tags a track with a colour, 1 to 8, or none with 0.
    pub fn set_colour(&mut self, track: TrackId, colour: u8) {
        self.change(Change::SetColour(track, colour));
    }

    /// Rates a track, 0 to 5.
    pub fn set_rating(&mut self, track: TrackId, rating: u8) {
        self.change(Change::SetRating(track, rating));
    }

    /// Deletes a track from the collection and from every playlist; its file stays.
    pub fn delete_track(&mut self, track: TrackId) {
        self.change(Change::DeleteTrack(track));
    }

    /// Sets the play count of a track to zero; it is no longer marked as played.
    pub fn reset_played(&mut self, track: TrackId) {
        self.change(Change::ResetPlayed(track));
    }

    /// The library is opening, importing or searching: answers are still to come.
    /// Whether the library opened: `None` while it has not answered yet.
    pub fn available(&self) -> Option<bool> {
        self.available
    }

    pub fn busy(&self) -> bool {
        self.available.is_none()
            || self.importing
            || self.maintaining
            || self.exchanging
            || self.pending_search()
    }

    fn pending_search(&self) -> bool {
        self.available == Some(true) && self.matched.is_none()
    }

    /// What the browser draws of the library; `shown` says whether the list shows it.
    pub fn view(&self, shown: bool) -> CollectionView {
        let source = if shown {
            Some(self.lookup.source)
        } else {
            None
        };
        let playlist = self.playlist().filter(|_| shown);
        let history = match source {
            Some(Source::History(session)) => Some(session),
            _ => None,
        };
        let rows: Vec<SessionRow> = self
            .sessions
            .iter()
            .map(|row| SessionRow {
                current: history == Some(row.id),
                ..row.clone()
            })
            .collect();
        let this_run = |row: &SessionRow| Some(row.id) == self.current_session;
        let session = rows.iter().find(|row| this_run(row)).cloned();
        CollectionView {
            available: self.available,
            tracks: self.total,
            matched: self.matched,
            current: source == Some(Source::Collection),
            playlist,
            history,
            session,
            sessions: rows
                .into_iter()
                .filter(|row| !this_run(row))
                .take(MAX_SESSIONS)
                .collect(),
            playlists: self
                .playlists
                .iter()
                .map(|row| PlaylistRow {
                    current: playlist == Some(row.id),
                    ..row.clone()
                })
                .collect(),
            importing: self.importing,
            import_looked: self.watch.looked.load(Ordering::Relaxed),
            imported: self.imported,
            missing: self.missing.clone(),
            missing_total: self.missing_total,
            maintaining: self.maintaining,
            maintained: self.maintained,
            exchanging: self.exchanging,
            exchanged: self.exchanged.clone(),
        }
    }
}

/// `at` as the tree labels a session: date and time to the minute, in UTC, such as
/// `2026-11-15 20:14`.
pub fn utc_label(at: Timestamp) -> String {
    let (year, month, day, hour, minute, _) = at.utc();
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

#[cfg(test)]
mod tests {
    use platter_library::Timestamp;

    use super::utc_label;

    #[test]
    fn a_session_is_labelled_by_its_date_and_time_in_utc() {
        assert_eq!(utc_label(Timestamp(0)), "1970-01-01 00:00");
        assert_eq!(utc_label(Timestamp(1_700_000_000)), "2023-11-14 22:13");
        // A leap day, and the last minute of a year.
        assert_eq!(utc_label(Timestamp(951_782_400)), "2000-02-29 00:00");
        assert_eq!(utc_label(Timestamp(1_735_689_599)), "2024-12-31 23:59");
        assert_eq!(utc_label(Timestamp(-60)), "1969-12-31 23:59");
    }
}
