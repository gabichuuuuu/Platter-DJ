//! The analysis of the tracks of the collection that no deck has analysed yet, one at a time
//! on a worker thread of its own, for the status bar of the browser.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Analysis in the background and keys*.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use platter_engine::{Analyser, PlatterAnalyser, SharedRange, TrackAnalysis, analyse_file};
use platter_library::{Library, LibraryError, SqliteLibrary, Track};

use platter_ui_egui::collection::{Kept, TagWriting, write_learned};
use platter_ui_egui::keys;

/// How long the worker rests when there is nothing to do, or while a deck loads.
const REST: Duration = Duration::from_millis(500);
/// Tracks asked of the library at a time.
const BATCH: usize = 16;

/// What the worker tells the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// `done` tracks of `total` analysed since the analysis started.
    Progress { done: usize, total: usize },
    /// Nothing is left to analyse.
    Idle,
    /// The library cannot be used; the analysis stops.
    Stopped(String),
    /// A file kept the tags it had: why.
    NotWritten(String),
}

/// What the window shares with the worker.
#[derive(Clone, Default)]
pub struct Shared {
    /// A deck is loading a track: the worker waits so the load is not slowed.
    pub pause: Arc<AtomicBool>,
    /// Platter is closing.
    pub stop: Arc<AtomicBool>,
    /// The tracks the decks hold, which the decks analyse themselves.
    pub held: Arc<Mutex<Vec<PathBuf>>>,
    /// What the analysis writes into the files.
    pub tag_writing: Arc<Mutex<TagWriting>>,
    /// The range of tempos of the preferences, which the decks read too.
    pub range: SharedRange,
}

/// Starts the worker on the library in `file`. Reports go to `reports`.
pub fn start(file: PathBuf, shared: Shared, reports: Sender<Report>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("platter-analysis".to_owned())
        .spawn(move || run(&file, &shared, &reports))
        .map(|_| ())
}

fn run(file: &std::path::Path, shared: &Shared, reports: &Sender<Report>) {
    let mut library = match SqliteLibrary::open(file) {
        Ok(library) => library,
        Err(error) => {
            let _ = reports.send(Report::Stopped(error.to_string()));
            return;
        }
    };
    let analyser = PlatterAnalyser::with_range(shared.range.clone());
    let mut skipped = HashSet::new();
    let mut done = 0;
    let mut idle = true;
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.pause.load(Ordering::Relaxed) {
            std::thread::sleep(REST);
            continue;
        }
        let held = shared
            .held
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default();
        let mode = shared
            .tag_writing
            .lock()
            .map(|mode| *mode)
            .unwrap_or(TagWriting::Nothing);
        match next(&mut library, &analyser, &held, &mut skipped, mode) {
            Ok(Some(written)) => {
                if let Err(why) = written {
                    let _ = reports.send(Report::NotWritten(why));
                }
                done += 1;
                idle = false;
                let left = library.count_unanalysed().unwrap_or(0);
                // The window may be gone; then nobody is left to tell.
                let _ = reports.send(Report::Progress {
                    done,
                    total: done + left.saturating_sub(skipped.len()),
                });
            }
            Ok(None) => {
                if !idle {
                    idle = true;
                    done = 0;
                    let _ = reports.send(Report::Idle);
                }
                // A deck lets go of its track, or an import brings new ones.
                skipped.clear();
                std::thread::sleep(REST);
            }
            Err(error) => {
                let _ = reports.send(Report::Stopped(error.to_string()));
                return;
            }
        }
    }
}

/// Analyses the next track to analyse that no deck holds, keeps what was found and writes it
/// into the file as `mode` lets it; `Some` says whether the file took it. `None` when there is
/// no track left. A track held by a deck is skipped until the next round.
pub fn next(
    library: &mut dyn Library,
    analyser: &dyn Analyser,
    held: &[PathBuf],
    skipped: &mut HashSet<PathBuf>,
    mode: TagWriting,
) -> Result<Option<Result<(), String>>, LibraryError> {
    let candidates = library.unanalysed(BATCH + skipped.len())?;
    let Some(track) = candidates.into_iter().find(|track| {
        if held.contains(&track.path) {
            skipped.insert(track.path.clone());
        }
        !skipped.contains(&track.path)
    }) else {
        return Ok(None);
    };
    // A file that cannot be read counts as analysed with nothing found.
    let Ok(analysis) = analyse_file(analyser, &track.path) else {
        library.set_analysis(track.id, None, None)?;
        return Ok(Some(Ok(())));
    };
    let kept = keep(library, &track, &analysis)?;
    Ok(Some(write_learned(&track.path, &kept, mode)))
}

/// Keeps what the analysis found and returns what the file should take: the grid when it
/// was set from the analysis, and the key.
fn keep(
    library: &mut dyn Library,
    track: &Track,
    analysis: &TrackAnalysis,
) -> Result<Kept, LibraryError> {
    let mut written = None;
    // A grid the user or a deck gave the track stays; so does a text this version cannot read.
    if let Some(tempo) = analysis.tempo {
        let kept = library.memory_at(&track.path)?;
        let readable = kept.is_some() || !library.has_memory(&track.path)?;
        let mut memory = kept.unwrap_or_default();
        if readable
            && memory.grid.is_empty()
            && !memory.grid.is_locked()
            && memory.grid.set_bpm_from(tempo.bpm, tempo.first_beat)
        {
            library.set_memory(track.id, &memory)?;
            written = Some(memory);
        }
    }
    let key = analysis.key.map(keys::number_of);
    library.set_analysis(track.id, key, analysis.loudness)?;
    Ok(Kept {
        memory: written,
        analysis: Some((key, analysis.loudness)),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use platter_core::{Bpm, SampleRate, TrackMemory};
    use platter_engine::{AnalysedTempo, Key, Mode};
    use platter_library::{Timestamp, TrackTags};

    use super::*;

    /// Finds the same tempo, key and loudness in every track.
    struct Fixed;

    impl Analyser for Fixed {
        fn analyse(&self, _stereo: &[f32], _rate: SampleRate) -> TrackAnalysis {
            TrackAnalysis {
                tempo: Some(AnalysedTempo {
                    bpm: Bpm::new(128.0).unwrap(),
                    first_beat: 0.25,
                }),
                key: Some(Key {
                    tonic: 2,
                    mode: Mode::Major,
                }),
                loudness: Some(-8.0),
            }
        }
    }

    /// A tenth of a second of silence as a WAV file.
    fn wav(path: &Path) {
        let frames = 4_800u32;
        let data = frames * 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        bytes.resize(bytes.len() + data as usize, 0);
        std::fs::write(path, bytes).unwrap();
    }

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join("platter-app-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn each_track_is_analysed_once_and_a_kept_grid_stays() {
        let root = folder("background");
        let mut library = SqliteLibrary::open(&root.join("library.sqlite")).unwrap();
        let (fresh, edited, broken, held) = (
            root.join("fresh.wav"),
            root.join("edited.wav"),
            root.join("broken.wav"),
            root.join("held.wav"),
        );
        for path in [&fresh, &edited, &held] {
            wav(path);
        }
        std::fs::write(&broken, b"not audio").unwrap();
        let tags = TrackTags::default();
        for path in [&fresh, &edited, &broken, &held] {
            library.import(path, &tags, Timestamp(0)).unwrap();
        }
        let edited_id = library.track_at(&edited).unwrap().unwrap().id;
        let kept = TrackMemory::parse("platter-track 1\nbpm 120\nmarker 1\n").unwrap();
        library.set_memory(edited_id, &kept).unwrap();

        let mut skipped = HashSet::new();
        let held_now = [held.clone()];
        let mut analysed = 0;
        while next(
            &mut library,
            &Fixed,
            &held_now,
            &mut skipped,
            TagWriting::Nothing,
        )
        .unwrap()
        .is_some()
        {
            analysed += 1;
            assert!(analysed <= 3, "a track was analysed twice");
        }
        assert_eq!(analysed, 3);
        assert_eq!(skipped, HashSet::from([held.clone()]));

        let found = library.memory_at(&fresh).unwrap().unwrap();
        assert_eq!(found.grid.bpm(), Some(Bpm::new(128.0).unwrap()));
        assert_eq!(found.grid.markers(), [0.25]);
        let fresh_track = library.track_at(&fresh).unwrap().unwrap();
        assert_eq!(
            (fresh_track.key, fresh_track.loudness),
            (Some(2), Some(-8.0))
        );
        assert_eq!(library.memory_at(&edited).unwrap(), Some(kept));
        // The file that is not audio counts as analysed, with nothing found.
        let broken_track = library.track_at(&broken).unwrap().unwrap();
        assert_eq!((broken_track.key, broken_track.loudness), (None, None));
        assert_eq!(library.memory_at(&broken).unwrap(), None);
        // The held track waits for its deck, or for the next round.
        assert_eq!(library.count_unanalysed().unwrap(), 1);
        skipped.clear();
        assert!(
            next(&mut library, &Fixed, &[], &mut skipped, TagWriting::Nothing)
                .unwrap()
                .is_some()
        );
        assert_eq!(library.count_unanalysed().unwrap(), 0);
    }

    #[test]
    fn what_the_analysis_found_is_written_into_the_file() {
        let root = folder("background-tags");
        let mut library = SqliteLibrary::open(&root.join("library.sqlite")).unwrap();
        let path = root.join("track.wav");
        wav(&path);
        library
            .import(&path, &TrackTags::default(), Timestamp(0))
            .unwrap();
        let mut skipped = HashSet::new();
        let written = next(
            &mut library,
            &Fixed,
            &[],
            &mut skipped,
            TagWriting::Everything,
        )
        .unwrap();
        assert_eq!(written, Some(Ok(())));
        let bytes = std::fs::read(&path).unwrap();
        let holds = |text: &str| bytes.windows(text.len()).any(|w| w == text.as_bytes());
        assert!(holds("platter-track 1"));
        assert!(holds("TBPM"));
        // D major, as ID3 writes a key.
        assert!(holds("TKEY"));
    }
}
