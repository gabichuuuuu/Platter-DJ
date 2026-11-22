//! The remix player of a deck: four slots of sixteen cells, each cell a sample that starts on
//! a beat of the master clock. Behaviour note: `docs/behavior/remix-deck.md`; the choices:
//! `docs/decisions/0042-remix-deck.md`.
//!
//! Like the decks it runs on the audio thread: it neither allocates nor frees. A sample
//! arrives inside a command and leaves through the retired queue, exactly one item per load
//! or clear; a sample that still fades out when it is replaced leaves once its fade ends.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::smooth::Smoothed;
use rtrb::Producer;

use super::{DeckTrack, Retired, TrackAudio};

pub const SLOTS: usize = 4;
pub const CELLS: usize = 16;
/// The longest sample a cell takes, in seconds.
pub const MAX_SAMPLE_SECONDS: f64 = 64.0;
/// The sizes of the quantize, in beats.
pub const QUANTIZE_BEATS: [u32; 5] = [1, 2, 4, 8, 16];
/// The fade of a cell that stops or gives way to another, and of the volume and the mute.
const FADE_MS: f32 = 5.0;
/// A cell that waits longer than the largest size of the quantize does so because the clock went
/// back, a received clock having started again: it starts at once.
const LONGEST_WAIT: f64 = 17.0;

/// How a cell plays its sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellMode {
    /// Back to its beginning at its end.
    #[default]
    Loop,
    /// Once, then the slot is silent.
    OneShot,
}

impl CellMode {
    pub const ALL: [Self; 2] = [Self::Loop, Self::OneShot];

    pub fn name(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::OneShot => "one_shot",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }
}

/// A place in the grid of a deck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellId {
    /// From zero, below [`SLOTS`].
    pub slot: usize,
    /// From zero, below [`CELLS`].
    pub cell: usize,
}

impl CellId {
    pub fn new(slot: usize, cell: usize) -> Option<Self> {
        (slot < SLOTS && cell < CELLS).then_some(Self { slot, cell })
    }
}

/// What sets how a cell plays, apart from its sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellSettings {
    pub mode: CellMode,
    /// The tempo of the sample; without one the cell plays at its own speed.
    pub bpm: Option<f64>,
    /// Linear, before the volume of the slot.
    pub gain: f32,
}

impl Default for CellSettings {
    fn default() -> Self {
        Self {
            mode: CellMode::Loop,
            bpm: None,
            gain: 1.0,
        }
    }
}

#[derive(Default)]
struct Cell {
    audio: Option<TrackAudio>,
    settings: CellSettings,
}

struct Voice {
    cell: usize,
    /// Play position in frames of the sample.
    position: f64,
    gain: Smoothed,
    /// The sample of a cell that was loaded again or cleared while this voice played: it is
    /// retired when the fade ends.
    owned: Option<TrackAudio>,
}

/// A cell that waits for its boundary, in beats of the clock; `None` starts on the next frame.
#[derive(Clone, Copy)]
struct Waiting {
    cell: usize,
    at: Option<f64>,
}

struct Slot {
    cells: [Cell; CELLS],
    playing: Option<Voice>,
    fading: Option<Voice>,
    waiting: Option<Waiting>,
    /// The boundary the cell that plays stops on.
    stop_at: Option<Option<f64>>,
    volume: Smoothed,
    mute: Smoothed,
}

/// The cell a slot plays and the one it waits to start, for the snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotState {
    pub playing: Option<usize>,
    pub waiting: Option<usize>,
}

pub(super) struct RemixPlayer {
    slots: [Slot; SLOTS],
    rate: SampleRate,
    engine_rate: f64,
    /// Beats of the boundary; `None` without quantize.
    quantize: Option<u32>,
}

impl RemixPlayer {
    pub(super) fn new(rate: SampleRate) -> Self {
        let slot = |_| Slot {
            cells: Default::default(),
            playing: None,
            fading: None,
            waiting: None,
            stop_at: None,
            volume: Smoothed::new(1.0, rate, FADE_MS),
            mute: Smoothed::new(1.0, rate, FADE_MS),
        };
        Self {
            slots: std::array::from_fn(slot),
            rate,
            engine_rate: rate.as_f64(),
            quantize: Some(1),
        }
    }

    pub(super) fn set_quantize(&mut self, beats: Option<u32>) {
        if beats.is_none_or(|beats| QUANTIZE_BEATS.contains(&beats)) {
            self.quantize = beats;
        }
    }

    /// Puts a sample in a cell, or empties it; one item goes back through `retired`.
    pub(super) fn load(
        &mut self,
        at: CellId,
        audio: Option<TrackAudio>,
        settings: CellSettings,
        retired: &mut Producer<Retired>,
    ) {
        let Some(slot) = self.slots.get_mut(at.slot) else {
            retire(audio, retired);
            return;
        };
        let Some(cell) = slot.cells.get_mut(at.cell) else {
            retire(audio, retired);
            return;
        };
        let earlier = std::mem::replace(&mut cell.audio, audio);
        cell.settings = settings;
        if slot.waiting.is_some_and(|waiting| waiting.cell == at.cell) {
            slot.waiting = None;
        }
        if slot
            .playing
            .as_ref()
            .is_some_and(|voice| voice.cell == at.cell)
        {
            slot.stop_at = None;
            fade_out(slot, retired);
        }
        // A voice that fades out on the sample replaced keeps it until its fade ends, and the
        // item of this command goes back then.
        if earlier.is_some()
            && let Some(voice) = &mut slot.fading
            && voice.cell == at.cell
            && voice.owned.is_none()
        {
            voice.owned = earlier;
            return;
        }
        retire(earlier, retired);
    }

    /// Changes how a cell plays, keeping its sample.
    pub(super) fn set_settings(&mut self, at: CellId, settings: CellSettings) {
        if let Some(cell) = self
            .slots
            .get_mut(at.slot)
            .and_then(|slot| slot.cells.get_mut(at.cell))
        {
            cell.settings = settings;
        }
    }

    /// Starts a cell on the next boundary after `beat`. An empty cell does nothing.
    pub(super) fn trigger(&mut self, at: CellId, beat: f64) {
        let boundary = self.boundary(beat);
        let Some(slot) = self.slots.get_mut(at.slot) else {
            return;
        };
        if slot
            .cells
            .get(at.cell)
            .is_none_or(|cell| cell.audio.is_none())
        {
            return;
        }
        slot.waiting = Some(Waiting {
            cell: at.cell,
            at: boundary,
        });
        slot.stop_at = None;
    }

    /// Stops the cell of a slot on the next boundary after `beat`, and forgets one that waits.
    pub(super) fn stop(&mut self, slot: usize, beat: f64) {
        let boundary = self.boundary(beat);
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.waiting = None;
            if slot.playing.is_some() {
                slot.stop_at = Some(boundary);
            }
        }
    }

    pub(super) fn set_volume(&mut self, slot: usize, level: f32) {
        if let Some(slot) = self.slots.get_mut(slot)
            && level.is_finite()
        {
            slot.volume.set(level.clamp(0.0, 1.0));
        }
    }

    pub(super) fn set_mute(&mut self, slot: usize, on: bool) {
        if let Some(slot) = self.slots.get_mut(slot) {
            slot.mute.set(if on { 0.0 } else { 1.0 });
        }
    }

    pub(super) fn state(&self, slot: usize) -> SlotState {
        self.slots
            .get(slot)
            .map_or_else(SlotState::default, |slot| SlotState {
                playing: slot.playing.as_ref().map(|voice| voice.cell),
                waiting: slot.waiting.map(|waiting| waiting.cell),
            })
    }

    /// The next frame of the grid. `beat` counts the beats of the clock with the phase of this
    /// frame; `bpm` is its tempo.
    pub(super) fn next(&mut self, beat: f64, bpm: f64, retired: &mut Producer<Retired>) -> Frame {
        let rate = self.rate;
        let engine_rate = self.engine_rate;
        let mut out = [0.0f32; 2];
        for slot in &mut self.slots {
            if let Some(waiting) = slot.waiting
                && waiting
                    .at
                    .is_none_or(|at| beat >= at || at - beat > LONGEST_WAIT)
            {
                slot.waiting = None;
                fade_out(slot, retired);
                slot.playing = Some(Voice {
                    cell: waiting.cell,
                    position: 0.0,
                    gain: Smoothed::new(1.0, rate, FADE_MS),
                    owned: None,
                });
            }
            if let Some(at) = slot.stop_at
                && at.is_none_or(|at| beat >= at)
            {
                slot.stop_at = None;
                fade_out(slot, retired);
            }
            let level = slot.volume.advance() * slot.mute.advance();
            let mut sum = [0.0f32; 2];
            let cells = &slot.cells;
            let mut ended = false;
            if let Some(voice) = &mut slot.playing {
                ended = play(voice, cells, bpm, engine_rate, &mut sum);
            }
            if ended {
                slot.playing = None;
            }
            let mut faded = false;
            if let Some(voice) = &mut slot.fading {
                faded = play(voice, cells, bpm, engine_rate, &mut sum)
                    || (voice.gain.target() == 0.0 && voice.gain.is_settled());
            }
            if faded && let Some(voice) = slot.fading.take() {
                finish(voice, retired);
            }
            out[0] += sum[0] * level;
            out[1] += sum[1] * level;
        }
        out
    }

    /// The beat a cell triggered at `beat` starts on.
    fn boundary(&self, beat: f64) -> Option<f64> {
        let size = f64::from(self.quantize?);
        if !beat.is_finite() {
            return None;
        }
        Some(((beat / size).floor() + 1.0) * size)
    }
}

/// Moves the voice that plays to the fading one, which fades out; a voice that still faded is
/// let go of at once.
fn fade_out(slot: &mut Slot, retired: &mut Producer<Retired>) {
    let Some(mut voice) = slot.playing.take() else {
        return;
    };
    voice.gain.set(0.0);
    if let Some(earlier) = slot.fading.replace(voice) {
        finish(earlier, retired);
    }
}

/// Lets go of a voice, retiring the sample it held.
fn finish(voice: Voice, retired: &mut Producer<Retired>) {
    if voice.owned.is_some() {
        retire(voice.owned, retired);
    }
}

/// Adds the next frame of a voice to `sum`; true when the voice has ended.
fn play(
    voice: &mut Voice,
    cells: &[Cell; CELLS],
    bpm: f64,
    engine_rate: f64,
    sum: &mut Frame,
) -> bool {
    let Some(cell) = cells.get(voice.cell) else {
        return true;
    };
    let Some(audio) = voice.owned.as_ref().or(cell.audio.as_ref()) else {
        return true;
    };
    let frames = audio.frames();
    if frames < 2 {
        return true;
    }
    let gain = voice.gain.advance() * cell.settings.gain;
    let index = voice.position.floor();
    let frame = index as usize;
    let fraction = (voice.position - index) as f32;
    let first = audio.frame(frame as i64);
    let second = audio.frame(((frame + 1) % frames) as i64);
    sum[0] += (first[0] + (second[0] - first[0]) * fraction) * gain;
    sum[1] += (first[1] + (second[1] - first[1]) * fraction) * gain;
    let mut step = audio.rate().as_f64() / engine_rate;
    if let Some(own) = cell.settings.bpm
        && own > 0.0
        && bpm.is_finite()
        && bpm > 0.0
    {
        step *= bpm / own;
    }
    voice.position += step;
    let end = frames as f64;
    if voice.position >= end {
        match cell.settings.mode {
            CellMode::Loop => voice.position %= end,
            CellMode::OneShot => return true,
        }
    }
    false
}

fn retire(audio: Option<TrackAudio>, retired: &mut Producer<Retired>) {
    let track = audio.map(|audio| DeckTrack {
        audio,
        stretch: None,
        memory: None,
        start: None,
        stems: None,
    });
    if let Err(rtrb::PushError::Full(item)) = retired.push(Retired(track, None, None)) {
        std::mem::forget(item);
    }
}
