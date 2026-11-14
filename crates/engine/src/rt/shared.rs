//! What the processor publishes for the control side, once per block, through atomics.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use platter_core::DeckId;

use super::clock::INITIAL_BPM;

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
    fn new(value: f64) -> Self {
        Self(AtomicU64::new(value.to_bits()))
    }

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

/// The tempo master: a deck, or the clock when it is none.
#[derive(Debug, Default)]
pub struct MasterChoice(AtomicU8);

impl MasterChoice {
    pub fn get(&self) -> Option<DeckId> {
        let slot = usize::from(self.0.load(ORDER));
        DeckId::ALL.get(slot.checked_sub(1)?).copied()
    }

    pub(super) fn set(&self, master: Option<DeckId>) {
        let slot = master.map_or(0, |deck| deck.index() + 1);
        self.0.store(u8::try_from(slot).unwrap_or(0), ORDER);
    }
}

#[derive(Debug)]
pub struct DeckShared {
    pub loaded: Flag,
    pub playing: Flag,
    /// Position in seconds of track time.
    pub position: AtomicF64,
    /// Cue point in seconds of track time.
    pub cue: AtomicF64,
    /// Post-fader peak level of the left and the right side, linear.
    pub levels: [AtomicF32; 2],
    pub sync: Flag,
    /// Speed the deck is set to, as a ratio of the original.
    pub speed: AtomicF64,
    /// How far the beat of the deck is from that of the clock, in beats: negative behind,
    /// positive ahead. Not a number while the deck has no beat.
    pub phase: AtomicF32,
}

impl Default for DeckShared {
    fn default() -> Self {
        let phase = AtomicF32::default();
        phase.set(f32::NAN);
        Self {
            loaded: Flag::default(),
            playing: Flag::default(),
            position: AtomicF64::default(),
            cue: AtomicF64::default(),
            levels: Default::default(),
            sync: Flag::default(),
            speed: AtomicF64::new(1.0),
            phase,
        }
    }
}

#[derive(Debug)]
pub struct Shared {
    pub decks: [DeckShared; 2],
    pub master: MasterChoice,
    /// Tempo the decks follow, in beats per minute.
    pub clock_bpm: AtomicF64,
    /// Peak level of the left and the right side of the master output, linear.
    pub master_levels: [AtomicF32; 2],
    /// Gain the limiter is applying: one when it is idle, lower while it works. It reads
    /// zero until the first block has been processed.
    pub limiter_gain: AtomicF32,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            decks: Default::default(),
            master: MasterChoice::default(),
            clock_bpm: AtomicF64::new(INITIAL_BPM),
            master_levels: Default::default(),
            limiter_gain: AtomicF32::default(),
        }
    }
}
