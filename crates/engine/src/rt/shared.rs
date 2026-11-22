//! What the processor publishes for the control side, once per block, through atomics.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use platter_core::DeckId;

use super::clock::INITIAL_BPM;
use super::input::INPUT_CHANNELS;

/// A relaxed ordering is enough: each value stands alone and readers only display it.
const ORDER: Ordering = Ordering::Relaxed;

#[derive(Debug, Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

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

#[derive(Debug, Default)]
pub struct Count(AtomicU64);

impl Count {
    pub fn get(&self) -> u64 {
        self.0.load(ORDER)
    }

    pub(super) fn set(&self, value: u64) {
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
    pub key_lock: Flag,
    pub beat_tick: Flag,
    /// Shift of the key in semitones.
    pub key_shift: AtomicF64,
    /// Pitch the deck plays against the original, in semitones: the shift while key lock
    /// acts, the pitch of the speed otherwise.
    pub pitch: AtomicF64,
    pub flux: Flag,
    pub reverse: Flag,
    /// Position of the timeline of flux in seconds of track time; not a number while the
    /// deck has none.
    pub timeline: AtomicF64,
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
            key_lock: Flag::default(),
            beat_tick: Flag::default(),
            key_shift: AtomicF64::default(),
            pitch: AtomicF64::default(),
            flux: Flag::default(),
            reverse: Flag::default(),
            timeline: AtomicF64::new(f64::NAN),
            speed: AtomicF64::new(1.0),
            phase,
        }
    }
}

#[derive(Debug)]
pub struct Shared {
    pub decks: [DeckShared; DeckId::COUNT],
    pub master: MasterChoice,
    /// Tempo the decks follow, in beats per minute.
    pub clock_bpm: AtomicF64,
    /// Where the crossfader is, which an automatic fade moves.
    pub crossfader: AtomicF32,
    /// Peak level of the left and the right side of the master output, linear.
    pub master_levels: [AtomicF32; 2],
    /// Gain the limiter is applying: one when it is idle, lower while it works. It reads
    /// zero until the first block has been processed.
    pub limiter_gain: AtomicF32,
    /// Gain the maximiser is applying: one while it is off or idle.
    pub maximiser_gain: AtomicF32,
    pub preview_playing: Flag,
    /// Play position of the preview player, in seconds of track time.
    pub preview_position: AtomicF64,
    /// Frames of the master since the recording started, and those of them the queue of the
    /// recorder had no room for.
    pub recorded_frames: Count,
    pub recorder_dropped: Count,
    /// The loop recorder: what it does, as a [`super::LoopPhase`] code, how far into the
    /// loop it is, from zero to one, the length of the loop and the bars of the size at the
    /// tempo of the clock.
    pub loop_phase: Count,
    pub loop_progress: AtomicF32,
    pub loop_seconds: AtomicF64,
    pub loop_bars: Count,
    pub loop_can_undo: Flag,
    pub loop_can_redo: Flag,
    /// Peak level of each input of the device, linear: `docs/behavior/live-input.md`.
    pub input_levels: [AtomicF32; INPUT_CHANNELS],
    /// Times the queue of the input ran dry while it was heard.
    pub input_gaps: Count,
    /// The input is heard; false while its queue fills, at the start or after a gap.
    pub input_reading: Flag,
    /// The speed the input is read at against the nominal one.
    pub input_ratio: AtomicF64,
    /// The frames the processor had rendered when its latest block began:
    /// `docs/decisions/0041-midi-clock.md`.
    pub block_frame: Count,
    /// Start of the MIDI clock has gone out and Stop has not.
    pub midi_clock: Flag,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            decks: Default::default(),
            master: MasterChoice::default(),
            clock_bpm: AtomicF64::new(INITIAL_BPM),
            crossfader: AtomicF32::new(0.5),
            master_levels: Default::default(),
            limiter_gain: AtomicF32::default(),
            maximiser_gain: AtomicF32::new(1.0),
            preview_playing: Flag::default(),
            preview_position: AtomicF64::default(),
            recorded_frames: Count::default(),
            recorder_dropped: Count::default(),
            loop_phase: Count::default(),
            loop_progress: AtomicF32::default(),
            loop_seconds: AtomicF64::default(),
            loop_bars: Count::default(),
            loop_can_undo: Flag::default(),
            loop_can_redo: Flag::default(),
            input_levels: Default::default(),
            input_gaps: Count::default(),
            input_reading: Flag::default(),
            input_ratio: AtomicF64::new(1.0),
            block_frame: Count::default(),
            midi_clock: Flag::default(),
        }
    }
}
