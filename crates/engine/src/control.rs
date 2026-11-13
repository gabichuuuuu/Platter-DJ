//! The control side of the engine. It validates commands, loads tracks on worker threads,
//! feeds the processor through its queue and publishes the state snapshot.
//!
//! It runs on a normal thread and may allocate, block on a channel and touch the disk; the
//! processor it feeds may not.

use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use platter_core::{DeckId, Gain, SampleRate, Tempo, ValueError};
use platter_decode::{Container, DecodeError, DecodedAudio, Tags, decode_file};
use platter_dsp::eq::band_gain;

use crate::command::{
    Command, MAX_CHANNEL_GAIN_DB, MAX_MASTER_GAIN_DB, MAX_NUDGE, MAX_SEEK_SECONDS,
};
use crate::rt::{self, COMMAND_CAPACITY, ControlEnds, Processor, RtCommand, TrackAudio};

/// What the interface shows about a loaded track.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
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
    Loaded { deck: DeckId, info: TrackInfo },
    LoadFailed { deck: DeckId, message: String },
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeckSnapshot {
    pub track: Option<TrackInfo>,
    /// A file is being decoded for this deck.
    pub loading: bool,
    pub playing: bool,
    pub position_seconds: f64,
    pub cue_seconds: f64,
    /// Post-fader peak level, linear.
    pub level: f32,
}

/// State of the engine at one moment, for the interface to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub decks: [DeckSnapshot; 2],
    /// Peak level of the master output, linear.
    pub master_level: f32,
    /// Gain the safety limiter is applying; below one while it works.
    pub limiter_gain: f32,
}

#[derive(Default)]
struct DeckControl {
    track: Option<TrackInfo>,
    loading: bool,
    /// Counts loads and ejects, so a decode that finishes late is recognised as stale.
    generation: u64,
}

struct LoadResult {
    deck: DeckId,
    generation: u64,
    result: Result<(TrackAudio, TrackInfo), DecodeError>,
}

pub struct Engine {
    ends: ControlEnds,
    decks: [DeckControl; 2],
    /// Swaps sent whose retired item has not come back yet.
    swaps_in_flight: usize,
    loads_running: usize,
    finished_loads: Receiver<LoadResult>,
    load_reports: Sender<LoadResult>,
}

impl Engine {
    /// Builds an engine that runs at `rate` and the processor that produces its audio. The
    /// processor goes to whoever drives the audio: the device callback or the render.
    pub fn new(rate: SampleRate) -> (Self, Processor) {
        let (ends, processor) = rt::channel(rate);
        let (load_reports, finished_loads) = channel();
        let engine = Self {
            ends,
            decks: Default::default(),
            swaps_in_flight: 0,
            loads_running: 0,
            finished_loads,
            load_reports,
        };
        (engine, processor)
    }

    /// Validates a command and applies it. An invalid command changes nothing.
    pub fn dispatch(&mut self, command: Command) -> Result<(), EngineError> {
        match command {
            Command::DeckLoad { deck, path } => self.start_load(deck, path),
            Command::DeckEject { deck } => self.eject(deck),
            Command::DeckPlay { deck } => self.send(RtCommand::Play { deck }),
            Command::DeckPause { deck } => self.send(RtCommand::Pause { deck }),
            Command::DeckPlayPause { deck } => self.send(RtCommand::TogglePlay { deck }),
            Command::DeckCueSet { deck } => self.send(RtCommand::CueSet { deck }),
            Command::DeckCueGoto { deck } => self.send(RtCommand::CueGoto { deck }),
            Command::DeckSeek { deck, seconds } => {
                let seconds = ValueError::check("seconds", seconds, 0.0, MAX_SEEK_SECONDS)?;
                self.send(RtCommand::Seek { deck, seconds })
            }
            Command::DeckTempoSet { deck, ratio } => {
                let ratio = Tempo::from_ratio(ratio)?.ratio() as f32;
                self.send(RtCommand::Tempo { deck, ratio })
            }
            Command::DeckNudge { deck, amount } => {
                let amount = ValueError::check("amount", amount, -MAX_NUDGE, MAX_NUDGE)? as f32;
                self.send(RtCommand::Nudge { deck, amount })
            }
            mixer => self.dispatch_mixer(mixer),
        }
    }

    fn dispatch_mixer(&mut self, command: Command) -> Result<(), EngineError> {
        let command = match command {
            Command::MixerGainSet { deck, db } => RtCommand::Gain {
                deck,
                linear: gain_up_to(db, MAX_CHANNEL_GAIN_DB)?,
            },
            Command::MixerEqSet {
                deck,
                band,
                position,
            } => RtCommand::Eq {
                deck,
                band,
                gain: band_gain(ValueError::check("position", position, 0.0, 1.0)? as f32),
            },
            Command::MixerFilterSet { deck, position } => RtCommand::Filter {
                deck,
                position: ValueError::check("position", position, -1.0, 1.0)? as f32,
            },
            Command::MixerFaderSet { deck, position } => {
                let travel = Gain::from_fraction(position)?.linear();
                // A squared taper: the upper part of the travel covers the useful range.
                RtCommand::Fader {
                    deck,
                    gain: travel * travel,
                }
            }
            Command::MixerCrossfaderSet { position } => RtCommand::Crossfader {
                position: ValueError::check("position", position, 0.0, 1.0)? as f32,
            },
            Command::MixerCrossfaderCurveSet { curve } => RtCommand::CrossfaderCurve { curve },
            Command::MasterGainSet { db } => RtCommand::MasterGain {
                linear: gain_up_to(db, MAX_MASTER_GAIN_DB)?,
            },
            // The deck commands are handled by `dispatch`.
            _ => return Ok(()),
        };
        self.send(command)
    }

    fn send(&mut self, command: RtCommand) -> Result<(), EngineError> {
        self.ends
            .commands
            .push(command)
            .map_err(|_| EngineError::Busy)
    }

    fn send_swap(&mut self, deck: DeckId, audio: Option<TrackAudio>) -> Result<(), EngineError> {
        self.collect_retired();
        // The retired queue has room for this many; see `rt::channel`.
        if self.swaps_in_flight >= COMMAND_CAPACITY {
            return Err(EngineError::Busy);
        }
        self.send(RtCommand::Swap { deck, audio })?;
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
        std::thread::Builder::new()
            .name("platter-load".to_owned())
            .spawn(move || {
                let result = decode_file(&path).map(prepare);
                // The engine may be gone by now; then nobody waits for the result.
                let _ = reports.send(LoadResult {
                    deck,
                    generation,
                    result,
                });
            })
            .map_err(EngineError::Worker)?;
        self.decks[deck.index()].loading = true;
        self.loads_running += 1;
        Ok(())
    }

    fn eject(&mut self, deck: DeckId) -> Result<(), EngineError> {
        self.send_swap(deck, None)?;
        let state = &mut self.decks[deck.index()];
        state.generation += 1;
        state.loading = false;
        state.track = None;
        Ok(())
    }

    /// Takes in the loads that finished, without waiting. Call it regularly: it is also
    /// what frees the audio of replaced tracks.
    pub fn poll(&mut self) -> Vec<Event> {
        self.collect_retired();
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
        self.loads_running = self.loads_running.saturating_sub(1);
        let deck = finished.deck;
        if self.decks[deck.index()].generation != finished.generation {
            return None;
        }
        self.decks[deck.index()].loading = false;
        let failed = |message: String| Some(Event::LoadFailed { deck, message });
        let (audio, info) = match finished.result {
            Ok(loaded) => loaded,
            Err(error) => return failed(error.to_string()),
        };
        if let Err(error) = self.send_swap(deck, Some(audio)) {
            return failed(error.to_string());
        }
        self.decks[deck.index()].track = Some(info.clone());
        Some(Event::Loaded { deck, info })
    }

    pub fn snapshot(&self) -> Snapshot {
        let shared = &self.ends.shared;
        let deck = |id: DeckId| {
            let (control, published) = (&self.decks[id.index()], &shared.decks[id.index()]);
            DeckSnapshot {
                track: control.track.clone(),
                loading: control.loading,
                playing: published.playing.get(),
                position_seconds: published.position.get(),
                cue_seconds: published.cue.get(),
                level: published.level.get(),
            }
        };
        Snapshot {
            decks: [deck(DeckId::A), deck(DeckId::B)],
            master_level: shared.master_level.get(),
            limiter_gain: shared.limiter_gain.get(),
        }
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

fn prepare(decoded: DecodedAudio) -> (TrackAudio, TrackInfo) {
    let info = TrackInfo {
        container: decoded.container,
        sample_rate: decoded.sample_rate,
        channels: decoded.channels,
        seconds: decoded.seconds(),
        tags: decoded.tags.clone(),
    };
    let rate = decoded.sample_rate;
    (TrackAudio::new(rate, decoded.into_stereo()), info)
}
