//! The real-time side of the engine: everything the audio callback runs.
//!
//! Rules of this module, from `AGENTS.md`: no allocation and no deallocation, no lock, no
//! I/O, no system call, bounded time. It talks to the rest of the program through two
//! lock-free queues and a block of atomics. It uses `platter-core`, `platter-dsp` and the
//! queue crate only; `cargo xtask layers` checks that.
//!
//! Memory is the delicate part. A track arrives inside a command and leaves through the
//! retired queue, so its buffer is always freed by a normal thread. The beatgrid of each
//! deck lives here, because its edits use the play position; it goes back to the control
//! side as a fixed-size value through a third queue.

mod deck;
mod processor;
mod shared;
mod strip;
mod tone;

use std::sync::Arc;

use platter_core::{BeatGrid, Bpm, DeckId, SampleRate};
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

/// An output pair of the engine. The master is the first two channels of the device; the
/// headphones are the third and the fourth, when the device has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Master,
    Headphones,
}

impl Output {
    pub const ALL: [Self; 2] = [Self::Master, Self::Headphones];

    pub fn name(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Headphones => "headphones",
        }
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
    CueButton {
        deck: DeckId,
        pressed: bool,
    },
    CupButton {
        deck: DeckId,
        pressed: bool,
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
    /// An edit of the beatgrid of the track the deck holds once the commands before are
    /// applied.
    Grid {
        deck: DeckId,
        edit: GridEdit,
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
    EqCut {
        deck: DeckId,
        band: Band,
        cut: bool,
    },
    Filter {
        deck: DeckId,
        position: f32,
    },
    Balance {
        deck: DeckId,
        position: f32,
    },
    /// Sends the deck to the headphones, before its fader.
    PreListen {
        deck: DeckId,
        on: bool,
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
    /// Balance in the headphones: `0.0` is the pre-listened decks alone, `1.0` the master.
    HeadphoneMix {
        position: f32,
    },
    HeadphoneGain {
        linear: f32,
    },
    TestTone {
        output: Output,
    },
}

/// An edit of a beatgrid. The ones that need a place use the play position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridEdit {
    SetBpm(Bpm),
    ScaleBpm(f64),
    StepBpm(f64),
    MarkerSet,
    /// Moves the marker on or before the play position by this many seconds.
    MarkerMove(f64),
    MarkerDelete,
    MarkerPrevious,
    MarkerNext,
    ToggleLock,
}

/// Audio the processor no longer uses, on its way to be freed by a normal thread.
#[derive(Debug)]
pub struct Retired(pub Option<TrackAudio>);

/// The beatgrid of a deck after it changed.
#[derive(Debug)]
pub struct GridReport {
    pub deck: DeckId,
    pub grid: BeatGrid,
}

/// Grid reports the queue holds. The processor sends at most one per deck and block, and
/// keeps a report it cannot send for the next block, so the latest grid always arrives.
const GRID_REPORT_CAPACITY: usize = 4 * DeckId::COUNT;

/// The ends of the queues that stay on the control side.
pub struct ControlEnds {
    pub commands: Producer<RtCommand>,
    pub retired: Consumer<Retired>,
    pub grids: Consumer<GridReport>,
    pub shared: Arc<Shared>,
}

/// The ends of the queues the processor keeps.
struct Queues {
    commands: Consumer<RtCommand>,
    retired: Producer<Retired>,
    grids: Producer<GridReport>,
}

/// Builds a processor for `rate` and the ends the control side keeps.
pub fn channel(rate: SampleRate) -> (ControlEnds, Processor) {
    let (commands, incoming) = RingBuffer::new(COMMAND_CAPACITY);
    // Each swap retires one item and at most `COMMAND_CAPACITY` swaps are in flight, so
    // this queue cannot fill up.
    let (outgoing, retired) = RingBuffer::new(COMMAND_CAPACITY + DeckId::COUNT);
    let (grid_reports, grids) = RingBuffer::new(GRID_REPORT_CAPACITY);
    let shared = Arc::new(Shared::default());
    let processor = Processor::new(
        rate,
        Queues {
            commands: incoming,
            retired: outgoing,
            grids: grid_reports,
        },
        Arc::clone(&shared),
    );
    (
        ControlEnds {
            commands,
            retired,
            grids,
            shared,
        },
        processor,
    )
}
