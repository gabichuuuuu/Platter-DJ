//! Processing blocks without global state. Each one is built before the audio starts and
//! then runs without allocating, locking or calling the system, as the real-time rules of
//! the project require.
//!
//! A frame is `[f32; 2]`: left and right.

pub mod biquad;
pub mod crossfade;
pub mod eq;
pub mod filter;
pub mod interpolate;
pub mod limiter;
pub mod meter;
pub mod smooth;

/// One stereo frame.
pub type Frame = [f32; 2];
