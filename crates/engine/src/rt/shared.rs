//! What the processor publishes for the control side, once per block, through atomics.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// A relaxed ordering is enough: each value stands alone and readers only display it.
const ORDER: Ordering = Ordering::Relaxed;

#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(ORDER))
    }

    pub(super) fn set(&self, value: f32) {
        self.0.store(value.to_bits(), ORDER);
    }
}

#[derive(Debug, Default)]
pub struct AtomicF64(AtomicU64);

impl AtomicF64 {
    pub fn get(&self) -> f64 {
        f64::from_bits(self.0.load(ORDER))
    }

    pub(super) fn set(&self, value: f64) {
        self.0.store(value.to_bits(), ORDER);
    }
}

#[derive(Debug, Default)]
pub struct Flag(AtomicBool);

impl Flag {
    pub fn get(&self) -> bool {
        self.0.load(ORDER)
    }

    pub(super) fn set(&self, value: bool) {
        self.0.store(value, ORDER);
    }
}

#[derive(Debug, Default)]
pub struct DeckShared {
    pub loaded: Flag,
    pub playing: Flag,
    /// Position in seconds of track time.
    pub position: AtomicF64,
    /// Cue point in seconds of track time.
    pub cue: AtomicF64,
    /// Post-fader peak level of the left and the right side, linear.
    pub levels: [AtomicF32; 2],
}

#[derive(Debug, Default)]
pub struct Shared {
    pub decks: [DeckShared; 2],
    /// Peak level of the left and the right side of the master output, linear.
    pub master_levels: [AtomicF32; 2],
    /// Gain the limiter is applying: one when it is idle, lower while it works. It reads
    /// zero until the first block has been processed.
    pub limiter_gain: AtomicF32,
}
