//! Autogain: the gain of a channel set from the loudness of its track.
//!
//! Behaviour note: `docs/behavior/autogain.md`.

mod common;

use common::{RATE, Rig, rate, sine_file};
use platter_core::DeckId;
use platter_engine::{AUTOGAIN_LUFS, Command, Engine, MAX_AUTOGAIN_DB, TrackAnalysis};
use platter_testkit::measure::sine_fit;

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;
/// A stereo sine at 997 Hz reads, in LUFS, the level of its amplitude in dBFS: the K filter
/// adds there the 0.691 dB the formula takes away, and two channels double one.
const HZ: f64 = 997.0;

/// A rig with autogain on, as an engine starts, and deck A alone on the master.
fn rig() -> Rig {
    let (engine, processor) = Engine::new(rate());
    let mut rig = Rig { engine, processor };
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig
}

/// A sine file whose loudness is `lufs`.
fn track(name: &str, lufs: f64) -> std::path::PathBuf {
    sine_file(name, HZ, 3.0, 10f64.powf(lufs / 20.0) as f32, RATE)
}

fn gain_db(rig: &Rig, deck: DeckId) -> f64 {
    rig.engine.snapshot().decks[deck.index()].controls.gain_db
}

/// The level of deck A on the master, in dBFS of the amplitude of its sine.
fn heard_db(rig: &mut Rig) -> f64 {
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.2);
    let amplitude = sine_fit(&rig.run(0.5), HZ, f64::from(RATE)).amplitude;
    rig.send(Command::DeckPause { deck: A });
    rig.run(0.05);
    20.0 * amplitude.log10()
}

fn assert_near(value: f64, expected: f64, within: f64) {
    assert!(
        (value - expected).abs() <= within,
        "{value} against {expected}"
    );
}

#[test]
fn autogain_starts_on_and_brings_a_track_to_minus_10_lufs() {
    let mut rig = rig();
    assert!(rig.engine.snapshot().mixer.autogain);
    for (name, lufs) in [("autogain-quiet", -20.0), ("autogain-loud", -1.0)] {
        rig.load(A, track(name, lufs));
        let snapshot = rig.engine.snapshot();
        let autogain = snapshot.decks[0].autogain_db.unwrap();
        assert_near(autogain, AUTOGAIN_LUFS - lufs, 0.05);
        assert_eq!(gain_db(&rig, A), autogain, "the knob shows the gain");
        assert_near(heard_db(&mut rig), AUTOGAIN_LUFS, 0.1);
    }
}

#[test]
fn the_gain_stops_at_12_db_either_way() {
    let mut rig = rig();
    rig.load(A, track("autogain-faint", -40.0));
    assert_eq!(gain_db(&rig, A), MAX_AUTOGAIN_DB);
    // A sine cannot read louder than 0 LUFS, so the bottom bound is checked on the value.
    let loud = TrackAnalysis {
        loudness: Some(5.0),
        ..TrackAnalysis::default()
    };
    assert_eq!(loud.autogain_db(), Some(-MAX_AUTOGAIN_DB));
    let unmeasured = TrackAnalysis::default();
    assert_eq!(unmeasured.autogain_db(), None);
}

#[test]
fn the_djs_gain_holds_until_the_next_track_and_restore_brings_back_the_value() {
    let mut rig = rig();
    rig.load(A, track("autogain-hold-1", -16.0));
    assert_near(gain_db(&rig, A), 6.0, 0.05);
    rig.send(Command::MixerGainSet { deck: A, db: -3.0 });
    assert_eq!(gain_db(&rig, A), -3.0);
    assert_near(heard_db(&mut rig), -19.0, 0.1);

    rig.send(Command::MixerGainRestore { deck: A });
    assert_near(gain_db(&rig, A), 6.0, 0.05);
    assert_near(heard_db(&mut rig), AUTOGAIN_LUFS, 0.1);

    rig.send(Command::MixerGainSet { deck: A, db: 0.0 });
    rig.load(A, track("autogain-hold-2", -13.0));
    assert_near(gain_db(&rig, A), 3.0, 0.05);
}

#[test]
fn off_it_leaves_the_gain_alone_and_on_again_it_sets_it() {
    let mut rig = rig();
    rig.send(Command::MixerAutogainToggle);
    assert!(!rig.engine.snapshot().mixer.autogain);
    rig.load(A, track("autogain-off-a", -18.0));
    rig.load(B, track("autogain-off-b", -14.0));
    assert_eq!(gain_db(&rig, A), 0.0);
    assert_eq!(gain_db(&rig, B), 0.0);
    // The value is still measured and shown.
    let snapshot = rig.engine.snapshot();
    assert_near(snapshot.decks[0].autogain_db.unwrap(), 8.0, 0.05);
    assert_near(heard_db(&mut rig), -18.0, 0.1);

    // Restore works while autogain is off.
    rig.send(Command::MixerGainRestore { deck: B });
    assert_near(gain_db(&rig, B), 4.0, 0.05);
    assert_eq!(gain_db(&rig, A), 0.0);

    // Switching it on sets both channels.
    rig.send(Command::MixerGainSet { deck: B, db: -6.0 });
    rig.send(Command::MixerAutogainToggle);
    assert_near(gain_db(&rig, A), 8.0, 0.05);
    assert_near(gain_db(&rig, B), 4.0, 0.05);

    // Switching it off again leaves them where they are.
    rig.send(Command::MixerAutogainToggle);
    assert_near(gain_db(&rig, A), 8.0, 0.05);
}

#[test]
fn silence_has_no_value_and_leaves_the_gain_where_it_was() {
    let mut rig = rig();
    rig.load(A, track("autogain-before-silence", -15.0));
    assert_near(gain_db(&rig, A), 5.0, 0.05);
    rig.load(A, sine_file("autogain-silence", HZ, 3.0, 0.0, RATE));
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].analysis.is_some());
    assert_eq!(snapshot.decks[0].autogain_db, None);
    assert_near(gain_db(&rig, A), 5.0, 0.05);
    rig.send(Command::MixerGainSet { deck: A, db: 1.0 });
    rig.send(Command::MixerGainRestore { deck: A });
    assert_eq!(gain_db(&rig, A), 1.0, "nothing to restore");
}

#[test]
fn restore_on_an_empty_deck_does_nothing() {
    let mut rig = rig();
    rig.send(Command::MixerGainSet { deck: B, db: 2.0 });
    rig.send(Command::MixerGainRestore { deck: B });
    assert_eq!(gain_db(&rig, B), 2.0);
    assert_eq!(rig.engine.snapshot().decks[1].autogain_db, None);
}

#[test]
fn a_new_device_rate_keeps_autogain_off_or_on() {
    for on in [true, false] {
        let mut rig = rig();
        if !on {
            rig.send(Command::MixerAutogainToggle);
        }
        let rate = platter_core::SampleRate::new(44_100).unwrap();
        let (mut engine, _processor) = Engine::with_controls_of(rate, &rig.engine);
        assert_eq!(engine.snapshot().mixer.autogain, on);
        engine
            .dispatch(Command::DeckLoad {
                deck: A,
                path: track(&format!("autogain-device-{on}"), -12.0),
            })
            .unwrap();
        engine.wait_for_loads();
        let expected = if on { 2.0 } else { 0.0 };
        assert_near(engine.snapshot().decks[0].controls.gain_db, expected, 0.05);
    }
}
