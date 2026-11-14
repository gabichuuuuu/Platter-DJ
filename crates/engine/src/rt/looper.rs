//! The loop of a deck: the stretch it repeats, or the in point that waits for its end.
//!
//! Positions are frames of the track. Behaviour notes: `docs/behavior/loops.md`.

use platter_core::LoopRegion;

use super::{LoopEdit, LoopPart, LoopState, Span};

/// Shortest loop: below the smallest preset size at the fastest tempo, and long enough for
/// the return to its start to stay a single step whatever the speed.
const MIN_LOOP_SECONDS: f64 = 0.001;

/// What an edit needs to know about the track.
#[derive(Clone, Copy, Debug)]
pub(super) struct Track {
    /// Frames per second.
    pub rate: f64,
    /// Length in frames.
    pub frames: f64,
    /// Frames one beat lasts; none on a grid without tempo.
    pub beat: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Region {
    pub start: f64,
    pub end: f64,
}

impl Region {
    pub(super) fn frames(self) -> f64 {
        self.end - self.start
    }

    fn holds(self, at: f64) -> bool {
        (self.start..self.end).contains(&at)
    }

    /// Where a position past the end is once it is counted around the loop.
    fn folded(self, at: f64) -> f64 {
        self.start + (at - self.start).rem_euclid(self.frames())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Off,
    /// The in point is set and waits for the out point.
    Open(f64),
    On(Region),
}

#[derive(Debug, Default)]
pub(super) struct Looper {
    state: State,
    /// The state changed since the control side last got it.
    unreported: bool,
}

impl Looper {
    pub(super) fn clear(&mut self) {
        self.set(State::Off);
    }

    fn set(&mut self, state: State) {
        if state != self.state {
            self.state = state;
            self.unreported = true;
        }
    }

    pub(super) fn region(&self) -> Option<Region> {
        match self.state {
            State::On(region) => Some(region),
            _ => None,
        }
    }

    /// A span in frames of the track, or nothing when it has no length here.
    pub(super) fn frames_of(&self, span: Span, track: Track) -> Option<f64> {
        let frames = match span {
            Span::Beats(beats) => beats * track.beat?,
            Span::Seconds(seconds) => seconds * track.rate,
            Span::Loop { forward, beats } => {
                let length = match self.region() {
                    Some(region) => region.frames(),
                    None => beats * track.beat?,
                };
                if forward { length } else { -length }
            }
        };
        frames.is_finite().then_some(frames)
    }

    /// Applies an edit with the play position at `at`. Returns where the play position has
    /// to go, when the edit moves it.
    pub(super) fn edit(&mut self, edit: LoopEdit, at: f64, track: Track) -> Option<f64> {
        let shortest = MIN_LOOP_SECONDS * track.rate;
        let fits = |region: Region| {
            region.start >= 0.0 && region.end <= track.frames && region.frames() >= shortest
        };
        match (edit, self.state) {
            (LoopEdit::In, _) => self.set(State::Open(at)),
            (LoopEdit::Out, State::Open(start)) => {
                let region = Region { start, end: at };
                if fits(region) {
                    self.set(State::On(region));
                    return Some(start);
                }
            }
            (LoopEdit::Out, State::On(region)) if region.holds(at) => {
                return self.replace(region, Region { end: at, ..region }, at, fits);
            }
            (LoopEdit::Toggle { .. }, State::On(_)) => self.set(State::Off),
            (LoopEdit::Toggle { beats }, _) => {
                let end = at + beats * track.beat?;
                let region = Region { start: at, end };
                if fits(region) {
                    self.set(State::On(region));
                }
            }
            (LoopEdit::Resize { beats }, State::On(region)) => {
                let end = region.start + beats * track.beat?;
                return self.replace(region, Region { end, ..region }, at, fits);
            }
            (LoopEdit::Move { part, by }, State::On(region)) => {
                let by = self.frames_of(by, track)?;
                let moved = match part {
                    LoopPart::Whole => Region {
                        start: region.start + by,
                        end: region.end + by,
                    },
                    LoopPart::In => Region {
                        start: region.start + by,
                        ..region
                    },
                    LoopPart::Out => Region {
                        end: region.end + by,
                        ..region
                    },
                };
                if part == LoopPart::Whole {
                    // The play position inside the loop travels with it.
                    if fits(moved) {
                        self.set(State::On(moved));
                        return region.holds(at).then_some(at + by);
                    }
                } else {
                    return self.replace(region, moved, at, fits);
                }
            }
            _ => {}
        }
        None
    }

    /// Puts `moved` in the place of `region` when it fits. A play position that was inside
    /// the loop and is now after its end goes back into it.
    fn replace(
        &mut self,
        region: Region,
        moved: Region,
        at: f64,
        fits: impl Fn(Region) -> bool,
    ) -> Option<f64> {
        if !moved.start.is_finite() || !moved.end.is_finite() || !fits(moved) {
            return None;
        }
        self.set(State::On(moved));
        (region.holds(at) && at >= moved.end).then(|| moved.folded(at))
    }

    /// Where a deck that played from `before` to `after` goes on from, when that step took
    /// it through the end of the loop, and the length of the loop.
    pub(super) fn wrap(&self, before: f64, after: f64) -> Option<(f64, f64)> {
        let region = self.region()?;
        (before < region.end && after >= region.end)
            .then(|| (region.folded(after), region.frames()))
    }

    /// The loop in seconds of a track of `rate` frames per second.
    pub(super) fn state(&self, rate: f64) -> LoopState {
        match self.state {
            State::Off => LoopState::default(),
            State::Open(start) => LoopState {
                in_point: Some(start / rate),
                region: None,
            },
            State::On(region) => LoopState {
                in_point: None,
                region: Some(LoopRegion {
                    start: region.start / rate,
                    end: region.end / rate,
                }),
            },
        }
    }

    pub(super) fn is_unreported(&self) -> bool {
        self.unreported
    }

    pub(super) fn reported(&mut self) {
        self.unreported = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{Looper, Region, Track};
    use crate::rt::{LoopEdit, LoopPart, Span};

    /// A track of 100 000 frames at 1 000 frames per second with beats of 500 frames.
    const TRACK: Track = Track {
        rate: 1_000.0,
        frames: 100_000.0,
        beat: Some(500.0),
    };
    const NO_TEMPO: Track = Track {
        beat: None,
        ..TRACK
    };

    fn region(start: f64, end: f64) -> Option<Region> {
        Some(Region { start, end })
    }

    fn looped(at: f64, beats: f64) -> Looper {
        let mut looper = Looper::default();
        assert_eq!(looper.edit(LoopEdit::Toggle { beats }, at, TRACK), None);
        looper
    }

    #[test]
    fn toggle_starts_a_loop_at_the_position_and_switches_it_off() {
        let mut looper = looped(1_000.0, 4.0);
        assert_eq!(looper.region(), region(1_000.0, 3_000.0));
        assert!(looper.is_unreported());
        looper.edit(LoopEdit::Toggle { beats: 4.0 }, 2_000.0, TRACK);
        assert_eq!(looper.region(), None);
    }

    #[test]
    fn a_loop_needs_a_tempo_and_room_in_the_track() {
        let mut looper = Looper::default();
        looper.edit(LoopEdit::Toggle { beats: 4.0 }, 0.0, NO_TEMPO);
        assert_eq!(looper.region(), None);
        looper.edit(LoopEdit::Toggle { beats: 4.0 }, 99_000.0, TRACK);
        assert_eq!(looper.region(), None, "it would end after the track");
        looper.edit(LoopEdit::Toggle { beats: 4.0 }, 98_000.0, TRACK);
        assert_eq!(looper.region(), region(98_000.0, 100_000.0));
    }

    #[test]
    fn in_and_out_make_a_loop_and_send_the_deck_to_its_start() {
        let mut looper = Looper::default();
        assert_eq!(looper.edit(LoopEdit::Out, 500.0, NO_TEMPO), None);
        assert_eq!(looper.edit(LoopEdit::In, 500.0, NO_TEMPO), None);
        assert_eq!(looper.state(1_000.0).in_point, Some(0.5));
        assert_eq!(
            looper.edit(LoopEdit::Out, 500.5, NO_TEMPO),
            None,
            "half a millisecond is too short"
        );
        assert_eq!(looper.edit(LoopEdit::Out, 400.0, NO_TEMPO), None);
        assert_eq!(looper.edit(LoopEdit::Out, 1_234.0, NO_TEMPO), Some(500.0));
        assert_eq!(looper.region(), region(500.0, 1_234.0));
        assert_eq!(looper.state(1_000.0).in_point, None);
        // Out inside the loop shortens it; In starts over.
        assert_eq!(looper.edit(LoopEdit::Out, 900.0, NO_TEMPO), Some(500.0));
        assert_eq!(looper.region(), region(500.0, 900.0));
        assert_eq!(looper.edit(LoopEdit::Out, 2_000.0, NO_TEMPO), None);
        looper.edit(LoopEdit::In, 2_000.0, NO_TEMPO);
        assert_eq!(looper.region(), None);
    }

    #[test]
    fn the_deck_goes_on_from_the_start_keeping_what_it_overshot() {
        let looper = looped(1_000.0, 4.0);
        assert_eq!(looper.wrap(2_990.0, 2_999.5), None);
        assert_eq!(looper.wrap(2_999.5, 3_000.0), Some((1_000.0, 2_000.0)));
        assert_eq!(looper.wrap(2_999.5, 3_001.25), Some((1_001.25, 2_000.0)));
        // A deck that is already past the end did not play through it.
        assert_eq!(looper.wrap(3_000.0, 3_001.0), None);
        assert_eq!(looper.wrap(5_000.0, 5_001.0), None);
        assert_eq!(Looper::default().wrap(0.0, 1.0e9), None);
    }

    #[test]
    fn a_new_size_keeps_the_start_and_folds_the_deck_into_the_loop() {
        let mut looper = looped(1_000.0, 4.0);
        let resize = |beats| LoopEdit::Resize { beats };
        // The deck is 1 800 past the start: three laps of the new 500 and 300 more.
        assert_eq!(looper.edit(resize(1.0), 2_800.0, TRACK), Some(1_300.0));
        assert_eq!(looper.region(), region(1_000.0, 1_500.0));
        assert_eq!(looper.edit(resize(8.0), 1_300.0, TRACK), None);
        assert_eq!(looper.region(), region(1_000.0, 5_000.0));
        // A deck that had left the loop is not pulled back.
        assert_eq!(looper.edit(resize(1.0), 9_000.0, TRACK), None);
        assert_eq!(looper.edit(resize(400.0), 1_000.0, TRACK), None);
        assert_eq!(
            looper.region(),
            region(1_000.0, 1_500.0),
            "400 beats do not fit"
        );
        assert_eq!(looper.edit(resize(2.0), 1_000.0, NO_TEMPO), None);
        assert_eq!(looper.region(), region(1_000.0, 1_500.0));
    }

    #[test]
    fn moving_the_loop_takes_the_deck_inside_it_along() {
        let mut looper = looped(1_000.0, 4.0);
        let whole = |beats| LoopEdit::Move {
            part: LoopPart::Whole,
            by: Span::Beats(beats),
        };
        assert_eq!(looper.edit(whole(2.0), 1_700.0, TRACK), Some(2_700.0));
        assert_eq!(looper.region(), region(2_000.0, 4_000.0));
        assert_eq!(looper.edit(whole(-4.0), 500.0, TRACK), None);
        assert_eq!(looper.region(), region(0.0, 2_000.0));
        assert_eq!(looper.edit(whole(-1.0), 100.0, TRACK), None);
        assert_eq!(
            looper.region(),
            region(0.0, 2_000.0),
            "it would start before the track"
        );
        let by_itself = LoopEdit::Move {
            part: LoopPart::Whole,
            by: Span::Loop {
                forward: true,
                beats: 32.0,
            },
        };
        assert_eq!(looper.edit(by_itself, 100.0, TRACK), Some(2_100.0));
        assert_eq!(looper.region(), region(2_000.0, 4_000.0));
    }

    #[test]
    fn moving_one_end_changes_the_length() {
        let mut looper = looped(1_000.0, 4.0);
        let part = |part, beats| LoopEdit::Move {
            part,
            by: Span::Beats(beats),
        };
        assert_eq!(looper.edit(part(LoopPart::In, 1.0), 1_200.0, TRACK), None);
        assert_eq!(looper.region(), region(1_500.0, 3_000.0));
        assert_eq!(looper.edit(part(LoopPart::In, 3.0), 2_000.0, TRACK), None);
        assert_eq!(looper.region(), region(1_500.0, 3_000.0), "no length left");
        // The end moves back behind the deck: 2 800 is 1 300 into a loop of 1 000.
        assert_eq!(
            looper.edit(part(LoopPart::Out, -1.0), 2_800.0, TRACK),
            Some(1_800.0)
        );
        assert_eq!(looper.region(), region(1_500.0, 2_500.0));
        let fine = LoopEdit::Move {
            part: LoopPart::Out,
            by: Span::Seconds(0.005),
        };
        assert_eq!(looper.edit(fine, 1_600.0, NO_TEMPO), None);
        assert_eq!(looper.region(), region(1_500.0, 2_505.0));
        assert_eq!(
            Looper::default().edit(part(LoopPart::Out, 1.0), 0.0, TRACK),
            None
        );
    }

    #[test]
    fn spans_have_a_length_when_the_track_can_give_one() {
        let looper = looped(0.0, 2.0);
        assert_eq!(looper.frames_of(Span::Beats(-0.5), TRACK), Some(-250.0));
        assert_eq!(looper.frames_of(Span::Beats(1.0), NO_TEMPO), None);
        assert_eq!(looper.frames_of(Span::Seconds(0.005), NO_TEMPO), Some(5.0));
        let by_loop = |forward| Span::Loop {
            forward,
            beats: 8.0,
        };
        assert_eq!(looper.frames_of(by_loop(false), NO_TEMPO), Some(-1_000.0));
        let none = Looper::default();
        assert_eq!(none.frames_of(by_loop(true), TRACK), Some(4_000.0));
        assert_eq!(none.frames_of(by_loop(true), NO_TEMPO), None);
        assert_eq!(none.frames_of(Span::Beats(f64::NAN), TRACK), None);
    }

    #[test]
    fn the_state_is_reported_in_seconds_once_per_change() {
        let mut looper = looped(1_000.0, 4.0);
        let state = looper.state(1_000.0);
        assert_eq!(
            state.region.map(|region| (region.start, region.end)),
            Some((1.0, 3.0))
        );
        looper.reported();
        looper.edit(LoopEdit::Resize { beats: 4.0 }, 1_000.0, TRACK);
        assert!(!looper.is_unreported(), "the same loop is no change");
        looper.clear();
        assert!(looper.is_unreported());
    }
}
