//! The loop recorder: a loop of a few bars recorded from the mix or a deck, played over the
//! mix, with overdubs and one layer of undo. Behaviour note: `docs/behavior/recording.md`,
//! *Loop recorder*.
//!
//! Two buffers of the longest loop are allocated with the processor. The overdub writes into
//! `current` and, the first time it passes a frame, keeps the frame it replaces in `previous`;
//! undo and redo swap the two, which takes no time. For the swap to be right `previous` must
//! hold the whole loop as it was before the layer, so a layer that starts after the buffers
//! went apart (after an overdub, an undo or a redo) first copies the frames it does not touch,
//! a bounded chunk per block. The same chunks clear what a first pass ended early left behind.

use platter_core::{DeckId, SampleRate};
use platter_dsp::Frame;
use platter_dsp::smooth::Smoothed;

/// The longest loop, in seconds: 8 bars at 60 beats per minute.
pub const MAX_LOOP_SECONDS: f64 = 32.0;
/// The sizes of a loop, in bars.
pub const LOOP_BARS: [u32; 4] = [1, 2, 4, 8];
const BEATS_PER_BAR: f64 = 4.0;
/// The fades at the seam, at play and pause, around a swap and on the input of an overdub.
const FADE_MS: f32 = 5.0;
/// Frames copied or cleared per block: the whole longest loop at 48 kHz in a few dozen blocks.
const CHUNK_FRAMES: usize = 65_536;
/// A phase that drops by more than this has crossed the start of a beat.
const WRAP: f64 = 0.5;
/// The tempo used when the clock gives none that can be used.
const FALLBACK_BPM: f64 = 120.0;

/// What the loop recorder records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopSource {
    /// The mix of the decks, before the master volume and without the loop.
    #[default]
    Master,
    /// One deck after its effects and before its fader.
    Deck(DeckId),
}

/// What the loop recorder is doing, as the panel shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopPhase {
    #[default]
    Empty,
    /// Rec was pressed; the first pass starts on the next beat.
    Waiting,
    /// The first pass.
    Recording,
    Overdubbing,
    Playing,
    Paused,
}

impl LoopPhase {
    pub(super) fn code(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Waiting => 1,
            Self::Recording => 2,
            Self::Overdubbing => 3,
            Self::Playing => 4,
            Self::Paused => 5,
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Waiting,
            2 => Self::Recording,
            3 => Self::Overdubbing,
            4 => Self::Playing,
            5 => Self::Paused,
            _ => Self::Empty,
        }
    }
}

/// The frames of an overdub: where it started and how many it has passed, at most the loop.
#[derive(Clone, Copy, Debug)]
struct Layer {
    start: usize,
    touched: usize,
}

impl Layer {
    fn contains(self, frame: usize, length: usize) -> bool {
        self.touched >= length || (frame + length - self.start) % length < self.touched
    }
}

/// What waits for the copy and the fade before the buffers are swapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Undo,
    Redo,
    /// The fade before the recorder is emptied.
    Delete,
}

pub(super) struct LoopRecorder {
    rate: SampleRate,
    current: Vec<f32>,
    previous: Vec<f32>,
    /// The frames each buffer holds.
    capacity: usize,
    fade_frames: f32,
    bars: u32,
    source: LoopSource,
    /// The share taken away from what the loop held on each pass of an overdub.
    decay: f32,
    dry_wet: f32,
    /// Frames of the loop; zero while nothing is recorded.
    length: usize,
    position: usize,
    /// The length of the first pass that waits for the beat.
    waiting: Option<usize>,
    first_pass: bool,
    overdub: bool,
    /// The position moves.
    running: bool,
    /// The loop fades out to stop.
    stopping: bool,
    layer: Option<Layer>,
    /// The buffers no longer hold the same loop outside the last layer.
    apart: bool,
    /// The next frame the copy into `previous` reaches.
    copy: Option<usize>,
    /// The next frame the clearing of both buffers reaches.
    clear: Option<usize>,
    can_undo: bool,
    can_redo: bool,
    pending: Option<Pending>,
    level: Smoothed,
    input: Smoothed,
    dry: Smoothed,
    wet: Smoothed,
    previous_phase: f64,
}

impl LoopRecorder {
    pub(super) fn new(rate: SampleRate) -> Self {
        let capacity = (MAX_LOOP_SECONDS * rate.as_f64()).ceil() as usize;
        let fade = |value| Smoothed::new(value, rate, FADE_MS);
        let mut recorder = Self {
            rate,
            current: vec![0.0; capacity * 2],
            previous: vec![0.0; capacity * 2],
            capacity,
            fade_frames: (FADE_MS * rate.as_f32() / 1_000.0).max(1.0),
            bars: 4,
            source: LoopSource::Master,
            decay: 0.0,
            dry_wet: 0.5,
            length: 0,
            position: 0,
            waiting: None,
            first_pass: false,
            overdub: false,
            running: false,
            stopping: false,
            layer: None,
            apart: false,
            copy: None,
            clear: None,
            can_undo: false,
            can_redo: false,
            pending: None,
            level: fade(1.0),
            input: fade(0.0),
            dry: fade(1.0),
            wet: fade(1.0),
            previous_phase: 0.0,
        };
        recorder.retarget_mix();
        recorder
    }

    pub(super) fn source(&self) -> LoopSource {
        self.source
    }

    pub(super) fn set_source(&mut self, source: LoopSource) {
        self.source = source;
    }

    /// Takes a size only while nothing is recorded or waits.
    pub(super) fn set_bars(&mut self, bars: u32) {
        if self.phase() == LoopPhase::Empty && LOOP_BARS.contains(&bars) {
            self.bars = bars;
        }
    }

    pub(super) fn set_decay(&mut self, decay: f32) {
        if decay.is_finite() {
            self.decay = decay.clamp(0.0, 1.0);
        }
    }

    pub(super) fn set_dry_wet(&mut self, amount: f32) {
        if amount.is_finite() {
            self.dry_wet = amount.clamp(0.0, 1.0);
            self.retarget_mix();
        }
    }

    pub(super) fn phase(&self) -> LoopPhase {
        if self.first_pass {
            LoopPhase::Recording
        } else if self.overdub {
            LoopPhase::Overdubbing
        } else if self.waiting.is_some() {
            LoopPhase::Waiting
        } else if self.length == 0 || self.pending == Some(Pending::Delete) {
            LoopPhase::Empty
        } else if self.running && !self.stopping {
            LoopPhase::Playing
        } else {
            LoopPhase::Paused
        }
    }

    /// How far into the loop the position is, from zero up to one.
    pub(super) fn progress(&self) -> f32 {
        if self.length == 0 {
            0.0
        } else {
            self.position as f32 / self.length as f32
        }
    }

    pub(super) fn length_seconds(&self) -> f64 {
        self.length as f64 / self.rate.as_f64()
    }

    pub(super) fn can_undo(&self) -> bool {
        self.can_undo && self.pending.is_none()
    }

    pub(super) fn can_redo(&self) -> bool {
        self.can_redo && self.pending.is_none()
    }

    /// The bars the size gives at `bpm`, halved until the loop fits.
    pub(super) fn fitting_bars(&self, bpm: f64) -> u32 {
        let bpm = usable(bpm);
        let mut bars = self.bars;
        while bars > 1 && seconds(bars, bpm) > MAX_LOOP_SECONDS {
            bars /= 2;
        }
        bars
    }

    /// Rec: waits for the beat, ends the first pass, starts or ends an overdub.
    pub(super) fn record(&mut self, bpm: f64) {
        if self.pending.is_some() {
            return;
        }
        if self.first_pass {
            // The rest of the pass stays silent.
            self.first_pass = false;
            self.input.set(0.0);
            self.clear = Some(self.position);
            self.play();
        } else if self.overdub {
            self.end_overdub();
        } else if self.waiting.is_some() {
            self.waiting = None;
        } else if self.length == 0 {
            let frames = seconds(self.fitting_bars(bpm), usable(bpm)) * self.rate.as_f64();
            self.waiting = Some((frames.round() as usize).clamp(1, self.capacity));
        } else {
            self.play();
            self.begin_overdub();
        }
        self.retarget_mix();
    }

    /// Play: pauses the loop or plays it on from where it is.
    pub(super) fn toggle_play(&mut self) {
        if self.length == 0 || self.first_pass || self.pending == Some(Pending::Delete) {
            return;
        }
        if self.running && !self.stopping {
            self.end_overdub();
            self.stopping = true;
            self.level.set(0.0);
        } else {
            self.play();
        }
        self.retarget_mix();
    }

    pub(super) fn undo(&mut self) {
        if self.can_undo && self.pending.is_none() {
            self.end_overdub();
            self.pending = Some(Pending::Undo);
        }
    }

    pub(super) fn redo(&mut self) {
        if self.can_redo && self.pending.is_none() {
            self.pending = Some(Pending::Redo);
        }
    }

    pub(super) fn delete(&mut self) {
        if self.length == 0 && self.waiting.is_none() {
            return;
        }
        self.waiting = None;
        self.first_pass = false;
        self.end_overdub();
        self.pending = Some(Pending::Delete);
        if !self.running {
            self.reset();
        }
        self.retarget_mix();
    }

    fn play(&mut self) {
        self.running = true;
        self.stopping = false;
        self.level.set(1.0);
    }

    fn begin_overdub(&mut self) {
        self.overdub = true;
        self.input.set(1.0);
        if self.apart {
            self.copy = Some(0);
        }
        self.layer = Some(Layer {
            start: self.position,
            touched: 0,
        });
        self.can_undo = false;
        self.can_redo = false;
    }

    fn end_overdub(&mut self) {
        if self.overdub {
            self.overdub = false;
            self.input.set(0.0);
        }
    }

    fn reset(&mut self) {
        self.length = 0;
        self.position = 0;
        self.running = false;
        self.stopping = false;
        self.layer = None;
        self.apart = false;
        self.copy = None;
        self.clear = None;
        self.can_undo = false;
        self.can_redo = false;
        self.pending = None;
        self.level = Smoothed::new(1.0, self.rate, FADE_MS);
        self.input = Smoothed::new(0.0, self.rate, FADE_MS);
    }

    fn retarget_mix(&mut self) {
        let sounding = self.length > 0 && self.running && !self.stopping;
        let dry = if sounding {
            (2.0 * (1.0 - self.dry_wet)).min(1.0)
        } else {
            1.0
        };
        self.dry.set(dry);
        self.wet.set((2.0 * self.dry_wet).min(1.0));
    }

    /// The work of a block before its frames: a chunk of the copy and of the clearing, and
    /// the swap or the emptying that waits for them and for its fade.
    pub(super) fn prepare(&mut self) {
        if let Some(from) = self.clear {
            let to = (from + CHUNK_FRAMES).min(self.length);
            self.current[from * 2..to * 2].fill(0.0);
            self.previous[from * 2..to * 2].fill(0.0);
            self.clear = (to < self.length).then_some(to);
        }
        if let Some(from) = self.copy {
            let to = (from + CHUNK_FRAMES).min(self.length);
            for frame in from..to {
                if !self
                    .layer
                    .is_some_and(|layer| layer.contains(frame, self.length))
                {
                    let index = frame * 2;
                    self.previous[index] = self.current[index];
                    self.previous[index + 1] = self.current[index + 1];
                }
            }
            self.copy = (to < self.length).then_some(to);
        }
        if self.stopping && self.level.is_settled() {
            self.running = false;
            self.stopping = false;
        }
        let Some(pending) = self.pending else {
            return;
        };
        if self.copy.is_some() {
            return;
        }
        // Silent first, so the swap does not click; the input of an overdub that ended
        // fades out over the same time.
        let silent = !self.running || (self.level.target() == 0.0 && self.level.is_settled());
        if !silent {
            self.level.set(0.0);
            return;
        }
        match pending {
            Pending::Delete => {
                self.reset();
                self.retarget_mix();
                return;
            }
            Pending::Undo => {
                self.can_undo = false;
                self.can_redo = true;
            }
            Pending::Redo => {
                self.can_undo = true;
                self.can_redo = false;
            }
        }
        std::mem::swap(&mut self.current, &mut self.previous);
        self.apart = true;
        self.layer = None;
        self.pending = None;
        if self.running && !self.stopping {
            self.level.set(1.0);
        }
    }

    /// One frame: takes the frame of the source and the phase and tempo of the clock, and
    /// gives the gain of the mix and the frame of the loop to add to it.
    pub(super) fn next(&mut self, source: Frame, phase: f64) -> (f32, Frame) {
        let beat = self.previous_phase - phase > WRAP;
        self.previous_phase = phase;
        if beat && let Some(length) = self.waiting.take() {
            self.length = length;
            self.position = 0;
            self.first_pass = true;
            self.apart = false;
            self.layer = None;
            self.input = Smoothed::new(0.0, self.rate, FADE_MS);
            self.input.set(1.0);
            self.level = Smoothed::new(1.0, self.rate, FADE_MS);
        }
        let dry = self.dry.advance();
        let wet = self.wet.advance();
        let level = self.level.advance();
        let input = self.input.advance();
        if self.length == 0 || !(self.first_pass || self.running) {
            return (dry, [0.0; 2]);
        }
        let frame = self.position;
        let index = frame * 2;
        if self.clear.is_some_and(|from| frame >= from) {
            self.current[index..index + 2].fill(0.0);
            self.previous[index..index + 2].fill(0.0);
        }
        let old = [self.current[index], self.current[index + 1]];
        let out = if self.first_pass {
            [0.0; 2]
        } else {
            let edge = frame.min(self.length - 1 - frame) as f32;
            let gain = (edge / self.fade_frames).min(1.0) * level * wet;
            [old[0] * gain, old[1] * gain]
        };
        if self.first_pass || input > 0.0 {
            match self.layer.as_mut() {
                None => {
                    let written = [source[0] * input, source[1] * input];
                    let kept = if self.first_pass { 0.0 } else { 1.0 };
                    for side in 0..2 {
                        let value = old[side] * kept + written[side];
                        self.current[index + side] = value;
                        self.previous[index + side] = value;
                    }
                }
                Some(layer) => {
                    if layer.touched < self.length {
                        self.previous[index] = old[0];
                        self.previous[index + 1] = old[1];
                        layer.touched += 1;
                    }
                    let kept = 1.0 - self.decay * input;
                    for side in 0..2 {
                        self.current[index + side] = old[side] * kept + source[side] * input;
                    }
                    self.can_undo = true;
                }
            }
        }
        self.position += 1;
        if self.position >= self.length {
            self.position = 0;
            if self.first_pass {
                // The pass is the loop and plays. The input fades out over the start of the
                // loop, which smooths the seam.
                self.first_pass = false;
                self.input.set(0.0);
                self.play();
                self.retarget_mix();
            }
        }
        (dry, out)
    }
}

fn usable(bpm: f64) -> f64 {
    if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        FALLBACK_BPM
    }
}

fn seconds(bars: u32, bpm: f64) -> f64 {
    f64::from(bars) * BEATS_PER_BAR * 60.0 / bpm
}
