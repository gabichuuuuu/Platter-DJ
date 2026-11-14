//! Shared types of Platter. This crate sits at the bottom of the layer stack and depends on
//! nothing outside the standard library.

mod deck;
mod error;
mod gain;
mod grid;
pub mod log;
mod rate;
mod tempo;

pub use deck::DeckId;
pub use error::ValueError;
pub use gain::Gain;
pub use grid::{BeatGrid, Bpm, MAX_MARKERS};
pub use rate::SampleRate;
pub use tempo::Tempo;
