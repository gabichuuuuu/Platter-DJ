//! The covers of the tracks of the list, read on a thread of their own.
//!
//! Reading a cover opens the file and decodes a picture, so it never runs on the interface
//! thread. Behaviour note: `docs/behavior/cover-art.md`, *In the list of tracks*.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use platter_decode::cover_file;

use crate::model::CoverImage;

/// Rows of a list whose covers are read, from its top.
pub const COVERS_PER_LIST: usize = 256;
/// Files whose covers are kept; past that the kept ones are dropped and read again when shown.
const MAX_KEPT: usize = 1024;

pub struct Covers {
    /// `None` once the thread has gone: nothing more is read.
    requests: Option<Sender<PathBuf>>,
    results: Receiver<(PathBuf, Option<CoverImage>)>,
    /// Files asked for, read or not yet; a file is asked for once while it is kept.
    asked: HashSet<PathBuf>,
    /// The covers read; `None` for a file without one.
    read: HashMap<PathBuf, Option<CoverImage>>,
}

impl Covers {
    pub fn start() -> Self {
        let (requests, waiting) = channel::<PathBuf>();
        let (answers, results) = channel();
        let spawned = std::thread::Builder::new()
            .name("platter-covers".to_owned())
            .spawn(move || {
                while let Ok(path) = waiting.recv() {
                    // A file that cannot be read shows the icon, as one without a cover.
                    let cover = cover_file(&path).ok().flatten().map(|cover| CoverImage {
                        width: cover.width,
                        height: cover.height,
                        rgba: Arc::from(cover.rgba),
                    });
                    if answers.send((path, cover)).is_err() {
                        return;
                    }
                }
            });
        Self {
            requests: spawned.is_ok().then_some(requests),
            results,
            asked: HashSet::new(),
            read: HashMap::new(),
        }
    }

    /// Asks for the covers of `paths` not asked for yet, in their order.
    pub fn want<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) {
        let Some(requests) = &self.requests else {
            return;
        };
        for path in paths {
            if self.asked.contains(path) {
                continue;
            }
            if self.asked.len() >= MAX_KEPT {
                self.asked.clear();
                self.read.clear();
            }
            if requests.send(path.to_path_buf()).is_err() {
                self.requests = None;
                return;
            }
            self.asked.insert(path.to_path_buf());
        }
    }

    /// Takes in the covers read since the last call. Returns whether any arrived.
    pub fn poll(&mut self) -> bool {
        let mut arrived = false;
        while let Ok((path, cover)) = self.results.try_recv() {
            // A cover asked for before the kept ones were dropped is of no use.
            if self.asked.contains(&path) {
                self.read.insert(path, cover);
                arrived = true;
            }
        }
        arrived
    }

    /// The cover of `path`, once it has been read and when the file has one.
    pub fn get(&self, path: &Path) -> Option<&CoverImage> {
        self.read.get(path).and_then(Option::as_ref)
    }

    /// Drops every cover kept, so each is read again when it is next asked for: covers were
    /// written into files. Dropping them all spares matching the paths of a file, which the
    /// list, the explorer and the decks may spell differently.
    pub fn forget(&mut self) {
        self.asked.clear();
        self.read.clear();
    }

    /// Whether every file asked for has been read.
    pub fn idle(&self) -> bool {
        self.asked.len() == self.read.len()
    }
}
