//! The engine of Platter: two decks, the mixer and the master section, driven by commands.
//!
//! [`Engine::new`] returns the two halves. The [`Engine`] is the control side: it takes
//! [`Command`]s, loads tracks in the background and publishes a [`Snapshot`]. The
//! [`Processor`] is the real-time side: whoever owns the audio calls
//! [`Processor::process`] for each block. [`render`] drives both without a sound card.

mod analyser;
pub mod command;
mod control;
mod peaks;
mod render;
pub mod rt;
mod stretch;
mod tap;
mod wav;

pub use analyser::{AnalysedTempo, Analyser, PlatterAnalyser, TrackAnalysis};
pub use command::{
    BpmScale, ClockMode, Command, CommandError, CueStep, Direction, MoveMode, MoveSize, SizeStep,
    SyncMode,
};
pub use control::{
    ClockSnapshot, DeckControls, DeckSnapshot, Engine, EngineError, Event, FADER_UNITY,
    FINE_MOVE_SECONDS, MixerControls, Snapshot, TEMPO_RANGES, TrackInfo,
};
pub use peaks::WaveformPeaks;
pub use platter_analysis::{Key, Mode, TempoRange, Transposed};
pub use platter_dsp::crossfade::CrossfaderCurve;
pub use platter_dsp::eq::Band;
pub use render::{MAX_RENDER_SECONDS, RenderError, RenderPlan, RenderReport, TimedCommand, render};
pub use rt::{LoopState, Output, Processor};
pub use wav::WavWriter;
