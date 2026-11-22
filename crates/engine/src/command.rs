//! The commands of the engine.
//!
//! Every action a user can take is one of these, with a stable identifier. The interface,
//! the keyboard, the CLI and, later, MIDI and MCP all build the same values, so there is one
//! path into the engine. [`SPECS`] describes each command for anything that needs to list,
//! label or build them from untyped input.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use platter_core::{Bpm, CueKind, DeckId, HotcueSlot, LoopSize, Stem, ValueError};
use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
use platter_dsp::effects::EffectKind;
use platter_dsp::effects::unit::{SLOTS, UnitMode};
use platter_dsp::eq::Band;
use platter_dsp::eq::EqModel;
use platter_dsp::filter::FilterModel;

use crate::effects::{FxRouting, FxUnit, KEYS, KNOBS, UNITS, index_of};
pub use crate::rt::MAX_KEY_SHIFT;
use crate::rt::{InputSource, InputTarget, LoopSource, Output};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Loads a file into the preview player, in the background, and plays it:
    /// `docs/behavior/browser.md`, *Preview player*.
    PreviewLoad {
        path: PathBuf,
    },
    /// Starts or stops the preview player.
    PreviewPlayPause,
    /// Moves the play position of the preview player to a share of its track, 0 to 1.
    PreviewSeek {
        position: f64,
    },
    /// Loads a file into a deck, in the background, with the stems the collection links to
    /// it when there are some: `docs/behavior/stems.md`.
    DeckLoad {
        deck: DeckId,
        path: PathBuf,
    },
    /// Loads a file into a deck without its stems.
    DeckLoadTrack {
        deck: DeckId,
        path: PathBuf,
    },
    /// Loads a file with its four stems into a deck, in the background:
    /// `docs/behavior/stems.md`. The stems are in the order of [`Stem::ALL`].
    DeckLoadStems {
        deck: DeckId,
        path: PathBuf,
        stems: Box<[PathBuf; Stem::COUNT]>,
    },
    /// Level of a stem, `0.0..=1.0`.
    DeckStemVolumeSet {
        deck: DeckId,
        stem: Stem,
        volume: f64,
    },
    /// Silences a stem, or brings it back at its level.
    DeckStemMuteToggle {
        deck: DeckId,
        stem: Stem,
    },
    /// Knob position of the filter of a stem, `-1.0..=1.0`.
    DeckStemFilterSet {
        deck: DeckId,
        stem: Stem,
        position: f64,
    },
    DeckStemFilterToggle {
        deck: DeckId,
        stem: Stem,
    },
    /// Share of a stem the effects get while its send is on, `0.0..=1.0`.
    DeckStemSendSet {
        deck: DeckId,
        stem: Stem,
        amount: f64,
    },
    DeckStemSendToggle {
        deck: DeckId,
        stem: Stem,
    },
    DeckEject {
        deck: DeckId,
    },
    DeckPlay {
        deck: DeckId,
    },
    DeckPause {
        deck: DeckId,
    },
    DeckPlayPause {
        deck: DeckId,
    },
    /// Sets the cue point at the current position.
    DeckCueSet {
        deck: DeckId,
    },
    /// Returns to the cue point and pauses there.
    DeckCueGoto {
        deck: DeckId,
    },
    /// The cue button, pressed or released. What it does depends on whether the deck is
    /// playing and on where it is: see `docs/behavior/transport.md`.
    DeckCue {
        deck: DeckId,
        pressed: bool,
    },
    /// The cue-play button: pressing returns to the cue point, releasing starts playback.
    DeckCup {
        deck: DeckId,
        pressed: bool,
    },
    DeckSeek {
        deck: DeckId,
        seconds: f64,
    },
    /// Playback speed as a ratio of the original, `0.0..=2.0`.
    DeckTempoSet {
        deck: DeckId,
        ratio: f64,
    },
    /// Temporary speed offset as a fraction of the tempo, `-0.5..=0.5`; zero releases it.
    DeckNudge {
        deck: DeckId,
        amount: f64,
    },
    /// Moves to the next range of the tempo fader.
    DeckTempoRangeCycle {
        deck: DeckId,
    },
    /// Gives the tempo fader one of its ranges, in percent: 8, 16 or 50.
    DeckTempoRangeSet {
        deck: DeckId,
        percent: u32,
    },
    /// Sets the tempo of the beatgrid, in beats per minute.
    DeckGridBpmSet {
        deck: DeckId,
        bpm: f64,
    },
    DeckGridBpmScale {
        deck: DeckId,
        factor: BpmScale,
    },
    /// Adds `amount` beats per minute to the tempo of the beatgrid, `-10.0..=10.0`.
    DeckGridBpmStep {
        deck: DeckId,
        amount: f64,
    },
    /// One tap of the TAP button; the tempo is set from the fourth tap on.
    DeckGridTap {
        deck: DeckId,
    },
    /// Sets a grid marker at the play position.
    DeckGridMarkerSet {
        deck: DeckId,
    },
    /// Moves the marker on or before the play position, `-1.0..=1.0` seconds.
    DeckGridMarkerMove {
        deck: DeckId,
        seconds: f64,
    },
    /// Gives the marker on or before the play position a tempo of its own up to the next
    /// marker, `20.0..=400.0` beats per minute.
    DeckGridMarkerBpmSet {
        deck: DeckId,
        bpm: f64,
    },
    /// Gives the marker on or before the play position the tempo of the grid again.
    DeckGridMarkerBpmClear {
        deck: DeckId,
    },
    /// Deletes the marker on or before the play position.
    DeckGridMarkerDelete {
        deck: DeckId,
    },
    /// Jumps to the marker before the play position.
    DeckGridMarkerPrevious {
        deck: DeckId,
    },
    /// Jumps to the marker after the play position.
    DeckGridMarkerNext {
        deck: DeckId,
    },
    DeckGridLockToggle {
        deck: DeckId,
    },
    /// Replaces the grid with the tempo and the first beat the analysis found.
    DeckGridReset {
        deck: DeckId,
    },
    /// Sets the tempo the analysis found, keeping the markers.
    DeckGridBpmReset {
        deck: DeckId,
    },
    /// Switches the loop off, or starts one of the selected size at the play position.
    DeckLoopToggle {
        deck: DeckId,
    },
    /// Sets the in point of a loop at the play position.
    DeckLoopIn {
        deck: DeckId,
    },
    /// Ends the loop at the play position and switches it on.
    DeckLoopOut {
        deck: DeckId,
    },
    /// Selects the loop size before or after; a loop that is on takes it.
    DeckLoopSizeStep {
        deck: DeckId,
        direction: SizeStep,
    },
    /// Moves the play position by the move size.
    DeckBeatjump {
        deck: DeckId,
        direction: Direction,
    },
    /// Moves what the move mode says by the move size.
    DeckMove {
        deck: DeckId,
        direction: Direction,
    },
    DeckMoveModeSet {
        deck: DeckId,
        mode: MoveMode,
    },
    DeckMoveSizeStep {
        deck: DeckId,
        direction: SizeStep,
    },
    /// Switches the fine steps of the moves on or off.
    DeckMoveFineToggle {
        deck: DeckId,
    },
    /// The key of a hotcue, pressed or released: stores into an empty slot, goes to a stored
    /// cue. See `docs/behavior/cues.md`.
    DeckHotcueTrigger {
        deck: DeckId,
        slot: HotcueSlot,
        pressed: bool,
    },
    DeckHotcueDelete {
        deck: DeckId,
        slot: HotcueSlot,
    },
    /// Moves the selected cue to a slot.
    DeckHotcueMap {
        deck: DeckId,
        slot: HotcueSlot,
    },
    /// Stores the floating cue, or the loop that is on, in the first empty slot.
    DeckCueStore {
        deck: DeckId,
    },
    /// Goes to the stored cue before or after the play position.
    DeckCueSelect {
        deck: DeckId,
        direction: CueStep,
    },
    /// Names the selected cue.
    DeckCueRename {
        deck: DeckId,
        name: String,
    },
    DeckCueTypeSet {
        deck: DeckId,
        kind: CueKind,
    },
    /// Deletes the selected cue.
    DeckCueDelete {
        deck: DeckId,
    },
    MixerGainSet {
        deck: DeckId,
        db: f64,
    },
    /// Sets the gain of the channel to the autogain value of its track:
    /// `docs/behavior/autogain.md`.
    MixerGainRestore {
        deck: DeckId,
    },
    /// Switches autogain on or off for both channels.
    MixerAutogainToggle,
    /// Knob position `0.0..=1.0`: zero removes the band, the centre leaves it untouched.
    MixerEqSet {
        deck: DeckId,
        band: Band,
        position: f64,
    },
    /// Removes a band completely, or gives it back to its knob.
    MixerEqKillToggle {
        deck: DeckId,
        band: Band,
    },
    /// Knob position `-1.0..=1.0`: low-pass below zero, high-pass above, off at zero.
    MixerFilterSet {
        deck: DeckId,
        position: f64,
    },
    /// Knob position `-1.0..=1.0`: left below zero, right above, both sides at zero.
    MixerPanSet {
        deck: DeckId,
        position: f64,
    },
    /// Sends the deck to the headphones, or stops sending it.
    MixerCueToggle {
        deck: DeckId,
    },
    /// Balance in the headphones, `0.0..=1.0`: the pre-listened decks alone at zero, the
    /// master alone at one.
    MixerCueMixSet {
        position: f64,
    },
    MixerCueVolumeSet {
        db: f64,
    },
    /// Fader position `0.0..=1.0`.
    MixerFaderSet {
        deck: DeckId,
        position: f64,
    },
    /// Crossfader position `0.0..=1.0`: zero is deck A alone, one is deck B alone.
    MixerCrossfaderSet {
        position: f64,
    },
    MixerCrossfaderCurveSet {
        curve: CrossfaderCurve,
    },
    /// The model of the equaliser of every channel: `docs/behavior/mixer.md`.
    MixerEqModelSet {
        model: EqModel,
    },
    /// The model of the filter of every channel.
    MixerFilterModelSet {
        model: FilterModel,
    },
    /// Puts the channel on a side of the crossfader, or past it.
    MixerCrossfaderAssign {
        deck: DeckId,
        side: CrossfaderSide,
    },
    /// Moves the crossfader [`CROSSFADER_STEP`] of its travel towards a side.
    MixerCrossfaderStep {
        direction: CrossfaderDirection,
    },
    /// Moves the crossfader to an end by itself: `docs/behavior/mixer.md`.
    MixerCrossfaderAutoFade {
        direction: CrossfaderDirection,
    },
    /// How many beats of the master clock an automatic fade across the whole travel takes.
    MixerCrossfaderFadeBeatsSet {
        beats: f64,
    },
    /// Mixes inside Platter, or sends each deck to its own outputs:
    /// `docs/behavior/external-mixer.md`.
    MixerModeSet {
        mode: MixerMode,
    },
    /// In external mode, lets the gain and the equaliser act on the decks, or not.
    MixerExternalEqGainToggle,
    /// In external mode, lets the master volume act on the decks, or not.
    MixerExternalMasterVolumeToggle,
    /// Volume of the auxiliary input in the master, `0.0..=1.0`:
    /// `docs/behavior/live-input.md`.
    MixerAuxSet {
        level: f64,
    },
    /// The deck plays the input routed to it, or its track.
    DeckTypeSet {
        deck: DeckId,
        live: bool,
    },
    /// The input a deck or the auxiliary input takes; `None` leaves it silent.
    InputRoutingSet {
        target: InputTarget,
        source: Option<InputSource>,
    },
    /// The mode of an effect unit: `docs/behavior/effects.md`.
    FxUnitModeSet {
        unit: FxUnit,
        mode: UnitMode,
    },
    /// Dry/wet of a unit, `0.0..=1.0`: the input alone at zero.
    FxUnitDryWetSet {
        unit: FxUnit,
        position: f64,
    },
    FxUnitRoutingSet {
        unit: FxUnit,
        routing: FxRouting,
    },
    /// Puts the knobs and the keys of single mode back to the defaults of its effect.
    FxUnitReset {
        unit: FxUnit,
    },
    /// Keeps the controls of single mode as the start of its effect in this unit.
    FxUnitSnapshotSave {
        unit: FxUnit,
    },
    /// Switches whether a newly chosen effect keeps the values of the controls, for every unit.
    FxKeepValuesToggle,
    /// The effect of a slot of group mode, from 0; `None` empties it.
    FxSlotEffectSet {
        unit: FxUnit,
        slot: usize,
        effect: Option<EffectKind>,
    },
    /// The amount knob of a slot, `0.0..=1.0`.
    FxSlotAmountSet {
        unit: FxUnit,
        slot: usize,
        position: f64,
    },
    FxSlotToggle {
        unit: FxUnit,
        slot: usize,
    },
    /// The effect of single mode; `None` empties it.
    FxSingleEffectSet {
        unit: FxUnit,
        effect: Option<EffectKind>,
    },
    /// A knob of single mode, from 0, at `0.0..=1.0`.
    FxSingleKnobSet {
        unit: FxUnit,
        knob: usize,
        position: f64,
    },
    /// A key of single mode, from 0.
    FxSingleKeyToggle {
        unit: FxUnit,
        key: usize,
    },
    FxSingleToggle {
        unit: FxUnit,
    },
    /// Puts the channel through the unit, or takes it out.
    MixerFxAssignToggle {
        deck: DeckId,
        unit: FxUnit,
    },
    /// The effect of the mixer effect knob of a channel.
    MixerEffectSelect {
        deck: DeckId,
        effect: EffectKind,
    },
    /// The mixer effect knob, `0.0..=1.0`: off at zero.
    MixerEffectSet {
        deck: DeckId,
        position: f64,
    },
    MasterGainSet {
        db: f64,
    },
    /// Plays a short tone on one output, to check the routing.
    MasterTestTone {
        output: Output,
    },
    /// The Limiter key: switches the maximiser of the master in or out
    /// (`docs/behavior/master.md`).
    MasterLimiterToggle,
    /// Gain before the maximiser, `0.0..=12.0` dB.
    MasterMaximiserDriveSet {
        db: f64,
    },
    /// Highest sample peak the maximiser lets out, `-12.0..=0.0` dB of full scale.
    MasterMaximiserCeilingSet {
        db: f64,
    },
    /// Time the maximiser takes to recover 90 % of its gain reduction, `10.0..=1000.0` ms.
    MasterMaximiserReleaseSet {
        ms: f64,
    },
    DeckSyncToggle {
        deck: DeckId,
    },
    /// Keeps the key of the track while its tempo changes, or stops doing so.
    DeckKeyLockToggle {
        deck: DeckId,
    },
    /// Turns the tick on each beat of the deck in the headphones on or off:
    /// `docs/behavior/beatgrid.md`, Beat tick.
    DeckBeatTickToggle {
        deck: DeckId,
    },
    /// Moves the key of the deck by a semitone and turns key lock on:
    /// `docs/behavior/keylock.md`, Transposition.
    DeckKeyStep {
        deck: DeckId,
        direction: SizeStep,
    },
    /// Sets the shift of the key in semitones, `-12..=12`, and turns key lock on.
    DeckKeySet {
        deck: DeckId,
        semitones: f64,
    },
    /// Turns key lock on with the key the deck has at its current speed.
    DeckKeyPersist {
        deck: DeckId,
    },
    /// Turns flux on or off: `docs/behavior/flux.md`.
    DeckFluxToggle {
        deck: DeckId,
    },
    /// Plays the track backwards while held: `docs/behavior/flux.md`.
    DeckReverse {
        deck: DeckId,
        pressed: bool,
    },
    /// Makes the deck the tempo master, while the mode of the clock is not automatic.
    DeckMasterSet {
        deck: DeckId,
    },
    ClockModeSet {
        mode: ClockMode,
    },
    /// Tempo of the clock in beats per minute.
    ClockTempoSet {
        bpm: f64,
    },
    /// Adds beats per minute to the tempo of the clock.
    ClockTempoStep {
        amount: f64,
    },
    ClockTap,
    /// The clock runs this fraction slower or faster until it is set back to zero.
    ClockBend {
        amount: f64,
    },
    /// A click on every beat of the clock, on the headphones.
    ClockMetronomeToggle,
    /// Starts or stops sending the master clock as MIDI clock: `docs/behavior/midi-clock.md`.
    ClockMidiSendToggle,
    /// Stop and Start of the MIDI clock again on the next beat.
    ClockMidiRestart,
    /// The master clock runs on its own, or follows a received MIDI clock.
    ClockSourceSet {
        external: bool,
    },
    GlobalSyncModeSet {
        mode: SyncMode,
    },
    /// New cues and loops land on the nearest beat.
    GlobalSnapToggle,
    /// Whether a track with a cue of the type load starts at it.
    GlobalLoadMarkerToggle,
    /// Switches whether a deck crossing a fade-out cue starts its partner from its fade-in cue.
    GlobalFadeMarkersToggle,
    /// A playing deck that goes to a stored cue keeps its place in the beat.
    GlobalQuantizeToggle,
    /// Starts recording the master into a file, or stops the recording:
    /// `docs/behavior/recording.md`.
    RecorderToggle,
    /// Closes the file of the recording and goes on in a new one, without losing a frame.
    RecorderCut,
    /// The mix recorder records the internal mix, or the input routed to the recorders.
    RecorderSourceSet {
        external: bool,
    },
    /// Rec of the loop recorder: waits for the beat and records a pass, or starts or ends an
    /// overdub: `docs/behavior/recording.md`, *Loop recorder*.
    LoopRecorderRecord,
    /// Pauses the loop or plays it on.
    LoopRecorderPlay,
    LoopRecorderUndo,
    LoopRecorderRedo,
    LoopRecorderDelete,
    /// The length of the next loop, 1, 2, 4 or 8 bars.
    LoopRecorderSizeSet {
        bars: f64,
    },
    LoopRecorderSourceSet {
        source: LoopSource,
    },
    /// 0 is the mix alone, 0.5 both at full level, 1 the loop alone.
    LoopRecorderDryWetSet {
        amount: f64,
    },
}

/// Kind of a command parameter, for anything that builds commands from untyped input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Deck,
    Number {
        min: f64,
        max: f64,
    },
    Choice(&'static [&'static str]),
    Path,
    /// Free text, such as the name of a device.
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandSpec {
    /// Stable identifier. It never changes once released.
    pub id: &'static str,
    /// Text for menus and lists, in English; the interface translates it by identifier.
    pub label: &'static str,
    pub params: &'static [ParamSpec],
}

const DECK: ParamSpec = ParamSpec {
    name: "deck",
    kind: ParamKind::Deck,
};

const STEM: ParamSpec = ParamSpec {
    name: "stem",
    kind: ParamKind::Choice(&Stem::NAMES),
};

const fn path(name: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Path,
    }
}

const fn number(name: &'static str, min: f64, max: f64) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Number { min, max },
    }
}

pub const MAX_SEEK_SECONDS: f64 = 86_400.0;
pub const MAX_NUDGE: f64 = 0.5;
pub const MAX_BPM_STEP: f64 = 10.0;
pub const MAX_MARKER_MOVE_SECONDS: f64 = 1.0;
pub const MAX_CHANNEL_GAIN_DB: f64 = 12.0;
pub const MAX_MASTER_GAIN_DB: f64 = 12.0;
pub const MAX_HEADPHONE_GAIN_DB: f64 = 12.0;
const MIN_GAIN_DB: f64 = -90.0;
/// The ranges of the maximiser: `docs/behavior/master.md`.
pub const MAX_DRIVE_DB: f64 = 12.0;
pub const MIN_CEILING_DB: f64 = -12.0;
pub const MAX_CEILING_DB: f64 = 0.0;
pub const MIN_RELEASE_MS: f64 = 10.0;
pub const MAX_RELEASE_MS: f64 = 1_000.0;
const BANDS: &[&str] = &["low", "mid", "high"];
const CURVES: &[&str] = &["smooth", "linear", "sharp"];
const SIDES: &[&str] = &["left", "thru", "right"];
const CROSSFADER_DIRECTIONS: &[&str] = &["left", "right"];
const MIXER_MODES: &[&str] = &["internal", "external"];
const DECK_TYPES: &[&str] = &["track", "live"];
const INPUT_TARGETS: &[&str] = &[
    "deck_a", "deck_b", "deck_c", "deck_d", "aux", "recorder", "fx_send",
];
const RECORDER_SOURCES: &[&str] = &["internal", "external"];
/// The sources of the input routing, and `none`.
pub const INPUT_SOURCES: &[&str] = &[
    "none", "1", "2", "3", "4", "5", "6", "7", "8", "1-2", "3-4", "5-6", "7-8",
];
const EQ_MODELS: &[&str] = &["smooth", "soft", "isolator", "steep"];
const FILTER_MODELS: &[&str] = &["gentle", "steep", "ladder"];
const UNIT: ParamSpec = number("unit", 1.0, UNITS as f64);
const FX_SLOT: ParamSpec = number("slot", 1.0, SLOTS as f64);
const UNIT_MODES: &[&str] = &["group", "single"];
const ROUTINGS: &[&str] = &["insert", "post_fader", "send"];
/// The effects of a unit, and `none` for an empty place.
pub const UNIT_EFFECTS: &[&str] = &[
    "none",
    "delay",
    "reverb",
    "flanger",
    "phaser",
    "filter",
    "gater",
    "slicer",
    "bitcrusher",
];
/// The effects of the mixer effect knob.
pub const MIXER_EFFECTS: &[&str] = &["delay", "reverb", "gater", "bitcrusher"];
/// The part of the travel of the crossfader one step moves it.
pub const CROSSFADER_STEP: f64 = 0.1;
pub const MIN_FADE_BEATS: f64 = 1.0;
pub const MAX_FADE_BEATS: f64 = 64.0;
const OUTPUTS: &[&str] = &["master", "headphones"];
const BUTTON_ACTIONS: &[&str] = &["press", "release"];
const BPM_SCALES: &[&str] = &["double", "half"];
const DIRECTIONS: &[&str] = &["back", "forward"];
const SIZE_STEPS: &[&str] = &["down", "up"];
const MOVE_MODES: &[&str] = &["beatjump", "loop", "loop_in", "loop_out"];
const CUE_STEPS: &[&str] = &["previous", "next"];
const CUE_TYPES: &[&str] = &["cue", "fade_in", "fade_out", "load", "loop"];
const CLOCK_MODES: &[&str] = &["auto", "master"];
const CLOCK_SOURCES: &[&str] = &["internal", "external"];
const SYNC_MODES: &[&str] = &["tempo", "beat"];

/// The sources of the loop recorder, by name: the mix, then the decks.
pub const LOOP_SOURCES: &[&str] = &["master", "a", "b", "c", "d", "external"];

/// Who the tempo master is: `docs/behavior/sync.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    /// The deck that starts to play, chosen by the engine.
    Auto,
    /// The clock, until `deck.master.set` names a deck.
    Master,
}

/// What the sync of a deck locks to the tempo master.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncMode {
    /// The tempo alone.
    Tempo,
    /// The tempo and the place in the beat.
    #[default]
    Beat,
}

impl SyncMode {
    pub const ALL: [Self; 2] = [Self::Tempo, Self::Beat];

    pub fn name(self) -> &'static str {
        match self {
            Self::Tempo => "tempo",
            Self::Beat => "beat",
        }
    }
}

/// Which neighbour of the play position `deck.cue.select` goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueStep {
    Previous,
    Next,
}

/// Which way a jump or a move goes along the track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Back,
    Forward,
}

impl Direction {
    pub fn sign(self) -> f64 {
        match self {
            Self::Back => -1.0,
            Self::Forward => 1.0,
        }
    }
}

/// Which end of the crossfader a step or an automatic fade goes towards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossfaderDirection {
    Left,
    Right,
}

impl CrossfaderDirection {
    /// The position of the crossfader at that end.
    pub fn end(self) -> f64 {
        match self {
            Self::Left => 0.0,
            Self::Right => 1.0,
        }
    }
}

/// Whether Platter mixes the decks or a hardware mixer does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MixerMode {
    #[default]
    Internal,
    /// Each deck goes to its own pair of outputs.
    External,
}

impl MixerMode {
    pub const ALL: [Self; 2] = [Self::Internal, Self::External];

    pub fn name(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::External => "external",
        }
    }
}

/// Which way a step through a list of sizes goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeStep {
    Down,
    Up,
}

/// What `deck.move` moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MoveMode {
    /// The play position.
    #[default]
    Beatjump,
    /// Both ends of the loop.
    Loop,
    LoopIn,
    LoopOut,
}

impl MoveMode {
    pub const ALL: [Self; 4] = [Self::Beatjump, Self::Loop, Self::LoopIn, Self::LoopOut];

    pub fn name(self) -> &'static str {
        match self {
            Self::Beatjump => "beatjump",
            Self::Loop => "loop",
            Self::LoopIn => "loop_in",
            Self::LoopOut => "loop_out",
        }
    }
}

/// How far a jump or a move goes: one of the preset sizes or the length of the loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveSize {
    Beats(LoopSize),
    /// The length of the loop that is on, or the selected loop size without one. It is the
    /// entry after the largest preset size.
    Loop,
}

impl Default for MoveSize {
    fn default() -> Self {
        Self::Beats(LoopSize::ONE_BEAT)
    }
}

impl MoveSize {
    /// The entry before or after this one, when there is one.
    pub fn stepped(self, direction: SizeStep) -> Option<Self> {
        match (self, direction) {
            (Self::Beats(size), SizeStep::Down) => size.smaller().map(Self::Beats),
            (Self::Beats(size), SizeStep::Up) => {
                Some(size.larger().map_or(Self::Loop, Self::Beats))
            }
            (Self::Loop, SizeStep::Down) => Some(Self::Beats(LoopSize::LARGEST)),
            (Self::Loop, SizeStep::Up) => None,
        }
    }
}

/// How `deck.grid.bpm.scale` changes the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BpmScale {
    Double,
    Half,
}

impl BpmScale {
    pub fn factor(self) -> f64 {
        match self {
            Self::Double => 2.0,
            Self::Half => 0.5,
        }
    }
}

const BAND: ParamSpec = ParamSpec {
    name: "band",
    kind: ParamKind::Choice(BANDS),
};

const DIRECTION: ParamSpec = ParamSpec {
    name: "direction",
    kind: ParamKind::Choice(DIRECTIONS),
};

const SIZE_STEP: ParamSpec = ParamSpec {
    name: "direction",
    kind: ParamKind::Choice(SIZE_STEPS),
};

const SLOT: ParamSpec = number("slot", 1.0, HotcueSlot::COUNT as f64);

/// Whether a button that acts while held is being pressed or released.
const BUTTON_ACTION: ParamSpec = ParamSpec {
    name: "action",
    kind: ParamKind::Choice(BUTTON_ACTIONS),
};

/// Every command, in the order a list shows them.
pub const SPECS: &[CommandSpec] = &[
    CommandSpec {
        id: "deck.load_track",
        label: "Load track without its stems",
        params: &[DECK, path("path")],
    },
    CommandSpec {
        id: "deck.load_stems",
        label: "Load track with its stems",
        params: &[
            DECK,
            path("path"),
            path("drums"),
            path("bass"),
            path("other"),
            path("vocals"),
        ],
    },
    CommandSpec {
        id: "deck.stem.volume.set",
        label: "Set stem volume",
        params: &[DECK, STEM, number("volume", 0.0, 1.0)],
    },
    CommandSpec {
        id: "deck.stem.mute.toggle",
        label: "Mute stem",
        params: &[DECK, STEM],
    },
    CommandSpec {
        id: "deck.stem.filter.set",
        label: "Set stem filter",
        params: &[DECK, STEM, number("position", -1.0, 1.0)],
    },
    CommandSpec {
        id: "deck.stem.filter.toggle",
        label: "Turn stem filter on or off",
        params: &[DECK, STEM],
    },
    CommandSpec {
        id: "deck.stem.send.set",
        label: "Set stem effect send",
        params: &[DECK, STEM, number("amount", 0.0, 1.0)],
    },
    CommandSpec {
        id: "deck.stem.send.toggle",
        label: "Turn stem effect send on or off",
        params: &[DECK, STEM],
    },
    CommandSpec {
        id: "preview.load",
        label: "Preview a track",
        params: &[ParamSpec {
            name: "path",
            kind: ParamKind::Path,
        }],
    },
    CommandSpec {
        id: "preview.play_pause",
        label: "Play or pause the preview",
        params: &[],
    },
    CommandSpec {
        id: "preview.seek",
        label: "Seek in the preview",
        params: &[number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "deck.load",
        label: "Load track",
        params: &[
            DECK,
            ParamSpec {
                name: "path",
                kind: ParamKind::Path,
            },
        ],
    },
    CommandSpec {
        id: "deck.eject",
        label: "Eject track",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.play",
        label: "Play",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.pause",
        label: "Pause",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.play_pause",
        label: "Play or pause",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.cue.set",
        label: "Set cue point",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.cue.goto",
        label: "Return to cue point",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.cue",
        label: "Cue",
        params: &[DECK, BUTTON_ACTION],
    },
    CommandSpec {
        id: "deck.cup",
        label: "Cue and play",
        params: &[DECK, BUTTON_ACTION],
    },
    CommandSpec {
        id: "deck.seek",
        label: "Jump to position",
        params: &[DECK, number("seconds", 0.0, MAX_SEEK_SECONDS)],
    },
    CommandSpec {
        id: "deck.tempo.set",
        label: "Set tempo",
        params: &[DECK, number("ratio", 0.0, 2.0)],
    },
    CommandSpec {
        id: "deck.nudge",
        label: "Bend tempo",
        params: &[DECK, number("amount", -MAX_NUDGE, MAX_NUDGE)],
    },
    CommandSpec {
        id: "deck.tempo.range.cycle",
        label: "Next tempo range",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.tempo.range.set",
        label: "Set tempo range",
        params: &[DECK, number("percent", 8.0, 50.0)],
    },
    CommandSpec {
        id: "deck.grid.bpm.set",
        label: "Set beatgrid tempo",
        params: &[DECK, number("bpm", Bpm::MIN, Bpm::MAX)],
    },
    CommandSpec {
        id: "deck.grid.bpm.scale",
        label: "Double or halve beatgrid tempo",
        params: &[
            DECK,
            ParamSpec {
                name: "factor",
                kind: ParamKind::Choice(BPM_SCALES),
            },
        ],
    },
    CommandSpec {
        id: "deck.grid.bpm.step",
        label: "Step beatgrid tempo",
        params: &[DECK, number("amount", -MAX_BPM_STEP, MAX_BPM_STEP)],
    },
    CommandSpec {
        id: "deck.grid.tap",
        label: "Tap tempo",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.marker.set",
        label: "Set grid marker",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.marker.move",
        label: "Move grid marker",
        params: &[
            DECK,
            number("seconds", -MAX_MARKER_MOVE_SECONDS, MAX_MARKER_MOVE_SECONDS),
        ],
    },
    CommandSpec {
        id: "deck.grid.marker.bpm.set",
        label: "Set the tempo of a grid marker",
        params: &[DECK, number("bpm", Bpm::MIN, Bpm::MAX)],
    },
    CommandSpec {
        id: "deck.grid.marker.bpm.clear",
        label: "Give a grid marker the tempo of the grid",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.marker.delete",
        label: "Delete grid marker",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.marker.previous",
        label: "Previous grid marker",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.marker.next",
        label: "Next grid marker",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.lock.toggle",
        label: "Lock beatgrid",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.reset",
        label: "Reset beatgrid",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.grid.bpm.reset",
        label: "Reset beatgrid tempo",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.loop.toggle",
        label: "Loop",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.loop.in",
        label: "Loop in",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.loop.out",
        label: "Loop out",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.loop.size.step",
        label: "Halve or double loop size",
        params: &[DECK, SIZE_STEP],
    },
    CommandSpec {
        id: "deck.beatjump",
        label: "Beatjump",
        params: &[DECK, DIRECTION],
    },
    CommandSpec {
        id: "deck.move",
        label: "Move",
        params: &[DECK, DIRECTION],
    },
    CommandSpec {
        id: "deck.move.mode.set",
        label: "Set move mode",
        params: &[
            DECK,
            ParamSpec {
                name: "mode",
                kind: ParamKind::Choice(MOVE_MODES),
            },
        ],
    },
    CommandSpec {
        id: "deck.move.size.step",
        label: "Step move size",
        params: &[DECK, SIZE_STEP],
    },
    CommandSpec {
        id: "deck.move.fine.toggle",
        label: "Fine move steps",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.hotcue.trigger",
        label: "Hotcue",
        params: &[DECK, SLOT, BUTTON_ACTION],
    },
    CommandSpec {
        id: "deck.hotcue.delete",
        label: "Delete hotcue",
        params: &[DECK, SLOT],
    },
    CommandSpec {
        id: "deck.hotcue.map",
        label: "Move selected cue to hotcue",
        params: &[DECK, SLOT],
    },
    CommandSpec {
        id: "deck.cue.store",
        label: "Store cue",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.cue.select",
        label: "Previous or next stored cue",
        params: &[
            DECK,
            ParamSpec {
                name: "direction",
                kind: ParamKind::Choice(CUE_STEPS),
            },
        ],
    },
    CommandSpec {
        id: "deck.cue.rename",
        label: "Rename selected cue",
        params: &[
            DECK,
            ParamSpec {
                name: "name",
                kind: ParamKind::Text,
            },
        ],
    },
    CommandSpec {
        id: "deck.cue.type.set",
        label: "Set type of selected cue",
        params: &[
            DECK,
            ParamSpec {
                name: "type",
                kind: ParamKind::Choice(CUE_TYPES),
            },
        ],
    },
    CommandSpec {
        id: "deck.cue.delete",
        label: "Delete selected cue",
        params: &[DECK],
    },
    CommandSpec {
        id: "mixer.gain.set",
        label: "Set channel gain",
        params: &[DECK, number("db", MIN_GAIN_DB, MAX_CHANNEL_GAIN_DB)],
    },
    CommandSpec {
        id: "mixer.eq.set",
        label: "Set equaliser band",
        params: &[DECK, BAND, number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.eq.kill.toggle",
        label: "Kill equaliser band",
        params: &[DECK, BAND],
    },
    CommandSpec {
        id: "mixer.filter.set",
        label: "Set channel filter",
        params: &[DECK, number("position", -1.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.pan.set",
        label: "Set channel pan",
        params: &[DECK, number("position", -1.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.gain.restore",
        label: "Restore autogain",
        params: &[DECK],
    },
    CommandSpec {
        id: "mixer.autogain.toggle",
        label: "Autogain",
        params: &[],
    },
    CommandSpec {
        id: "mixer.cue.toggle",
        label: "Pre-listen",
        params: &[DECK],
    },
    CommandSpec {
        id: "mixer.cue.mix.set",
        label: "Set headphone mix",
        params: &[number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.cue.volume.set",
        label: "Set headphone volume",
        params: &[number("db", MIN_GAIN_DB, MAX_HEADPHONE_GAIN_DB)],
    },
    CommandSpec {
        id: "mixer.fader.set",
        label: "Set channel fader",
        params: &[DECK, number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.crossfader.set",
        label: "Set crossfader",
        params: &[number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "mixer.crossfader.curve.set",
        label: "Set crossfader curve",
        params: &[ParamSpec {
            name: "curve",
            kind: ParamKind::Choice(CURVES),
        }],
    },
    CommandSpec {
        id: "mixer.eq.model.set",
        label: "Set equaliser model",
        params: &[ParamSpec {
            name: "model",
            kind: ParamKind::Choice(EQ_MODELS),
        }],
    },
    CommandSpec {
        id: "mixer.filter.model.set",
        label: "Set filter model",
        params: &[ParamSpec {
            name: "model",
            kind: ParamKind::Choice(FILTER_MODELS),
        }],
    },
    CommandSpec {
        id: "mixer.mode.set",
        label: "Mixer mode",
        params: &[ParamSpec {
            name: "mode",
            kind: ParamKind::Choice(MIXER_MODES),
        }],
    },
    CommandSpec {
        id: "mixer.external.eq_gain.toggle",
        label: "Gain and equaliser in external mode",
        params: &[],
    },
    CommandSpec {
        id: "mixer.external.master_volume.toggle",
        label: "Master volume in external mode",
        params: &[],
    },
    CommandSpec {
        id: "mixer.aux.set",
        label: "Auxiliary input volume",
        params: &[number("level", 0.0, 1.0)],
    },
    CommandSpec {
        id: "deck.type.set",
        label: "Deck type",
        params: &[
            DECK,
            ParamSpec {
                name: "type",
                kind: ParamKind::Choice(DECK_TYPES),
            },
        ],
    },
    CommandSpec {
        id: "input.routing.set",
        label: "Route an input",
        params: &[
            ParamSpec {
                name: "target",
                kind: ParamKind::Choice(INPUT_TARGETS),
            },
            ParamSpec {
                name: "input",
                kind: ParamKind::Choice(INPUT_SOURCES),
            },
        ],
    },
    CommandSpec {
        id: "mixer.crossfader.step",
        label: "Step the crossfader",
        params: &[ParamSpec {
            name: "direction",
            kind: ParamKind::Choice(CROSSFADER_DIRECTIONS),
        }],
    },
    CommandSpec {
        id: "mixer.crossfader.auto_fade",
        label: "Auto fade",
        params: &[ParamSpec {
            name: "direction",
            kind: ParamKind::Choice(CROSSFADER_DIRECTIONS),
        }],
    },
    CommandSpec {
        id: "mixer.crossfader.fade_beats.set",
        label: "Set the length of an auto fade",
        params: &[number("beats", MIN_FADE_BEATS, MAX_FADE_BEATS)],
    },
    CommandSpec {
        id: "mixer.xfader_assign.set",
        label: "Assign to crossfader side",
        params: &[
            DECK,
            ParamSpec {
                name: "side",
                kind: ParamKind::Choice(SIDES),
            },
        ],
    },
    CommandSpec {
        id: "fx.unit.mode.set",
        label: "Set the mode of an effect unit",
        params: &[
            UNIT,
            ParamSpec {
                name: "mode",
                kind: ParamKind::Choice(UNIT_MODES),
            },
        ],
    },
    CommandSpec {
        id: "fx.unit.dry_wet.set",
        label: "Set the dry/wet of an effect unit",
        params: &[UNIT, number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "fx.unit.routing.set",
        label: "Set the routing of an effect unit",
        params: &[
            UNIT,
            ParamSpec {
                name: "routing",
                kind: ParamKind::Choice(ROUTINGS),
            },
        ],
    },
    CommandSpec {
        id: "fx.unit.reset",
        label: "Reset an effect unit",
        params: &[UNIT],
    },
    CommandSpec {
        id: "fx.unit.snapshot.save",
        label: "Save a snapshot of an effect",
        params: &[UNIT],
    },
    CommandSpec {
        id: "fx.keep_values.toggle",
        label: "Keep the values when the effect changes",
        params: &[],
    },
    CommandSpec {
        id: "fx.slot.effect.set",
        label: "Choose the effect of a slot",
        params: &[
            UNIT,
            FX_SLOT,
            ParamSpec {
                name: "effect",
                kind: ParamKind::Choice(UNIT_EFFECTS),
            },
        ],
    },
    CommandSpec {
        id: "fx.slot.amount.set",
        label: "Set the amount of a slot",
        params: &[UNIT, FX_SLOT, number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "fx.slot.toggle",
        label: "Switch the effect of a slot",
        params: &[UNIT, FX_SLOT],
    },
    CommandSpec {
        id: "fx.single.effect.set",
        label: "Choose the effect of single mode",
        params: &[
            UNIT,
            ParamSpec {
                name: "effect",
                kind: ParamKind::Choice(UNIT_EFFECTS),
            },
        ],
    },
    CommandSpec {
        id: "fx.single.knob.set",
        label: "Set a knob of single mode",
        params: &[
            UNIT,
            number("knob", 1.0, KNOBS as f64),
            number("position", 0.0, 1.0),
        ],
    },
    CommandSpec {
        id: "fx.single.key.toggle",
        label: "Switch a key of single mode",
        params: &[UNIT, number("key", 1.0, KEYS as f64)],
    },
    CommandSpec {
        id: "fx.single.toggle",
        label: "Switch the effect of single mode",
        params: &[UNIT],
    },
    CommandSpec {
        id: "mixer.fx_assign.toggle",
        label: "Send to an effect unit",
        params: &[DECK, UNIT],
    },
    CommandSpec {
        id: "mixer.effect.select",
        label: "Choose the mixer effect",
        params: &[
            DECK,
            ParamSpec {
                name: "effect",
                kind: ParamKind::Choice(MIXER_EFFECTS),
            },
        ],
    },
    CommandSpec {
        id: "mixer.effect.set",
        label: "Set the mixer effect",
        params: &[DECK, number("position", 0.0, 1.0)],
    },
    CommandSpec {
        id: "master.gain.set",
        label: "Set main volume",
        params: &[number("db", MIN_GAIN_DB, MAX_MASTER_GAIN_DB)],
    },
    CommandSpec {
        id: "master.test_tone.play",
        label: "Play test tone",
        params: &[ParamSpec {
            name: "output",
            kind: ParamKind::Choice(OUTPUTS),
        }],
    },
    CommandSpec {
        id: "master.limiter.toggle",
        label: "Limiter",
        params: &[],
    },
    CommandSpec {
        id: "master.maximiser.drive.set",
        label: "Set maximiser drive",
        params: &[number("db", 0.0, MAX_DRIVE_DB)],
    },
    CommandSpec {
        id: "master.maximiser.ceiling.set",
        label: "Set maximiser ceiling",
        params: &[number("db", MIN_CEILING_DB, MAX_CEILING_DB)],
    },
    CommandSpec {
        id: "master.maximiser.release.set",
        label: "Set maximiser release",
        params: &[number("ms", MIN_RELEASE_MS, MAX_RELEASE_MS)],
    },
    CommandSpec {
        id: "deck.sync.toggle",
        label: "Sync",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.keylock.toggle",
        label: "Key lock",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.tick.toggle",
        label: "Beat tick",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.key.step",
        label: "Transpose by a semitone",
        params: &[DECK, SIZE_STEP],
    },
    CommandSpec {
        id: "deck.key.set",
        label: "Set key",
        params: &[DECK, number("semitones", -MAX_KEY_SHIFT, MAX_KEY_SHIFT)],
    },
    CommandSpec {
        id: "deck.key.persist",
        label: "Lock the current key",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.flux.toggle",
        label: "Flux",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.reverse",
        label: "Reverse",
        params: &[DECK, BUTTON_ACTION],
    },
    CommandSpec {
        id: "deck.master.set",
        label: "Make tempo master",
        params: &[DECK],
    },
    CommandSpec {
        id: "clock.mode.set",
        label: "Set tempo master mode",
        params: &[ParamSpec {
            name: "mode",
            kind: ParamKind::Choice(CLOCK_MODES),
        }],
    },
    CommandSpec {
        id: "clock.tempo.set",
        label: "Set clock tempo",
        params: &[number("bpm", Bpm::MIN, Bpm::MAX)],
    },
    CommandSpec {
        id: "clock.tempo.step",
        label: "Step clock tempo",
        params: &[number("amount", -MAX_BPM_STEP, MAX_BPM_STEP)],
    },
    CommandSpec {
        id: "clock.tap",
        label: "Tap clock tempo",
        params: &[],
    },
    CommandSpec {
        id: "clock.bend",
        label: "Bend clock tempo",
        params: &[number("amount", -MAX_NUDGE, MAX_NUDGE)],
    },
    CommandSpec {
        id: "clock.metronome.toggle",
        label: "Metronome",
        params: &[],
    },
    CommandSpec {
        id: "clock.midi.send.toggle",
        label: "Send MIDI clock",
        params: &[],
    },
    CommandSpec {
        id: "clock.midi.restart",
        label: "Restart MIDI clock",
        params: &[],
    },
    CommandSpec {
        id: "clock.source.set",
        label: "Clock source",
        params: &[ParamSpec {
            name: "source",
            kind: ParamKind::Choice(CLOCK_SOURCES),
        }],
    },
    CommandSpec {
        id: "global.snap.toggle",
        label: "Snap",
        params: &[],
    },
    CommandSpec {
        id: "global.load_marker.toggle",
        label: "Start at the load marker",
        params: &[],
    },
    CommandSpec {
        id: "global.fade_markers.toggle",
        label: "Mix with the fade cues",
        params: &[],
    },
    CommandSpec {
        id: "global.quantize.toggle",
        label: "Quantize",
        params: &[],
    },
    CommandSpec {
        id: "global.sync.mode.set",
        label: "Set sync mode",
        params: &[ParamSpec {
            name: "mode",
            kind: ParamKind::Choice(SYNC_MODES),
        }],
    },
    CommandSpec {
        id: "recorder.toggle",
        label: "Record the mix",
        params: &[],
    },
    CommandSpec {
        id: "recorder.source.set",
        label: "Source of the mix recorder",
        params: &[ParamSpec {
            name: "source",
            kind: ParamKind::Choice(RECORDER_SOURCES),
        }],
    },
    CommandSpec {
        id: "recorder.cut",
        label: "Cut the recording into a new file",
        params: &[],
    },
    CommandSpec {
        id: "loop_recorder.record",
        label: "Record a loop or overdub it",
        params: &[],
    },
    CommandSpec {
        id: "loop_recorder.play",
        label: "Play or pause the loop",
        params: &[],
    },
    CommandSpec {
        id: "loop_recorder.undo",
        label: "Undo the last overdub",
        params: &[],
    },
    CommandSpec {
        id: "loop_recorder.redo",
        label: "Redo the overdub",
        params: &[],
    },
    CommandSpec {
        id: "loop_recorder.delete",
        label: "Delete the loop",
        params: &[],
    },
    CommandSpec {
        id: "loop_recorder.size.set",
        label: "Set the size of the loop",
        params: &[number("bars", 1.0, 8.0)],
    },
    CommandSpec {
        id: "loop_recorder.source.set",
        label: "Set the source of the loop recorder",
        params: &[ParamSpec {
            name: "source",
            kind: ParamKind::Choice(LOOP_SOURCES),
        }],
    },
    CommandSpec {
        id: "loop_recorder.dry_wet.set",
        label: "Set the dry / wet of the loop recorder",
        params: &[number("amount", 0.0, 1.0)],
    },
];

/// An untyped parameter value, as a script, a mapping or a remote caller provides it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
}

/// Untyped parameters by name.
pub type Args = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq)]
pub enum CommandError {
    UnknownCommand(String),
    MissingParam {
        command: &'static str,
        param: &'static str,
    },
    UnknownParam {
        command: &'static str,
        param: String,
    },
    /// The parameter is text where a number is expected, or the other way round.
    WrongType {
        command: &'static str,
        param: &'static str,
    },
    Value(ValueError),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand(id) => write!(f, "unknown command `{id}`"),
            Self::MissingParam { command, param } => {
                write!(f, "`{command}` needs the parameter `{param}`")
            }
            Self::UnknownParam { command, param } => {
                write!(f, "`{command}` has no parameter `{param}`")
            }
            Self::WrongType { command, param } => {
                write!(
                    f,
                    "the parameter `{param}` of `{command}` has the wrong type"
                )
            }
            Self::Value(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<ValueError> for CommandError {
    fn from(error: ValueError) -> Self {
        Self::Value(error)
    }
}

/// Reads the parameters of one command, by the names its specification declares.
struct Reader<'a> {
    spec: &'static CommandSpec,
    args: &'a Args,
}

impl Reader<'_> {
    fn param(&self, param: &'static str) -> Result<&Value, CommandError> {
        self.args.get(param).ok_or(CommandError::MissingParam {
            command: self.spec.id,
            param,
        })
    }

    fn number(&self, param: &'static str) -> Result<f64, CommandError> {
        match self.param(param)? {
            Value::Number(number) => Ok(*number),
            Value::Text(_) => Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            }),
        }
    }

    fn text(&self, param: &'static str) -> Result<&str, CommandError> {
        match self.param(param)? {
            Value::Text(text) => Ok(text),
            Value::Number(_) => Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            }),
        }
    }

    fn deck(&self) -> Result<DeckId, CommandError> {
        Ok(DeckId::parse(self.text("deck")?)?)
    }

    fn stem(&self) -> Result<Stem, CommandError> {
        Ok(Stem::parse(self.text("stem")?)?)
    }

    fn loop_source(&self) -> Result<LoopSource, CommandError> {
        match self.text("source")? {
            "master" => Ok(LoopSource::Master),
            "external" => Ok(LoopSource::External),
            deck => DeckId::parse(deck).map(LoopSource::Deck).map_err(|_| {
                CommandError::Value(ValueError::UnknownChoice {
                    name: "source",
                    expected: "master, a, b, c, d, external",
                })
            }),
        }
    }

    fn unit(&self) -> Result<FxUnit, CommandError> {
        Ok(FxUnit::from_number(self.number("unit")?)?)
    }

    /// A parameter numbered from 1 to `count`, as an index from 0.
    fn index(&self, name: &'static str, count: usize) -> Result<usize, CommandError> {
        Ok(index_of(name, self.number(name)?, count)?)
    }

    fn unit_effect(&self) -> Result<Option<EffectKind>, CommandError> {
        match self.text("effect")? {
            "none" => Ok(None),
            name => Ok(Some(EffectKind::parse(name)?)),
        }
    }

    fn mixer_effect(&self) -> Result<EffectKind, CommandError> {
        let name = self.text("effect")?;
        EffectKind::MIXER
            .into_iter()
            .find(|kind| kind.name() == name)
            .ok_or(CommandError::Value(ValueError::UnknownChoice {
                name: "effect",
                expected: "delay, reverb, gater, bitcrusher",
            }))
    }

    fn band(&self) -> Result<Band, CommandError> {
        match self.text("band")? {
            "low" => Ok(Band::Low),
            "mid" => Ok(Band::Mid),
            "high" => Ok(Band::High),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "band",
                expected: "low, mid, high",
            })),
        }
    }

    fn pressed(&self) -> Result<bool, CommandError> {
        match self.text("action")? {
            "press" => Ok(true),
            "release" => Ok(false),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "action",
                expected: "press, release",
            })),
        }
    }

    fn bpm_scale(&self) -> Result<BpmScale, CommandError> {
        match self.text("factor")? {
            "double" => Ok(BpmScale::Double),
            "half" => Ok(BpmScale::Half),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "factor",
                expected: "double, half",
            })),
        }
    }

    fn direction(&self) -> Result<Direction, CommandError> {
        match self.text("direction")? {
            "back" => Ok(Direction::Back),
            "forward" => Ok(Direction::Forward),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "direction",
                expected: "back, forward",
            })),
        }
    }

    fn crossfader_direction(&self) -> Result<CrossfaderDirection, CommandError> {
        match self.text("direction")? {
            "left" => Ok(CrossfaderDirection::Left),
            "right" => Ok(CrossfaderDirection::Right),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "direction",
                expected: "left, right",
            })),
        }
    }

    /// Whether `type` names the live type.
    fn deck_type(&self) -> Result<bool, CommandError> {
        match self.text("type")? {
            "track" => Ok(false),
            "live" => Ok(true),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "type",
                expected: "track, live",
            })),
        }
    }

    fn input_target(&self) -> Result<InputTarget, CommandError> {
        InputTarget::parse(self.text("target")?).ok_or(CommandError::Value(
            ValueError::UnknownChoice {
                name: "target",
                expected: "deck_a, deck_b, deck_c, deck_d, aux",
            },
        ))
    }

    fn input_source(&self) -> Result<Option<InputSource>, CommandError> {
        match self.text("input")? {
            "none" => Ok(None),
            name => InputSource::parse(name)
                .map(Some)
                .ok_or(CommandError::Value(ValueError::UnknownChoice {
                    name: "input",
                    expected: "none, 1 to 8, 1-2, 3-4, 5-6, 7-8",
                })),
        }
    }

    fn mixer_mode(&self) -> Result<MixerMode, CommandError> {
        let text = self.text("mode")?;
        MixerMode::ALL
            .into_iter()
            .find(|mode| mode.name() == text)
            .ok_or(CommandError::Value(ValueError::UnknownChoice {
                name: "mode",
                expected: "internal, external",
            }))
    }

    fn size_step(&self) -> Result<SizeStep, CommandError> {
        match self.text("direction")? {
            "down" => Ok(SizeStep::Down),
            "up" => Ok(SizeStep::Up),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "direction",
                expected: "down, up",
            })),
        }
    }

    fn move_mode(&self) -> Result<MoveMode, CommandError> {
        let name = self.text("mode")?;
        MoveMode::ALL
            .into_iter()
            .find(|mode| mode.name() == name)
            .ok_or(CommandError::Value(ValueError::UnknownChoice {
                name: "mode",
                expected: "beatjump, loop, loop_in, loop_out",
            }))
    }

    fn slot(&self) -> Result<HotcueSlot, CommandError> {
        Ok(HotcueSlot::from_number(self.number("slot")?)?)
    }

    fn cue_step(&self) -> Result<CueStep, CommandError> {
        match self.text("direction")? {
            "previous" => Ok(CueStep::Previous),
            "next" => Ok(CueStep::Next),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "direction",
                expected: "previous, next",
            })),
        }
    }

    fn cue_kind(&self) -> Result<CueKind, CommandError> {
        let name = self.text("type")?;
        CueKind::ALL
            .into_iter()
            .find(|kind| kind.name() == name)
            .ok_or(CommandError::Value(ValueError::UnknownChoice {
                name: "type",
                expected: "cue, fade_in, fade_out, load, loop",
            }))
    }

    fn output(&self) -> Result<Output, CommandError> {
        match self.text("output")? {
            "master" => Ok(Output::Master),
            "headphones" => Ok(Output::Headphones),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "output",
                expected: "master, headphones",
            })),
        }
    }

    fn clock_mode(&self) -> Result<ClockMode, CommandError> {
        match self.text("mode")? {
            "auto" => Ok(ClockMode::Auto),
            "master" => Ok(ClockMode::Master),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "mode",
                expected: "auto, master",
            })),
        }
    }

    fn sync_mode(&self) -> Result<SyncMode, CommandError> {
        match self.text("mode")? {
            "tempo" => Ok(SyncMode::Tempo),
            "beat" => Ok(SyncMode::Beat),
            _ => Err(CommandError::Value(ValueError::UnknownChoice {
                name: "mode",
                expected: "tempo, beat",
            })),
        }
    }
}

impl Command {
    /// The specification of a command identifier.
    pub fn spec_of(id: &str) -> Option<&'static CommandSpec> {
        SPECS.iter().find(|spec| spec.id == id)
    }

    /// Builds a command from its identifier and untyped parameters. Parameters the command
    /// does not declare are an error, so a typing mistake in a script does not pass
    /// unnoticed. Ranges are checked when the command is dispatched.
    pub fn from_args(id: &str, args: &Args) -> Result<Self, CommandError> {
        let spec = Self::spec_of(id).ok_or_else(|| CommandError::UnknownCommand(id.to_owned()))?;
        if let Some(extra) = args
            .keys()
            .find(|name| spec.params.iter().all(|param| param.name != name.as_str()))
        {
            return Err(CommandError::UnknownParam {
                command: spec.id,
                param: extra.clone(),
            });
        }
        let read = Reader { spec, args };
        Ok(match spec.id {
            "preview.load" => Self::PreviewLoad {
                path: PathBuf::from(read.text("path")?),
            },
            "preview.play_pause" => Self::PreviewPlayPause,
            "preview.seek" => Self::PreviewSeek {
                position: read.number("position")?,
            },
            "deck.load" => Self::DeckLoad {
                deck: read.deck()?,
                path: PathBuf::from(read.text("path")?),
            },
            "deck.load_track" => Self::DeckLoadTrack {
                deck: read.deck()?,
                path: PathBuf::from(read.text("path")?),
            },
            "deck.load_stems" => Self::DeckLoadStems {
                deck: read.deck()?,
                path: PathBuf::from(read.text("path")?),
                stems: Box::new([
                    PathBuf::from(read.text("drums")?),
                    PathBuf::from(read.text("bass")?),
                    PathBuf::from(read.text("other")?),
                    PathBuf::from(read.text("vocals")?),
                ]),
            },
            "deck.stem.volume.set" => Self::DeckStemVolumeSet {
                deck: read.deck()?,
                stem: read.stem()?,
                volume: read.number("volume")?,
            },
            "deck.stem.mute.toggle" => Self::DeckStemMuteToggle {
                deck: read.deck()?,
                stem: read.stem()?,
            },
            "deck.stem.filter.set" => Self::DeckStemFilterSet {
                deck: read.deck()?,
                stem: read.stem()?,
                position: read.number("position")?,
            },
            "deck.stem.filter.toggle" => Self::DeckStemFilterToggle {
                deck: read.deck()?,
                stem: read.stem()?,
            },
            "deck.stem.send.set" => Self::DeckStemSendSet {
                deck: read.deck()?,
                stem: read.stem()?,
                amount: read.number("amount")?,
            },
            "deck.stem.send.toggle" => Self::DeckStemSendToggle {
                deck: read.deck()?,
                stem: read.stem()?,
            },
            "deck.eject" => Self::DeckEject { deck: read.deck()? },
            "deck.play" => Self::DeckPlay { deck: read.deck()? },
            "deck.pause" => Self::DeckPause { deck: read.deck()? },
            "deck.play_pause" => Self::DeckPlayPause { deck: read.deck()? },
            stored
                if stored.starts_with("deck.hotcue.")
                    || matches!(
                        stored,
                        "deck.cue.store"
                            | "deck.cue.select"
                            | "deck.cue.rename"
                            | "deck.cue.type.set"
                            | "deck.cue.delete"
                    ) =>
            {
                return Self::stored_cue_from_args(stored, &read);
            }
            "deck.cue.set" => Self::DeckCueSet { deck: read.deck()? },
            "deck.cue.goto" => Self::DeckCueGoto { deck: read.deck()? },
            "deck.cue" => Self::DeckCue {
                deck: read.deck()?,
                pressed: read.pressed()?,
            },
            "deck.cup" => Self::DeckCup {
                deck: read.deck()?,
                pressed: read.pressed()?,
            },
            "deck.seek" => Self::DeckSeek {
                deck: read.deck()?,
                seconds: read.number("seconds")?,
            },
            "deck.tempo.set" => Self::DeckTempoSet {
                deck: read.deck()?,
                ratio: read.number("ratio")?,
            },
            "deck.nudge" => Self::DeckNudge {
                deck: read.deck()?,
                amount: read.number("amount")?,
            },
            "deck.tempo.range.cycle" => Self::DeckTempoRangeCycle { deck: read.deck()? },
            "deck.tempo.range.set" => Self::DeckTempoRangeSet {
                deck: read.deck()?,
                percent: read.number("percent")?.round() as u32,
            },
            grid if grid.starts_with("deck.grid.") => return Self::grid_from_args(grid, &read),
            "deck.loop.toggle" => Self::DeckLoopToggle { deck: read.deck()? },
            "deck.loop.in" => Self::DeckLoopIn { deck: read.deck()? },
            "deck.loop.out" => Self::DeckLoopOut { deck: read.deck()? },
            "deck.loop.size.step" => Self::DeckLoopSizeStep {
                deck: read.deck()?,
                direction: read.size_step()?,
            },
            "deck.beatjump" => Self::DeckBeatjump {
                deck: read.deck()?,
                direction: read.direction()?,
            },
            "deck.move" => Self::DeckMove {
                deck: read.deck()?,
                direction: read.direction()?,
            },
            "deck.move.mode.set" => Self::DeckMoveModeSet {
                deck: read.deck()?,
                mode: read.move_mode()?,
            },
            "deck.move.size.step" => Self::DeckMoveSizeStep {
                deck: read.deck()?,
                direction: read.size_step()?,
            },
            "deck.move.fine.toggle" => Self::DeckMoveFineToggle { deck: read.deck()? },
            "deck.keylock.toggle" => Self::DeckKeyLockToggle { deck: read.deck()? },
            "deck.tick.toggle" => Self::DeckBeatTickToggle { deck: read.deck()? },
            "deck.key.step" => Self::DeckKeyStep {
                deck: read.deck()?,
                direction: read.size_step()?,
            },
            "deck.key.set" => Self::DeckKeySet {
                deck: read.deck()?,
                semitones: read.number("semitones")?,
            },
            "deck.key.persist" => Self::DeckKeyPersist { deck: read.deck()? },
            "deck.flux.toggle" => Self::DeckFluxToggle { deck: read.deck()? },
            "deck.reverse" => Self::DeckReverse {
                deck: read.deck()?,
                pressed: read.pressed()?,
            },
            sync if sync.starts_with("clock.")
                || matches!(
                    sync,
                    "deck.sync.toggle"
                        | "deck.master.set"
                        | "global.sync.mode.set"
                        | "global.snap.toggle"
                        | "global.load_marker.toggle"
                        | "global.quantize.toggle"
                ) =>
            {
                return Self::sync_from_args(sync, &read);
            }
            "global.fade_markers.toggle" => Self::GlobalFadeMarkersToggle,
            "recorder.toggle" => Self::RecorderToggle,
            "recorder.cut" => Self::RecorderCut,
            "recorder.source.set" => Self::RecorderSourceSet {
                external: match read.text("source")? {
                    "internal" => false,
                    "external" => true,
                    _ => {
                        return Err(CommandError::Value(ValueError::UnknownChoice {
                            name: "source",
                            expected: "internal, external",
                        }));
                    }
                },
            },
            "loop_recorder.record" => Self::LoopRecorderRecord,
            "loop_recorder.play" => Self::LoopRecorderPlay,
            "loop_recorder.undo" => Self::LoopRecorderUndo,
            "loop_recorder.redo" => Self::LoopRecorderRedo,
            "loop_recorder.delete" => Self::LoopRecorderDelete,
            "loop_recorder.size.set" => Self::LoopRecorderSizeSet {
                bars: read.number("bars")?,
            },
            "loop_recorder.source.set" => Self::LoopRecorderSourceSet {
                source: read.loop_source()?,
            },
            "loop_recorder.dry_wet.set" => Self::LoopRecorderDryWetSet {
                amount: read.number("amount")?,
            },
            mixer => return Self::mixer_from_args(mixer, &read),
        })
    }

    fn sync_from_args(id: &str, read: &Reader<'_>) -> Result<Self, CommandError> {
        Ok(match id {
            "deck.sync.toggle" => Self::DeckSyncToggle { deck: read.deck()? },
            "deck.master.set" => Self::DeckMasterSet { deck: read.deck()? },
            "clock.mode.set" => Self::ClockModeSet {
                mode: read.clock_mode()?,
            },
            "clock.tempo.set" => Self::ClockTempoSet {
                bpm: read.number("bpm")?,
            },
            "clock.tempo.step" => Self::ClockTempoStep {
                amount: read.number("amount")?,
            },
            "clock.tap" => Self::ClockTap,
            "clock.bend" => Self::ClockBend {
                amount: read.number("amount")?,
            },
            "clock.metronome.toggle" => Self::ClockMetronomeToggle,
            "clock.midi.send.toggle" => Self::ClockMidiSendToggle,
            "clock.midi.restart" => Self::ClockMidiRestart,
            "clock.source.set" => Self::ClockSourceSet {
                external: match read.text("source")? {
                    "internal" => false,
                    "external" => true,
                    _ => {
                        return Err(CommandError::Value(ValueError::UnknownChoice {
                            name: "source",
                            expected: "internal, external",
                        }));
                    }
                },
            },
            "global.sync.mode.set" => Self::GlobalSyncModeSet {
                mode: read.sync_mode()?,
            },
            "global.snap.toggle" => Self::GlobalSnapToggle,
            "global.load_marker.toggle" => Self::GlobalLoadMarkerToggle,
            "global.quantize.toggle" => Self::GlobalQuantizeToggle,
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    fn stored_cue_from_args(id: &str, read: &Reader<'_>) -> Result<Self, CommandError> {
        let deck = read.deck()?;
        Ok(match id {
            "deck.hotcue.trigger" => Self::DeckHotcueTrigger {
                deck,
                slot: read.slot()?,
                pressed: read.pressed()?,
            },
            "deck.hotcue.delete" => Self::DeckHotcueDelete {
                deck,
                slot: read.slot()?,
            },
            "deck.hotcue.map" => Self::DeckHotcueMap {
                deck,
                slot: read.slot()?,
            },
            "deck.cue.store" => Self::DeckCueStore { deck },
            "deck.cue.select" => Self::DeckCueSelect {
                deck,
                direction: read.cue_step()?,
            },
            "deck.cue.rename" => Self::DeckCueRename {
                deck,
                name: read.text("name")?.to_owned(),
            },
            "deck.cue.type.set" => Self::DeckCueTypeSet {
                deck,
                kind: read.cue_kind()?,
            },
            "deck.cue.delete" => Self::DeckCueDelete { deck },
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    fn grid_from_args(id: &str, read: &Reader<'_>) -> Result<Self, CommandError> {
        let deck = read.deck()?;
        Ok(match id {
            "deck.grid.bpm.set" => Self::DeckGridBpmSet {
                deck,
                bpm: read.number("bpm")?,
            },
            "deck.grid.bpm.scale" => Self::DeckGridBpmScale {
                deck,
                factor: read.bpm_scale()?,
            },
            "deck.grid.bpm.step" => Self::DeckGridBpmStep {
                deck,
                amount: read.number("amount")?,
            },
            "deck.grid.tap" => Self::DeckGridTap { deck },
            "deck.grid.marker.set" => Self::DeckGridMarkerSet { deck },
            "deck.grid.marker.move" => Self::DeckGridMarkerMove {
                deck,
                seconds: read.number("seconds")?,
            },
            "deck.grid.marker.bpm.set" => Self::DeckGridMarkerBpmSet {
                deck,
                bpm: read.number("bpm")?,
            },
            "deck.grid.marker.bpm.clear" => Self::DeckGridMarkerBpmClear { deck },
            "deck.grid.marker.delete" => Self::DeckGridMarkerDelete { deck },
            "deck.grid.marker.previous" => Self::DeckGridMarkerPrevious { deck },
            "deck.grid.marker.next" => Self::DeckGridMarkerNext { deck },
            "deck.grid.lock.toggle" => Self::DeckGridLockToggle { deck },
            "deck.grid.reset" => Self::DeckGridReset { deck },
            "deck.grid.bpm.reset" => Self::DeckGridBpmReset { deck },
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    fn mixer_from_args(id: &str, read: &Reader<'_>) -> Result<Self, CommandError> {
        Ok(match id {
            "mixer.gain.set" => Self::MixerGainSet {
                deck: read.deck()?,
                db: read.number("db")?,
            },
            "mixer.eq.kill.toggle" => Self::MixerEqKillToggle {
                deck: read.deck()?,
                band: read.band()?,
            },
            "mixer.pan.set" => Self::MixerPanSet {
                deck: read.deck()?,
                position: read.number("position")?,
            },
            "mixer.cue.toggle" => Self::MixerCueToggle { deck: read.deck()? },
            "mixer.gain.restore" => Self::MixerGainRestore { deck: read.deck()? },
            "mixer.autogain.toggle" => Self::MixerAutogainToggle,
            "mixer.cue.mix.set" => Self::MixerCueMixSet {
                position: read.number("position")?,
            },
            "mixer.cue.volume.set" => Self::MixerCueVolumeSet {
                db: read.number("db")?,
            },
            "master.limiter.toggle" => Self::MasterLimiterToggle,
            "master.maximiser.drive.set" => Self::MasterMaximiserDriveSet {
                db: read.number("db")?,
            },
            "master.maximiser.ceiling.set" => Self::MasterMaximiserCeilingSet {
                db: read.number("db")?,
            },
            "master.maximiser.release.set" => Self::MasterMaximiserReleaseSet {
                ms: read.number("ms")?,
            },
            "master.test_tone.play" => Self::MasterTestTone {
                output: read.output()?,
            },
            "mixer.eq.set" => Self::MixerEqSet {
                deck: read.deck()?,
                band: read.band()?,
                position: read.number("position")?,
            },
            "mixer.filter.set" => Self::MixerFilterSet {
                deck: read.deck()?,
                position: read.number("position")?,
            },
            "mixer.fader.set" => Self::MixerFaderSet {
                deck: read.deck()?,
                position: read.number("position")?,
            },
            "mixer.crossfader.set" => Self::MixerCrossfaderSet {
                position: read.number("position")?,
            },
            "mixer.crossfader.curve.set" => Self::MixerCrossfaderCurveSet {
                curve: CrossfaderCurve::parse(read.text("curve")?)?,
            },
            "mixer.eq.model.set" => Self::MixerEqModelSet {
                model: EqModel::parse(read.text("model")?)?,
            },
            "mixer.filter.model.set" => Self::MixerFilterModelSet {
                model: FilterModel::parse(read.text("model")?)?,
            },
            "mixer.crossfader.step" => Self::MixerCrossfaderStep {
                direction: read.crossfader_direction()?,
            },
            "mixer.crossfader.auto_fade" => Self::MixerCrossfaderAutoFade {
                direction: read.crossfader_direction()?,
            },
            "mixer.crossfader.fade_beats.set" => Self::MixerCrossfaderFadeBeatsSet {
                beats: read.number("beats")?,
            },
            "mixer.mode.set" => Self::MixerModeSet {
                mode: read.mixer_mode()?,
            },
            "mixer.external.eq_gain.toggle" => Self::MixerExternalEqGainToggle,
            "mixer.external.master_volume.toggle" => Self::MixerExternalMasterVolumeToggle,
            "mixer.aux.set" => Self::MixerAuxSet {
                level: read.number("level")?,
            },
            "deck.type.set" => Self::DeckTypeSet {
                deck: read.deck()?,
                live: read.deck_type()?,
            },
            "input.routing.set" => Self::InputRoutingSet {
                target: read.input_target()?,
                source: read.input_source()?,
            },
            "mixer.xfader_assign.set" => Self::MixerCrossfaderAssign {
                deck: read.deck()?,
                side: CrossfaderSide::parse(read.text("side")?)?,
            },
            "master.gain.set" => Self::MasterGainSet {
                db: read.number("db")?,
            },
            "fx.unit.mode.set" => Self::FxUnitModeSet {
                unit: read.unit()?,
                mode: UnitMode::parse(read.text("mode")?)?,
            },
            "fx.unit.dry_wet.set" => Self::FxUnitDryWetSet {
                unit: read.unit()?,
                position: read.number("position")?,
            },
            "fx.unit.routing.set" => Self::FxUnitRoutingSet {
                unit: read.unit()?,
                routing: FxRouting::parse(read.text("routing")?)?,
            },
            "fx.unit.reset" => Self::FxUnitReset { unit: read.unit()? },
            "fx.unit.snapshot.save" => Self::FxUnitSnapshotSave { unit: read.unit()? },
            "fx.keep_values.toggle" => Self::FxKeepValuesToggle,
            "fx.slot.effect.set" => Self::FxSlotEffectSet {
                unit: read.unit()?,
                slot: read.index("slot", SLOTS)?,
                effect: read.unit_effect()?,
            },
            "fx.slot.amount.set" => Self::FxSlotAmountSet {
                unit: read.unit()?,
                slot: read.index("slot", SLOTS)?,
                position: read.number("position")?,
            },
            "fx.slot.toggle" => Self::FxSlotToggle {
                unit: read.unit()?,
                slot: read.index("slot", SLOTS)?,
            },
            "fx.single.effect.set" => Self::FxSingleEffectSet {
                unit: read.unit()?,
                effect: read.unit_effect()?,
            },
            "fx.single.knob.set" => Self::FxSingleKnobSet {
                unit: read.unit()?,
                knob: read.index("knob", KNOBS)?,
                position: read.number("position")?,
            },
            "fx.single.key.toggle" => Self::FxSingleKeyToggle {
                unit: read.unit()?,
                key: read.index("key", KEYS)?,
            },
            "fx.single.toggle" => Self::FxSingleToggle { unit: read.unit()? },
            "mixer.fx_assign.toggle" => Self::MixerFxAssignToggle {
                deck: read.deck()?,
                unit: read.unit()?,
            },
            "mixer.effect.select" => Self::MixerEffectSelect {
                deck: read.deck()?,
                effect: read.mixer_effect()?,
            },
            "mixer.effect.set" => Self::MixerEffectSet {
                deck: read.deck()?,
                position: read.number("position")?,
            },
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    /// The stable identifier of the command.
    pub fn id(&self) -> &'static str {
        match self {
            Self::PreviewLoad { .. } => "preview.load",
            Self::PreviewPlayPause => "preview.play_pause",
            Self::PreviewSeek { .. } => "preview.seek",
            Self::DeckLoad { .. } => "deck.load",
            Self::DeckLoadTrack { .. } => "deck.load_track",
            Self::DeckLoadStems { .. } => "deck.load_stems",
            Self::DeckStemVolumeSet { .. } => "deck.stem.volume.set",
            Self::DeckStemMuteToggle { .. } => "deck.stem.mute.toggle",
            Self::DeckStemFilterSet { .. } => "deck.stem.filter.set",
            Self::DeckStemFilterToggle { .. } => "deck.stem.filter.toggle",
            Self::DeckStemSendSet { .. } => "deck.stem.send.set",
            Self::DeckStemSendToggle { .. } => "deck.stem.send.toggle",
            Self::DeckEject { .. } => "deck.eject",
            Self::DeckPlay { .. } => "deck.play",
            Self::DeckPause { .. } => "deck.pause",
            Self::DeckPlayPause { .. } => "deck.play_pause",
            Self::DeckCueSet { .. } => "deck.cue.set",
            Self::DeckCueGoto { .. } => "deck.cue.goto",
            Self::DeckCue { .. } => "deck.cue",
            Self::DeckCup { .. } => "deck.cup",
            Self::DeckSeek { .. } => "deck.seek",
            Self::DeckTempoSet { .. } => "deck.tempo.set",
            Self::DeckNudge { .. } => "deck.nudge",
            Self::DeckTempoRangeCycle { .. } => "deck.tempo.range.cycle",
            Self::DeckTempoRangeSet { .. } => "deck.tempo.range.set",
            Self::DeckGridBpmSet { .. } => "deck.grid.bpm.set",
            Self::DeckGridBpmScale { .. } => "deck.grid.bpm.scale",
            Self::DeckGridBpmStep { .. } => "deck.grid.bpm.step",
            Self::DeckGridTap { .. } => "deck.grid.tap",
            Self::DeckGridMarkerSet { .. } => "deck.grid.marker.set",
            Self::DeckGridMarkerMove { .. } => "deck.grid.marker.move",
            Self::DeckGridMarkerBpmSet { .. } => "deck.grid.marker.bpm.set",
            Self::DeckGridMarkerBpmClear { .. } => "deck.grid.marker.bpm.clear",
            Self::DeckGridMarkerDelete { .. } => "deck.grid.marker.delete",
            Self::DeckGridMarkerPrevious { .. } => "deck.grid.marker.previous",
            Self::DeckGridMarkerNext { .. } => "deck.grid.marker.next",
            Self::DeckGridLockToggle { .. } => "deck.grid.lock.toggle",
            Self::DeckGridReset { .. } => "deck.grid.reset",
            Self::DeckGridBpmReset { .. } => "deck.grid.bpm.reset",
            Self::DeckLoopToggle { .. } => "deck.loop.toggle",
            Self::DeckLoopIn { .. } => "deck.loop.in",
            Self::DeckLoopOut { .. } => "deck.loop.out",
            Self::DeckLoopSizeStep { .. } => "deck.loop.size.step",
            Self::DeckBeatjump { .. } => "deck.beatjump",
            Self::DeckMove { .. } => "deck.move",
            Self::DeckMoveModeSet { .. } => "deck.move.mode.set",
            Self::DeckMoveSizeStep { .. } => "deck.move.size.step",
            Self::DeckMoveFineToggle { .. } => "deck.move.fine.toggle",
            Self::DeckHotcueTrigger { .. } => "deck.hotcue.trigger",
            Self::DeckHotcueDelete { .. } => "deck.hotcue.delete",
            Self::DeckHotcueMap { .. } => "deck.hotcue.map",
            Self::DeckCueStore { .. } => "deck.cue.store",
            Self::DeckCueSelect { .. } => "deck.cue.select",
            Self::DeckCueRename { .. } => "deck.cue.rename",
            Self::DeckCueTypeSet { .. } => "deck.cue.type.set",
            Self::DeckCueDelete { .. } => "deck.cue.delete",
            Self::MixerGainSet { .. } => "mixer.gain.set",
            Self::MixerEqSet { .. } => "mixer.eq.set",
            Self::MixerEqKillToggle { .. } => "mixer.eq.kill.toggle",
            Self::MixerFilterSet { .. } => "mixer.filter.set",
            Self::MixerPanSet { .. } => "mixer.pan.set",
            Self::MixerCueToggle { .. } => "mixer.cue.toggle",
            Self::MixerGainRestore { .. } => "mixer.gain.restore",
            Self::MixerAutogainToggle => "mixer.autogain.toggle",
            Self::MixerCueMixSet { .. } => "mixer.cue.mix.set",
            Self::MixerCueVolumeSet { .. } => "mixer.cue.volume.set",
            Self::MixerFaderSet { .. } => "mixer.fader.set",
            Self::MixerCrossfaderSet { .. } => "mixer.crossfader.set",
            Self::MixerCrossfaderCurveSet { .. } => "mixer.crossfader.curve.set",
            Self::MixerEqModelSet { .. } => "mixer.eq.model.set",
            Self::MixerFilterModelSet { .. } => "mixer.filter.model.set",
            Self::MixerCrossfaderAssign { .. } => "mixer.xfader_assign.set",
            Self::MixerCrossfaderStep { .. } => "mixer.crossfader.step",
            Self::MixerCrossfaderAutoFade { .. } => "mixer.crossfader.auto_fade",
            Self::MixerCrossfaderFadeBeatsSet { .. } => "mixer.crossfader.fade_beats.set",
            Self::MixerModeSet { .. } => "mixer.mode.set",
            Self::MixerExternalEqGainToggle => "mixer.external.eq_gain.toggle",
            Self::MixerExternalMasterVolumeToggle => "mixer.external.master_volume.toggle",
            Self::MixerAuxSet { .. } => "mixer.aux.set",
            Self::DeckTypeSet { .. } => "deck.type.set",
            Self::InputRoutingSet { .. } => "input.routing.set",
            Self::FxUnitModeSet { .. } => "fx.unit.mode.set",
            Self::FxUnitDryWetSet { .. } => "fx.unit.dry_wet.set",
            Self::FxUnitRoutingSet { .. } => "fx.unit.routing.set",
            Self::FxUnitReset { .. } => "fx.unit.reset",
            Self::FxUnitSnapshotSave { .. } => "fx.unit.snapshot.save",
            Self::FxKeepValuesToggle => "fx.keep_values.toggle",
            Self::FxSlotEffectSet { .. } => "fx.slot.effect.set",
            Self::FxSlotAmountSet { .. } => "fx.slot.amount.set",
            Self::FxSlotToggle { .. } => "fx.slot.toggle",
            Self::FxSingleEffectSet { .. } => "fx.single.effect.set",
            Self::FxSingleKnobSet { .. } => "fx.single.knob.set",
            Self::FxSingleKeyToggle { .. } => "fx.single.key.toggle",
            Self::FxSingleToggle { .. } => "fx.single.toggle",
            Self::MixerFxAssignToggle { .. } => "mixer.fx_assign.toggle",
            Self::MixerEffectSelect { .. } => "mixer.effect.select",
            Self::MixerEffectSet { .. } => "mixer.effect.set",
            Self::MasterGainSet { .. } => "master.gain.set",
            Self::MasterTestTone { .. } => "master.test_tone.play",
            Self::MasterLimiterToggle => "master.limiter.toggle",
            Self::MasterMaximiserDriveSet { .. } => "master.maximiser.drive.set",
            Self::MasterMaximiserCeilingSet { .. } => "master.maximiser.ceiling.set",
            Self::MasterMaximiserReleaseSet { .. } => "master.maximiser.release.set",
            Self::DeckSyncToggle { .. } => "deck.sync.toggle",
            Self::DeckKeyLockToggle { .. } => "deck.keylock.toggle",
            Self::DeckBeatTickToggle { .. } => "deck.tick.toggle",
            Self::DeckKeyStep { .. } => "deck.key.step",
            Self::DeckKeySet { .. } => "deck.key.set",
            Self::DeckKeyPersist { .. } => "deck.key.persist",
            Self::DeckFluxToggle { .. } => "deck.flux.toggle",
            Self::DeckReverse { .. } => "deck.reverse",
            Self::DeckMasterSet { .. } => "deck.master.set",
            Self::ClockModeSet { .. } => "clock.mode.set",
            Self::ClockTempoSet { .. } => "clock.tempo.set",
            Self::ClockTempoStep { .. } => "clock.tempo.step",
            Self::ClockTap => "clock.tap",
            Self::ClockBend { .. } => "clock.bend",
            Self::ClockMetronomeToggle => "clock.metronome.toggle",
            Self::ClockMidiSendToggle => "clock.midi.send.toggle",
            Self::ClockMidiRestart => "clock.midi.restart",
            Self::ClockSourceSet { .. } => "clock.source.set",
            Self::GlobalSyncModeSet { .. } => "global.sync.mode.set",
            Self::GlobalSnapToggle => "global.snap.toggle",
            Self::GlobalLoadMarkerToggle => "global.load_marker.toggle",
            Self::GlobalFadeMarkersToggle => "global.fade_markers.toggle",
            Self::GlobalQuantizeToggle => "global.quantize.toggle",
            Self::RecorderToggle => "recorder.toggle",
            Self::RecorderCut => "recorder.cut",
            Self::RecorderSourceSet { .. } => "recorder.source.set",
            Self::LoopRecorderRecord => "loop_recorder.record",
            Self::LoopRecorderPlay => "loop_recorder.play",
            Self::LoopRecorderUndo => "loop_recorder.undo",
            Self::LoopRecorderRedo => "loop_recorder.redo",
            Self::LoopRecorderDelete => "loop_recorder.delete",
            Self::LoopRecorderSizeSet { .. } => "loop_recorder.size.set",
            Self::LoopRecorderSourceSet { .. } => "loop_recorder.source.set",
            Self::LoopRecorderDryWetSet { .. } => "loop_recorder.dry_wet.set",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use platter_core::DeckId;

    use super::{Args, Command, CommandError, ParamKind, SPECS, Value};

    /// Valid arguments for a specification: the first choice, the lower bound, deck A.
    fn sample_args(spec: &super::CommandSpec) -> Args {
        spec.params
            .iter()
            .map(|param| {
                let value = match param.kind {
                    ParamKind::Deck => Value::Text("a".to_owned()),
                    ParamKind::Number { min, .. } => Value::Number(min),
                    ParamKind::Choice(choices) => Value::Text(choices[0].to_owned()),
                    ParamKind::Path => Value::Text("track.wav".to_owned()),
                    ParamKind::Text => Value::Text("text".to_owned()),
                };
                (param.name.to_owned(), value)
            })
            .collect()
    }

    #[test]
    fn identifiers_are_unique_and_well_formed() {
        let ids: BTreeSet<&str> = SPECS.iter().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), SPECS.len());
        for spec in SPECS {
            assert!(
                spec.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{}",
                spec.id
            );
            assert!(!spec.label.is_empty());
        }
    }

    #[test]
    fn every_specification_builds_its_command() {
        for spec in SPECS {
            let command = Command::from_args(spec.id, &sample_args(spec)).unwrap();
            assert_eq!(command.id(), spec.id);
        }
    }

    #[test]
    fn every_declared_parameter_is_required() {
        for spec in SPECS {
            for param in spec.params {
                let mut args = sample_args(spec);
                args.remove(param.name);
                assert_eq!(
                    Command::from_args(spec.id, &args),
                    Err(CommandError::MissingParam {
                        command: spec.id,
                        param: param.name
                    })
                );
            }
        }
    }

    #[test]
    fn rejects_what_a_specification_does_not_declare() {
        let spec = Command::spec_of("deck.play").unwrap();
        let mut args = sample_args(spec);
        args.insert("speed".to_owned(), Value::Number(1.0));
        assert!(matches!(
            Command::from_args("deck.play", &args),
            Err(CommandError::UnknownParam { .. })
        ));
        assert!(matches!(
            Command::from_args("deck.explode", &Args::new()),
            Err(CommandError::UnknownCommand(_))
        ));
    }

    #[test]
    fn rejects_values_of_the_wrong_type_or_choice() {
        let mut args = Args::new();
        args.insert("deck".to_owned(), Value::Number(1.0));
        assert!(matches!(
            Command::from_args("deck.play", &args),
            Err(CommandError::WrongType { .. })
        ));
        args.insert("deck".to_owned(), Value::Text("z".to_owned()));
        assert!(matches!(
            Command::from_args("deck.play", &args),
            Err(CommandError::Value(_))
        ));
        args.insert("deck".to_owned(), Value::Text("b".to_owned()));
        assert_eq!(
            Command::from_args("deck.play", &args),
            Ok(Command::DeckPlay { deck: DeckId::B })
        );
    }
}
