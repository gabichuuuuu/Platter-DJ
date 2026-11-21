//! The master clock and the choice of the tempo master.
//!
//! Behaviour notes: `docs/behavior/sync.md`.

use platter_core::{Bpm, DeckId, SampleRate};
use platter_dsp::smooth::Smoothed;

use super::deck::SPEED_RAMP_MS;

/// Tempo of the clock until something sets it.
pub(super) const INITIAL_BPM: f64 = 120.0;
/// A clock that follows another one closes a gap in its beat over this long.
const FOLLOW_MS: f64 = 200.0;
/// A gap wider than this share of a beat is closed at once: the clock followed has started
/// again, or the gap is too wide to close unheard.
const FOLLOW_JUMP: f64 = 0.25;

/// A tempo and a place in the beat: what a synced deck follows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pulse {
    /// Beats per minute, bend included.
    pub bpm: f64,
    /// How far into the beat, from zero up to one.
    pub phase: f64,
}

/// What the choice of the tempo master needs to know about a deck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Candidate {
    /// It has a track with a tempo in its grid.
    pub eligible: bool,
    /// It is eligible and plays, and not because a cue key is held.
    pub live: bool,
    pub synced: bool,
}

/// What the tempo master deck gives the clock for one frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lead {
    pub pulse: Pulse,
    /// The deck plays and its beats are the reference; otherwise only its tempo is.
    pub keeps_the_beat: bool,
}

pub(super) struct Clock {
    /// Tempo the clock has on its own. It follows a deck that is the tempo master while
    /// that deck is in the range a clock accepts, so the clock goes on from there.
    bpm: f64,
    bend: Smoothed,
    /// What the decks followed on the latest frame.
    pulse: Pulse,
    auto: bool,
    /// The deck that is the tempo master; without one, the clock is.
    master: Option<DeckId>,
    /// Which decks were live at the latest choice: a deck takes over when it starts.
    was_live: [bool; DeckId::COUNT],
    /// Minutes one frame of output lasts.
    frame_minutes: f64,
    /// While the clock follows another one: the share of a beat added to each frame, and
    /// for how many more frames, to close the gap to its beat.
    correction: f64,
    correction_frames: u32,
    follow_frames: u32,
}

impl Clock {
    pub(super) fn new(rate: SampleRate) -> Self {
        Self {
            bpm: INITIAL_BPM,
            bend: Smoothed::new(0.0, rate, SPEED_RAMP_MS),
            pulse: Pulse {
                bpm: INITIAL_BPM,
                phase: 0.0,
            },
            auto: true,
            master: None,
            was_live: [false; DeckId::COUNT],
            frame_minutes: 1.0 / (60.0 * rate.as_f64()),
            correction: 0.0,
            correction_frames: 0,
            follow_frames: (rate.as_f64() * FOLLOW_MS / 1_000.0) as u32,
        }
    }

    /// Takes the tempo of a clock it follows, and where that clock is in its beat for the
    /// frame that comes next: `docs/behavior/midi-clock.md`, *Following*. The control side
    /// makes the clock the tempo master first.
    pub(super) fn follow(&mut self, bpm: Bpm, phase: f64) {
        self.bpm = bpm.value();
        self.pulse.bpm = self.bpm;
        let gap = (phase - self.pulse.phase + 1.5).rem_euclid(1.0) - 0.5;
        if gap.abs() > FOLLOW_JUMP || self.follow_frames == 0 {
            self.pulse.phase = phase.rem_euclid(1.0);
            self.correction_frames = 0;
        } else {
            self.correction = gap / f64::from(self.follow_frames);
            self.correction_frames = self.follow_frames;
        }
    }

    pub(super) fn master(&self) -> Option<DeckId> {
        self.master
    }

    pub(super) fn pulse(&self) -> Pulse {
        self.pulse
    }

    /// Switches the automatic choice on, or makes the clock the tempo master.
    pub(super) fn set_auto(&mut self, auto: bool, decks: [Candidate; DeckId::COUNT]) {
        self.auto = auto;
        if !auto {
            self.master = None;
        } else if !self.master.is_some_and(|deck| decks[deck.index()].live) {
            self.master = DeckId::ALL
                .into_iter()
                .find(|deck| decks[deck.index()].live);
        }
        self.was_live = decks.map(|deck| deck.live);
    }

    /// Makes a deck the tempo master, while the choice is not automatic.
    pub(super) fn set_master(&mut self, deck: DeckId, candidate: Candidate) {
        if !self.auto && candidate.eligible {
            self.master = Some(deck);
        }
    }

    /// Keeps the tempo master in line with what the decks do. Called once per block.
    pub(super) fn choose(&mut self, decks: [Candidate; DeckId::COUNT]) {
        match self.master {
            Some(deck) if self.auto => {
                if !decks[deck.index()].live {
                    self.master = DeckId::ALL.into_iter().find(|other| {
                        let other = decks[other.index()];
                        other.live && other.synced
                    });
                }
            }
            Some(deck) => {
                if !decks[deck.index()].eligible {
                    self.master = None;
                }
            }
            None if self.auto => {
                self.master = DeckId::ALL
                    .into_iter()
                    .find(|deck| decks[deck.index()].live && !self.was_live[deck.index()]);
            }
            None => {}
        }
        self.was_live = decks.map(|deck| deck.live);
    }

    /// The commands of the tempo act only while the clock is the tempo master.
    pub(super) fn set_bpm(&mut self, bpm: Bpm) {
        if self.master.is_none() {
            self.bpm = bpm.value();
            // What the clock shows does not wait for the next frame.
            self.pulse.bpm = self.bpm * (1.0 + f64::from(self.bend.target()));
        }
    }

    pub(super) fn step_bpm(&mut self, amount: f64) {
        if let Ok(bpm) = Bpm::new(self.bpm + amount) {
            self.set_bpm(bpm);
        }
    }

    /// A bend is kept whoever the tempo master is, so the release of its key always ends
    /// it; it is heard only while the clock is the tempo master.
    pub(super) fn set_bend(&mut self, amount: f32) {
        self.bend.set(amount);
    }

    /// Advances one frame and returns what the decks follow for it. `lead` is what the
    /// tempo master deck gives, when a deck is the tempo master and has a tempo.
    pub(super) fn tick(&mut self, lead: Option<Lead>) -> Pulse {
        let bend = f64::from(self.bend.advance());
        let (bpm, locked) = match lead {
            Some(lead) => {
                if (Bpm::MIN..=Bpm::MAX).contains(&lead.pulse.bpm) {
                    self.bpm = lead.pulse.bpm;
                }
                let locked = lead.keeps_the_beat.then_some(lead.pulse.phase);
                (lead.pulse.bpm, locked)
            }
            None => (self.bpm * (1.0 + bend), None),
        };
        let phase = match locked {
            Some(phase) => phase,
            None => {
                let mut step = bpm * self.frame_minutes;
                if self.correction_frames > 0 {
                    self.correction_frames -= 1;
                    step += self.correction;
                }
                (self.pulse.phase + step).rem_euclid(1.0)
            }
        };
        self.pulse = Pulse { bpm, phase };
        self.pulse
    }
}

#[cfg(test)]
mod tests {
    use platter_core::{Bpm, DeckId, SampleRate};

    use super::{Candidate, Clock, Lead, Pulse};

    const A: DeckId = DeckId::A;
    const B: DeckId = DeckId::B;
    const C: DeckId = DeckId::C;
    const D: DeckId = DeckId::D;
    const STOPPED: Candidate = Candidate {
        eligible: true,
        live: false,
        synced: false,
    };
    const LIVE: Candidate = Candidate {
        eligible: true,
        live: true,
        synced: false,
    };
    const SYNCED: Candidate = Candidate {
        eligible: true,
        live: true,
        synced: true,
    };
    const EMPTY: Candidate = Candidate {
        eligible: false,
        live: false,
        synced: false,
    };

    fn clock() -> Clock {
        Clock::new(SampleRate::new(48_000).unwrap())
    }

    fn chosen(clock: &mut Clock, decks: [Candidate; DeckId::COUNT]) -> Option<DeckId> {
        clock.choose(decks);
        clock.master()
    }

    #[test]
    fn in_auto_the_deck_that_starts_takes_over_from_the_clock() {
        let mut clock = clock();
        assert_eq!(chosen(&mut clock, [STOPPED, STOPPED, EMPTY, EMPTY]), None);
        assert_eq!(chosen(&mut clock, [STOPPED, LIVE, EMPTY, EMPTY]), Some(B));
        assert_eq!(
            chosen(&mut clock, [LIVE, LIVE, EMPTY, EMPTY]),
            Some(B),
            "a second deck that starts leaves the tempo master alone"
        );
    }

    #[test]
    fn in_auto_a_tempo_master_that_stops_hands_over_to_a_synced_deck_or_the_clock() {
        let mut clock = clock();
        assert_eq!(chosen(&mut clock, [LIVE, STOPPED, EMPTY, EMPTY]), Some(A));
        assert_eq!(chosen(&mut clock, [LIVE, SYNCED, EMPTY, EMPTY]), Some(A));
        assert_eq!(chosen(&mut clock, [STOPPED, SYNCED, EMPTY, EMPTY]), Some(B));
        assert_eq!(chosen(&mut clock, [LIVE, SYNCED, EMPTY, EMPTY]), Some(B));
        assert_eq!(
            chosen(&mut clock, [LIVE, EMPTY, EMPTY, EMPTY]),
            None,
            "deck A plays without sync"
        );
        assert_eq!(
            chosen(&mut clock, [LIVE, EMPTY, EMPTY, EMPTY]),
            None,
            "it did not start now"
        );
        assert_eq!(chosen(&mut clock, [STOPPED, EMPTY, EMPTY, EMPTY]), None);
        assert_eq!(chosen(&mut clock, [LIVE, EMPTY, EMPTY, EMPTY]), Some(A));
    }

    #[test]
    fn the_lower_decks_take_and_hand_over_the_tempo_master_like_the_upper_ones() {
        let mut clock = clock();
        assert_eq!(chosen(&mut clock, [EMPTY, EMPTY, STOPPED, LIVE]), Some(D));
        assert_eq!(chosen(&mut clock, [LIVE, LIVE, SYNCED, LIVE]), Some(D));
        assert_eq!(
            chosen(&mut clock, [LIVE, LIVE, SYNCED, STOPPED]),
            Some(C),
            "the only synced deck that plays"
        );
        clock.set_auto(false, [LIVE, LIVE, SYNCED, STOPPED]);
        clock.set_master(D, STOPPED);
        assert_eq!(clock.master(), Some(D));
    }

    #[test]
    fn without_auto_a_deck_is_the_tempo_master_only_when_told() {
        let mut clock = clock();
        clock.set_master(A, LIVE);
        assert_eq!(clock.master(), None, "the choice is automatic");
        clock.choose([LIVE, STOPPED, EMPTY, EMPTY]);
        clock.set_auto(false, [LIVE, STOPPED, EMPTY, EMPTY]);
        assert_eq!(clock.master(), None, "the clock takes over");
        assert_eq!(chosen(&mut clock, [STOPPED, LIVE, EMPTY, EMPTY]), None);
        clock.set_master(B, EMPTY);
        assert_eq!(clock.master(), None, "no tempo to follow");
        clock.set_master(B, STOPPED);
        assert_eq!(clock.master(), Some(B));
        assert_eq!(
            chosen(&mut clock, [LIVE, STOPPED, EMPTY, EMPTY]),
            Some(B),
            "paused"
        );
        assert_eq!(
            chosen(&mut clock, [LIVE, EMPTY, EMPTY, EMPTY]),
            None,
            "its track left"
        );
    }

    #[test]
    fn auto_switched_on_keeps_a_tempo_master_that_plays() {
        let mut clock = clock();
        clock.set_auto(false, [LIVE, LIVE, EMPTY, EMPTY]);
        clock.set_master(B, LIVE);
        clock.set_auto(true, [LIVE, LIVE, EMPTY, EMPTY]);
        assert_eq!(clock.master(), Some(B));
        clock.set_auto(false, [LIVE, LIVE, EMPTY, EMPTY]);
        clock.set_master(B, STOPPED);
        clock.set_auto(true, [LIVE, STOPPED, EMPTY, EMPTY]);
        assert_eq!(clock.master(), Some(A));
        clock.set_auto(true, [STOPPED, STOPPED, EMPTY, EMPTY]);
        assert_eq!(clock.master(), None);
    }

    #[test]
    fn the_clock_counts_beats_at_its_tempo() {
        let mut clock = clock();
        // Half a second at 120 beats per minute is one beat.
        let pulse = (0..23_999).map(|_| clock.tick(None)).last().unwrap();
        assert!((pulse.phase - (1.0 - 1.0 / 24_000.0)).abs() < 1e-9);
        assert!(clock.tick(None).phase < 1e-9);
        clock.set_bpm(Bpm::new(60.0).unwrap());
        clock.step_bpm(0.5);
        assert_eq!(clock.tick(None).bpm, 60.5);
        clock.step_bpm(-100.0);
        assert_eq!(clock.tick(None).bpm, 60.5, "it would leave the range");
    }

    #[test]
    fn the_clock_follows_the_deck_that_is_the_tempo_master() {
        let mut clock = clock();
        clock.choose([LIVE, STOPPED, EMPTY, EMPTY]);
        let lead = |bpm, phase, keeps_the_beat| {
            Some(Lead {
                pulse: Pulse { bpm, phase },
                keeps_the_beat,
            })
        };
        assert_eq!(
            clock.tick(lead(140.0, 0.25, true)),
            Pulse {
                bpm: 140.0,
                phase: 0.25
            }
        );
        clock.set_bpm(Bpm::new(100.0).unwrap());
        let free = clock.tick(lead(140.0, 0.9, false));
        assert_eq!(free.bpm, 140.0, "the clock is not the tempo master");
        assert!(
            (free.phase - (0.25 + 140.0 / (60.0 * 48_000.0))).abs() < 1e-12,
            "a deck that does not keep the beat leaves the clock counting"
        );
        // A deck slowed down to a stop is followed, but the clock does not keep that tempo.
        assert_eq!(clock.tick(lead(0.0, 0.5, true)).bpm, 0.0);
        clock.choose([STOPPED, STOPPED, EMPTY, EMPTY]);
        assert_eq!(clock.tick(None).bpm, 140.0);
    }
}
