//! What the engine asks of the analysis of a track, and Platter's own detection behind it.
//! Behaviour notes: `docs/behavior/analysis.md`, `docs/behavior/autogain.md`; the choice of
//! the detection: `docs/decisions/0020-own-analysis.md`; the level of autogain:
//! `docs/decisions/0029-autogain-level.md`.

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

/// Platter's own detection, in the range of tempos of the preferences.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlatterAnalyser {
    pub range: TempoRange,
}

impl Analyser for PlatterAnalyser {
    /// A track the detection cannot read counts as one where nothing was found.
    fn analyse(&self, stereo: &[f32], rate: SampleRate) -> TrackAnalysis {
        let Ok(found) = detect(stereo, 2, rate.hz(), self.range) else {
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
