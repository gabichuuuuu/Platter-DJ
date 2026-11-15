//! The steps and the automatic fades of the crossfader.
//!
//! Behaviour note: `docs/behavior/mixer.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::command::{Args, Value};
use platter_engine::{Command, CrossfaderDirection, Engine};
use platter_testkit::measure::sine_fit;

use CrossfaderDirection::{Left, Right};

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

fn position(rig: &Rig) -> f64 {
    rig.engine.snapshot().mixer.crossfader
}

fn assert_near(value: f64, expected: f64, within: f64) {
    assert!(
        (value - expected).abs() <= within,
        "{value} against {expected}"
    );
}

/// One block of 256 frames, the step the rig processes in.
const BLOCK: f64 = 256.0 / RATE as f64;

#[test]
fn a_step_moves_a_tenth_of_the_travel_and_stops_at_the_end() {
    let mut rig = Rig::new();
    for _ in 0..3 {
        rig.send(Command::MixerCrossfaderStep { direction: Right });
    }
    assert_near(position(&rig), 0.8, 1e-9);
    for _ in 0..3 {
        rig.send(Command::MixerCrossfaderStep { direction: Right });
    }
    assert_eq!(position(&rig), 1.0);
    rig.send(Command::MixerCrossfaderStep { direction: Left });
    assert_near(position(&rig), 0.9, 1e-9);
    // The processor is where the snapshot says.
    rig.run(0.05);
    assert_near(
        f64::from(rig.engine.snapshot().mixer.crossfader as f32),
        0.9,
        1e-6,
    );
}

#[test]
fn an_automatic_fade_crosses_in_the_beats_set_at_the_tempo_of_the_clock() {
    let mut rig = Rig::new();
    rig.load(A, sine_file("crossfade-a", 440.0, 12.0, 0.5, RATE));
    rig.load(B, sine_file("crossfade-b", 1_000.0, 12.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    assert_eq!(rig.engine.snapshot().mixer.fade_beats, 8.0);

    // 8 beats at the 120 BPM of the clock: the whole travel in 4 s.
    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(1.0);
    assert_near(position(&rig), 0.25, 0.01);
    rig.run(1.0);
    assert_near(position(&rig), 0.5, 0.01);
    rig.run(2.0 + 2.0 * BLOCK);
    assert_eq!(position(&rig), 1.0, "it stops at the end");
    let output = rig.run(0.5);
    let a = sine_fit(&output, 440.0, f64::from(RATE)).amplitude;
    let b = sine_fit(&output, 1_000.0, f64::from(RATE)).amplitude;
    assert!(a < 1e-4 && (b - 0.5).abs() < 0.003, "{a} {b}");

    // At 60 BPM and 4 beats, back to the left in 4 s, from the centre in 2 s.
    rig.send(Command::ClockTempoSet { bpm: 60.0 });
    rig.send(Command::MixerCrossfaderFadeBeatsSet { beats: 4.0 });
    rig.send(Command::MixerCrossfaderSet { position: 0.5 });
    rig.send(Command::MixerCrossfaderAutoFade { direction: Left });
    rig.run(1.0);
    assert_near(position(&rig), 0.25, 0.01);
    rig.run(1.0 + 2.0 * BLOCK);
    assert_eq!(position(&rig), 0.0);
}

#[test]
fn the_fade_follows_a_change_of_tempo() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(1.0);
    assert_near(position(&rig), 0.25, 0.01);
    // Half the tempo, half the speed.
    rig.send(Command::ClockTempoSet { bpm: 60.0 });
    rig.run(1.0);
    assert_near(position(&rig), 0.375, 0.01);
}

#[test]
fn the_dj_stops_a_fade_by_moving_the_crossfader() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(1.0);
    let reached = position(&rig);
    // A step goes from where the fade has got to, and the fade stops.
    rig.send(Command::MixerCrossfaderStep { direction: Left });
    assert_near(position(&rig), reached - 0.1, 1e-6);
    rig.run(1.0);
    assert_near(position(&rig), reached - 0.1, 1e-6);

    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(0.5);
    rig.send(Command::MixerCrossfaderSet { position: 0.7 });
    rig.run(1.0);
    assert_eq!(position(&rig), 0.7);

    // Another fade turns it round.
    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(0.5);
    rig.send(Command::MixerCrossfaderAutoFade { direction: Left });
    let turned = position(&rig);
    rig.run(1.0);
    assert_near(position(&rig), turned - 0.25, 0.01);
}

#[test]
fn a_fade_towards_the_end_it_is_at_does_nothing() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(1.0);
    assert_eq!(position(&rig), 1.0);
}

#[test]
fn a_new_device_rate_stops_the_fade_where_it_is_and_keeps_its_length() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderFadeBeatsSet { beats: 16.0 });
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::MixerCrossfaderAutoFade { direction: Right });
    rig.run(2.0);
    let reached = position(&rig);
    // 16 beats at 120 BPM: the whole travel in 8 s.
    assert_near(reached, 0.25, 0.01);
    let rate = platter_core::SampleRate::new(44_100).unwrap();
    let (engine, mut processor) = Engine::with_controls_of(rate, &rig.engine);
    let mut output = vec![0.0f32; 44_100 * 2];
    for block in output.chunks_mut(512) {
        processor.process(block, 2);
    }
    let snapshot = engine.snapshot();
    assert_near(snapshot.mixer.crossfader, reached, 1e-6);
    assert_eq!(snapshot.mixer.fade_beats, 16.0);
}

#[test]
fn bad_parameters_are_rejected() {
    let mut rig = Rig::new();
    for beats in [0.5, 65.0, f64::NAN] {
        assert!(
            rig.engine
                .dispatch(Command::MixerCrossfaderFadeBeatsSet { beats })
                .is_err()
        );
    }
    assert_eq!(rig.engine.snapshot().mixer.fade_beats, 8.0);
    let mut args = Args::new();
    args.insert("direction".to_owned(), Value::Text("up".to_owned()));
    assert!(Command::from_args("mixer.crossfader.step", &args).is_err());
    args.insert("direction".to_owned(), Value::Text("left".to_owned()));
    assert_eq!(
        Command::from_args("mixer.crossfader.auto_fade", &args),
        Ok(Command::MixerCrossfaderAutoFade { direction: Left })
    );
}
