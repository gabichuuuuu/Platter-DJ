//! The preview player: a track heard in the headphones without loading it into a deck, at
//! its own speed, with nothing else applied.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Preview player*. Like the decks it runs on
//! the audio thread: it neither allocates nor frees, and the audio it lets go of goes back
//! through the retired queue.

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::smooth::Smoothed;
use rtrb::Producer;

use super::{DeckTrack, Retired, TrackAudio};

/// How long play and pause fade, in milliseconds.
const FADE_MS: f32 = 5.0;

pub struct PreviewPlayer {
    audio: Option<TrackAudio>,
    /// Play position in frames of the track.
    position: f64,
    /// Frames of the track per frame of the output.
    step: f64,
    playing: bool,
    /// The gain that fades play and pause in and out.
    gain: Smoothed,
    engine_rate: f64,
}

impl PreviewPlayer {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            audio: None,
            position: 0.0,
            step: 1.0,
            playing: false,
            gain: Smoothed::new(0.0, rate, FADE_MS),
            engine_rate: rate.as_f64(),
        }
    }

    /// Takes another track, or none, and starts it from its beginning, playing when `play`.
    pub fn swap(&mut self, track: Option<DeckTrack>, play: bool, retired: &mut Producer<Retired>) {
        let (audio, stretch, memory) = track.map_or((None, None, None), |track| {
            (Some(track.audio), track.stretch, track.memory)
        });
        let previous = std::mem::replace(&mut self.audio, audio);
        // Whatever came with the track and is not used here goes back with the old audio.
        let item = Retired(
            previous.map(|audio| DeckTrack {
                audio,
                stretch,
                memory: None,
                start: None,
            }),
            memory,
            None,
        );
        if let Err(rtrb::PushError::Full(item)) = retired.push(item) {
            std::mem::forget(item);
        }
        self.position = 0.0;
        self.step = self
            .audio
            .as_ref()
            .map_or(1.0, |audio| audio.rate().as_f64() / self.engine_rate);
        self.set_playing(play && self.audio.is_some());
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing && self.audio.is_some();
        self.gain.set(if self.playing { 1.0 } else { 0.0 });
    }

    pub fn toggle(&mut self) {
        self.set_playing(!self.playing);
    }

    /// Moves the play position to `seconds` of the track, within it.
    pub fn seek(&mut self, seconds: f64) {
        let Some(audio) = &self.audio else {
            return;
        };
        if seconds.is_finite() {
            let end = audio.frames() as f64;
            self.position = (seconds * audio.rate().as_f64()).clamp(0.0, end);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn position_seconds(&self) -> f64 {
        self.audio
            .as_ref()
            .map_or(0.0, |audio| self.position / audio.rate().as_f64())
    }

    /// The next frame, between two frames of the track.
    pub fn next(&mut self) -> Frame {
        let gain = self.gain.advance();
        let Some(audio) = &self.audio else {
            return [0.0; 2];
        };
        if gain <= 0.0 && !self.playing {
            return [0.0; 2];
        }
        let frames = audio.frames();
        let samples = audio.samples();
        let index = self.position.floor();
        let frame = index as usize;
        if frame + 1 >= frames {
            // The end of the track: the fade out runs on silence.
            self.set_playing(false);
            return [0.0; 2];
        }
        let fraction = (self.position - index) as f32;
        let at = |frame: usize, side: usize| samples.get(frame * 2 + side).copied().unwrap_or(0.0);
        let sample = |side: usize| {
            let (first, second) = (at(frame, side), at(frame + 1, side));
            (first + (second - first) * fraction) * gain
        };
        self.position += self.step;
        [sample(0), sample(1)]
    }
}
