//! The engine of Platter: two decks, the mixer and the master section, driven by commands.
//!
//! [`Engine::new`] returns the two halves. The [`Engine`] is the control side: it takes
//! [`Command`]s, loads tracks in the background and publishes a [`Snapshot`]. The
//! [`Processor`] is the real-time side: whoever owns the audio calls
//! [`Processor::process`] for each block. [`render`] drives both without a sound card.

pub mod command;
mod control;
mod peaks;
mod render;
pub mod rt;
mod wav;

pub use command::{Command, CommandError};
pub use control::{
    DeckControls, DeckSnapshot, Engine, EngineError, Event, FADER_UNITY, MixerControls, Snapshot,
    TEMPO_RANGES, TrackInfo,
};
pub use peaks::WaveformPeaks;
pub use platter_dsp::crossfade::CrossfaderCurve;
pub use platter_dsp::eq::Band;
pub use render::{MAX_RENDER_SECONDS, RenderError, RenderPlan, RenderReport, TimedCommand, render};
pub use rt::{Output, Processor};
pub use wav::WavWriter;
