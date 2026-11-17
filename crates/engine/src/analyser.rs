//! What the engine asks of the analysis of a track, and Platter's own detection behind it.
//! Behaviour notes: `docs/behavior/analysis.md`, `docs/behavior/autogain.md`; the choice of
//! the detection: `docs/decisions/0020-own-analysis.md`; the level of autogain:
//! `docs/decisions/0029-autogain-level.md`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use platter_analysis::{Key, TempoRange, detect, integrated_loudness};
use platter_core::{Bpm, SampleRate};

/// What the analysis of a loaded track found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackAnalysis {
    pub tempo: Option<AnalysedTempo>,
    pub key: Option<Key>,
    /// Integrated loudness in LUFS, as ITU-R BS.1770-5 measures it.
    pub loudness: Option<f64>,
}

/// The loudness autogain brings a track to, in LUFS.
pub const AUTOGAIN_LUFS: f64 = -10.0;
/// The most autogain moves the gain of a channel either way, in decibels.
pub const MAX_AUTOGAIN_DB: f64 = 12.0;

impl TrackAnalysis {
    /// The gain that brings the track to [`AUTOGAIN_LUFS`], within [`MAX_AUTOGAIN_DB`] either
    /// way; none when no loudness was found.
    pub fn autogain_db(&self) -> Option<f64> {
        self.loudness
            .filter(|lufs| lufs.is_finite())
            .map(|lufs| (AUTOGAIN_LUFS - lufs).clamp(-MAX_AUTOGAIN_DB, MAX_AUTOGAIN_DB))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnalysedTempo {
    pub bpm: Bpm,
    /// The first beat of the track, in seconds.
    pub first_beat: f64,
}

/// Analyses a track on the worker thread that loaded it. Another detection takes the place
/// of Platter's through [`crate::Engine::with_analyser`]; the tests use one to choose when a
/// result arrives.
pub trait Analyser: Send + Sync {
    /// `stereo` holds interleaved stereo samples at `rate`.
    fn analyse(&self, stereo: &[f32], rate: SampleRate) -> TrackAnalysis;
}

/// The range of tempos of the preferences, shared by the analyses that read it: the loads of
/// the decks and the analysis of the collection. A new range applies to the analyses that start
/// after it is set.
#[derive(Clone, Debug, Default)]
pub struct SharedRange(Arc<AtomicU64>);

impl SharedRange {
    pub fn new(range: TempoRange) -> Self {
        let shared = Self::default();
        shared.set(range);
        shared
    }

    /// Both ends go in one atomic word, in hundredths of a BPM, so a reader never sees the
    /// bottom of one range with the top of another.
    pub fn set(&self, range: TempoRange) {
        let hundredths = |bpm: f64| (bpm * 100.0).round() as u64 & u64::from(u32::MAX);
        self.0.store(
            (hundredths(range.min()) << 32) | hundredths(range.max()),
            Ordering::Relaxed,
        );
    }

    /// The range set last; the default range before any is set.
    pub fn get(&self) -> TempoRange {
        let word = self.0.load(Ordering::Relaxed);
        let bpm = |hundredths: u64| hundredths as f64 / 100.0;
        TempoRange::new(bpm(word >> 32), bpm(word & u64::from(u32::MAX))).unwrap_or_default()
    }
}

/// Platter's own detection, in the range of tempos of the preferences.
#[derive(Clone, Debug, Default)]
pub struct PlatterAnalyser {
    range: SharedRange,
}

impl PlatterAnalyser {
    /// A detection that looks in the range `range` holds when each analysis starts.
    pub fn with_range(range: SharedRange) -> Self {
        Self { range }
    }
}

impl Analyser for PlatterAnalyser {
    /// A track the detection cannot read counts as one where nothing was found.
    fn analyse(&self, stereo: &[f32], rate: SampleRate) -> TrackAnalysis {
        let Ok(found) = detect(stereo, 2, rate.hz(), self.range.get()) else {
            return TrackAnalysis::default();
        };
        TrackAnalysis {
            loudness: integrated_loudness(stereo, 2, rate.hz()).ok().flatten(),
            tempo: found.tempo.and_then(|tempo| {
                Bpm::new(tempo.bpm).ok().map(|bpm| AnalysedTempo {
                    bpm,
                    first_beat: tempo.first_beat,
                })
            }),
            key: found.key,
        }
    }
}

/// Decodes the file at `path` and analyses it, as a load does, for a track no deck holds:
/// the analysis of the collection in the background (`docs/behavior/browser.md`). Slow: it
/// runs on a worker thread.
pub fn analyse_file(
    analyser: &dyn Analyser,
    path: &std::path::Path,
) -> Result<TrackAnalysis, platter_decode::DecodeError> {
    let decoded = platter_decode::decode_file(path)?;
    let rate = decoded.sample_rate;
    let stereo = decoded.into_stereo();
    Ok(analyser.analyse(&stereo, rate))
}
