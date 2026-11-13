//! The real-time side of the engine: everything the audio callback runs.
//!
//! Rules of this module, from `AGENTS.md`: no allocation and no deallocation, no lock, no
//! I/O, no system call, bounded time. It talks to the rest of the program through two
//! lock-free queues and a block of atomics. It uses `platter-core`, `platter-dsp` and the
//! queue crate only; `cargo xtask layers` checks that.
//!
//! Memory is the delicate part. A track arrives inside a command and leaves through the
//! retired queue, so its buffer is always freed by a normal thread.

mod deck;
mod processor;
mod shared;
mod strip;

use std::sync::Arc;

use platter_core::{DeckId, SampleRate};
use platter_dsp::Frame;
use platter_dsp::crossfade::CrossfaderCurve;
use platter_dsp::eq::Band;
use rtrb::{Consumer, Producer, RingBuffer};

pub use processor::Processor;
pub use shared::{DeckShared, Shared};

/// Commands the processor takes in one call. The queue holds this many; a control side
/// that sends faster gets an error instead of blocking.
pub const COMMAND_CAPACITY: usize = 256;

/// Stereo audio of a loaded track, shared without copying.
#[derive(Clone, Debug)]
pub struct TrackAudio {
    samples: Arc<[f32]>,
    rate: SampleRate,
}

impl TrackAudio {
    /// Wraps interleaved stereo samples. A trailing half frame is dropped.
    pub fn new(rate: SampleRate, mut stereo: Vec<f32>) -> Self {
        stereo.truncate(stereo.len() / 2 * 2);
        Self {
            samples: stereo.into(),
            rate,
        }
    }

    pub fn rate(&self) -> SampleRate {
        self.rate
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    /// The frame at `index`, or silence outside the track.
    fn frame(&self, index: i64) -> Frame {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.samples.get(index * 2..index * 2 + 2))
            .map_or([0.0; 2], |frame| [frame[0], frame[1]])
    }
}

/// What the control side asks of the processor. Values are already validated and converted;
/// the processor applies them as they are.
#[derive(Debug)]
pub enum RtCommand {
    /// Replaces the track of a deck; `None` empties it. The previous audio comes back
    /// through the retired queue, exactly one item per command.
    Swap {
        deck: DeckId,
        audio: Option<TrackAudio>,
    },
    Play {
        deck: DeckId,
    },
    Pause {
        deck: DeckId,
    },
    TogglePlay {
        deck: DeckId,
    },
    CueSet {
        deck: DeckId,
    },
    CueGoto {
        deck: DeckId,
    },
    Seek {
        deck: DeckId,
        seconds: f64,
    },
    Tempo {
        deck: DeckId,
        ratio: f32,
    },
    Nudge {
        deck: DeckId,
        amount: f32,
    },
    Gain {
        deck: DeckId,
        linear: f32,
    },
    Eq {
        deck: DeckId,
        band: Band,
        gain: f32,
    },
    Filter {
        deck: DeckId,
        position: f32,
    },
    Fader {
        deck: DeckId,
        gain: f32,
    },
    Crossfader {
        position: f32,
    },
    CrossfaderCurve {
        curve: CrossfaderCurve,
    },
    MasterGain {
        linear: f32,
    },
}

/// Audio the processor no longer uses, on its way to be freed by a normal thread.
#[derive(Debug)]
pub struct Retired(pub Option<TrackAudio>);

/// The ends of the queues that stay on the control side.
pub struct ControlEnds {
    pub commands: Producer<RtCommand>,
    pub retired: Consumer<Retired>,
    pub shared: Arc<Shared>,
}

/// Builds a processor for `rate` and the ends the control side keeps.
pub fn channel(rate: SampleRate) -> (ControlEnds, Processor) {
    let (commands, incoming) = RingBuffer::new(COMMAND_CAPACITY);
    // Each swap retires one item and at most `COMMAND_CAPACITY` swaps are in flight, so
    // this queue cannot fill up.
    let (outgoing, retired) = RingBuffer::new(COMMAND_CAPACITY + DeckId::COUNT);
    let shared = Arc::new(Shared::default());
    let processor = Processor::new(rate, incoming, outgoing, Arc::clone(&shared));
    (
        ControlEnds {
            commands,
            retired,
            shared,
        },
        processor,
    )
}
