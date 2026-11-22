//! The control side of the engine. It validates commands, loads tracks on worker threads,
//! feeds the processor through its queue and publishes the state snapshot.
//!
//! It runs on a normal thread and may allocate, block on a channel and touch the disk; the
//! processor it feeds may not.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use platter_analysis::Transposed;
use platter_core::{
    BeatGrid, Bpm, CueKind, CueName, Cues, DeckId, Gain, LoopSize, SampleRate, Stem, Tempo,
    TrackMemory, ValueError,
};
use platter_decode::{Container, DecodeError, DecodedAudio, Tags, decode_file};
use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
use platter_dsp::effects::unit::Place;
use platter_dsp::effects::{Controls, DEFAULT_AMOUNT, Effect, EffectKind};
use platter_dsp::eq::{Band, EqModel, band_gain};
use platter_dsp::filter::FilterModel;
use platter_dsp::stretch::Stretch;

use crate::analyser::{Analyser, PlatterAnalyser, TrackAnalysis};
use crate::command::{
    CROSSFADER_STEP, ClockMode, Command, CrossfaderDirection, CueStep, Direction, MAX_BPM_STEP,
    MAX_CEILING_DB, MAX_CELL_GAIN_DB, MAX_CHANNEL_GAIN_DB, MAX_DRIVE_DB, MAX_FADE_BEATS,
    MAX_HEADPHONE_GAIN_DB, MAX_KEY_SHIFT, MAX_MARKER_MOVE_SECONDS, MAX_MASTER_GAIN_DB, MAX_NUDGE,
    MAX_RELEASE_MS, MAX_SEEK_SECONDS, MIN_CEILING_DB, MIN_FADE_BEATS, MIN_RELEASE_MS, MixerMode,
    MoveMode, MoveSize, SizeStep, SyncMode,
};
use crate::effects::{EffectControls, FxUnit, UnitControls};
use crate::peaks::WaveformPeaks;
use crate::recorder::{MIN_SPLIT_BYTES, Recorder, RecorderSnapshot, RecordingNames, SPLIT_BYTES};
use crate::remix::{DecodedSample, RemixControl, RemixSnapshot, capture_sample, decode_sample};
use crate::rt::{
    self, COMMAND_CAPACITY, CROSSFADER_SIDES, CellId, CellSettings, ControlEnds, CueEdit, DeckKind,
    DeckStems, DeckTrack, GridEdit, INPUT_CHANNELS, InputFeed, InputSource, InputTarget, LOOP_BARS,
    LoopEdit, LoopPart, LoopPhase, LoopSource, LoopState, Processor, RtCommand, Span, StemSettings,
    Stretcher, TrackAudio,
};
use crate::stretch::TrackStretch;
use crate::stretch_pool::StretchPool;
use crate::tap::TapTempo;

/// Ranges of the tempo fader, as a fraction of the original speed either way.
pub const TEMPO_RANGES: [f64; 3] = [0.08, 0.16, 0.5];

/// Fader position that leaves the level untouched. The travel above it adds gain.
pub const FADER_UNITY: f64 = 0.75;

/// Step of a jump or a move while the fine steps are on, in seconds.
pub const FINE_MOVE_SECONDS: f64 = 0.005;

/// What the interface shows about a loaded track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub path: PathBuf,
    pub container: Container,
    pub sample_rate: SampleRate,
    /// Channels of the file; the engine plays it as stereo.
    pub channels: usize,
    pub seconds: f64,
    pub tags: Tags,
}

/// Something that happened in the background since the last poll.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Loaded {
        deck: DeckId,
        info: TrackInfo,
    },
    LoadFailed {
        deck: DeckId,
        message: String,
    },
    /// The stems of a track could not be played; the track loaded alone.
    StemsFailed {
        deck: DeckId,
        message: String,
    },
    /// The file sent to a cell of a remix deck could not be played there.
    RemixLoadFailed {
        deck: DeckId,
        at: CellId,
        message: String,
    },
    /// The file sent to the preview player could not be read.
    PreviewFailed {
        message: String,
    },
    /// The recorder finished a file: `docs/behavior/recording.md`.
    Recorded {
        path: PathBuf,
        frames: u64,
    },
    /// The recording stopped by itself, or lost frames, for this reason.
    RecordingFailed {
        message: String,
    },
}

#[derive(Debug)]
pub enum EngineError {
    /// A parameter is outside what the command accepts.
    Value(ValueError),
    /// The processor is not taking commands fast enough; the command was not applied.
    Busy,
    /// A worker thread could not be started.
    Worker(std::io::Error),
    /// Nothing says where the recordings go.
    NoRecordingFolder,
    /// The loop recorder does not record in external mixer mode, where the mix it plays into
    /// is not heard.
    ExternalMixer,
    /// The deck to capture from has no track, or neither a loop nor a tempo to size one.
    NothingToCapture,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => error.fmt(f),
            Self::Busy => f.write_str("the engine is busy; the command was not applied"),
            Self::Worker(error) => write!(f, "cannot start a worker thread: {error}"),
            Self::NoRecordingFolder => f.write_str("there is no folder for the recordings"),
            Self::ExternalMixer => {
                f.write_str("the loop recorder does not record in external mixer mode")
            }
            Self::NothingToCapture => {
                f.write_str("the deck has no track, or neither a loop nor a tempo to capture")
            }
        }
    }
}

impl std::error::Error for EngineError {}

impl From<ValueError> for EngineError {
    fn from(error: ValueError) -> Self {
        Self::Value(error)
    }
}

/// Where the controls of a deck and of its mixer channel stand. The values are the ones the
/// commands carry, so a control drawn from them sits where the last command put it.
#[derive(Clone, Debug, PartialEq)]
pub struct DeckControls {
    /// Playback speed as a ratio of the original.
    pub tempo_ratio: f64,
    /// Range of the tempo fader, one of [`TEMPO_RANGES`].
    pub tempo_range: f64,
    pub gain_db: f64,
    /// Positions of the low, mid and high knobs, `0.0..=1.0`.
    pub eq: [f64; 3],
    pub eq_kill: [bool; 3],
    pub filter: f64,
    pub pan: f64,
    pub fader: f64,
    /// The deck is sent to the headphones.
    pub pre_listen: bool,
    /// Size a new loop gets.
    pub loop_size: LoopSize,
    pub move_mode: MoveMode,
    pub move_size: MoveSize,
    /// Jumps and moves go by [`FINE_MOVE_SECONDS`] instead of the move size.
    pub move_fine: bool,
    /// The controls of the four stems, in the order of [`Stem::ALL`]. A load puts them back
    /// to their defaults.
    pub stems: [StemSettings; Stem::COUNT],
    /// What the deck plays: its track, the input routed to it (`docs/behavior/live-input.md`)
    /// or its grid of samples (`docs/behavior/remix-deck.md`).
    pub kind: DeckKind,
}

impl Default for DeckControls {
    fn default() -> Self {
        Self {
            tempo_ratio: 1.0,
            tempo_range: TEMPO_RANGES[0],
            gain_db: 0.0,
            eq: [0.5; 3],
            eq_kill: [false; 3],
            filter: 0.0,
            pan: 0.0,
            fader: FADER_UNITY,
            pre_listen: false,
            loop_size: LoopSize::FOUR_BEATS,
            move_mode: MoveMode::default(),
            move_size: MoveSize::default(),
            move_fine: false,
            stems: [StemSettings::default(); Stem::COUNT],
            kind: DeckKind::Track,
        }
    }
}

/// Where the controls shared by all the decks stand.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerControls {
    pub crossfader: f64,
    pub crossfader_curve: CrossfaderCurve,
    /// The model of the equaliser of every channel.
    pub eq_model: EqModel,
    /// The model of the filter of every channel.
    pub filter_model: FilterModel,
    /// The side of the crossfader each channel is on, deck A first.
    pub crossfader_sides: [CrossfaderSide; DeckId::COUNT],
    /// Beats of the master clock an automatic fade across the whole travel takes.
    pub fade_beats: f64,
    pub master_gain_db: f64,
    /// The maximiser of the master, which the Limiter key switches.
    pub maximiser: MaximiserControls,
    /// Balance in the headphones: the pre-listened decks alone at zero, the master at one.
    pub headphone_mix: f64,
    pub headphone_gain_db: f64,
    /// The gain of a channel is set from the loudness of each track analysed:
    /// `docs/behavior/autogain.md`.
    pub autogain: bool,
    /// Platter mixes, or sends each deck to its own outputs:
    /// `docs/behavior/external-mixer.md`.
    pub mode: MixerMode,
    /// In external mode, the gain and the equaliser act on the decks.
    pub external_eq_gain: bool,
    /// In external mode, the master volume acts on the decks.
    pub external_master_volume: bool,
    /// Volume of the auxiliary input in the master, `0.0..=1.0`.
    pub aux_level: f64,
    /// The input each target of the routing takes, in the order of [`InputTarget::ALL`]:
    /// `docs/behavior/live-input.md`.
    pub input_routes: [Option<InputSource>; InputTarget::ALL.len()],
}

impl Default for MixerControls {
    fn default() -> Self {
        Self {
            crossfader: 0.5,
            crossfader_curve: CrossfaderCurve::default(),
            eq_model: EqModel::default(),
            filter_model: FilterModel::default(),
            crossfader_sides: CROSSFADER_SIDES,
            fade_beats: 8.0,
            master_gain_db: 0.0,
            maximiser: MaximiserControls::default(),
            headphone_mix: 0.0,
            headphone_gain_db: 0.0,
            autogain: true,
            mode: MixerMode::Internal,
            external_eq_gain: false,
            external_master_volume: false,
            aux_level: 0.0,
            input_routes: [None; InputTarget::ALL.len()],
        }
    }
}

/// The maximiser of the master: `docs/behavior/master.md`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaximiserControls {
    /// The Limiter key is lit: the master goes through the maximiser.
    pub on: bool,
    /// Gain before limiting, in decibels.
    pub drive_db: f64,
    /// Highest sample peak of the master, in decibels of full scale.
    pub ceiling_db: f64,
    /// Time to recover 90 % of a gain reduction, in milliseconds.
    pub release_ms: f64,
}

impl Default for MaximiserControls {
    fn default() -> Self {
        Self {
            on: false,
            drive_db: 0.0,
            ceiling_db: -1.0,
            release_ms: 100.0,
        }
    }
}

impl MaximiserControls {
    fn command(&self) -> RtCommand {
        let linear = |db: f64| Gain::from_db(db).map_or(1.0, Gain::linear);
        RtCommand::Maximiser {
            on: self.on,
            drive: linear(self.drive_db),
            ceiling: linear(self.ceiling_db),
            release_seconds: (self.release_ms / 1_000.0) as f32,
        }
    }
}

/// The master clock and the tempo master: `docs/behavior/sync.md`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClockSnapshot {
    /// The tempo master is chosen automatically.
    pub auto: bool,
    pub sync_mode: SyncMode,
    /// The deck that is the tempo master; without one, the clock is.
    pub master: Option<DeckId>,
    /// Tempo the synced decks follow, in beats per minute.
    pub bpm: f64,
    /// New cues and loops land on the nearest beat.
    pub snap: bool,
    /// A playing deck that goes to a stored cue keeps its place in the beat.
    pub quantize: bool,
    /// A click on every beat of the clock, on the headphones.
    pub metronome: bool,
    /// The user asked to send the MIDI clock, and whether it is going out: it starts on the
    /// next beat.
    pub midi_clock: bool,
    pub midi_clock_running: bool,
    /// The master clock follows a received MIDI clock: `docs/behavior/midi-clock.md`.
    pub external: bool,
}

impl Default for ClockSnapshot {
    fn default() -> Self {
        let published = rt::Shared::default();
        Self {
            auto: true,
            sync_mode: SyncMode::default(),
            master: None,
            bpm: published.clock_bpm.get(),
            snap: false,
            quantize: false,
            metronome: false,
            midi_clock: false,
            midi_clock_running: false,
            external: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeckSnapshot {
    pub track: Option<TrackInfo>,
    /// Waveform data of the loaded track.
    pub peaks: Option<Arc<WaveformPeaks>>,
    /// Waveform data of each stem, while the deck plays stems: `docs/behavior/stems.md`.
    pub stem_peaks: Option<Arc<[WaveformPeaks; Stem::COUNT]>>,
    /// A file is being decoded for this deck.
    pub loading: bool,
    /// The loaded track is being analysed.
    pub analysing: bool,
    /// What the analysis of the loaded track found, once it has finished.
    pub analysis: Option<TrackAnalysis>,
    /// The gain autogain gives the loaded track, once the analysis has found its loudness.
    pub autogain_db: Option<f64>,
    pub playing: bool,
    pub position_seconds: f64,
    pub cue_seconds: f64,
    /// Beatgrid of the loaded track, as of the last [`Engine::poll`].
    /// On the heap: the snapshot is copied often, on the stack of the interface too.
    pub grid: Box<BeatGrid>,
    /// Loop of the loaded track, as of the last [`Engine::poll`].
    pub looping: LoopState,
    /// Stored cues of the loaded track, as of the last [`Engine::poll`].
    pub cues: Cues,
    pub controls: DeckControls,
    /// Post-fader peak level of the left and the right side, linear.
    pub levels: [f32; 2],
    pub sync: bool,
    /// The deck keeps the key of its track while its tempo changes:
    /// `docs/behavior/keylock.md`.
    pub key_lock: bool,
    /// The deck ticks on each beat in the headphones: `docs/behavior/beatgrid.md`.
    pub beat_tick: bool,
    /// Shift of the key in semitones, from -12 to +12.
    pub key_shift: f64,
    /// The key the analysis found, moved by the pitch the deck plays: the shift while key
    /// lock acts, the pitch of the speed otherwise.
    pub resulting_key: Option<Transposed>,
    /// Flux is on: `docs/behavior/flux.md`.
    pub flux: bool,
    /// Reverse is held.
    pub reverse: bool,
    /// Where the timeline of flux is, in seconds, while the deck has one.
    pub timeline_seconds: Option<f64>,
    /// Speed the deck is set to, as a ratio of the original. It is `controls.tempo_ratio`
    /// until sync sets it.
    pub speed: f64,
    /// How far the beat of the deck is from that of the clock, in beats: negative behind,
    /// positive ahead. A deck without a tempo in its grid has none.
    pub phase: Option<f32>,
    /// The grid of samples, played while the deck is a remix deck: `docs/behavior/remix-deck.md`.
    pub remix: Box<RemixSnapshot>,
}

impl Default for DeckSnapshot {
    fn default() -> Self {
        Self {
            track: None,
            peaks: None,
            stem_peaks: None,
            loading: false,
            analysing: false,
            analysis: None,
            autogain_db: None,
            playing: false,
            position_seconds: 0.0,
            cue_seconds: 0.0,
            grid: Box::default(),
            looping: LoopState::default(),
            cues: Cues::default(),
            controls: DeckControls::default(),
            levels: [0.0; 2],
            sync: false,
            key_lock: false,
            beat_tick: false,
            key_shift: 0.0,
            resulting_key: None,
            flux: false,
            reverse: false,
            timeline_seconds: None,
            speed: 1.0,
            phase: None,
            remix: Box::default(),
        }
    }
}

/// State of the engine at one moment, for the interface to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub decks: [DeckSnapshot; DeckId::COUNT],
    /// A track with a cue of the type load starts at it: `docs/behavior/cues.md`.
    pub load_marker: bool,
    /// A deck crossing a fade-out cue starts its partner: `docs/behavior/auto-mix.md`.
    pub fade_markers: bool,
    pub preview: PreviewSnapshot,
    pub mixer: MixerControls,
    /// The effect units and the mixer effects: `docs/behavior/effects.md`.
    pub effects: EffectControls,
    pub clock: ClockSnapshot,
    /// Peak level of the left and the right side of the master output, linear.
    pub master_levels: [f32; 2],
    /// Gain the safety limiter is applying; below one while it works.
    pub limiter_gain: f32,
    /// Gain the maximiser is applying; one while it is off or idle.
    pub maximiser_gain: f32,
    pub recorder: RecorderSnapshot,
    pub loop_recorder: LoopRecorderSnapshot,
    pub input: InputSnapshot,
}

/// What the processor reads of the input device: `docs/behavior/live-input.md`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputSnapshot {
    /// Peak level of each input, linear.
    pub levels: [f32; INPUT_CHANNELS],
    /// Times the input ran dry while it was heard.
    pub gaps: u64,
    /// The input is heard; false while it fills, at the start or after a gap.
    pub reading: bool,
    /// The speed the input is read at, against the nominal one.
    pub ratio: f64,
}

impl Default for InputSnapshot {
    fn default() -> Self {
        Self {
            levels: [0.0; INPUT_CHANNELS],
            gaps: 0,
            reading: false,
            ratio: 1.0,
        }
    }
}

/// What the interface shows of the loop recorder: `docs/behavior/recording.md`, *Loop
/// recorder*.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopRecorderSnapshot {
    pub phase: LoopPhase,
    /// The size chosen, in bars.
    pub bars: u32,
    /// The bars a loop recorded now would have, at the tempo of the clock.
    pub fitting_bars: u32,
    pub source: LoopSource,
    pub dry_wet: f64,
    /// The share of the loop taken away on each pass of an overdub, 0 to 1.
    pub decay: f64,
    /// How far into the loop the position is, from zero to one.
    pub progress: f32,
    /// The length of the loop; zero while nothing is recorded.
    pub seconds: f64,
    pub can_undo: bool,
    pub can_redo: bool,
}

impl Default for LoopRecorderSnapshot {
    fn default() -> Self {
        let controls = LoopControls::default();
        Self {
            phase: LoopPhase::Empty,
            bars: controls.bars,
            fitting_bars: controls.bars,
            source: controls.source,
            dry_wet: controls.dry_wet,
            decay: controls.decay,
            progress: 0.0,
            seconds: 0.0,
            can_undo: false,
            can_redo: false,
        }
    }
}

/// The settings of the loop recorder the control side keeps, to give a new engine.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LoopControls {
    bars: u32,
    source: LoopSource,
    dry_wet: f64,
    decay: f64,
}

impl Default for LoopControls {
    fn default() -> Self {
        Self {
            bars: 4,
            source: LoopSource::Master,
            dry_wet: 0.5,
            decay: 0.0,
        }
    }
}

impl LoopControls {
    fn as_commands(self) -> [RtCommand; 4] {
        [
            RtCommand::LoopBars { bars: self.bars },
            RtCommand::LoopSource {
                source: self.source,
            },
            RtCommand::LoopDryWet {
                amount: self.dry_wet as f32,
            },
            RtCommand::LoopDecay {
                decay: self.decay as f32,
            },
        ]
    }
}

#[derive(Default)]
struct DeckControl {
    track: Option<TrackInfo>,
    peaks: Option<Arc<WaveformPeaks>>,
    stem_peaks: Option<Arc<[WaveformPeaks; Stem::COUNT]>>,
    loading: bool,
    analysing: bool,
    analysis: Option<TrackAnalysis>,
    /// The result of the analysis, waiting for room in the queue to the processor.
    pending_analysis: Option<GridEdit>,
    /// A gain in decibels autogain sets, waiting for room in the queue.
    pending_gain: Option<f64>,
    /// Counts loads and ejects, so a decode that finishes late is recognised as stale.
    generation: u64,
    /// The generation of the track the deck holds, so the analysis of a track that is no
    /// longer in it, or never got in, is recognised as stale.
    held: Option<u64>,
    controls: DeckControls,
    /// The grid, the loop and the stored cues as the processor last reported them.
    grid: BeatGrid,
    looping: LoopState,
    cues: Cues,
    tap: TapTempo,
    /// The audio of the track the deck holds, kept for a capture into a remix deck.
    audio: Option<TrackAudio>,
    /// The grid of samples: `docs/behavior/remix-deck.md`. A restart of the engine empties it.
    /// Boxed, so the engine stays small on the stack of the thread that builds it.
    remix: Box<RemixControl>,
}

struct Loaded {
    audio: TrackAudio,
    /// What the collection kept of the track.
    memory: TrackMemory,
    /// The stretch of key lock, built on the load worker since building it allocates.
    stretch: Option<Stretcher>,
    info: TrackInfo,
    peaks: WaveformPeaks,
    /// The stems, when they were asked for and could be read, with their waveforms.
    stems: Option<(Box<DeckStems>, [WaveformPeaks; Stem::COUNT])>,
    /// Why the stems asked for could not be played.
    stems_failed: Option<String>,
}

/// What a load worker sends back: the decoded track and then, once it is analysed, the
/// analysis. A worker whose decode fails sends only the first.
enum LoadResult {
    Preview {
        generation: u64,
        result: Result<Box<PreviewLoaded>, DecodeError>,
    },
    Decoded {
        deck: DeckId,
        generation: u64,
        result: Result<Box<Loaded>, DecodeError>,
    },
    Analysed {
        deck: DeckId,
        generation: u64,
        analysis: TrackAnalysis,
    },
    Remix {
        deck: DeckId,
        at: CellId,
        generation: u64,
        result: Result<Box<DecodedSample>, String>,
    },
}

/// The track of the preview player, as the control side keeps it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreviewSnapshot {
    pub track: Option<TrackInfo>,
    pub peaks: Option<Arc<WaveformPeaks>>,
    /// A file is being decoded for the player.
    pub loading: bool,
    pub playing: bool,
    pub position_seconds: f64,
}

/// What a worker decoded for the preview player.
struct PreviewLoaded {
    audio: TrackAudio,
    info: TrackInfo,
    peaks: WaveformPeaks,
}

#[derive(Default)]
struct PreviewControl {
    track: Option<TrackInfo>,
    peaks: Option<Arc<WaveformPeaks>>,
    loading: bool,
    /// Counts loads, so a decode that finishes late is recognised as stale.
    generation: u64,
}

pub struct Engine {
    ends: ControlEnds,
    decks: [DeckControl; DeckId::COUNT],
    preview: PreviewControl,
    mixer: MixerControls,
    effects: EffectControls,
    /// The tempo master is chosen automatically.
    clock_auto: bool,
    sync_mode: SyncMode,
    snap: bool,
    quantize: bool,
    metronome: bool,
    midi_clock: bool,
    /// The master clock follows a received MIDI clock.
    external_clock: bool,
    /// A track with a cue of the type load starts at it.
    load_marker: bool,
    /// A deck crossing a fade-out cue starts its partner from its fade-in cue.
    fade_markers: bool,
    /// Where each playing deck was at the last poll, to see a fade-out cue being crossed.
    marker_positions: [Option<f64>; DeckId::COUNT],
    clock_tap: TapTempo,
    /// An automatic fade was started and nothing has set the crossfader since: the
    /// processor knows where it is.
    crossfading: bool,
    /// Swaps sent whose retired item has not come back yet.
    swaps_in_flight: usize,
    /// Load workers that have not sent their last report.
    loads_running: usize,
    /// Events a finished load gives besides its own, for the next poll to hand out.
    later_events: Vec<Event>,
    finished_loads: Receiver<LoadResult>,
    load_reports: Sender<LoadResult>,
    analyser: Arc<dyn Analyser>,
    /// The workers key lock runs on, when it runs on worker threads.
    stretch_pool: Option<Arc<StretchPool>>,
    recall: Option<Recall>,
    stem_recall: Option<StemRecall>,
    recorder: Recorder,
    loop_recorder: LoopControls,
    rate: SampleRate,
}

/// Gives the grid and the cues kept for the file at a path. It runs on the worker that
/// loads the track, so it may read a file.
pub type Recall = Arc<dyn Fn(&Path) -> Option<TrackMemory> + Send + Sync>;

/// What the collection links to a track as its stems, in the order of [`Stem::ALL`]; it is
/// called on the worker that loads the track: `docs/behavior/stems.md`.
pub type StemRecall = Arc<dyn Fn(&Path) -> Option<[PathBuf; Stem::COUNT]> + Send + Sync>;

/// Which stems a load plays.
enum Stems {
    /// Those the collection links to the track, if any.
    Linked,
    None,
    Given(Box<[PathBuf; Stem::COUNT]>),
}

impl Engine {
    /// Builds an engine that runs at `rate` and the processor that produces its audio. The
    /// processor goes to whoever drives the audio: the device callback or the render.
    pub fn new(rate: SampleRate) -> (Self, Processor) {
        Self::with_analyser(rate, Arc::new(PlatterAnalyser::default()))
    }

    /// Runs key lock on worker threads at the priority of audio, or on the audio thread:
    /// `docs/behavior/keylock.md`, *Worker threads*. Tracks loaded from then on follow it; a
    /// loaded track keeps where it plays, and its workers, until it is loaded again.
    pub fn set_key_lock_threads(&mut self, on: bool) -> Result<(), EngineError> {
        if on == self.stretch_pool.is_some() {
            return Ok(());
        }
        self.stretch_pool = if on {
            Some(Arc::new(
                StretchPool::start(self.rate).map_err(EngineError::Worker)?,
            ))
        } else {
            None
        };
        Ok(())
    }

    /// As [`Self::set_key_lock_threads`] on, with workers that run only when
    /// [`Self::pump_key_lock`] is called: for the tests, which run faster than real time.
    pub fn set_key_lock_pumped(&mut self) {
        self.stretch_pool = Some(Arc::new(StretchPool::pumped(self.rate)));
    }

    /// One pass of the workers of [`Self::set_key_lock_pumped`]; nothing otherwise.
    pub fn pump_key_lock(&self) {
        if let Some(pool) = &self.stretch_pool {
            pool.pump();
        }
    }

    /// The workers of key lock that run at the priority of audio, of all of them; `None` while
    /// key lock runs on the audio thread.
    pub fn key_lock_threads(&self) -> Option<(usize, usize)> {
        self.stretch_pool.as_ref().map(|pool| pool.raised())
    }

    /// The frames key lock asked while playing that its workers had not rendered in time.
    pub fn key_lock_starved(&self) -> usize {
        self.stretch_pool.as_ref().map_or(0, |pool| pool.starved())
    }

    /// Takes the writing end of the queue of the input, for the sink of the input stream:
    /// `docs/decisions/0040-audio-input.md`. There is one per engine; it is `None` once
    /// taken.
    pub fn take_input_feed(&mut self) -> Option<InputFeed> {
        self.ends.input.take()
    }

    /// Tells the reader of the input the delay to keep, in frames: the buffer of the input
    /// plus that of the output.
    pub fn set_input_latency(&mut self, frames: usize) -> Result<(), EngineError> {
        self.send(RtCommand::InputLatency { frames })
    }

    /// Brings the master clock to the tempo of a received clock and to where that clock is in
    /// its beat for the next block, while the source of the clock is external; nothing
    /// otherwise.
    pub fn follow_clock(&mut self, bpm: f64, phase: f64) -> Result<(), EngineError> {
        if !self.external_clock {
            return Ok(());
        }
        let bpm = Bpm::new(bpm)?;
        let phase = ValueError::check("phase", phase, 0.0, 1.0)?;
        self.send(RtCommand::ClockFollow { bpm, phase })
    }

    /// Takes the reading end of the queue of the MIDI clock, for its sender:
    /// `docs/decisions/0041-midi-clock.md`. There is one per engine; it is None once
    /// taken.
    pub fn take_clock_feed(&mut self) -> Option<crate::midi_clock::ClockFeed> {
        let events = self.ends.clock.take()?;
        Some(crate::midi_clock::ClockFeed::new(
            events,
            Arc::clone(&self.ends.shared),
        ))
    }

    /// Sets where each load finds the grid and the cues kept for its track.
    pub fn set_recall(&mut self, recall: Recall) {
        self.recall = Some(recall);
    }

    /// Lets `deck.load` find the stems of a track. Without it a track loads alone.
    pub fn set_stem_recall(&mut self, recall: StemRecall) {
        self.stem_recall = Some(recall);
    }

    /// Sets where each recording goes and how it is named. Without it `recorder.toggle` is
    /// refused.
    pub fn set_recording_names(&mut self, names: RecordingNames) {
        self.recorder.names = Some(names);
    }

    /// Sets the size of samples at which the next recording closes a file and opens the next
    /// one, between [`MIN_SPLIT_BYTES`] and [`SPLIT_BYTES`]: `docs/behavior/recording.md`,
    /// *Split*. A recording that runs keeps the size it started with.
    pub fn set_recording_split(&mut self, bytes: u64) {
        self.recorder.split_bytes = bytes.clamp(MIN_SPLIT_BYTES, SPLIT_BYTES);
    }

    /// Sets the share of the loop an overdub takes away on each pass, 0 to 1:
    /// `docs/behavior/recording.md`, *Loop recorder*.
    pub fn set_loop_decay(&mut self, decay: f64) -> Result<(), EngineError> {
        let decay = ValueError::check("decay", decay, 0.0, 1.0)?;
        self.send(RtCommand::LoopDecay {
            decay: decay as f32,
        })?;
        self.loop_recorder.decay = decay;
        Ok(())
    }

    /// Stops a recording that runs and waits up to `wait` for its file to be finished, for
    /// the application to call before it closes. The processor must go on running meanwhile:
    /// it marks where the recording ends.
    pub fn finish_recording(&mut self, wait: Duration) -> Vec<Event> {
        let mut events = Vec::new();
        self.recorder.finish(
            wait,
            &mut self.ends.commands,
            &self.ends.shared,
            &mut events,
        );
        events
    }

    /// As [`Self::new`], with another analysis of the loaded tracks.
    pub fn with_analyser(rate: SampleRate, analyser: Arc<dyn Analyser>) -> (Self, Processor) {
        let (ends, recorded, processor) = rt::channel(rate);
        let (load_reports, finished_loads) = channel();
        let engine = Self {
            ends,
            decks: Default::default(),
            preview: PreviewControl::default(),
            mixer: MixerControls::default(),
            effects: EffectControls::default(),
            clock_auto: true,
            sync_mode: SyncMode::default(),
            snap: false,
            quantize: false,
            metronome: false,
            midi_clock: false,
            external_clock: false,
            load_marker: true,
            fade_markers: false,
            marker_positions: [None; DeckId::COUNT],
            clock_tap: TapTempo::default(),
            crossfading: false,
            swaps_in_flight: 0,
            loads_running: 0,
            later_events: Vec::new(),
            finished_loads,
            load_reports,
            analyser,
            stretch_pool: None,
            recall: None,
            stem_recall: None,
            recorder: Recorder::new(recorded),
            loop_recorder: LoopControls::default(),
            rate,
        };
        let mut engine = engine;
        // The mixer effect of every channel starts on its effect; a fresh queue has room.
        let _ = engine.replay_effects(&EffectControls::default());
        (engine, processor)
    }

    /// Builds an engine at `rate` whose controls stand where those of `previous` stood, and
    /// its processor. The audio device changes its sample rate by replacing the processor;
    /// this keeps the mixer, the clock and the sync of each deck as the user left them.
    /// Tracks are not carried over: they are loaded again by whoever calls this, and the
    /// tempo master is chosen again.
    pub fn with_controls_of(rate: SampleRate, previous: &Self) -> (Self, Processor) {
        let (mut engine, processor) = Self::with_analyser(rate, Arc::clone(&previous.analyser));
        engine.recall = previous.recall.clone();
        engine.stem_recall = previous.stem_recall.clone();
        // New workers at the new rate; without them key lock runs on the audio thread.
        let _ = engine.set_key_lock_threads(previous.stretch_pool.is_some());
        // A recording is not carried over: it ends with the engine that made it.
        engine.recorder.names = previous.recorder.names.clone();
        engine.recorder.split_bytes = previous.recorder.split_bytes;
        let _ = engine.send(RtCommand::RecorderSource {
            external: previous.recorder.external,
        });
        engine.recorder.external = previous.recorder.external;
        // The loop is lost with the engine; its settings are not.
        for command in previous.loop_recorder.as_commands() {
            let _ = engine.send(command);
        }
        engine.loop_recorder = previous.loop_recorder;
        // A fresh queue holds every one of these commands, so none can be refused.
        for deck in DeckId::ALL {
            let controls = previous.decks[deck.index()].controls.clone();
            for command in controls.as_commands(deck) {
                let _ = engine.send(command);
            }
            engine.decks[deck.index()].controls = controls;
        }
        // A running fade stops where the crossfader is.
        let mixer = MixerControls {
            crossfader: previous.crossfader(),
            ..previous.mixer.clone()
        };
        for command in mixer.as_commands() {
            let _ = engine.send(command);
        }
        engine.mixer = mixer;
        let published = &previous.ends.shared;
        let mut clock = vec![
            RtCommand::ClockAuto {
                on: previous.clock_auto,
            },
            RtCommand::KeepBeat {
                on: previous.sync_mode == SyncMode::Beat,
            },
            RtCommand::Snap { on: previous.snap },
            RtCommand::Quantize {
                on: previous.quantize,
            },
            RtCommand::Metronome {
                on: previous.metronome,
            },
            RtCommand::MidiClock {
                on: previous.midi_clock,
            },
        ];
        // The tempo of a deck that was the tempo master may be one a clock does not take.
        clock.extend(
            Bpm::new(published.clock_bpm.get())
                .ok()
                .map(|bpm| RtCommand::ClockTempo { bpm }),
        );
        let synced = DeckId::ALL
            .into_iter()
            .filter(|deck| published.decks[deck.index()].sync.get());
        clock.extend(synced.map(|deck| RtCommand::SyncToggle { deck }));
        let locked = DeckId::ALL
            .into_iter()
            .filter(|deck| published.decks[deck.index()].key_lock.get());
        clock.extend(locked.map(|deck| RtCommand::KeyLockToggle { deck }));
        let ticking = DeckId::ALL
            .into_iter()
            .filter(|deck| published.decks[deck.index()].beat_tick.get());
        clock.extend(ticking.map(|deck| RtCommand::BeatTickToggle { deck }));
        // After key lock, which a shift turns on.
        let shifted = DeckId::ALL.into_iter().filter_map(|deck| {
            let semitones = published.decks[deck.index()].key_shift.get();
            (semitones != 0.0).then_some(RtCommand::KeySet { deck, semitones })
        });
        clock.extend(shifted);
        let fluxed = DeckId::ALL
            .into_iter()
            .filter(|deck| published.decks[deck.index()].flux.get());
        clock.extend(fluxed.map(|deck| RtCommand::FluxToggle { deck }));
        for command in clock {
            let _ = engine.send(command);
        }
        engine.clock_auto = previous.clock_auto;
        engine.sync_mode = previous.sync_mode;
        engine.snap = previous.snap;
        engine.quantize = previous.quantize;
        engine.metronome = previous.metronome;
        engine.midi_clock = previous.midi_clock;
        engine.external_clock = previous.external_clock;
        engine.load_marker = previous.load_marker;
        engine.fade_markers = previous.fade_markers;
        // A fresh queue has room for all of it: see `docs/decisions/0033-effect-chain.md`.
        let _ = engine.replay_effects(&previous.effects);
        (engine, processor)
    }

    /// Validates a command and applies it. An invalid command changes nothing.
    pub fn dispatch(&mut self, command: Command) -> Result<(), EngineError> {
        match command {
            Command::DeckLoad { deck, path } => self.start_load(deck, path, Stems::Linked),
            Command::DeckLoadTrack { deck, path } => self.start_load(deck, path, Stems::None),
            Command::DeckLoadStems { deck, path, stems } => {
                self.start_load(deck, path, Stems::Given(stems))
            }
            stem @ (Command::DeckStemVolumeSet { .. }
            | Command::DeckStemMuteToggle { .. }
            | Command::DeckStemFilterSet { .. }
            | Command::DeckStemFilterToggle { .. }
            | Command::DeckStemSendSet { .. }
            | Command::DeckStemSendToggle { .. }) => self.dispatch_stem(stem),
            Command::GlobalLoadMarkerToggle => {
                self.load_marker = !self.load_marker;
                Ok(())
            }
            Command::GlobalFadeMarkersToggle => {
                self.fade_markers = !self.fade_markers;
                Ok(())
            }
            Command::PreviewLoad { path } => self.start_preview(path),
            Command::PreviewPlayPause => match self.preview.track {
                Some(_) => self.send(RtCommand::PreviewToggle),
                None => Ok(()),
            },
            Command::PreviewSeek { position } => {
                let position = ValueError::check("position", position, 0.0, 1.0)?;
                match self.preview.track.as_ref().map(|track| track.seconds) {
                    Some(seconds) => self.send(RtCommand::PreviewSeek {
                        seconds: position * seconds,
                    }),
                    None => Ok(()),
                }
            }
            Command::DeckEject { deck } => self.eject(deck),
            Command::DeckPlay { deck } => self.send(RtCommand::Play { deck }),
            Command::DeckPause { deck } => self.send(RtCommand::Pause { deck }),
            Command::DeckPlayPause { deck } => self.send(RtCommand::TogglePlay { deck }),
            Command::DeckCueSet { deck } => self.send(RtCommand::CueSet { deck }),
            Command::DeckCueGoto { deck } => self.send(RtCommand::CueGoto { deck }),
            Command::DeckCue { deck, pressed } => self.send(RtCommand::CueButton { deck, pressed }),
            Command::DeckReverse { deck, pressed } => {
                self.send(RtCommand::Reverse { deck, pressed })
            }
            Command::DeckCup { deck, pressed } => self.send(RtCommand::CupButton { deck, pressed }),
            Command::DeckSeek { deck, seconds } => {
                let seconds = ValueError::check("seconds", seconds, 0.0, MAX_SEEK_SECONDS)?;
                self.send(RtCommand::Seek { deck, seconds })
            }
            Command::DeckTempoSet { deck, ratio } => self.set_tempo(deck, ratio),
            Command::DeckNudge { deck, amount } => {
                let amount = ValueError::check("amount", amount, -MAX_NUDGE, MAX_NUDGE)? as f32;
                self.send(RtCommand::Nudge { deck, amount })
            }
            Command::DeckTempoRangeCycle { deck } => self.cycle_tempo_range(deck),
            Command::DeckTempoRangeSet { deck, percent } => {
                let range = TEMPO_RANGES
                    .into_iter()
                    .find(|range| (range * 100.0).round() as u32 == percent)
                    .ok_or(ValueError::UnknownChoice {
                        name: "percent",
                        expected: "8, 16, 50",
                    })?;
                self.set_tempo_range(deck, range)
            }
            other => self.dispatch_cues(other),
        }
    }

    /// The commands of the stored cues. The slots and the selection are kept by the
    /// processor, so commands in a row act on what the ones before left.
    fn dispatch_cues(&mut self, command: Command) -> Result<(), EngineError> {
        let (deck, edit) = match command {
            Command::DeckHotcueTrigger {
                deck,
                slot,
                pressed,
            } => (deck, CueEdit::Hotcue { slot, pressed }),
            Command::DeckHotcueDelete { deck, slot } => (deck, CueEdit::Delete(slot)),
            Command::DeckHotcueMap { deck, slot } => (deck, CueEdit::Map(slot)),
            Command::DeckCueStore { deck } => (deck, CueEdit::Store),
            Command::DeckCueSelect { deck, direction } => {
                let forward = direction == CueStep::Next;
                (deck, CueEdit::Step { forward })
            }
            Command::DeckCueRename { deck, name } => (deck, CueEdit::Rename(CueName::new(&name))),
            Command::DeckCueTypeSet { deck, kind } => {
                let beats = self.decks[deck.index()].controls.loop_size.beats();
                (deck, CueEdit::Retype { kind, beats })
            }
            Command::DeckCueDelete { deck } => (deck, CueEdit::DeleteSelected),
            other => return self.dispatch_loop(other),
        };
        self.send(RtCommand::Cues { deck, edit })
    }

    /// The commands of loops, jumps and moves. The selections of size and mode are kept
    /// here; the processor gets what they amount to with each loop, jump or move.
    fn dispatch_loop(&mut self, command: Command) -> Result<(), EngineError> {
        let (deck, edit) = match command {
            Command::DeckLoopToggle { deck } => {
                let beats = self.decks[deck.index()].controls.loop_size.beats();
                (deck, LoopEdit::Toggle { beats })
            }
            Command::DeckLoopIn { deck } => (deck, LoopEdit::In),
            Command::DeckLoopOut { deck } => (deck, LoopEdit::Out),
            Command::DeckLoopSizeStep { deck, direction } => {
                let size = self.decks[deck.index()].controls.loop_size;
                let stepped = match direction {
                    SizeStep::Down => size.smaller(),
                    SizeStep::Up => size.larger(),
                };
                let Some(size) = stepped else {
                    return Ok(());
                };
                let beats = size.beats();
                self.send(RtCommand::Loop {
                    deck,
                    edit: LoopEdit::Resize { beats },
                })?;
                self.decks[deck.index()].controls.loop_size = size;
                return Ok(());
            }
            Command::DeckBeatjump { deck, direction } => {
                let by = self.move_span(deck, direction);
                return self.send(RtCommand::Jump { deck, by });
            }
            Command::DeckMove { deck, direction } => {
                let by = self.move_span(deck, direction);
                let part = match self.decks[deck.index()].controls.move_mode {
                    MoveMode::Beatjump => return self.send(RtCommand::Jump { deck, by }),
                    MoveMode::Loop => LoopPart::Whole,
                    MoveMode::LoopIn => LoopPart::In,
                    MoveMode::LoopOut => LoopPart::Out,
                };
                (deck, LoopEdit::Move { part, by })
            }
            Command::DeckMoveModeSet { deck, mode } => {
                self.decks[deck.index()].controls.move_mode = mode;
                return Ok(());
            }
            Command::DeckMoveSizeStep { deck, direction } => {
                let controls = &mut self.decks[deck.index()].controls;
                if let Some(size) = controls.move_size.stepped(direction) {
                    controls.move_size = size;
                }
                return Ok(());
            }
            Command::DeckMoveFineToggle { deck } => {
                let controls = &mut self.decks[deck.index()].controls;
                controls.move_fine = !controls.move_fine;
                return Ok(());
            }
            other => return self.dispatch_grid(other),
        };
        self.send(RtCommand::Loop { deck, edit })
    }

    /// How far one jump or move of a deck goes, as its move size and fine steps stand.
    fn move_span(&self, deck: DeckId, direction: Direction) -> Span {
        let controls = &self.decks[deck.index()].controls;
        if controls.move_fine {
            return Span::Seconds(direction.sign() * FINE_MOVE_SECONDS);
        }
        match controls.move_size {
            MoveSize::Beats(size) => Span::Beats(direction.sign() * size.beats()),
            MoveSize::Loop => Span::Loop {
                forward: direction == Direction::Forward,
                beats: controls.loop_size.beats(),
            },
        }
    }

    /// The commands of the beatgrid.
    fn dispatch_grid(&mut self, command: Command) -> Result<(), EngineError> {
        let (deck, edit) = match command {
            Command::DeckGridBpmSet { deck, bpm } => (deck, GridEdit::SetBpm(Bpm::new(bpm)?)),
            Command::DeckGridBpmScale { deck, factor } => {
                (deck, GridEdit::ScaleBpm(factor.factor()))
            }
            Command::DeckGridBpmStep { deck, amount } => {
                let amount = ValueError::check("amount", amount, -MAX_BPM_STEP, MAX_BPM_STEP)?;
                (deck, GridEdit::StepBpm(amount))
            }
            Command::DeckGridTap { deck } => {
                match self.decks[deck.index()].tap.tap(Instant::now()) {
                    Some(bpm) => (deck, GridEdit::SetBpm(bpm)),
                    None => return Ok(()),
                }
            }
            Command::DeckGridMarkerSet { deck } => (deck, GridEdit::MarkerSet),
            Command::DeckGridMarkerMove { deck, seconds } => {
                let by = ValueError::check(
                    "seconds",
                    seconds,
                    -MAX_MARKER_MOVE_SECONDS,
                    MAX_MARKER_MOVE_SECONDS,
                )?;
                (deck, GridEdit::MarkerMove(by))
            }
            Command::DeckGridMarkerBpmSet { deck, bpm } => {
                (deck, GridEdit::MarkerBpm(Some(Bpm::new(bpm)?)))
            }
            Command::DeckGridMarkerBpmClear { deck } => (deck, GridEdit::MarkerBpm(None)),
            Command::DeckGridMarkerDelete { deck } => (deck, GridEdit::MarkerDelete),
            Command::DeckGridMarkerPrevious { deck } => (deck, GridEdit::MarkerPrevious),
            Command::DeckGridMarkerNext { deck } => (deck, GridEdit::MarkerNext),
            Command::DeckGridLockToggle { deck } => (deck, GridEdit::ToggleLock),
            Command::DeckGridReset { deck } => match self.decks[deck.index()].analysis {
                Some(analysis) => (
                    deck,
                    GridEdit::Reset(analysis.tempo.map(|tempo| (tempo.bpm, tempo.first_beat))),
                ),
                None => return Ok(()),
            },
            Command::DeckGridBpmReset { deck } => {
                match self.decks[deck.index()]
                    .analysis
                    .and_then(|found| found.tempo)
                {
                    Some(tempo) => (
                        deck,
                        GridEdit::ResetBpm {
                            bpm: tempo.bpm,
                            first_beat: tempo.first_beat,
                        },
                    ),
                    None => return Ok(()),
                }
            }
            other => return self.dispatch_sync(other),
        };
        self.send(RtCommand::Grid { deck, edit })
    }

    /// The commands of sync and of the master clock. The tempo master, the tempo of the
    /// clock and the sync of each deck are kept by the processor, which follows what the
    /// decks play.
    fn dispatch_sync(&mut self, command: Command) -> Result<(), EngineError> {
        let command = match command {
            Command::DeckSyncToggle { deck } => RtCommand::SyncToggle { deck },
            Command::DeckKeyLockToggle { deck } => RtCommand::KeyLockToggle { deck },
            Command::DeckBeatTickToggle { deck } => RtCommand::BeatTickToggle { deck },
            Command::DeckKeyStep { deck, direction } => RtCommand::KeyStep {
                deck,
                up: direction == SizeStep::Up,
            },
            Command::DeckKeySet { deck, semitones } => RtCommand::KeySet {
                deck,
                semitones: ValueError::check(
                    "semitones",
                    semitones,
                    -MAX_KEY_SHIFT,
                    MAX_KEY_SHIFT,
                )?,
            },
            Command::DeckKeyPersist { deck } => RtCommand::KeyPersist { deck },
            Command::DeckFluxToggle { deck } => RtCommand::FluxToggle { deck },
            // While the clock follows a received one, it is the tempo master and its tempo is
            // that clock's: these do nothing.
            Command::DeckMasterSet { .. }
            | Command::ClockModeSet { .. }
            | Command::ClockTempoSet { .. }
            | Command::ClockTempoStep { .. }
            | Command::ClockTap
            | Command::ClockBend { .. }
                if self.external_clock =>
            {
                return Ok(());
            }
            Command::ClockSourceSet { external } => {
                if external {
                    self.send(RtCommand::ClockAuto { on: false })?;
                    self.send(RtCommand::ClockBend { amount: 0.0 })?;
                    self.clock_auto = false;
                }
                self.external_clock = external;
                return Ok(());
            }
            Command::DeckMasterSet { deck } => RtCommand::Master { deck },
            Command::ClockModeSet { mode } => {
                let on = mode == ClockMode::Auto;
                self.send(RtCommand::ClockAuto { on })?;
                self.clock_auto = on;
                return Ok(());
            }
            Command::ClockTempoSet { bpm } => RtCommand::ClockTempo {
                bpm: Bpm::new(bpm)?,
            },
            Command::ClockTempoStep { amount } => {
                let amount = ValueError::check("amount", amount, -MAX_BPM_STEP, MAX_BPM_STEP)?;
                RtCommand::ClockStep { amount }
            }
            Command::ClockTap => match self.clock_tap.tap(Instant::now()) {
                Some(bpm) => RtCommand::ClockTempo { bpm },
                None => return Ok(()),
            },
            Command::ClockBend { amount } => {
                let amount = ValueError::check("amount", amount, -MAX_NUDGE, MAX_NUDGE)? as f32;
                RtCommand::ClockBend { amount }
            }
            Command::GlobalSnapToggle => {
                let on = !self.snap;
                self.send(RtCommand::Snap { on })?;
                self.snap = on;
                return Ok(());
            }
            Command::GlobalQuantizeToggle => {
                let on = !self.quantize;
                self.send(RtCommand::Quantize { on })?;
                self.quantize = on;
                return Ok(());
            }
            Command::ClockMidiSendToggle => {
                let on = !self.midi_clock;
                self.send(RtCommand::MidiClock { on })?;
                self.midi_clock = on;
                return Ok(());
            }
            Command::ClockMidiRestart => RtCommand::MidiClockRestart,
            Command::ClockMetronomeToggle => {
                let on = !self.metronome;
                self.send(RtCommand::Metronome { on })?;
                self.metronome = on;
                return Ok(());
            }
            Command::RecorderToggle => {
                return self.recorder.toggle(self.rate, &mut self.ends.commands);
            }
            Command::RecorderSourceSet { external } => {
                self.send(RtCommand::RecorderSource { external })?;
                self.recorder.external = external;
                return Ok(());
            }
            Command::RecorderCut => {
                self.recorder.cut();
                return Ok(());
            }
            Command::LoopRecorderRecord
            | Command::LoopRecorderPlay
            | Command::LoopRecorderUndo
            | Command::LoopRecorderRedo
            | Command::LoopRecorderDelete
            | Command::LoopRecorderSizeSet { .. }
            | Command::LoopRecorderSourceSet { .. }
            | Command::LoopRecorderDryWetSet { .. } => return self.dispatch_loop_recorder(command),
            Command::GlobalSyncModeSet { mode } => {
                let on = mode == SyncMode::Beat;
                self.send(RtCommand::KeepBeat { on })?;
                self.sync_mode = mode;
                return Ok(());
            }
            mixer => return self.dispatch_channel(mixer),
        };
        self.send(command)
    }

    fn set_tempo(&mut self, deck: DeckId, ratio: f64) -> Result<(), EngineError> {
        let ratio = Tempo::from_ratio(ratio)?.ratio();
        self.send(RtCommand::Tempo {
            deck,
            ratio: ratio as f32,
        })?;
        self.decks[deck.index()].controls.tempo_ratio = ratio;
        Ok(())
    }

    /// Moves to the next range. A tempo the new range cannot reach moves to its edge, so
    /// the fader and the speed never disagree.
    fn cycle_tempo_range(&mut self, deck: DeckId) -> Result<(), EngineError> {
        let controls = &self.decks[deck.index()].controls;
        let current = TEMPO_RANGES
            .iter()
            .position(|range| *range == controls.tempo_range)
            .unwrap_or(0);
        let range = TEMPO_RANGES[(current + 1) % TEMPO_RANGES.len()];
        self.set_tempo_range(deck, range)
    }

    /// Gives the fader `range`. A tempo the range cannot reach moves to its edge, so the fader
    /// and the speed never disagree.
    fn set_tempo_range(&mut self, deck: DeckId, range: f64) -> Result<(), EngineError> {
        let controls = &self.decks[deck.index()].controls;
        let reachable = controls.tempo_ratio.clamp(1.0 - range, 1.0 + range);
        if reachable != controls.tempo_ratio {
            self.set_tempo(deck, reachable)?;
        }
        self.decks[deck.index()].controls.tempo_range = range;
        Ok(())
    }

    /// The commands of one mixer channel.
    fn dispatch_channel(&mut self, command: Command) -> Result<(), EngineError> {
        match command {
            Command::MixerGainSet { deck, db } => {
                self.set_gain(deck, db)?;
                // An autogain still waiting for room in the queue would undo the knob.
                self.decks[deck.index()].pending_gain = None;
            }
            Command::MixerGainRestore { deck } => {
                let state = &self.decks[deck.index()];
                if let Some(db) = state.analysis.and_then(|analysis| analysis.autogain_db()) {
                    self.set_gain(deck, db)?;
                }
            }
            Command::MixerEqSet {
                deck,
                band,
                position,
            } => {
                let position = ValueError::check("position", position, 0.0, 1.0)?;
                let gain = band_gain(position as f32);
                self.send(RtCommand::Eq { deck, band, gain })?;
                self.decks[deck.index()].controls.eq[band as usize] = position;
            }
            Command::MixerEqKillToggle { deck, band } => {
                let cut = !self.decks[deck.index()].controls.eq_kill[band as usize];
                self.send(RtCommand::EqCut { deck, band, cut })?;
                self.decks[deck.index()].controls.eq_kill[band as usize] = cut;
            }
            Command::MixerFilterSet { deck, position } => {
                let position = ValueError::check("position", position, -1.0, 1.0)?;
                self.send(RtCommand::Filter {
                    deck,
                    position: position as f32,
                })?;
                self.decks[deck.index()].controls.filter = position;
            }
            Command::MixerPanSet { deck, position } => {
                let position = ValueError::check("position", position, -1.0, 1.0)?;
                self.send(RtCommand::Balance {
                    deck,
                    position: position as f32,
                })?;
                self.decks[deck.index()].controls.pan = position;
            }
            Command::MixerFaderSet { deck, position } => {
                let gain = fader_gain(position)?;
                self.send(RtCommand::Fader { deck, gain })?;
                self.decks[deck.index()].controls.fader = position;
            }
            Command::MixerCueToggle { deck } => {
                let on = !self.decks[deck.index()].controls.pre_listen;
                self.send(RtCommand::PreListen { deck, on })?;
                self.decks[deck.index()].controls.pre_listen = on;
            }
            shared => return self.dispatch_shared(shared),
        }
        Ok(())
    }

    /// The commands of the controls all the decks share.
    fn dispatch_shared(&mut self, command: Command) -> Result<(), EngineError> {
        match command {
            Command::MixerCrossfaderSet { position } => {
                let position = ValueError::check("position", position, 0.0, 1.0)?;
                self.set_crossfader(position)?;
            }
            Command::MixerCrossfaderStep { direction } => {
                let towards = direction.end();
                let from = self.crossfader();
                let position = if towards > from {
                    (from + CROSSFADER_STEP).min(towards)
                } else {
                    (from - CROSSFADER_STEP).max(towards)
                };
                self.set_crossfader(position)?;
            }
            Command::MixerCrossfaderAutoFade { direction } => {
                self.send(RtCommand::CrossfaderFade {
                    to: direction.end() as f32,
                    beats: self.mixer.fade_beats as f32,
                })?;
                self.crossfading = true;
            }
            Command::MixerCrossfaderFadeBeatsSet { beats } => {
                let beats = ValueError::check("beats", beats, MIN_FADE_BEATS, MAX_FADE_BEATS)?;
                self.mixer.fade_beats = beats;
            }
            Command::MixerCrossfaderCurveSet { curve } => {
                self.send(RtCommand::CrossfaderCurve { curve })?;
                self.mixer.crossfader_curve = curve;
            }
            Command::MixerEqModelSet { model } => {
                self.send(RtCommand::EqModel { model })?;
                self.mixer.eq_model = model;
            }
            Command::MixerFilterModelSet { model } => {
                self.send(RtCommand::FilterModel { model })?;
                self.mixer.filter_model = model;
            }
            Command::MixerModeSet { mode } => self.set_external(MixerControls {
                mode,
                ..self.mixer.clone()
            })?,
            Command::MixerExternalEqGainToggle => self.set_external(MixerControls {
                external_eq_gain: !self.mixer.external_eq_gain,
                ..self.mixer.clone()
            })?,
            Command::MixerExternalMasterVolumeToggle => self.set_external(MixerControls {
                external_master_volume: !self.mixer.external_master_volume,
                ..self.mixer.clone()
            })?,
            Command::MixerAuxSet { level } => {
                let level = ValueError::check("level", level, 0.0, 1.0)?;
                self.send(RtCommand::AuxLevel {
                    level: level as f32,
                })?;
                self.mixer.aux_level = level;
            }
            Command::InputRoutingSet { target, source } => {
                self.send(RtCommand::InputRoute { target, source })?;
                self.mixer.input_routes[target.index()] = source;
            }
            Command::DeckTypeSet { deck, kind } => {
                self.send(RtCommand::DeckType { deck, kind })?;
                self.decks[deck.index()].controls.kind = kind;
            }
            Command::MixerCrossfaderAssign { deck, side } => {
                self.send(RtCommand::CrossfaderAssign { deck, side })?;
                self.mixer.crossfader_sides[deck.index()] = side;
            }
            Command::MixerCueMixSet { position } => {
                let position = ValueError::check("position", position, 0.0, 1.0)?;
                self.send(RtCommand::HeadphoneMix {
                    position: position as f32,
                })?;
                self.mixer.headphone_mix = position;
            }
            Command::MixerCueVolumeSet { db } => {
                let linear = gain_up_to(db, MAX_HEADPHONE_GAIN_DB)?;
                self.send(RtCommand::HeadphoneGain { linear })?;
                self.mixer.headphone_gain_db = db.max(Gain::MIN_DB);
            }
            Command::MasterGainSet { db } => {
                let linear = gain_up_to(db, MAX_MASTER_GAIN_DB)?;
                self.send(RtCommand::MasterGain { linear })?;
                self.mixer.master_gain_db = db.max(Gain::MIN_DB);
            }
            Command::MasterTestTone { output } => self.send(RtCommand::TestTone { output })?,
            Command::MasterLimiterToggle => self.set_maximiser(MaximiserControls {
                on: !self.mixer.maximiser.on,
                ..self.mixer.maximiser
            })?,
            Command::MasterMaximiserDriveSet { db } => {
                let drive_db = ValueError::check("db", db, 0.0, MAX_DRIVE_DB)?;
                self.set_maximiser(MaximiserControls {
                    drive_db,
                    ..self.mixer.maximiser
                })?;
            }
            Command::MasterMaximiserCeilingSet { db } => {
                let ceiling_db = ValueError::check("db", db, MIN_CEILING_DB, MAX_CEILING_DB)?;
                self.set_maximiser(MaximiserControls {
                    ceiling_db,
                    ..self.mixer.maximiser
                })?;
            }
            Command::MasterMaximiserReleaseSet { ms } => {
                let release_ms = ValueError::check("ms", ms, MIN_RELEASE_MS, MAX_RELEASE_MS)?;
                self.set_maximiser(MaximiserControls {
                    release_ms,
                    ..self.mixer.maximiser
                })?;
            }
            Command::MixerAutogainToggle => {
                let on = !self.mixer.autogain;
                self.mixer.autogain = on;
                for state in &mut self.decks {
                    state.pending_gain = state
                        .analysis
                        .filter(|_| on)
                        .and_then(|analysis| analysis.autogain_db());
                }
                self.send_pending_analyses();
            }
            effect @ (Command::FxUnitModeSet { .. }
            | Command::FxUnitDryWetSet { .. }
            | Command::FxUnitRoutingSet { .. }
            | Command::FxUnitReset { .. }
            | Command::FxUnitSnapshotSave { .. }
            | Command::FxKeepValuesToggle
            | Command::FxSlotEffectSet { .. }
            | Command::FxSlotAmountSet { .. }
            | Command::FxSlotToggle { .. }
            | Command::FxSingleEffectSet { .. }
            | Command::FxSingleKnobSet { .. }
            | Command::FxSingleKeyToggle { .. }
            | Command::FxSingleToggle { .. }
            | Command::MixerFxAssignToggle { .. }
            | Command::MixerEffectSelect { .. }
            | Command::MixerEffectSet { .. }) => self.dispatch_effects(effect)?,
            remix @ (Command::RemixCellLoad { .. }
            | Command::RemixCellClear { .. }
            | Command::RemixCellTrigger { .. }
            | Command::RemixCellRelease { .. }
            | Command::RemixCellTriggerModeSet { .. }
            | Command::RemixCellReverseToggle { .. }
            | Command::RemixCellCapture { .. }
            | Command::RemixSlotPunchToggle { .. }
            | Command::RemixCellModeSet { .. }
            | Command::RemixCellBpmSet { .. }
            | Command::RemixCellBpmClear { .. }
            | Command::RemixCellGainSet { .. }
            | Command::RemixSlotStop { .. }
            | Command::RemixSlotVolumeSet { .. }
            | Command::RemixSlotMuteToggle { .. }
            | Command::RemixQuantizeSet { .. }) => self.dispatch_remix(remix)?,
            // Every other command is handled before this point.
            _ => {}
        }
        Ok(())
    }

    /// The controls of the stems of a deck: `docs/behavior/stems.md`.
    fn dispatch_stem(&mut self, command: Command) -> Result<(), EngineError> {
        let (deck, stem) = match command {
            Command::DeckStemVolumeSet { deck, stem, .. }
            | Command::DeckStemMuteToggle { deck, stem }
            | Command::DeckStemFilterSet { deck, stem, .. }
            | Command::DeckStemFilterToggle { deck, stem }
            | Command::DeckStemSendSet { deck, stem, .. }
            | Command::DeckStemSendToggle { deck, stem } => (deck, stem),
            _ => return Ok(()),
        };
        let mut settings = self.decks[deck.index()].controls.stems[stem.index()];
        match command {
            Command::DeckStemVolumeSet { volume, .. } => {
                settings.volume = ValueError::check("volume", volume, 0.0, 1.0)? as f32;
            }
            Command::DeckStemMuteToggle { .. } => settings.muted = !settings.muted,
            Command::DeckStemFilterSet { position, .. } => {
                settings.filter = ValueError::check("position", position, -1.0, 1.0)? as f32;
            }
            Command::DeckStemFilterToggle { .. } => settings.filter_on = !settings.filter_on,
            Command::DeckStemSendSet { amount, .. } => {
                settings.send = ValueError::check("amount", amount, 0.0, 1.0)? as f32;
            }
            Command::DeckStemSendToggle { .. } => settings.send_on = !settings.send_on,
            _ => {}
        }
        self.send(RtCommand::Stem {
            deck,
            stem,
            settings,
        })?;
        self.decks[deck.index()].controls.stems[stem.index()] = settings;
        Ok(())
    }

    /// The commands of the loop recorder. Rec is refused in external mixer mode, where the
    /// mix the loop plays into is not heard, whatever its source.
    fn dispatch_loop_recorder(&mut self, command: Command) -> Result<(), EngineError> {
        let rt = match command {
            Command::LoopRecorderRecord => {
                if self.mixer.mode == MixerMode::External {
                    return Err(EngineError::ExternalMixer);
                }
                RtCommand::LoopRecord
            }
            Command::LoopRecorderPlay => RtCommand::LoopPlay,
            Command::LoopRecorderUndo => RtCommand::LoopUndo,
            Command::LoopRecorderRedo => RtCommand::LoopRedo,
            Command::LoopRecorderDelete => RtCommand::LoopDelete,
            Command::LoopRecorderSizeSet { bars } => {
                let bars = LOOP_BARS
                    .into_iter()
                    .find(|size| f64::from(*size) == bars)
                    .ok_or(ValueError::UnknownChoice {
                        name: "bars",
                        expected: "1, 2, 4, 8",
                    })?;
                // The processor takes a size only while nothing is recorded.
                if LoopPhase::from_code(
                    u8::try_from(self.ends.shared.loop_phase.get()).unwrap_or(0),
                ) != LoopPhase::Empty
                {
                    return Ok(());
                }
                self.send(RtCommand::LoopBars { bars })?;
                self.loop_recorder.bars = bars;
                return Ok(());
            }
            Command::LoopRecorderSourceSet { source } => {
                self.send(RtCommand::LoopSource { source })?;
                self.loop_recorder.source = source;
                return Ok(());
            }
            Command::LoopRecorderDryWetSet { amount } => {
                let amount = ValueError::check("amount", amount, 0.0, 1.0)?;
                self.send(RtCommand::LoopDryWet {
                    amount: amount as f32,
                })?;
                self.loop_recorder.dry_wet = amount;
                return Ok(());
            }
            _ => return Ok(()),
        };
        self.send(rt)
    }

    /// Sends the external mode and its options of `mixer`, and keeps them once sent.
    fn set_external(&mut self, mixer: MixerControls) -> Result<(), EngineError> {
        self.send(mixer.external())?;
        self.mixer = mixer;
        Ok(())
    }

    fn set_maximiser(&mut self, maximiser: MaximiserControls) -> Result<(), EngineError> {
        self.send(maximiser.command())?;
        self.mixer.maximiser = maximiser;
        Ok(())
    }

    fn set_crossfader(&mut self, position: f64) -> Result<(), EngineError> {
        self.send(RtCommand::Crossfader {
            position: position as f32,
        })?;
        self.mixer.crossfader = position;
        self.crossfading = false;
        Ok(())
    }

    /// Where the crossfader is: during an automatic fade, as the processor last reported.
    fn crossfader(&self) -> f64 {
        if self.crossfading {
            f64::from(self.ends.shared.crossfader.get())
        } else {
            self.mixer.crossfader
        }
    }

    fn set_gain(&mut self, deck: DeckId, db: f64) -> Result<(), EngineError> {
        let linear = gain_up_to(db, MAX_CHANNEL_GAIN_DB)?;
        self.send(RtCommand::Gain { deck, linear })?;
        self.decks[deck.index()].controls.gain_db = db.max(Gain::MIN_DB);
        Ok(())
    }

    fn send(&mut self, command: RtCommand) -> Result<(), EngineError> {
        self.ends
            .commands
            .push(command)
            .map_err(|_| EngineError::Busy)
    }

    fn send_swap(&mut self, deck: DeckId, track: Option<DeckTrack>) -> Result<(), EngineError> {
        self.collect_retired();
        // The retired queue has room for this many; see `rt::channel`.
        if self.swaps_in_flight >= COMMAND_CAPACITY {
            return Err(EngineError::Busy);
        }
        self.send(RtCommand::Swap { deck, track })?;
        self.swaps_in_flight += 1;
        Ok(())
    }

    /// Frees, on this thread, the audio the processor has stopped using.
    fn collect_retired(&mut self) {
        while self.ends.retired.pop().is_ok() {
            self.swaps_in_flight = self.swaps_in_flight.saturating_sub(1);
        }
    }

    /// Applies a command of a remix deck: `docs/behavior/remix-deck.md`.
    fn dispatch_remix(&mut self, command: Command) -> Result<(), EngineError> {
        match command {
            Command::RemixCellLoad { deck, at, path } => self.start_remix_load(deck, at, path)?,
            Command::RemixCellClear { deck, at } => {
                let control = &mut self.decks[deck.index()].remix;
                // A decode still running for the cell is forgotten.
                control.generations[at.slot][at.cell] += 1;
                control.shown.loading[at.slot][at.cell] = false;
                self.send_effect_swap(RtCommand::RemixLoad {
                    deck,
                    at,
                    audio: None,
                    settings: CellSettings::default(),
                })?;
                self.decks[deck.index()].remix.shown.cells[at.slot][at.cell] = None;
            }
            Command::RemixCellTrigger { deck, at } => {
                self.send(RtCommand::RemixTrigger { deck, at })?;
            }
            Command::RemixCellRelease { deck, at } => {
                self.send(RtCommand::RemixRelease { deck, at })?;
            }
            Command::RemixCellTriggerModeSet { deck, at, gate } => {
                self.update_cell(deck, at, |cell| cell.gate = gate)?;
            }
            Command::RemixCellReverseToggle { deck, at } => {
                self.update_cell(deck, at, |cell| cell.reverse = !cell.reverse)?;
            }
            Command::RemixCellCapture { deck, at, from } => self.start_capture(deck, at, from)?,
            Command::RemixSlotPunchToggle { deck, slot } => {
                let on = !self.decks[deck.index()].remix.shown.punch[slot];
                self.send(RtCommand::RemixPunch { deck, slot, on })?;
                self.decks[deck.index()].remix.shown.punch[slot] = on;
            }
            Command::RemixCellModeSet { deck, at, mode } => {
                self.update_cell(deck, at, |cell| cell.mode = mode)?;
            }
            Command::RemixCellBpmSet { deck, at, bpm } => {
                self.update_cell(deck, at, |cell| cell.bpm = Some(bpm))?;
            }
            Command::RemixCellBpmClear { deck, at } => {
                self.update_cell(deck, at, |cell| cell.bpm = None)?;
            }
            Command::RemixCellGainSet { deck, at, db } => {
                let db = ValueError::check("db", db, -MAX_CELL_GAIN_DB, MAX_CELL_GAIN_DB)?;
                self.update_cell(deck, at, |cell| cell.gain_db = db)?;
            }
            Command::RemixSlotStop { deck, slot } => {
                self.send(RtCommand::RemixStop { deck, slot })?;
            }
            Command::RemixSlotVolumeSet { deck, slot, level } => {
                let level = ValueError::check("level", level, 0.0, 1.0)?;
                self.send(RtCommand::RemixVolume {
                    deck,
                    slot,
                    level: level as f32,
                })?;
                self.decks[deck.index()].remix.shown.volume[slot] = level;
            }
            Command::RemixSlotMuteToggle { deck, slot } => {
                let on = !self.decks[deck.index()].remix.shown.muted[slot];
                self.send(RtCommand::RemixMute { deck, slot, on })?;
                self.decks[deck.index()].remix.shown.muted[slot] = on;
            }
            Command::RemixQuantizeSet { deck, beats } => {
                self.send(RtCommand::RemixQuantize { deck, beats })?;
                self.decks[deck.index()].remix.shown.quantize = beats;
            }
            // Every other command is handled by `dispatch`.
            _ => {}
        }
        Ok(())
    }

    /// Changes how a cell that holds a sample plays; an empty cell is left as it is.
    fn update_cell(
        &mut self,
        deck: DeckId,
        at: CellId,
        change: impl FnOnce(&mut crate::remix::CellInfo),
    ) -> Result<(), EngineError> {
        let Some(cell) = self.decks[deck.index()].remix.cell(at) else {
            return Ok(());
        };
        let mut changed = cell.clone();
        change(&mut changed);
        self.send(RtCommand::RemixSettings {
            deck,
            at,
            settings: changed.settings(),
        })?;
        if let Some(cell) = self.decks[deck.index()].remix.cell(at) {
            *cell = changed;
        }
        Ok(())
    }

    /// Copies the loop of deck `from` into a cell, on a worker thread: the loop that is on or,
    /// without one, the size of a new loop from where the deck plays.
    fn start_capture(&mut self, deck: DeckId, at: CellId, from: DeckId) -> Result<(), EngineError> {
        // The loop and the grid as the processor has them now, not as of the last poll.
        self.take_reports();
        let source = &self.decks[from.index()];
        let (Some(audio), Some(info)) = (source.audio.clone(), source.track.clone()) else {
            return Err(EngineError::NothingToCapture);
        };
        let position = self.ends.shared.decks[from.index()].position.get();
        let bpm = source.grid.bpm_at(position);
        let (start, seconds) = match source.looping.region {
            Some(region) => (region.start, region.seconds()),
            None => {
                let bpm = bpm.ok_or(EngineError::NothingToCapture)?;
                (
                    position,
                    source.controls.loop_size.beats() * bpm.beat_seconds(),
                )
            }
        };
        let control = &mut self.decks[deck.index()].remix;
        control.generations[at.slot][at.cell] += 1;
        let generation = control.generations[at.slot][at.cell];
        let reports = self.load_reports.clone();
        std::thread::Builder::new()
            .name("platter-capture".to_owned())
            .spawn(move || {
                let result = capture_sample(&audio, &info, start, seconds, bpm).map(Box::new);
                // The engine may be gone by now; then nobody waits for the result.
                let _ = reports.send(LoadResult::Remix {
                    deck,
                    at,
                    generation,
                    result,
                });
            })
            .map_err(EngineError::Worker)?;
        self.decks[deck.index()].remix.shown.loading[at.slot][at.cell] = true;
        self.loads_running += 1;
        Ok(())
    }

    /// Decodes `path` for a cell on a worker thread of its own.
    fn start_remix_load(
        &mut self,
        deck: DeckId,
        at: CellId,
        path: PathBuf,
    ) -> Result<(), EngineError> {
        let control = &mut self.decks[deck.index()].remix;
        control.generations[at.slot][at.cell] += 1;
        let generation = control.generations[at.slot][at.cell];
        let reports = self.load_reports.clone();
        let analyser = Arc::clone(&self.analyser);
        let recall = self.recall.clone();
        std::thread::Builder::new()
            .name("platter-sample".to_owned())
            .spawn(move || {
                let memory = recall.as_ref().and_then(|recall| recall(&path));
                let result = decode_sample(&path, memory, analyser.as_ref()).map(Box::new);
                // The engine may be gone by now; then nobody waits for the result.
                let _ = reports.send(LoadResult::Remix {
                    deck,
                    at,
                    generation,
                    result,
                });
            })
            .map_err(EngineError::Worker)?;
        self.decks[deck.index()].remix.shown.loading[at.slot][at.cell] = true;
        self.loads_running += 1;
        Ok(())
    }

    fn finish_remix(
        &mut self,
        deck: DeckId,
        at: CellId,
        generation: u64,
        result: Result<Box<DecodedSample>, String>,
    ) -> Option<Event> {
        self.loads_running = self.loads_running.saturating_sub(1);
        let control = &mut self.decks[deck.index()].remix;
        if control.generations[at.slot][at.cell] != generation {
            return None;
        }
        control.shown.loading[at.slot][at.cell] = false;
        let failed = |message: String| Some(Event::RemixLoadFailed { deck, at, message });
        let sample = match result {
            Ok(sample) => *sample,
            Err(message) => return failed(message),
        };
        let settings = sample.info.settings();
        if let Err(error) = self.send_effect_swap(RtCommand::RemixLoad {
            deck,
            at,
            audio: Some(sample.audio),
            settings,
        }) {
            return failed(error.to_string());
        }
        self.decks[deck.index()].remix.shown.cells[at.slot][at.cell] = Some(sample.info);
        None
    }

    /// Applies a command of the effect units or of a mixer effect: `docs/behavior/effects.md`.
    fn dispatch_effects(&mut self, command: Command) -> Result<(), EngineError> {
        let unit_value = |position: f64| ValueError::check("position", position, 0.0, 1.0);
        match command {
            Command::FxUnitModeSet { unit, mode } => {
                self.update_unit(unit, |controls| controls.values.mode = mode)
            }
            Command::FxUnitDryWetSet { unit, position } => {
                let position = unit_value(position)? as f32;
                self.update_unit(unit, |controls| controls.values.dry_wet = position)
            }
            Command::FxUnitRoutingSet { unit, routing } => {
                self.send(RtCommand::FxRouting {
                    unit: unit.index(),
                    routing,
                })?;
                self.effects.units[unit.index()].routing = routing;
                Ok(())
            }
            Command::FxUnitReset { unit } => self.update_unit(unit, |controls| {
                if let Some(kind) = controls.single {
                    controls.values.single.controls = kind.defaults();
                }
            }),
            Command::FxUnitSnapshotSave { unit } => {
                let controls = &mut self.effects.units[unit.index()];
                if let Some(index) = controls
                    .single
                    .and_then(|kind| EffectKind::ALL.iter().position(|each| *each == kind))
                {
                    controls.snapshots[index] = Some(controls.values.single.controls);
                }
                Ok(())
            }
            Command::FxKeepValuesToggle => {
                self.effects.keep_values = !self.effects.keep_values;
                Ok(())
            }
            Command::FxSlotEffectSet { unit, slot, effect } => {
                self.set_place(unit, Place::Group(slot), effect)
            }
            Command::FxSlotAmountSet {
                unit,
                slot,
                position,
            } => {
                let position = unit_value(position)? as f32;
                self.update_unit(unit, |controls| {
                    if let Some(values) = controls.values.slots.get_mut(slot) {
                        values.amount = position;
                    }
                })
            }
            Command::FxSlotToggle { unit, slot } => self.update_unit(unit, |controls| {
                if let Some(values) = controls.values.slots.get_mut(slot) {
                    values.on = !values.on;
                }
            }),
            Command::FxSingleEffectSet { unit, effect } => {
                self.set_place(unit, Place::Single, effect)
            }
            Command::FxSingleKnobSet {
                unit,
                knob,
                position,
            } => {
                let position = unit_value(position)? as f32;
                self.update_unit(unit, |controls| {
                    if let Some(value) = controls.values.single.controls.knobs.get_mut(knob) {
                        *value = position;
                    }
                })
            }
            Command::FxSingleKeyToggle { unit, key } => self.update_unit(unit, |controls| {
                if let Some(value) = controls.values.single.controls.keys.get_mut(key) {
                    *value = !*value;
                }
            }),
            Command::FxSingleToggle { unit } => self.update_unit(unit, |controls| {
                controls.values.single.on = !controls.values.single.on;
            }),
            Command::MixerFxAssignToggle { deck, unit } => self.toggle_assign(deck, unit),
            Command::MixerEffectSelect { deck, effect } => {
                self.ensure_swap_room(1)?;
                self.send_effect_swap(RtCommand::MixerEffectPlace {
                    deck,
                    effect: Some(self.build_effect(effect)),
                })?;
                self.effects.mixer[deck.index()].effect = effect;
                Ok(())
            }
            Command::MixerEffectSet { deck, position } => {
                let position = unit_value(position)?;
                self.send(RtCommand::MixerEffectAmount {
                    deck,
                    amount: position as f32,
                })?;
                self.effects.mixer[deck.index()].amount = position;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Changes the values of a unit and sends them; nothing changes if they cannot be sent.
    fn update_unit(
        &mut self,
        unit: FxUnit,
        change: impl FnOnce(&mut UnitControls),
    ) -> Result<(), EngineError> {
        let mut controls = self.effects.units[unit.index()].clone();
        change(&mut controls);
        self.send(RtCommand::FxValues {
            unit: unit.index(),
            values: controls.values,
        })?;
        self.effects.units[unit.index()] = controls;
        Ok(())
    }

    /// Puts `effect` in a place of a unit, built for every channel the unit is assigned to.
    fn set_place(
        &mut self,
        unit: FxUnit,
        place: Place,
        effect: Option<EffectKind>,
    ) -> Result<(), EngineError> {
        let decks: Vec<DeckId> = self.effects.decks_of(unit).collect();
        self.ensure_swap_room(decks.len() + 1)?;
        for deck in decks {
            self.send_effect_swap(RtCommand::FxPlace {
                deck,
                unit: unit.index(),
                place,
                effect: effect.map(|kind| self.build_effect(kind)),
            })?;
        }
        // The send chain holds the effects of the unit whatever its routing.
        self.send_effect_swap(RtCommand::FxSendPlace {
            unit: unit.index(),
            place,
            effect: effect.map(|kind| self.build_effect(kind)),
        })?;
        let keep = self.effects.keep_values;
        self.update_unit(unit, |controls| match place {
            Place::Group(slot) => {
                if let Some(kind) = controls.slots.get_mut(slot) {
                    *kind = effect;
                }
                if let Some(values) = controls.values.slots.get_mut(slot).filter(|_| !keep) {
                    values.amount = DEFAULT_AMOUNT;
                }
            }
            Place::Single => {
                controls.single = effect;
                if !keep {
                    controls.values.single.controls =
                        effect.map_or_else(Controls::default, |kind| controls.start_of(kind));
                }
            }
        })
    }

    /// Puts a channel through a unit, with the unit's effects built for it, or takes it out.
    fn toggle_assign(&mut self, deck: DeckId, unit: FxUnit) -> Result<(), EngineError> {
        let on = !self.effects.is_assigned(deck, unit);
        let places: Vec<(Place, EffectKind)> = self.effects.unit(unit).places().collect();
        self.ensure_swap_room(places.len())?;
        self.send(RtCommand::FxAssign {
            deck,
            unit: unit.index(),
            on,
        })?;
        for (place, kind) in places {
            self.send_effect_swap(RtCommand::FxPlace {
                deck,
                unit: unit.index(),
                place,
                effect: on.then(|| self.build_effect(kind)),
            })?;
        }
        self.effects.assigned[deck.index()][unit.index()] = on;
        Ok(())
    }

    /// Puts the effect units and the mixer effects as `effects` has them, as the settings
    /// file kept them.
    pub fn restore_effects(&mut self, effects: &EffectControls) -> Result<(), EngineError> {
        self.replay_effects(effects)
    }

    /// Sends the whole state of the effects to the processor, building every instance it
    /// needs: at the start, and after a restart, which loses them.
    fn replay_effects(&mut self, effects: &EffectControls) -> Result<(), EngineError> {
        self.effects = effects.clone();
        for unit in FxUnit::ALL {
            let controls = self.effects.unit(unit).clone();
            self.send(RtCommand::FxValues {
                unit: unit.index(),
                values: controls.values,
            })?;
            self.send(RtCommand::FxRouting {
                unit: unit.index(),
                routing: controls.routing,
            })?;
            for (place, kind) in controls.places() {
                self.send_effect_swap(RtCommand::FxSendPlace {
                    unit: unit.index(),
                    place,
                    effect: Some(self.build_effect(kind)),
                })?;
            }
            for deck in DeckId::ALL {
                if !self.effects.is_assigned(deck, unit) {
                    continue;
                }
                self.send(RtCommand::FxAssign {
                    deck,
                    unit: unit.index(),
                    on: true,
                })?;
                for (place, kind) in controls.places() {
                    self.send_effect_swap(RtCommand::FxPlace {
                        deck,
                        unit: unit.index(),
                        place,
                        effect: Some(self.build_effect(kind)),
                    })?;
                }
            }
        }
        for deck in DeckId::ALL {
            let mixer = self.effects.mixer[deck.index()];
            self.send_effect_swap(RtCommand::MixerEffectPlace {
                deck,
                effect: Some(self.build_effect(mixer.effect)),
            })?;
            self.send(RtCommand::MixerEffectAmount {
                deck,
                amount: mixer.amount as f32,
            })?;
        }
        Ok(())
    }

    /// Builds an effect for the rate of the engine, with its buffers, on this thread.
    fn build_effect(&self, kind: EffectKind) -> Box<Effect> {
        Box::new(Effect::new(kind, self.rate))
    }

    /// Fails at once when `count` more swaps would not fit, so a command is sent whole or not
    /// at all.
    fn ensure_swap_room(&mut self, count: usize) -> Result<(), EngineError> {
        self.collect_retired();
        if self.swaps_in_flight + count > COMMAND_CAPACITY {
            Err(EngineError::Busy)
        } else {
            Ok(())
        }
    }

    fn send_effect_swap(&mut self, command: RtCommand) -> Result<(), EngineError> {
        self.ensure_swap_room(1)?;
        self.send(command)?;
        self.swaps_in_flight += 1;
        Ok(())
    }

    fn start_load(&mut self, deck: DeckId, path: PathBuf, stems: Stems) -> Result<(), EngineError> {
        let state = &mut self.decks[deck.index()];
        state.generation += 1;
        let generation = state.generation;
        let reports = self.load_reports.clone();
        let analyser = Arc::clone(&self.analyser);
        let recall = self.recall.clone();
        let stem_recall = self.stem_recall.clone();
        let pool = self.stretch_pool.clone();
        let rate = self.rate;
        std::thread::Builder::new()
            .name("platter-load".to_owned())
            .spawn(move || {
                let result = decode_file(&path).map(|decoded| {
                    let memory = recall
                        .as_ref()
                        .and_then(|recall| recall(&path))
                        .unwrap_or_default();
                    let stems = match stems {
                        Stems::Linked => stem_recall.as_ref().and_then(|recall| recall(&path)),
                        Stems::None => None,
                        Stems::Given(paths) => Some(*paths),
                    };
                    let pool = pool.as_ref();
                    let mut loaded = prepare(decoded, path, rate, memory, pool);
                    if let Some(paths) = &stems {
                        match prepare_stems(&loaded.audio, paths, rate, pool) {
                            Ok(stems) => loaded.stems = Some(stems),
                            Err(message) => loaded.stems_failed = Some(message),
                        }
                    }
                    Box::new(loaded)
                });
                let audio = result.as_ref().ok().map(|loaded| loaded.audio.clone());
                // The engine may be gone by now; then nobody waits for the result.
                let _ = reports.send(LoadResult::Decoded {
                    deck,
                    generation,
                    result,
                });
                // The deck plays the track while it is analysed.
                if let Some(audio) = audio {
                    let _ = reports.send(LoadResult::Analysed {
                        deck,
                        generation,
                        analysis: analyser.analyse(audio.samples(), audio.rate()),
                    });
                }
            })
            .map_err(EngineError::Worker)?;
        self.decks[deck.index()].loading = true;
        self.loads_running += 1;
        Ok(())
    }

    /// Decodes `path` for the preview player on a worker thread of its own.
    fn start_preview(&mut self, path: PathBuf) -> Result<(), EngineError> {
        self.preview.generation += 1;
        let generation = self.preview.generation;
        let reports = self.load_reports.clone();
        std::thread::Builder::new()
            .name("platter-preview".to_owned())
            .spawn(move || {
                let result = decode_file(&path).map(|decoded| {
                    let info = TrackInfo {
                        path,
                        container: decoded.container,
                        sample_rate: decoded.sample_rate,
                        channels: decoded.channels,
                        seconds: decoded.seconds(),
                        tags: decoded.tags.clone(),
                    };
                    let rate = decoded.sample_rate;
                    let stereo = decoded.into_stereo();
                    let peaks = WaveformPeaks::compute(&stereo, rate);
                    Box::new(PreviewLoaded {
                        audio: TrackAudio::new(rate, stereo),
                        info,
                        peaks,
                    })
                });
                // The engine may be gone by now; then nobody waits for the result.
                let _ = reports.send(LoadResult::Preview { generation, result });
            })
            .map_err(EngineError::Worker)?;
        self.preview.loading = true;
        self.loads_running += 1;
        Ok(())
    }

    fn finish_preview(
        &mut self,
        generation: u64,
        result: Result<Box<PreviewLoaded>, DecodeError>,
    ) -> Option<Event> {
        self.loads_running = self.loads_running.saturating_sub(1);
        if self.preview.generation != generation {
            return None;
        }
        self.preview.loading = false;
        let loaded = match result {
            Ok(loaded) => *loaded,
            Err(error) => {
                return Some(Event::PreviewFailed {
                    message: error.to_string(),
                });
            }
        };
        self.collect_retired();
        // The retired queue has room for this many; see `rt::channel`.
        if self.swaps_in_flight >= COMMAND_CAPACITY {
            return Some(Event::PreviewFailed {
                message: EngineError::Busy.to_string(),
            });
        }
        let track = DeckTrack {
            audio: loaded.audio,
            stretch: None,
            memory: None,
            start: None,
            stems: None,
        };
        if let Err(error) = self.send(RtCommand::PreviewSwap {
            track: Some(track),
            play: true,
        }) {
            return Some(Event::PreviewFailed {
                message: error.to_string(),
            });
        }
        self.swaps_in_flight += 1;
        self.preview.track = Some(loaded.info);
        self.preview.peaks = Some(Arc::new(loaded.peaks));
        None
    }

    fn eject(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_swap(deck, None)?;
        let state = &mut self.decks[deck.index()];
        state.generation += 1;
        state.loading = false;
        state.track = None;
        state.audio = None;
        state.peaks = None;
        state.stem_peaks = None;
        state.held = None;
        state.forget_analysis();
        Ok(())
    }

    /// Takes in the loads that finished, without waiting. Call it regularly: it is also
    /// what frees the audio of replaced tracks.
    pub fn poll(&mut self) -> Vec<Event> {
        self.collect_retired();
        self.send_pending_analyses();
        self.take_reports();
        self.follow_fade_markers();
        let mut events = Vec::new();
        while let Ok(finished) = self.finished_loads.try_recv() {
            events.extend(self.finish_load(finished));
        }
        events.append(&mut self.later_events);
        let dropped = self.ends.shared.recorder_dropped.get();
        self.recorder
            .poll(&mut self.ends.commands, dropped, &mut events);
        events
    }

    /// Takes in what the processor reported of the grids, loops and cues of the decks.
    fn take_reports(&mut self) {
        while let Ok(report) = self.ends.reports.pop() {
            let deck = &mut self.decks[report.deck.index()];
            if let Some(grid) = report.grid {
                deck.grid = grid;
            }
            if let Some(looping) = report.looping {
                deck.looping = looping;
            }
            if let Some(cues) = report.cues {
                deck.cues = cues;
            }
        }
    }

    /// Starts the partner of a playing deck from its fade-in cue when the deck crosses one of
    /// its fade-out cues, while the preference asks for it: `docs/behavior/auto-mix.md`.
    fn follow_fade_markers(&mut self) {
        for deck in DeckId::ALL {
            let published = &self.ends.shared.decks[deck.index()];
            let (now, playing) = (published.position.get(), published.playing.get());
            let before = self.marker_positions[deck.index()];
            self.marker_positions[deck.index()] = (playing && now.is_finite()).then_some(now);
            let Some(before) = before.filter(|_| self.fade_markers && playing) else {
                continue;
            };
            // A jump forward is not playback crossing the cue.
            if !(now > before && now - before <= MAX_MARKER_STEP_SECONDS) {
                continue;
            }
            let crossed = self.decks[deck.index()].cues.iter().any(|(_, cue)| {
                cue.kind() == CueKind::FadeOut && cue.position > before && cue.position <= now
            });
            // With the queue full, the crossing is seen again at the next poll.
            if crossed && self.start_partner(deck).is_err() {
                self.marker_positions[deck.index()] = Some(before);
            }
        }
    }

    /// Plays the partner of `deck` from its first fade-in cue, if it holds a stopped track
    /// with one, and fades the crossfader towards it when the two are on opposite sides.
    fn start_partner(&mut self, deck: DeckId) -> Result<(), EngineError> {
        let partner = fade_partner(deck);
        let state = &self.decks[partner.index()];
        if state.track.is_none()
            || state.loading
            || self.ends.shared.decks[partner.index()].playing.get()
        {
            return Ok(());
        }
        let Some(start) = state
            .cues
            .iter()
            .find(|(_, cue)| cue.kind() == CueKind::FadeIn)
            .map(|(_, cue)| cue.position)
        else {
            return Ok(());
        };
        self.dispatch(Command::DeckSeek {
            deck: partner,
            seconds: start,
        })?;
        self.dispatch(Command::DeckPlay { deck: partner })?;
        let sides = self.mixer.crossfader_sides;
        let direction = match sides[partner.index()] {
            CrossfaderSide::Left => Some(CrossfaderDirection::Left),
            CrossfaderSide::Right => Some(CrossfaderDirection::Right),
            CrossfaderSide::Thru => None,
        };
        match direction {
            Some(direction) if sides[deck.index()] != sides[partner.index()] => {
                self.dispatch(Command::MixerCrossfaderAutoFade { direction })
            }
            _ => Ok(()),
        }
    }

    /// Waits until no load is running. The deviceless render uses it so that a script gives
    /// the same result on every run.
    pub fn wait_for_loads(&mut self) -> Vec<Event> {
        let mut events = self.poll();
        while self.loads_running > 0 {
            // The engine holds a sender, so the channel cannot close while it waits.
            let Ok(finished) = self.finished_loads.recv() else {
                break;
            };
            events.extend(self.finish_load(finished));
        }
        events.append(&mut self.later_events);
        events
    }

    fn finish_load(&mut self, finished: LoadResult) -> Option<Event> {
        let (deck, generation, result) = match finished {
            LoadResult::Preview { generation, result } => {
                return self.finish_preview(generation, result);
            }
            LoadResult::Decoded {
                deck,
                generation,
                result,
            } => (deck, generation, result),
            LoadResult::Analysed {
                deck,
                generation,
                analysis,
            } => {
                self.finish_analysis(deck, generation, analysis);
                return None;
            }
            LoadResult::Remix {
                deck,
                at,
                generation,
                result,
            } => return self.finish_remix(deck, at, generation, result),
        };
        if result.is_err() {
            self.loads_running = self.loads_running.saturating_sub(1);
        }
        if self.decks[deck.index()].generation != generation {
            return None;
        }
        self.decks[deck.index()].loading = false;
        let failed = |message: String| Some(Event::LoadFailed { deck, message });
        let loaded = match result {
            Ok(loaded) => *loaded,
            Err(error) => return failed(error.to_string()),
        };
        // A track with a cue of the type load starts there, while the preference asks for it.
        let start = self
            .load_marker
            .then(|| {
                loaded
                    .memory
                    .cues
                    .iter()
                    .find(|(_, cue)| cue.kind() == CueKind::Load)
                    .map(|(_, cue)| cue.position)
            })
            .flatten();
        let (stems, stem_peaks) = loaded
            .stems
            .map_or((None, None), |(stems, peaks)| (Some(stems), Some(peaks)));
        let kept = loaded.audio.clone();
        let track = DeckTrack {
            audio: loaded.audio,
            stretch: loaded.stretch,
            memory: Some(Box::new(loaded.memory)),
            start,
            stems,
        };
        if let Err(error) = self.send_swap(deck, Some(track)) {
            return failed(error.to_string());
        }
        // A new track starts with its stems as they sound together. These follow the swap
        // in the queue; one refused leaves that stem as it was.
        for stem in Stem::ALL {
            let settings = StemSettings::default();
            if self
                .send(RtCommand::Stem {
                    deck,
                    stem,
                    settings,
                })
                .is_ok()
            {
                self.decks[deck.index()].controls.stems[stem.index()] = settings;
            }
        }
        let state = &mut self.decks[deck.index()];
        state.held = Some(generation);
        state.audio = Some(kept);
        state.track = Some(loaded.info.clone());
        state.peaks = Some(Arc::new(loaded.peaks));
        state.stem_peaks = stem_peaks.map(Arc::new);
        state.forget_analysis();
        state.analysing = true;
        if let Some(message) = loaded.stems_failed {
            self.later_events.push(Event::StemsFailed { deck, message });
        }
        Some(Event::Loaded {
            deck,
            info: loaded.info,
        })
    }

    fn finish_analysis(&mut self, deck: DeckId, generation: u64, analysis: TrackAnalysis) {
        self.loads_running = self.loads_running.saturating_sub(1);
        let state = &mut self.decks[deck.index()];
        if state.held != Some(generation) {
            return;
        }
        state.analysing = false;
        state.analysis = Some(analysis);
        state.pending_analysis = analysis.tempo.map(|tempo| GridEdit::Analysed {
            bpm: tempo.bpm,
            first_beat: tempo.first_beat,
        });
        if self.mixer.autogain {
            state.pending_gain = analysis.autogain_db();
        }
        self.send_pending_analyses();
    }

    /// Sends the results of the analyses to the processor, as far as its queue has room; the
    /// rest wait for the next poll.
    fn send_pending_analyses(&mut self) {
        for deck in DeckId::ALL {
            if let Some(edit) = self.decks[deck.index()].pending_analysis
                && self.send(RtCommand::Grid { deck, edit }).is_ok()
            {
                self.decks[deck.index()].pending_analysis = None;
            }
            if let Some(db) = self.decks[deck.index()].pending_gain
                && self.set_gain(deck, db).is_ok()
            {
                self.decks[deck.index()].pending_gain = None;
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let shared = &self.ends.shared;
        let deck = |id: DeckId| {
            let (control, published) = (&self.decks[id.index()], &shared.decks[id.index()]);
            DeckSnapshot {
                track: control.track.clone(),
                peaks: control.peaks.clone(),
                stem_peaks: control.stem_peaks.clone(),
                loading: control.loading,
                analysing: control.analysing,
                analysis: control.analysis,
                autogain_db: control.analysis.and_then(|analysis| analysis.autogain_db()),
                playing: published.playing.get(),
                position_seconds: published.position.get(),
                cue_seconds: published.cue.get(),
                grid: Box::new(control.grid),
                looping: control.looping,
                cues: control.cues,
                controls: control.controls.clone(),
                levels: [published.levels[0].get(), published.levels[1].get()],
                sync: published.sync.get(),
                key_lock: published.key_lock.get(),
                beat_tick: published.beat_tick.get(),
                key_shift: published.key_shift.get(),
                resulting_key: control
                    .analysis
                    .and_then(|analysis| analysis.key)
                    .and_then(|key| key.transposed(published.pitch.get())),
                flux: published.flux.get(),
                reverse: published.reverse.get(),
                timeline_seconds: Some(published.timeline.get())
                    .filter(|seconds| seconds.is_finite()),
                speed: published.speed.get(),
                phase: Some(published.phase.get()).filter(|phase| phase.is_finite()),
                remix: {
                    let mut remix = control.remix.shown.clone();
                    let cell = |code: u64| usize::try_from(code).ok()?.checked_sub(1);
                    for slot in 0..rt::SLOTS {
                        remix.playing[slot] = cell(published.remix_playing[slot].get());
                        remix.waiting[slot] = cell(published.remix_waiting[slot].get());
                    }
                    Box::new(remix)
                },
            }
        };
        Snapshot {
            decks: DeckId::ALL.map(deck),
            mixer: MixerControls {
                crossfader: self.crossfader(),
                ..self.mixer.clone()
            },
            effects: self.effects.clone(),
            clock: ClockSnapshot {
                auto: self.clock_auto,
                sync_mode: self.sync_mode,
                master: shared.master.get(),
                bpm: shared.clock_bpm.get(),
                snap: self.snap,
                quantize: self.quantize,
                metronome: self.metronome,
                midi_clock: self.midi_clock,
                midi_clock_running: self.ends.shared.midi_clock.get(),
                external: self.external_clock,
            },
            master_levels: [shared.master_levels[0].get(), shared.master_levels[1].get()],
            limiter_gain: shared.limiter_gain.get(),
            maximiser_gain: shared.maximiser_gain.get(),
            recorder: self.recorder.snapshot(shared, self.rate),
            loop_recorder: LoopRecorderSnapshot {
                phase: LoopPhase::from_code(u8::try_from(shared.loop_phase.get()).unwrap_or(0)),
                bars: self.loop_recorder.bars,
                fitting_bars: u32::try_from(shared.loop_bars.get())
                    .unwrap_or(self.loop_recorder.bars),
                source: self.loop_recorder.source,
                dry_wet: self.loop_recorder.dry_wet,
                decay: self.loop_recorder.decay,
                progress: shared.loop_progress.get(),
                seconds: shared.loop_seconds.get(),
                can_undo: shared.loop_can_undo.get(),
                can_redo: shared.loop_can_redo.get(),
            },
            input: InputSnapshot {
                levels: shared.input_levels.each_ref().map(|level| level.get()),
                gaps: shared.input_gaps.get(),
                reading: shared.input_reading.get(),
                ratio: shared.input_ratio.get(),
            },
            load_marker: self.load_marker,
            fade_markers: self.fade_markers,
            preview: PreviewSnapshot {
                track: self.preview.track.clone(),
                peaks: self.preview.peaks.clone(),
                loading: self.preview.loading,
                playing: shared.preview_playing.get(),
                position_seconds: shared.preview_position.get(),
            },
        }
    }
}

impl DeckControls {
    /// What the processor needs to stand where these controls stand.
    fn as_commands(&self, deck: DeckId) -> Vec<RtCommand> {
        let mut commands = vec![
            RtCommand::Tempo {
                deck,
                ratio: self.tempo_ratio as f32,
            },
            RtCommand::Gain {
                deck,
                linear: Gain::from_db(self.gain_db).map_or(1.0, Gain::linear),
            },
            RtCommand::Filter {
                deck,
                position: self.filter as f32,
            },
            RtCommand::Balance {
                deck,
                position: self.pan as f32,
            },
            RtCommand::Fader {
                deck,
                gain: fader_gain(self.fader).unwrap_or(1.0),
            },
            RtCommand::PreListen {
                deck,
                on: self.pre_listen,
            },
        ];
        for band in [Band::Low, Band::Mid, Band::High] {
            commands.push(RtCommand::Eq {
                deck,
                band,
                gain: band_gain(self.eq[band as usize] as f32),
            });
            commands.push(RtCommand::EqCut {
                deck,
                band,
                cut: self.eq_kill[band as usize],
            });
        }
        for stem in Stem::ALL {
            commands.push(RtCommand::Stem {
                deck,
                stem,
                settings: self.stems[stem.index()],
            });
        }
        commands.push(RtCommand::DeckType {
            deck,
            kind: self.kind,
        });
        commands
    }
}

impl MixerControls {
    fn external(&self) -> RtCommand {
        RtCommand::ExternalMixer {
            on: self.mode == MixerMode::External,
            eq_gain: self.external_eq_gain,
            master_volume: self.external_master_volume,
        }
    }

    fn as_commands(&self) -> impl Iterator<Item = RtCommand> {
        let linear = |db: f64| Gain::from_db(db).map_or(1.0, Gain::linear);
        let assigns = DeckId::ALL.map(|deck| RtCommand::CrossfaderAssign {
            deck,
            side: self.crossfader_sides[deck.index()],
        });
        let rest = [
            RtCommand::Crossfader {
                position: self.crossfader as f32,
            },
            RtCommand::CrossfaderCurve {
                curve: self.crossfader_curve,
            },
            RtCommand::EqModel {
                model: self.eq_model,
            },
            RtCommand::FilterModel {
                model: self.filter_model,
            },
            RtCommand::MasterGain {
                linear: linear(self.master_gain_db),
            },
            RtCommand::HeadphoneMix {
                position: self.headphone_mix as f32,
            },
            RtCommand::HeadphoneGain {
                linear: linear(self.headphone_gain_db),
            },
            self.maximiser.command(),
            self.external(),
            RtCommand::AuxLevel {
                level: self.aux_level as f32,
            },
        ];
        let routes = InputTarget::ALL.map(|target| RtCommand::InputRoute {
            target,
            source: self.input_routes[target.index()],
        });
        assigns.into_iter().chain(rest).chain(routes)
    }
}

fn gain_up_to(db: f64, max_db: f64) -> Result<f32, ValueError> {
    if db.is_finite() && db > max_db {
        return Err(ValueError::OutOfRange {
            name: "gain",
            min: Gain::MIN_DB,
            max: max_db,
        });
    }
    Gain::from_db(db).map(Gain::linear)
}

/// Gain of a fader position. A squared taper around [`FADER_UNITY`]: silence at the bottom,
/// no change at three quarters of the travel and about 5 dB of gain at the top.
fn fader_gain(position: f64) -> Result<f32, ValueError> {
    let travel = f64::from(Gain::from_fraction(position)?.linear()) / FADER_UNITY;
    Ok((travel * travel) as f32)
}

impl DeckControl {
    fn forget_analysis(&mut self) {
        self.analysing = false;
        self.analysis = None;
        self.pending_analysis = None;
        self.pending_gain = None;
    }
}

fn prepare(
    decoded: DecodedAudio,
    path: PathBuf,
    engine_rate: SampleRate,
    memory: TrackMemory,
    pool: Option<&Arc<StretchPool>>,
) -> Loaded {
    let info = TrackInfo {
        path,
        container: decoded.container,
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        seconds: decoded.seconds(),
        tags: decoded.tags.clone(),
    };
    let rate = decoded.sample_rate;
    let stereo = decoded.into_stereo();
    let peaks = WaveformPeaks::compute(&stereo, rate);
    let audio = TrackAudio::new(rate, stereo);
    // A track the stretch does not take plays without key lock.
    let stretch = TrackStretch::new(audio.frames(), rate, engine_rate)
        .map(|stretch| Stretcher(on_worker(Box::new(stretch), &audio, pool)));
    Loaded {
        audio,
        memory,
        stretch,
        info,
        peaks,
        stems: None,
        stems_failed: None,
    }
}

/// The stretch of `audio` on a worker of `pool`, or as it is without one.
fn on_worker(
    stretch: Box<dyn Stretch>,
    audio: &TrackAudio,
    pool: Option<&Arc<StretchPool>>,
) -> Box<dyn Stretch> {
    match pool {
        Some(pool) => pool.adopt(stretch, audio),
        None => stretch,
    }
}

/// Decodes the four stems of `track` and builds what the deck plays them with. A stem at
/// another rate is brought to that of the track; a shorter one is completed with silence and a
/// longer one cut at the length of the track.
fn prepare_stems(
    track: &TrackAudio,
    paths: &[PathBuf; Stem::COUNT],
    engine_rate: SampleRate,
    pool: Option<&Arc<StretchPool>>,
) -> Result<(Box<DeckStems>, [WaveformPeaks; Stem::COUNT]), String> {
    let mut audio = Vec::with_capacity(Stem::COUNT);
    let mut peaks = Vec::with_capacity(Stem::COUNT);
    let mut stretch = Vec::with_capacity(Stem::COUNT);
    for (stem, path) in Stem::ALL.into_iter().zip(paths) {
        let decoded = decode_file(path).map_err(|error| format!("{stem}: {error}"))?;
        let from = decoded.sample_rate.hz();
        let mut stereo =
            crate::separation::resample(&decoded.into_stereo(), from, track.rate().hz());
        stereo.resize(track.frames() * 2, 0.0);
        peaks.push(WaveformPeaks::compute(&stereo, track.rate()));
        let part = TrackAudio::new(track.rate(), stereo);
        stretch.push(
            TrackStretch::new(part.frames(), part.rate(), engine_rate)
                .map(|stretch| Stretcher(on_worker(Box::new(stretch), &part, pool))),
        );
        audio.push(part);
    }
    let missing = || "the stems are incomplete".to_owned();
    let stems = DeckStems {
        audio: audio.try_into().map_err(|_| missing())?,
        stretch: stretch.try_into().map_err(|_| missing())?,
    };
    let peaks = peaks.try_into().map_err(|_| missing())?;
    Ok((Box::new(stems), peaks))
}

/// The longest step between two polls that still counts as playback crossing a fade-out cue.
const MAX_MARKER_STEP_SECONDS: f64 = 1.0;

/// The deck a fade-out cue starts: A and B start each other, and so do C and D.
fn fade_partner(deck: DeckId) -> DeckId {
    match deck {
        DeckId::A => DeckId::B,
        DeckId::B => DeckId::A,
        DeckId::C => DeckId::D,
        DeckId::D => DeckId::C,
    }
}
