//! The control side of the engine. It validates commands, loads tracks on worker threads,
//! feeds the processor through its queue and publishes the state snapshot.
//!
//! It runs on a normal thread and may allocate, block on a channel and touch the disk; the
//! processor it feeds may not.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use platter_analysis::Transposed;
use platter_core::{
    BeatGrid, Bpm, CueKind, CueName, Cues, DeckId, Gain, LoopSize, SampleRate, Tempo, TrackMemory,
    ValueError,
};
use platter_decode::{Container, DecodeError, DecodedAudio, Tags, decode_file};
use platter_dsp::crossfade::{CrossfaderCurve, CrossfaderSide};
use platter_dsp::eq::{Band, band_gain};

use crate::analyser::{Analyser, PlatterAnalyser, TrackAnalysis};
use crate::command::{
    CROSSFADER_STEP, ClockMode, Command, CueStep, Direction, MAX_BPM_STEP, MAX_CHANNEL_GAIN_DB,
    MAX_FADE_BEATS, MAX_HEADPHONE_GAIN_DB, MAX_KEY_SHIFT, MAX_MARKER_MOVE_SECONDS,
    MAX_MASTER_GAIN_DB, MAX_NUDGE, MAX_SEEK_SECONDS, MIN_FADE_BEATS, MixerMode, MoveMode, MoveSize,
    SizeStep, SyncMode,
};
use crate::peaks::WaveformPeaks;
use crate::rt::{
    self, COMMAND_CAPACITY, CROSSFADER_SIDES, ControlEnds, CueEdit, DeckTrack, GridEdit, LoopEdit,
    LoopPart, LoopState, Processor, RtCommand, Span, Stretcher, TrackAudio,
};
use crate::stretch::TrackStretch;
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
    /// The file sent to the preview player could not be read.
    PreviewFailed {
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
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => error.fmt(f),
            Self::Busy => f.write_str("the engine is busy; the command was not applied"),
            Self::Worker(error) => write!(f, "cannot start a worker thread: {error}"),
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
        }
    }
}

/// Where the controls shared by both decks stand.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerControls {
    pub crossfader: f64,
    pub crossfader_curve: CrossfaderCurve,
    /// The side of the crossfader each channel is on, deck A first.
    pub crossfader_sides: [CrossfaderSide; DeckId::COUNT],
    /// Beats of the master clock an automatic fade across the whole travel takes.
    pub fade_beats: f64,
    pub master_gain_db: f64,
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
}

impl Default for MixerControls {
    fn default() -> Self {
        Self {
            crossfader: 0.5,
            crossfader_curve: CrossfaderCurve::default(),
            crossfader_sides: CROSSFADER_SIDES,
            fade_beats: 8.0,
            master_gain_db: 0.0,
            headphone_mix: 0.0,
            headphone_gain_db: 0.0,
            autogain: true,
            mode: MixerMode::Internal,
            external_eq_gain: false,
            external_master_volume: false,
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
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeckSnapshot {
    pub track: Option<TrackInfo>,
    /// Waveform data of the loaded track.
    pub peaks: Option<Arc<WaveformPeaks>>,
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
    pub grid: BeatGrid,
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
}

impl Default for DeckSnapshot {
    fn default() -> Self {
        Self {
            track: None,
            peaks: None,
            loading: false,
            analysing: false,
            analysis: None,
            autogain_db: None,
            playing: false,
            position_seconds: 0.0,
            cue_seconds: 0.0,
            grid: BeatGrid::default(),
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
        }
    }
}

/// State of the engine at one moment, for the interface to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub decks: [DeckSnapshot; DeckId::COUNT],
    /// A track with a cue of the type load starts at it: `docs/behavior/cues.md`.
    pub load_marker: bool,
    pub preview: PreviewSnapshot,
    pub mixer: MixerControls,
    pub clock: ClockSnapshot,
    /// Peak level of the left and the right side of the master output, linear.
    pub master_levels: [f32; 2],
    /// Gain the safety limiter is applying; below one while it works.
    pub limiter_gain: f32,
}

#[derive(Default)]
struct DeckControl {
    track: Option<TrackInfo>,
    peaks: Option<Arc<WaveformPeaks>>,
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
}

struct Loaded {
    audio: TrackAudio,
    /// What the collection kept of the track.
    memory: TrackMemory,
    /// The stretch of key lock, built on the load worker since building it allocates.
    stretch: Option<Stretcher>,
    info: TrackInfo,
    peaks: WaveformPeaks,
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
    /// The tempo master is chosen automatically.
    clock_auto: bool,
    sync_mode: SyncMode,
    snap: bool,
    quantize: bool,
    metronome: bool,
    /// A track with a cue of the type load starts at it.
    load_marker: bool,
    clock_tap: TapTempo,
    /// An automatic fade was started and nothing has set the crossfader since: the
    /// processor knows where it is.
    crossfading: bool,
    /// Swaps sent whose retired item has not come back yet.
    swaps_in_flight: usize,
    /// Load workers that have not sent their last report.
    loads_running: usize,
    finished_loads: Receiver<LoadResult>,
    load_reports: Sender<LoadResult>,
    analyser: Arc<dyn Analyser>,
    recall: Option<Recall>,
    rate: SampleRate,
}

/// Gives the grid and the cues kept for the file at a path. It runs on the worker that
/// loads the track, so it may read a file.
pub type Recall = Arc<dyn Fn(&Path) -> Option<TrackMemory> + Send + Sync>;

impl Engine {
    /// Builds an engine that runs at `rate` and the processor that produces its audio. The
    /// processor goes to whoever drives the audio: the device callback or the render.
    pub fn new(rate: SampleRate) -> (Self, Processor) {
        Self::with_analyser(rate, Arc::new(PlatterAnalyser::default()))
    }

    /// Sets where each load finds the grid and the cues kept for its track.
    pub fn set_recall(&mut self, recall: Recall) {
        self.recall = Some(recall);
    }

    /// As [`Self::new`], with another analysis of the loaded tracks.
    pub fn with_analyser(rate: SampleRate, analyser: Arc<dyn Analyser>) -> (Self, Processor) {
        let (ends, processor) = rt::channel(rate);
        let (load_reports, finished_loads) = channel();
        let engine = Self {
            ends,
            decks: Default::default(),
            preview: PreviewControl::default(),
            mixer: MixerControls::default(),
            clock_auto: true,
            sync_mode: SyncMode::default(),
            snap: false,
            quantize: false,
            metronome: false,
            load_marker: true,
            clock_tap: TapTempo::default(),
            crossfading: false,
            swaps_in_flight: 0,
            loads_running: 0,
            finished_loads,
            load_reports,
            analyser,
            recall: None,
            rate,
        };
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
        engine.load_marker = previous.load_marker;
        (engine, processor)
    }

    /// Validates a command and applies it. An invalid command changes nothing.
    pub fn dispatch(&mut self, command: Command) -> Result<(), EngineError> {
        match command {
            Command::DeckLoad { deck, path } => self.start_load(deck, path),
            Command::GlobalLoadMarkerToggle => {
                self.load_marker = !self.load_marker;
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
            Command::ClockMetronomeToggle => {
                let on = !self.metronome;
                self.send(RtCommand::Metronome { on })?;
                self.metronome = on;
                return Ok(());
            }
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

    /// The commands of the controls both decks share.
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
            // Every other command is handled before this point.
            _ => {}
        }
        Ok(())
    }

    /// Sends the external mode and its options of `mixer`, and keeps them once sent.
    fn set_external(&mut self, mixer: MixerControls) -> Result<(), EngineError> {
        self.send(mixer.external())?;
        self.mixer = mixer;
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

    fn start_load(&mut self, deck: DeckId, path: PathBuf) -> Result<(), EngineError> {
        let state = &mut self.decks[deck.index()];
        state.generation += 1;
        let generation = state.generation;
        let reports = self.load_reports.clone();
        let analyser = Arc::clone(&self.analyser);
        let recall = self.recall.clone();
        let rate = self.rate;
        std::thread::Builder::new()
            .name("platter-load".to_owned())
            .spawn(move || {
                let result = decode_file(&path).map(|decoded| {
                    let memory = recall
                        .as_ref()
                        .and_then(|recall| recall(&path))
                        .unwrap_or_default();
                    Box::new(prepare(decoded, path, rate, memory))
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
        state.peaks = None;
        state.held = None;
        state.forget_analysis();
        Ok(())
    }

    /// Takes in the loads that finished, without waiting. Call it regularly: it is also
    /// what frees the audio of replaced tracks.
    pub fn poll(&mut self) -> Vec<Event> {
        self.collect_retired();
        self.send_pending_analyses();
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
        let mut events = Vec::new();
        while let Ok(finished) = self.finished_loads.try_recv() {
            events.extend(self.finish_load(finished));
        }
        events
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
        let track = DeckTrack {
            audio: loaded.audio,
            stretch: loaded.stretch,
            memory: Some(Box::new(loaded.memory)),
            start,
        };
        if let Err(error) = self.send_swap(deck, Some(track)) {
            return failed(error.to_string());
        }
        let state = &mut self.decks[deck.index()];
        state.held = Some(generation);
        state.track = Some(loaded.info.clone());
        state.peaks = Some(Arc::new(loaded.peaks));
        state.forget_analysis();
        state.analysing = true;
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
                loading: control.loading,
                analysing: control.analysing,
                analysis: control.analysis,
                autogain_db: control.analysis.and_then(|analysis| analysis.autogain_db()),
                playing: published.playing.get(),
                position_seconds: published.position.get(),
                cue_seconds: published.cue.get(),
                grid: control.grid,
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
            }
        };
        Snapshot {
            decks: DeckId::ALL.map(deck),
            mixer: MixerControls {
                crossfader: self.crossfader(),
                ..self.mixer.clone()
            },
            clock: ClockSnapshot {
                auto: self.clock_auto,
                sync_mode: self.sync_mode,
                master: shared.master.get(),
                bpm: shared.clock_bpm.get(),
                snap: self.snap,
                quantize: self.quantize,
                metronome: self.metronome,
            },
            master_levels: [shared.master_levels[0].get(), shared.master_levels[1].get()],
            limiter_gain: shared.limiter_gain.get(),
            load_marker: self.load_marker,
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

    fn as_commands(&self) -> [RtCommand; 8] {
        let linear = |db: f64| Gain::from_db(db).map_or(1.0, Gain::linear);
        let assign = |deck: DeckId| RtCommand::CrossfaderAssign {
            deck,
            side: self.crossfader_sides[deck.index()],
        };
        [
            assign(DeckId::A),
            assign(DeckId::B),
            RtCommand::Crossfader {
                position: self.crossfader as f32,
            },
            RtCommand::CrossfaderCurve {
                curve: self.crossfader_curve,
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
            self.external(),
        ]
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
        .map(|stretch| Stretcher(Box::new(stretch)));
    Loaded {
        audio,
        memory,
        stretch,
        info,
        peaks,
    }
}
