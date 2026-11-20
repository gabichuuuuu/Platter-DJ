//! The real-time side of the engine: everything the audio callback runs.
//!
//! Rules of this module, from `AGENTS.md`: no allocation and no deallocation, no lock, no
//! I/O, no system call, bounded time. It talks to the rest of the program through four
//! lock-free queues, the fourth carrying the master to the recorder, and a block of atomics. It uses `platter-core`, `platter-dsp` and the
//! queue crate only; `cargo xtask layers` checks that.
//!
//! Memory is the delicate part. A track arrives inside a command and leaves through the
//! retired queue, so its buffer is always freed by a normal thread. The beatgrid, the
//! loop and the stored cues of each deck live here, because their edits use the play
//! position; they go back to
//! the control side as fixed-size values through a third queue. The master clock and the
//! choice of the tempo master live here too, because they follow what the decks play.

mod clock;
mod deck;
mod effects;
mod lock;
mod looper;
mod metronome;
mod preview;
mod processor;
mod recorder;
mod shared;
mod strip;
mod tick;
mod tone;

use std::sync::Arc;

use platter_core::{
    BeatGrid, Bpm, CueKind, CueName, Cues, DeckId, HotcueSlot, LoopRegion, SampleRate, TrackMemory,
};
use platter_dsp::Frame;
use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
use platter_dsp::effects::Effect;
use platter_dsp::effects::unit::{Place, UnitValues};
use platter_dsp::eq::{Band, EqModel};
use platter_dsp::filter::FilterModel;
use rtrb::{Consumer, Producer, RingBuffer};

pub use effects::{FxRouting, UNITS};

pub use lock::Stretcher;
pub use processor::Processor;
pub use recorder::Recorded;
pub use shared::{DeckShared, MasterChoice, Shared};

/// Commands the processor takes in one call. The queue holds this many; a control side
/// that sends faster gets an error instead of blocking.
pub const COMMAND_CAPACITY: usize = 256;

/// Seconds of the master the queue of the recorder holds, so the disk may fall behind that
/// long before a frame is dropped: `docs/behavior/recording.md`.
pub const RECORDER_QUEUE_SECONDS: usize = 4;

/// The most the key of a deck moves either way, in semitones.
pub const MAX_KEY_SHIFT: f64 = 12.0;

/// The side of the crossfader each channel starts on, deck A first. Decks C and D take the
/// side of the deck above them: `docs/behavior/decks.md`.
pub const CROSSFADER_SIDES: [CrossfaderSide; DeckId::COUNT] = [
    CrossfaderSide::Left,
    CrossfaderSide::Right,
    CrossfaderSide::Left,
    CrossfaderSide::Right,
];

/// A track as a deck holds it: its audio and the stretch key lock plays it with, when one
/// could be built for it.
#[derive(Debug)]
pub struct DeckTrack {
    pub audio: TrackAudio,
    pub stretch: Option<Stretcher>,
    /// The grid and the cues kept for the track, which the deck starts with. Boxed so the
    /// commands stay small; the box goes back with the retired items, to be freed off the
    /// audio thread.
    pub memory: Option<Box<TrackMemory>>,
    /// Where the deck starts, in seconds of track time, when not at the beginning: the load
    /// marker. The cue point starts there too.
    pub start: Option<f64>,
}

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

    /// The interleaved stereo samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
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
    /// Replaces the track of the preview player, playing it from its start when `play`. The
    /// previous audio comes back through the retired queue, exactly one item per command.
    PreviewSwap {
        track: Option<DeckTrack>,
        play: bool,
    },
    PreviewToggle,
    /// Moves the play position of the preview player, in seconds of track time.
    PreviewSeek {
        seconds: f64,
    },
    /// Replaces the track of a deck; `None` empties it. The previous audio comes back
    /// through the retired queue, exactly one item per command.
    Swap {
        deck: DeckId,
        track: Option<DeckTrack>,
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
    /// An edit of the loop, at the play position the deck has once the commands before are
    /// applied.
    Loop {
        deck: DeckId,
        edit: LoopEdit,
    },
    /// Moves the play position by a span, to the frame.
    Jump {
        deck: DeckId,
        by: Span,
    },
    /// An edit of the stored cues, at the play position the deck has once the commands
    /// before are applied.
    Cues {
        deck: DeckId,
        edit: CueEdit,
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
    EqModel {
        model: EqModel,
    },
    FilterModel {
        model: FilterModel,
    },
    CrossfaderAssign {
        deck: DeckId,
        side: CrossfaderSide,
    },
    /// Moves the crossfader to `to` by itself, the whole travel in `beats` of the clock.
    CrossfaderFade {
        to: f32,
        beats: f32,
    },
    MasterGain {
        linear: f32,
    },
    /// The maximiser of the master: switched in or out, its drive and ceiling, linear, and
    /// its release.
    Maximiser {
        on: bool,
        drive: f32,
        ceiling: f32,
        release_seconds: f32,
    },
    /// Sends each deck to its own outputs instead of mixing them, and which controls of the
    /// mixer still act then.
    ExternalMixer {
        on: bool,
        eq_gain: bool,
        master_volume: bool,
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
    SyncToggle {
        deck: DeckId,
    },
    KeyLockToggle {
        deck: DeckId,
    },
    BeatTickToggle {
        deck: DeckId,
    },
    /// Moves the key by a semitone, up or down, and turns key lock on.
    KeyStep {
        deck: DeckId,
        up: bool,
    },
    /// Sets the shift of the key in semitones and turns key lock on.
    KeySet {
        deck: DeckId,
        semitones: f64,
    },
    /// Turns key lock on with the key of the current speed.
    KeyPersist {
        deck: DeckId,
    },
    FluxToggle {
        deck: DeckId,
    },
    Reverse {
        deck: DeckId,
        pressed: bool,
    },
    /// Makes a deck the tempo master, while the choice is not automatic.
    Master {
        deck: DeckId,
    },
    /// Switches the automatic choice of the tempo master on, or makes the clock the tempo
    /// master.
    ClockAuto {
        on: bool,
    },
    ClockTempo {
        bpm: Bpm,
    },
    /// Adds beats per minute to the tempo of the clock.
    ClockStep {
        amount: f64,
    },
    ClockBend {
        amount: f32,
    },
    /// Whether a synced deck keeps its beats on those of the clock, or only its tempo.
    KeepBeat {
        on: bool,
    },
    /// New cues and loops of both decks land on the nearest beat.
    Snap {
        on: bool,
    },
    /// A playing deck that goes to a stored cue keeps its place in the beat.
    Quantize {
        on: bool,
    },
    /// A click on every beat of the clock, on the headphones.
    /// The values of an effect unit, for every channel it is assigned to.
    FxValues {
        unit: usize,
        values: UnitValues,
    },
    FxRouting {
        unit: usize,
        routing: FxRouting,
    },
    /// Puts the channel through the unit, or takes it out, with a short fade.
    FxAssign {
        deck: DeckId,
        unit: usize,
        on: bool,
    },
    /// Puts an effect, built on the control side, in a place of a channel's chain; `None`
    /// empties it. Exactly one item comes back through the retired queue per command.
    FxPlace {
        deck: DeckId,
        unit: usize,
        place: Place,
        effect: Option<Box<Effect>>,
    },
    /// Puts an effect behind the mixer effect knob of a channel; as [`Self::FxPlace`].
    MixerEffectPlace {
        deck: DeckId,
        effect: Option<Box<Effect>>,
    },
    /// The mixer effect knob of a channel, 0 to 1.
    MixerEffectAmount {
        deck: DeckId,
        amount: f32,
    },
    Metronome {
        on: bool,
    },
    /// Starts or stops sending the master to the recorder, for the recording with this
    /// number.
    Record {
        on: bool,
        recording: u32,
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
    /// The tempo of the marker on or before the play position, or that of the grid.
    MarkerBpm(Option<Bpm>),
    MarkerDelete,
    MarkerPrevious,
    MarkerNext,
    ToggleLock,
    /// The tempo and the first beat an analysis found, taken only by an empty grid.
    Analysed {
        bpm: Bpm,
        first_beat: f64,
    },
    /// The grid becomes what the analysis found: a tempo and a marker on its first beat, or
    /// neither.
    Reset(Option<(Bpm, f64)>),
    /// The tempo found by the analysis, with a marker on its first beat for a grid without
    /// markers.
    ResetBpm {
        bpm: Bpm,
        first_beat: f64,
    },
}

/// How far a jump or a move goes. Negative values go back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Span {
    /// Beats at the tempo of the grid; nothing without a tempo.
    Beats(f64),
    Seconds(f64),
    /// The length of the loop that is on or, without one, this many beats.
    Loop {
        forward: bool,
        beats: f64,
    },
}

/// An edit of the loop of a deck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoopEdit {
    /// Sets the in point and switches off the loop.
    In,
    /// Ends the loop at the play position and switches it on.
    Out,
    /// Switches the loop off, or starts one this many beats long.
    Toggle {
        beats: f64,
    },
    /// Gives the loop that is on this many beats from its start.
    Resize {
        beats: f64,
    },
    Move {
        part: LoopPart,
        by: Span,
    },
}

/// What a move of the loop moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopPart {
    Whole,
    In,
    Out,
}

/// An edit of the stored cues of a deck, or a visit to one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CueEdit {
    /// The key of a slot, pressed or released: stores into an empty slot, goes to a stored
    /// cue.
    Hotcue {
        slot: HotcueSlot,
        pressed: bool,
    },
    /// Stores the floating cue, or the loop that is on, in the first empty slot.
    Store,
    Delete(HotcueSlot),
    DeleteSelected,
    /// Moves the selected cue to a slot.
    Map(HotcueSlot),
    /// Goes to the cue after the play position, or to the one before.
    Step {
        forward: bool,
    },
    Rename(CueName),
    /// Gives the selected cue a kind; one that becomes a loop gets this many beats.
    Retype {
        kind: CueKind,
        beats: f64,
    },
}

/// The loop of a deck, in seconds of its track. A loop is on for as long as it exists.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoopState {
    /// An in point that waits for its out point.
    pub in_point: Option<f64>,
    pub region: Option<LoopRegion>,
}

/// Audio the processor no longer uses, on its way to be freed by a normal thread.
#[derive(Debug)]
pub struct Retired(
    pub Option<DeckTrack>,
    pub Option<Box<TrackMemory>>,
    pub Option<Box<Effect>>,
);

/// What changed in a deck that the control side keeps a copy of: each part is there when it
/// changed since the report before.
#[derive(Debug)]
pub struct DeckReport {
    pub deck: DeckId,
    pub grid: Option<BeatGrid>,
    pub looping: Option<LoopState>,
    pub cues: Option<Cues>,
}

/// Reports the queue holds. The processor sends at most one per deck and block, with all
/// that changed, and keeps what it cannot send for the next block, so the latest state
/// always arrives.
const REPORT_CAPACITY: usize = 4 * DeckId::COUNT;

/// The ends of the queues that stay on the control side.
pub struct ControlEnds {
    pub commands: Producer<RtCommand>,
    pub retired: Consumer<Retired>,
    pub reports: Consumer<DeckReport>,
    pub shared: Arc<Shared>,
}

/// The ends of the queues the processor keeps.
struct Queues {
    commands: Consumer<RtCommand>,
    retired: Producer<Retired>,
    reports: Producer<DeckReport>,
    recorded: Producer<Recorded>,
}

/// Builds a processor for `rate`, the ends the control side keeps and the end of the queue of
/// the recorder, which goes to the worker of each recording in turn.
pub fn channel(rate: SampleRate) -> (ControlEnds, Consumer<Recorded>, Processor) {
    let (commands, incoming) = RingBuffer::new(COMMAND_CAPACITY);
    // Each swap retires one item and at most `COMMAND_CAPACITY` swaps are in flight, so
    // this queue cannot fill up.
    let (outgoing, retired) = RingBuffer::new(COMMAND_CAPACITY + DeckId::COUNT);
    let (outgoing_reports, reports) = RingBuffer::new(REPORT_CAPACITY);
    let (outgoing_recorded, recorded) =
        RingBuffer::new(rate.hz() as usize * RECORDER_QUEUE_SECONDS);
    let shared = Arc::new(Shared::default());
    let processor = Processor::new(
        rate,
        Queues {
            commands: incoming,
            retired: outgoing,
            reports: outgoing_reports,
            recorded: outgoing_recorded,
        },
        Arc::clone(&shared),
    );
    (
        ControlEnds {
            commands,
            retired,
            reports,
            shared,
        },
        recorded,
        processor,
    )
}
