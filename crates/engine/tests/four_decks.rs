//! Decks C and D: commands that name them, their channels, the crossfader and the outputs of
//! external mixer mode.
//!
//! Behaviour note: `docs/behavior/decks.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::command::{Args, Value};
use platter_engine::{Command, CrossfaderSide, MixerMode};
use platter_testkit::measure::sine_fit;
use platter_testkit::signal::channel;

/// The frequency of the sine each deck plays, deck A first.
const TONES: [f64; DeckId::COUNT] = [300.0, 500.0, 700.0, 900.0];
const LEVEL: f32 = 0.2;

/// The four decks playing a sine each at `LEVEL`, small enough that their sum stays below
/// the safety limiter.
fn four_decks(name: &str) -> Rig {
    let mut rig = Rig::new();
    for (deck, hz) in DeckId::ALL.into_iter().zip(TONES) {
        let file = sine_file(&format!("{name}-{deck}"), hz, 10.0, LEVEL, RATE);
        rig.load(deck, file);
        rig.send(Command::DeckPlay { deck });
    }
    rig
}

/// Amplitude of the sine of each deck in one output of a device with `channels` outputs,
/// once the ramps are over.
fn amplitudes(rig: &mut Rig, channels: usize, output: usize) -> [f64; DeckId::COUNT] {
    rig.run_channels(0.1, channels);
    let block = channel(&rig.run_channels(0.5, channels), channels, output);
    TONES.map(|hz| sine_fit(&block, hz, f64::from(RATE)).amplitude)
}

/// Each deck is heard at `LEVEL` where `heard` says so and is silent elsewhere.
fn assert_heard(amplitudes: [f64; DeckId::COUNT], heard: [bool; DeckId::COUNT]) {
    for ((deck, amplitude), heard) in DeckId::ALL.into_iter().zip(amplitudes).zip(heard) {
        if heard {
            assert!(
                (amplitude - f64::from(LEVEL)).abs() < 0.002,
                "deck {deck}: {amplitude}"
            );
        } else {
            assert!(
                amplitude < 1e-4,
                "deck {deck} should be silent: {amplitude}"
            );
        }
    }
}

#[test]
fn the_four_decks_play_on_their_own_side_of_the_crossfader() {
    let mut rig = four_decks("four-sides");
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    assert_heard(amplitudes(&mut rig, 2, 0), [true, false, true, false]);
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks.iter().all(|deck| deck.playing));
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    assert_heard(amplitudes(&mut rig, 2, 0), [false, true, false, true]);
}

#[test]
fn the_channels_of_decks_c_and_d_act_on_them_alone() {
    let mut rig = four_decks("four-channels");
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    rig.send(Command::MixerFaderSet {
        deck: DeckId::D,
        position: 0.0,
    });
    rig.send(Command::MixerCrossfaderAssign {
        deck: DeckId::C,
        side: CrossfaderSide::Thru,
    });
    assert_heard(amplitudes(&mut rig, 2, 0), [false, true, true, false]);
    rig.send(Command::DeckPause { deck: DeckId::C });
    assert_heard(amplitudes(&mut rig, 2, 0), [false, true, false, false]);
    assert!(rig.engine.snapshot().decks[DeckId::D.index()].playing);
}

#[test]
fn in_external_mode_each_column_shares_a_pair() {
    let mut rig = four_decks("four-external");
    rig.send(Command::MixerModeSet {
        mode: MixerMode::External,
    });
    for side in [0, 1] {
        assert_heard(amplitudes(&mut rig, 4, side), [true, false, true, false]);
        assert_heard(
            amplitudes(&mut rig, 4, side + 2),
            [false, true, false, true],
        );
    }
    // With one pair, the four decks share it.
    assert_heard(amplitudes(&mut rig, 2, 0), [true; DeckId::COUNT]);
}

#[test]
fn a_command_names_decks_c_and_d_and_no_other() {
    let mut args = Args::new();
    for (letter, deck) in [("c", DeckId::C), ("D", DeckId::D)] {
        args.insert("deck".to_owned(), Value::Text(letter.to_owned()));
        assert_eq!(
            Command::from_args("deck.play_pause", &args),
            Ok(Command::DeckPlayPause { deck })
        );
    }
    args.insert("deck".to_owned(), Value::Text("e".to_owned()));
    assert!(Command::from_args("deck.play_pause", &args).is_err());
}

#[test]
fn four_decks_at_full_scale_stay_under_the_ceiling() {
    let mut rig = Rig::new();
    for (deck, hz) in DeckId::ALL.into_iter().zip(TONES) {
        rig.load(
            deck,
            sine_file(&format!("four-loud-{deck}"), hz, 10.0, 0.9, RATE),
        );
        rig.send(Command::MixerCrossfaderAssign {
            deck,
            side: CrossfaderSide::Thru,
        });
        rig.send(Command::DeckPlay { deck });
    }
    let output = rig.run_stereo(1.0);
    assert!(output.iter().all(|sample| sample.is_finite()));
    let peak = output
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak <= 1.0, "{peak}");
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.limiter_gain < 0.5, "{}", snapshot.limiter_gain);
}
