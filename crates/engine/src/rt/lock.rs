//! Key lock of a deck: plays the stretch of its track, and bridges with the plain read of the
//! track while the stretch gets ready after a jump.
//!
//! Behaviour note: `docs/behavior/keylock.md`; the stretch and why it needs the bridge:
//! `docs/decisions/0021-key-lock-stretch.md`.

use std::fmt;

use platter_dsp::Frame;
use platter_dsp::stretch::Stretch;

/// Time from a jump to the moment the stretch takes over. Its work after a seek is about 310
/// steps, spread one step every so many frames so that it ends with a third of this time to
/// spare: every third frame at 48 kHz.
const AHEAD_SECONDS: f64 = 0.032;
/// Frames of output per step of that work, for each 32 ms at 48 kHz: 1536 / 512.
const FRAMES_PER_STEP_DIVISOR: f64 = 512.0;
/// Length of the crossfade between the plain read and the stretch.
const JOIN_SECONDS: f64 = 0.005;
/// What the stretch was playing when the deck jumped, for the deck to fade it out under the
/// new place: as long as the splice of the deck, 5 ms, up to 192 kHz.
const TAIL_SECONDS: f64 = 0.005;
const TAIL_CAPACITY: usize = 1024;
/// A deck this many frames of its track away from where it would have been has jumped.
const JUMP_FRAMES: f64 = 0.5;
/// Tempos the stretch plays, as ratios of the original speed, with a margin inside its own
/// range of 0.03125 to 4.
const MIN_TEMPO: f64 = 0.04;
const MAX_TEMPO: f64 = 3.9;

/// The stretch of a track, built for it off the audio thread.
pub struct Stretcher(pub Box<dyn Stretch>);

impl fmt::Debug for Stretcher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Stretcher")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// Key lock is off, or there is no stretch to play.
    Off,
    /// The stretch has to start again where the deck is.
    Waiting,
    /// The stretch does its work for `target`, a frame of the track the deck gets to later.
    Preparing {
        target: f64,
        count: u32,
        ready: bool,
    },
    /// The stretch plays, at `level` in the crossfade from the plain read.
    Playing { level: f32 },
    /// Key lock was turned off: the stretch fades out under the plain read.
    Leaving { level: f32 },
}

/// What key lock plays for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Locked {
    pub frame: Frame,
    /// The frame of the track it plays, which becomes the position of the deck.
    pub position: f64,
    /// Share of the stretch against the plain read, from 0 to 1.
    pub level: f32,
}

pub struct KeyLock {
    on: bool,
    /// Pitch the stretch plays, as a ratio of the original: the shift of the key.
    pitch: f64,
    stretch: Option<Stretcher>,
    state: State,
    /// Where the deck is on the next frame unless something moves it.
    expected: f64,
    join_step: f32,
    ahead_frames: f64,
    prepare_every: u32,
    /// What the stretch went on to play after the last jump, and how much of it was taken.
    tail: [Frame; TAIL_CAPACITY],
    tail_frames: usize,
    tail_length: usize,
    tail_taken: usize,
}

impl KeyLock {
    pub fn new(engine_rate: f64) -> Self {
        Self {
            on: false,
            pitch: 1.0,
            stretch: None,
            state: State::Off,
            expected: 0.0,
            join_step: (1.0 / (JOIN_SECONDS * engine_rate).max(1.0)) as f32,
            ahead_frames: AHEAD_SECONDS * engine_rate,
            prepare_every: (AHEAD_SECONDS * engine_rate / FRAMES_PER_STEP_DIVISOR).max(1.0) as u32,
            tail: [[0.0; 2]; TAIL_CAPACITY],
            tail_frames: ((TAIL_SECONDS * engine_rate).ceil() as usize + 2).min(TAIL_CAPACITY),
            tail_length: 0,
            tail_taken: 0,
        }
    }

    /// The next frame of what the stretch was playing before the last jump, while the deck
    /// fades it out; `None` when the stretch was not playing then.
    pub fn tail_frame(&mut self) -> Option<Frame> {
        let frame = self
            .tail
            .get(self.tail_taken)
            .filter(|_| self.tail_taken < self.tail_length);
        self.tail_taken += 1;
        frame.copied()
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Key lock is on with a stretch that plays at `speed`.
    pub fn acts_at(&self, speed: f64) -> bool {
        self.on && self.stretch.is_some() && (MIN_TEMPO..=MAX_TEMPO).contains(&speed)
    }

    /// Sets the pitch the stretch plays, as a ratio of the original.
    pub fn set_pitch(&mut self, pitch: f64) {
        if pitch.is_finite() && pitch > 0.0 {
            self.pitch = pitch;
        }
    }

    pub fn toggle(&mut self) {
        self.on = !self.on;
        self.state = match (self.on, self.state) {
            (true, _) => State::Waiting,
            (false, State::Playing { level } | State::Leaving { level }) => {
                State::Leaving { level }
            }
            (false, _) => State::Off,
        };
    }

    /// Puts in the stretch of the track the deck now holds and gives back the one before.
    pub fn replace(&mut self, stretch: Option<Stretcher>) -> Option<Stretcher> {
        self.state = if self.on { State::Waiting } else { State::Off };
        self.tail_length = 0;
        std::mem::replace(&mut self.stretch, stretch)
    }

    /// Nothing of the deck sounds: the stretch starts again when it does.
    pub fn silence(&mut self) {
        self.state = match self.state {
            State::Leaving { .. } | State::Off => State::Off,
            _ => State::Waiting,
        };
    }

    /// The deck turns to play backwards, where the stretch cannot follow. As at a jump, the
    /// stretch goes on a little where it was, for the deck to fade out what was heard, and
    /// starts again when the deck plays forward.
    pub fn turn_back(&mut self, track: &[f32], speed: f64) {
        self.tail_length = 0;
        self.tail_taken = 0;
        if let (Some(stretch), State::Playing { .. }) = (self.stretch.as_mut(), self.state)
            && (MIN_TEMPO..=MAX_TEMPO).contains(&speed)
        {
            while self.tail_length < self.tail_frames {
                let Some((frame, _)) = stretch.0.next(track, speed, self.pitch) else {
                    break;
                };
                self.tail[self.tail_length] = frame;
                self.tail_length += 1;
            }
        }
        self.silence();
    }

    /// The frame key lock plays for a deck at `position` that moves `step` frames of its
    /// track per frame of output at `speed`; `None` where the plain read plays alone.
    pub fn play(&mut self, track: &[f32], position: f64, step: f64, speed: f64) -> Option<Locked> {
        let stretch = &mut self.stretch.as_mut()?.0;
        if !(MIN_TEMPO..=MAX_TEMPO).contains(&speed) || !step.is_finite() {
            self.state = match self.state {
                State::Leaving { .. } | State::Off => State::Off,
                _ => State::Waiting,
            };
            return None;
        }
        let jumped = (position - self.expected).abs() > JUMP_FRAMES;
        if jumped {
            self.tail_length = 0;
            self.tail_taken = 0;
        }
        if jumped && let State::Playing { .. } = self.state {
            // The stretch goes on a little where it was, so the deck fades out what was heard.
            while self.tail_length < self.tail_frames {
                let Some((frame, _)) = stretch.next(track, speed, self.pitch) else {
                    break;
                };
                self.tail[self.tail_length] = frame;
                self.tail_length += 1;
            }
        }
        if jumped && matches!(self.state, State::Preparing { .. } | State::Playing { .. }) {
            self.state = State::Waiting;
        }
        if jumped && matches!(self.state, State::Leaving { .. }) {
            self.state = State::Off;
        }
        if self.state == State::Waiting {
            let target = position + self.ahead_frames * step;
            stretch.seek(target, speed, self.pitch);
            self.state = State::Preparing {
                target,
                count: 0,
                ready: false,
            };
        }
        if let State::Preparing {
            target,
            count,
            ready,
        } = self.state
        {
            let count = count.wrapping_add(1);
            let ready = ready || (count % self.prepare_every == 0 && stretch.prepare(track));
            if ready && position >= target - step / 2.0 {
                self.state = State::Playing { level: 0.0 };
            } else {
                // Arrived before the stretch was ready: it starts again further on.
                self.state = if position > target + step {
                    State::Waiting
                } else {
                    State::Preparing {
                        target,
                        count,
                        ready,
                    }
                };
                self.expected = position + step;
                return None;
            }
        }
        let level = match self.state {
            State::Playing { level } => (level + self.join_step).min(1.0),
            State::Leaving { level } => level - self.join_step,
            _ => {
                self.expected = position + step;
                return None;
            }
        };
        let Some((frame, at)) = stretch.next(track, speed, self.pitch) else {
            self.state = if self.on { State::Waiting } else { State::Off };
            self.expected = position + step;
            return None;
        };
        self.state = match self.state {
            State::Leaving { .. } if level <= 0.0 => State::Off,
            State::Leaving { .. } => State::Leaving { level },
            _ => State::Playing { level },
        };
        self.expected = at + step;
        (level > 0.0).then_some(Locked {
            frame,
            position: at,
            level,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A stretch that reads the track at the nearest frame, ready after `work` steps.
    struct Fake {
        position: f64,
        work: u32,
        done: u32,
        seeks: Arc<AtomicUsize>,
    }

    impl Stretch for Fake {
        fn seek(&mut self, frame: f64, _: f64, _: f64) {
            self.position = frame;
            self.done = 0;
            self.seeks.fetch_add(1, Ordering::Relaxed);
        }
        fn prepare(&mut self, _: &[f32]) -> bool {
            self.done += 1;
            self.done >= self.work
        }
        fn next(&mut self, track: &[f32], tempo: f64, _: f64) -> Option<(Frame, f64)> {
            if self.done < self.work {
                return None;
            }
            let at = self.position;
            let index = at.round() as usize * 2;
            let frame = [*track.get(index)?, *track.get(index + 1)?];
            self.position += tempo;
            Some((frame, at))
        }
    }

    /// A lock holding a fake stretch, and the count of its seeks.
    fn lock(work: u32) -> (KeyLock, Arc<AtomicUsize>) {
        let seeks = Arc::new(AtomicUsize::new(0));
        let mut lock = KeyLock::new(48_000.0);
        lock.replace(Some(Stretcher(Box::new(Fake {
            position: 0.0,
            work,
            done: 0,
            seeks: Arc::clone(&seeks),
        }))));
        (lock, seeks)
    }

    /// Plays `frames` frames at `speed` from `start`, as the deck does, and returns the
    /// position and the share of the stretch for each.
    fn run(
        lock: &mut KeyLock,
        track: &[f32],
        start: f64,
        speed: f64,
        frames: usize,
    ) -> Vec<(f64, f32)> {
        let mut position = start;
        let mut out = Vec::new();
        for _ in 0..frames {
            let level = match lock.play(track, position, speed, speed) {
                Some(locked) => {
                    position = locked.position;
                    locked.level
                }
                None => 0.0,
            };
            out.push((position, level));
            position += speed;
        }
        out
    }

    #[test]
    fn the_plain_read_bridges_until_the_stretch_takes_over_where_the_deck_is() {
        let track: Vec<f32> = (0..200_000).flat_map(|i| [i as f32, i as f32]).collect();
        let (mut lock, _) = lock(300);
        lock.toggle();
        let played = run(&mut lock, &track, 1000.0, 1.06, 4000);
        let first = played.iter().position(|(_, level)| *level > 0.0).unwrap();
        // About 900 frames of work, then the deck reaches the place the stretch starts.
        assert!((1400..1600).contains(&first), "took over at {first}");
        let full = played.iter().position(|(_, level)| *level >= 1.0).unwrap();
        assert!(
            full - first <= 241,
            "the crossfade took {} frames",
            full - first
        );
        for pair in played.windows(2) {
            let moved = pair[1].0 - pair[0].0;
            assert!((moved - 1.06).abs() < 0.6, "the position moved by {moved}");
        }
    }

    #[test]
    fn a_jump_starts_the_stretch_again() {
        let track = vec![0.0; 400_000];
        let (mut lock, seeks) = lock(10);
        lock.toggle();
        run(&mut lock, &track, 0.0, 1.0, 3000);
        assert_eq!(seeks.load(Ordering::Relaxed), 1);
        run(&mut lock, &track, 50_000.0, 1.0, 3000);
        assert_eq!(seeks.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_speed_the_stretch_cannot_play_leaves_the_plain_read() {
        let track = vec![0.0; 400_000];
        let (mut lock, seeks) = lock(10);
        lock.toggle();
        assert!(run(&mut lock, &track, 0.0, 1.0, 3000).last().unwrap().1 >= 1.0);
        assert!(lock.play(&track, 3000.0, 0.0, 0.0).is_none());
        // Back in range, it prepares from where the deck is.
        assert!(run(&mut lock, &track, 3000.0, 1.0, 3000).last().unwrap().1 >= 1.0);
        assert_eq!(seeks.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn turning_key_lock_off_fades_the_stretch_out() {
        let track = vec![0.0; 400_000];
        let (mut lock, _) = lock(10);
        lock.toggle();
        let played = run(&mut lock, &track, 0.0, 1.0, 3000);
        lock.toggle();
        let start = played.last().unwrap().0 + 1.0;
        let after = run(&mut lock, &track, start, 1.0, 300);
        assert!(after[0].1 > 0.9 && after[0].1 < 1.0);
        let off = after.iter().position(|(_, level)| *level == 0.0).unwrap();
        assert!((200..=241).contains(&off), "faded out in {off} frames");
        assert!(lock.play(&track, 5000.0, 1.0, 1.0).is_none());
    }

    #[test]
    fn without_key_lock_or_without_a_stretch_nothing_is_locked() {
        let track = vec![0.0; 400_000];
        let (mut lock, _) = lock(1);
        assert!(
            run(&mut lock, &track, 0.0, 1.0, 3000)
                .iter()
                .all(|(_, level)| *level == 0.0)
        );
        let mut empty = KeyLock::new(48_000.0);
        empty.toggle();
        assert!(empty.play(&track, 0.0, 1.0, 1.0).is_none());
    }
}
