//! Key lock of a deck, with the stretch the engine ships. Behaviour note:
//! `docs/behavior/keylock.md`; the stretch and its measurement:
//! `docs/decisions/0021-key-lock-stretch.md`.
//!
//! The stretch is checked against tolerances, never sample by sample: its FFT may take a
//! different path on each processor.

mod common;

use std::path::PathBuf;
use std::sync::OnceLock;

use common::{RATE, Rig, rate, sine_file};
use platter_core::{DeckId, HotcueSlot};
use platter_engine::{Command, Direction, Engine};
use platter_testkit::measure::{max_step, peak, sine_fit};

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

/// Twenty seconds at 440 Hz, written once for the tests of this file.
fn tone() -> PathBuf {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| sine_file("keylock-440", 440.0, 20.0, 0.4, RATE))
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

fn playing_at(tempo: f64, key_lock: bool) -> Rig {
    let mut rig = Rig::with_deck_a(tone());
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: tempo,
    });
    if key_lock {
        rig.send(Command::DeckKeyLockToggle { deck: A });
    }
    rig.send(Command::DeckPlay { deck: A });
    rig
}

fn position(rig: &mut Rig) -> f64 {
    rig.processor.process(&mut [], 2);
    rig.engine.poll();
    rig.engine.snapshot().decks[A.index()].position_seconds
}

#[test]
fn key_lock_keeps_the_pitch_at_another_tempo_and_without_it_the_pitch_follows() {
    for tempo in [0.92, 1.08] {
        let mut locked = playing_at(tempo, true);
        locked.run(0.5);
        let heard = locked.run(2.0);
        assert!(
            is_tone(&heard, 440.0),
            "at {tempo} with key lock: {:?}",
            tone_of(&heard)
        );

        let mut plain = playing_at(tempo, false);
        plain.run(0.5);
        let heard = plain.run(2.0);
        assert!(
            is_tone(&heard, 440.0 * tempo),
            "at {tempo}: {:?}",
            tone_of(&heard)
        );
    }
}

#[test]
fn the_tempo_of_a_key_locked_deck_is_the_one_asked() {
    let mut rig = playing_at(1.08, true);
    rig.run(0.25);
    let start = position(&mut rig);
    rig.run(8.0);
    let moved = position(&mut rig) - start;
    // Eight seconds at 1.08, to within a millisecond of the track.
    assert!(
        (moved - 8.0 * 1.08).abs() < 0.001,
        "moved {moved:.5} s of the track"
    );
}

#[test]
fn a_synced_deck_with_key_lock_stays_on_the_beat() {
    let mut rig = Rig::new();
    rig.load(A, sine_file("keylock-sync-a", 330.0, 30.0, 0.4, RATE));
    rig.load(B, tone());
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 126.0,
    });
    rig.send(Command::DeckGridBpmSet {
        deck: B,
        bpm: 120.0,
    });
    rig.send(Command::DeckKeyLockToggle { deck: B });
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
    let heard = common::Rig::run(&mut rig, 1.0);
    assert!(peak(&heard) > 0.1);
}

#[test]
fn a_jump_with_key_lock_is_heard_at_once_and_settles_on_the_key() {
    let mut rig = playing_at(1.08, true);
    rig.run(1.0);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 10.0,
    });
    let after = rig.run(1.0);
    // The seek fades out and in over 5 ms each; after that, sound from the first moment,
    // while the stretch gets ready.
    let silent = after[960..2400]
        .iter()
        .filter(|sample| sample.abs() < 1e-4)
        .count();
    assert!(silent < 20, "{silent} silent samples after the jump");
    assert!(
        is_tone(&after[24_000..], 440.0),
        "{:?}",
        tone_of(&after[24_000..])
    );
    assert!((position(&mut rig) - (10.0 + 1.0 * 1.08)).abs() < 0.01);
}

#[test]
fn a_beatjump_with_key_lock_keeps_the_time() {
    let mut rig = playing_at(1.0, true);
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    rig.run(4.0);
    let before = position(&mut rig);
    rig.send(Command::DeckBeatjump {
        deck: A,
        direction: Direction::Back,
    });
    rig.run(1.0);
    let after = position(&mut rig);
    let jumped = before + 1.0 - after;
    assert!(jumped > 0.0, "the deck did not jump back");
    assert!(
        (jumped - (jumped * 2.0).round() / 2.0).abs() < 0.002,
        "jumped {jumped:.4} s, not a whole number of beats at 120"
    );
}

#[test]
fn turning_key_lock_on_or_off_while_playing_does_not_click() {
    let mut rig = playing_at(1.08, false);
    rig.run(1.0);
    let steady = max_step(&rig.run(0.5));
    rig.send(Command::DeckKeyLockToggle { deck: A });
    let on = rig.run(1.0);
    rig.send(Command::DeckKeyLockToggle { deck: A });
    let off = rig.run(1.0);
    // Two sines a semitone apart crossfading step at most a little more than one.
    assert!(
        max_step(&on) < steady * 2.0,
        "{} against {steady}",
        max_step(&on)
    );
    assert!(
        max_step(&off) < steady * 2.0,
        "{} against {steady}",
        max_step(&off)
    );
    assert!(
        is_tone(&on[24_000..], 440.0),
        "{:?}",
        tone_of(&on[24_000..])
    );
    assert!(
        is_tone(&off[24_000..], 440.0 * 1.08),
        "{:?}",
        tone_of(&off[24_000..])
    );
}

#[test]
fn key_lock_stays_with_the_deck_for_the_next_track_and_a_new_device_rate() {
    let mut rig = Rig::new();
    rig.send(Command::DeckKeyLockToggle { deck: A });
    rig.load(A, tone());
    rig.processor.process(&mut [], 2);
    assert!(rig.engine.snapshot().decks[A.index()].key_lock);
    assert!(!rig.engine.snapshot().decks[B.index()].key_lock);
    let (engine, mut processor) = Engine::with_controls_of(rate(), &rig.engine);
    processor.process(&mut [], 2);
    assert!(engine.snapshot().decks[A.index()].key_lock);
    assert!(!engine.snapshot().decks[B.index()].key_lock);
}

#[test]
fn a_track_the_stretch_does_not_take_plays_with_the_pitch_following() {
    // 22.05 kHz is below what the stretch takes.
    let mut rig = Rig::with_deck_a(sine_file("keylock-22k", 440.0, 4.0, 0.4, 22_050));
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.08,
    });
    rig.send(Command::DeckKeyLockToggle { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    let heard = rig.run(1.0);
    assert!(is_tone(&heard, 440.0 * 1.08), "{:?}", tone_of(&heard));
}

/// A deck playing the tone at 1.08 with key lock, its grid at 120 BPM, a second into it.
fn locked_at_120() -> (Rig, f32) {
    let mut rig = playing_at(1.08, true);
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    rig.run(1.0);
    let steady = max_step(&rig.run(0.5));
    (rig, steady)
}

#[test]
fn jumps_and_loops_with_key_lock_do_not_click() {
    // A beatjump.
    let (mut rig, steady) = locked_at_120();
    rig.send(Command::DeckBeatjump {
        deck: A,
        direction: Direction::Back,
    });
    let heard = rig.run(0.5);
    assert!(
        max_step(&heard) < steady * 2.0,
        "beatjump: {} against {steady}",
        max_step(&heard)
    );

    // A loop of one beat, through several passes.
    let (mut rig, steady) = locked_at_120();
    rig.send(Command::DeckLoopToggle { deck: A });
    let heard = rig.run(2.0);
    assert!(
        max_step(&heard) < steady * 2.0,
        "loop: {} against {steady}",
        max_step(&heard)
    );

    // A hotcue stored earlier, pressed while the deck plays.
    let (mut rig, steady) = locked_at_120();
    let slot = HotcueSlot::ALL[0];
    rig.send(Command::DeckHotcueTrigger {
        deck: A,
        slot,
        pressed: true,
    });
    rig.send(Command::DeckHotcueTrigger {
        deck: A,
        slot,
        pressed: false,
    });
    rig.run(1.0);
    rig.send(Command::DeckHotcueTrigger {
        deck: A,
        slot,
        pressed: true,
    });
    let heard = rig.run(0.5);
    assert!(
        max_step(&heard) < steady * 2.0,
        "hotcue: {} against {steady}",
        max_step(&heard)
    );
}

#[test]
fn key_lock_holds_on_a_44_1_khz_track_and_at_the_ends_of_the_tempo_range() {
    let track = sine_file("keylock-44k", 440.0, 12.0, 0.4, 44_100);
    for tempo in [0.5, 0.84, 1.16, 1.6] {
        let mut rig = Rig::with_deck_a(track.clone());
        rig.send(Command::DeckTempoSet {
            deck: A,
            ratio: tempo,
        });
        rig.send(Command::DeckKeyLockToggle { deck: A });
        rig.send(Command::DeckPlay { deck: A });
        rig.run(0.5);
        let heard = rig.run(2.0);
        assert!(
            is_tone(&heard, 440.0),
            "44.1 kHz at {tempo}: {:?}",
            tone_of(&heard)
        );
    }
}
