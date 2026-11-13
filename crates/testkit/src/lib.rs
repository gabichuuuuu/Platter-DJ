//! Test support shared by the Platter crates. It is a dev-dependency only: nothing here ships.
//!
//! Every signal and every audio file used by the tests is synthesised here; the repository
//! holds no recorded audio.

pub mod alloc;
pub mod files;
pub mod measure;
pub mod signal;
