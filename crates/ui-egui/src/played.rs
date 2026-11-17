//! When a track counts as played: each deck adds up the time it spends playing the track
//! loaded in it, and the track counts once that time reaches the minimum play time.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Played state*.

use std::path::{Path, PathBuf};

use platter_core::DeckId;

/// Shortest and longest minimum play time, in seconds.
pub const MIN_PLAYTIME: u32 = 5;
pub const MAX_PLAYTIME: u32 = 600;
/// The minimum play time when the settings name none.
pub const DEFAULT_PLAYTIME: u32 = 30;
/// Most time one step may add: a longer gap between two frames is taken as this.
const MAX_STEP: f64 = 60.0;

/// A track that has just counted as played.
#[derive(Clone, Debug, PartialEq)]
pub struct Played {
    pub deck: DeckId,
    pub path: PathBuf,
    /// When its deck first played it, in seconds since 1 January 1970 UTC.
    pub started: i64,
    /// How long it had played when it counted.
    pub seconds: f64,
}

/// What one deck has played of its track.
#[derive(Debug, Default)]
struct Count {
    path: Option<PathBuf>,
    seconds: f64,
    started: Option<i64>,
    counted: bool,
}

#[derive(Debug)]
pub struct PlayClock {
    decks: [Count; DeckId::COUNT],
    minimum: f64,
}

impl PlayClock {
    /// A clock that counts a track after `minimum` seconds of play, kept within
    /// [`MIN_PLAYTIME`] and [`MAX_PLAYTIME`].
    pub fn new(minimum: u32) -> Self {
        Self {
            decks: Default::default(),
            minimum: f64::from(minimum.clamp(MIN_PLAYTIME, MAX_PLAYTIME)),
        }
    }

    /// Changes the minimum play time, kept within [`MIN_PLAYTIME`] and [`MAX_PLAYTIME`]. A track
    /// that has already played longer counts at the next call.
    pub fn set_minimum(&mut self, minimum: u32) {
        self.minimum = f64::from(minimum.clamp(MIN_PLAYTIME, MAX_PLAYTIME));
    }

    /// Takes in what each deck has loaded and whether it plays, `elapsed` seconds after the
    /// last call, at `now` seconds since 1 January 1970 UTC. Returns the tracks that have just
    /// counted as played.
    pub fn advance(
        &mut self,
        decks: [(Option<&Path>, bool); DeckId::COUNT],
        elapsed: f64,
        now: i64,
    ) -> Vec<Played> {
        let step = if elapsed.is_finite() {
            elapsed.clamp(0.0, MAX_STEP)
        } else {
            0.0
        };
        let mut played = Vec::new();
        for ((deck, count), (path, playing)) in
            DeckId::ALL.into_iter().zip(&mut self.decks).zip(decks)
        {
            // Another track, or none: the count starts again.
            if count.path.as_deref() != path {
                *count = Count {
                    path: path.map(Path::to_path_buf),
                    ..Count::default()
                };
            }
            let Some(path) = path.filter(|_| playing) else {
                continue;
            };
            let started = *count.started.get_or_insert(now);
            count.seconds += step;
            if !count.counted && count.seconds >= self.minimum {
                count.counted = true;
                played.push(Played {
                    deck,
                    path: path.to_path_buf(),
                    started,
                    seconds: count.seconds,
                });
            }
        }
        played
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use platter_core::DeckId;

    use super::{MAX_PLAYTIME, MIN_PLAYTIME, PlayClock};

    const TRACK: &str = "music/opener.wav";

    fn deck_a(clock: &mut PlayClock, track: Option<&str>, playing: bool, elapsed: f64) -> usize {
        clock
            .advance(
                [
                    (track.map(Path::new), playing),
                    (None, false),
                    (None, false),
                    (None, false),
                ],
                elapsed,
                1_000,
            )
            .len()
    }

    #[test]
    fn a_track_counts_once_its_deck_has_played_it_for_the_minimum_across_pauses() {
        let mut clock = PlayClock::new(30);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 20.0), 0);
        // Paused: the time stands still.
        assert_eq!(deck_a(&mut clock, Some(TRACK), false, 100.0), 0);
        let played = clock.advance(
            [
                (Some(Path::new(TRACK)), true),
                (None, false),
                (None, false),
                (None, false),
            ],
            10.0,
            2_000,
        );
        assert_eq!(played.len(), 1);
        assert_eq!(played[0].deck, DeckId::A);
        // It started playing at the first call.
        assert_eq!(played[0].started, 1_000);
        assert!((played[0].seconds - 30.0).abs() < 1e-9);
        // Once per load.
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 100.0), 0);
    }

    #[test]
    fn loading_again_starts_a_new_count() {
        let mut clock = PlayClock::new(30);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 40.0), 1);
        assert_eq!(deck_a(&mut clock, None, false, 1.0), 0);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 29.0), 0);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 1.0), 1);
    }

    #[test]
    fn a_long_or_broken_step_adds_little_and_the_minimum_stays_in_range() {
        let mut clock = PlayClock::new(1_000);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, f64::NAN), 0);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, -5.0), 0);
        // Ten steps of a minute reach the longest minimum.
        let mut counted = 0;
        for _ in 0..10 {
            counted += deck_a(&mut clock, Some(TRACK), true, 1e9);
        }
        assert_eq!(counted, 1);
        assert_eq!(MAX_PLAYTIME, 600);
        // A shorter minimum set while a track plays counts it at the next call.
        let mut clock = PlayClock::new(60);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 40.0), 0);
        clock.set_minimum(30);
        assert_eq!(deck_a(&mut clock, Some(TRACK), true, 0.0), 1);
        let mut short = PlayClock::new(0);
        assert_eq!(
            deck_a(&mut short, Some(TRACK), true, f64::from(MIN_PLAYTIME)),
            1
        );
    }
}
