//! The engine of Platter: four decks, the mixer and the master section, driven by commands.
//!
//! [`Engine::new`] returns the two halves. The [`Engine`] is the control side: it takes
//! [`Command`]s, loads tracks in the background and publishes a [`Snapshot`]. The
//! [`Processor`] is the real-time side: whoever owns the audio calls
//! [`Processor::process`] for each block. [`render`] drives both without a sound card. The
//! mix recorder writes the master into files on a worker thread of its own.

mod analyser;
pub mod command;
mod control;
pub mod effects;
mod peaks;
mod recorder;
mod render;
pub mod rt;
pub mod separation;
mod stretch;
mod tap;
mod wav;

pub use analyser::{
    AUTOGAIN_LUFS, AnalysedTempo, Analyser, MAX_AUTOGAIN_DB, PlatterAnalyser, SharedRange,
    TrackAnalysis, analyse_file,
};
pub use command::{
    BpmScale, CROSSFADER_STEP, ClockMode, Command, CommandError, CrossfaderDirection, CueStep,
    Direction, LOOP_SOURCES, MAX_CEILING_DB, MAX_DRIVE_DB, MAX_FADE_BEATS, MAX_RELEASE_MS,
    MIN_CEILING_DB, MIN_FADE_BEATS, MIN_RELEASE_MS, MixerMode, MoveMode, MoveSize, SizeStep,
    SyncMode,
};
pub use control::{
    ClockSnapshot, DeckControls, DeckSnapshot, Engine, EngineError, Event, FADER_UNITY,
    FINE_MOVE_SECONDS, LoopRecorderSnapshot, MaximiserControls, MixerControls, PreviewSnapshot,
    Recall, Snapshot, StemRecall, TEMPO_RANGES, TrackInfo,
};
pub use effects::{EffectControls, FxRouting, FxUnit, MixerEffectControls, UnitControls};
pub use peaks::WaveformPeaks;
pub use platter_analysis::{Key, Mode, TempoRange, Transposed};
pub use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
pub use platter_dsp::effects::unit::{UnitMode, UnitValues};
pub use platter_dsp::effects::{Controls, EffectKind};
pub use platter_dsp::eq::{Band, EqModel};
pub use platter_dsp::filter::FilterModel;
pub use platter_stems::{
    InstallError, SPLEETER_SHA256, SPLEETER_URL, SeparationError, Separator, Spleeter,
    WeightsError, install_spleeter, load_spleeter,
};
pub use recorder::{MIN_SPLIT_BYTES, RecorderSnapshot, RecordingNames, SPLIT_BYTES};
pub use render::{MAX_RENDER_SECONDS, RenderError, RenderPlan, RenderReport, TimedCommand, render};
pub use rt::{
    LOOP_BARS, LoopPhase, LoopSource, LoopState, MAX_LOOP_SECONDS, Output, Processor, StemSettings,
};
pub use wav::WavWriter;
