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
    Library, LibraryError, Page, Query, Reading, TagReader, Timestamp, Track, TrackTags,
    import_folder,
};

use crate::files::is_audio;
use crate::model::{CollectionView, FileRow, ImportOutcome, RowKind, TrackRow};

enum Request {
    Read,
    Import(PathBuf),
}

enum Reply {
    Unavailable,
    Page(Page),
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
    while let Ok(request) = requests.recv() {
        if let Request::Import(folder) = request {
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
        // Reads that piled up behind a long import are answered once.
        let reply = match library.search(&Query::default()) {
            Ok(page) => Reply::Page(page),
            Err(_) => Reply::Unavailable,
        };
        if replies.send(reply).is_err() {
            return;
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
    total: Option<usize>,
    importing: bool,
    imported: Option<ImportOutcome>,
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
            total: None,
            importing: false,
            imported: None,
        };
        collection.send(Request::Read);
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

    pub fn can_import(&self) -> bool {
        !self.importing && self.available == Some(true)
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
                    self.total = None;
                }
                Reply::Page(page) => {
                    // The worker reads the collection again after an import: the import is
                    // over once the list shows what it added.
                    self.importing = false;
                    self.available = Some(true);
                    self.total = Some(page.total);
                    self.rows = page.hits.into_iter().map(|hit| row_of(hit.track)).collect();
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

    /// The library is opening or importing: answers are still to come.
    pub fn busy(&self) -> bool {
        self.available.is_none() || self.importing
    }

    pub fn view(&self, current: bool) -> CollectionView {
        CollectionView {
            available: self.available,
            tracks: self.total,
            current,
            importing: self.importing,
            imported: self.imported,
        }
    }
}
