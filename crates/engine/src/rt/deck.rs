//! One deck: reads its track at the current speed and handles the transport.
//!
//! Behaviour notes: `docs/behavior/transport.md`; for the grid, `docs/behavior/beatgrid.md`;
//! for loops and jumps, `docs/behavior/loops.md`; for the stored cues,
//! `docs/behavior/cues.md`.

use platter_core::{BeatGrid, Cue, CueKind, Cues, HotcueSlot, SampleRate};
use platter_dsp::Frame;
use platter_dsp::interpolate::cubic;
use platter_dsp::smooth::Smoothed;
use rtrb::Producer;

use super::looper::{Looper, Region, Track};
use super::{CueEdit, GridEdit, LoopEdit, LoopState, Retired, Span, TrackAudio};

/// Length of the fade applied on play, pause, seek and track change.
const TRANSPORT_FADE_MS: f32 = 5.0;
const SPEED_RAMP_MS: f32 = 20.0;
/// Length of the crossfade at a jump that keeps the time: a beatjump or the return of a
/// loop to its start.
const SPLICE_SECONDS: f64 = 0.005;

/// What the deck was playing before a jump that keeps the time. It goes on from where it
/// was while it fades out under what the deck plays now.
struct Tail {
    position: f64,
    level: f32,
    /// What the level loses per frame.
    decay: f32,
}

impl Tail {
    /// A tail that fades over `frames` frames of output, when that is long enough to fade.
    fn over(position: f64, frames: f64) -> Option<Self> {
        (frames >= 1.0).then_some(Self {
            position,
            level: 1.0,
            decay: (1.0 / frames) as f32,
        })
    }
}

/// Where a seek lands. A position given in seconds is resolved when the seek is applied,
/// because by then the deck may hold another track with another sample rate.
#[derive(Clone, Copy)]
enum SeekTarget {
    Frame(f64),
    Seconds(f64),
}

pub(super) struct DeckPlayer {
    engine_rate: f64,
    audio: Option<TrackAudio>,
    /// Position in frames of the track.
    position: f64,
    cue: f64,
    playing: bool,
    fade: Smoothed,
    tempo: Smoothed,
    nudge: Smoothed,
    // Changes that wait for the fade-out to finish, so they never cut the waveform. A swap
    // is applied before a seek: the seek belongs to the track that arrives.
    pending_swap: Option<Option<TrackAudio>>,
    pending_seek: Option<SeekTarget>,
    /// Cue point set for a track that is still arriving, in its frames.
    arriving_cue: Option<f64>,
    /// The cue button is held on a paused deck and the track plays until it is released.
    cue_preview: bool,
    /// The hotcue whose key holds that preview, when it is not the cue button.
    held_hotcue: Option<HotcueSlot>,
    /// The cue-play button is held: the deck waits at the cue point for its release.
    cup_held: bool,
    grid: BeatGrid,
    /// Grid edited for a track that is still arriving.
    arriving_grid: Option<BeatGrid>,
    /// The grid changed since the control side last got it.
    grid_unreported: bool,
    /// The loop of the track the deck holds once the pending swap is applied.
    looper: Looper,
    /// The stored cues of the track the deck holds once the pending swap is applied.
    cues: Cues,
    /// The cues changed since the control side last got them.
    cues_unreported: bool,
    tail: Option<Tail>,
}

impl DeckPlayer {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            engine_rate: rate.as_f64(),
            audio: None,
            position: 0.0,
            cue: 0.0,
            playing: false,
            fade: Smoothed::new(0.0, rate, TRANSPORT_FADE_MS),
            tempo: Smoothed::new(1.0, rate, SPEED_RAMP_MS),
            nudge: Smoothed::new(0.0, rate, SPEED_RAMP_MS),
            pending_swap: None,
            pending_seek: None,
            arriving_cue: None,
            cue_preview: false,
            held_hotcue: None,
            cup_held: false,
            grid: BeatGrid::default(),
            arriving_grid: None,
            grid_unreported: false,
            looper: Looper::default(),
            cues: Cues::default(),
            cues_unreported: false,
            tail: None,
        }
    }

    /// Replaces the track. The new one starts paused at its beginning; a play or a seek
    /// that follows applies to it.
    pub(super) fn swap(&mut self, audio: Option<TrackAudio>, retired: &mut Producer<Retired>) {
        // A swap that never got applied still owes its item to the retired queue.
        if let Some(superseded) = self.pending_swap.take() {
            retire(superseded, retired);
        }
        self.playing = false;
        self.cue_preview = false;
        self.held_hotcue = None;
        self.cup_held = false;
        self.pending_seek = None;
        self.arriving_cue = None;
        self.arriving_grid = None;
        self.looper.clear();
        self.set_cues(Cues::default());
        self.pending_swap = Some(audio);
    }

    /// The track the deck holds once the pending swap is applied.
    fn settled_audio(&self) -> Option<&TrackAudio> {
        match &self.pending_swap {
            Some(arriving) => arriving.as_ref(),
            None => self.audio.as_ref(),
        }
    }

    fn will_be_loaded(&self) -> bool {
        self.settled_audio().is_some()
    }

    pub(super) fn set_playing(&mut self, playing: bool) {
        self.cue_preview = false;
        self.held_hotcue = None;
        self.playing = playing && self.will_be_loaded();
    }

    /// Play pressed while the cue button holds a preview keeps the deck playing after the
    /// cue button is released.
    pub(super) fn toggle_playing(&mut self) {
        let keep_playing = self.cue_preview;
        self.set_playing(keep_playing || !self.playing);
    }

    /// Where the deck is once the pending changes are applied, in frames of the track it
    /// will hold.
    fn settled_position(&self) -> f64 {
        match (self.pending_seek, self.settled_audio()) {
            (Some(SeekTarget::Frame(frame)), _) => frame,
            (Some(SeekTarget::Seconds(seconds)), Some(audio)) => seconds * audio.rate().as_f64(),
            _ if self.pending_swap.is_some() => 0.0,
            _ => self.position,
        }
    }

    /// The cue point once the pending swap is applied: a track that arrives has it at its
    /// beginning unless it was set since.
    fn settled_cue(&self) -> f64 {
        if self.pending_swap.is_some() {
            self.arriving_cue.unwrap_or(0.0)
        } else {
            self.cue
        }
    }

    pub(super) fn set_cue(&mut self) {
        let end = self
            .settled_audio()
            .map_or(0.0, |audio| audio.frames() as f64);
        self.set_cue_at(self.settled_position().clamp(0.0, end));
    }

    fn set_cue_at(&mut self, cue: f64) {
        if self.pending_swap.is_some() {
            self.arriving_cue = Some(cue);
        } else {
            self.cue = cue;
        }
    }

    /// The cue button. While the deck plays, pressing it returns to the cue point and
    /// pauses. On a paused deck away from the cue point, pressing it moves the cue point
    /// there. On a paused deck at the cue point, the track plays while the button is held
    /// and returns to the cue point when it is released.
    pub(super) fn cue_button(&mut self, pressed: bool) {
        if !pressed {
            // A preview a hotcue holds ends with the key of that hotcue.
            if self.cue_preview && self.held_hotcue.is_none() {
                self.goto_cue();
            }
            return;
        }
        if self.playing {
            if !self.cue_preview {
                self.goto_cue();
            }
        } else if (self.settled_position() - self.settled_cue()).abs() >= 1.0 {
            self.set_cue();
        } else if self.will_be_loaded() {
            self.playing = true;
            self.cue_preview = true;
            self.held_hotcue = None;
        }
    }

    /// The cue-play button: pressing it returns to the cue point and waits there; releasing
    /// it starts playback.
    pub(super) fn cup_button(&mut self, pressed: bool) {
        if pressed {
            self.cue_preview = false;
            self.held_hotcue = None;
            self.goto_cue();
            self.cup_held = true;
        } else if self.cup_held {
            self.cup_held = false;
            self.set_playing(true);
        }
    }

    pub(super) fn goto_cue(&mut self) {
        self.playing = false;
        self.cue_preview = false;
        self.held_hotcue = None;
        self.pending_seek = Some(SeekTarget::Frame(self.settled_cue()));
    }

    pub(super) fn seek_seconds(&mut self, seconds: f64) {
        if seconds.is_finite() && self.will_be_loaded() {
            self.pending_seek = Some(SeekTarget::Seconds(seconds));
        }
    }

    /// Applies an edit to the grid of the track the deck holds once the pending changes are
    /// applied, at the play position it will have then.
    pub(super) fn edit_grid(&mut self, edit: GridEdit) {
        let Some(audio) = self.settled_audio() else {
            return;
        };
        let rate = audio.rate().as_f64();
        let end = audio.frames() as f64 / rate;
        let at = (self.settled_position() / rate).clamp(0.0, end);
        let arriving = self.pending_swap.is_some();
        let grid = if arriving {
            self.arriving_grid.get_or_insert_with(BeatGrid::default)
        } else {
            &mut self.grid
        };
        let jump = match edit {
            GridEdit::MarkerPrevious => grid.previous_marker(at),
            GridEdit::MarkerNext => grid.next_marker(at),
            _ => None,
        };
        let changed = match edit {
            GridEdit::SetBpm(bpm) => grid.set_bpm(bpm),
            GridEdit::ScaleBpm(factor) => grid.scale_bpm(factor),
            GridEdit::StepBpm(amount) => grid.step_bpm(amount),
            GridEdit::MarkerSet => grid.add_marker(at),
            GridEdit::MarkerMove(by) => grid.move_marker(at, by, end),
            GridEdit::MarkerDelete => grid.delete_marker(at),
            GridEdit::ToggleLock => {
                grid.set_locked(!grid.is_locked());
                true
            }
            GridEdit::MarkerPrevious | GridEdit::MarkerNext => false,
        };
        if changed && !arriving {
            self.grid_unreported = true;
        }
        if let Some(seconds) = jump {
            self.pending_seek = Some(SeekTarget::Seconds(seconds));
        }
    }

    /// What the loop needs to know about the track the deck holds once the pending swap is
    /// applied.
    fn settled_track(&self) -> Option<Track> {
        let audio = self.settled_audio()?;
        let rate = audio.rate().as_f64();
        let grid = if self.pending_swap.is_some() {
            self.arriving_grid.as_ref()
        } else {
            Some(&self.grid)
        };
        Some(Track {
            rate,
            frames: audio.frames() as f64,
            beat: grid
                .and_then(BeatGrid::bpm)
                .map(|bpm| bpm.beat_seconds() * rate),
        })
    }

    /// Applies an edit to the loop, at the play position the deck has once the pending
    /// changes are applied.
    pub(super) fn edit_loop(&mut self, edit: LoopEdit) {
        let Some(track) = self.settled_track() else {
            return;
        };
        let at = self.settled_position().clamp(0.0, track.frames);
        if let Some(target) = self.looper.edit(edit, at, track) {
            self.jump_to(target);
        }
    }

    /// Moves the play position by a span. A jump that would leave the track does nothing.
    pub(super) fn jump(&mut self, by: Span) {
        let Some(track) = self.settled_track() else {
            return;
        };
        let at = self.settled_position().clamp(0.0, track.frames);
        let Some(target) = self.looper.frames_of(by, track).map(|by| at + by) else {
            return;
        };
        if target != at && (0.0..=track.frames).contains(&target) {
            self.jump_to(target);
        }
    }

    /// Goes to a frame of the settled track without losing time: a seek that still waits
    /// takes the new target, and otherwise the deck is there from the next frame on, with
    /// what it was playing fading out under it.
    fn jump_to(&mut self, frame: f64) {
        if self.pending_swap.is_some() || self.pending_seek.is_some() {
            self.pending_seek = Some(SeekTarget::Frame(frame));
        } else {
            self.tail = Tail::over(self.position, SPLICE_SECONDS * self.engine_rate);
            self.position = frame;
        }
    }

    fn set_cues(&mut self, cues: Cues) {
        if cues != self.cues {
            self.cues = cues;
            self.cues_unreported = true;
        }
    }

    /// Applies an edit to the stored cues of the track the deck holds once the pending
    /// changes are applied, at the play position it will have then.
    pub(super) fn edit_cues(&mut self, edit: CueEdit) {
        let Some(track) = self.settled_track() else {
            return;
        };
        let at = self.settled_position().clamp(0.0, track.frames);
        let mut cues = self.cues;
        match edit {
            CueEdit::Hotcue {
                slot,
                pressed: true,
            } => match cues.get(slot).copied() {
                Some(cue) => {
                    cues.select(slot);
                    self.go_to_cue(cue, track, Some(slot));
                }
                None => {
                    cues.store(slot, self.cue_to_store(at, track));
                }
            },
            CueEdit::Hotcue {
                slot,
                pressed: false,
            } => {
                if self.cue_preview && self.held_hotcue == Some(slot) {
                    self.goto_cue();
                }
            }
            CueEdit::Store => {
                if let Some(slot) = cues.first_free() {
                    let floating = self.settled_cue().clamp(0.0, track.frames);
                    cues.store(slot, self.cue_to_store(floating, track));
                }
            }
            CueEdit::Delete(slot) => {
                cues.delete(slot);
            }
            CueEdit::DeleteSelected => {
                if let Some(slot) = cues.selected() {
                    cues.delete(slot);
                }
            }
            CueEdit::Map(slot) => {
                cues.map_selected(slot);
            }
            CueEdit::Step { forward } => {
                let seconds = at / track.rate;
                let slot = if forward {
                    cues.next(seconds)
                } else {
                    cues.previous(seconds)
                };
                if let Some((slot, cue)) = slot.and_then(|slot| Some((slot, *cues.get(slot)?))) {
                    cues.select(slot);
                    self.go_to_cue(cue, track, None);
                }
            }
            CueEdit::Rename(name) => {
                cues.rename_selected(name);
            }
            CueEdit::Retype { kind, beats } => {
                // A loop the track cannot hold from the cue on is not made.
                let length = cues.selected_cue().and_then(|cue| {
                    let length = beats * track.beat?;
                    let fits =
                        kind == CueKind::Loop && cue.position * track.rate + length <= track.frames;
                    fits.then_some(length / track.rate)
                });
                cues.retype_selected(kind, length);
            }
        }
        self.set_cues(cues);
    }

    /// What a store at frame `at` stores: the loop that is on, or else that place.
    fn cue_to_store(&self, at: f64, track: Track) -> Cue {
        match self.looper.state(track.rate).region {
            Some(region) => Cue::looping(region),
            None => Cue::at(at / track.rate),
        }
    }

    /// Goes to a stored cue and switches on the loop it stores. A deck that plays goes on
    /// from there. A paused one moves there and, under the key of a hotcue, takes the cue as
    /// its floating cue and plays until that key is released, as under the cue button.
    fn go_to_cue(&mut self, cue: Cue, track: Track, key: Option<HotcueSlot>) {
        let frame = (cue.position * track.rate).clamp(0.0, track.frames);
        if let Some(region) = cue.region() {
            // The end went through seconds and back: it may be a hair past the track.
            let end = (region.end * track.rate).min(track.frames);
            self.looper.set_region(Region { start: frame, end }, track);
        }
        if self.playing && !self.cue_preview {
            self.jump_to(frame);
            return;
        }
        self.pending_seek = Some(SeekTarget::Frame(frame));
        if key.is_some() {
            self.set_cue_at(frame);
            self.playing = true;
            self.cue_preview = true;
            self.held_hotcue = key;
        }
    }

    /// The stored cues, once, after each change.
    pub(super) fn unreported_cues(&self) -> Option<Cues> {
        self.cues_unreported.then_some(self.cues)
    }

    pub(super) fn cues_reported(&mut self) {
        self.cues_unreported = false;
    }

    /// The loop, once, after each change.
    pub(super) fn unreported_loop(&self) -> Option<LoopState> {
        if !self.looper.is_unreported() {
            return None;
        }
        // An empty deck has no loop: the swap that emptied it cleared it.
        let rate = self
            .settled_audio()
            .map_or(1.0, |audio| audio.rate().as_f64());
        Some(self.looper.state(rate))
    }

    pub(super) fn loop_reported(&mut self) {
        self.looper.reported();
    }

    /// The grid, once, after each change.
    pub(super) fn unreported_grid(&self) -> Option<&BeatGrid> {
        self.grid_unreported.then_some(&self.grid)
    }

    pub(super) fn grid_reported(&mut self) {
        self.grid_unreported = false;
    }

    pub(super) fn set_tempo(&mut self, ratio: f32) {
        self.tempo.set(ratio);
    }

    pub(super) fn set_nudge(&mut self, amount: f32) {
        self.nudge.set(amount);
    }

    /// Produces the next frame of the deck.
    pub(super) fn next(&mut self, retired: &mut Producer<Retired>) -> Frame {
        let audible = self.playing && self.pending_swap.is_none() && self.pending_seek.is_none();
        self.fade.set(if audible { 1.0 } else { 0.0 });
        let gain = self.fade.advance();
        if gain == 0.0 {
            self.tail = None;
            self.apply_pending(retired);
            return [0.0; 2];
        }
        let Some(audio) = &self.audio else {
            return [0.0; 2];
        };
        let mut frame = read(audio, self.position);
        let speed = f64::from(self.tempo.advance()) * (1.0 + f64::from(self.nudge.advance()));
        let step = speed * audio.rate().as_f64() / self.engine_rate;
        if let Some(tail) = &mut self.tail {
            tail.level -= tail.decay;
            if tail.level > 0.0 {
                let old = read(audio, tail.position);
                frame = [0, 1].map(|side| frame[side] + (old[side] - frame[side]) * tail.level);
                tail.position += step;
            } else {
                self.tail = None;
            }
        }
        let before = self.position;
        self.position += step;
        // The loop belongs to the track that arrives while a swap waits.
        if self.pending_swap.is_none()
            && let Some((start, length)) = self.looper.wrap(before, self.position)
        {
            // At most half a pass of the loop, so one crossfade ends before the next.
            let frames = (SPLICE_SECONDS * self.engine_rate).min(length / (2.0 * step));
            self.tail = Tail::over(self.position, frames);
            self.position = start;
        }
        let end = audio.frames() as f64;
        if self.position >= end {
            self.position = end;
            self.playing = false;
        }
        [frame[0] * gain, frame[1] * gain]
    }

    fn apply_pending(&mut self, retired: &mut Producer<Retired>) {
        if let Some(arriving) = self.pending_swap.take() {
            let previous = std::mem::replace(&mut self.audio, arriving);
            retire(previous, retired);
            self.position = 0.0;
            self.cue = self.arriving_cue.take().unwrap_or(0.0);
            self.grid = self.arriving_grid.take().unwrap_or_default();
            self.grid_unreported = true;
        }
        if let Some(target) = self.pending_seek.take()
            && let Some(audio) = &self.audio
        {
            let frame = match target {
                SeekTarget::Frame(frame) => frame,
                SeekTarget::Seconds(seconds) => seconds * audio.rate().as_f64(),
            };
            self.position = frame.clamp(0.0, audio.frames() as f64);
        }
    }

    pub(super) fn is_loaded(&self) -> bool {
        self.audio.is_some()
    }

    pub(super) fn is_playing(&self) -> bool {
        self.playing
    }

    pub(super) fn position_seconds(&self) -> f64 {
        self.seconds(self.position)
    }

    pub(super) fn cue_seconds(&self) -> f64 {
        self.seconds(self.cue)
    }

    fn seconds(&self, frame: f64) -> f64 {
        self.audio
            .as_ref()
            .map_or(0.0, |audio| frame / audio.rate().as_f64())
    }
}

fn read(audio: &TrackAudio, position: f64) -> Frame {
    let base = position.floor();
    let fraction = (position - base) as f32;
    let base = base as i64;
    let taps = [-1, 0, 1, 2].map(|offset| audio.frame(base + offset));
    [
        cubic(taps.map(|frame| frame[0]), fraction),
        cubic(taps.map(|frame| frame[1]), fraction),
    ]
}

/// Hands audio back to the control side. The queue is sized so that it cannot be full; if
/// it ever were, leaking the buffer is the only option that keeps the audio thread from
/// freeing memory.
fn retire(audio: Option<TrackAudio>, retired: &mut Producer<Retired>) {
    if let Err(rtrb::PushError::Full(item)) = retired.push(Retired(audio)) {
        std::mem::forget(item);
    }
}
