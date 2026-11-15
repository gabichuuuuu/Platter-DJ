//! What the engine asks of the analysis of a track, and Platter's own detection behind it.
//! Behaviour note: `docs/behavior/analysis.md`; the choice of the detection:
//! `docs/decisions/0020-own-analysis.md`.

use platter_analysis::{Key, TempoRange, detect};
use platter_core::{Bpm, SampleRate};

/// What the analysis of a loaded track found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackAnalysis {
    pub tempo: Option<AnalysedTempo>,
    pub key: Option<Key>,
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
