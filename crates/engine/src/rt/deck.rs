//! One deck: reads its track at the current speed and handles the transport.
//!
//! Behaviour notes: `docs/behavior/transport.md`.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::interpolate::cubic;
use platter_dsp::smooth::Smoothed;
use rtrb::Producer;

use super::{Retired, TrackAudio};

/// Length of the fade applied on play, pause, seek and track change.
const TRANSPORT_FADE_MS: f32 = 5.0;
const SPEED_RAMP_MS: f32 = 20.0;

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
        self.pending_seek = None;
        self.pending_swap = Some(audio);
    }

    /// Whether the deck holds a track once the pending swap is applied.
    fn will_be_loaded(&self) -> bool {
        match &self.pending_swap {
            Some(arriving) => arriving.is_some(),
            None => self.audio.is_some(),
        }
    }

    pub(super) fn set_playing(&mut self, playing: bool) {
        self.playing = playing && self.will_be_loaded();
    }

    pub(super) fn toggle_playing(&mut self) {
        self.set_playing(!self.playing);
    }

    pub(super) fn set_cue(&mut self) {
        if self.pending_swap.is_none() {
            self.cue = self.position;
        }
    }

    pub(super) fn goto_cue(&mut self) {
        self.playing = false;
        // The cue of a track that is still arriving is its beginning.
        let cue = if self.pending_swap.is_some() {
            0.0
        } else {
            self.cue
        };
        self.pending_seek = Some(SeekTarget::Frame(cue));
    }

    pub(super) fn seek_seconds(&mut self, seconds: f64) {
        if seconds.is_finite() && self.will_be_loaded() {
            self.pending_seek = Some(SeekTarget::Seconds(seconds));
        }
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
            self.apply_pending(retired);
            return [0.0; 2];
        }
        let Some(audio) = &self.audio else {
            return [0.0; 2];
        };
        let frame = read(audio, self.position);
        let speed = f64::from(self.tempo.advance()) * (1.0 + f64::from(self.nudge.advance()));
        self.position += speed * audio.rate().as_f64() / self.engine_rate;
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
            self.cue = 0.0;
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
