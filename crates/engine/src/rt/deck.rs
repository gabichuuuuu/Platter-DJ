//! One deck: reads its track at the current speed and handles the transport.
//!
//! Behaviour notes: `docs/behavior/transport.md`; for the grid, `docs/behavior/beatgrid.md`;
//! for loops and jumps, `docs/behavior/loops.md`; for the stored cues,
//! `docs/behavior/cues.md`; for sync, `docs/behavior/sync.md`; for flux and reverse,
//! `docs/behavior/flux.md`; for the beat tick, `docs/behavior/beatgrid.md`.

use platter_core::{BeatGrid, Cue, CueKind, Cues, HotcueSlot, SampleRate, Tempo, TrackMemory};
use platter_dsp::Frame;
use platter_dsp::interpolate::cubic;
use platter_dsp::smooth::Smoothed;
use rtrb::Producer;

use super::clock::{Candidate, Lead, Pulse};
use super::lock::KeyLock;
use super::looper::{Looper, Region, Track};
use super::{
    CueEdit, DeckTrack, GridEdit, LoopEdit, LoopState, MAX_KEY_SHIFT, Retired, Span, TrackAudio,
};

/// Length of the fade applied on play, pause, seek and track change.
const TRANSPORT_FADE_MS: f32 = 5.0;
pub(super) const SPEED_RAMP_MS: f32 = 20.0;
/// Length of the crossfade at a jump that keeps the time: a beatjump or the return of a
/// loop to its start.
const SPLICE_SECONDS: f64 = 0.005;
/// Offset from the beat of the clock, in beats, above which a deck that keeps the beat
/// moves there at once. At 120 beats per minute it is 16 ms, where two beats start to be
/// heard apart.
const ALIGN_BEATS: f64 = 1.0 / 32.0;
/// Speed a deck gains per beat it is behind the clock, and the most it gains or loses.
const PULL_PER_BEAT: f64 = 2.0;
const MAX_PULL: f64 = 0.02;
/// A loop this close to a whole number of beats keeps the beat.
const WHOLE_BEATS_TOLERANCE: f64 = 1e-6;

/// The shortest way from one place in the beat to another, in beats: from minus a half up
/// to a half.
fn beats_between(from: f64, to: f64) -> f64 {
    (to - from + 0.5).rem_euclid(1.0) - 0.5
}

/// What the deck was playing before a jump that keeps the time. It goes on from where it
/// was while it fades out under what the deck plays now.
struct Tail {
    position: f64,
    level: f32,
    /// What the level loses per frame.
    decay: f32,
    /// The direction it goes on in: the one the deck had when it left it.
    forward: bool,
}

impl Tail {
    /// A tail that fades over `frames` frames of output, when that is long enough to fade.
    fn over(position: f64, frames: f64, forward: bool) -> Option<Self> {
        (frames >= 1.0).then_some(Self {
            position,
            level: 1.0,
            decay: (1.0 / frames) as f32,
            forward,
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
    pending_swap: Option<Option<DeckTrack>>,
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
    sync: bool,
    /// New cues and loops land on the nearest beat.
    snap: bool,
    /// A playing deck that goes to a stored cue keeps its place in the beat.
    quantize: bool,
    /// The place in its beat the clock is at on this frame, when the deck keeps its beats
    /// on those of the clock.
    beat_to_keep: Option<f64>,
    lock: KeyLock,
    /// Shift of the key in semitones, played through key lock.
    key_shift: f64,
    /// A loop or a hotcue of a playing deck keeps the timeline.
    flux: bool,
    /// Reverse is held.
    reverse: bool,
    /// Where the deck would be had it played on, while a detour lasts.
    timeline: Option<f64>,
    /// The hotcue whose key holds a detour of flux.
    flux_hotcue: Option<HotcueSlot>,
    beat_tick: bool,
    /// The frame of the track heard on the frame before, while the deck plays.
    heard: Option<f64>,
    /// The frame just played passed a beat of the grid, and the beat tick is on.
    on_beat: bool,
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
            sync: false,
            snap: false,
            quantize: false,
            beat_to_keep: None,
            lock: KeyLock::new(rate.as_f64()),
            key_shift: 0.0,
            flux: false,
            reverse: false,
            timeline: None,
            flux_hotcue: None,
            beat_tick: false,
            heard: None,
            on_beat: false,
        }
    }

    /// Replaces the track. The new one starts paused at its beginning; a play or a seek
    /// that follows applies to it.
    pub(super) fn swap(&mut self, track: Option<DeckTrack>, retired: &mut Producer<Retired>) {
        // A swap that never got applied still owes its item to the retired queue.
        if let Some(superseded) = self.pending_swap.take() {
            retire(superseded, None, retired);
        }
        self.playing = false;
        self.cue_preview = false;
        self.held_hotcue = None;
        self.cup_held = false;
        self.pending_seek = None;
        self.arriving_cue = None;
        // Copied out of its box, which stays with the track until it is retired.
        let memory = track
            .as_ref()
            .and_then(|track| track.memory.as_deref())
            .copied()
            .unwrap_or_default();
        // A grid never kept stays as the deck had none, as before the collection kept them.
        self.arriving_grid = (memory.grid != BeatGrid::default()).then_some(memory.grid);
        self.looper.clear();
        self.set_cues(memory.cues);
        if let Some((seconds, rate)) = track
            .as_ref()
            .and_then(|track| Some((track.start?, track.audio.rate().as_f64())))
            .filter(|(seconds, _)| seconds.is_finite() && *seconds > 0.0)
        {
            self.pending_seek = Some(SeekTarget::Seconds(seconds));
            self.arriving_cue = Some(seconds * rate);
        }
        self.pending_swap = Some(track);
        self.drop_timeline();
    }

    /// The track the deck holds once the pending swap is applied.
    fn settled_audio(&self) -> Option<&TrackAudio> {
        match &self.pending_swap {
            Some(arriving) => arriving.as_ref().map(|track| &track.audio),
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

    /// Sets the floating cue at the play position or, with Snap, at the nearest beat; a
    /// paused deck then moves there, so the cue key plays from it.
    pub(super) fn set_cue(&mut self) {
        let Some(track) = self.settled_track() else {
            self.set_cue_at(0.0);
            return;
        };
        let at = self.settled_position().clamp(0.0, track.frames);
        let cue = self.snapped(at, track);
        self.set_cue_at(cue);
        if cue != at && !self.playing {
            self.pending_seek = Some(SeekTarget::Frame(cue));
        }
    }

    pub(super) fn set_snap(&mut self, on: bool) {
        self.snap = on;
    }

    pub(super) fn set_quantize(&mut self, on: bool) {
        self.quantize = on;
    }

    /// The grid of the track the deck holds once the pending swap is applied.
    fn settled_grid(&self) -> Option<&BeatGrid> {
        if self.pending_swap.is_some() {
            self.arriving_grid.as_ref()
        } else {
            Some(&self.grid)
        }
    }

    /// Where a cue or a loop set at `frame` goes: with Snap on, the nearest beat.
    fn snapped(&self, frame: f64, track: Track) -> f64 {
        if !self.snap {
            return frame;
        }
        self.settled_grid()
            .and_then(|grid| grid.nearest_beat(frame / track.rate, track.frames / track.rate))
            .map_or(frame, |seconds| {
                (seconds * track.rate).clamp(0.0, track.frames)
            })
    }

    /// Where a playing deck at `at` lands when it goes to a cue at `cue`: with Quantize on,
    /// as far into the beat as it is now. `loop_frames` is the length of a stored loop,
    /// which the deck has to land inside.
    fn quantized(&self, cue: f64, at: f64, loop_frames: Option<f64>, track: Track) -> f64 {
        let (true, Some(beat), Some(grid)) = (self.quantize, track.beat, self.settled_grid())
        else {
            return cue;
        };
        let phase = |frame: f64| grid.phase_at(frame / track.rate);
        let (Some(cue_phase), Some(now)) = (phase(cue), phase(at)) else {
            return cue;
        };
        let mut offset = beats_between(cue_phase, now);
        if let Some(length) = loop_frames {
            if length < beat {
                return cue;
            }
            if offset < 0.0 {
                offset += 1.0;
            }
        }
        let mut target = cue + offset * beat;
        if target < 0.0 {
            target += beat;
        }
        if target > track.frames {
            target -= beat;
        }
        target.clamp(0.0, track.frames)
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
        self.drop_timeline();
    }

    pub(super) fn seek_seconds(&mut self, seconds: f64) {
        if seconds.is_finite() && self.will_be_loaded() {
            self.pending_seek = Some(SeekTarget::Seconds(seconds));
            self.drop_timeline();
        }
    }

    pub(super) fn toggle_flux(&mut self) {
        self.flux = !self.flux;
        if !self.flux {
            self.flux_hotcue = None;
            if !self.reverse {
                self.timeline = None;
            }
        }
    }

    pub(super) fn is_flux_on(&self) -> bool {
        self.flux
    }

    pub(super) fn is_reversed(&self) -> bool {
        self.reverse
    }

    /// The reverse button. Pressing it starts a timeline; releasing it returns to the
    /// timeline when no other detour holds it. The turn crossfades what was heard, going on
    /// in its old direction, under what the deck plays now.
    pub(super) fn reverse(&mut self, pressed: bool) {
        if pressed == self.reverse || (pressed && !self.will_be_loaded()) {
            return;
        }
        let heard = Tail::over(
            self.position,
            SPLICE_SECONDS * self.engine_rate,
            !self.reverse,
        );
        self.reverse = pressed;
        if pressed {
            self.start_timeline(self.settled_position());
        } else {
            self.end_detour();
        }
        if self.playing && self.pending_swap.is_none() && self.pending_seek.is_none() {
            self.tail = heard;
            if pressed && let Some(audio) = &self.audio {
                let speed = f64::from(self.tempo.target()) * (1.0 + f64::from(self.nudge.target()));
                self.lock.turn_back(audio.samples(), speed);
            }
        }
    }

    /// Whether something still keeps the timeline: reverse, the key of a hotcue, or a loop
    /// with flux on.
    fn detour_held(&self) -> bool {
        self.reverse || self.flux_hotcue.is_some() || (self.flux && self.looper.region().is_some())
    }

    fn start_timeline(&mut self, at: f64) {
        if self.timeline.is_none() {
            self.timeline = Some(at);
        }
    }

    /// Goes to the timeline once no detour keeps it any more.
    fn end_detour(&mut self) {
        if self.detour_held() {
            return;
        }
        if let Some(timeline) = self.timeline.take()
            && (timeline - self.settled_position()).abs() >= 1.0
        {
            self.jump_to(timeline);
        }
    }

    /// Ends the timeline without going there; reverse held starts its own from where the
    /// deck will be.
    fn drop_timeline(&mut self) {
        self.flux_hotcue = None;
        self.timeline = self.reverse.then(|| self.settled_position());
    }

    /// Where the timeline is, in seconds, while there is one.
    pub(super) fn timeline_seconds(&self) -> Option<f64> {
        self.timeline.map(|frame| self.seconds(frame))
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
            GridEdit::Analysed { bpm, first_beat } => {
                grid.is_empty() && grid.set_bpm_from(bpm, first_beat.clamp(0.0, end))
            }
            GridEdit::Reset(found) => {
                grid.replace(found.map(|(bpm, first_beat)| (bpm, first_beat.clamp(0.0, end))))
            }
            GridEdit::ResetBpm { bpm, first_beat } => {
                grid.set_bpm_from(bpm, first_beat.clamp(0.0, end))
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
        let placed = match edit {
            LoopEdit::In | LoopEdit::Out | LoopEdit::Toggle { .. } => self.snapped(at, track),
            LoopEdit::Resize { .. } | LoopEdit::Move { .. } => at,
        };
        let before = self.looper.region();
        let mut target = self.looper.edit(edit, placed, track);
        let looping = self.looper.region().is_some();
        if looping && self.flux && self.playing && !self.cue_preview {
            self.start_timeline(at);
        }
        // An end that Snap put elsewhere than the play position: the deck plays on to it,
        // or, when it is already past it, goes on from where it is counted around the loop.
        if edit == LoopEdit::Out && placed != at {
            let after = self.looper.region();
            target = after.filter(|_| after != before).and_then(|region| {
                (at >= region.end && at >= region.start)
                    .then(|| region.start + (at - region.start).rem_euclid(region.frames()))
            });
        }
        if let Some(target) = target {
            self.jump_to(target);
        }
        if !looping {
            self.end_detour();
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
            self.tail = Tail::over(
                self.position,
                SPLICE_SECONDS * self.engine_rate,
                !self.reverse,
            );
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
                } else if self.flux_hotcue == Some(slot) {
                    self.flux_hotcue = None;
                    if cues.get(slot).and_then(Cue::region).is_some() {
                        self.looper.clear();
                    }
                    self.end_detour();
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
            None => Cue::at(self.snapped(at, track) / track.rate),
        }
    }

    /// Goes to a stored cue and switches on the loop it stores. A deck that plays goes on
    /// from there. A paused one moves there and, under the key of a hotcue, takes the cue as
    /// its floating cue and plays until that key is released, as under the cue button.
    fn go_to_cue(&mut self, cue: Cue, track: Track, key: Option<HotcueSlot>) {
        let frame = (cue.position * track.rate).clamp(0.0, track.frames);
        let mut loop_frames = None;
        if let Some(region) = cue.region() {
            // The end went through seconds and back: it may be a hair past the track.
            let end = (region.end * track.rate).min(track.frames);
            self.looper.set_region(Region { start: frame, end }, track);
            loop_frames = Some(end - frame);
        }
        if self.playing && !self.cue_preview {
            let at = self.settled_position().clamp(0.0, track.frames);
            if self.flux && (key.is_some() || loop_frames.is_some()) {
                self.start_timeline(at);
                if key.is_some() {
                    self.flux_hotcue = key;
                }
            }
            self.jump_to(self.quantized(frame, at, loop_frames, track));
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

    pub(super) fn toggle_sync(&mut self) {
        self.sync = !self.sync;
    }

    pub(super) fn is_synced(&self) -> bool {
        self.sync
    }

    pub(super) fn toggle_key_lock(&mut self) {
        self.lock.toggle();
        // Turned on again, key lock keeps the original key.
        if !self.lock.is_on() {
            self.shift_key(0.0);
        }
    }

    pub(super) fn is_key_locked(&self) -> bool {
        self.lock.is_on()
    }

    pub(super) fn toggle_beat_tick(&mut self) {
        self.beat_tick = !self.beat_tick;
    }

    pub(super) fn has_beat_tick(&self) -> bool {
        self.beat_tick
    }

    /// The frame just played passed a beat and the beat tick is on.
    pub(super) fn on_beat(&self) -> bool {
        self.on_beat
    }

    /// Whether the deck, heard at frame `at` of the track after `before` on the frame
    /// before, passed a beat of its grid. `step` is how far the deck moves in one frame.
    /// A move of more than a quarter of a beat, or against the direction of play, is a
    /// jump: it passes a beat only when it lands on one, within one step.
    fn passes_a_beat(&self, at: f64, before: Option<f64>, step: f64, rate: f64) -> bool {
        let (Some(bpm), Some(phase), Some((_, run_end))) = (
            self.grid.bpm(),
            self.grid.phase_at(at / rate),
            self.grid.run_at(at / rate),
        ) else {
            return false;
        };
        let beat = bpm.beat_seconds() * rate;
        // The beat on or before the position is `since` frames back; the next one is the
        // next beat of the run or the marker that starts the next run.
        let since = phase * beat;
        let next = if phase == 0.0 {
            at
        } else {
            (at + (1.0 - phase) * beat).min(run_end * rate)
        };
        let moved = before.map(|before| {
            if self.reverse {
                before - at
            } else {
                at - before
            }
        });
        match moved {
            Some(moved) if moved >= 0.0 && moved < beat / 4.0 => {
                if self.reverse {
                    next < at + moved
                } else {
                    since < moved
                }
            }
            _ if self.reverse => next - at < step,
            _ => since < step,
        }
    }

    pub(super) fn step_key(&mut self, up: bool) {
        self.lock_key(self.key_shift + if up { 1.0 } else { -1.0 });
    }

    pub(super) fn set_key(&mut self, semitones: f64) {
        self.lock_key(semitones);
    }

    /// Keeps the key the deck has at its speed from now on.
    pub(super) fn persist_key(&mut self) {
        self.lock_key(12.0 * self.set_speed().log2());
    }

    fn lock_key(&mut self, semitones: f64) {
        if semitones.is_nan() {
            return;
        }
        if !self.lock.is_on() {
            self.lock.toggle();
        }
        self.shift_key(semitones.clamp(-MAX_KEY_SHIFT, MAX_KEY_SHIFT));
    }

    fn shift_key(&mut self, semitones: f64) {
        self.key_shift = semitones;
        self.lock.set_pitch((semitones / 12.0).exp2());
    }

    pub(super) fn key_shift(&self) -> f64 {
        self.key_shift
    }

    /// Pitch the deck plays against the original, in semitones: the shift while key lock acts,
    /// the pitch of the speed otherwise, in reverse too.
    pub(super) fn pitch_semitones(&self) -> f64 {
        let speed = self.set_speed();
        if !self.reverse && self.lock.acts_at(speed) {
            self.key_shift
        } else {
            12.0 * speed.log2()
        }
    }

    /// Speed the deck is set to, bend included, without the pull of sync.
    fn set_speed(&self) -> f64 {
        f64::from(self.tempo.target()) * (1.0 + f64::from(self.nudge.target()))
    }

    /// Speed the deck is set to, as a ratio of the original.
    pub(super) fn speed(&self) -> f32 {
        self.tempo.target()
    }

    /// What the choice of the tempo master needs to know, about the track the deck holds
    /// once the pending swap is applied.
    pub(super) fn candidate(&self) -> Candidate {
        let eligible = self
            .settled_track()
            .is_some_and(|track| track.beat.is_some());
        Candidate {
            eligible,
            live: eligible && self.playing && !self.cue_preview,
            synced: self.sync,
        }
    }

    /// The tempo of the deck and the place in its beat, when its grid has a tempo.
    fn pulse(&self) -> Option<Pulse> {
        if self.pending_swap.is_some() {
            return None;
        }
        let audio = self.audio.as_ref()?;
        let speed = f64::from(self.tempo.target()) * (1.0 + f64::from(self.nudge.target()));
        // A deck on a detour of flux keeps the beat of the track as it would have played on.
        let at = self.timeline.unwrap_or(self.position);
        Some(Pulse {
            bpm: self.grid.bpm()?.value() * speed,
            phase: self.grid.phase_at(at / audio.rate().as_f64())?,
        })
    }

    /// What the deck gives the clock as its tempo master.
    pub(super) fn lead(&self) -> Option<Lead> {
        Some(Lead {
            pulse: self.pulse()?,
            // A deck held playing by a cue key is on its way back to the cue.
            keeps_the_beat: self.playing
                && !self.cue_preview
                && (self.timeline.is_some() || self.loop_keeps_the_beat()),
        })
    }

    /// A loop that is not a whole number of beats long takes the deck off the beat on
    /// every pass.
    fn loop_keeps_the_beat(&self) -> bool {
        let (Some(region), Some(track)) = (self.looper.region(), self.settled_track()) else {
            return true;
        };
        track.beat.is_none_or(|beat| {
            let beats = region.frames() / beat;
            beats.round() >= 1.0 && (beats - beats.round()).abs() < WHOLE_BEATS_TOLERANCE
        })
    }

    /// Whether a clock sets the speed of the deck: its sync is on and its grid has a tempo.
    /// The tempo master is told apart by the processor.
    pub(super) fn can_follow(&self) -> bool {
        self.sync && self.pulse().is_some()
    }

    /// Sets the speed of a synced deck to the tempo of the clock for the next frame, and
    /// with `keep_beat` has it keep its beats on those of the clock. Without a clock the
    /// deck follows nothing.
    pub(super) fn follow(&mut self, clock: Option<Pulse>, keep_beat: bool) {
        self.beat_to_keep = None;
        let (Some(clock), true, None) = (clock, self.sync, &self.pending_swap) else {
            return;
        };
        let Some(bpm) = self.grid.bpm() else {
            return;
        };
        let ratio = clock.bpm / bpm.value();
        if !ratio.is_finite() {
            return;
        }
        let reachable = (Tempo::MIN_RATIO..=Tempo::MAX_RATIO).contains(&ratio);
        self.tempo
            .set(ratio.clamp(Tempo::MIN_RATIO, Tempo::MAX_RATIO) as f32);
        // A deck that cannot reach the tempo cannot stay on the beat either.
        if keep_beat && reachable {
            self.beat_to_keep = Some(clock.phase);
        }
    }

    /// How far the beat of the deck is from that of the clock, in beats: negative behind,
    /// positive ahead.
    pub(super) fn beats_ahead(&self, clock: Pulse) -> Option<f64> {
        Some(beats_between(clock.phase, self.pulse()?.phase))
    }

    /// Keeps the beats of the deck on those of the clock: a large offset by moving there,
    /// a small one by the factor on the speed this returns.
    fn pull_to_the_beat(&mut self) -> f64 {
        let Some(clock) = self.beat_to_keep else {
            return 1.0;
        };
        // A deck held playing by a cue key plays from its cue, wherever the beat is.
        let steady = self.playing
            && !self.cue_preview
            && self.pending_swap.is_none()
            && self.pending_seek.is_none()
            && self.nudge.target() == 0.0
            && self.nudge.is_settled();
        if !steady || !self.loop_keeps_the_beat() {
            return 1.0;
        }
        let (Some(track), Some(pulse)) = (self.settled_track(), self.pulse()) else {
            return 1.0;
        };
        let Some(beat) = track.beat else {
            return 1.0;
        };
        let behind = beats_between(pulse.phase, clock);
        if behind.abs() <= ALIGN_BEATS {
            return 1.0 + (behind * PULL_PER_BEAT).clamp(-MAX_PULL, MAX_PULL);
        }
        // One move at a time: the crossfade of the one before has to end first. And none
        // while the speed still ramps to the tempo of the clock: the ramp would take the
        // deck off the beat again.
        if self.tail.is_none()
            && self.tempo.is_settled()
            && let Some(target) = self.on_the_beat(self.position + behind * beat, beat, track)
        {
            self.jump_to(target);
        }
        1.0
    }

    /// The frame nearest to `target` with the same place in the beat that the deck can go
    /// to: inside the track, inside the run of beats the deck is in, whose beats are the
    /// ones the offset was measured on, and inside the loop that holds the deck.
    fn on_the_beat(&self, target: f64, beat: f64, track: Track) -> Option<f64> {
        let (start, end) = self.grid.run_at(self.position / track.rate)?;
        let run = (start * track.rate).max(0.0)..(end * track.rate).min(track.frames);
        let inside = |frame: f64| run.contains(&frame);
        let target = [target, target + beat, target - beat]
            .into_iter()
            .find(|frame| inside(*frame))?;
        let Some(region) = self.looper.region() else {
            return Some(target);
        };
        if !(region.start..region.end).contains(&self.position) {
            return Some(target);
        }
        // The loop is a whole number of beats long, so its length keeps the place.
        let length = region.frames();
        [target, target - length, target + length]
            .into_iter()
            .find(|frame| (region.start..region.end).contains(frame))
    }

    pub(super) fn set_nudge(&mut self, amount: f32) {
        self.nudge.set(amount);
    }

    /// Produces the next frame of the deck.
    pub(super) fn next(&mut self, retired: &mut Producer<Retired>) -> Frame {
        let audible = self.playing && self.pending_swap.is_none() && self.pending_seek.is_none();
        self.fade.set(if audible { 1.0 } else { 0.0 });
        let gain = self.fade.advance();
        self.on_beat = false;
        if !audible {
            self.heard = None;
        }
        if gain == 0.0 {
            self.tail = None;
            self.lock.silence();
            // A change of speed needs no ramp while nothing sounds.
            self.tempo.advance();
            self.apply_pending(retired);
            return [0.0; 2];
        }
        let reverse = self.reverse;
        // On a detour the timeline keeps the beat; the deck comes back to it.
        let pull = if reverse || self.timeline.is_some() {
            1.0
        } else {
            self.pull_to_the_beat()
        };
        let Some(audio) = &self.audio else {
            return [0.0; 2];
        };
        let speed =
            f64::from(self.tempo.advance()) * (1.0 + f64::from(self.nudge.advance())) * pull;
        let step = speed * audio.rate().as_f64() / self.engine_rate;
        let locked = if reverse {
            self.lock.silence();
            None
        } else {
            self.lock.play(audio.samples(), self.position, step, speed)
        };
        let mut frame = match locked {
            Some(locked) => {
                // The stretch says where the deck is: its tempo is the one asked, its
                // position is what is heard.
                self.position = locked.position;
                if locked.level >= 1.0 {
                    locked.frame
                } else {
                    let plain = read(audio, self.position);
                    [0, 1]
                        .map(|side| plain[side] + (locked.frame[side] - plain[side]) * locked.level)
                }
            }
            // Backwards at the start of the track there is nothing to read.
            None if reverse && self.position <= 0.0 => [0.0; 2],
            None => read(audio, self.position),
        };
        if audible {
            let heard = self.position;
            self.on_beat = self.beat_tick
                && self.passes_a_beat(heard, self.heard, step, audio.rate().as_f64());
            self.heard = Some(heard);
        }
        if let Some(tail) = &mut self.tail {
            tail.level -= tail.decay;
            if tail.level > 0.0 {
                // What was heard before the jump: the stretch, when it was playing forward.
                let old = if !tail.forward {
                    read(audio, tail.position)
                } else {
                    self.lock
                        .tail_frame()
                        .unwrap_or_else(|| read(audio, tail.position))
                };
                frame = [0, 1].map(|side| frame[side] + (old[side] - frame[side]) * tail.level);
                tail.position += if tail.forward { step } else { -step };
            } else {
                self.tail = None;
            }
        }
        let end = audio.frames() as f64;
        if let Some(timeline) = &mut self.timeline {
            *timeline = (*timeline + step).min(end);
        }
        let before = self.position;
        let after = if reverse {
            before - step
        } else {
            before + step
        };
        // The loop belongs to the track that arrives while a swap waits.
        let wrapped = if self.pending_swap.is_some() {
            None
        } else if reverse {
            self.looper.wrap_back(before, after)
        } else {
            self.looper.wrap(before, after)
        };
        if let Some((start, length)) = wrapped {
            // At most half a pass of the loop, so one crossfade ends before the next.
            let frames = (SPLICE_SECONDS * self.engine_rate).min(length / (2.0 * step));
            self.tail = Tail::over(after, frames, !reverse);
            self.position = start;
        } else {
            self.position = after.max(0.0);
        }
        if self.position >= end {
            self.position = end;
            self.playing = false;
        }
        [frame[0] * gain, frame[1] * gain]
    }

    fn apply_pending(&mut self, retired: &mut Producer<Retired>) {
        if let Some(arriving) = self.pending_swap.take() {
            let (audio, stretch, memory) = arriving.map_or((None, None, None), |track| {
                (Some(track.audio), track.stretch, track.memory)
            });
            let previous = std::mem::replace(&mut self.audio, audio);
            let previous_stretch = self.lock.replace(stretch);
            retire(
                previous.map(|audio| DeckTrack {
                    audio,
                    stretch: previous_stretch,
                    memory: None,
                    start: None,
                }),
                memory,
                retired,
            );
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
fn retire(
    track: Option<DeckTrack>,
    memory: Option<Box<TrackMemory>>,
    retired: &mut Producer<Retired>,
) {
    if let Err(rtrb::PushError::Full(item)) = retired.push(Retired(track, memory)) {
        std::mem::forget(item);
    }
}
