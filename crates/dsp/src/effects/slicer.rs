//! Slicer: keeps the last bar, by the beat of the master clock, and when switched on repeats
//! the slice that was playing. Knob 1 mixes the repeated slice with the live signal, knob 2
//! rotates the slice in eighth notes, knob 3 is the slice length; key 1 plays the slice
//! backwards, key 2 takes a new bar from the live signal at each bar line.

use platter_core::SampleRate;

use super::shared::{buffer_frames, phase, step};
use super::{Controls, Tempo};
use crate::Frame;

const BEATS_PER_BAR: f64 = 4.0;
/// The slice lengths of knob 3, in beats.
const LENGTHS: [f64; 6] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0];
/// Knob 3 at the middle of the step of one beat.
pub(super) const DEFAULT_LENGTH: f32 = 3.5 / LENGTHS.len() as f32;
const ROTATIONS: usize = 8;
const ROTATION_BEATS: f64 = 0.5;
/// The edges of a slice fade over this time, so its repeats do not click.
const EDGE_MS: f32 = 2.0;

pub(super) struct Slicer {
    /// The bar being played and the bar being taken from the live signal.
    front: [Vec<f32>; 2],
    back: [Vec<f32>; 2],
    edge_frames: f32,
    /// While it repeats, the start of the repeated slice in beats of the bar; not a number
    /// for the first frame after it is switched on, when the slice is chosen.
    anchor: Option<f64>,
    last_index: usize,
}

impl Slicer {
    pub(super) fn new(rate: SampleRate) -> Self {
        let frames = buffer_frames(rate);
        Self {
            front: [vec![0.0; frames], vec![0.0; frames]],
            back: [vec![0.0; frames], vec![0.0; frames]],
            edge_frames: (rate.as_f32() * EDGE_MS / 1_000.0).max(1.0),
            anchor: None,
            last_index: 0,
        }
    }

    /// The beats of one bar that fit the buffer, halved at very slow tempos.
    fn bar(&self, tempo: Tempo) -> (f64, f32) {
        let capacity = self.front[0].len() as f32;
        let mut beats = BEATS_PER_BAR;
        let mut frames = tempo.samples_per_beat * beats as f32;
        while frames >= capacity && beats > LENGTHS[0] {
            beats /= 2.0;
            frames /= 2.0;
        }
        (beats, frames.clamp(1.0, capacity - 1.0))
    }

    fn index(at: f64, bar: (f64, f32)) -> usize {
        (phase(at, bar.0) * bar.1) as usize
    }

    pub(super) fn switched(&mut self, on: bool) {
        // The slice is chosen at the next frame, from the beat then.
        self.anchor = if on { Some(f64::NAN) } else { None };
    }

    pub(super) fn process(&mut self, input: Frame, controls: &Controls, tempo: Tempo) -> Frame {
        let [mix, rotation, length] = controls.knobs;
        let [reverse, resync] = controls.keys;
        let bar = self.bar(tempo);
        let index = Self::index(tempo.beat, bar);
        let length = LENGTHS[step(length, LENGTHS.len())].min(bar.0);

        let Some(anchor) = self.anchor else {
            for (buffer, sample) in self.front.iter_mut().zip(input) {
                buffer[index] = sample;
            }
            self.last_index = index;
            return input;
        };
        let anchor = if anchor.is_nan() {
            let start = (phase(tempo.beat, bar.0) as f64 * bar.0 / length).floor() * length;
            self.anchor = Some(start);
            start
        } else {
            anchor
        };

        if resync {
            if index < self.last_index {
                std::mem::swap(&mut self.front, &mut self.back);
            }
            for (buffer, sample) in self.back.iter_mut().zip(input) {
                buffer[index] = sample;
            }
        }
        self.last_index = index;

        let within = phase(tempo.beat, length) as f64 * length;
        let offset = if reverse { length - within } else { within };
        let rotate = step(rotation, ROTATIONS) as f64 * ROTATION_BEATS;
        let read = Self::index(anchor + rotate + offset, bar);
        let frames_in = within as f32 * tempo.samples_per_beat;
        let frames_left = (length - within) as f32 * tempo.samples_per_beat;
        let edge = (frames_in.min(frames_left) / self.edge_frames).clamp(0.0, 1.0);
        let mix = mix.clamp(0.0, 1.0);
        let mut output = input;
        for (side, sample) in output.iter_mut().enumerate() {
            let slice = self.front[side][read] * edge;
            *sample += mix * (slice - *sample);
        }
        output
    }

    pub(super) fn reset(&mut self) {
        for buffer in self.front.iter_mut().chain(self.back.iter_mut()) {
            buffer.fill(0.0);
        }
        self.anchor = None;
    }
}
