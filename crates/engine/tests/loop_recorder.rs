//! The loop recorder. Behaviour note: `docs/behavior/recording.md`, *Loop recorder*.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Command, EngineError, LoopPhase, LoopSource, MixerMode};
use platter_testkit::measure::{max_step, rms};

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;
/// One bar at the 120 beats per minute of the clock, in seconds.
const BAR_120: f64 = 2.0;

fn phase(rig: &Rig) -> LoopPhase {
    rig.engine.snapshot().loop_recorder.phase
}

/// Deck A plays a sine alone on the master; the loop recorder is set to one bar.
fn playing_a(name: &str) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(name, 440.0, 12.0, 0.5, RATE));
    rig.send(Command::LoopRecorderSizeSet { bars: 1.0 });
    rig.send(Command::DeckPlay { deck: A });
    rig
}

/// Records one bar of deck A and pauses the deck, so only the loop sounds.
fn recorded_a(name: &str) -> Rig {
    let mut rig = playing_a(name);
    rig.send(Command::LoopRecorderRecord);
    // The pass starts on the beat after half a second and lasts a bar.
    rig.run(0.5 + BAR_120 + 0.05);
    rig.send(Command::DeckPause { deck: A });
    rig
}

#[test]
fn a_pass_waits_for_the_beat_and_lasts_the_size_at_the_tempo_of_the_clock() {
    let mut rig = playing_a("loop-pass");
    rig.send(Command::LoopRecorderRecord);
    rig.run(0.1);
    assert_eq!(phase(&rig), LoopPhase::Waiting);
    rig.run(0.45);
    assert_eq!(phase(&rig), LoopPhase::Recording);
    rig.run(BAR_120);
    let snapshot = rig.engine.snapshot().loop_recorder;
    assert_eq!(snapshot.phase, LoopPhase::Playing);
    assert!((snapshot.seconds - BAR_120).abs() < 1e-9, "{snapshot:?}");
    assert!(!snapshot.can_undo, "the first pass is not a layer");
    // The size is kept while a loop is recorded.
    rig.send(Command::LoopRecorderSizeSet { bars: 8.0 });
    assert_eq!(rig.engine.snapshot().loop_recorder.bars, 1);
}

#[test]
fn the_loop_plays_over_the_mix_without_clicks_and_follows_the_dry_wet() {
    let mut rig = recorded_a("loop-play");
    rig.run(0.05);
    // Deck A is paused: what sounds is the loop, at full level with the knob in the middle.
    let looped = rig.run(BAR_120 * 2.0);
    let level = rms(&looped);
    assert!((level - 0.5 / 2f64.sqrt()).abs() < 0.02, "rms {level}");
    // A sine of 440 Hz at 0.5 moves at most 0.03 between samples; the seam adds no step.
    assert!(max_step(&looped) < 0.05, "step {}", max_step(&looped));
    rig.send(Command::LoopRecorderDryWetSet { amount: 0.0 });
    rig.run(0.05);
    assert!(rms(&rig.run(0.5)) < 1e-6, "no wet with the knob at 0");
    // Paused, the loop is silent.
    rig.send(Command::LoopRecorderDryWetSet { amount: 0.5 });
    rig.send(Command::LoopRecorderPlay);
    rig.run(0.05);
    assert_eq!(phase(&rig), LoopPhase::Paused);
    assert!(rms(&rig.run(0.5)) < 1e-6);
}

#[test]
fn an_overdub_is_undone_and_redone() {
    let mut rig = recorded_a("loop-undo");
    rig.load(B, sine_file("loop-b", 97.0, 12.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: B });
    rig.run(0.05);
    let alone = rms(&rig.run(BAR_120));
    // Deck B is off the master, behind the crossfader, but before its fader it is recorded.
    rig.send(Command::LoopRecorderSourceSet {
        source: LoopSource::Deck(B),
    });
    rig.send(Command::LoopRecorderRecord);
    rig.run(0.01);
    assert_eq!(phase(&rig), LoopPhase::Overdubbing);
    rig.run(BAR_120);
    rig.send(Command::LoopRecorderRecord);
    rig.send(Command::DeckPause { deck: B });
    rig.run(0.05);
    let both = rms(&rig.run(BAR_120));
    let expected = (alone * alone * 2.0).sqrt();
    assert!((both - expected).abs() < 0.03, "{alone} {both}");
    assert!(rig.engine.snapshot().loop_recorder.can_undo);

    rig.send(Command::LoopRecorderUndo);
    rig.run(0.05);
    assert!((rms(&rig.run(BAR_120)) - alone).abs() < 0.01);
    let snapshot = rig.engine.snapshot().loop_recorder;
    assert!(!snapshot.can_undo && snapshot.can_redo, "{snapshot:?}");
    rig.send(Command::LoopRecorderRedo);
    rig.run(0.05);
    assert!((rms(&rig.run(BAR_120)) - both).abs() < 0.01);
}

#[test]
fn a_decay_of_one_replaces_the_loop_with_what_the_overdub_records() {
    let mut rig = recorded_a("loop-decay");
    rig.engine.set_loop_decay(1.0).unwrap();
    // The source is the master, silent with deck A paused.
    rig.send(Command::LoopRecorderRecord);
    rig.run(BAR_120 + 0.05);
    rig.send(Command::LoopRecorderRecord);
    rig.run(0.05);
    assert!(rms(&rig.run(BAR_120)) < 1e-4);
    assert!(rig.engine.set_loop_decay(1.5).is_err());
}

#[test]
fn delete_empties_the_recorder_and_the_size_can_be_chosen_again() {
    let mut rig = recorded_a("loop-delete");
    rig.send(Command::LoopRecorderDelete);
    rig.run(0.05);
    let snapshot = rig.engine.snapshot().loop_recorder;
    assert_eq!(snapshot.phase, LoopPhase::Empty);
    assert_eq!(snapshot.seconds, 0.0);
    assert!(rms(&rig.run(BAR_120)) < 1e-6);
    rig.send(Command::LoopRecorderSizeSet { bars: 2.0 });
    assert_eq!(rig.engine.snapshot().loop_recorder.bars, 2);
}

#[test]
fn a_size_that_does_not_fit_is_halved() {
    let mut rig = Rig::new();
    rig.send(Command::ClockTempoSet { bpm: 50.0 });
    rig.send(Command::LoopRecorderSizeSet { bars: 8.0 });
    rig.run(0.05);
    assert_eq!(rig.engine.snapshot().loop_recorder.fitting_bars, 4);
    rig.send(Command::LoopRecorderRecord);
    // A beat at 50 beats per minute is 1.2 seconds.
    rig.run(1.25);
    let snapshot = rig.engine.snapshot().loop_recorder;
    assert_eq!(snapshot.phase, LoopPhase::Recording);
    assert!((snapshot.seconds - 19.2).abs() < 1e-9, "{snapshot:?}");
}

#[test]
fn commands_the_recorder_cannot_take_are_refused() {
    let mut rig = Rig::new();
    assert!(
        rig.engine
            .dispatch(Command::LoopRecorderSizeSet { bars: 3.0 })
            .is_err()
    );
    assert!(
        rig.engine
            .dispatch(Command::LoopRecorderDryWetSet { amount: f64::NAN })
            .is_err()
    );
    // With nothing recorded, play, undo, redo and delete do nothing.
    for command in [
        Command::LoopRecorderPlay,
        Command::LoopRecorderUndo,
        Command::LoopRecorderRedo,
        Command::LoopRecorderDelete,
    ] {
        rig.send(command);
    }
    rig.run(0.05);
    assert_eq!(phase(&rig), LoopPhase::Empty);
    rig.send(Command::MixerModeSet {
        mode: MixerMode::External,
    });
    assert!(matches!(
        rig.engine.dispatch(Command::LoopRecorderRecord),
        Err(EngineError::ExternalMixer)
    ));
}
