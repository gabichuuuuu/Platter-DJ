//! Transport of a deck: play, pause, cue, seek, tempo and nudge.
//!
//! Behaviour note: `docs/behavior/transport.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Command, Event};
use platter_testkit::measure::{max_step, peak, sine_fit};

const A: DeckId = DeckId::A;

/// Largest step between samples of a sine, with a margin of 10 %.
fn sine_step(hz: f64, amplitude: f64) -> f32 {
    (1.1 * std::f64::consts::TAU * hz / f64::from(RATE) * amplitude) as f32
}

#[test]
fn a_playing_deck_reproduces_its_track() {
    let mut rig = Rig::with_deck_a(sine_file("transport-plain", 440.0, 2.0, 0.5, RATE));
    assert!(rig.run(0.1).iter().all(|sample| *sample == 0.0));
    rig.send(Command::DeckPlay { deck: A });
    let output = rig.run(1.0);
    let fit = sine_fit(&output[4_800..], 440.0, f64::from(RATE));
    assert!((fit.amplitude - 0.5).abs() < 0.005, "{}", fit.amplitude);
    assert!(fit.snr_db() > 60.0, "{}", fit.snr_db());
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].playing);
    assert!((snapshot.decks[0].position_seconds - 1.0).abs() < 0.01);
    assert!((snapshot.decks[0].levels[0] - 0.5).abs() < 0.01);
}

#[test]
fn a_track_at_another_sample_rate_keeps_its_pitch() {
    let mut rig = Rig::with_deck_a(sine_file("transport-44k", 440.0, 2.0, 0.5, 44_100));
    rig.send(Command::DeckPlay { deck: A });
    let output = rig.run(1.0);
    let fit = sine_fit(&output[4_800..], 440.0, f64::from(RATE));
    assert!((fit.amplitude - 0.5).abs() < 0.005);
    assert!(fit.snr_db() > 60.0, "{}", fit.snr_db());
    assert!((rig.engine.snapshot().decks[0].position_seconds - 1.0).abs() < 0.01);
}

#[test]
fn pausing_fades_out_and_then_is_exact_silence() {
    let mut rig = Rig::with_deck_a(sine_file("transport-pause", 100.0, 2.0, 0.8, RATE));
    rig.send(Command::DeckPlay { deck: A });
    let before = rig.run(0.5);
    rig.send(Command::DeckPause { deck: A });
    let after = rig.run(0.5);
    // The fade lasts 5 ms. The filters of the channel then ring down: 20 ms after the
    // pause the output is under -100 dB, and within 300 ms it is exact silence.
    assert!(peak(&after[960..]) < 1e-5, "{}", peak(&after[960..]));
    assert!(after[14_400..].iter().all(|sample| *sample == 0.0));
    assert!(peak(&after[..48]) > 0.0);
    let whole: Vec<f32> = before.into_iter().chain(after).collect();
    assert!(max_step(&whole) < sine_step(100.0, 0.8) + 0.8 / 240.0);
    assert!(!rig.engine.snapshot().decks[0].playing);
}

#[test]
fn play_pause_and_seek_never_step() {
    let mut rig = Rig::with_deck_a(sine_file("transport-steps", 100.0, 4.0, 0.8, RATE));
    let mut output = Vec::new();
    rig.send(Command::DeckPlayPause { deck: A });
    output.extend(rig.run(0.2037));
    // A jump to another phase of the sine would show as a step without the fade.
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 2.0026,
    });
    output.extend(rig.run(0.2));
    rig.send(Command::DeckPlayPause { deck: A });
    output.extend(rig.run(0.003));
    // Resuming in the middle of the fade-out.
    rig.send(Command::DeckPlayPause { deck: A });
    output.extend(rig.run(0.2));
    rig.send(Command::DeckCueGoto { deck: A });
    output.extend(rig.run(0.1));
    assert!(peak(&output) > 0.7);
    let limit = sine_step(100.0, 0.8) + 0.8 / 240.0;
    assert!(
        max_step(&output) < limit,
        "{} >= {limit}",
        max_step(&output)
    );
}

#[test]
fn seeking_moves_the_position() {
    let mut rig = Rig::with_deck_a(sine_file("transport-seek", 440.0, 3.0, 0.5, RATE));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.5,
    });
    rig.run(0.01);
    assert!((rig.engine.snapshot().decks[0].position_seconds - 1.5).abs() < 1e-6);
    // Past the end lands on the end.
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 500.0,
    });
    rig.run(0.01);
    assert!((rig.engine.snapshot().decks[0].position_seconds - 3.0).abs() < 1e-6);
}

#[test]
fn tempo_changes_speed_and_pitch_together() {
    let mut rig = Rig::with_deck_a(sine_file("transport-tempo", 440.0, 4.0, 0.5, RATE));
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.5,
    });
    rig.send(Command::DeckPlay { deck: A });
    let output = rig.run(1.0);
    let fit = sine_fit(&output[4_800..], 660.0, f64::from(RATE));
    assert!((fit.amplitude - 0.5).abs() < 0.005);
    assert!(fit.snr_db() > 50.0, "{}", fit.snr_db());
    assert!((rig.engine.snapshot().decks[0].position_seconds - 1.5).abs() < 0.02);
}

#[test]
fn a_tempo_of_zero_holds_the_deck_still() {
    let mut rig = Rig::with_deck_a(sine_file("transport-stop", 440.0, 2.0, 0.5, RATE));
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 0.0,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    let held = rig.engine.snapshot().decks[0].position_seconds;
    rig.run(0.5);
    assert_eq!(rig.engine.snapshot().decks[0].position_seconds, held);
}

#[test]
fn nudge_offsets_the_speed_until_released() {
    let mut rig = Rig::with_deck_a(sine_file("transport-nudge", 440.0, 5.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckNudge {
        deck: A,
        amount: 0.1,
    });
    rig.run(1.0);
    let nudged = rig.engine.snapshot().decks[0].position_seconds;
    assert!((nudged - 1.1).abs() < 0.01, "{nudged}");
    rig.send(Command::DeckNudge {
        deck: A,
        amount: 0.0,
    });
    rig.run(1.0);
    let released = rig.engine.snapshot().decks[0].position_seconds - nudged;
    assert!((released - 1.0).abs() < 0.01, "{released}");
}

#[test]
fn cue_returns_to_the_stored_point_and_pauses() {
    let mut rig = Rig::with_deck_a(sine_file("transport-cue", 440.0, 3.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    rig.send(Command::DeckCueSet { deck: A });
    rig.run(0.7);
    let cue = rig.engine.snapshot().decks[0].cue_seconds;
    assert!((cue - 0.5).abs() < 0.01, "{cue}");
    rig.send(Command::DeckCueGoto { deck: A });
    let tail = rig.run(0.5);
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].playing);
    assert_eq!(snapshot.decks[0].position_seconds, cue);
    assert!(tail[14_400..].iter().all(|sample| *sample == 0.0));
}

#[test]
fn the_deck_stops_at_the_end_of_the_track() {
    let mut rig = Rig::with_deck_a(sine_file("transport-end", 440.0, 0.5, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    let output = rig.run(1.0);
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].playing);
    assert!((snapshot.decks[0].position_seconds - 0.5).abs() < 1e-6);
    assert!(output[33_600..].iter().all(|sample| *sample == 0.0));
}

#[test]
fn eject_silences_and_empties_the_deck() {
    let mut rig = Rig::with_deck_a(sine_file("transport-eject", 100.0, 2.0, 0.8, RATE));
    rig.send(Command::DeckPlay { deck: A });
    let mut output = rig.run(0.3);
    rig.send(Command::DeckEject { deck: A });
    output.extend(rig.run(0.5));
    assert!(output[28_800..].iter().all(|sample| *sample == 0.0));
    assert!(max_step(&output) < sine_step(100.0, 0.8) + 0.8 / 240.0);
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].track.is_none());
    assert!(!snapshot.decks[0].playing);
    // Play on an empty deck does nothing.
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.1);
    assert!(!rig.engine.snapshot().decks[0].playing);
    assert!(rig.engine.poll().is_empty());
}

#[test]
fn loading_replaces_the_track_and_reports_its_details() {
    let mut rig = Rig::with_deck_a(sine_file("transport-first", 440.0, 1.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.2);
    rig.load(A, sine_file("transport-second", 880.0, 2.0, 0.5, 44_100));
    rig.run(0.1);
    let snapshot = rig.engine.snapshot();
    let track = snapshot.decks[0].track.as_ref().unwrap();
    assert_eq!(track.sample_rate.hz(), 44_100);
    assert!((track.seconds - 2.0).abs() < 1e-6);
    assert_eq!(track.channels, 2);
    // A new track starts paused at its beginning.
    assert!(!snapshot.decks[0].playing);
    assert_eq!(snapshot.decks[0].position_seconds, 0.0);
    rig.send(Command::DeckPlay { deck: A });
    let output = rig.run(0.5);
    let fit = sine_fit(&output[4_800..], 880.0, f64::from(RATE));
    assert!((fit.amplitude - 0.5).abs() < 0.005);
}

#[test]
fn a_file_that_cannot_be_loaded_is_reported_and_leaves_the_deck_as_it_was() {
    let mut rig = Rig::with_deck_a(sine_file("transport-kept", 440.0, 1.0, 0.5, RATE));
    let missing = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("transport-missing.wav");
    rig.send(Command::DeckLoad {
        deck: A,
        path: missing,
    });
    assert!(rig.engine.snapshot().decks[0].loading);
    let events = rig.engine.wait_for_loads();
    assert!(matches!(
        events.as_slice(),
        [Event::LoadFailed { deck: A, .. }]
    ));
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].loading);
    assert!(snapshot.decks[0].track.is_some());
}

#[test]
fn commands_with_values_out_of_range_are_rejected() {
    let mut rig = Rig::new();
    let rejected = [
        Command::DeckSeek {
            deck: A,
            seconds: -1.0,
        },
        Command::DeckSeek {
            deck: A,
            seconds: f64::NAN,
        },
        Command::DeckTempoSet {
            deck: A,
            ratio: 2.5,
        },
        Command::DeckNudge {
            deck: A,
            amount: f64::INFINITY,
        },
        Command::MixerGainSet { deck: A, db: 13.0 },
        Command::MixerGainSet {
            deck: A,
            db: f64::NAN,
        },
        Command::MixerEqSet {
            deck: A,
            band: platter_engine::Band::Low,
            position: 1.5,
        },
        Command::MixerFilterSet {
            deck: A,
            position: -1.01,
        },
        Command::MixerFaderSet {
            deck: A,
            position: 2.0,
        },
        Command::MixerCrossfaderSet { position: -0.1 },
        Command::MasterGainSet { db: 12.5 },
    ];
    for command in rejected {
        assert!(rig.engine.dispatch(command.clone()).is_err(), "{command:?}");
    }
}

fn cue(pressed: bool) -> Command {
    Command::DeckCue { deck: A, pressed }
}

fn cup(pressed: bool) -> Command {
    Command::DeckCup { deck: A, pressed }
}

#[test]
fn the_cue_button_on_a_playing_deck_returns_to_the_cue_point_and_pauses() {
    let mut rig = Rig::with_deck_a(sine_file("cue-playing", 440.0, 4.0, 0.5, RATE));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.0,
    });
    rig.run(0.05);
    rig.send(Command::DeckCueSet { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    rig.send(cue(true));
    let after = rig.run(0.5);
    rig.send(cue(false));
    rig.run(0.1);
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].playing);
    assert!((snapshot.decks[0].position_seconds - 1.0).abs() < 1e-6);
    assert!(after[14_400..].iter().all(|sample| *sample == 0.0));
    assert!(max_step(&after) < sine_step(440.0, 0.5) + 0.5 / 240.0);
}

#[test]
fn the_cue_button_on_a_paused_deck_moves_the_cue_point() {
    let mut rig = Rig::with_deck_a(sine_file("cue-paused", 440.0, 4.0, 0.5, RATE));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 2.5,
    });
    rig.send(cue(true));
    let output = rig.run(0.2);
    rig.send(cue(false));
    rig.run(0.1);
    let snapshot = rig.engine.snapshot();
    assert!((snapshot.decks[0].cue_seconds - 2.5).abs() < 1e-6);
    assert!((snapshot.decks[0].position_seconds - 2.5).abs() < 1e-6);
    // Setting the point makes no sound.
    assert!(!snapshot.decks[0].playing);
    assert!(output.iter().all(|sample| *sample == 0.0));
}

#[test]
fn holding_the_cue_button_at_the_cue_point_plays_until_it_is_released() {
    let mut rig = Rig::with_deck_a(sine_file("cue-preview", 440.0, 4.0, 0.5, RATE));
    rig.send(cue(true));
    let held = rig.run(0.5);
    assert!(rig.engine.snapshot().decks[0].playing);
    let fit = sine_fit(&held[4_800..], 440.0, f64::from(RATE));
    assert!((fit.amplitude - 0.5).abs() < 0.005, "{}", fit.amplitude);
    rig.send(cue(false));
    let released = rig.run(0.5);
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].playing);
    assert_eq!(snapshot.decks[0].position_seconds, 0.0);
    assert!(released[14_400..].iter().all(|sample| *sample == 0.0));
    assert!(max_step(&released) < sine_step(440.0, 0.5) + 0.5 / 240.0);
}

#[test]
fn play_during_a_cue_preview_keeps_the_deck_playing() {
    let mut rig = Rig::with_deck_a(sine_file("cue-keep", 440.0, 4.0, 0.5, RATE));
    rig.send(cue(true));
    rig.run(0.2);
    rig.send(Command::DeckPlayPause { deck: A });
    rig.send(cue(false));
    rig.run(0.3);
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].playing);
    assert!((snapshot.decks[0].position_seconds - 0.5).abs() < 0.01);
}

#[test]
fn the_cue_play_button_waits_at_the_cue_point_and_plays_on_release() {
    let mut rig = Rig::with_deck_a(sine_file("cup", 440.0, 4.0, 0.5, RATE));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.0,
    });
    rig.run(0.05);
    rig.send(Command::DeckCueSet { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.5);
    rig.send(cup(true));
    let held = rig.run(0.5);
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].playing);
    assert!((snapshot.decks[0].position_seconds - 1.0).abs() < 1e-6);
    assert!(held[14_400..].iter().all(|sample| *sample == 0.0));
    rig.send(cup(false));
    let released = rig.run(0.5);
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].playing);
    assert!((snapshot.decks[0].position_seconds - 1.5).abs() < 0.01);
    let whole: Vec<f32> = held.into_iter().chain(released).collect();
    assert!(max_step(&whole) < sine_step(440.0, 0.5) + 0.5 / 240.0);
}

#[test]
fn cue_buttons_do_nothing_on_an_empty_deck_or_released_alone() {
    let mut rig = Rig::new();
    for command in [cue(true), cue(false), cup(true), cup(false)] {
        rig.send(command);
        assert!(rig.run(0.05).iter().all(|sample| *sample == 0.0));
        assert!(!rig.engine.snapshot().decks[0].playing);
    }
    // A release without a press changes nothing on a loaded deck either.
    rig.load(A, sine_file("cue-release-alone", 440.0, 1.0, 0.5, RATE));
    rig.send(cup(false));
    rig.send(cue(false));
    rig.run(0.05);
    assert!(!rig.engine.snapshot().decks[0].playing);
}
