//! The file selector behind the browser strip: which folder is shown, what it holds and
//! which row is selected.
//!
//! Reading a folder and the headers of its audio files is slow work. It runs on a worker
//! thread and arrives through a channel, so the interface never waits for the disk: a
//! folder appears at once with the names of its files, and title, artist and the rest fill
//! in as each file is read. Behaviour note: `docs/behavior/browser.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use platter_core::DeckId;
use platter_decode::probe_file;
use platter_engine::Command;
use platter_library::{NodeId, NodeKind, Source, TrackId};

use crate::collection::Place;
use crate::collection::{Collection, Kept, TagWriting};
use crate::command::{
    Action, AppCommand, BrowserNode, DetailFields, ListColumn, NewItem, SortColumn, TrackDetails,
};
use crate::covers::{COVERS_PER_LIST, Covers};
use crate::model::{AnalysisStatus, BrowserView, FileRow, RowKind, TrackRow, TreeRow};
use crate::played::Played;

/// Extensions of the formats Platter plays. The content decides whether a file really is
/// one; the extension only decides whether it is listed.
const AUDIO_EXTENSIONS: [&str; 9] = [
    "mp3", "m4a", "mp4", "wav", "aif", "aiff", "flac", "ogg", "oga",
];
/// Extensions of the pictures a cover can be imported from, in any case.
const PICTURE_EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];
/// Pictures of a folder offered for a cover, the first by name: as many as a menu shows in
/// the window, 28 points a row in 900.
pub const MAX_PICTURES: usize = 20;
/// Entries of one folder that are listed. A folder with more shows its first ones.
const MAX_ENTRIES: usize = 10_000;
/// Levels of the tree that can be open below a root.
const MAX_DEPTH: usize = 32;

pub(crate) fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            PICTURE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// What a folder holds, as the browser lists it.
struct Listing {
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
    /// The first [`MAX_PICTURES`] JPEG and PNG files.
    pictures: Vec<PathBuf>,
}

/// The folders, the audio files and the pictures of `folder`, each sorted by name without
/// regard to case. Hidden entries and what cannot be read are left out.
fn list(folder: &Path) -> Listing {
    let (mut folders, mut files, mut pictures) = (Vec::new(), Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Listing {
            folders,
            files,
            pictures,
        };
    };
    for entry in entries.flatten().take(MAX_ENTRIES) {
        let path = entry.path();
        if name_of(&path).starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => folders.push(path),
            Ok(kind) if kind.is_file() && is_audio(&path) => files.push(path),
            Ok(kind) if kind.is_file() && is_picture(&path) => pictures.push(path),
            _ => {}
        }
    }
    let key = |path: &PathBuf| name_of(path).to_lowercase();
    folders.sort_by_key(key);
    files.sort_by_key(key);
    pictures.sort_by_key(key);
    pictures.truncate(MAX_PICTURES);
    Listing {
        folders,
        files,
        pictures,
    }
}

/// The words of a search text, folded to lower case.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether every word is in the name, title, artist or album of a row of a folder.
fn matches(row: &FileRow, words: &[String]) -> bool {
    if words.is_empty() {
        return true;
    }
    let mut fields = vec![row.name.to_lowercase()];
    if let RowKind::Track(track) = &row.kind {
        for tag in [&track.title, &track.artist, &track.album]
            .into_iter()
            .flatten()
        {
            fields.push(tag.to_lowercase());
        }
    }
    words
        .iter()
        .all(|word| fields.iter().any(|field| field.contains(word.as_str())))
}

/// The value of a row in a column, for sorting. Rows without one sort last whatever the
/// direction; a file without a title sorts by its name.
enum Key {
    Text(String),
    Number(f64),
}

fn key(row: &FileRow, column: SortColumn) -> Option<Key> {
    let RowKind::Track(track) = &row.kind else {
        return None;
    };
    let text = |value: &Option<String>| value.as_ref().map(|text| Key::Text(text.to_lowercase()));
    match column {
        SortColumn::Title => Some(Key::Text(
            track.title.as_ref().unwrap_or(&row.name).to_lowercase(),
        )),
        SortColumn::Artist => text(&track.artist),
        SortColumn::Album => text(&track.album),
        SortColumn::Bpm => track.bpm.filter(|bpm| bpm.is_finite()).map(Key::Number),
        SortColumn::Time => track
            .seconds
            .filter(|seconds| seconds.is_finite())
            .map(Key::Number),
        SortColumn::Key => track
            .key
            .map(|key| Key::Number(f64::from(crate::keys::sort_place(Some(key))))),
        SortColumn::Genre => text(&track.genre),
        SortColumn::Rating => track
            .rating
            .filter(|rating| *rating > 0)
            .map(|rating| Key::Number(f64::from(rating))),
        SortColumn::Colour => (track.colour > 0).then(|| Key::Number(f64::from(track.colour))),
    }
}

fn compare(a: &FileRow, b: &FileRow, column: SortColumn, descending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (key(a, column), key(b, column)) {
        (Some(a), Some(b)) => {
            let order = match (a, b) {
                (Key::Text(a), Key::Text(b)) => a.cmp(&b),
                (Key::Number(a), Key::Number(b)) => a.total_cmp(&b),
                _ => Ordering::Equal,
            };
            if descending { order.reverse() } else { order }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn probe(path: &Path) -> RowKind {
    match probe_file(path) {
        Ok(info) => RowKind::Track(TrackRow {
            title: info.tags.title,
            artist: info.tags.artist,
            album: info.tags.album,
            bpm: info.tags.bpm,
            seconds: info.seconds,
            format: info.container.name().to_owned(),
            sample_rate: info.sample_rate.hz(),
            missing: false,
            played: false,
            prepared: false,
            colour: 0,
            rating: None,
            genre: info.tags.genre,
            comment: None,
            key: None,
        }),
        Err(_) => RowKind::Unreadable,
    }
}

enum Message {
    /// What a folder holds, for the tree and, when it is the current one, for the list.
    Listed {
        folder: PathBuf,
        folders: Vec<PathBuf>,
        files: Vec<PathBuf>,
        pictures: Vec<PathBuf>,
    },
    /// The header of one file of the current folder has been read.
    Probed {
        visit: u64,
        path: PathBuf,
        kind: RowKind,
    },
}

/// A root of the tree: the home folder, a drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub name: String,
    pub path: PathBuf,
}

pub struct FileBrowser {
    roots: Vec<Root>,
    open: BTreeSet<PathBuf>,
    /// Folders inside each folder that has been read.
    folders: BTreeMap<PathBuf, Vec<PathBuf>>,
    /// Audio files in each folder that has been read.
    counts: BTreeMap<PathBuf, usize>,
    current: Option<PathBuf>,
    /// The pictures of the current folder, for a cover.
    pictures: Vec<PathBuf>,
    rows: Vec<FileRow>,
    /// The current row.
    selected: Option<usize>,
    /// The files of the selected rows while more than one row is selected; empty when the
    /// current row is selected alone. Rows are selected by their file, never by their place,
    /// so a list read again keeps the same tracks selected wherever they are.
    chosen: Vec<PathBuf>,
    reading: bool,
    /// Counts the folders shown, so results for a folder the user has left are dropped and
    /// the worker that reads them stops.
    visit: Arc<AtomicU64>,
    results: Receiver<Message>,
    reports: Sender<Message>,
    /// The collection and the playlists of the tree, when the browser has a library.
    collection: Option<Collection>,
    /// The list shows the collection or a playlist instead of the current folder.
    showing_library: bool,
    /// The track loaded last into a deck, where *Add as next* puts a track after.
    last_loaded: Option<PathBuf>,
    /// The covers of the tracks of the list.
    covers: Covers,
    /// The text of the search field, for the collection and for the folders alike.
    search: String,
    /// The column the list is sorted by, and whether the order is turned round.
    sort: Option<(SortColumn, bool)>,
    /// The columns the list shows besides the title, in their order.
    columns: Vec<ListColumn>,
    /// The folders of playlists closed in the tree, and whether *History* hides the earlier
    /// sessions.
    closed_folders: BTreeSet<NodeId>,
    history_closed: bool,
    /// The folder of the mix recorder, for *Recordings*.
    recordings: Option<PathBuf>,
    analysis: Option<AnalysisStatus>,
    problem: Option<String>,
    /// The tracks the decks hold, whose covers the deck headers show.
    deck_tracks: [Option<PathBuf>; DeckId::COUNT],
}

impl FileBrowser {
    pub fn new(roots: Vec<Root>) -> Self {
        let (reports, results) = channel();
        Self {
            roots,
            open: BTreeSet::new(),
            folders: BTreeMap::new(),
            counts: BTreeMap::new(),
            current: None,
            pictures: Vec::new(),
            rows: Vec::new(),
            selected: None,
            chosen: Vec::new(),
            reading: false,
            visit: Arc::new(AtomicU64::new(0)),
            results,
            reports,
            collection: None,
            showing_library: false,
            last_loaded: None,
            covers: Covers::start(),
            search: String::new(),
            sort: None,
            columns: ListColumn::SHOWN_AT_FIRST.to_vec(),
            closed_folders: BTreeSet::new(),
            history_closed: false,
            recordings: None,
            analysis: None,
            problem: None,
            deck_tracks: Default::default(),
        }
    }

    /// Gives the tree *Recordings*, after the folders, which shows `folder`: where the mix
    /// recorder writes (`docs/behavior/browser.md`, *Recordings*).
    pub fn set_recordings(&mut self, folder: Option<PathBuf>) {
        self.recordings = folder;
    }

    /// Gives the tree the collection and the playlists, above the folders.
    pub fn attach(&mut self, collection: Collection) {
        self.collection = Some(collection);
    }

    /// Whether the library opened: `None` while it has not answered, `Some(false)` without one.
    pub fn library_available(&self) -> Option<bool> {
        match &self.collection {
            Some(collection) => collection.available(),
            None => Some(false),
        }
    }

    /// Answers are still to come from the library: the window draws again to take them.
    pub fn collection_busy(&self) -> bool {
        self.collection.as_ref().is_some_and(Collection::busy)
    }

    /// Shows the whole collection, a playlist or a history in the list.
    fn show_library(&mut self, source: Source) {
        let Some(collection) = &mut self.collection else {
            return;
        };
        if self.showing_library && collection.source() == source {
            return;
        }
        self.showing_library = true;
        self.chosen.clear();
        collection.show(source);
        // The rows are those of the last answer for the same node, or none; the selection
        // follows when a newer one comes.
        self.selected = (!collection.rows().is_empty()).then_some(0);
    }

    /// The rows the list shows: those of the collection or the playlist, which the library
    /// has already searched, or those of the current folder that match the search.
    fn shown(&self) -> Vec<&FileRow> {
        match &self.collection {
            Some(collection) if self.showing_library => collection.rows().iter().collect(),
            _ => {
                let words = words(&self.search);
                let mut rows: Vec<&FileRow> = self
                    .rows
                    .iter()
                    .filter(|row| matches(row, &words))
                    .collect();
                if let Some((column, descending)) = self.sort {
                    // Folders stay first; the sort is stable, so ties keep the order of the name.
                    rows.sort_by(|a, b| {
                        let folder = |row: &FileRow| !matches!(row.kind, RowKind::Folder);
                        folder(a)
                            .cmp(&folder(b))
                            .then_with(|| compare(a, b, column, descending))
                    });
                }
                rows
            }
        }
    }

    /// The selection on the first row shown, or none when nothing is.
    fn select_first(&mut self) {
        self.chosen.clear();
        self.selected = (!self.shown().is_empty()).then_some(0);
    }

    /// The selected rows, in the order of the list: those of the chosen files, otherwise the
    /// current row alone.
    fn selection(&self) -> Vec<usize> {
        let shown = self.shown();
        let rows: Vec<usize> = shown
            .iter()
            .enumerate()
            .filter(|(_, row)| self.chosen.contains(&row.path))
            .map(|(index, _)| index)
            .collect();
        if rows.is_empty() {
            self.selected
                .filter(|index| *index < shown.len())
                .into_iter()
                .collect()
        } else {
            rows
        }
    }

    /// Selects `rows`, which must be rows of the list, with `current` as the current row.
    fn choose(&mut self, rows: impl IntoIterator<Item = usize>, current: usize) {
        let shown = self.shown();
        let mut chosen: Vec<PathBuf> = rows
            .into_iter()
            .filter_map(|index| Some(shown.get(index)?.path.clone()))
            .collect();
        chosen.sort();
        chosen.dedup();
        self.chosen = if chosen.len() > 1 { chosen } else { Vec::new() };
        self.selected = Some(current);
    }

    /// The commands that change which rows are selected.
    fn select(&mut self, command: &AppCommand) {
        let count = self.shown().len();
        match *command {
            AppCommand::BrowserSelect { index } if index < count => {
                self.chosen.clear();
                self.selected = Some(index);
            }
            AppCommand::BrowserSelectToggle { index } if index < count => {
                let mut rows = self.selection();
                if rows.contains(&index) {
                    // Every row of the same track goes, and the selection is never left empty.
                    let shown = self.shown();
                    let path = shown.get(index).map(|row| row.path.clone());
                    let rest: Vec<usize> = rows
                        .into_iter()
                        .filter(|row| shown.get(*row).map(|row| &row.path) != path.as_ref())
                        .collect();
                    if let Some(first) = rest.first().copied() {
                        let current = self
                            .selected
                            .filter(|current| rest.contains(current))
                            .unwrap_or(first);
                        self.choose(rest, current);
                    }
                } else {
                    rows.push(index);
                    self.choose(rows, index);
                }
            }
            AppCommand::BrowserSelectExtend { index } if index < count => {
                match self.selected.filter(|current| *current < count) {
                    Some(current) => {
                        self.choose(current.min(index)..=current.max(index), current);
                    }
                    None => {
                        self.chosen.clear();
                        self.selected = Some(index);
                    }
                }
            }
            AppCommand::BrowserSelectAll if count > 0 => {
                let current = self
                    .selected
                    .filter(|current| *current < count)
                    .unwrap_or(0);
                self.choose(0..count, current);
            }
            _ => {}
        }
    }

    fn sort(&mut self, column: SortColumn) {
        self.chosen.clear();
        let path = self.selected_row().map(|row| row.path.clone());
        self.sort = match self.sort {
            Some((sorted, descending)) if sorted == column => Some((column, !descending)),
            _ => Some((column, false)),
        };
        if let Some(collection) = &mut self.collection {
            collection.sort(self.sort);
        }
        // The same row stays selected in the folder; the collection answers later and keeps
        // the place of the selection.
        if !self.showing_library {
            self.selected = path
                .and_then(|path| self.shown().iter().position(|row| row.path == path))
                .or(self.selected);
        }
    }

    fn search(&mut self, text: &str) {
        if self.search == text {
            return;
        }
        text.clone_into(&mut self.search);
        if let Some(collection) = &mut self.collection {
            collection.search(text);
        }
        self.select_first();
    }

    /// The commands that change what the tree and the list show, not what they hold.
    fn arrange(&mut self, command: &AppCommand) {
        match command {
            AppCommand::BrowserColumnToggle { column } => {
                let mut columns = self.columns.clone();
                if let Some(at) = columns.iter().position(|shown| shown == column) {
                    columns.remove(at);
                } else {
                    columns.push(*column);
                }
                self.set_columns(&columns);
            }
            AppCommand::BrowserFolderToggle { folder } => {
                if !self.closed_folders.remove(folder) {
                    self.closed_folders.insert(*folder);
                }
            }
            AppCommand::BrowserHistoryToggle => self.history_closed = !self.history_closed,
            _ => {}
        }
    }

    /// The commands of the menu of a history.
    fn history(&mut self, command: &AppCommand) {
        let Some(collection) = &mut self.collection else {
            return;
        };
        match command {
            AppCommand::BrowserHistoryToPlaylist { session } => {
                collection.history_to_playlist(*session);
            }
            AppCommand::BrowserHistoryExport { session, path } => {
                collection.export_history(*session, path.clone());
            }
            AppCommand::BrowserHistoryDelete { session } => collection.delete_history(*session),
            _ => {}
        }
    }

    /// The commands of a smart playlist.
    fn smart(&mut self, command: &AppCommand) {
        let Some(collection) = &mut self.collection else {
            return;
        };
        match command {
            AppCommand::BrowserSmartRulesSet { smart, rules } => {
                collection.set_rules(*smart, rules.clone());
            }
            AppCommand::BrowserSmartToPlaylist { smart } => collection.smart_to_playlist(*smart),
            AppCommand::BrowserSmartDuplicate { smart } => collection.duplicate_smart(*smart),
            _ => {}
        }
    }

    /// The commands of the maintenance of the collection, which run on the worker of the library.
    fn maintain(&mut self, command: &AppCommand) {
        let Some(collection) = &mut self.collection else {
            return;
        };
        match command {
            AppCommand::LibraryMaintenanceOpen => collection.list_missing(),
            AppCommand::LibraryCheck => collection.check(),
            AppCommand::LibraryRelocate { path } => collection.relocate(path.clone()),
            AppCommand::LibraryMissingRemove => collection.remove_missing(),
            _ => {}
        }
    }

    /// Adds the selected track of the collection, a playlist or a history to the preparation
    /// list.
    fn prepare_selected(&mut self, place: Place) {
        if let Some(collection) = &mut self.collection
            && self.showing_library
            && let Some(track) = self.selected.and_then(|index| collection.track(index))
        {
            collection.prepare(track, place);
        }
    }

    /// The commands that change the selected track of the collection, a playlist or a history.
    fn change_selected(&mut self, command: &AppCommand) {
        let Some((collection, track)) = self.selected_track() else {
            return;
        };
        match command {
            AppCommand::BrowserTrackDelete => collection.delete_track(track),
            AppCommand::BrowserPlayedReset => collection.reset_played(track),
            AppCommand::BrowserTrackColourSet { colour } => collection.set_colour(track, *colour),
            AppCommand::BrowserTrackRatingSet { rating } => collection.set_rating(track, *rating),
            _ => {}
        }
    }

    /// The tracks of the library the selected rows hold, each once, in the order of their files.
    fn selected_tracks(&self) -> Vec<TrackId> {
        let Some(collection) = self.collection.as_ref().filter(|_| self.showing_library) else {
            return Vec::new();
        };
        let rows = collection.rows();
        let mut tracks: Vec<(&Path, TrackId)> = Vec::new();
        for index in self.selection() {
            if let (Some(row), Some(track)) = (rows.get(index), collection.track(index))
                && !tracks.iter().any(|(_, seen)| *seen == track)
            {
                tracks.push((&row.path, track));
            }
        }
        // By file, as the dialog of the details goes through them: a save that sorts the list
        // again does not change which track a position names.
        tracks.sort_by(|a, b| a.0.cmp(b.0));
        tracks.into_iter().map(|(_, track)| track).collect()
    }

    /// Imports the picture at `picture` as the cover of every selected track of the library,
    /// or deletes their covers. `docs/behavior/cover-art.md`, *Importing and deleting a
    /// cover*.
    fn set_cover(&mut self, picture: Option<PathBuf>) {
        let tracks = self.selected_tracks();
        if let Some(collection) = &mut self.collection
            && !tracks.is_empty()
        {
            collection.set_cover(tracks, picture);
        }
    }

    /// Stores the fields of the dialog of the details for the tracks at `position`: every
    /// selected track at 0, one of them from 1. `docs/behavior/browser.md`, *Track details*.
    fn save_details(&mut self, details: &TrackDetails, fields: DetailFields, position: usize) {
        let tracks = self.selected_tracks();
        let tracks: Vec<TrackId> = match position {
            0 => tracks,
            at => tracks.get(at - 1).copied().into_iter().collect(),
        };
        if let Some(collection) = &mut self.collection
            && !fields.is_empty()
            && !tracks.is_empty()
        {
            collection.save_details(tracks, details.clone(), fields);
        }
    }

    /// The library and the selected track of the collection, a playlist or a history.
    fn selected_track(&mut self) -> Option<(&mut Collection, TrackId)> {
        let selected = self.selected?;
        if !self.showing_library {
            return None;
        }
        let collection = self.collection.as_mut()?;
        let track = collection.track(selected)?;
        Some((collection, track))
    }

    /// Notes the track a deck has just loaded, for *Add as next to preparation list*.
    pub fn loaded(&mut self, path: PathBuf) {
        self.last_loaded = Some(path);
    }

    /// Records a track that has counted as played, when there is a library.
    pub fn played(&mut self, played: Played) {
        if let Some(collection) = &mut self.collection {
            collection.played(played);
        }
    }

    /// The tracks the decks hold, whose covers are read for their headers.
    pub fn set_deck_tracks(&mut self, tracks: [Option<PathBuf>; DeckId::COUNT]) {
        if tracks != self.deck_tracks {
            self.covers
                .want(tracks.iter().flatten().map(PathBuf::as_path));
            self.deck_tracks = tracks;
        }
    }

    /// Says how far the analysis of the collection has gone, and why it stopped if it did.
    /// A step of the analysis changes what the rows of the collection show.
    pub fn set_analysis(&mut self, status: Option<AnalysisStatus>, problem: Option<String>) {
        if status.is_some_and(|status| Some(status) != self.analysis)
            && let Some(collection) = &mut self.collection
            && self.showing_library
        {
            collection.refresh();
        }
        self.analysis = status;
        self.problem = problem;
    }

    /// What Platter writes into the files from now on.
    /// Shows `columns` besides the title, in their own order whatever the order given.
    pub fn set_columns(&mut self, columns: &[ListColumn]) {
        self.columns = ListColumn::ALL
            .into_iter()
            .filter(|column| columns.contains(column))
            .collect();
    }

    /// The columns the list shows besides the title.
    pub fn columns(&self) -> &[ListColumn] {
        &self.columns
    }

    pub fn set_tag_writing(&mut self, mode: TagWriting) {
        if let Some(collection) = &mut self.collection {
            collection.set_tag_writing(mode);
        }
    }

    /// Why files kept what they had since the last call.
    pub fn take_unwritten(&mut self) -> Vec<String> {
        self.collection
            .as_mut()
            .map(Collection::take_unwritten)
            .unwrap_or_default()
    }

    /// Keeps what a deck learned of the track at `path`, when there is a library.
    pub fn keep(&mut self, path: PathBuf, kept: Kept) {
        if let Some(collection) = &mut self.collection {
            collection.keep(path, kept);
        }
    }

    /// The folder the list shows.
    pub fn current(&self) -> Option<&Path> {
        self.current.as_deref()
    }

    /// Reads `folder` on a worker thread. `probe_files` also reads the header of each of
    /// its audio files, for the list.
    fn read(&self, folder: PathBuf, probe_files: bool) {
        let reports = self.reports.clone();
        let visit = Arc::clone(&self.visit);
        let started = visit.load(Ordering::Relaxed);
        let spawned = std::thread::Builder::new()
            .name("platter-files".to_owned())
            .spawn(move || {
                let Listing {
                    folders,
                    files,
                    pictures,
                } = list(&folder);
                let listed = Message::Listed {
                    folder,
                    folders,
                    files: files.clone(),
                    pictures,
                };
                if reports.send(listed).is_err() || !probe_files {
                    return;
                }
                for path in files {
                    // The user moved on: nobody wants the rest of this folder.
                    if visit.load(Ordering::Relaxed) != started {
                        return;
                    }
                    let kind = probe(&path);
                    let probed = Message::Probed {
                        visit: started,
                        path,
                        kind,
                    };
                    if reports.send(probed).is_err() {
                        return;
                    }
                }
            });
        // Without a thread the folder stays empty; the next visit tries again.
        drop(spawned);
    }

    /// Shows `folder` in the list and opens the tree down to it.
    pub fn navigate(&mut self, folder: PathBuf) {
        self.showing_library = false;
        self.visit.fetch_add(1, Ordering::Relaxed);
        self.rows.clear();
        self.selected = None;
        self.chosen.clear();
        self.reading = true;
        for ancestor in folder.ancestors().skip(1).take(MAX_DEPTH) {
            if self.open.insert(ancestor.to_path_buf()) && !self.folders.contains_key(ancestor) {
                self.read(ancestor.to_path_buf(), false);
            }
        }
        self.read(folder.clone(), true);
        self.pictures.clear();
        self.current = Some(folder);
    }

    fn toggle(&mut self, folder: PathBuf) {
        if !self.open.remove(&folder) {
            if !self.folders.contains_key(&folder) {
                self.read(folder.clone(), false);
            }
            self.open.insert(folder);
        }
    }

    /// Takes in what the workers have read since the last call. Returns whether anything
    /// arrived, so the application knows to draw again.
    pub fn poll(&mut self) -> bool {
        let mut changed = self.covers.poll();
        if let Some(collection) = &mut self.collection {
            let shown = collection.source();
            if collection.poll() {
                changed = true;
                if self.showing_library {
                    let count = collection.rows().len();
                    self.selected = match self.selected {
                        _ if count == 0 => None,
                        // The playlist shown was deleted: the collection starts at its top.
                        _ if collection.source() != shown => Some(0),
                        Some(index) => Some(index.min(count - 1)),
                        None => Some(0),
                    };
                }
            }
        }
        // Covers were written into files: every cover kept is read again when shown.
        if self
            .collection
            .as_mut()
            .is_some_and(Collection::take_covers_changed)
        {
            self.covers.forget();
            changed = true;
        }
        // A playlist made from the window is shown once it exists.
        if let Some(created) = self.collection.as_mut().and_then(Collection::take_created) {
            self.show_library(Source::Playlist(created));
        }
        while let Ok(message) = self.results.try_recv() {
            changed = true;
            match message {
                Message::Listed {
                    folder,
                    folders,
                    files,
                    pictures,
                } => {
                    if self.current.as_ref() == Some(&folder) {
                        self.reading = false;
                        self.pictures = pictures;
                        self.rows = folders
                            .iter()
                            .map(|path| (path, RowKind::Folder))
                            .chain(files.iter().map(|path| (path, RowKind::Pending)))
                            .map(|(path, kind)| FileRow {
                                path: path.clone(),
                                name: name_of(path),
                                kind,
                            })
                            .collect();
                        if !self.showing_library {
                            self.select_first();
                        }
                    }
                    self.counts.insert(folder.clone(), files.len());
                    self.folders.insert(folder, folders);
                }
                Message::Probed { visit, path, kind } => {
                    if visit == self.visit.load(Ordering::Relaxed)
                        && let Some(row) = self.rows.iter_mut().find(|row| row.path == path)
                    {
                        row.kind = kind;
                    }
                }
            }
        }
        // The covers of the tracks the list now shows, from its top.
        let tracks: Vec<PathBuf> = self
            .shown()
            .into_iter()
            .filter(|row| matches!(row.kind, RowKind::Track(_) | RowKind::Pending))
            .take(COVERS_PER_LIST)
            .map(|row| row.path.clone())
            .collect();
        self.covers.want(tracks.iter().map(PathBuf::as_path));
        // And those of the decks, which a cover written or too many kept made it drop.
        self.covers
            .want(self.deck_tracks.iter().flatten().map(PathBuf::as_path));
        changed
    }

    /// Applies a command of the file selector. A command that loads a track answers with
    /// the command for the engine; the others change what the selector shows.
    pub fn apply(&mut self, command: &AppCommand) -> Option<Action> {
        match command {
            AppCommand::BrowserNavigate { path } => self.navigate(path.clone()),
            AppCommand::BrowserToggle { path } => self.toggle(path.clone()),
            AppCommand::BrowserUp if !self.showing_library => {
                let parent = self.current.as_deref().and_then(Path::parent);
                // Above a root there is nothing to show.
                let inside_root =
                    |parent: &Path| self.roots.iter().any(|root| parent.starts_with(&root.path));
                if let Some(parent) = parent.filter(|parent| inside_root(parent)) {
                    self.navigate(parent.to_path_buf());
                }
            }
            AppCommand::BrowserSelect { .. }
            | AppCommand::BrowserSelectToggle { .. }
            | AppCommand::BrowserSelectExtend { .. }
            | AppCommand::BrowserSelectAll => self.select(command),
            AppCommand::BrowserMove { delta } => self.move_selection(*delta),
            AppCommand::BrowserTrackDetailsSave {
                details,
                fields,
                position,
                ..
            } => self.save_details(details, *fields, *position),
            AppCommand::BrowserTrackCoverImport { path } => self.set_cover(Some(path.clone())),
            AppCommand::BrowserTrackCoverDelete => self.set_cover(None),
            AppCommand::BrowserActivate => {
                if let Some(row) = self.selected_row()
                    && matches!(row.kind, RowKind::Folder)
                {
                    self.navigate(row.path.clone());
                }
            }
            AppCommand::BrowserLoadSelected { deck } => return self.load_selected(*deck),
            AppCommand::PreviewLoadSelected => {
                let row = self.selected_row()?;
                return match row.kind {
                    RowKind::Folder => None,
                    _ => Some(Action::Engine(Command::PreviewLoad {
                        path: row.path.clone(),
                    })),
                };
            }
            AppCommand::BrowserNodeSelect {
                node: BrowserNode::Collection,
            } => self.show_library(Source::Collection),
            AppCommand::BrowserNodeSelect {
                node: BrowserNode::Preparation,
            } => {
                if let Some(list) = self.collection.as_ref().and_then(Collection::preparation) {
                    self.show_library(Source::Playlist(list));
                }
            }
            AppCommand::BrowserNodeSelect {
                node: BrowserNode::History,
            } => {
                // Nothing to show until the library has opened the session of this run.
                if let Some(session) = self
                    .collection
                    .as_ref()
                    .and_then(Collection::current_session)
                {
                    self.show_library(Source::History(session));
                }
            }
            AppCommand::BrowserHistorySelect { session } => {
                self.show_library(Source::History(*session));
            }
            AppCommand::BrowserPlaylistSelect { playlist } => {
                self.show_library(Source::Playlist(*playlist));
            }
            AppCommand::BrowserSearchSet { text } => self.search(text),
            AppCommand::BrowserSort { column } => self.sort(*column),
            AppCommand::BrowserColumnToggle { .. }
            | AppCommand::BrowserFolderToggle { .. }
            | AppCommand::BrowserHistoryToggle => self.arrange(command),
            AppCommand::LibraryImportFolder { path } => {
                if let Some(collection) = &mut self.collection {
                    collection.import(path.clone());
                }
            }
            AppCommand::LibraryImportStop => {
                if let Some(collection) = &mut self.collection {
                    collection.stop_import();
                }
            }
            AppCommand::BrowserPlaylistCreate { folder, item, name } => {
                if let Some(collection) = &mut self.collection {
                    let kind = match item {
                        NewItem::Playlist => NodeKind::Playlist,
                        NewItem::Folder => NodeKind::Folder,
                        NewItem::Smart => NodeKind::Smart,
                    };
                    collection.create(*folder, kind, name.clone());
                }
            }
            AppCommand::BrowserPlaylistRename { item, name } => {
                if let Some(collection) = &mut self.collection {
                    collection.rename(*item, name.clone());
                }
            }
            AppCommand::BrowserPlaylistMove { item, folder } => {
                if let Some(collection) = &mut self.collection {
                    collection.move_to(*item, *folder);
                }
            }
            AppCommand::BrowserPlaylistAdd { playlist, path } => {
                if let Some(collection) = &mut self.collection {
                    collection.add(*playlist, path.clone());
                }
            }
            AppCommand::BrowserPlaylistDelete { playlist } => {
                if let Some(collection) = &mut self.collection {
                    collection.delete(*playlist);
                }
            }
            AppCommand::BrowserPlaylistClear { playlist } => {
                if let Some(collection) = &mut self.collection {
                    collection.clear(*playlist);
                }
            }
            AppCommand::BrowserPlaylistRemoveRepeats { playlist } => {
                if let Some(collection) = &mut self.collection {
                    collection.remove_repeats(*playlist);
                }
            }
            AppCommand::BrowserPlaylistRemove => self.remove_selected(),
            AppCommand::BrowserPreparationSelect { playlist } => {
                if let Some(collection) = &mut self.collection {
                    collection.set_preparation(*playlist);
                }
            }
            AppCommand::BrowserPreparationAppend => self.prepare_selected(Place::End),
            AppCommand::BrowserFavouriteSet { place, playlist } => {
                if let Some(collection) = &mut self.collection {
                    collection.set_favourite(*place, *playlist);
                }
            }
            AppCommand::BrowserFavouriteClear { place } => {
                if let Some(collection) = &mut self.collection {
                    collection.clear_favourite(*place);
                }
            }
            AppCommand::BrowserPreparationNext => {
                self.prepare_selected(Place::Next(self.last_loaded.clone()));
            }
            AppCommand::BrowserPlaylistExport { playlist, path } => {
                if let Some(collection) = &mut self.collection {
                    collection.export(*playlist, path.clone());
                }
            }
            AppCommand::BrowserSmartRulesSet { .. }
            | AppCommand::BrowserSmartToPlaylist { .. }
            | AppCommand::BrowserSmartDuplicate { .. } => self.smart(command),
            AppCommand::BrowserHistoryToPlaylist { .. }
            | AppCommand::BrowserHistoryExport { .. }
            | AppCommand::BrowserHistoryDelete { .. } => self.history(command),
            AppCommand::BrowserPlaylistImport { folder, path } => {
                if let Some(collection) = &mut self.collection {
                    collection.import_playlists(*folder, path.clone());
                }
            }
            AppCommand::BrowserCollectionImport { path } => {
                if let Some(collection) = &mut self.collection {
                    collection.import_collections(path.clone());
                }
            }
            AppCommand::LibraryMaintenanceOpen
            | AppCommand::LibraryCheck
            | AppCommand::LibraryRelocate { .. }
            | AppCommand::LibraryMissingRemove => self.maintain(command),
            AppCommand::BrowserTrackDelete
            | AppCommand::BrowserPlayedReset
            | AppCommand::BrowserTrackColourSet { .. }
            | AppCommand::BrowserTrackRatingSet { .. } => self.change_selected(command),
            _ => {}
        }
        None
    }

    /// Takes the selected entry out of the playlist the list shows. The selection stays on
    /// its place when the answer comes, which [`FileBrowser::poll`] keeps within the rows.
    fn remove_selected(&mut self) {
        let Some(collection) = &mut self.collection else {
            return;
        };
        if !self.showing_library {
            return;
        }
        // The entry, not the track: a track in the playlist twice has two entries.
        if let (Some(playlist), Some(entry)) = (
            collection.playlist(),
            self.selected.and_then(|index| collection.entry(index)),
        ) {
            collection.remove(playlist, entry);
        }
    }

    fn selected_row(&self) -> Option<&FileRow> {
        self.shown().get(self.selected?).copied()
    }

    fn move_selection(&mut self, delta: i32) {
        let count = self.shown().len();
        if count == 0 {
            return;
        }
        let last = count - 1;
        let from = self.selected.unwrap_or(0) as i64;
        let to = (from + i64::from(delta)).clamp(0, last as i64);
        self.chosen.clear();
        self.selected = Some(to as usize);
    }

    fn load_selected(&self, deck: DeckId) -> Option<Action> {
        let row = self.selected_row()?;
        match row.kind {
            RowKind::Folder => None,
            // A file that cannot be read is sent anyway: the deck says why it fails.
            _ => Some(Action::Engine(Command::DeckLoad {
                deck,
                path: row.path.clone(),
            })),
        }
    }

    fn tree_rows(&self, folder: &Path, name: String, depth: usize, rows: &mut Vec<TreeRow>) {
        let open = self.open.contains(folder);
        rows.push(TreeRow {
            path: folder.to_path_buf(),
            name,
            depth,
            open,
            current: !self.showing_library && self.current.as_deref() == Some(folder),
            files: self.counts.get(folder).copied(),
        });
        if !open || depth >= MAX_DEPTH {
            return;
        }
        for child in self.folders.get(folder).into_iter().flatten() {
            self.tree_rows(child, name_of(child), depth + 1, rows);
        }
    }

    /// Gives the root at `path` a new name, as when the language of the interface changes
    /// and the home folder is named in the new one.
    pub fn rename_root(&mut self, path: &Path, name: &str) {
        for root in self.roots.iter_mut().filter(|root| root.path == path) {
            name.clone_into(&mut root.name);
        }
    }

    /// What the browser strip draws.
    pub fn view(&self) -> BrowserView {
        let mut tree = Vec::new();
        for root in &self.roots {
            self.tree_rows(&root.path, root.name.clone(), 0, &mut tree);
        }
        // The path from the root that holds the current folder down to it.
        let crumbs = self.current.as_deref().map_or_else(Vec::new, |current| {
            let root = self
                .roots
                .iter()
                .filter(|root| current.starts_with(&root.path))
                .max_by_key(|root| root.path.components().count());
            match root {
                Some(root) => {
                    let below = current.strip_prefix(&root.path).unwrap_or(current);
                    std::iter::once(root.name.clone())
                        .chain(
                            below
                                .components()
                                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
                        )
                        .collect()
                }
                None => vec![name_of(current)],
            }
        });
        BrowserView {
            collection: self
                .collection
                .as_ref()
                .map(|collection| collection.view(self.showing_library)),
            tree,
            folder: self.current.clone().filter(|_| !self.showing_library),
            explorer: self.current.clone(),
            pictures: self.pictures.clone(),
            closed_folders: self.closed_folders.iter().copied().collect(),
            history_open: !self.history_closed,
            recordings: self.recordings.clone(),
            // The collection and the playlists name themselves; the panel draws their names.
            crumbs: if self.showing_library {
                Vec::new()
            } else {
                crumbs
            },
            rows: self.shown().into_iter().cloned().collect(),
            covers: self
                .shown()
                .into_iter()
                .take(COVERS_PER_LIST)
                .filter_map(|row| Some((row.path.clone(), self.covers.get(&row.path)?.clone())))
                .collect(),
            search: self.search.clone(),
            sort: self.sort,
            columns: self.columns.clone(),
            selected: self.selected,
            marked: self.selection(),
            reading: match &self.collection {
                Some(collection) if self.showing_library => collection.busy(),
                _ => self.reading,
            },
            analysis: self.analysis,
            problem: self.problem.clone(),
            deck_covers: self.deck_tracks.each_ref().map(|path| {
                path.as_ref()
                    .and_then(|path| self.covers.get(path))
                    .cloned()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is_audio;

    #[test]
    fn audio_files_are_known_by_their_extension_in_any_case() {
        for name in ["a.mp3", "a.FLAC", "a.Ogg", "dir/a.aiff", "a.m4a"] {
            assert!(is_audio(Path::new(name)), "{name}");
        }
        for name in ["a.txt", "a", "mp3", "a.wma", "a.wav.bak"] {
            assert!(!is_audio(Path::new(name)), "{name}");
        }
    }
}
