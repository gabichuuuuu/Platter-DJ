//! The grid of a remix deck as the control side keeps it, and the decoding of a sample on a
//! worker thread. Behaviour note: `docs/behavior/remix-deck.md`; the choices:
//! `docs/decisions/0042-remix-deck.md`.

use std::path::Path;
use std::sync::Arc;

use platter_core::{Bpm, SampleRate, TrackMemory};
use platter_decode::{decode_file, probe_file};

use crate::analyser::Analyser;
use crate::control::TrackInfo;
use crate::rt::{CELLS, CellId, CellMode, CellSettings, MAX_SAMPLE_SECONDS, SLOTS, TrackAudio};

/// A cell that holds a sample, as the snapshot shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct CellInfo {
    /// The title of the file, or its name without the extension.
    pub name: Arc<str>,
    pub mode: CellMode,
    /// The tempo the cell plays at the tempo of the clock from; none plays at its own speed.
    pub bpm: Option<Bpm>,
    pub gain_db: f64,
    pub seconds: f64,
    /// The cell plays only while its key is held.
    pub gate: bool,
    pub reverse: bool,
}

impl CellInfo {
    pub(crate) fn settings(&self) -> CellSettings {
        CellSettings {
            mode: self.mode,
            bpm: self.bpm.map(Bpm::value),
            gain: 10f64.powf(self.gain_db / 20.0) as f32,
            gate: self.gate,
            reverse: self.reverse,
        }
    }
}

/// The grid of a deck.
#[derive(Clone, Debug, PartialEq)]
pub struct RemixSnapshot {
    /// Slot by slot, cell by cell.
    pub cells: [[Option<CellInfo>; CELLS]; SLOTS],
    /// A file is being decoded for the cell.
    pub loading: [[bool; CELLS]; SLOTS],
    /// The cell each slot plays and the one it waits to start.
    pub playing: [Option<usize>; SLOTS],
    pub waiting: [Option<usize>; SLOTS],
    pub volume: [f64; SLOTS],
    pub muted: [bool; SLOTS],
    /// A new loop of the slot starts where the one before had reached.
    pub punch: [bool; SLOTS],
    /// Beats of the boundary; none starts the cells at once.
    pub quantize: Option<u32>,
}

impl Default for RemixSnapshot {
    fn default() -> Self {
        Self {
            cells: Default::default(),
            loading: [[false; CELLS]; SLOTS],
            playing: [None; SLOTS],
            waiting: [None; SLOTS],
            volume: [1.0; SLOTS],
            muted: [false; SLOTS],
            punch: [false; SLOTS],
            quantize: Some(1),
        }
    }
}

/// The grid of a deck on the control side: what the snapshot shows, and the count of loads of
/// each cell, so a decode that finishes late is recognised as stale.
#[derive(Default)]
pub(crate) struct RemixControl {
    pub(crate) shown: RemixSnapshot,
    pub(crate) generations: [[u64; CELLS]; SLOTS],
}

impl RemixControl {
    pub(crate) fn cell(&mut self, at: CellId) -> Option<&mut CellInfo> {
        self.shown.cells[at.slot][at.cell].as_mut()
    }
}

/// What a worker decoded for a cell.
pub(crate) struct DecodedSample {
    pub(crate) audio: TrackAudio,
    pub(crate) info: CellInfo,
}

/// Copies `seconds` of a track from `start` into a sample for a cell, on the worker of the
/// capture. A part that runs past the end of the track is cut there.
pub(crate) fn capture_sample(
    audio: &TrackAudio,
    info: &TrackInfo,
    start: f64,
    seconds: f64,
    bpm: Option<Bpm>,
) -> Result<DecodedSample, String> {
    if !seconds.is_finite() || seconds > MAX_SAMPLE_SECONDS || !start.is_finite() {
        return Err(format!(
            "a sample is at most {MAX_SAMPLE_SECONDS:.0} seconds long"
        ));
    }
    let rate = audio.rate();
    let frames = audio.frames();
    let first = (start.max(0.0) * rate.as_f64()).round() as usize;
    let count = (seconds.max(0.0) * rate.as_f64()).round() as usize;
    let last = first.saturating_add(count).min(frames);
    let samples = audio
        .samples()
        .get(first.saturating_mul(2)..last.saturating_mul(2))
        .unwrap_or_default();
    if samples.len() < 4 {
        return Err("the part to capture is empty".to_owned());
    }
    let name = info
        .tags
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            info.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let seconds = (last - first) as f64 / rate.as_f64();
    Ok(DecodedSample {
        audio: TrackAudio::new(rate, samples.to_vec()),
        info: CellInfo {
            name: name.into(),
            mode: CellMode::Loop,
            bpm,
            gain_db: 0.0,
            seconds,
            gate: false,
            reverse: false,
        },
    })
}

/// Decodes `path` for a cell, on the worker that loads it. The length the file declares is
/// checked before it is decoded, and the decoded length after.
pub(crate) fn decode_sample(
    path: &Path,
    memory: Option<TrackMemory>,
    analyser: &dyn Analyser,
) -> Result<DecodedSample, String> {
    let too_long = || format!("a sample is at most {MAX_SAMPLE_SECONDS:.0} seconds long");
    let declared = probe_file(path).map_err(|error| error.to_string())?;
    if declared
        .seconds
        .is_some_and(|seconds| !seconds.is_finite() || seconds > MAX_SAMPLE_SECONDS)
    {
        return Err(too_long());
    }
    let decoded = decode_file(path).map_err(|error| error.to_string())?;
    let seconds = decoded.seconds();
    if !seconds.is_finite() || seconds > MAX_SAMPLE_SECONDS {
        return Err(too_long());
    }
    let name = decoded
        .tags
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
        .into();
    let rate: SampleRate = decoded.sample_rate;
    let stereo = decoded.into_stereo();
    let bpm = memory
        .and_then(|memory| memory.grid.bpm())
        .or_else(|| analyser.analyse(&stereo, rate).tempo.map(|tempo| tempo.bpm));
    Ok(DecodedSample {
        audio: TrackAudio::new(rate, stereo),
        info: CellInfo {
            name,
            mode: CellMode::default(),
            bpm,
            gain_db: 0.0,
            seconds,
            gate: false,
            reverse: false,
        },
    })
}
