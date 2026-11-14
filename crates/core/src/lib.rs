//! Shared types of Platter. This crate sits at the bottom of the layer stack and depends on
//! nothing outside the standard library.

mod cue;
mod deck;
mod error;
mod gain;
mod grid;
pub mod log;
mod loops;
mod rate;
mod tempo;

pub use cue::{Cue, CueKind, CueName, Cues, HotcueSlot};
pub use deck::DeckId;
pub use error::ValueError;
pub use gain::Gain;
pub use grid::{BEATS_PER_BAR, Beat, BeatGrid, Bpm, MAX_BEATS_VISITED, MAX_MARKERS};
pub use loops::{LoopRegion, LoopSize};
pub use rate::SampleRate;
pub use tempo::Tempo;
