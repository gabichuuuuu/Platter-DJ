//! The jobs of stem separation of the application: installing the model from its original
//! archive and separating one track at a time, each on a worker thread of its own.
//!
//! Behaviour note: `docs/behavior/stems.md`, *Separation*; the choices:
//! `docs/decisions/0038-stem-separation.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use platter_engine::separation::separate_track;
use platter_engine::{install_spleeter, load_spleeter};
use platter_library::TrackId;
use platter_ui_egui::model::SeparationStatus;

/// The name of the archive of the model, as its release publishes it.
pub const ARCHIVE_NAME: &str = "4stems.tar.gz";
const MODEL_FILE: &str = "separation.weights";
/// The archive is about 146 MB; anything far larger is not it.
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

struct Running {
    track: TrackId,
    name: String,
    /// The share done, as the bits of an `f32`.
    done: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<PathBuf, String>>,
}

/// What a job that ended gave.
pub enum Finished {
    Separated { track: TrackId, folder: PathBuf },
    Failed(String),
    Installed,
    InstallFailed(String),
}

#[derive(Default)]
pub struct Jobs {
    /// The folder of the settings; without one nothing is installed nor separated.
    root: Option<PathBuf>,
    running: Option<Running>,
    installing: Option<Receiver<Result<(), String>>>,
}

/// A folder name of its own for the stems of each track: the name of its file and a digest of
/// its path, so two files of one name in two folders do not share their stems.
fn folder_name(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    let stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let clean: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    format!("{}-{hash:016x}", clean.trim())
}

impl Jobs {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            running: None,
            installing: None,
        }
    }

    fn model_path(&self) -> Option<PathBuf> {
        self.root
            .as_ref()
            .map(|root| root.join("Models").join(MODEL_FILE))
    }

    pub fn model_installed(&self) -> bool {
        self.model_path().is_some_and(|path| path.is_file())
    }

    /// Installs the model from `4stems.tar.gz` in `folder`, or from `folder` when it is that
    /// file itself.
    pub fn install(&mut self, folder: &Path) -> Result<(), String> {
        if self.installing.is_some() {
            return Err("the model is being installed already".to_owned());
        }
        let target = self
            .model_path()
            .ok_or("there is no folder for the settings")?;
        let archive = if folder.is_dir() {
            folder.join(ARCHIVE_NAME)
        } else {
            folder.to_path_buf()
        };
        let (send, receive) = channel();
        std::thread::Builder::new()
            .name("platter-model".to_owned())
            .spawn(move || {
                let outcome = (|| {
                    let size = std::fs::metadata(&archive)
                        .map_err(|error| format!("{}: {error}", archive.display()))?
                        .len();
                    if size > MAX_ARCHIVE_BYTES {
                        return Err(format!(
                            "{} is not the archive of the model",
                            archive.display()
                        ));
                    }
                    let bytes = std::fs::read(&archive).map_err(|error| error.to_string())?;
                    let weights = install_spleeter(&bytes).map_err(|error| error.to_string())?;
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    // Written beside and renamed, so a half file is never taken for the model.
                    let partial = target.with_extension("partial");
                    std::fs::write(&partial, weights).map_err(|error| error.to_string())?;
                    std::fs::rename(&partial, &target).map_err(|error| error.to_string())
                })();
                let _ = send.send(outcome);
            })
            .map_err(|error| error.to_string())?;
        self.installing = Some(receive);
        Ok(())
    }

    /// Separates the track at `path` into stems, unless another separation runs.
    pub fn start(&mut self, track: TrackId, path: PathBuf, name: String) -> Result<(), String> {
        if self.running.is_some() {
            return Err("another track is being separated".to_owned());
        }
        let (Some(model), Some(root)) = (self.model_path(), self.root.clone()) else {
            return Err("the separation model is not installed".to_owned());
        };
        let folder = root.join("Stems").join(folder_name(&path));
        let done = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, receive) = channel();
        let (worker_done, worker_cancel) = (Arc::clone(&done), Arc::clone(&cancel));
        // Half the processors, so the audio and the window keep theirs.
        let threads =
            std::thread::available_parallelism().map_or(2, |count| (count.get() / 2).max(1));
        std::thread::Builder::new()
            .name("platter-separation".to_owned())
            .spawn(move || {
                let outcome = (|| {
                    let bytes = std::fs::read(&model).map_err(|error| error.to_string())?;
                    let model =
                        load_spleeter(&bytes, threads).map_err(|error| error.to_string())?;
                    separate_track(&path, &folder, &model, &mut |share| {
                        worker_done.store(share.to_bits(), Ordering::Relaxed);
                        !worker_cancel.load(Ordering::Relaxed)
                    })
                    .map_err(|error| error.to_string())?;
                    Ok(folder)
                })();
                let _ = send.send(outcome);
            })
            .map_err(|error| error.to_string())?;
        self.running = Some(Running {
            track,
            name,
            done,
            cancel,
            result: receive,
        });
        Ok(())
    }

    pub fn cancel(&self) {
        if let Some(running) = &self.running {
            running.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn status(&self) -> Option<SeparationStatus> {
        self.running.as_ref().map(|running| SeparationStatus {
            track: running.name.clone(),
            done: f32::from_bits(running.done.load(Ordering::Relaxed)).clamp(0.0, 1.0),
        })
    }

    /// The jobs that ended since the last call.
    pub fn poll(&mut self) -> Vec<Finished> {
        let mut finished = Vec::new();
        if let Some(running) = &self.running {
            match running.result.try_recv() {
                Ok(Ok(folder)) => finished.push(Finished::Separated {
                    track: running.track,
                    folder,
                }),
                Ok(Err(why)) => finished.push(Finished::Failed(why)),
                Err(TryRecvError::Disconnected) => {
                    finished.push(Finished::Failed("the separation stopped".to_owned()));
                }
                Err(TryRecvError::Empty) => {}
            }
            if !finished.is_empty() {
                self.running = None;
            }
        }
        if let Some(installing) = &self.installing {
            let outcome = match installing.try_recv() {
                Ok(Ok(())) => Some(Finished::Installed),
                Ok(Err(why)) => Some(Finished::InstallFailed(why)),
                Err(TryRecvError::Disconnected) => Some(Finished::InstallFailed(
                    "the installation stopped".to_owned(),
                )),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(outcome) = outcome {
                finished.push(outcome);
                self.installing = None;
            }
        }
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_track_gets_a_folder_of_its_own() {
        let a = folder_name(Path::new("/music/a/Song: One.mp3"));
        let b = folder_name(Path::new("/music/b/Song: One.mp3"));
        assert!(a.starts_with("Song_ One-") && a != b, "{a} {b}");
    }

    #[test]
    fn nothing_is_installed_without_a_folder_for_the_settings() {
        let mut jobs = Jobs::new(None);
        assert!(!jobs.model_installed());
        assert!(jobs.install(Path::new("/nowhere")).is_err());
        assert!(jobs.poll().is_empty());
    }
}
