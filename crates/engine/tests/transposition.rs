//! Transposition by semitones and the resulting key, with the stretch the engine ships.
//! Behaviour note: `docs/behavior/keylock.md`, sections Transposition and The resulting key;
//! the choices: `docs/decisions/0031-transposition.md`.
//!
//! The stretch is checked against tolerances, never sample by sample: its FFT may take a
//! different path on each processor.

// The helpers that read the snapshot are not tests themselves.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use common::{RATE, Rig, rate, sine_file};
use platter_core::{DeckId, SampleRate};
use platter_engine::{
    Analyser, Command, DeckSnapshot, Engine, Key, Mode, SizeStep, TrackAnalysis, Transposed,
};
use platter_testkit::files::wav_pcm16;
use platter_testkit::measure::{max_step, sine_fit};
use platter_testkit::signal::{stereo, to_i16};

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;
/// A minor.
const A_MINOR: Key = Key {
    tonic: 9,
    mode: Mode::Minor,
};

/// Twenty seconds at 440 Hz, written once for the tests of this file.
fn tone() -> PathBuf {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| sine_file("transposition-440", 440.0, 20.0, 0.4, RATE))
        .clone()
}

/// The frequency of a sine, from its rising zero crossings, and how clean it is at that
/// frequency, in dB of the sine over what is not.
fn tone_of(samples: &[f32]) -> (f64, f64) {
    let rising: Vec<f64> = (1..samples.len())
        .filter(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0)
        .map(|i| {
            let (a, b) = (f64::from(samples[i - 1]), f64::from(samples[i]));
            (i - 1) as f64 + a / (a - b)
        })
        .collect();
    let (Some(first), Some(last)) = (rising.first(), rising.last()) else {
        return (0.0, f64::NEG_INFINITY);
    };
    let hz = (rising.len() - 1) as f64 * f64::from(RATE) / (last - first);
    (hz, sine_fit(samples, hz, f64::from(RATE)).snr_db())
}

/// `samples` are a clean sine within 5 cents of `hz`: key lock moves the pitch by at most a
/// few cents to keep the tempo exact.
fn is_tone(samples: &[f32], hz: f64) -> bool {
    let (found, clean) = tone_of(samples);
    (1200.0 * (found / hz).log2()).abs() < 5.0 && clean > 40.0
}

/// 440 Hz moved by `semitones`.
fn shifted(semitones: f64) -> f64 {
    440.0 * (semitones / 12.0).exp2()
}

fn playing_at(path: PathBuf, tempo: f64, semitones: f64) -> Rig {
    let mut rig = Rig::with_deck_a(path);
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: tempo,
    });
    rig.send(Command::DeckKeySet { deck: A, semitones });
    rig.send(Command::DeckPlay { deck: A });
    rig
}

fn snapshot(rig: &mut Rig) -> DeckSnapshot {
    rig.processor.process(&mut [], 2);
    rig.engine.poll();
    rig.engine.snapshot().decks[A.index()].clone()
}

#[test]
fn a_shift_plays_the_original_key_moved_at_any_tempo() {
    for tempo in [1.0, 1.08] {
        for semitones in [1.0, -2.0, 12.0, -12.0] {
            let mut rig = playing_at(tone(), tempo, semitones);
            rig.run(0.5);
            let heard = rig.run(2.0);
            assert!(
                is_tone(&heard, shifted(semitones)),
                "at {tempo}, {semitones} semitones: {:?}",
                tone_of(&heard)
            );
        }
    }
}

#[test]
fn a_shift_does_not_move_the_position() {
    let mut plain = playing_at(tone(), 1.08, 0.0);
    let mut moved = playing_at(tone(), 1.08, 2.0);
    plain.run(0.25);
    moved.run(0.25);
    let start = snapshot(&mut plain).position_seconds;
    // The stretch takes over at the same place: before the tempo and the pitch were applied
    // ahead of a seek, a shift of 2 put it 240 frames behind.
    let moved_start = snapshot(&mut moved).position_seconds;
    assert!(
        (moved_start - start).abs() < 0.001,
        "{moved_start} against {start}"
    );
    plain.run(8.0);
    moved.run(8.0);
    let (plain, moved) = (
        snapshot(&mut plain).position_seconds,
        snapshot(&mut moved).position_seconds,
    );
    // Eight seconds at 1.08 either way, to within a millisecond of the track.
    assert!((plain - start - 8.0 * 1.08).abs() < 0.001, "{plain}");
    assert!((moved - plain).abs() < 0.001, "{moved} against {plain}");
}

#[test]
fn a_synced_deck_with_a_shift_stays_on_the_beat() {
    let mut rig = Rig::new();
    rig.load(A, sine_file("transposition-sync-a", 330.0, 30.0, 0.4, RATE));
    rig.load(B, tone());
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 126.0,
    });
    rig.send(Command::DeckGridBpmSet {
        deck: B,
        bpm: 120.0,
    });
    rig.send(Command::DeckKeySet {
        deck: B,
        semitones: 2.0,
    });
    rig.send(Command::DeckSyncToggle { deck: B });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    rig.run(2.0);
    let mut worst = 0.0f32;
    for _ in 0..10 {
        rig.run(1.0);
        rig.engine.poll();
        let phase = rig.engine.snapshot().decks[B.index()].phase.unwrap();
        worst = worst.max(phase.abs());
    }
    // A hundredth of a beat at 126 BPM is under 5 ms.
    assert!(worst < 0.01, "deck B was {worst:.4} beats off");
}

#[test]
fn a_step_while_playing_does_not_click_and_moves_the_key_one_semitone() {
    let mut rig = playing_at(tone(), 1.08, 0.0);
    rig.run(1.0);
    let steady = max_step(&rig.run(0.5));
    rig.send(Command::DeckKeyStep {
        deck: A,
        direction: SizeStep::Up,
    });
    let after = rig.run(1.0);
    // A sine a semitone higher steps by about 6 % more; a click would be far above.
    assert!(
        max_step(&after) < steady * 1.5,
        "{} against {steady}",
        max_step(&after)
    );
    assert!(
        is_tone(&after[24_000..], shifted(1.0)),
        "{:?}",
        tone_of(&after[24_000..])
    );
}

#[test]
fn turning_key_lock_off_and_on_returns_to_the_original_key() {
    let mut rig = playing_at(tone(), 1.08, 3.0);
    rig.run(1.0);
    assert_eq!(snapshot(&mut rig).key_shift, 3.0);
    rig.send(Command::DeckKeyLockToggle { deck: A });
    let off = rig.run(1.0);
    assert!(
        is_tone(&off[24_000..], 440.0 * 1.08),
        "{:?}",
        tone_of(&off[24_000..])
    );
    rig.send(Command::DeckKeyLockToggle { deck: A });
    let on = rig.run(1.0);
    let deck = snapshot(&mut rig);
    assert!(deck.key_lock);
    assert_eq!(deck.key_shift, 0.0);
    assert!(
        is_tone(&on[24_000..], 440.0),
        "{:?}",
        tone_of(&on[24_000..])
    );
}

#[test]
fn the_commands_turn_key_lock_on_and_keep_the_shift_in_range() {
    let mut rig = Rig::with_deck_a(tone());
    rig.send(Command::DeckKeyStep {
        deck: A,
        direction: SizeStep::Up,
    });
    let deck = snapshot(&mut rig);
    assert!(deck.key_lock);
    assert_eq!(deck.key_shift, 1.0);
    rig.send(Command::DeckKeySet {
        deck: A,
        semitones: 11.5,
    });
    for _ in 0..3 {
        rig.send(Command::DeckKeyStep {
            deck: A,
            direction: SizeStep::Up,
        });
    }
    assert_eq!(snapshot(&mut rig).key_shift, 12.0);
    rig.send(Command::DeckKeySet {
        deck: A,
        semitones: -0.25,
    });
    rig.send(Command::DeckKeyStep {
        deck: A,
        direction: SizeStep::Down,
    });
    assert_eq!(snapshot(&mut rig).key_shift, -1.25);
    // Out of range is refused before it reaches the deck.
    assert!(
        rig.engine
            .dispatch(Command::DeckKeySet {
                deck: A,
                semitones: 13.0,
            })
            .is_err()
    );
    assert_eq!(snapshot(&mut rig).key_shift, -1.25);
}

#[test]
fn persist_keeps_the_key_of_the_current_speed() {
    let mut rig = Rig::with_deck_a(tone());
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.06,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    rig.send(Command::DeckKeyPersist { deck: A });
    let deck = snapshot(&mut rig);
    assert!(deck.key_lock);
    assert!(
        (deck.key_shift - 12.0 * 1.06f64.log2()).abs() < 1e-6,
        "{}",
        deck.key_shift
    );
    // The tempo changes; the key heard does not.
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 0.94,
    });
    rig.run(0.5);
    let heard = rig.run(1.5);
    assert!(is_tone(&heard, 440.0 * 1.06), "{:?}", tone_of(&heard));
    // At a speed the range does not reach, the shift stops at its end.
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 2.0,
    });
    rig.send(Command::DeckNudge {
        deck: A,
        amount: 0.2,
    });
    rig.run(0.1);
    rig.send(Command::DeckKeyPersist { deck: A });
    assert_eq!(snapshot(&mut rig).key_shift, 12.0);
}

#[test]
fn a_shift_holds_on_a_44_1_khz_track() {
    let track = sine_file("transposition-44k", 440.0, 8.0, 0.4, 44_100);
    for (tempo, semitones) in [(0.84, 2.0), (1.16, -2.0)] {
        let mut rig = playing_at(track.clone(), tempo, semitones);
        rig.run(0.5);
        let heard = rig.run(2.0);
        assert!(
            is_tone(&heard, shifted(semitones)),
            "44.1 kHz at {tempo}, {semitones} semitones: {:?}",
            tone_of(&heard)
        );
    }
}

#[test]
fn a_track_without_a_stretch_follows_the_speed_whatever_the_shift() {
    let track = sine_file("transposition-22k", 440.0, 4.0, 0.4, 22_050);
    let mut rig = playing_at(track, 1.08, 2.0);
    rig.run(0.5);
    let heard = rig.run(1.0);
    assert!(is_tone(&heard, 440.0 * 1.08), "{:?}", tone_of(&heard));
}

#[test]
fn the_shift_stays_for_the_next_track_and_a_new_device_rate() {
    let mut rig = Rig::new();
    rig.send(Command::DeckKeySet {
        deck: A,
        semitones: -3.5,
    });
    rig.load(A, tone());
    assert_eq!(snapshot(&mut rig).key_shift, -3.5);
    let other = SampleRate::new(44_100).unwrap();
    let (engine, mut processor) = Engine::with_controls_of(other, &rig.engine);
    processor.process(&mut [], 2);
    let decks = engine.snapshot().decks;
    assert!(decks[A.index()].key_lock);
    assert_eq!(decks[A.index()].key_shift, -3.5);
    assert!(!decks[B.index()].key_lock);
    assert_eq!(decks[B.index()].key_shift, 0.0);
    let (engine, mut processor) = Engine::with_controls_of(rate(), &engine);
    processor.process(&mut [], 2);
    assert_eq!(engine.snapshot().decks[A.index()].key_shift, -3.5);
}

/// Says every track is in A minor.
struct InAMinor;

impl Analyser for InAMinor {
    fn analyse(&self, _: &[f32], _: SampleRate) -> TrackAnalysis {
        TrackAnalysis {
            tempo: None,
            key: Some(A_MINOR),
            loudness: None,
        }
    }
}

/// A rig whose analysis finds A minor, with `path` loaded on deck A and analysed.
fn in_a_minor(path: PathBuf) -> Rig {
    let (engine, processor) = Engine::with_analyser(rate(), Arc::new(InAMinor));
    let mut rig = Rig { engine, processor };
    rig.load(A, path);
    while snapshot(&mut rig).analysis.is_none() {
        std::thread::yield_now();
    }
    rig
}

fn key(tonic: u8) -> Key {
    Key {
        tonic,
        mode: Mode::Minor,
    }
}

#[test]
fn the_resulting_key_follows_the_shift_with_key_lock_and_the_speed_without() {
    let mut rig = in_a_minor(tone());
    assert_eq!(
        snapshot(&mut rig).resulting_key,
        Some(Transposed {
            key: A_MINOR,
            cents: 0.0
        })
    );
    // 1.06 is 1.0087 semitones up: B flat minor, 0.87 cents sharp.
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.06,
    });
    rig.run(0.1);
    let found = snapshot(&mut rig).resulting_key.unwrap();
    assert_eq!(found.key, key(10));
    assert!((found.cents - 0.874).abs() < 0.01, "{found:?}");
    // With key lock and a shift of -2, the speed no longer counts: G minor.
    rig.send(Command::DeckKeySet {
        deck: A,
        semitones: -2.0,
    });
    assert_eq!(
        snapshot(&mut rig).resulting_key,
        Some(Transposed {
            key: key(7),
            cents: 0.0
        })
    );
    // A fine shift of the knob: half a semitone down from A is G sharp, 50 cents sharp.
    rig.send(Command::DeckKeySet {
        deck: A,
        semitones: -0.5,
    });
    let found = snapshot(&mut rig).resulting_key.unwrap();
    assert_eq!(found.key, key(8));
    assert!((found.cents - 50.0).abs() < 1e-9, "{found:?}");
}

#[test]
fn a_track_without_a_stretch_has_the_resulting_key_of_its_speed() {
    let mut rig = in_a_minor(sine_file("transposition-22k-key", 440.0, 2.0, 0.4, 22_050));
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.06,
    });
    rig.send(Command::DeckKeySet {
        deck: A,
        semitones: -2.0,
    });
    rig.run(0.1);
    assert_eq!(snapshot(&mut rig).resulting_key.unwrap().key, key(10));
}

/// Finds no key in any track.
struct NoKey;

impl Analyser for NoKey {
    fn analyse(&self, _: &[f32], _: SampleRate) -> TrackAnalysis {
        TrackAnalysis {
            tempo: None,
            key: None,
            loudness: None,
        }
    }
}

#[test]
fn a_track_whose_key_was_not_found_has_no_resulting_key() {
    let (engine, processor) = Engine::with_analyser(rate(), Arc::new(NoKey));
    let mut rig = Rig { engine, processor };
    rig.load(A, tone());
    while snapshot(&mut rig).analysis.is_none() {
        std::thread::yield_now();
    }
    assert_eq!(snapshot(&mut rig).resulting_key, None);
}

#[test]
fn a_shift_changed_while_playing_keeps_the_position() {
    for (from, tempo, to) in [(0.0, 1.08, 12.0), (0.0, 1.0, -12.0), (5.0, 0.94, 0.0)] {
        let mut plain = playing_at(tone(), tempo, 0.0);
        let mut moved = playing_at(tone(), tempo, from);
        plain.run(1.0);
        moved.run(1.0);
        moved.send(Command::DeckKeySet {
            deck: A,
            semitones: to,
        });
        plain.run(1.0);
        moved.run(1.0);
        let (plain, moved) = (
            snapshot(&mut plain).position_seconds,
            snapshot(&mut moved).position_seconds,
        );
        assert!(
            (moved - plain).abs() < 0.001,
            "{from} to {to} at {tempo}: {moved} against {plain}"
        );
    }
}

/// A tone at 440 Hz with a gap from 2.5 to 3 s of the track.
fn gap_file() -> PathBuf {
    let rate = f64::from(RATE);
    let mono: Vec<f32> = (0..(6.0 * rate) as usize)
        .map(|frame| {
            let t = frame as f64 / rate;
            if (2.5..3.0).contains(&t) {
                0.0
            } else {
                (0.4 * (std::f64::consts::TAU * 440.0 * t).sin()) as f32
            }
        })
        .collect();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("transposition-gap.wav");
    std::fs::write(&path, wav_pcm16(RATE, 2, &to_i16(&stereo(&mono)))).unwrap();
    path
}

#[test]
fn the_position_of_a_transposed_deck_is_that_of_what_is_heard() {
    let path = gap_file();
    for tempo in [1.0, 1.08] {
        for semitones in [-12.0, -2.0, 2.0, 12.0] {
            let mut rig = playing_at(path.clone(), tempo, semitones);
            let mut block = [0.0f32; 16 * 2];
            let (mut frame, mut silent) = (0usize, false);
            let (mut heard, mut reached) = (None, None);
            while frame < 4 * RATE as usize && (heard.is_none() || reached.is_none()) {
                rig.processor.process(&mut block, 2);
                frame += 16;
                let peak = block
                    .iter()
                    .fold(0.0f32, |most, sample| most.max(sample.abs()));
                silent |= frame > RATE as usize && peak < 0.01;
                if silent && heard.is_none() && peak > 0.2 {
                    heard = Some(frame);
                }
                if reached.is_none() && snapshot(&mut rig).position_seconds >= 3.0 {
                    reached = Some(frame);
                }
            }
            let (heard, reached) = (heard.unwrap(), reached.unwrap());
            // The tone comes back where the position says 3 s, within the softened attack
            // of the stretch: 8 ms at most measured, 43 ms off before the fix.
            let off = (heard as f64 - reached as f64) / f64::from(RATE);
            assert!(
                off.abs() < 0.01,
                "{semitones} semitones at {tempo}: heard {heard}, position at {reached}"
            );
        }
    }
}
