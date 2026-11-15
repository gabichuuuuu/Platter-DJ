//! The collection behind the browser: the library runs on a worker thread of its own, and
//! the browser sends it what to read or import and takes in the answers.
//!
//! Every call of the library may touch the disk, and an import may take minutes, so none
//! runs on the interface thread. Behaviour note: `docs/behavior/browser.md`, *The collection*.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use platter_decode::probe_file;
use platter_library::{
    Column, Library, LibraryError, Page, Query, Reading, TagReader, Timestamp, Track, TrackTags,
    import_folder,
};

use crate::command::SortColumn;
use crate::files::is_audio;
use crate::model::{CollectionView, FileRow, ImportOutcome, RowKind, TrackRow};

/// What the list asks of the collection: a search text, where an empty one matches every
/// track, and the column it is sorted by with whether the order is turned round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Lookup {
    text: String,
    sort: Option<(SortColumn, bool)>,
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
                },
                descending,
            ),
            None => (Column::Order, false),
        };
        Query {
            text: self.text.clone(),
            column,
            descending,
            ..Query::default()
        }
    }
}

enum Request {
    Read(Lookup),
    Import(PathBuf),
}

enum Reply {
    Unavailable,
    /// The tracks found for a lookup.
    Page(Lookup, Page),
    Imported(ImportOutcome),
}

/// Reads the tags of a file the way the file list does: from its header, without decoding.
struct HeaderReader;

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
                genre: None,
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

/// Serves the requests of the browser until it goes away.
fn serve(library: &mut dyn Library, requests: &Receiver<Request>, replies: &Sender<Reply>) {
    let mut lookup = Lookup::default();
    while let Ok(first) = requests.recv() {
        // Of the searches waiting, typed while the library was busy, only the last one
        // matters; imports all run, in order.
        let mut imports = Vec::new();
        for request in std::iter::once(first).chain(requests.try_iter()) {
            match request {
                Request::Read(newer) => lookup = newer,
                Request::Import(folder) => imports.push(folder),
            }
        }
        let imported = !imports.is_empty();
        for folder in imports {
            let outcome = match import_folder(library, &folder, &mut HeaderReader, now()) {
                Ok(report) => ImportOutcome::Done {
                    added: report.added.len(),
                    known: report.known,
                    unreadable: report.unreadable,
                },
                Err(_) => ImportOutcome::Failed,
            };
            if replies.send(Reply::Imported(outcome)).is_err() {
                return;
            }
        }
        // After an import the size of the whole collection changed too, which only an
        // empty search tells.
        let mut lookups = vec![lookup.clone()];
        if imported && !lookup.text.trim().is_empty() {
            let everything = Lookup {
                text: String::new(),
                ..lookup.clone()
            };
            lookups.insert(0, everything);
        }
        for lookup in lookups {
            let reply = match library.search(&lookup.query()) {
                Ok(page) => Reply::Page(lookup, page),
                Err(_) => Reply::Unavailable,
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
    }
}

fn row_of(track: Track) -> FileRow {
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
    /// Tracks that matched the search, beyond the rows too.
    matched: Option<usize>,
    /// Tracks in the whole collection, as the last answer to an empty search said.
    total: Option<usize>,
    importing: bool,
    imported: Option<ImportOutcome>,
    /// What the rows are for, or will be once its answer comes.
    lookup: Lookup,
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
        let spawned = std::thread::Builder::new()
            .name("platter-library".to_owned())
            .spawn(move || match open() {
                Ok(mut library) => serve(library.as_mut(), &waiting, &answers),
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
            matched: None,
            total: None,
            importing: false,
            imported: None,
            lookup: Lookup::default(),
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
        self.send(Request::Import(folder));
    }

    /// Asks for the tracks that match `text`; the rows change when the answer comes.
    pub fn search(&mut self, text: &str) {
        let lookup = Lookup {
            text: text.to_owned(),
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    /// Asks for the tracks in another order; the rows change when the answer comes.
    pub fn sort(&mut self, sort: Option<(SortColumn, bool)>) {
        let lookup = Lookup {
            sort,
            ..self.lookup.clone()
        };
        self.ask(lookup);
    }

    fn ask(&mut self, lookup: Lookup) {
        if self.lookup == lookup {
            return;
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
                    self.rows.clear();
                    self.matched = None;
                    self.total = None;
                }
                Reply::Page(lookup, page) => {
                    // The worker reads the collection again after an import: the import is
                    // over once the list shows what it added.
                    self.importing = false;
                    self.available = Some(true);
                    if lookup.text.trim().is_empty() {
                        self.total = Some(page.total);
                    }
                    // An answer to a lookup the user has since changed is of no use.
                    if lookup == self.lookup {
                        self.matched = Some(page.total);
                        self.rows = page.hits.into_iter().map(|hit| row_of(hit.track)).collect();
                    }
                }
                Reply::Imported(outcome) => self.imported = Some(outcome),
            }
        }
        changed
    }

    /// The tracks of the collection, sorted by path.
    pub fn rows(&self) -> &[FileRow] {
        &self.rows
    }

    /// The library is opening, importing or searching: answers are still to come.
    pub fn busy(&self) -> bool {
        self.available.is_none() || self.importing || self.pending_search()
    }

    fn pending_search(&self) -> bool {
        self.available == Some(true) && self.matched.is_none()
    }

    pub fn view(&self, current: bool) -> CollectionView {
        CollectionView {
            available: self.available,
            tracks: self.total,
            matched: self.matched,
            current,
            importing: self.importing,
            imported: self.imported,
        }
    }
}
