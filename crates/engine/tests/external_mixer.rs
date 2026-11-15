//! External mixer mode: each deck on its own pair of outputs.
//!
//! Behaviour note: `docs/behavior/external-mixer.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{DeckId, SampleRate};
use platter_engine::command::{Args, Value};
use platter_engine::{Band, Command, Engine, MixerMode, Output};
use platter_testkit::measure::{max_step, sine_fit};
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

/// Deck A at 1 kHz and 0.5, deck B at 300 Hz and 0.25, both playing, the internal mix set
/// to silence both: faders down, crossfader in the middle, filter of A closed.
fn two_decks(name: &str) -> Rig {
    let mut rig = Rig::new();
    rig.load(A, sine_file(&format!("{name}-a"), 1_000.0, 10.0, 0.5, RATE));
    rig.load(B, sine_file(&format!("{name}-b"), 300.0, 10.0, 0.25, RATE));
    for deck in [A, B] {
        rig.send(Command::MixerFaderSet {
            deck,
            position: 0.0,
        });
        rig.send(Command::DeckPlay { deck });
    }
    rig.send(Command::MixerFilterSet {
        deck: A,
        position: -1.0,
    });
    rig
}

fn external(rig: &mut Rig) {
    rig.send(Command::MixerModeSet {
        mode: MixerMode::External,
    });
}

/// Amplitude of the sine at `hz` in one output of a device with `channels` outputs, once
/// the ramps are over.
fn amplitude(rig: &mut Rig, channels: usize, output: usize, hz: f64) -> f64 {
    rig.run_channels(0.1, channels);
    let block = rig.run_channels(0.5, channels);
    sine_fit(&channel(&block, channels, output), hz, f64::from(RATE)).amplitude
}

fn assert_near(value: f64, expected: f64, within: f64) {
    assert!(
        (value - expected).abs() <= within,
        "{value} against {expected}"
    );
}

#[test]
fn each_deck_reaches_its_own_pair_whatever_the_internal_mix() {
    let mut rig = two_decks("external-pairs");
    assert!(
        amplitude(&mut rig, 4, 0, 1_000.0) < 1e-4,
        "the mix is silent"
    );
    external(&mut rig);
    assert_eq!(rig.engine.snapshot().mixer.mode, MixerMode::External);
    for side in [0, 1] {
        assert_near(amplitude(&mut rig, 4, side, 1_000.0), 0.5, 0.002);
        assert!(amplitude(&mut rig, 4, side, 300.0) < 1e-4);
        assert_near(amplitude(&mut rig, 4, side + 2, 300.0), 0.25, 0.002);
        assert!(amplitude(&mut rig, 4, side + 2, 1_000.0) < 1e-4);
    }
    // The internal controls keep taking commands without acting.
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    rig.send(Command::MixerPanSet {
        deck: A,
        position: 1.0,
    });
    assert_near(amplitude(&mut rig, 4, 0, 1_000.0), 0.5, 0.002);
}

#[test]
fn a_device_with_one_pair_gets_both_decks_summed() {
    let mut rig = two_decks("external-one-pair");
    external(&mut rig);
    assert_near(amplitude(&mut rig, 2, 0, 1_000.0), 0.5, 0.002);
    assert_near(amplitude(&mut rig, 2, 1, 300.0), 0.25, 0.002);
    // Below full scale no limiter reduces anything.
    assert_eq!(rig.engine.snapshot().limiter_gain, 1.0);
}

#[test]
fn gain_and_equaliser_act_only_with_their_option() {
    let mut rig = two_decks("external-eq-gain");
    external(&mut rig);
    rig.send(Command::MixerGainSet { deck: A, db: -6.0 });
    for band in [Band::Low, Band::Mid] {
        rig.send(Command::MixerEqKillToggle { deck: B, band });
    }
    assert_near(amplitude(&mut rig, 4, 0, 1_000.0), 0.5, 0.002);
    assert_near(amplitude(&mut rig, 4, 2, 300.0), 0.25, 0.002);
    rig.send(Command::MixerExternalEqGainToggle);
    assert!(rig.engine.snapshot().mixer.external_eq_gain);
    assert_near(amplitude(&mut rig, 4, 0, 1_000.0), 0.5 * 0.501_19, 0.002);
    assert!(
        amplitude(&mut rig, 4, 2, 300.0) < 0.25 * 0.1,
        "the low and the mid band are cut"
    );
}

#[test]
fn the_master_volume_acts_only_with_its_option() {
    let mut rig = two_decks("external-master-volume");
    external(&mut rig);
    rig.send(Command::MasterGainSet { db: -6.0 });
    assert_near(amplitude(&mut rig, 4, 0, 1_000.0), 0.5, 0.002);
    rig.send(Command::MixerExternalMasterVolumeToggle);
    assert!(rig.engine.snapshot().mixer.external_master_volume);
    assert_near(amplitude(&mut rig, 4, 0, 1_000.0), 0.5 * 0.501_19, 0.002);
    assert_near(amplitude(&mut rig, 4, 2, 300.0), 0.25 * 0.501_19, 0.002);
}

#[test]
fn there_are_no_headphones_in_external_mode() {
    let mut rig = two_decks("external-headphones");
    rig.send(Command::MixerCueToggle { deck: A });
    external(&mut rig);
    assert!(amplitude(&mut rig, 4, 2, 1_000.0) < 1e-4);
    // A headphone tone plays nothing; a master tone plays on the first pair.
    let mut rig = Rig::new();
    external(&mut rig);
    rig.run_channels(0.1, 4);
    rig.send(Command::MasterTestTone {
        output: Output::Headphones,
    });
    let block = rig.run_channels(0.5, 4);
    assert!(block.iter().all(|sample| *sample == 0.0));
    rig.send(Command::MasterTestTone {
        output: Output::Master,
    });
    let block = rig.run_channels(0.5, 4);
    let fit = sine_fit(&channel(&block, 4, 0)[4_800..], 440.0, f64::from(RATE));
    assert_near(fit.amplitude, 0.125, 0.001);
    assert!(channel(&block, 4, 2).iter().all(|sample| *sample == 0.0));
}

#[test]
fn the_switch_between_the_modes_does_not_click_and_comes_back_to_the_mix() {
    let mut rig = Rig::new();
    rig.load(A, sine_file("external-switch", 100.0, 10.0, 0.5, RATE));
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    rig.send(Command::DeckPlay { deck: A });
    rig.run_channels(0.1, 4);
    external(&mut rig);
    let switching = channel(&rig.run_channels(0.1, 4), 4, 0);
    // A step would jump by up to 0.5; the sine alone moves by less than 0.007 a frame.
    assert!(max_step(&switching) < 0.01, "{}", max_step(&switching));
    assert_near(amplitude(&mut rig, 4, 0, 100.0), 0.5, 0.002);
    rig.send(Command::MixerModeSet {
        mode: MixerMode::Internal,
    });
    let switching = channel(&rig.run_channels(0.1, 4), 4, 0);
    assert!(max_step(&switching) < 0.01, "{}", max_step(&switching));
    assert!(
        amplitude(&mut rig, 4, 0, 100.0) < 1e-4,
        "behind the crossfader again"
    );
}

#[test]
fn the_meter_of_a_channel_reads_what_its_pair_gets() {
    // The fader of deck A is down: the meter would read nothing in internal mode.
    let mut rig = two_decks("external-meter");
    external(&mut rig);
    rig.run_channels(0.3, 4);
    let snapshot = rig.engine.snapshot();
    assert_near(f64::from(snapshot.decks[A.index()].levels[0]), 0.5, 0.01);
    assert_near(f64::from(snapshot.master_levels[0]), 0.5, 0.01);
}

#[test]
fn a_new_device_rate_keeps_the_mode_and_its_options() {
    let mut rig = Rig::new();
    external(&mut rig);
    rig.send(Command::MixerExternalEqGainToggle);
    let rate = SampleRate::new(44_100).unwrap();
    let (engine, mut processor) = Engine::with_controls_of(rate, &rig.engine);
    let mixer = engine.snapshot().mixer;
    assert_eq!(mixer.mode, MixerMode::External);
    assert!(mixer.external_eq_gain);
    assert!(!mixer.external_master_volume);
    // The new processor routes too: a master tone reaches the first pair only, with no
    // ramp from the internal mix to wait for.
    let mut engine = engine;
    engine
        .dispatch(Command::MasterTestTone {
            output: Output::Master,
        })
        .unwrap();
    let mut output = vec![0.0f32; 44_100 * 4 / 2];
    for block in output.chunks_mut(256 * 4) {
        processor.process(block, 4);
    }
    let fit = sine_fit(&channel(&output, 4, 0)[4_410..], 440.0, 44_100.0);
    assert_near(fit.amplitude, 0.125, 0.001);
}

#[test]
fn an_unknown_mode_is_rejected() {
    let mut args = Args::new();
    args.insert("mode".to_owned(), Value::Text("hardware".to_owned()));
    assert!(Command::from_args("mixer.mode.set", &args).is_err());
    args.insert("mode".to_owned(), Value::Text("external".to_owned()));
    assert_eq!(
        Command::from_args("mixer.mode.set", &args),
        Ok(Command::MixerModeSet {
            mode: MixerMode::External
        })
    );
}
