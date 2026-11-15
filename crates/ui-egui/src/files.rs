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

use crate::collection::Collection;
use crate::command::{Action, AppCommand, BrowserNode};
use crate::model::{BrowserView, FileRow, RowKind, TrackRow, TreeRow};

/// Extensions of the formats Platter plays. The content decides whether a file really is
/// one; the extension only decides whether it is listed.
const AUDIO_EXTENSIONS: [&str; 9] = [
    "mp3", "m4a", "mp4", "wav", "aif", "aiff", "flac", "ogg", "oga",
];
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

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The folders and the audio files of `folder`, each sorted by name without regard to
/// case. Hidden entries and what cannot be read are left out.
fn list(folder: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut folders, mut files) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(folder) else {
        return (folders, files);
    };
    for entry in entries.flatten().take(MAX_ENTRIES) {
        let path = entry.path();
        if name_of(&path).starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => folders.push(path),
            Ok(kind) if kind.is_file() && is_audio(&path) => files.push(path),
            _ => {}
        }
    }
    let key = |path: &PathBuf| name_of(path).to_lowercase();
    folders.sort_by_key(key);
    files.sort_by_key(key);
    (folders, files)
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
    rows: Vec<FileRow>,
    selected: Option<usize>,
    reading: bool,
    /// Counts the folders shown, so results for a folder the user has left are dropped and
    /// the worker that reads them stops.
    visit: Arc<AtomicU64>,
    results: Receiver<Message>,
    reports: Sender<Message>,
    /// The collection node of the tree, when the browser has a library.
    collection: Option<Collection>,
    /// The list shows the collection instead of the current folder.
    showing_collection: bool,
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
            rows: Vec::new(),
            selected: None,
            reading: false,
            visit: Arc::new(AtomicU64::new(0)),
            results,
            reports,
            collection: None,
            showing_collection: false,
        }
    }

    /// Gives the tree a collection node, above the folders.
    pub fn attach(&mut self, collection: Collection) {
        self.collection = Some(collection);
    }

    /// Answers are still to come from the library: the window draws again to take them.
    pub fn collection_busy(&self) -> bool {
        self.collection.as_ref().is_some_and(Collection::busy)
    }

    fn show_collection(&mut self) {
        if self.collection.is_none() || self.showing_collection {
            return;
        }
        self.showing_collection = true;
        self.selected = self
            .collection
            .as_ref()
            .and_then(|collection| (!collection.rows().is_empty()).then_some(0));
    }

    /// The rows the list shows: those of the collection or those of the current folder.
    fn shown(&self) -> &[FileRow] {
        match &self.collection {
            Some(collection) if self.showing_collection => collection.rows(),
            _ => &self.rows,
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
                let (folders, files) = list(&folder);
                let listed = Message::Listed {
                    folder,
                    folders,
                    files: files.clone(),
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
        self.showing_collection = false;
        self.visit.fetch_add(1, Ordering::Relaxed);
        self.rows.clear();
        self.selected = None;
        self.reading = true;
        for ancestor in folder.ancestors().skip(1).take(MAX_DEPTH) {
            if self.open.insert(ancestor.to_path_buf()) && !self.folders.contains_key(ancestor) {
                self.read(ancestor.to_path_buf(), false);
            }
        }
        self.read(folder.clone(), true);
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
        let mut changed = false;
        if let Some(collection) = &mut self.collection
            && collection.poll()
        {
            changed = true;
            if self.showing_collection {
                let count = collection.rows().len();
                self.selected = match self.selected {
                    _ if count == 0 => None,
                    Some(index) => Some(index.min(count - 1)),
                    None => Some(0),
                };
            }
        }
        while let Ok(message) = self.results.try_recv() {
            changed = true;
            match message {
                Message::Listed {
                    folder,
                    folders,
                    files,
                } => {
                    if self.current.as_ref() == Some(&folder) {
                        self.reading = false;
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
                        if !self.showing_collection {
                            self.selected = (!self.rows.is_empty()).then_some(0);
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
        changed
    }

    /// Applies a command of the file selector. A command that loads a track answers with
    /// the command for the engine; the others change what the selector shows.
    pub fn apply(&mut self, command: &AppCommand) -> Option<Action> {
        match command {
            AppCommand::BrowserNavigate { path } => self.navigate(path.clone()),
            AppCommand::BrowserToggle { path } => self.toggle(path.clone()),
            AppCommand::BrowserUp if !self.showing_collection => {
                let parent = self.current.as_deref().and_then(Path::parent);
                // Above a root there is nothing to show.
                let inside_root =
                    |parent: &Path| self.roots.iter().any(|root| parent.starts_with(&root.path));
                if let Some(parent) = parent.filter(|parent| inside_root(parent)) {
                    self.navigate(parent.to_path_buf());
                }
            }
            AppCommand::BrowserSelect { index } => {
                if *index < self.shown().len() {
                    self.selected = Some(*index);
                }
            }
            AppCommand::BrowserMove { delta } => self.move_selection(*delta),
            AppCommand::BrowserActivate => {
                if let Some(row) = self.selected_row()
                    && matches!(row.kind, RowKind::Folder)
                {
                    self.navigate(row.path.clone());
                }
            }
            AppCommand::BrowserLoadSelected { deck } => return self.load_selected(*deck),
            AppCommand::BrowserNodeSelect {
                node: BrowserNode::Collection,
            } => self.show_collection(),
            AppCommand::LibraryImportFolder { path } => {
                if let Some(collection) = &mut self.collection {
                    collection.import(path.clone());
                }
            }
            _ => {}
        }
        None
    }

    fn selected_row(&self) -> Option<&FileRow> {
        self.shown().get(self.selected?)
    }

    fn move_selection(&mut self, delta: i32) {
        let count = self.shown().len();
        if count == 0 {
            return;
        }
        let last = count - 1;
        let from = self.selected.unwrap_or(0) as i64;
        let to = (from + i64::from(delta)).clamp(0, last as i64);
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
            current: !self.showing_collection && self.current.as_deref() == Some(folder),
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
                .map(|collection| collection.view(self.showing_collection)),
            tree,
            folder: self.current.clone().filter(|_| !self.showing_collection),
            // The collection names itself in the list; the panel draws its name.
            crumbs: if self.showing_collection {
                Vec::new()
            } else {
                crumbs
            },
            rows: self.shown().to_vec(),
            selected: self.selected,
            reading: match &self.collection {
                Some(collection) if self.showing_collection => collection.busy(),
                _ => self.reading,
            },
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
